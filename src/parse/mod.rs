pub mod streamscharts;
pub mod sullygnome;
mod tests;
pub mod twitchtracker;

use anyhow::{anyhow, Context};
use bytes::{Buf, Bytes};
use chrono::{NaiveDateTime, Timelike};
use futures::Future;
use m3u8_rs::{MediaPlaylist, MediaSegment};
use reqwest::Client;
use sha1::{Digest, Sha1};
use std::{
    fmt::Display,
    io::{stdout, Write},
    sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc;
use url::Url;

use crate::first_ok;

pub const DOMAINS: [&str; 12] = [
    "https://vod-secure.twitch.tv/",
    "https://vod-metro.twitch.tv/",
    "https://vod-pop-secure.twitch.tv/",
    "https://d1m7jfoe9zdc1j.cloudfront.net/",
    "https://d1mhjrowxxagfy.cloudfront.net/",
    "https://d1ymi26ma8va5x.cloudfront.net/",
    "https://d2nvs31859zcd8.cloudfront.net/",
    "https://d2vjef5jvl6bfs.cloudfront.net/",
    "https://d3vd9lfkzbru3h.cloudfront.net/",
    "https://dgeft87wbj63p.cloudfront.net/",
    "https://dqrpb9wgowsf5.cloudfront.net/",
    "https://ds0h3roq6wcgc.cloudfront.net/",
];

#[derive(PartialEq, Debug)]
pub struct VideoData {
    pub streamer_name: Arc<String>,
    pub video_id: Arc<String>,
    pub unix_time_seconds: NaiveDateTime,
}

pub struct VideoPath {
    pub url_path: String,
    pub video_data: Arc<VideoData>,
}

pub struct DomainWithPath<T: Clone + 'static + Send + Display> {
    pub domain: T, // e.g. https://d1m7jfoe9zdc1j.cloudfront.net/
    pub path: Arc<VideoPath>,
}

pub struct DomainWithPaths<T: Clone + 'static + Send + Display> {
    pub domain: T, // e.g. https://d1m7jfoe9zdc1j.cloudfront.net/
    pub paths: Arc<Vec<Arc<VideoPath>>>,
}

pub struct ValidDwpResponse<T: Clone + 'static + Send + Display> {
    pub dwp: DomainWithPath<T>,
    pub body: Bytes,
}

async fn retry_on_error<F, T, E, Fut>(doer: F) -> Result<T, E>
where
    F: (FnOnce() -> Fut) + Clone,
    Fut: Future<Output = Result<T, E>>,
{
    let doer_clone = F::clone(&doer);
    let result = doer().await;
    match result {
        Ok(good) => Ok(good),
        Err(_) => doer_clone().await,
    }
}

// e.g. c5992ececce7bd7d350d_gmhikaru_47198535725_1664038929
pub fn url_path_to_video_data(url_path: &str) -> anyhow::Result<VideoData> {
    let all_underscore_indices = url_path
        .char_indices()
        .filter(|(_i, char)| char == &'_')
        .map(|(i, _char)| i)
        .collect::<Vec<_>>();
    let num_underscores = all_underscore_indices.len();
    if num_underscores < 3 {
        return Err(anyhow!("url path does not have enough underscores"));
    }
    let underscore_indices = [
        all_underscore_indices[0],
        all_underscore_indices[num_underscores - 2],
        all_underscore_indices[num_underscores - 1],
    ];
    let streamer_name = &url_path[underscore_indices[0] + 1..underscore_indices[1]];
    let video_id = &url_path[underscore_indices[1] + 1..underscore_indices[2]];
    let unix_time_string = &url_path[underscore_indices[2] + 1..];
    let unix_time_int = unix_time_string.parse::<i64>()?;
    let time =
        NaiveDateTime::from_timestamp_opt(unix_time_int, 0).context("unix time out of range")?;
    Ok(VideoData {
        streamer_name: Arc::new(streamer_name.to_string()),
        video_id: Arc::new(video_id.to_string()),
        unix_time_seconds: time,
    })
}

// e.g. https://d1m7jfoe9zdc1j.cloudfront.net/c5992ececce7bd7d350d_gmhikaru_47198535725_1664038929
// e.g. https://d1m7jfoe9zdc1j.cloudfront.net/c5992ececce7bd7d350d_gmhikaru_47198535725_1664038929/storyboards/1600104857-info.json
pub fn url_to_domain_with_path(url_str: &str) -> anyhow::Result<DomainWithPath<Arc<String>>> {
    let parsed = Url::parse(url_str)?;
    let host = parsed.host().context("url host absent")?;
    let main_part = parsed
        .path()
        .split('/')
        .nth(1)
        .context("url is not valid")?
        .to_string();
    let video_data = Arc::new(url_path_to_video_data(&main_part)?);
    let result = DomainWithPath {
        domain: Arc::new(format!("{}://{}/", parsed.scheme(), host)),
        path: Arc::new(VideoPath {
            url_path: main_part,
            video_data,
        }),
    };
    Ok(result)
}

impl Display for VideoData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}_{}_{}",
            self.streamer_name,
            self.unix_time_seconds.format("%Y-%m-%d_%H:%M:%S"),
            self.video_id
        )
    }
}

impl VideoData {
    pub fn get_video_path(self, to_unix: bool) -> VideoPath {
        VideoPath {
            url_path: self.get_url_path(to_unix),
            video_data: Arc::new(self),
        }
    }

    pub fn get_url_path(&self, to_unix: bool) -> String {
        if to_unix {
            self.get_url_path_helper(|t| t.timestamp().to_string())
        } else {
            self.get_url_path_helper(|t| t.second().to_string())
        }
    }

    fn get_url_path_helper<F>(&self, time_to_string: F) -> String
    where
        F: FnOnce(&NaiveDateTime) -> String,
    {
        let base_url = format!(
            "{}_{}_{}",
            self.streamer_name,
            self.video_id,
            time_to_string(&self.unix_time_seconds)
        );
        let mut hasher = Sha1::new();
        <Sha1 as Digest>::update(&mut hasher, &base_url);
        let hash_result = <Sha1 as Digest>::finalize(hasher);
        let hash = hex::encode(hash_result);
        let hashed_base_url = &hash[..20];
        format!("{}_{}", hashed_base_url, base_url)
    }

    pub fn with_offset(&self, seconds: i64) -> Self {
        Self {
            streamer_name: Arc::clone(&self.streamer_name),
            video_id: Arc::clone(&self.video_id),
            unix_time_seconds: self.unix_time_seconds + chrono::Duration::seconds(seconds),
        }
    }

    pub fn get_domain_with_paths_list(
        &self,
        domains: &[&'static str],
        seconds: i64,
        to_unix: bool,
    ) -> Vec<DomainWithPaths<&'static str>> {
        let video_paths = (0..seconds)
            .map(|i| Arc::new(self.with_offset(i).get_video_path(to_unix)))
            .collect::<Vec<_>>();
        let video_paths = Arc::new(video_paths);
        domains
            .iter()
            .map(|domain| DomainWithPaths {
                domain: *domain,
                paths: Arc::clone(&video_paths),
            })
            .collect::<Vec<_>>()
    }
}

impl<T: Clone + 'static + Send + Display + Sync> DomainWithPaths<T> {
    pub fn to_list_of_domain_with_path(&self) -> Vec<DomainWithPath<T>> {
        self.paths
            .iter()
            .map(|path| DomainWithPath {
                domain: self.domain.clone(),
                path: Arc::clone(path),
            })
            .collect::<Vec<_>>()
    }

    /// If the list of items is empty, it returns `None`.
    /// If all of the results are errors, it returns the last error.
    pub async fn get_first_valid_dwp(&self, client: Client) -> anyhow::Result<ValidDwpResponse<T>> {
        let mut domain_with_path_list = self.to_list_of_domain_with_path();
        let last = domain_with_path_list.pop().context("no urls")?;
        // establish TCP connection for reuse
        // https://groups.google.com/g/golang-nuts/c/5T5aiDRl_cw/m/zYPGtCOYBwAJ
        let body = last.get_m3u8_body(Client::clone(&client)).await;
        match body {
            Ok(body) => Ok(ValidDwpResponse { dwp: last, body }),
            Err(err) => {
                // reuse with other requests
                let items = domain_with_path_list
                    .into_iter()
                    .map(move |item| (item, Client::clone(&client)));
                let response =
                    first_ok::get_first_ok_bounded(items, 0, |(item, client)| async move {
                        let body = item.get_m3u8_body(client).await?;
                        Ok(ValidDwpResponse { body, dwp: item })
                    })
                    .await;
                match response {
                    Some(result) => result,
                    None => Err(err),
                }
            }
        }
    }
}

/// If the list of items is empty, it returns `None`.
/// If all of the results are errors, it returns the last error.
pub async fn get_first_valid_dwp<T: Clone + 'static + Send + Display + Sync>(
    domain_with_paths_list: Vec<DomainWithPaths<T>>,
    client: Client,
) -> Option<anyhow::Result<ValidDwpResponse<T>>> {
    first_ok::get_first_ok_bounded(
        domain_with_paths_list
            .into_iter()
            .map(move |item| (item, Client::clone(&client))),
        0,
        |(item, client)| async move { item.get_first_valid_dwp(client).await },
    )
    .await
}

impl<T: Clone + 'static + Send + Display> DomainWithPath<T> {
    pub fn get_domain(&self) -> T {
        self.domain.clone()
    }

    pub fn get_video_data(self) -> Arc<VideoData> {
        Arc::clone(&self.path.video_data)
    }

    pub fn get_index_dvr_url(&self) -> String {
        format!(
            "{}{}/chunked/index-dvr.m3u8",
            self.domain, self.path.url_path
        )
    }

    pub fn get_segment_chunked_url(&self, segment: &MediaSegment) -> String {
        format!(
            "{}{}/chunked/{}",
            self.domain, self.path.url_path, segment.uri
        )
    }
    pub fn make_paths_explicit(&self, playlist: &mut MediaPlaylist) {
        for segment in &mut playlist.segments {
            segment.uri = self.get_segment_chunked_url(segment);
        }
    }

    pub async fn get_m3u8_body(&self, client: Client) -> anyhow::Result<Bytes> {
        let url = Arc::new(self.get_index_dvr_url());
        let response = retry_on_error(|| async { client.get(url.as_ref()).send().await }).await?;
        let status_code = response.status().as_u16();
        if status_code != 200 {
            return Err(anyhow!(format!("status code is {}", status_code)));
        }
        let bytes = response.bytes().await?;
        Ok(bytes)
    }
}

pub fn decode_media_playlist_filter_nil_segments(data: Bytes) -> anyhow::Result<MediaPlaylist> {
    let result = m3u8_rs::parse_media_playlist_res(data.chunk())
        .map_err(|_| anyhow!("m3u8 is not media type"))?;
    Ok(result)
}

pub fn mute_uri(segment_uri: &mut String) {
    if let Some(start) = segment_uri.find("-unmuted") {
        *segment_uri = String::from(&segment_uri[..start]) + "-muted.ts";
    }
}

pub fn mute_media_segments(playlist: &mut MediaPlaylist) {
    for segment in &mut playlist.segments {
        mute_uri(&mut segment.uri);
    }
}

pub fn get_media_playlist_duration(playlist: &MediaPlaylist) -> Duration {
    let mut duration: f64 = 0.0;
    for segment in &playlist.segments {
        duration += segment.duration as f64;
    }
    Duration::from_secs_f64(duration)
}

pub async fn get_media_playlist_with_valid_segments(
    mut raw_playlist: MediaPlaylist,
    concurrent: usize,
    client: Client,
) -> MediaPlaylist {
    raw_playlist.segments = get_valid_segments(raw_playlist.segments, concurrent, client).await;
    raw_playlist
}

async fn get_valid_segments(
    segments: Vec<MediaSegment>,
    concurrent: usize,
    client: Client,
) -> Vec<MediaSegment> {
    let urls = segments
        .iter()
        .map(|segment| String::clone(&segment.uri))
        .collect::<Vec<_>>();
    let index_is_valid = get_valid_indices(urls, concurrent, client).await;
    segments
        .into_iter()
        .enumerate()
        .filter_map(|(i, elem)| if index_is_valid[i] { Some(elem) } else { None })
        .collect()
}

static CLEAR_LINE: &str = "\x1b[2K";

async fn get_valid_indices(urls: Vec<String>, concurrent: usize, client: Client) -> Vec<bool> {
    let urls = Arc::new(urls);
    let (valid_indices_sender, mut valid_indices_receiver) = mpsc::channel::<Option<usize>>(1);
    let (request_indices_sender, request_indices_receiver) = async_channel::bounded::<usize>(1);
    for _ in 0..concurrent {
        let request_indices_receiver = async_channel::Receiver::clone(&request_indices_receiver);
        let urls = Arc::clone(&urls);
        let client = Client::clone(&client);
        let valid_indices_sender = mpsc::Sender::clone(&valid_indices_sender);
        tokio::task::spawn(async move {
            while let Ok(request_index) = request_indices_receiver.recv().await {
                let url = &urls[request_index];
                let client = Client::clone(&client);
                let result = if url_is_valid(url, client).await {
                    Some(request_index)
                } else {
                    None
                };
                if valid_indices_sender.send(result).await.is_err() {
                    return;
                };
            }
        });
    }
    tokio::task::spawn({
        let urls = Arc::clone(&urls);
        async move {
            for i in 0..urls.len() {
                if request_indices_sender.send(i).await.is_err() {
                    return;
                }
            }
        }
    });
    let mut done_count = 0;
    let mut result = vec![false; urls.len()];
    for _ in &*urls {
        if let Some(response) = valid_indices_receiver.recv().await {
            done_count += 1;
            print!("{}", CLEAR_LINE);
            print!("\r");
            print!("Processed {} segments out of {}", done_count, urls.len());
            let _ = stdout().flush();
            if let Some(index) = response {
                result[index] = true;
            }
        }
    }
    println!();
    result
}

async fn url_is_valid(url: &String, client: Client) -> bool {
    let response = retry_on_error(|| async { client.get(url).send().await }).await;
    match response {
        Ok(response) => response.status().as_u16() == 200,
        Err(_) => false,
    }
}

use std::{fmt::Display, fs, io::BufWriter, path::PathBuf, time::Duration};

use anyhow::anyhow;
use clap::{Args, Parser, Subcommand};
use m3u8_rs::MediaPlaylist;
use reqwest::Client;
use vods::{
    self, DomainWithPath, StreamsChartsData, SullyGnomeData, TwitchTrackerData, ValidDwpResponse,
    VideoData,
};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Subcommand)]
enum Commands {
    /// Using twitchtracker.com data, get an .m3u8 file which can be viewed in a media player.
    #[command(name = "tt-manual-get-m3u8")]
    TwitchTracker(TwitchTrackerArgs),
    /// Using streamscharts.com data, get an .m3u8 file which can be viewed in a media player.
    #[command(name = "sc-manual-get-m3u8")]
    StreamsCharts(StreamsChartsArgs),
    /// Using sullygnome.com data, get an .m3u8 file which can be viewed in a media player.
    #[command(name = "sg-manual-get-m3u8")]
    SullyGnome(SullyGnomeArgs),
}

#[derive(Args, Clone)]
struct TwitchTrackerArgs {
    /// twitch streamer name
    #[arg(long = "streamer")]
    streamer_name: String,
    /// twitch video id
    #[arg(long = "videoid")]
    video_id: String,
    /// stream UTC start time in the format '2006-01-02 15:04:05' (year-month-day hour:minute:second)
    #[arg(long)]
    time: String,
    /// Filter out all of the invalid segments in the m3u8 file with concurrency level
    #[arg(long)]
    filter_invalid: Option<usize>,
}

#[derive(Args, Clone)]
struct StreamsChartsArgs {
    /// twitch streamer name
    #[arg(long = "streamer")]
    streamer_name: String,
    /// twitch video id
    #[arg(long = "videoid")]
    video_id: String,
    /// stream UTC start time in the format '02-01-2006 15:04' (day-month-year hour:minute)
    #[arg(long)]
    time: String,
    /// Filter out all of the invalid segments in the m3u8 file with concurrency level
    #[arg(long)]
    filter_invalid: Option<usize>,
}

#[derive(Args, Clone)]
struct SullyGnomeArgs {
    /// twitch streamer name
    #[arg(long = "streamer")]
    streamer_name: String,
    /// twitch video id
    #[arg(long = "videoid")]
    video_id: String,
    /// stream UTC start time in the format '2006-01-02T15:04:05Z' (year-month-dayThour:minute:secondZ)
    #[arg(long)]
    time: String,
    /// Filter out all of the invalid segments in the m3u8 file with concurrency level
    #[arg(long)]
    filter_invalid: Option<usize>,
}

fn duration_to_human_readable(dur: &Duration) -> String {
    let secs = dur.as_secs() % 60;
    let minutes = (dur.as_secs() / 60) % 60;
    let hours = (dur.as_secs() / 60) / 60;
    format!("{:0>2}h{:0>2}m{:0>2}s", hours, minutes, secs)
}

fn write_media_playlist<T: Clone + 'static + Send + Display>(
    mediapl: &MediaPlaylist,
    dwp: DomainWithPath<T>,
) -> anyhow::Result<()> {
    let video_data = dwp.get_video_data();
    let mut path = PathBuf::from_iter(
        vec![
            "Downloads".to_string(),
            video_data.streamer_name.to_string(),
        ]
        .iter(),
    );
    fs::create_dir_all(&path)?;
    let rounded_duration = vods::get_media_playlist_duration(mediapl);
    path.push(format!(
        "{}_{}.m3u8",
        video_data,
        duration_to_human_readable(&rounded_duration)
    ));
    let mut file_path = BufWriter::new(fs::File::create(&path)?);
    mediapl.write_to(&mut file_path)?;
    Ok(())
}

fn make_robust_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .timeout(Duration::from_secs(5))
        .trust_dns(true)
        .build()
}

async fn get_valid_dwp(
    domains: &[&'static str],
    seconds: i64,
    video_data: VideoData,
    client: Client,
) -> anyhow::Result<ValidDwpResponse<&'static str>> {
    let domain_with_paths_list = video_data.get_domain_with_paths_list(domains, seconds, true);
    let dwp_and_body = vods::get_first_valid_dwp(domain_with_paths_list, client.clone()).await;
    if let Some(Ok(dwp_and_body)) = dwp_and_body {
        return Ok(dwp_and_body);
    }
    let domain_with_paths_list = video_data.get_domain_with_paths_list(domains, seconds, false);
    let dwp_and_body = vods::get_first_valid_dwp(domain_with_paths_list, client).await;
    match dwp_and_body {
        Some(dwp_and_body) => dwp_and_body,
        None => Err(anyhow!("no domains supplied")),
    }
}

async fn main_helper(
    seconds: i64,
    video_data: VideoData,
    filter_invalid: Option<usize>,
) -> anyhow::Result<()> {
    let video_data = video_data.with_offset(-1); // some m3u8 file names use a time that is 1 second minus the provided time
    let client = make_robust_client()?;
    let dwp_and_body =
        get_valid_dwp(&vods::DOMAINS, seconds + 1, video_data, client.clone()).await?;
    println!("Found valid url {}", dwp_and_body.dwp.get_index_dvr_url());
    let mut mediapl = vods::decode_media_playlist_filter_nil_segments(dwp_and_body.body)?;
    vods::mute_media_segments(&mut mediapl);
    dwp_and_body.dwp.make_paths_explicit(&mut mediapl);
    match filter_invalid {
        Some(check_invalid_concurrent) if check_invalid_concurrent > 0 => {
            let num_total_segments = mediapl.segments.len();
            mediapl = vods::get_media_playlist_with_valid_segments(
                mediapl,
                check_invalid_concurrent,
                client,
            )
            .await;
            let num_valid_segments = mediapl.segments.len();
            println!(
                "{} valid segments out of {}",
                num_valid_segments, num_total_segments
            );
            if num_valid_segments == 0 {
                return Err(anyhow!("0 valid segments found"));
            }
        }
        _ => {}
    };
    write_media_playlist(&mediapl, dwp_and_body.dwp)?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        match cli.command {
            Commands::TwitchTracker(args) => {
                let twitch_data = TwitchTrackerData {
                    streamer_name: args.streamer_name,
                    utc_time: args.time,
                    video_id: args.video_id,
                };
                let video_data: VideoData = twitch_data.try_into()?;
                main_helper(1, video_data, args.filter_invalid).await?;
            }
            Commands::StreamsCharts(args) => {
                let sc_data = StreamsChartsData {
                    streamer_name: args.streamer_name,
                    utc_time: args.time,
                    video_id: args.video_id,
                };
                let video_data: VideoData = sc_data.try_into()?;
                main_helper(60, video_data, args.filter_invalid).await?;
            }
            Commands::SullyGnome(args) => {
                let twitch_data = SullyGnomeData {
                    streamer_name: args.streamer_name,
                    utc_time: args.time,
                    video_id: args.video_id,
                };
                let video_data: VideoData = twitch_data.try_into()?;
                main_helper(1, video_data, args.filter_invalid).await?;
            }
        }
        Ok(())
    })
}

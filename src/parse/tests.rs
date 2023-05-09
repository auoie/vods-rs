#[cfg(test)]
use super::*;

#[test]
fn test_url_path_to_video_data() {
    let url_path = "24b0b82db0edbea186df_malek_04_47198535725_1664038929";
    let result = url_path_to_video_data(url_path).unwrap();
    let want = VideoData {
        streamer_name: Arc::new("malek_04".to_string()),
        video_id: Arc::new("47198535725".to_string()),
        unix_time_seconds: NaiveDateTime::from_timestamp_opt(1664038929, 0).unwrap(),
    };
    assert_eq!(result, want);
}

#[test]
fn test_url_to_domain_with_path() {
    let url = "https://d1m7jfoe9zdc1j.cloudfront.net/c5992ececce7bd7d350d_gmhikaru_47198535725_1664038929/storyboards/1600104857-info.json";
    let result = url_to_domain_with_path(url).unwrap();
    assert_eq!(*result.domain, "https://d1m7jfoe9zdc1j.cloudfront.net/");
    assert_eq!(
        result.path.video_data,
        Arc::new(VideoData {
            streamer_name: Arc::new("gmhikaru".to_string()),
            video_id: Arc::new("47198535725".to_string()),
            unix_time_seconds: NaiveDateTime::from_timestamp_opt(1664038929, 0).unwrap()
        })
    );
    assert_eq!(
        result.path.url_path,
        "c5992ececce7bd7d350d_gmhikaru_47198535725_1664038929"
    );
}

#[test]
fn test_video_data_to_string() {
    let url_path = "c5992ececce7bd7d350d_gmhikaru_47198535725_1664038929";
    let video_data = url_path_to_video_data(url_path).unwrap();
    assert_eq!(
        "gmhikaru_2022-09-24_17:02:09_47198535725",
        video_data.to_string()
    );
}

#[test]
fn test_get_url_path() {
    {
        let url_path = "c5992ececce7bd7d350d_gmhikaru_47198535725_1664038929";
        let video_data = url_path_to_video_data(url_path).unwrap();
        assert_eq!(video_data.get_url_path(true), url_path);
    }
    {
        let url_path = "24b0b82db0edbea186df_malek_04_47198535725_1664038929";
        let video_data = url_path_to_video_data(url_path).unwrap();
        assert_eq!(video_data.get_url_path(true), url_path);
    }
}

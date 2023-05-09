use std::sync::Arc;

use chrono::NaiveDateTime;

use super::VideoData;

pub struct StreamsChartsData {
    pub streamer_name: String,
    pub video_id: String,
    pub utc_time: String,
}

impl TryInto<VideoData> for StreamsChartsData {
    type Error = chrono::ParseError;

    fn try_into(self) -> Result<VideoData, Self::Error> {
        Ok(VideoData {
            streamer_name: Arc::new(self.streamer_name),
            video_id: Arc::new(self.video_id),
            unix_time_seconds: NaiveDateTime::parse_from_str(&self.utc_time, "%d-%m-%Y %H:%M")?,
        })
    }
}

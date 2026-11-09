use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoMeta {
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub video_codec: String,
    pub audio_codec: Option<String>,
    pub bitrate: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub is_guest: bool,
    pub username: Option<String>,
    pub max_video_bytes: u64,
    pub guest_frame_limit_bytes: u64,
    /// 抽帧时的视频大小限制，None 表示不限制
    pub frame_limit_bytes: Option<u64>,
    /// 允许插入的切割点数量上限，None 表示不限制
    pub max_cut_points: Option<u32>,
    pub ffmpeg_ok: bool,
    pub ffmpeg_message: String,
    pub last_out_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameItem {
    pub index: u32,
    pub time_ms: u64,
    pub file: String,
    pub thumb: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FramesResult {
    pub job_id: String,
    pub video: VideoMeta,
    pub frames: Vec<FrameItem>,
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FramePick {
    pub file: String,
    pub index: u32,
    pub time_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub saved: Vec<String>,
    pub out_dir: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentInput {
    pub start_ms: u64,
    pub end_ms: u64,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub outputs: Vec<String>,
    pub out_dir: String,
    pub mode: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressPayload {
    pub job: String,
    pub percent: f64,
    pub message: String,
}

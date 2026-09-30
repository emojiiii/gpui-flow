use std::{fmt, path::PathBuf, sync::Arc, time::Duration};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaError {
    InvalidSource(String),
    ToolUnavailable(String),
    InvalidMedia(String),
    ResourceLimit(String),
    Process(String),
    Timeout(String),
    Cancelled,
    CommandQueueFull,
    PlayerClosed,
}
impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSource(s) => write!(f, "invalid local media source: {s}"),
            Self::ToolUnavailable(s) => write!(f, "media executable unavailable: {s}"),
            Self::InvalidMedia(s) => write!(f, "invalid or unsupported media: {s}"),
            Self::ResourceLimit(s) => write!(f, "media resource limit: {s}"),
            Self::Process(s) => write!(f, "media process failed: {s}"),
            Self::Timeout(s) => write!(f, "media operation timed out: {s}"),
            Self::Cancelled => write!(f, "media operation cancelled"),
            Self::CommandQueueFull => write!(f, "media command queue is full; retry after a frame"),
            Self::PlayerClosed => write!(f, "media player is closed"),
        }
    }
}
impl std::error::Error for MediaError {}

/// Executables are configured by the trusted host, never by serialized node data.
#[derive(Clone, Debug)]
pub struct FfmpegTools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub ffplay: PathBuf,
}
impl Default for FfmpegTools {
    fn default() -> Self {
        Self {
            ffmpeg: "ffmpeg".into(),
            ffprobe: "ffprobe".into(),
            ffplay: "ffplay".into(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct MediaLimits {
    pub max_input_bytes: u64,
    pub max_source_pixels: u64,
    pub max_duration: Duration,
    pub probe_timeout: Duration,
    pub decode_stall_timeout: Duration,
}
impl Default for MediaLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 1024 * 1024 * 1024,
            max_source_pixels: 4096 * 4096,
            max_duration: Duration::from_secs(4 * 60 * 60),
            probe_timeout: Duration::from_secs(10),
            decode_stall_timeout: Duration::from_secs(10),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioOutput {
    System,
    Disabled,
}
#[derive(Clone, Debug)]
pub struct PlaybackOptions {
    pub audio: AudioOutput,
    pub volume: f32,
    /// Frames are resampled to this fixed preview rate (1..=60).
    pub frames_per_second: u32,
    /// Maximum preview dimensions, aspect ratio preserved, no upscaling.
    pub max_width: u32,
    pub max_height: u32,
    pub autoplay: bool,
}
impl Default for PlaybackOptions {
    fn default() -> Self {
        Self {
            audio: AudioOutput::System,
            volume: 0.25,
            frames_per_second: 24,
            max_width: 640,
            max_height: 360,
            autoplay: false,
        }
    }
}
impl PlaybackOptions {
    pub(crate) fn validate(&self) -> Result<(), MediaError> {
        if !(1..=60).contains(&self.frames_per_second)
            || !(1..=1920).contains(&self.max_width)
            || !(1..=1080).contains(&self.max_height)
            || !self.volume.is_finite()
            || !(0. ..=1.).contains(&self.volume)
        {
            return Err(MediaError::ResourceLimit(
                "preview requires 1–60 fps, at most 1920×1080 and volume 0–1".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct MediaMetadata {
    pub duration: f64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub has_video: bool,
    pub has_audio: bool,
    pub audio_sample_rate: Option<u32>,
    pub audio_channels: Option<u32>,
    pub format: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    Loading,
    Ready,
    Playing,
    Paused,
    Stopped,
    Ended,
    Error,
}
#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    /// Packed, top-down BGRA pixels (4 bytes per pixel), matching GPUI RenderImage.
    pub bgra: Arc<[u8]>,
    pub position: f64,
    pub sequence: u64,
}
#[derive(Clone, Debug)]
pub struct PlaybackSnapshot {
    pub state: PlaybackState,
    pub position: f64,
    pub metadata: Option<MediaMetadata>,
    pub frame: Option<VideoFrame>,
    pub volume: f32,
    pub audio_output: AudioOutput,
    pub error: Option<MediaError>,
    pub revision: u64,
    /// Owned subprocess IDs, exposed for diagnostics and teardown verification.
    pub process_ids: Vec<u32>,
}
impl PlaybackSnapshot {
    pub(crate) fn loading(options: &PlaybackOptions) -> Self {
        Self {
            state: PlaybackState::Loading,
            position: 0.,
            metadata: None,
            frame: None,
            volume: options.volume,
            audio_output: options.audio,
            error: None,
            revision: 0,
            process_ids: Vec::new(),
        }
    }
}

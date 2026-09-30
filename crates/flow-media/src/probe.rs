use crate::{FfmpegTools, MediaError, MediaLimits, MediaMetadata, process};
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

pub(crate) fn source(path: &Path, limits: &MediaLimits) -> Result<std::path::PathBuf, MediaError> {
    let path = path
        .canonicalize()
        .map_err(|error| MediaError::InvalidSource(error.to_string()))?;
    let metadata = path
        .metadata()
        .map_err(|error| MediaError::InvalidSource(error.to_string()))?;
    if !metadata.is_file() {
        return Err(MediaError::InvalidSource(
            "only regular local files are accepted".into(),
        ));
    }
    if metadata.len() == 0 || metadata.len() > limits.max_input_bytes {
        return Err(MediaError::ResourceLimit(
            "empty file or input exceeds configured byte limit".into(),
        ));
    }
    Ok(path)
}
pub(crate) fn probe(
    path: &Path,
    tools: &FfmpegTools,
    limits: &MediaLimits,
    cancel: &Arc<AtomicBool>,
) -> Result<MediaMetadata, MediaError> {
    let mut command = process::command(&tools.ffprobe);
    command.args(["-v","error","-max_alloc","67108864","-protocol_whitelist","file","-format_whitelist",process::FORMAT_WHITELIST,
        "-show_entries","format=format_name,duration:stream=codec_type,width,height,sample_rate,channels,duration","-of","json"])
        .arg(path);
    let bytes = process::capture(&mut command, limits.probe_timeout, cancel, 1024 * 1024)?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| MediaError::InvalidMedia(e.to_string()))?;
    let streams = value["streams"]
        .as_array()
        .ok_or_else(|| MediaError::InvalidMedia("no stream information".into()))?;
    if streams.len() > 64 {
        return Err(MediaError::ResourceLimit("more than 64 streams".into()));
    }
    let video = streams
        .iter()
        .find(|s| s["codec_type"].as_str() == Some("video"));
    let audio = streams
        .iter()
        .find(|s| s["codec_type"].as_str() == Some("audio"));
    if video.is_none() && audio.is_none() {
        return Err(MediaError::InvalidMedia("no video or audio stream".into()));
    }
    let number = |v: &serde_json::Value| v.as_f64().or_else(|| v.as_str()?.parse().ok());
    let duration = number(&value["format"]["duration"])
        .or_else(|| {
            streams
                .iter()
                .filter_map(|s| number(&s["duration"]))
                .max_by(f64::total_cmp)
        })
        .ok_or_else(|| {
            MediaError::InvalidMedia(
                "finite duration is required (live streams are unsupported)".into(),
            )
        })?;
    if !duration.is_finite() || duration <= 0. || duration > limits.max_duration.as_secs_f64() {
        return Err(MediaError::ResourceLimit(
            "duration is invalid or exceeds configured limit".into(),
        ));
    }
    let width = video
        .and_then(|s| s["width"].as_u64())
        .and_then(|n| u32::try_from(n).ok());
    let height = video
        .and_then(|s| s["height"].as_u64())
        .and_then(|n| u32::try_from(n).ok());
    if video.is_some()
        && (width.unwrap_or(0) == 0
            || height.unwrap_or(0) == 0
            || u64::from(width.unwrap_or(0)) * u64::from(height.unwrap_or(0))
                > limits.max_source_pixels)
    {
        return Err(MediaError::ResourceLimit(
            "video dimensions are missing or too large".into(),
        ));
    }
    Ok(MediaMetadata {
        duration,
        width,
        height,
        has_video: video.is_some(),
        has_audio: audio.is_some(),
        audio_sample_rate: audio
            .and_then(|s| number(&s["sample_rate"]))
            .map(|n| n as u32),
        audio_channels: audio.and_then(|s| s["channels"].as_u64()).map(|n| n as u32),
        format: value["format"]["format_name"]
            .as_str()
            .unwrap_or("unknown")
            .into(),
    })
}

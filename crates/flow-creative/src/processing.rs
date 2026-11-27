//! Bounded, local-only media transforms for explicit background jobs.
//!
//! The host resolves an immutable project asset, chooses a fresh temporary
//! destination, and imports the returned file only after success. This module
//! never updates a project or resolves a path from serialized node settings.
//! A completed output belongs to the caller; incomplete outputs are removed.

use crate::ProjectAsset;
use gpui_flow_core::DataType;
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Component, Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(feature = "desktop")]
pub(crate) mod image;
mod media;

pub const MAX_PROCESS_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
/// Processing deliberately has a tighter pixel limit than project import.
pub const MAX_PROCESS_IMAGE_PIXELS: u64 = 16 * 1024 * 1024;
pub const MAX_PROCESS_MEDIA_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_PROCESS_MEDIA_SECONDS: u64 = 30 * 60;
pub const PROCESS_IMAGE_TIMEOUT: Duration = Duration::from_secs(60);
pub const PROCESS_MEDIA_TIMEOUT: Duration = Duration::from_secs(5 * 60);

// CPU image processing and all FFmpeg/FFprobe work are serialized. A host may
// queue jobs, but this module never starts concurrent transcoders or blocks
// indefinitely waiting for the gate.
static PROCESS_GATE: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingKind {
    Image,
    Video,
    Audio,
}

impl ProcessingKind {
    pub fn node_kind(self) -> &'static str {
        match self {
            Self::Image => "process_image",
            Self::Video => "process_video",
            Self::Audio => "process_audio",
        }
    }

    pub fn from_node_kind(kind: &str) -> Option<Self> {
        [Self::Image, Self::Video, Self::Audio]
            .into_iter()
            .find(|candidate| candidate.node_kind() == kind)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Image => crate::tr("图片处理", "Image processing"),
            Self::Video => crate::tr("视频处理", "Video processing"),
            Self::Audio => crate::tr("音频处理", "Audio processing"),
        }
    }

    pub fn input_type(self) -> DataType {
        DataType::Named(
            match self {
                Self::Image => "image",
                Self::Video => "video",
                Self::Audio => "audio",
            }
            .into(),
        )
    }

    pub fn output_type(self) -> DataType {
        self.input_type()
    }

    pub fn default_settings(self) -> ProcessingSettings {
        match self {
            Self::Image => ProcessingSettings::Image {
                format: ImageOutputFormat::Jpeg,
                quality: 85,
                max_edge: 2048,
            },
            Self::Video => ProcessingSettings::Video {
                crf: 23,
                max_edge: 1920,
            },
            Self::Audio => ProcessingSettings::Audio {
                format: AudioOutputFormat::Mp3,
                bitrate_kbps: 192,
                sample_rate: 48000,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageOutputFormat {
    Png,
    Jpeg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioOutputFormat {
    Wav,
    Mp3,
}

/// Data only: no executable, path, script, free-form filter, or remote URL.
/// Image JPEG quality is 1..=100; PNG encoding is always lossless. Image and
/// video maximum edge preserve aspect ratio and never upscale. WAV is PCM16;
/// MP3 uses the chosen bitrate. Audio processing selects the first audio track.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProcessingSettings {
    Image {
        format: ImageOutputFormat,
        quality: u8,
        max_edge: u32,
    },
    Video {
        crf: u8,
        max_edge: u32,
    },
    Audio {
        format: AudioOutputFormat,
        bitrate_kbps: u16,
        sample_rate: u32,
    },
}

impl ProcessingSettings {
    pub fn kind(&self) -> ProcessingKind {
        match self {
            Self::Image { .. } => ProcessingKind::Image,
            Self::Video { .. } => ProcessingKind::Video,
            Self::Audio { .. } => ProcessingKind::Audio,
        }
    }

    pub fn validate(&self) -> Result<(), ProcessingError> {
        match *self {
            Self::Image {
                quality, max_edge, ..
            } if (1..=100).contains(&quality) && (1..=8192).contains(&max_edge) => Ok(()),
            Self::Video { crf, max_edge } if crf <= 51 && (2..=4096).contains(&max_edge) => Ok(()),
            Self::Audio {
                bitrate_kbps,
                sample_rate,
                ..
            } if matches!(bitrate_kbps, 64 | 96 | 128 | 160 | 192 | 256 | 320)
                && matches!(sample_rate, 44100 | 48000) =>
            {
                Ok(())
            }
            _ => Err(ProcessingError::InvalidSettings(
                "invalid image quality/size, video CRF/size, or audio bitrate/sample rate".into(),
            )),
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::Image {
                format: ImageOutputFormat::Png,
                ..
            } => "png",
            Self::Image {
                format: ImageOutputFormat::Jpeg,
                ..
            } => "jpg",
            Self::Video { .. } => "mp4",
            Self::Audio {
                format: AudioOutputFormat::Wav,
                ..
            } => "wav",
            Self::Audio {
                format: AudioOutputFormat::Mp3,
                ..
            } => "mp3",
        }
    }

    pub fn media_type(&self) -> &'static str {
        match self {
            Self::Image {
                format: ImageOutputFormat::Png,
                ..
            } => "image/png",
            Self::Image {
                format: ImageOutputFormat::Jpeg,
                ..
            } => "image/jpeg",
            Self::Video { .. } => "video/mp4",
            Self::Audio {
                format: AudioOutputFormat::Wav,
                ..
            } => "audio/wav",
            Self::Audio {
                format: AudioOutputFormat::Mp3,
                ..
            } => "audio/mpeg",
        }
    }
}

#[derive(Debug)]
pub enum ProcessingError {
    InvalidSettings(String),
    InvalidSource(String),
    InvalidOutput(String),
    Unsupported(String),
    ResourceLimit(String),
    ToolUnavailable(String),
    Process(String),
    Io(io::Error),
    Busy,
    Cancelled,
    Timeout,
}

impl fmt::Display for ProcessingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSettings(message)
            | Self::InvalidSource(message)
            | Self::InvalidOutput(message)
            | Self::Unsupported(message)
            | Self::ResourceLimit(message)
            | Self::ToolUnavailable(message)
            | Self::Process(message) => f.write_str(message),
            Self::Io(error) => error.fmt(f),
            Self::Busy => f.write_str("another local media processing job is running"),
            Self::Cancelled => f.write_str("media processing cancelled"),
            Self::Timeout => f.write_str("media processing exceeded its time limit"),
        }
    }
}

impl std::error::Error for ProcessingError {}

impl From<io::Error> for ProcessingError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Real, fully written output. Dimensions are zero for audio. The host must
/// import this file into its current project before exposing an asset key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessingOutput {
    pub path: PathBuf,
    pub media_type: String,
    pub byte_len: u64,
    pub width: u32,
    pub height: u32,
}

/// Run on a background worker. Only a host-resolved project source and an
/// independent fresh destination may be passed. Existing files, symlinks,
/// aliases and non-regular sources are refused. This protects against
/// untrusted settings/assets, not concurrent hostile replacement of trusted
/// parent directories. Image cancellation is cooperative between bounded
/// decode/resize/encode steps and during reads/writes; subprocess cancellation
/// kills and waits for the child before returning.
pub fn execute_processing(
    settings: &ProcessingSettings,
    source: &Path,
    source_asset: &ProjectAsset,
    output: &Path,
    cancel: &AtomicBool,
) -> Result<ProcessingOutput, ProcessingError> {
    settings.validate()?;
    check_cancel(cancel)?;
    let _gate = PROCESS_GATE.try_lock().map_err(|_| ProcessingError::Busy)?;
    let image = settings.kind() == ProcessingKind::Image;
    let limit = if image {
        MAX_PROCESS_IMAGE_BYTES
    } else {
        MAX_PROCESS_MEDIA_BYTES
    };
    let source = checked_source(source, source_asset, settings.kind(), limit)?;
    let output = checked_destination(&source, output)?;
    match settings {
        ProcessingSettings::Image { .. } => {
            #[cfg(feature = "desktop")]
            {
                image::execute(settings, &source, source_asset, &output, cancel)
            }
            #[cfg(not(feature = "desktop"))]
            {
                Err(ProcessingError::Unsupported(
                    "image processing requires the desktop image codec feature".into(),
                ))
            }
        }
        _ => media::execute(settings, &source, source_asset, &output, cancel),
    }
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), ProcessingError> {
    if cancel.load(Ordering::Acquire) {
        Err(ProcessingError::Cancelled)
    } else {
        Ok(())
    }
}

fn check_deadline(
    cancel: &AtomicBool,
    start: Instant,
    timeout: Duration,
) -> Result<(), ProcessingError> {
    check_cancel(cancel)?;
    if start.elapsed() >= timeout {
        Err(ProcessingError::Timeout)
    } else {
        Ok(())
    }
}

fn checked_components(path: &Path) -> Result<(), ProcessingError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(ProcessingError::InvalidSource(
            "processing requires absolute local paths without parent traversal".into(),
        ));
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() {
            return Err(ProcessingError::InvalidSource(
                "symlink path components are not accepted".into(),
            ));
        }
    }
    Ok(())
}

fn checked_source(
    path: &Path,
    asset: &ProjectAsset,
    kind: ProcessingKind,
    limit: u64,
) -> Result<PathBuf, ProcessingError> {
    checked_components(path)?;
    let path = path.canonicalize()?;
    let metadata = fs::metadata(&path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(ProcessingError::ResourceLimit(
            "source must be a nonempty regular local file within the processing byte limit".into(),
        ));
    }
    if metadata.len() != asset.byte_len {
        return Err(ProcessingError::InvalidSource(
            "source bytes no longer match the selected project asset".into(),
        ));
    }
    let expected = match kind {
        ProcessingKind::Image => matches!(
            asset.media_type.as_str(),
            "image/png" | "image/jpeg" | "image/gif" | "image/webp"
        ),
        ProcessingKind::Video => matches!(asset.media_type.as_str(), "video/mp4" | "video/webm"),
        ProcessingKind::Audio => matches!(
            asset.media_type.as_str(),
            "audio/wav" | "audio/mpeg" | "audio/flac" | "audio/ogg" | "audio/mp4"
        ),
    };
    if !expected {
        return Err(ProcessingError::InvalidSource(
            "source asset has the wrong exact media type for this processor".into(),
        ));
    }
    Ok(path)
}

fn checked_destination(source: &Path, output: &Path) -> Result<PathBuf, ProcessingError> {
    let parent = output.parent().ok_or_else(|| {
        ProcessingError::InvalidOutput("output needs a local parent directory".into())
    })?;
    let name = output
        .file_name()
        .ok_or_else(|| ProcessingError::InvalidOutput("output needs a filename".into()))?;
    checked_components(parent)?;
    if !fs::metadata(parent)?.is_dir() {
        return Err(ProcessingError::InvalidOutput(
            "output parent is not a directory".into(),
        ));
    }
    let output = parent.canonicalize()?.join(name);
    if output == source {
        return Err(ProcessingError::InvalidOutput(
            "source and output cannot be the same file".into(),
        ));
    }
    match fs::symlink_metadata(&output) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(output),
        Err(error) => Err(error.into()),
        Ok(_) => Err(ProcessingError::InvalidOutput(
            "output already exists; existing files are never overwritten".into(),
        )),
    }
}

#[cfg(feature = "desktop")]
fn read_bounded(path: &Path, limit: u64, cancel: &AtomicBool) -> Result<Vec<u8>, ProcessingError> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(ProcessingError::ResourceLimit(
            "source exceeds the byte limit or is not a regular file".into(),
        ));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(metadata.len() as usize)
        .map_err(|_| {
            ProcessingError::ResourceLimit("could not allocate bounded source bytes".into())
        })?;
    let mut reader = file.take(limit + 1);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        check_cancel(cancel)?;
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        if bytes.len() as u64 + count as u64 > limit {
            return Err(ProcessingError::ResourceLimit(
                "source grew beyond its byte limit".into(),
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(bytes)
}

/// Owns only the file successfully created here; never deletes a pre-existing
/// output on error. All writers/processes must finish before this guard drops.
struct OutputGuard {
    path: PathBuf,
    file: Option<File>,
    complete: bool,
}

impl OutputGuard {
    fn create(path: &Path) -> Result<Self, ProcessingError> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        Ok(Self {
            path: path.to_owned(),
            file: Some(file),
            complete: false,
        })
    }

    fn finish(
        mut self,
        settings: &ProcessingSettings,
        dimensions: (u32, u32),
        limit: u64,
        cancel: &AtomicBool,
    ) -> Result<ProcessingOutput, ProcessingError> {
        check_cancel(cancel)?;
        if let Some(file) = self.file.take() {
            file.sync_all()?;
        }
        let metadata = fs::symlink_metadata(&self.path)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
            return Err(ProcessingError::InvalidOutput(
                "processor did not produce a nonempty bounded regular file".into(),
            ));
        }
        check_cancel(cancel)?;
        let result = ProcessingOutput {
            path: self.path.clone(),
            media_type: settings.media_type().into(),
            byte_len: metadata.len(),
            width: dimensions.0,
            height: dimensions.1,
        };
        self.complete = true;
        Ok(result)
    }
}

impl Drop for OutputGuard {
    fn drop(&mut self) {
        if !self.complete {
            self.file.take();
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);
    pub(super) struct TestDirectory(pub PathBuf);
    impl TestDirectory {
        pub(super) fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "flow-processing-{}-{}",
                std::process::id(),
                NEXT_TEST.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn settings_reject_invalid_values_and_freeform_commands() {
        for kind in [
            ProcessingKind::Image,
            ProcessingKind::Video,
            ProcessingKind::Audio,
        ] {
            let settings = kind.default_settings();
            assert_eq!(settings.kind(), kind);
            assert_eq!(kind.input_type(), kind.output_type());
            assert!(settings.validate().is_ok());
            assert_eq!(
                serde_json::from_str::<ProcessingSettings>(
                    &serde_json::to_string(&settings).unwrap()
                )
                .unwrap(),
                settings
            );
        }
        for settings in [
            ProcessingSettings::Image {
                format: ImageOutputFormat::Jpeg,
                quality: 0,
                max_edge: 2048,
            },
            ProcessingSettings::Image {
                format: ImageOutputFormat::Png,
                quality: 85,
                max_edge: 0,
            },
            ProcessingSettings::Video {
                crf: 52,
                max_edge: 1920,
            },
            ProcessingSettings::Video {
                crf: 23,
                max_edge: 4097,
            },
            ProcessingSettings::Audio {
                format: AudioOutputFormat::Mp3,
                bitrate_kbps: 1,
                sample_rate: 48000,
            },
            ProcessingSettings::Audio {
                format: AudioOutputFormat::Wav,
                bitrate_kbps: 192,
                sample_rate: u32::MAX,
            },
        ] {
            assert!(settings.validate().is_err());
        }
        assert!(
            serde_json::from_str::<ProcessingSettings>(
                r#"{"kind":"video","crf":23,"max_edge":1920,"filter":"movie=https://example.com"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<ProcessingSettings>(
                r#"{"kind":"audio","format":"script","bitrate_kbps":192,"sample_rate":48000}"#
            )
            .is_err()
        );
    }

    #[test]
    fn output_guard_never_overwrites_and_cleans_up_failed_work() {
        let directory = TestDirectory::new();
        let existing = directory.0.join("existing.png");
        fs::write(&existing, b"keep").unwrap();
        assert!(OutputGuard::create(&existing).is_err());
        assert_eq!(fs::read(existing).unwrap(), b"keep");
        let failed = directory.0.join("failed.png");
        {
            let _output = OutputGuard::create(&failed).unwrap();
        }
        assert!(!failed.exists());
        let cancelled = directory.0.join("cancelled.png");
        let output = OutputGuard::create(&cancelled).unwrap();
        assert!(matches!(
            output.finish(
                &ProcessingKind::Image.default_settings(),
                (1, 1),
                MAX_PROCESS_IMAGE_BYTES,
                &AtomicBool::new(true)
            ),
            Err(ProcessingError::Cancelled)
        ));
        assert!(!cancelled.exists());
    }

    #[test]
    fn source_and_existing_alias_destinations_are_rejected() {
        let directory = TestDirectory::new();
        let source = directory.0.join("source.png");
        fs::write(&source, b"unchanged").unwrap();
        assert!(checked_destination(&source, &source).is_err());
        let alias = directory.0.join("alias.png");
        fs::hard_link(&source, &alias).unwrap();
        assert!(checked_destination(&source, &alias).is_err());
        #[cfg(unix)]
        {
            let symlink = directory.0.join("symlink.png");
            std::os::unix::fs::symlink(&source, &symlink).unwrap();
            assert!(checked_destination(&source, &symlink).is_err());
            assert!(checked_components(&symlink).is_err());
        }
        assert_eq!(fs::read(source).unwrap(), b"unchanged");
    }
}

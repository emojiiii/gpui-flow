use crate::{
    FfmpegTools, MediaController, MediaError, MediaLimits, MediaMetadata, MediaPlayer,
    PlaybackOptions, process,
};
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

/// Implement this trait to supply another decoder/player to the same GPUI adapter.
pub trait MediaBackend: Send + Sync {
    fn open(
        &self,
        source: &Path,
        options: PlaybackOptions,
    ) -> Result<Arc<dyn MediaController>, MediaError>;
}
#[derive(Clone, Debug, Default)]
pub struct FfmpegBackend {
    pub tools: FfmpegTools,
    pub limits: MediaLimits,
}
#[derive(Clone, Debug)]
pub struct ToolAvailability {
    pub ffmpeg: Result<String, MediaError>,
    pub ffprobe: Result<String, MediaError>,
    pub ffplay: Result<String, MediaError>,
}
impl FfmpegBackend {
    /// Extract a real, fixed-size local audio overview without starting playback.
    /// Run on a background worker. Probe and decode share a 15-second deadline;
    /// input is capped by `limits` and 30 minutes, with mono/stereo audio only.
    /// Cancellation kills/reaps the decoder and joins its bounded pipe readers.
    pub fn waveform(
        &self,
        source: &Path,
        cancel: &Arc<AtomicBool>,
    ) -> Result<crate::AudioWaveform, MediaError> {
        crate::waveform::extract(source, &self.tools, &self.limits, cancel)
    }
    /// Inspect one explicitly selected local file without starting playback.
    /// This bounded subprocess operation belongs on a background worker.
    pub fn probe(&self, source: &Path) -> Result<MediaMetadata, MediaError> {
        let source = crate::probe::source(source, &self.limits)?;
        crate::probe::probe(
            &source,
            &self.tools,
            &self.limits,
            &Arc::new(AtomicBool::new(false)),
        )
    }
    /// Bounded executable detection. This runs processes; call off the UI thread.
    pub fn availability(&self) -> ToolAvailability {
        let version = |path: &Path| {
            let mut command = process::command(path);
            command.arg("-version");
            process::capture(
                &mut command,
                Duration::from_secs(3),
                &Arc::new(AtomicBool::new(false)),
                64 * 1024,
            )
            .map(|bytes| {
                String::from_utf8_lossy(&bytes)
                    .lines()
                    .next()
                    .unwrap_or("unknown version")
                    .to_owned()
            })
        };
        ToolAvailability {
            ffmpeg: version(&self.tools.ffmpeg),
            ffprobe: version(&self.tools.ffprobe),
            ffplay: version(&self.tools.ffplay),
        }
    }
}
impl MediaBackend for FfmpegBackend {
    fn open(
        &self,
        source: &Path,
        options: PlaybackOptions,
    ) -> Result<Arc<dyn MediaController>, MediaError> {
        options.validate()?;
        let source = crate::probe::source(source, &self.limits)?;
        MediaPlayer::spawn(source, self.clone(), options)
            .map(|player| Arc::new(player) as Arc<dyn MediaController>)
    }
}

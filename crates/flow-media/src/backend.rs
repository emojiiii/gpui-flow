use crate::{
    FfmpegTools, MediaController, MediaError, MediaLimits, MediaPlayer, PlaybackOptions, process,
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

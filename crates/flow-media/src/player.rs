use crate::{FfmpegBackend, MediaError, PlaybackOptions, PlaybackSnapshot};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Command {
    Play,
    Pause,
    Stop,
    Seek(f64),
    Volume(f32),
    Shutdown,
}
/// Backend-independent control surface consumed by native UI adapters.
pub trait MediaController: Send + Sync {
    fn snapshot(&self) -> PlaybackSnapshot;
    /// Owned backends expose the same pool-generation protocol as image jobs.
    fn resource_event(&self) -> Option<crate::ResourceLoadEvent> {
        None
    }
    fn play(&self) -> Result<(), MediaError>;
    fn pause(&self) -> Result<(), MediaError>;
    fn stop(&self) -> Result<(), MediaError>;
    fn seek(&self, seconds: f64) -> Result<(), MediaError>;
    fn set_volume(&self, volume: f32) -> Result<(), MediaError>;
    fn shutdown(&self);
}
/// Thread-safe handle. Commands never block the UI. `shutdown` and `Drop` stop
/// and join the playback worker.
pub struct MediaPlayer {
    commands: SyncSender<Command>,
    shared: Arc<Mutex<PlaybackSnapshot>>,
    cancel: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl MediaPlayer {
    pub(crate) fn spawn(
        source: PathBuf,
        backend: FfmpegBackend,
        options: PlaybackOptions,
    ) -> Result<Self, MediaError> {
        Self::spawn_worker(options, move |options, receiver, state, cancelled| {
            crate::engine::Engine::run(source, backend, options, receiver, state, cancelled);
        })
    }

    pub(crate) fn spawn_worker(
        options: PlaybackOptions,
        run: impl FnOnce(
            PlaybackOptions,
            mpsc::Receiver<Command>,
            Arc<Mutex<PlaybackSnapshot>>,
            Arc<AtomicBool>,
        ) + Send
        + 'static,
    ) -> Result<Self, MediaError> {
        let (commands, receiver) = mpsc::sync_channel(32);
        let shared = Arc::new(Mutex::new(PlaybackSnapshot::loading(&options)));
        let cancel = Arc::new(AtomicBool::new(false));
        let state = shared.clone();
        let cancelled = cancel.clone();
        let worker = thread::Builder::new()
            .name("flow-media-player".into())
            .spawn(move || run(options, receiver, state, cancelled))
            .map_err(|error| MediaError::Process(error.to_string()))?;
        Ok(Self {
            commands,
            shared,
            cancel,
            worker: Mutex::new(Some(worker)),
        })
    }
    pub fn snapshot(&self) -> PlaybackSnapshot {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    fn send(&self, command: Command) -> Result<(), MediaError> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(MediaError::PlayerClosed);
        }
        self.commands.try_send(command).map_err(|e| match e {
            TrySendError::Full(_) => MediaError::CommandQueueFull,
            TrySendError::Disconnected(_) => MediaError::PlayerClosed,
        })
    }
    pub fn play(&self) -> Result<(), MediaError> {
        self.send(Command::Play)
    }
    pub fn pause(&self) -> Result<(), MediaError> {
        self.send(Command::Pause)
    }
    /// Stop decoding/output and reset position. Use `play` to begin again.
    pub fn stop(&self) -> Result<(), MediaError> {
        self.send(Command::Stop)
    }
    /// Finite, nonnegative seconds. Values beyond duration are clamped after probe.
    pub fn seek(&self, seconds: f64) -> Result<(), MediaError> {
        if !seconds.is_finite() || seconds < 0. {
            return Err(MediaError::InvalidSource(
                "seek position must be finite and nonnegative".into(),
            ));
        }
        self.send(Command::Seek(seconds))
    }
    pub fn set_volume(&self, volume: f32) -> Result<(), MediaError> {
        if !volume.is_finite() || !(0. ..=1.).contains(&volume) {
            return Err(MediaError::InvalidSource(
                "volume must be between 0 and 1".into(),
            ));
        }
        self.send(Command::Volume(volume))
    }
    pub fn shutdown(&self) {
        self.cancel.store(true, Ordering::Release);
        let _ = self.commands.try_send(Command::Shutdown);
        if let Some(worker) = self.worker.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = worker.join();
        }
    }
}
impl Drop for MediaPlayer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl MediaController for MediaPlayer {
    fn snapshot(&self) -> PlaybackSnapshot {
        MediaPlayer::snapshot(self)
    }
    fn play(&self) -> Result<(), MediaError> {
        MediaPlayer::play(self)
    }
    fn pause(&self) -> Result<(), MediaError> {
        MediaPlayer::pause(self)
    }
    fn stop(&self) -> Result<(), MediaError> {
        MediaPlayer::stop(self)
    }
    fn seek(&self, seconds: f64) -> Result<(), MediaError> {
        MediaPlayer::seek(self, seconds)
    }
    fn set_volume(&self, volume: f32) -> Result<(), MediaError> {
        MediaPlayer::set_volume(self, volume)
    }
    fn shutdown(&self) {
        MediaPlayer::shutdown(self)
    }
}

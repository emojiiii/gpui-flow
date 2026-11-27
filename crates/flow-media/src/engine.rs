//! Command-driven playback state machine and common preview clock.
use crate::{
    FfmpegBackend, MediaError, MediaMetadata, PlaybackOptions, PlaybackSnapshot, PlaybackState,
    decoder::DecodeSession, player::Command, probe,
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

pub(crate) struct Engine {
    source: PathBuf,
    backend: FfmpegBackend,
    options: PlaybackOptions,
    metadata: MediaMetadata,
    quarter_turn: bool,
    snapshot: PlaybackSnapshot,
    shared: Arc<Mutex<PlaybackSnapshot>>,
    session: Option<DecodeSession>,
    clock: Option<Instant>,
    start_position: f64,
    sequence: u64,
}
impl Engine {
    pub(crate) fn run(
        source: PathBuf,
        backend: FfmpegBackend,
        options: PlaybackOptions,
        commands: Receiver<Command>,
        shared: Arc<Mutex<PlaybackSnapshot>>,
        cancel: Arc<AtomicBool>,
    ) {
        let playback =
            match probe::probe_playback(&source, &backend.tools, &backend.limits, &cancel) {
                Ok(value) => value,
                Err(error) => {
                    let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
                    state.state = if cancel.load(Ordering::Acquire) {
                        PlaybackState::Stopped
                    } else {
                        PlaybackState::Error
                    };
                    state.error = (!cancel.load(Ordering::Acquire)).then_some(error);
                    state.revision += 1;
                    return;
                }
            };
        Self::run_probed(source, backend, options, commands, shared, cancel, playback);
    }

    pub(crate) fn run_probed(
        source: PathBuf,
        backend: FfmpegBackend,
        options: PlaybackOptions,
        commands: Receiver<Command>,
        shared: Arc<Mutex<PlaybackSnapshot>>,
        cancel: Arc<AtomicBool>,
        playback: probe::PlaybackProbe,
    ) {
        let probe::PlaybackProbe {
            metadata,
            quarter_turn,
        } = playback;
        let mut snapshot = PlaybackSnapshot::loading(&options);
        snapshot.metadata = Some(metadata.clone());
        snapshot.state = PlaybackState::Ready;
        let autoplay = options.autoplay;
        let mut engine = Self {
            source,
            backend,
            options,
            metadata,
            quarter_turn,
            snapshot,
            shared,
            session: None,
            clock: None,
            start_position: 0.,
            sequence: 0,
        };
        if let Err(error) = engine.start(autoplay) {
            engine.fail(error);
        }
        loop {
            if cancel.load(Ordering::Acquire) {
                break;
            }
            match commands.recv_timeout(Duration::from_millis(10)) {
                Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(command) => {
                    if let Err(error) = engine.command(command) {
                        engine.fail(error);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if let Err(error) = engine.tick() {
                engine.fail(error);
            }
        }
        engine.session = None;
        engine.snapshot.state = PlaybackState::Stopped;
        engine.snapshot.frame = None;
        engine.publish();
    }
    fn publish(&mut self) {
        self.snapshot.process_ids = self
            .session
            .as_ref()
            .map(DecodeSession::process_ids)
            .unwrap_or_default();
        self.snapshot.revision += 1;
        *self.shared.lock().unwrap_or_else(|e| e.into_inner()) = self.snapshot.clone();
    }
    fn update_position(&mut self) {
        if let Some(clock) = self.clock {
            self.snapshot.position =
                (self.start_position + clock.elapsed().as_secs_f64()).min(self.metadata.duration);
        }
    }
    fn start(&mut self, playing: bool) -> Result<(), MediaError> {
        self.session = None;
        self.clock = None;
        self.snapshot.error = None;
        self.start_position = self.snapshot.position;
        if self.snapshot.position >= self.metadata.duration {
            self.snapshot.state = PlaybackState::Ended;
            self.publish();
            return Ok(());
        }
        self.session = Some(DecodeSession::start(
            &self.source,
            &self.backend,
            &self.options,
            &self.metadata,
            self.quarter_turn,
            self.snapshot.position,
            playing,
        )?);
        if playing {
            self.clock = Some(Instant::now());
            self.snapshot.state = PlaybackState::Playing;
        }
        self.publish();
        Ok(())
    }
    fn command(&mut self, command: Command) -> Result<(), MediaError> {
        self.update_position();
        match command {
            Command::Play => {
                if self.snapshot.state == PlaybackState::Playing {
                    return Ok(());
                }
                if self.snapshot.position >= self.metadata.duration {
                    self.snapshot.position = 0.;
                    self.snapshot.frame = None;
                }
                self.start(true)?;
            }
            Command::Pause => {
                self.clock = None;
                self.session = None;
                self.snapshot.state = PlaybackState::Paused;
                self.publish();
            }
            Command::Stop => {
                self.clock = None;
                self.session = None;
                self.snapshot.position = 0.;
                self.snapshot.frame = None;
                self.snapshot.state = PlaybackState::Stopped;
                self.publish();
            }
            Command::Seek(position) => {
                let playing = self.snapshot.state == PlaybackState::Playing;
                self.snapshot.position = position.min(self.metadata.duration);
                self.snapshot.frame = None;
                if !playing {
                    self.snapshot.state = PlaybackState::Paused;
                }
                self.start(playing)?;
            }
            Command::Volume(volume) => {
                self.options.volume = volume;
                self.snapshot.volume = volume;
                if self.snapshot.state == PlaybackState::Playing {
                    self.start(true)?;
                } else {
                    self.publish();
                }
            }
            Command::Shutdown => {}
        }
        Ok(())
    }
    fn tick(&mut self) -> Result<(), MediaError> {
        self.update_position();
        let mut changed = self.snapshot.state == PlaybackState::Playing;
        if let Some(session) = &mut self.session {
            if let Some(frame) = session.frame(self.snapshot.position, &mut self.sequence)? {
                self.snapshot.frame = Some(frame);
                changed = true;
            }
            session.check(&self.backend.limits)?;
        }
        if self.snapshot.state == PlaybackState::Playing
            && self.snapshot.position >= self.metadata.duration
        {
            self.clock = None;
            self.session = None;
            self.snapshot.state = PlaybackState::Ended;
            changed = true;
        }
        if changed {
            self.publish();
        }
        Ok(())
    }
    fn fail(&mut self, error: MediaError) {
        self.update_position();
        self.clock = None;
        self.session = None;
        self.snapshot.state = PlaybackState::Error;
        self.snapshot.error = Some(error);
        self.publish();
    }
}

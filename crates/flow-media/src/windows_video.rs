//! Windows MP4 playback through Media Foundation's frame server.
use crate::{
    AudioOutput, FfmpegBackend, MediaBackend, MediaController, MediaError, MediaLimits,
    MediaMetadata, MediaPlayer, PlaybackOptions, PlaybackSnapshot, PlaybackState, VideoFrame,
    backend::tool_version, engine::Engine, player::Command, probe::PlaybackProbe,
};
use std::{
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{RECT, RPC_E_CHANGED_MODE, S_FALSE},
        Graphics::{
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Imaging::{
                CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICBitmap,
                IWICImagingFactory, WICBitmapCacheOnLoad,
            },
        },
        Media::MediaFoundation::*,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
            StructuredStorage::{
                PROPVARIANT, PropVariantClear, PropVariantToGUID, PropVariantToUInt32,
                PropVariantToUInt64,
            },
        },
    },
    core::{BSTR, GUID, Interface, PCWSTR, implement, w},
};

/// Local MP4 playback using Windows' installed Media Foundation codecs.
#[derive(Clone, Debug, Default)]
pub struct WindowsVideoBackend {
    pub limits: MediaLimits,
}

enum FfmpegPlayback {
    Disabled,
    Fallback(FfmpegBackend),
    Preferred(FfmpegBackend),
}

impl WindowsVideoBackend {
    /// Probe a local MP4 file on a background worker.
    pub fn probe(&self, source: &Path) -> Result<MediaMetadata, MediaError> {
        let source = crate::probe::source(source, &self.limits)?;
        let _foundation = MediaFoundation::start()?;
        Player::open(&source, &self.limits, MF_MEDIA_ENGINE_PRELOAD_METADATA)?
            .metadata(&self.limits, &AtomicBool::new(false))
    }

    /// Attempt native decoding first, then hand compatibility failures to a
    /// host-supplied fallback on the same worker and controller.
    pub fn open_with_fallback(
        &self,
        source: &Path,
        options: PlaybackOptions,
        fallback: FfmpegBackend,
    ) -> Result<Arc<dyn MediaController>, MediaError> {
        self.open_worker(source, options, FfmpegPlayback::Fallback(fallback))
    }

    /// Use FFmpeg when the required tools are available, or Media Foundation
    /// when they are missing. Detection and playback share the same worker.
    pub fn open_preferring_ffmpeg(
        &self,
        source: &Path,
        options: PlaybackOptions,
        ffmpeg: FfmpegBackend,
    ) -> Result<Arc<dyn MediaController>, MediaError> {
        self.open_worker(source, options, FfmpegPlayback::Preferred(ffmpeg))
    }

    fn open_worker(
        &self,
        source: &Path,
        options: PlaybackOptions,
        ffmpeg: FfmpegPlayback,
    ) -> Result<Arc<dyn MediaController>, MediaError> {
        options.validate()?;
        let source = crate::probe::source(source, &self.limits)?;
        let limits = self.limits.clone();
        MediaPlayer::spawn_worker(options, move |options, commands, shared, cancel| {
            let preferred = match &ffmpeg {
                FfmpegPlayback::Preferred(backend) => {
                    probe_ffmpeg_playback(&source, backend, &options, &cancel)
                        .map(|probe| probe.map(|probe| (backend.clone(), probe)))
                }
                _ => Ok(None),
            };
            let result = match preferred {
                Ok(Some((backend, probe))) => {
                    Engine::run_probed(source, backend, options, commands, shared, cancel, probe);
                    return;
                }
                Ok(None) => run_native(&source, &options, &limits, &commands, &shared, &cancel),
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                if matches!(error, MediaError::InvalidMedia(_) | MediaError::Process(_))
                    && let FfmpegPlayback::Fallback(backend) = ffmpeg
                    && !cancel.load(Ordering::Acquire)
                {
                    Engine::run(source, backend, options, commands, shared, cancel);
                    return;
                }
                let mut snapshot = PlaybackSnapshot::loading(&options);
                snapshot.finish(Err(error));
                publish(&mut snapshot, &shared);
            }
        })
        .map(|player| Arc::new(player) as Arc<dyn MediaController>)
    }
}

fn probe_ffmpeg_playback(
    source: &Path,
    backend: &FfmpegBackend,
    options: &PlaybackOptions,
    cancel: &Arc<AtomicBool>,
) -> Result<Option<PlaybackProbe>, MediaError> {
    let available = |tool: &Path| match tool_version(tool, cancel) {
        Ok(_) => Ok(true),
        Err(MediaError::Cancelled) => Err(MediaError::Cancelled),
        Err(_) => Ok(false),
    };
    if !available(&backend.tools.ffmpeg)? {
        return Ok(None);
    }
    let source = crate::probe::source(source, &backend.limits)?;
    let probe = match crate::probe::probe_playback(&source, &backend.tools, &backend.limits, cancel)
    {
        Ok(probe) => probe,
        Err(MediaError::ToolUnavailable(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if probe.metadata.has_audio
        && options.audio == AudioOutput::System
        && !available(&backend.tools.ffplay)?
    {
        return Ok(None);
    }
    Ok(Some(probe))
}

impl MediaBackend for WindowsVideoBackend {
    fn open(
        &self,
        source: &Path,
        options: PlaybackOptions,
    ) -> Result<Arc<dyn MediaController>, MediaError> {
        self.open_worker(source, options, FfmpegPlayback::Disabled)
    }
}

fn media_error(error: windows::core::Error) -> MediaError {
    MediaError::InvalidMedia(format!("Windows Media Foundation: {error}"))
}

struct MediaFoundation {
    initialized_com: bool,
}

impl MediaFoundation {
    fn start() -> Result<Self, MediaError> {
        // The player owns an MTA thread. A probe can also run on a thread
        // already initialized by its host, where COM must remain balanced.
        unsafe {
            let com = CoInitializeEx(None, COINIT_MULTITHREADED);
            if com != RPC_E_CHANGED_MODE {
                com.ok().map_err(media_error)?;
            }
            let initialized_com = com.is_ok();
            if let Err(error) = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) {
                if initialized_com {
                    CoUninitialize();
                }
                return Err(media_error(error));
            }
            Ok(Self { initialized_com })
        }
    }
}

impl Drop for MediaFoundation {
    fn drop(&mut self) {
        // All media-engine and imaging interfaces are released before this
        // guard, while their factories and work queues are still available.
        unsafe {
            let _ = MFShutdown();
            if self.initialized_com {
                CoUninitialize();
            }
        }
    }
}

#[implement(IMFMediaEngineNotify)]
struct Notify(Arc<AtomicU64>);

impl IMFMediaEngineNotify_Impl for Notify_Impl {
    fn EventNotify(&self, event: u32, kind: usize, detail: u32) -> windows::core::Result<()> {
        if event == MF_MEDIA_ENGINE_EVENT_ERROR.0 as u32 {
            self.0
                .store((kind as u64) << 32 | u64::from(detail), Ordering::Release);
        }
        Ok(())
    }
}

struct Player {
    engine: IMFMediaEngine,
    _stream: IMFByteStream,
    error: Arc<AtomicU64>,
}

impl Player {
    fn open(
        path: &Path,
        limits: &MediaLimits,
        preload: MF_MEDIA_ENGINE_PRELOAD,
    ) -> Result<Self, MediaError> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // A local byte stream and explicit MIME type give the decoder only
        // the admitted file, without interpreting a filename as a URL.
        unsafe {
            let stream = MFCreateFile(
                MF_ACCESSMODE_READ,
                MF_OPENMODE_FAIL_IF_NOT_EXIST,
                MF_FILEFLAGS_NONE,
                PCWSTR(wide.as_ptr()),
            )
            .map_err(media_error)?;
            let size = stream.GetLength().map_err(media_error)?;
            if size == 0 || size > limits.max_input_bytes {
                return Err(MediaError::ResourceLimit(
                    "empty file or input exceeds configured byte limit".into(),
                ));
            }
            let mut header = [0; 8];
            let mut read = 0;
            stream.Read(&mut header, &mut read).map_err(media_error)?;
            if read != 8 || &header[4..] != b"ftyp" {
                return Err(MediaError::InvalidMedia("expected an MP4 file".into()));
            }
            stream.SetCurrentPosition(0).map_err(media_error)?;
            stream
                .cast::<IMFAttributes>()
                .and_then(|attributes| {
                    attributes.SetString(&MF_BYTESTREAM_CONTENT_TYPE, w!("video/mp4"))
                })
                .map_err(media_error)?;
            let error = Arc::new(AtomicU64::new(0));
            let notify: IMFMediaEngineNotify = Notify(error.clone()).into();
            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 2).map_err(media_error)?;
            let attributes = attributes.expect("Media Foundation returned attributes");
            attributes
                .SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &notify)
                .map_err(media_error)?;
            attributes
                .SetUINT32(
                    &MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT,
                    DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32,
                )
                .map_err(media_error)?;
            let factory: IMFMediaEngineClassFactory =
                CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER)
                    .map_err(media_error)?;
            let engine = factory
                .CreateInstance(MF_MEDIA_ENGINE_DISABLE_LOCAL_PLUGINS.0 as u32, &attributes)
                .map_err(media_error)?;
            let player = Self {
                engine,
                _stream: stream,
                error,
            };
            player.engine.SetPreload(preload).map_err(media_error)?;
            player
                .engine
                .cast::<IMFMediaEngineEx>()
                .and_then(|extension| {
                    extension.SetSourceFromByteStream(&player._stream, &BSTR::from("video.mp4"))
                })
                .map_err(media_error)?;
            Ok(player)
        }
    }

    fn check_error(&self) -> Result<(), MediaError> {
        let error = self.error.load(Ordering::Acquire);
        if error != 0 {
            let kind = error >> 32;
            let detail = error as u32;
            return Err(MediaError::InvalidMedia(format!(
                "Windows playback error {kind} (0x{detail:08X})"
            )));
        }
        Ok(())
    }

    fn metadata(
        &self,
        limits: &MediaLimits,
        cancel: &AtomicBool,
    ) -> Result<MediaMetadata, MediaError> {
        wait_until(cancel, limits.probe_timeout, "MP4 metadata", || {
            self.check_error()?;
            // The media engine is only used on the thread which created it.
            unsafe {
                if self.engine.GetReadyState() < MF_MEDIA_ENGINE_READY_HAVE_METADATA.0 as u16 {
                    return Ok(None);
                }
                if !self.engine.HasVideo().as_bool() {
                    return Err(MediaError::InvalidMedia("MP4 has no video stream".into()));
                }
                let duration = self.engine.GetDuration();
                if !duration.is_finite()
                    || duration <= 0.
                    || duration > limits.max_duration.as_secs_f64()
                {
                    return Err(MediaError::ResourceLimit(
                        "duration is invalid or exceeds configured limit".into(),
                    ));
                }
                let [width, height] = self.coded_dimensions()?;
                if width == 0
                    || height == 0
                    || u64::from(width) * u64::from(height) > limits.max_source_pixels
                {
                    return Err(MediaError::ResourceLimit(
                        "video dimensions are missing or too large".into(),
                    ));
                }
                Ok(Some(MediaMetadata {
                    duration,
                    width: Some(width),
                    height: Some(height),
                    has_video: true,
                    has_audio: self.engine.HasAudio().as_bool(),
                    audio_sample_rate: None,
                    audio_channels: None,
                    format: "mov,mp4".into(),
                }))
            }
        })
    }

    fn coded_dimensions(&self) -> Result<[u32; 2], MediaError> {
        // Project manifests have always stored coded dimensions. Media
        // Engine's display size accounts for rotation metadata instead.
        unsafe {
            let extension = self
                .engine
                .cast::<IMFMediaEngineEx>()
                .map_err(media_error)?;
            let count = extension.GetNumberOfStreams().map_err(media_error)?;
            if count > 64 {
                return Err(MediaError::ResourceLimit("more than 64 streams".into()));
            }
            for index in 0..count {
                let packed = match stream_attribute(
                    &extension,
                    index,
                    &MF_MT_FRAME_SIZE,
                    PropVariantToUInt64,
                ) {
                    Ok(packed) => packed,
                    Err(error) if error.code() == MF_E_ATTRIBUTENOTFOUND => continue,
                    Err(error) => return Err(media_error(error)),
                };
                let subtype =
                    stream_attribute(&extension, index, &MF_MT_SUBTYPE, PropVariantToGUID)
                        .map_err(media_error)?;
                if subtype == MFVideoFormat_H264 || subtype == MFVideoFormat_H264_ES {
                    let profile = stream_attribute(
                        &extension,
                        index,
                        &MF_MT_MPEG2_PROFILE,
                        PropVariantToUInt32,
                    )
                    .map_err(media_error)?;
                    // Media Foundation can report incorrect dimensions from
                    // unsupported streams, for example High 4:4:4 Predictive.
                    if ![
                        eAVEncH264VProfile_Base.0 as u32,
                        eAVEncH264VProfile_ConstrainedBase.0 as u32,
                        eAVEncH264VProfile_Main.0 as u32,
                        eAVEncH264VProfile_High.0 as u32,
                        eAVEncH264VProfile_UCConstrainedHigh.0 as u32,
                    ]
                    .contains(&profile)
                    {
                        return Err(MediaError::InvalidMedia(format!(
                            "H.264 profile {profile} requires the FFmpeg backend"
                        )));
                    }
                }
                return Ok([(packed >> 32) as u32, packed as u32]);
            }
            Err(MediaError::InvalidMedia(
                "no coded MP4 frame dimensions".into(),
            ))
        }
    }
}

fn stream_attribute<T>(
    engine: &IMFMediaEngineEx,
    index: u32,
    name: &GUID,
    decode: unsafe fn(*const PROPVARIANT) -> windows::core::Result<T>,
) -> windows::core::Result<T> {
    unsafe {
        let mut attribute = engine.GetStreamAttribute(index, name)?;
        let value = decode(&attribute);
        PropVariantClear(&mut attribute)?;
        value
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // Shutdown releases the source and stops the native playback and
        // callbacks before their COM apartment is torn down.
        let _ = unsafe { self.engine.Shutdown() };
    }
}

fn wait_until<T>(
    cancel: &AtomicBool,
    timeout: Duration,
    operation: &str,
    mut sample: impl FnMut() -> Result<Option<T>, MediaError>,
) -> Result<T, MediaError> {
    let started = Instant::now();
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(MediaError::Cancelled);
        }
        if let Some(value) = sample()? {
            return Ok(value);
        }
        if started.elapsed() >= timeout {
            return Err(MediaError::Timeout(operation.into()));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

struct Bitmap {
    image: IWICBitmap,
    width: u32,
    height: u32,
}

impl Bitmap {
    fn new(player: &Player, options: &PlaybackOptions) -> Result<Self, MediaError> {
        // The bitmap stays in the player's COM apartment and fits the
        // validated preview dimensions.
        unsafe {
            let (mut width, mut height) = (0, 0);
            player
                .engine
                .GetNativeVideoSize(Some(&mut width), Some(&mut height))
                .map_err(media_error)?;
            if width == 0 || height == 0 {
                return Err(MediaError::InvalidMedia(
                    "MP4 display dimensions are missing".into(),
                ));
            }
            let scale = (f64::from(options.max_width) / f64::from(width))
                .min(f64::from(options.max_height) / f64::from(height))
                .min(1.);
            let width = (f64::from(width) * scale).round().max(1.) as u32;
            let height = (f64::from(height) * scale).round().max(1.) as u32;
            let factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                    .map_err(media_error)?;
            let image = factory
                .CreateBitmap(
                    width,
                    height,
                    &GUID_WICPixelFormat32bppBGRA,
                    WICBitmapCacheOnLoad,
                )
                .map_err(media_error)?;
            Ok(Self {
                image,
                width,
                height,
            })
        }
    }

    fn frame(&self, player: &Player, sequence: u64) -> Result<Option<VideoFrame>, MediaError> {
        player.check_error()?;
        let engine = &player.engine;
        // S_FALSE means that no new frame is available. The generated
        // convenience method erases that distinction from S_OK.
        unsafe {
            if engine.GetReadyState() < MF_MEDIA_ENGINE_READY_HAVE_CURRENT_DATA.0 as u16 {
                return Ok(None);
            }
            let mut pts = 0;
            let status = (engine.vtable().OnVideoStreamTick)(engine.as_raw(), &mut pts);
            if status == S_FALSE {
                return Ok(None);
            }
            status.ok().map_err(media_error)?;
            let stride = self.width * 4;
            let mut bgra = vec![0; (stride * self.height) as usize];
            let rect = RECT {
                left: 0,
                top: 0,
                right: self.width as i32,
                bottom: self.height as i32,
            };
            engine
                .TransferVideoFrame(&self.image, None, &rect, None)
                .map_err(media_error)?;
            self.image
                .CopyPixels(ptr::null(), stride, &mut bgra)
                .map_err(media_error)?;
            Ok(Some(VideoFrame {
                width: self.width,
                height: self.height,
                bgra: bgra.into(),
                position: (pts as f64 / 10_000_000.).max(0.),
                sequence,
            }))
        }
    }
}

fn run_native(
    path: &Path,
    options: &PlaybackOptions,
    limits: &MediaLimits,
    commands: &Receiver<Command>,
    shared: &Mutex<PlaybackSnapshot>,
    cancel: &AtomicBool,
) -> Result<(), MediaError> {
    let _foundation = MediaFoundation::start()?;
    let player = Player::open(path, limits, MF_MEDIA_ENGINE_PRELOAD_AUTOMATIC)?;
    let metadata = player.metadata(limits, cancel)?;
    let bitmap = Bitmap::new(&player, options)?;
    let mut snapshot = PlaybackSnapshot::loading(options);
    snapshot.frame = Some(wait_until(
        cancel,
        limits.decode_stall_timeout,
        "first MP4 frame",
        || bitmap.frame(&player, 1),
    )?);
    snapshot.state = PlaybackState::Ready;
    let duration = metadata.duration;
    snapshot.metadata = Some(metadata);
    unsafe {
        player
            .engine
            .SetVolume(f64::from(options.volume))
            .map_err(media_error)?;
        player
            .engine
            .SetMuted(options.audio == AudioOutput::Disabled)
            .map_err(media_error)?;
    }
    let interval = Duration::from_secs_f64(1. / f64::from(options.frames_per_second));
    let mut playback = Playback {
        bitmap,
        player,
        snapshot,
        interval,
        duration,
        stall_timeout: limits.decode_stall_timeout,
        active_seek: None,
        queued_seek: None,
    };
    if options.autoplay {
        playback.command(Command::Play)?;
    }
    publish(&mut playback.snapshot, shared);
    let result = playback.run(commands, shared, cancel);
    playback.snapshot.finish(result);
    publish(&mut playback.snapshot, shared);
    Ok(())
}

struct Playback {
    bitmap: Bitmap,
    player: Player,
    snapshot: PlaybackSnapshot,
    interval: Duration,
    duration: f64,
    stall_timeout: Duration,
    active_seek: Option<f64>,
    queued_seek: Option<f64>,
}

impl Playback {
    fn run(
        &mut self,
        commands: &Receiver<Command>,
        shared: &Mutex<PlaybackSnapshot>,
        cancel: &AtomicBool,
    ) -> Result<(), MediaError> {
        let mut last_poll = Instant::now();
        let mut last_progress = last_poll;
        let mut sequence = 1;
        while !cancel.load(Ordering::Acquire) {
            match commands.recv_timeout(self.interval.saturating_sub(last_poll.elapsed())) {
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(command) => {
                    self.command(command)?;
                    publish(&mut self.snapshot, shared);
                    last_progress = Instant::now();
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            if last_poll.elapsed() < self.interval {
                continue;
            }
            last_poll = Instant::now();
            self.player.check_error()?;
            let playing = self.snapshot.state == PlaybackState::Playing;
            if !playing && self.active_seek.is_none() {
                continue;
            }
            let mut updated = false;
            let engine = &self.player.engine;
            // Media Engine also has to be polled while paused and seeking. A
            // seek can remain pending until its target frame has been consumed.
            if self.active_seek.is_none() && !unsafe { engine.IsSeeking().as_bool() } {
                let position = unsafe { engine.GetCurrentTime() }.clamp(0., self.duration);
                if position.is_finite() && position != self.snapshot.position {
                    self.snapshot.position = position;
                    updated = true;
                }
            }
            if let Some(frame) = self.bitmap.frame(&self.player, sequence + 1)? {
                self.active_seek = None;
                if let Some(target) = self.queued_seek.take() {
                    // Consume the previous seek's frame before issuing the
                    // newer request. Publishing it would expose stale pixels.
                    self.seek(target)?;
                    last_progress = last_poll;
                    continue;
                }
                if matches!(
                    self.snapshot.state,
                    PlaybackState::Playing | PlaybackState::Paused
                ) {
                    sequence = frame.sequence;
                    self.snapshot.frame = Some(frame);
                    updated = true;
                }
            }
            if playing && self.active_seek.is_none() && unsafe { engine.IsEnded().as_bool() } {
                self.snapshot.position = self.duration;
                self.snapshot.state = PlaybackState::Ended;
                updated = true;
            }
            if updated {
                last_progress = last_poll;
                publish(&mut self.snapshot, shared);
            } else if last_progress.elapsed() >= self.stall_timeout {
                return Err(MediaError::Timeout(
                    "playback stopped providing MP4 frames".into(),
                ));
            }
        }
        Ok(())
    }

    fn seek(&mut self, position: f64) -> Result<(), MediaError> {
        self.snapshot.position = position;
        self.snapshot.frame = None;
        if let Some(active) = self.active_seek {
            self.queued_seek = (position != active).then_some(position);
        } else {
            // The worker owns the engine and permits one system seek at a time.
            unsafe { self.player.engine.SetCurrentTime(position) }.map_err(media_error)?;
            self.active_seek = Some(position);
        }
        Ok(())
    }

    fn command(&mut self, command: Command) -> Result<(), MediaError> {
        let duration = self.duration;
        // These commands are executed only on the worker that owns the engine.
        unsafe {
            match command {
                Command::Play => {
                    if matches!(
                        self.snapshot.state,
                        PlaybackState::Stopped | PlaybackState::Ended
                    ) {
                        self.seek(0.)?;
                    }
                    self.player.engine.Play().map_err(media_error)?;
                    self.snapshot.state = PlaybackState::Playing;
                }
                Command::Pause => {
                    if self.snapshot.state == PlaybackState::Playing {
                        self.player.engine.Pause().map_err(media_error)?;
                        if self.active_seek.is_none() {
                            self.snapshot.position =
                                self.player.engine.GetCurrentTime().clamp(0., duration);
                        }
                        self.snapshot.state = PlaybackState::Paused;
                    }
                }
                Command::Stop => {
                    self.player.engine.Pause().map_err(media_error)?;
                    self.queued_seek = None;
                    self.snapshot.position = 0.;
                    self.snapshot.frame = None;
                    self.snapshot.state = PlaybackState::Stopped;
                }
                Command::Seek(seconds) => {
                    let position = seconds.min(duration);
                    if position >= duration {
                        self.player.engine.Pause().map_err(media_error)?;
                        self.queued_seek = None;
                        self.snapshot.position = position;
                        self.snapshot.frame = None;
                        self.snapshot.state = PlaybackState::Ended;
                    } else if self.active_seek.is_some()
                        || position != self.snapshot.position
                        || self.snapshot.frame.is_none()
                    {
                        if self.snapshot.state != PlaybackState::Playing {
                            self.snapshot.state = PlaybackState::Paused;
                        }
                        self.seek(position)?;
                    }
                }
                Command::Volume(volume) => {
                    self.player
                        .engine
                        .SetVolume(f64::from(volume))
                        .map_err(media_error)?;
                    self.snapshot.volume = volume;
                }
                Command::Shutdown => {}
            }
        }
        Ok(())
    }
}

impl PlaybackSnapshot {
    fn finish(&mut self, result: Result<(), MediaError>) {
        match result {
            Ok(()) | Err(MediaError::Cancelled) => {
                self.state = PlaybackState::Stopped;
                self.frame = None;
            }
            Err(error) => {
                self.state = PlaybackState::Error;
                self.error = Some(error);
            }
        }
    }
}

fn publish(snapshot: &mut PlaybackSnapshot, shared: &Mutex<PlaybackSnapshot>) {
    snapshot.revision += 1;
    *shared.lock().unwrap_or_else(|error| error.into_inner()) = snapshot.clone();
}

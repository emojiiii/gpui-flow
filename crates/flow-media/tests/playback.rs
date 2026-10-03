use gpui_flow_media::{
    AudioOutput, FfmpegBackend, MediaBackend, MediaController, MediaError, PlaybackOptions,
    PlaybackSnapshot, PlaybackState,
};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Fixture {
    directory: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "gpui-flow-media-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();
        Self { directory }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }
    fn wav(&self) -> PathBuf {
        let sample_rate: u32 = 8000;
        let count = sample_rate * 3;
        let data_bytes = count * 2;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_bytes.to_le_bytes());
        for i in 0..count {
            let sample = ((i as f32 * 440. * std::f32::consts::TAU / sample_rate as f32).sin()
                * 1000.) as i16;
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        let path = self.path("sine with spaces.wav");
        fs::write(&path, bytes).unwrap();
        path
    }
    fn video(&self) -> PathBuf {
        let path = self.path("test pattern with spaces.mkv");
        let output = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=96x64:rate=12",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=16000",
                "-t",
                "3",
                "-c:v",
                "ffv1",
                "-threads",
                "1",
                "-c:a",
                "pcm_s16le",
                "-y",
            ])
            .arg(&path)
            .output()
            .expect("FFmpeg must be installed for ignored integration tests");
        assert!(
            output.status.success(),
            "fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
fn wait(
    player: &Arc<dyn MediaController>,
    condition: impl Fn(&PlaybackSnapshot) -> bool,
) -> PlaybackSnapshot {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let state = player.snapshot();
        assert_ne!(state.state, PlaybackState::Error, "{:?}", state.error);
        if condition(&state) {
            return state;
        }
        assert!(
            Instant::now() < deadline,
            "timed out in {:?} at {}",
            state.state,
            state.position
        );
        thread::sleep(Duration::from_millis(15));
    }
}
#[test]
fn rejects_network_directory_empty_and_invalid_options_before_spawning() {
    let backend = FfmpegBackend::default();
    assert!(matches!(
        backend.open(
            std::path::Path::new("https://example.com/video.mp4"),
            PlaybackOptions::default()
        ),
        Err(MediaError::InvalidSource(_))
    ));
    let fixture = Fixture::new();
    assert!(
        backend
            .open(&fixture.directory, PlaybackOptions::default())
            .is_err()
    );
    let empty = fixture.path("empty.wav");
    fs::write(&empty, []).unwrap();
    assert!(matches!(
        backend.open(&empty, PlaybackOptions::default()),
        Err(MediaError::ResourceLimit(_))
    ));
    let options = PlaybackOptions {
        frames_per_second: 1000,
        ..Default::default()
    };
    assert!(backend.open(&fixture.wav(), options).is_err());
}

#[test]
#[ignore = "requires ffmpeg/ffprobe; validates pooled real decoder release without audio"]
fn pooled_video_has_generational_events_and_reaps_before_releasing_budget() {
    use gpui_flow_media::{LoadKind, LoadUsage, ResourceLoadPool, ResourceLoadStatus};
    let fixture = Fixture::new();
    let source = fixture.video();
    let pool = ResourceLoadPool::default();
    let backend = FfmpegBackend::default().with_load_pool(pool.clone());
    let options = PlaybackOptions {
        audio: AudioOutput::Disabled,
        autoplay: true,
        max_width: 96,
        max_height: 64,
        ..Default::default()
    };
    let player = backend.open(&source, options.clone()).unwrap();
    let first = player.resource_event().unwrap().ticket;
    assert_eq!(first.kind(), LoadKind::Media);
    let ready = wait(&player, |s| s.frame.is_some());
    assert!(!ready.process_ids.is_empty());
    assert_eq!(
        player.resource_event().unwrap().status,
        ResourceLoadStatus::Ready
    );
    assert_eq!(pool.usage().in_flight, 1);
    assert!(pool.usage().reserved_bytes > 0);
    player.shutdown();
    assert!(player.snapshot().process_ids.is_empty());
    #[cfg(target_os = "linux")]
    for pid in ready.process_ids {
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    }
    assert_eq!(pool.usage(), LoadUsage::default());
    assert_eq!(
        player.resource_event().unwrap().status,
        ResourceLoadStatus::Cancelled
    );
    let second = backend.open(&source, options).unwrap();
    assert_ne!(first, second.resource_event().unwrap().ticket);
    drop(second);
    assert_eq!(pool.usage(), LoadUsage::default());
}
#[test]
fn rejects_missing_source_and_caps_input_size() {
    let fixture = Fixture::new();
    let mut backend = FfmpegBackend::default();
    assert!(
        backend
            .open(&fixture.path("missing.wav"), PlaybackOptions::default())
            .is_err()
    );
    backend.limits.max_input_bytes = 10;
    assert!(matches!(
        backend.open(&fixture.wav(), PlaybackOptions::default()),
        Err(MediaError::ResourceLimit(_))
    ));
}
#[test]
#[ignore = "requires host-installed ffmpeg and ffprobe; no audio device"]
fn video_decode_pause_seek_restart_end_and_drop() {
    let fixture = Fixture::new();
    let path = fixture.video();
    let player = FfmpegBackend::default()
        .open(
            &path,
            PlaybackOptions {
                audio: AudioOutput::Disabled,
                frames_per_second: 12,
                max_width: 96,
                max_height: 64,
                ..Default::default()
            },
        )
        .unwrap();
    let ready = wait(&player, |s| {
        s.state == PlaybackState::Ready && s.frame.is_some()
    });
    assert!(ready.metadata.as_ref().unwrap().has_audio);
    let first = ready.frame.unwrap();
    assert_eq!(first.bgra.len(), 96 * 64 * 4);
    assert!(first.bgra.as_chunks::<4>().0.iter().any(|p| p[0] != p[2]));
    player.play().unwrap();
    wait(&player, |s| {
        s.state == PlaybackState::Playing
            && s.position > 0.3
            && s.frame
                .as_ref()
                .is_some_and(|f| f.sequence > first.sequence)
    });
    let process_ids = player.snapshot().process_ids;
    assert!(!process_ids.is_empty());
    player.pause().unwrap();
    let paused = wait(&player, |s| s.state == PlaybackState::Paused);
    assert!(paused.process_ids.is_empty());
    #[cfg(target_os = "linux")]
    for pid in process_ids {
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "process {pid} survived pause"
        );
    }
    thread::sleep(Duration::from_millis(120));
    assert_eq!(player.snapshot().position, paused.position);
    player.seek(1.5).unwrap();
    let sought = wait(&player, |s| {
        s.state == PlaybackState::Paused
            && s.frame
                .as_ref()
                .is_some_and(|f| (f.position - 1.5).abs() < 0.01)
    });
    assert!((sought.position - 1.5).abs() < 0.01);
    assert!(player.seek(f64::NAN).is_err());
    assert!(player.set_volume(2.).is_err());
    player.play().unwrap();
    wait(&player, |s| {
        s.state == PlaybackState::Playing && s.position > 1.7
    });
    player.stop().unwrap();
    let stopped = wait(&player, |s| s.state == PlaybackState::Stopped);
    assert_eq!(stopped.position, 0.);
    assert!(stopped.frame.is_none());
    player.seek(2.8).unwrap();
    player.play().unwrap();
    wait(&player, |s| s.state == PlaybackState::Ended);
    player.play().unwrap();
    wait(&player, |s| {
        s.state == PlaybackState::Playing && s.position < 1.
    });
    let start = Instant::now();
    player.shutdown();
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(player.snapshot().state, PlaybackState::Stopped);
    assert!(matches!(player.play(), Err(MediaError::PlayerClosed)));
}
#[test]
#[ignore = "requires ffprobe and ffplay; use SDL_AUDIODRIVER=dummy for headless output-path test"]
fn audio_output_process_plays_pauses_seeks_and_releases() {
    let fixture = Fixture::new();
    let player = FfmpegBackend::default()
        .open(&fixture.wav(), PlaybackOptions::default())
        .unwrap();
    let ready = wait(&player, |s| s.state == PlaybackState::Ready);
    assert!(ready.metadata.unwrap().has_audio);
    assert_eq!(ready.volume, 0.25);
    player.play().unwrap();
    wait(&player, |s| {
        s.state == PlaybackState::Playing && s.position > 0.5
    });
    let process_ids = player.snapshot().process_ids;
    assert!(!process_ids.is_empty());
    player.pause().unwrap();
    let paused = wait(&player, |s| s.state == PlaybackState::Paused);
    assert!(paused.process_ids.is_empty());
    #[cfg(target_os = "linux")]
    for pid in process_ids {
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "process {pid} survived pause"
        );
    }
    assert!(paused.position > 0.5);
    player.seek(1.).unwrap();
    player.set_volume(0.1).unwrap();
    player.play().unwrap();
    wait(&player, |s| {
        s.state == PlaybackState::Playing && s.position > 1.2
    });
    let start = Instant::now();
    drop(player);
    assert!(start.elapsed() < Duration::from_secs(2));
}
#[test]
#[ignore = "requires ffprobe"]
fn corrupt_input_reports_error_and_loading_drop_is_bounded() {
    let fixture = Fixture::new();
    let path = fixture.path("corrupt.mp4");
    fs::write(&path, b"this is not a media file").unwrap();
    let player = FfmpegBackend::default()
        .open(&path, PlaybackOptions::default())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while player.snapshot().state != PlaybackState::Error {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert!(player.snapshot().error.is_some());
    let start = Instant::now();
    drop(player);
    assert!(start.elapsed() < Duration::from_secs(2));
    let player = FfmpegBackend::default()
        .open(&fixture.wav(), PlaybackOptions::default())
        .unwrap();
    let start = Instant::now();
    drop(player);
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
#[ignore = "requires ffmpeg/ffprobe/ffplay; SDL_AUDIODRIVER=dummy is suitable"]
fn shorter_audio_stream_does_not_stop_longer_video() {
    let fixture = Fixture::new();
    let path = fixture.path("unequal-durations.mkv");
    let output = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=96x64:rate=12:duration=2.5",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=16000:duration=0.4",
            "-c:v",
            "ffv1",
            "-threads",
            "1",
            "-c:a",
            "pcm_s16le",
            "-y",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let player = FfmpegBackend::default()
        .open(&path, PlaybackOptions::default())
        .unwrap();
    wait(&player, |s| {
        s.state == PlaybackState::Ready && s.frame.is_some()
    });
    player.play().unwrap();
    let after_audio = wait(&player, |s| {
        s.state == PlaybackState::Playing && s.position > 1.5
    });
    assert!(
        after_audio
            .frame
            .as_ref()
            .is_some_and(|frame| frame.position > 1.)
    );
    let ended = wait(&player, |s| s.state == PlaybackState::Ended);
    assert!(ended.position >= 2.4);
    assert!(ended.error.is_none());
}

#[test]
#[ignore = "requires ffprobe; deliberately missing ffplay must surface an error"]
fn missing_audio_executable_is_an_explicit_playback_error() {
    let fixture = Fixture::new();
    let mut backend = FfmpegBackend::default();
    backend.tools.ffplay = fixture.path("missing-ffplay-executable");
    let player = backend
        .open(&fixture.wav(), PlaybackOptions::default())
        .unwrap();
    wait(&player, |s| s.state == PlaybackState::Ready);
    player.play().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let state = player.snapshot();
        if state.state == PlaybackState::Error {
            assert!(matches!(state.error, Some(MediaError::ToolUnavailable(_))));
            assert!(state.process_ids.is_empty());
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires ffmpeg/ffprobe; repeated commands must not leak or revive stale decoders"]
fn rapid_seek_pause_and_shutdown_discards_stale_frames() {
    let fixture = Fixture::new();
    let player = FfmpegBackend::default()
        .open(
            &fixture.video(),
            PlaybackOptions {
                audio: AudioOutput::Disabled,
                ..Default::default()
            },
        )
        .unwrap();
    wait(&player, |s| {
        s.state == PlaybackState::Ready && s.frame.is_some()
    });
    player.play().unwrap();
    wait(&player, |s| {
        s.state == PlaybackState::Playing && s.position > 0.1
    });
    for target in [0.8, 0.2, 1.4, 0.5, 2.0, 1.1] {
        player.seek(target).unwrap();
    }
    player.pause().unwrap();
    wait(&player, |s| s.state == PlaybackState::Paused);
    player.seek(0.75).unwrap();
    let settled = wait(&player, |s| {
        s.state == PlaybackState::Paused
            && s.frame
                .as_ref()
                .is_some_and(|f| (f.position - 0.75).abs() < 0.001)
    });
    let sequence = settled.frame.unwrap().sequence;
    thread::sleep(Duration::from_millis(100));
    let same = player.snapshot();
    assert_eq!(same.position, 0.75);
    assert_eq!(same.frame.unwrap().sequence, sequence);
    for target in [0.1, 0.3, 0.6, 1.2] {
        player.seek(target).unwrap();
    }
    let start = Instant::now();
    player.shutdown();
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(player.snapshot().process_ids.is_empty());
}

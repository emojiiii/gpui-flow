#![cfg(feature = "image-loader-worker")]
use gpui_flow_media::*;
use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    task::{Context, Poll, Waker},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "flow-owned-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn png(&self, name: &str, size: u32) -> PathBuf {
        let path = self.0.join(name);
        image::RgbaImage::from_fn(size, size, |x, y| {
            image::Rgba([
                (x % 251) as u8,
                (y % 241) as u8,
                73,
                if x % 3 == 0 { 128 } else { 255 },
            ])
        })
        .save(&path)
        .unwrap();
        path
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn loader(pool: ResourceLoadPool) -> OwnedImageLoader {
    OwnedImageLoader::new(
        pool,
        PathBuf::from(env!("CARGO_BIN_EXE_flow-image-worker")),
        ImageLoadLimits {
            max_input_bytes: 8 * 1024 * 1024,
            max_decoded_bytes: 8 * 1024 * 1024,
            ..Default::default()
        },
    )
    .unwrap()
}
fn wait(job: &ImageLoad) -> ImageLoadState {
    let start = Instant::now();
    loop {
        let s = job.state();
        if !matches!(s, ImageLoadState::Pending { .. }) {
            return s;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn real_decode_is_shared_and_result_ownership_retains_budget() {
    let files = Files::new();
    let path = files.png("small.png", 16);
    let pool = ResourceLoadPool::default();
    let loader = loader(pool.clone());
    let a = loader.load(&path).unwrap();
    let b = loader.load(&path).unwrap();
    assert_eq!(a.ticket(), b.ticket());
    assert_eq!(pool.usage().leases, 1);
    let old = a.ticket();
    drop(a);
    let ImageLoadState::Loaded(image) = wait(&b) else {
        panic!("valid PNG did not decode")
    };
    assert_eq!(image.frames.len(), 1);
    assert_eq!(&image.frames[0].rgba[..8], &[0, 0, 73, 128, 1, 0, 73, 255]);
    assert_eq!(b.resource_event().status, ResourceLoadStatus::Ready);
    assert_eq!(pool.usage().in_flight, 0);
    drop(b);
    assert!(
        pool.usage().reserved_bytes > 0,
        "delivered pixels still own their reservation"
    );
    drop(image);
    assert_eq!(pool.usage(), LoadUsage::default());
    let new = loader.load(&path).unwrap();
    assert_ne!(old, new.ticket());
    drop(new);
    assert_eq!(pool.usage(), LoadUsage::default());
}

#[test]
fn explicit_reload_does_not_replace_another_consumers_immutable_result() {
    let files = Files::new();
    let path = files.png("reload.png", 16);
    let pool = ResourceLoadPool::default();
    let loader = loader(pool.clone());
    let old = loader.load(&path).unwrap();
    let ImageLoadState::Loaded(original) = wait(&old) else {
        panic!("initial image")
    };
    image::RgbaImage::from_pixel(16, 16, image::Rgba([31, 73, 211, 255]))
        .save(&path)
        .unwrap();
    loader.invalidate();
    let fresh = loader.load(&path).unwrap();
    assert_ne!(old.ticket(), fresh.ticket());
    let ImageLoadState::Loaded(updated) = wait(&fresh) else {
        panic!("updated image")
    };
    assert_eq!(&original.frames[0].rgba[..4], &[0, 0, 73, 128]);
    assert_eq!(&updated.frames[0].rgba[..4], &[31, 73, 211, 255]);
    assert_eq!(old.resource_event().status, ResourceLoadStatus::Ready);
    drop((old, fresh, original, updated));
    assert_eq!(pool.usage(), LoadUsage::default());
}

#[test]
fn animated_frames_keep_delays_and_late_frame_overflow_rejects_whole_result() {
    let files = Files::new();
    let path = files.0.join("frames.gif");
    {
        let mut encoder =
            image::codecs::gif::GifEncoder::new(std::fs::File::create(&path).unwrap());
        for (color, delay) in [([255, 0, 0, 255], 40), ([0, 0, 255, 255], 90)] {
            encoder
                .encode_frame(image::Frame::from_parts(
                    image::RgbaImage::from_pixel(16, 16, image::Rgba(color)),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(delay, 1),
                ))
                .unwrap();
        }
    }
    let pool = ResourceLoadPool::default();
    let loader = loader(pool.clone());
    let job = loader.load(&path).unwrap();
    let ImageLoadState::Loaded(image) = wait(&job) else {
        panic!("GIF decode")
    };
    assert_eq!(image.frames.len(), 2);
    assert_eq!(
        image.frames[1].delay_numerator_ms / image.frames[1].delay_denominator,
        90
    );
    assert_eq!(&image.frames[1].rgba[..4], &[0, 0, 255, 255]);
    drop((image, job));
    let tight = OwnedImageLoader::new(
        pool.clone(),
        PathBuf::from(env!("CARGO_BIN_EXE_flow-image-worker")),
        ImageLoadLimits {
            max_decoded_bytes: 1024,
            max_frames: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let job = tight.load(&path).unwrap();
    assert!(matches!(wait(&job), ImageLoadState::Failed(_)));
    drop(job);
    assert_eq!(pool.usage(), LoadUsage::default());
}
#[test]
fn corrupt_oversized_and_missing_sources_do_not_leak_admission() {
    let files = Files::new();
    let bad = files.0.join("bad.png");
    std::fs::write(&bad, b"not an image").unwrap();
    let pool = ResourceLoadPool::default();
    let loader = loader(pool.clone());
    let job = loader.load(&bad).unwrap();
    assert!(matches!(
        wait(&job),
        ImageLoadState::Failed(MediaError::Process(_))
    ));
    assert!(matches!(
        job.resource_event().status,
        ResourceLoadStatus::Failed(_)
    ));
    drop(job);
    assert_eq!(pool.usage(), LoadUsage::default());
    assert!(loader.load(&files.0.join("missing.png")).is_err());
    let huge = files.0.join("oversized.png");
    let f = std::fs::File::create(&huge).unwrap();
    f.set_len(9 * 1024 * 1024).unwrap();
    assert!(matches!(
        loader.load(&huge),
        Err(MediaError::ResourceLimit(_))
    ));
    assert_eq!(pool.usage(), LoadUsage::default());
}
#[test]
#[cfg(target_os = "linux")]
fn last_consumer_kills_stopped_real_decoder_and_releases_future() {
    let files = Files::new();
    let path = files.png("large.png", 1024);
    let pool = ResourceLoadPool::new(LoadLimits {
        max_bytes: 64 * 1024 * 1024,
        max_jobs: 1,
    })
    .unwrap();
    let loader = loader(pool.clone());
    let job = loader.load(&path).unwrap();
    let other = job.clone();
    let mut future = job.completion();
    let start = Instant::now();
    let pid = loop {
        match job.state() {
            ImageLoadState::Pending { process_id } if process_id != 0 => break process_id,
            ImageLoadState::Pending { .. } => {}
            _ => panic!("decoder completed before suspension"),
        };
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::yield_now();
    };
    // This is our just-started child, not an unrelated application. SIGSTOP
    // proves cancellation can kill a decoder which cannot cooperate or finish.
    assert!(
        std::process::Command::new("kill")
            .args(["-STOP", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    let stop_deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
        let state = status
            .lines()
            .find(|line| line.starts_with("State:"))
            .unwrap();
        if state.contains('T') {
            break;
        }
        assert!(
            Instant::now() < stop_deadline,
            "child did not enter stopped state: {state}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        matches!(
            FfmpegBackend::default().with_load_pool(pool.clone()).open(
                &path,
                PlaybackOptions {
                    audio: AudioOutput::Disabled,
                    max_width: 16,
                    max_height: 16,
                    ..Default::default()
                }
            ),
            Err(MediaError::ResourceLimit(_))
        ),
        "media shares the image job slot"
    );
    drop(job);
    assert!(
        std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "another consumer still owns the job"
    );
    let start = Instant::now();
    drop(other);
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    assert!(matches!(
        Pin::new(&mut future).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Err(MediaError::Cancelled))
    ));
    drop(future);
    assert_eq!(pool.usage(), LoadUsage::default());
}
#[test]
fn budget_rejection_and_media_terminal_errors_keep_identity() {
    let files = Files::new();
    let path = files.png("image.png", 16);
    let pool = ResourceLoadPool::new(LoadLimits {
        max_bytes: 1024,
        max_jobs: 1,
    })
    .unwrap();
    assert!(matches!(
        loader(pool.clone()).load(&path),
        Err(MediaError::ResourceLimit(_))
    ));
    assert_eq!(pool.usage(), LoadUsage::default());
    let pool = ResourceLoadPool::default();
    let backend = FfmpegBackend {
        tools: FfmpegTools {
            ffprobe: files.0.join("no-probe"),
            ..Default::default()
        },
        ..Default::default()
    }
    .with_load_pool(pool.clone());
    let controller = backend
        .open(
            &path,
            PlaybackOptions {
                audio: AudioOutput::Disabled,
                max_width: 16,
                max_height: 16,
                ..Default::default()
            },
        )
        .unwrap();
    let ticket = controller.resource_event().unwrap().ticket;
    assert_eq!(ticket.kind(), LoadKind::Media);
    let start = Instant::now();
    loop {
        let event = controller.resource_event().unwrap();
        assert_eq!(event.ticket, ticket);
        if matches!(
            event.status,
            ResourceLoadStatus::Failed(MediaError::ToolUnavailable(_))
        ) {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::yield_now();
    }
    controller.shutdown();
    assert_eq!(
        controller.resource_event().unwrap().status,
        ResourceLoadStatus::Cancelled
    );
    assert_eq!(pool.usage(), LoadUsage::default());
    drop(controller);
}

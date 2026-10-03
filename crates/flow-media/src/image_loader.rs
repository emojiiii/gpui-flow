//! Owned local image jobs. The last consumer cancels, kills and joins decoding.
use crate::{
    LoadKind, LoadTicket, MediaError, ResourceLoadPool, loading::LoadReservation, process,
};
use std::{
    collections::BTreeMap,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    task::{Context, Poll, Waker},
    thread::JoinHandle,
    time::Duration,
};

const FRAME_METADATA_BYTES: u64 = 256;
#[derive(Clone, Debug)]
pub struct ImageLoadLimits {
    pub max_input_bytes: u64,
    pub max_decoded_bytes: u64,
    pub max_frames: u32,
    pub max_dimension: u32,
    pub timeout: Duration,
}
impl Default for ImageLoadLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 16 * 1024 * 1024,
            max_decoded_bytes: 16 * 1024 * 1024,
            max_frames: 120,
            max_dimension: 4096,
            timeout: Duration::from_secs(10),
        }
    }
}
impl ImageLoadLimits {
    fn validate(&self) -> Result<(), MediaError> {
        if self.max_input_bytes == 0
            || self.max_input_bytes > 1024 * 1024 * 1024
            || self.max_decoded_bytes == 0
            || self.max_decoded_bytes > 256 * 1024 * 1024
            || !(1..=4096).contains(&self.max_frames)
            || !(1..=16384).contains(&self.max_dimension)
            || self.timeout.is_zero()
            || self.timeout > Duration::from_secs(60)
        {
            return Err(MediaError::ResourceLimit(
                "invalid image worker limits".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct DecodedImageFrame {
    pub width: u32,
    pub height: u32,
    pub delay_numerator_ms: u32,
    pub delay_denominator: u32,
    pub rgba: Vec<u8>,
}
pub struct DecodedImage {
    pub frames: Vec<DecodedImageFrame>,
    _reservation: Arc<Mutex<LoadReservation>>,
}
type Outcome = Result<Arc<DecodedImage>, MediaError>;
#[derive(Clone)]
pub enum ImageLoadState {
    Pending { process_id: u32 },
    Loaded(Arc<DecodedImage>),
    Failed(MediaError),
}
struct Progress {
    outcome: Option<Outcome>,
    waiters: Vec<Weak<Mutex<Option<Waker>>>>,
}
struct Job {
    ticket: LoadTicket,
    progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
    pid: Arc<AtomicU32>,
    thread: Mutex<Option<JoinHandle<()>>>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(thread) = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = thread.join();
        }
    }
}
/// Clones share one job. Future/wakeup subscriptions do not keep it alive.
#[derive(Clone)]
pub struct ImageLoad(Arc<Job>);
impl ImageLoad {
    pub fn ticket(&self) -> LoadTicket {
        self.0.ticket
    }
    pub fn resource_event(&self) -> crate::ResourceLoadEvent {
        let status = match self.state() {
            ImageLoadState::Pending { .. } => crate::ResourceLoadStatus::Pending,
            ImageLoadState::Loaded(_) => crate::ResourceLoadStatus::Ready,
            ImageLoadState::Failed(MediaError::Cancelled) => crate::ResourceLoadStatus::Cancelled,
            ImageLoadState::Failed(e) => crate::ResourceLoadStatus::Failed(e),
        };
        crate::ResourceLoadEvent {
            ticket: self.ticket(),
            status,
        }
    }
    pub fn state(&self) -> ImageLoadState {
        match self
            .0
            .progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .outcome
            .clone()
        {
            None => ImageLoadState::Pending {
                process_id: self.0.pid.load(Ordering::Acquire),
            },
            Some(Ok(image)) => ImageLoadState::Loaded(image),
            Some(Err(error)) => ImageLoadState::Failed(error),
        }
    }
    pub fn completion(&self) -> ImageLoadCompletion {
        let slot = Arc::new(Mutex::new(None));
        let mut progress = self.0.progress.lock().unwrap_or_else(|e| e.into_inner());
        progress.waiters.retain(|w| w.strong_count() > 0);
        progress.waiters.push(Arc::downgrade(&slot));
        ImageLoadCompletion {
            progress: self.0.progress.clone(),
            slot,
        }
    }
}
pub struct ImageLoadCompletion {
    progress: Arc<Mutex<Progress>>,
    slot: Arc<Mutex<Option<Waker>>>,
}
impl Future for ImageLoadCompletion {
    type Output = Outcome;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Outcome> {
        let progress = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(result) = &progress.outcome {
            return Poll::Ready(result.clone());
        }
        *self.slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(cx.waker().clone());
        Poll::Pending
    }
}
struct Loader {
    pool: ResourceLoadPool,
    executable: PathBuf,
    limits: ImageLoadLimits,
    jobs: Mutex<BTreeMap<PathBuf, Weak<Job>>>,
}
/// Executable is trusted host configuration, never an imported document field.
/// Build `gpui-flow-media --features image-loader-worker --bin flow-image-worker`.
#[derive(Clone)]
pub struct OwnedImageLoader(Arc<Loader>);
impl OwnedImageLoader {
    pub fn new(
        pool: ResourceLoadPool,
        executable: PathBuf,
        limits: ImageLoadLimits,
    ) -> Result<Self, MediaError> {
        limits.validate()?;
        if !executable.is_absolute() {
            return Err(MediaError::InvalidSource(
                "image worker requires an absolute executable path".into(),
            ));
        }
        Ok(Self(Arc::new(Loader {
            pool,
            executable,
            limits,
            jobs: Mutex::new(BTreeMap::new()),
        })))
    }
    pub fn pool(&self) -> &ResourceLoadPool {
        &self.0.pool
    }
    /// Future acquisitions get new generations. Existing consumers keep their
    /// immutable result/job until they release it; no other view is cancelled.
    pub fn invalidate(&self) {
        self.0
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
    pub fn load(&self, source: &Path) -> Result<ImageLoad, MediaError> {
        let source = source
            .canonicalize()
            .map_err(|e| MediaError::InvalidSource(e.to_string()))?;
        let metadata = source
            .metadata()
            .map_err(|e| MediaError::InvalidSource(e.to_string()))?;
        if !metadata.is_file() || metadata.len() > self.0.limits.max_input_bytes {
            return Err(MediaError::ResourceLimit(
                "image input is not a bounded regular file".into(),
            ));
        }
        let mut jobs = self.0.jobs.lock().unwrap_or_else(|e| e.into_inner());
        jobs.retain(|_, j| j.strong_count() > 0);
        if let Some(job) = jobs.get(&source).and_then(Weak::upgrade) {
            return Ok(ImageLoad(job));
        }
        let limits = self.0.limits.clone();
        // Child encoded bytes + parent/child/output conversion buffers. Decoder
        // private allocations and GPU uploads have separate backend limits.
        let bytes = limits
            .max_input_bytes
            .checked_mul(2)
            .and_then(|n| {
                n.checked_add(
                    limits.max_decoded_bytes * 4
                        + 4096
                        + u64::from(limits.max_frames) * FRAME_METADATA_BYTES,
                )
            })
            .ok_or_else(|| MediaError::ResourceLimit("image admission overflow".into()))?;
        let reservation = Arc::new(Mutex::new(self.0.pool.reserve(LoadKind::Image, bytes)?));
        let ticket = reservation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .ticket();
        let progress = Arc::new(Mutex::new(Progress {
            outcome: None,
            waiters: vec![],
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let pid = Arc::new(AtomicU32::new(0));
        let (state, signal, process_id) = (progress.clone(), cancel.clone(), pid.clone());
        let worker = self.0.executable.clone();
        let path = source.clone();
        let thread = std::thread::Builder::new()
            .name("flow-owned-image".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut cmd = process::command(&worker);
                    cmd.arg(&path)
                        .arg(limits.max_input_bytes.to_string())
                        .arg(limits.max_decoded_bytes.to_string())
                        .arg(limits.max_frames.to_string())
                        .arg(limits.max_dimension.to_string());
                    let data = process::capture_observed(
                        &mut cmd,
                        limits.timeout,
                        &signal,
                        (limits.max_decoded_bytes + 8 + u64::from(limits.max_frames) * 16) as usize,
                        |id| process_id.store(id, Ordering::Release),
                    )?;
                    if signal.load(Ordering::Acquire) {
                        return Err(MediaError::Cancelled);
                    }
                    let frames = decode_response(&data, &limits)?;
                    let retained = frames.iter().map(|f| f.rgba.len() as u64).sum::<u64>() * 2
                        + frames.len() as u64 * 256
                        + 4096;
                    reservation
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .complete(retained)?;
                    Ok(Arc::new(DecodedImage {
                        frames,
                        _reservation: reservation.clone(),
                    }))
                }))
                .unwrap_or_else(|_| Err(MediaError::Process("image job panicked".into())));
                process_id.store(0, Ordering::Release);
                if result.is_err() {
                    let _ = reservation
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .complete(0);
                }
                let waiters = {
                    let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                    s.outcome = Some(result);
                    std::mem::take(&mut s.waiters)
                };
                for waiter in waiters {
                    let wake = waiter
                        .upgrade()
                        .and_then(|slot| slot.lock().unwrap_or_else(|e| e.into_inner()).take());
                    if let Some(wake) = wake {
                        wake.wake();
                    }
                }
            })
            .map_err(|e| MediaError::Process(e.to_string()))?;
        let job = Arc::new(Job {
            ticket,
            progress,
            cancel,
            pid,
            thread: Mutex::new(Some(thread)),
        });
        jobs.insert(source, Arc::downgrade(&job));
        Ok(ImageLoad(job))
    }
}
fn decode_response(
    data: &[u8],
    limits: &ImageLoadLimits,
) -> Result<Vec<DecodedImageFrame>, MediaError> {
    let bad = || MediaError::InvalidMedia("invalid image worker response".into());
    if data.len() < 8 || &data[..4] != b"FWI1" {
        return Err(bad());
    }
    let count = u32::from_le_bytes(data[4..8].try_into().map_err(|_| bad())?);
    if count == 0 || count > limits.max_frames {
        return Err(bad());
    }
    let mut offset = 8;
    let mut frames = vec![];
    let mut total = 0u64;
    for _ in 0..count {
        let metadata = data.get(offset..offset + 16).ok_or_else(bad)?;
        let mut ints = [0; 4];
        for (value, bytes) in ints.iter_mut().zip(metadata.as_chunks::<4>().0) {
            *value = u32::from_le_bytes(*bytes);
        }
        offset += 16;
        let (w, h) = (ints[0], ints[1]);
        let bytes = u64::from(w)
            .checked_mul(u64::from(h))
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(bad)?;
        total = total.checked_add(bytes).ok_or_else(bad)?;
        if w == 0
            || h == 0
            || w > limits.max_dimension
            || h > limits.max_dimension
            || ints[3] == 0
            || total > limits.max_decoded_bytes
            || bytes > data.len().saturating_sub(offset) as u64
        {
            return Err(bad());
        }
        frames.push(DecodedImageFrame {
            width: w,
            height: h,
            delay_numerator_ms: ints[2],
            delay_denominator: ints[3],
            rgba: data[offset..offset + bytes as usize].to_vec(),
        });
        offset += bytes as usize;
    }
    if offset != data.len() {
        return Err(bad());
    }
    Ok(frames)
}

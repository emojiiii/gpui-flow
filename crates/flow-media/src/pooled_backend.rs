use crate::{
    FfmpegBackend, LoadKind, LoadTicket, MediaBackend, MediaController, MediaError,
    PlaybackOptions, PlaybackSnapshot, PlaybackState, ResourceLoadEvent, ResourceLoadPool,
    ResourceLoadStatus, loading::LoadReservation,
};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// Shares known load/queue admission with OwnedImageLoader. Existing FFmpeg
/// source validation, process limits and synchronous shutdown remain in force.
#[derive(Clone)]
pub struct PooledMediaBackend {
    backend: FfmpegBackend,
    pool: ResourceLoadPool,
}
impl FfmpegBackend {
    pub fn with_load_pool(self, pool: ResourceLoadPool) -> PooledMediaBackend {
        PooledMediaBackend {
            backend: self,
            pool,
        }
    }
}
impl MediaBackend for PooledMediaBackend {
    fn open(
        &self,
        source: &Path,
        options: PlaybackOptions,
    ) -> Result<Arc<dyn MediaController>, MediaError> {
        options.validate()?;
        let source = crate::probe::source(source, &self.backend.limits)?;
        let input = source
            .metadata()
            .map_err(|e| MediaError::InvalidSource(e.to_string()))?
            .len();
        // Decoder ring, pending/read/current frames, UI conversion and bounded
        // probe/stderr capture. Codec-private allocations retain their own cap.
        let retained =
            u64::from(options.max_width) * u64::from(options.max_height) * 4 * 8 + 128 * 1024;
        let bytes = input
            .checked_add(retained)
            .ok_or_else(|| MediaError::ResourceLimit("media load byte overflow".into()))?;
        let reservation = self.pool.reserve(LoadKind::Media, bytes)?;
        let ticket = reservation.ticket();
        let inner = self.backend.open(&source, options)?;
        Ok(Arc::new(PooledController {
            inner,
            reservation: Mutex::new(Some(reservation)),
            retained,
            ticket,
            closed: AtomicBool::new(false),
        }))
    }
}
struct PooledController {
    inner: Arc<dyn MediaController>,
    reservation: Mutex<Option<LoadReservation>>,
    retained: u64,
    ticket: LoadTicket,
    closed: AtomicBool,
}
impl PooledController {
    fn close(&self) {
        let mut reservation = self.reservation.lock().unwrap_or_else(|e| e.into_inner());
        if !self.closed.load(Ordering::Acquire) {
            self.inner.shutdown();
            reservation.take();
            self.closed.store(true, Ordering::Release);
        }
    }
}
impl Drop for PooledController {
    fn drop(&mut self) {
        self.close();
    }
}
impl MediaController for PooledController {
    fn snapshot(&self) -> PlaybackSnapshot {
        let snapshot = self.inner.snapshot();
        if snapshot.state != PlaybackState::Loading
            && let Some(reservation) = self
                .reservation
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_mut()
        {
            let _ = reservation.reduce(self.retained);
        }
        snapshot
    }
    fn resource_event(&self) -> Option<ResourceLoadEvent> {
        let snapshot = self.snapshot();
        let status = if self.closed.load(Ordering::Acquire) {
            ResourceLoadStatus::Cancelled
        } else if let Some(error) = snapshot.error {
            ResourceLoadStatus::Failed(error)
        } else if snapshot.state == PlaybackState::Loading {
            ResourceLoadStatus::Pending
        } else {
            ResourceLoadStatus::Ready
        };
        Some(ResourceLoadEvent {
            ticket: self.ticket,
            status,
        })
    }
    fn play(&self) -> Result<(), MediaError> {
        self.inner.play()
    }
    fn pause(&self) -> Result<(), MediaError> {
        self.inner.pause()
    }
    fn stop(&self) -> Result<(), MediaError> {
        self.inner.stop()
    }
    fn seek(&self, s: f64) -> Result<(), MediaError> {
        self.inner.seek(s)
    }
    fn set_volume(&self, v: f32) -> Result<(), MediaError> {
        self.inner.set_volume(v)
    }
    fn shutdown(&self) {
        self.close();
    }
}

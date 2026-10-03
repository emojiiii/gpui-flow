//! Shared admission for owned image jobs and local media players.
use crate::MediaError;
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicU64, Ordering},
};

static NEXT_POOL: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LoadKind {
    Image,
    Media,
}

/// Identity belongs to a pool and never reuses a generation after release.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LoadTicket {
    pool: u64,
    generation: u64,
    kind: LoadKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceLoadStatus {
    Pending,
    Ready,
    Failed(MediaError),
    Cancelled,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceLoadEvent {
    pub ticket: LoadTicket,
    pub status: ResourceLoadStatus,
}
impl LoadTicket {
    pub fn generation(self) -> u64 {
        self.generation
    }
    pub fn kind(self) -> LoadKind {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadLimits {
    pub max_bytes: u64,
    pub max_jobs: usize,
}
impl Default for LoadLimits {
    fn default() -> Self {
        Self {
            max_bytes: 256 * 1024 * 1024,
            max_jobs: 8,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadUsage {
    pub reserved_bytes: u64,
    pub in_flight: usize,
    pub leases: usize,
}
struct Retired {
    alive: Box<dyn Fn() -> bool + Send>,
    _owner: Box<dyn std::any::Any + Send>,
}
struct State {
    limits: LoadLimits,
    usage: LoadUsage,
    next: u64,
    retired: Vec<Retired>,
}
struct Pool {
    id: u64,
    state: Mutex<State>,
}

/// Clone this same pool into image loaders and media backends to share admission.
/// Counts cover known loader buffers, not codec-private/OS memory or GPU textures.
#[derive(Clone)]
pub struct ResourceLoadPool(Arc<Pool>);
impl Default for ResourceLoadPool {
    fn default() -> Self {
        Self::new(LoadLimits::default()).expect("valid defaults")
    }
}
impl ResourceLoadPool {
    pub fn new(limits: LoadLimits) -> Result<Self, MediaError> {
        if limits.max_bytes == 0 || limits.max_jobs == 0 {
            return Err(MediaError::ResourceLimit(
                "load limits must be positive".into(),
            ));
        }
        let id = NEXT_POOL
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| MediaError::ResourceLimit("load pool identity exhausted".into()))?;
        Ok(Self(Arc::new(Pool {
            id,
            state: Mutex::new(State {
                limits,
                usage: LoadUsage::default(),
                next: 1,
                retired: vec![],
            }),
        })))
    }
    pub fn usage(&self) -> LoadUsage {
        self.collect();
        self.0.state.lock().unwrap_or_else(|e| e.into_inner()).usage
    }
    fn collect(&self) {
        let retired = std::mem::take(
            &mut self
                .0
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retired,
        );
        let mut alive = vec![];
        for item in retired {
            if (item.alive)() {
                alive.push(item);
            }
        }
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retired
            .extend(alive);
    }
    /// Keep the result's reservation until a downstream CPU image is released.
    /// Collection happens on usage/admission; it never evicts a live consumer.
    pub fn retain_until<T: Send + Sync + 'static, G: Send + 'static>(
        &self,
        image: &Arc<T>,
        guard: G,
    ) {
        let weak = Arc::downgrade(image);
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retired
            .push(Retired {
                alive: Box::new(move || weak.strong_count() > 0),
                _owner: Box::new(guard),
            });
    }
    pub(crate) fn reserve(
        &self,
        kind: LoadKind,
        bytes: u64,
    ) -> Result<LoadReservation, MediaError> {
        self.collect();
        let mut s = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        let total = s
            .usage
            .reserved_bytes
            .checked_add(bytes)
            .ok_or_else(|| MediaError::ResourceLimit("shared load byte overflow".into()))?;
        if bytes == 0 || total > s.limits.max_bytes || s.usage.in_flight >= s.limits.max_jobs {
            return Err(MediaError::ResourceLimit(
                "shared load byte/job budget exhausted".into(),
            ));
        }
        let generation = s.next;
        s.next = s
            .next
            .checked_add(1)
            .ok_or_else(|| MediaError::ResourceLimit("load generation exhausted".into()))?;
        s.usage.reserved_bytes = total;
        s.usage.in_flight += 1;
        s.usage.leases += 1;
        Ok(LoadReservation {
            pool: Arc::downgrade(&self.0),
            ticket: LoadTicket {
                pool: self.0.id,
                generation,
                kind,
            },
            bytes,
            in_flight: true,
        })
    }
}
pub(crate) struct LoadReservation {
    pool: Weak<Pool>,
    ticket: LoadTicket,
    bytes: u64,
    in_flight: bool,
}
impl LoadReservation {
    pub fn ticket(&self) -> LoadTicket {
        self.ticket
    }
    pub fn reduce(&mut self, retained_bytes: u64) -> Result<(), MediaError> {
        if retained_bytes > self.bytes {
            return Err(MediaError::ResourceLimit(
                "loader exceeded admitted bytes".into(),
            ));
        }
        if let Some(pool) = self.pool.upgrade() {
            pool.state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .usage
                .reserved_bytes -= self.bytes - retained_bytes;
        }
        self.bytes = retained_bytes;
        Ok(())
    }
    /// Release transient workspace only after its actual work has finished.
    pub fn complete(&mut self, retained_bytes: u64) -> Result<(), MediaError> {
        if retained_bytes > self.bytes {
            return Err(MediaError::ResourceLimit(
                "loader exceeded admitted bytes".into(),
            ));
        }
        if let Some(pool) = self.pool.upgrade() {
            let mut s = pool.state.lock().unwrap_or_else(|e| e.into_inner());
            s.usage.reserved_bytes -= self.bytes - retained_bytes;
            if self.in_flight {
                s.usage.in_flight -= 1;
            }
        }
        self.bytes = retained_bytes;
        self.in_flight = false;
        Ok(())
    }
}
impl Drop for LoadReservation {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.upgrade() {
            let mut s = pool.state.lock().unwrap_or_else(|e| e.into_inner());
            s.usage.reserved_bytes -= self.bytes;
            s.usage.leases -= 1;
            if self.in_flight {
                s.usage.in_flight -= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn downstream_image_retirement_collects_without_a_pool_cycle() {
        let pool = ResourceLoadPool::new(LoadLimits {
            max_bytes: 100,
            max_jobs: 1,
        })
        .unwrap();
        let image = Arc::new(vec![1u8; 40]);
        let mut reservation = pool.reserve(LoadKind::Image, 100).unwrap();
        reservation.complete(40).unwrap();
        pool.retain_until(&image, reservation);
        assert_eq!(pool.usage().reserved_bytes, 40);
        drop(image);
        assert_eq!(pool.usage(), LoadUsage::default());
        let image = Arc::new(());
        let reservation = pool.reserve(LoadKind::Image, 100).unwrap();
        pool.retain_until(&image, reservation);
        let weak = Arc::downgrade(&pool.0);
        drop(pool);
        assert!(weak.upgrade().is_none());
        drop(image);
    }
    #[test]
    fn shared_bytes_jobs_generations_and_retained_results() {
        let pool = ResourceLoadPool::new(LoadLimits {
            max_bytes: 100,
            max_jobs: 2,
        })
        .unwrap();
        let mut image = pool.reserve(LoadKind::Image, 60).unwrap();
        let old = image.ticket();
        let media = pool.clone().reserve(LoadKind::Media, 40).unwrap();
        assert!(pool.reserve(LoadKind::Image, 1).is_err());
        assert!(image.complete(61).is_err());
        assert_eq!(pool.usage().reserved_bytes, 100);
        image.complete(10).unwrap();
        let newer = pool.reserve(LoadKind::Image, 50).unwrap();
        assert_ne!(newer.ticket(), old);
        assert_eq!(
            pool.usage(),
            LoadUsage {
                reserved_bytes: 100,
                in_flight: 2,
                leases: 3
            }
        );
        drop((image, media, newer));
        assert_eq!(pool.usage(), LoadUsage::default());
    }
}

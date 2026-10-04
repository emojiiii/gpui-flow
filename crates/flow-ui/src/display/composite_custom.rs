//! Explicit CPU producer bridge. Native DisplayRenderer is a separate capability.
use gpui_flow_composite::{
    CompositeError, CompositeLimits, CustomImage, CustomInputs, DomainSource, RasterViewport,
    StaticImage,
};
use gpui_flow_engine::{ResourceCancellationToken, ResourceTicket};
use gpui_flow_scene::{Affine, Bounds, DisplayId, DisplayObject, Shape};
use std::{
    collections::BTreeMap,
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Cooperative signal only: the host must bound its own execution and temporary allocations.
pub struct CustomRasterCancellation<'a> {
    worker: &'a AtomicBool,
    registration: &'a AtomicBool,
    resource: Option<&'a ResourceCancellationToken>,
}
impl CustomRasterCancellation<'_> {
    pub fn is_cancelled(&self) -> bool {
        self.worker.load(Ordering::Acquire)
            || self.registration.load(Ordering::Acquire)
            || self
                .resource
                .is_some_and(ResourceCancellationToken::is_cancelled)
    }
}
/// Accepted bytes from the existing authorized resource provider. Pure computation has none.
pub struct CustomRasterResource<'a> {
    pub ticket: ResourceTicket,
    pub bytes: &'a [u8],
}
pub struct CustomRasterRequest<'a> {
    /// Immutable presented values. Produce only this node's local ink; do not apply
    /// its opacity, blend, masks, erasure, effects, world transform or ancestor clips.
    pub object: &'a DisplayObject,
    pub world_transform: Affine,
    pub viewport: RasterViewport,
    pub resource: Option<CustomRasterResource<'a>>,
    pub cancellation: CustomRasterCancellation<'a>,
    pub limits: CompositeLimits,
    pub max_output_bytes: u64,
    pub max_pixel_work: u64,
}
pub struct CustomRasterOutput {
    pub width: u32,
    pub height: u32,
    pub premul_rgba: Vec<u8>,
    /// Positive tight subrectangle contained in Custom.bounds. Empty geometry uses None.
    pub local_bounds: Bounds,
    /// Host-reported production writes, at least width * height. Temporary host
    /// memory and arbitrary callback CPU time are outside the compositor guarantee.
    pub pixel_work: u64,
}
pub trait CustomRasterizer: Send + Sync + 'static {
    /// Ok(None) explicitly declares no ink. Errors and panics reject the complete domain.
    fn rasterize(
        &self,
        request: CustomRasterRequest<'_>,
    ) -> Result<Option<CustomRasterOutput>, String>;
}
struct Entry {
    producer: Arc<dyn CustomRasterizer>,
    cancelled: AtomicBool,
}
#[derive(Clone)]
pub(crate) struct CustomBackendSnapshot(Arc<Entry>);
impl fmt::Debug for CustomBackendSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomBackendSnapshot")
            .field("current", &self.current())
            .finish()
    }
}
impl PartialEq for CustomBackendSnapshot {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl CustomBackendSnapshot {
    pub fn current(&self) -> bool {
        !self.0.cancelled.load(Ordering::Acquire)
    }
}
#[derive(Default)]
pub(crate) struct CustomRasterizers(BTreeMap<String, CustomBackendSnapshot>);
impl CustomRasterizers {
    pub fn set(&mut self, kind: String, producer: Option<Arc<dyn CustomRasterizer>>) {
        if let Some(old) = self.0.remove(&kind) {
            old.0.cancelled.store(true, Ordering::Release);
        }
        if let Some(producer) = producer {
            self.0.insert(
                kind,
                CustomBackendSnapshot(Arc::new(Entry {
                    producer,
                    cancelled: AtomicBool::new(false),
                })),
            );
        }
    }
    pub fn invalidate(&mut self, kind: &str) -> bool {
        let Some(old) = self.0.get(kind) else {
            return false;
        };
        self.set(kind.into(), Some(old.0.producer.clone()));
        true
    }
    pub fn get(&self, kind: &str) -> Option<CustomBackendSnapshot> {
        self.0.get(kind).cloned()
    }
}
impl Drop for CustomRasterizers {
    fn drop(&mut self) {
        for entry in self.0.values() {
            entry.0.cancelled.store(true, Ordering::Release);
        }
    }
}
#[derive(Clone)]
pub(crate) struct CustomObservation {
    pub backend: Option<CustomBackendSnapshot>,
    pub ticket: Option<ResourceTicket>,
    pub cancellation: Option<ResourceCancellationToken>,
    pub bytes: Result<Option<Arc<[u8]>>, CompositeError>,
}
impl fmt::Debug for CustomObservation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomObservation")
            .field("backend", &self.backend)
            .field("ticket", &self.ticket)
            .field(
                "bytes",
                &self.bytes.as_ref().map(|b| b.as_ref().map(|b| b.len())),
            )
            .finish()
    }
}
impl PartialEq for CustomObservation {
    fn eq(&self, other: &Self) -> bool {
        self.backend == other.backend
            && self.ticket == other.ticket
            && match (&self.bytes, &other.bytes) {
                (Ok(Some(a)), Ok(Some(b))) => Arc::ptr_eq(a, b),
                (Ok(None), Ok(None)) => true,
                (Err(a), Err(b)) => a == b,
                _ => false,
            }
    }
}
impl CustomObservation {
    pub fn current(&self) -> bool {
        self.backend
            .as_ref()
            .is_none_or(CustomBackendSnapshot::current)
            && self.cancellation.as_ref().is_none_or(|c| !c.is_cancelled())
    }
}
pub(crate) struct CustomPreparation<'a> {
    pub viewport: RasterViewport,
    pub cancelled: &'a AtomicBool,
    pub retained: u64,
    pub source_bytes: &'a mut u64,
    pub remaining_work: &'a mut u64,
    pub limits: CompositeLimits,
}
impl CustomPreparation<'_> {
    pub fn domain(
        &mut self,
        source: &DomainSource,
        observations: &BTreeMap<DisplayId, CustomObservation>,
        inputs: &mut CustomInputs,
    ) {
        for (object, world) in source.customs() {
            let result = self.produce(object, world, observations.get(&object.id));
            inputs.insert(object.id.clone(), result);
        }
    }
    fn produce(
        &mut self,
        object: &DisplayObject,
        world: Affine,
        observation: Option<&CustomObservation>,
    ) -> Result<Option<Arc<CustomImage>>, CompositeError> {
        let Shape::Custom(custom) = &object.shape else {
            unreachable!()
        };
        let observation =
            observation.ok_or_else(|| CompositeError::CustomBackendRequired(object.id.clone()))?;
        let backend = observation
            .backend
            .as_ref()
            .ok_or_else(|| CompositeError::CustomBackendRequired(object.id.clone()))?;
        let cancellation = CustomRasterCancellation {
            worker: self.cancelled,
            registration: &backend.0.cancelled,
            resource: observation.cancellation.as_ref(),
        };
        if cancellation.is_cancelled() {
            return Err(CompositeError::Superseded);
        }
        let bytes = observation.bytes.as_ref().map_err(Clone::clone)?;
        let resource = match (observation.ticket, bytes.as_ref()) {
            (Some(ticket), Some(bytes)) => Some(CustomRasterResource { ticket, bytes }),
            (None, None) => None,
            _ => return Err(CompositeError::CustomPending(object.id.clone())),
        };
        let capacity = (32 * 1024 * 1024u64)
            .saturating_sub(*self.source_bytes)
            .min(
                self.limits
                    .max_surface_bytes
                    .saturating_sub(self.retained)
                    .saturating_sub(*self.source_bytes),
            );
        let request = CustomRasterRequest {
            object,
            world_transform: world,
            viewport: self.viewport,
            resource,
            cancellation,
            limits: self.limits.bounded(),
            max_output_bytes: capacity,
            max_pixel_work: *self.remaining_work,
        };
        let result = catch_unwind(AssertUnwindSafe(|| backend.0.producer.rasterize(request)));
        if self.cancelled.load(Ordering::Acquire) || !observation.current() {
            return Err(CompositeError::Superseded);
        }
        let raster = result
            .map_err(|_| CompositeError::CustomRasterization {
                object: object.id.clone(),
                reason: "custom rasterizer panicked".into(),
            })?
            .map_err(|reason| CompositeError::CustomRasterization {
                object: object.id.clone(),
                reason: reason.chars().take(4096).collect(),
            })?;
        let Some(raster) = raster else {
            return Ok(None);
        };
        let pixels = u64::from(raster.width) * u64::from(raster.height);
        if raster.pixel_work < pixels {
            return Err(CompositeError::InvalidGeometry(
                "custom pixel work underreported".into(),
            ));
        }
        let Some(remaining) = self.remaining_work.checked_sub(raster.pixel_work) else {
            *self.remaining_work = 0;
            return Err(CompositeError::WorkLimit);
        };
        *self.remaining_work = remaining;
        if raster.premul_rgba.capacity() as u64 > capacity {
            return Err(CompositeError::WorkingSetLimit);
        }
        let image = Arc::new(StaticImage::from_premultiplied_rgba(
            raster.width,
            raster.height,
            raster.premul_rgba,
            self.limits,
        )?);
        let ink = CustomImage {
            image,
            local_bounds: raster.local_bounds,
        };
        ink.validate(custom.bounds, self.limits)?;
        *self.source_bytes = self
            .source_bytes
            .checked_add(ink.image.byte_len())
            .ok_or(CompositeError::WorkingSetLimit)?;
        Ok(Some(Arc::new(ink)))
    }
}

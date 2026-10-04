//! Explicit host-owned pixels, independent of native renderers and fallback paths.
use crate::{CompositeError, CompositeLimits, StaticImage};
use gpui_flow_scene::{Bounds, DisplayId};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Complete local ink before node opacity, blend, effects or ancestor clipping.
#[derive(Debug)]
pub struct CustomImage {
    pub image: Arc<StaticImage>,
    pub local_bounds: Bounds,
}
impl CustomImage {
    /// A producer may return a tight subrectangle, never ink outside its declared geometry.
    pub fn validate(
        &self,
        declared: Bounds,
        limits: CompositeLimits,
    ) -> Result<(), CompositeError> {
        let b = self.local_bounds;
        // Subtract origins before comparing extents: adding a small width to a
        // large finite origin can lose the width even in f64. Check both
        // remaining extents so a tiny ink width at a huge right edge cannot
        // disappear in outer_size - inner_size either.
        let contained = |inner_origin: f32, inner_size: f32, outer_origin: f32, outer_size: f32| {
            let offset = f64::from(inner_origin) - f64::from(outer_origin);
            inner_origin >= outer_origin
                && inner_size <= outer_size
                && offset <= f64::from(outer_size) - f64::from(inner_size)
                && f64::from(inner_size) <= f64::from(outer_size) - offset
        };
        if !declared.is_valid()
            || !declared.size.is_positive()
            || !b.is_valid()
            || !b.size.is_positive()
            || !contained(
                b.origin.x,
                b.size.width,
                declared.origin.x,
                declared.size.width,
            )
            || !contained(
                b.origin.y,
                b.size.height,
                declared.origin.y,
                declared.size.height,
            )
        {
            return Err(CompositeError::InvalidGeometry("custom ink bounds".into()));
        }
        let limits = limits.bounded();
        if self.image.width() > limits.max_dimension
            || self.image.height() > limits.max_dimension
            || u64::from(self.image.width()) * u64::from(self.image.height()) > limits.max_pixels
        {
            return Err(CompositeError::TooLarge);
        }
        if self.image.byte_len() > limits.max_surface_bytes {
            return Err(CompositeError::WorkingSetLimit);
        }
        Ok(())
    }
}
#[derive(Default)]
pub struct CustomInputs {
    entries: BTreeMap<DisplayId, Result<Option<Arc<CustomImage>>, CompositeError>>,
}
impl CustomInputs {
    /// Only an explicit Ok(None) means no ink. Missing inputs fail the entire domain.
    pub fn insert(
        &mut self,
        id: DisplayId,
        value: Result<Option<Arc<CustomImage>>, CompositeError>,
    ) {
        self.entries.insert(id, value);
    }
    pub(crate) fn get(&self, id: &DisplayId) -> Result<Option<Arc<CustomImage>>, CompositeError> {
        self.entries
            .get(id)
            .cloned()
            .unwrap_or_else(|| Err(CompositeError::CustomBackendRequired(id.clone())))
    }
    pub fn byte_len(&self) -> u64 {
        let mut seen = BTreeSet::new();
        self.entries
            .values()
            .filter_map(|v| v.as_ref().ok()?.as_ref())
            .filter(|v| seen.insert(Arc::as_ptr(&v.image)))
            .fold(0, |sum, v| sum.saturating_add(v.image.byte_len()))
    }
}

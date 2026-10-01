//! Immutable, owned CPU input. The host owns loading and animation policy.
use crate::{CompositeError, CompositeLimits};
use gpui_flow_scene::DisplayId;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// A validated single static frame in tiny-skia's premultiplied RGBA format.
/// Construction accepts straight BGRA, the locked GPUI RenderImage contract.
#[derive(Debug)]
pub struct StaticImage {
    pub(crate) pixmap: tiny_skia::Pixmap,
}
impl StaticImage {
    pub fn from_straight_bgra(
        width: u32,
        height: u32,
        data: &[u8],
        limits: CompositeLimits,
    ) -> Result<Self, CompositeError> {
        let limits = limits.bounded();
        let pixels = u64::from(width) * u64::from(height);
        if width == 0 || height == 0 {
            return Err(CompositeError::InvalidImage);
        }
        if width > limits.max_dimension
            || height > limits.max_dimension
            || pixels > limits.max_pixels
        {
            return Err(CompositeError::TooLarge);
        }
        let len = pixels.checked_mul(4).ok_or(CompositeError::TooLarge)?;
        if len > limits.max_surface_bytes {
            return Err(CompositeError::WorkingSetLimit);
        }
        if u64::try_from(data.len()).ok() != Some(len) {
            return Err(CompositeError::InvalidImage);
        }
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(data.len())
            .map_err(|_| CompositeError::AllocationFailed)?;
        for bgra in data.as_chunks::<4>().0 {
            let p = tiny_skia::ColorU8::from_rgba(bgra[2], bgra[1], bgra[0], bgra[3]).premultiply();
            rgba.extend_from_slice(&[p.red(), p.green(), p.blue(), p.alpha()]);
        }
        let size =
            tiny_skia::IntSize::from_wh(width, height).ok_or(CompositeError::InvalidImage)?;
        Ok(Self {
            pixmap: tiny_skia::Pixmap::from_vec(rgba, size).ok_or(CompositeError::InvalidImage)?,
        })
    }
    /// Consume an already premultiplied RGBA buffer without an additional copy.
    pub fn from_premultiplied_rgba(
        width: u32,
        height: u32,
        data: Vec<u8>,
        limits: CompositeLimits,
    ) -> Result<Self, CompositeError> {
        let limits = limits.bounded();
        let pixels = u64::from(width) * u64::from(height);
        if width == 0 || height == 0 {
            return Err(CompositeError::InvalidImage);
        }
        if width > limits.max_dimension
            || height > limits.max_dimension
            || pixels > limits.max_pixels
        {
            return Err(CompositeError::TooLarge);
        }
        let bytes = pixels.checked_mul(4).ok_or(CompositeError::TooLarge)?;
        if data.len() as u64 != bytes {
            return Err(CompositeError::InvalidImage);
        }
        if bytes > limits.max_surface_bytes {
            return Err(CompositeError::WorkingSetLimit);
        }
        if data
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[..3].iter().any(|v| *v > p[3]))
        {
            return Err(CompositeError::InvalidImage);
        }
        let size =
            tiny_skia::IntSize::from_wh(width, height).ok_or(CompositeError::InvalidImage)?;
        Ok(Self {
            pixmap: tiny_skia::Pixmap::from_vec(data, size).ok_or(CompositeError::InvalidImage)?,
        })
    }
    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }
    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }
    pub fn byte_len(&self) -> u64 {
        self.pixmap.data().len() as u64
    }
    pub fn premultiplied_rgba(&self) -> &[u8] {
        self.pixmap.data()
    }
}

/// Current-resource observations only. Missing entries fail the entire domain;
/// callers must not silently paint an image's layout rectangle instead.
#[derive(Default)]
pub struct ImageInputs {
    entries: BTreeMap<DisplayId, Result<Arc<StaticImage>, CompositeError>>,
}
impl ImageInputs {
    pub fn insert(&mut self, id: DisplayId, input: Result<Arc<StaticImage>, CompositeError>) {
        self.entries.insert(id, input);
    }
    pub(crate) fn get(&self, id: &DisplayId) -> Result<Arc<StaticImage>, CompositeError> {
        self.entries
            .get(id)
            .cloned()
            .unwrap_or_else(|| Err(CompositeError::ImagePending(id.clone())))
    }
    /// Count shared buffers once, including inputs referenced by other domains.
    pub fn byte_len(&self) -> u64 {
        let mut seen = BTreeSet::new();
        self.entries
            .values()
            .filter_map(|v| v.as_ref().ok())
            .filter(|v| seen.insert(Arc::as_ptr(v)))
            .map(|v| v.byte_len())
            .sum()
    }
}

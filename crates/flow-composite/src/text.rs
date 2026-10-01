//! Host-produced glyph ink. This crate never resolves fonts or fabricates text.
use crate::{CompositeError, StaticImage};
use gpui_flow_scene::{Bounds, DisplayId};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// A rasterized text snapshot in the text object's local coordinate system.
/// The producer clips to its alignment box and preserves transparent glyph gaps.
#[derive(Debug)]
pub struct TextImage {
    pub image: Arc<StaticImage>,
    pub local_bounds: Bounds,
}
#[derive(Default)]
pub struct TextInputs {
    entries: BTreeMap<DisplayId, Result<Option<Arc<TextImage>>, CompositeError>>,
}
impl TextInputs {
    pub fn insert(&mut self, id: DisplayId, value: Result<Option<Arc<TextImage>>, CompositeError>) {
        self.entries.insert(id, value);
    }
    pub(crate) fn get(&self, id: &DisplayId) -> Result<Option<Arc<TextImage>>, CompositeError> {
        self.entries
            .get(id)
            .cloned()
            .unwrap_or_else(|| Err(CompositeError::TextBackendRequired(id.clone())))
    }
    pub fn byte_len(&self) -> u64 {
        let mut seen = BTreeSet::new();
        self.entries
            .values()
            .filter_map(|v| v.as_ref().ok()?.as_ref())
            .filter(|v| seen.insert(Arc::as_ptr(&v.image)))
            .map(|v| v.image.byte_len())
            .sum()
    }
}

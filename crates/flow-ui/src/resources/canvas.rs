//! Canvas resource ownership, separate from URL/image loading and playback.
use gpui::{RenderImage, swap_rgba_pa_to_bgra};
use gpui_flow_composite::{CanvasSurface, CompositeError, CompositeLimits};
use gpui_flow_engine::{
    ElementHandle, InstanceState, LifecycleKind, ObserverId, ResourceTicket, SceneEngine,
};
use gpui_flow_scene::{Shape, display::Canvas};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};

struct Entry {
    id: gpui_flow_scene::DisplayId,
    ticket: ResourceTicket,
    surface: CanvasSurface,
    image: Arc<RenderImage>,
    reset: bool,
    path_reset: bool,
    crop: [u32; 2],
}
/// Bounded to 64 MiB of owned CPU pixels + GPUI upload buffers and a separate
/// 16 MiB of current-path geometry, including candidates. Excludes externally
/// retained snapshots and GPUI's private GPU atlas. No URL producer.
/// The change observer drops detached/destroyed/retyped resources immediately.
pub(super) struct CanvasResources {
    entries: Rc<RefCell<BTreeMap<ElementHandle, Entry>>>,
    observer: Option<ObserverId>,
}
impl CanvasResources {
    const MAX_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_PATH_BYTES: u64 = 16 * 1024 * 1024;
    const MAX_ENTRIES: usize = 64;
    pub fn new(engine: &mut SceneEngine) -> Result<Self, gpui_flow_engine::ResourceError> {
        let entries: Rc<RefCell<BTreeMap<ElementHandle, Entry>>> = Rc::default();
        let weak = Rc::downgrade(&entries);
        let observer = engine.on_change(None, move |changes| {
            let Some(entries) = weak.upgrade() else {
                return;
            };
            entries.borrow_mut().retain(|handle, entry| {
                if changes.lifecycle.iter().any(|event| {
                    event.handle == *handle
                        && matches!(
                            event.kind,
                            LifecycleKind::Unmounted
                                | LifecycleKind::Detached
                                | LifecycleKind::Destroyed
                        )
                }) {
                    return false;
                }
                for p in changes.properties.iter().filter(|p| p.property == "shape") {
                    // Runtime handle is stable; changes also expose the document ID.
                    if p.id.as_ref().is_none_or(|id| id != &entry.id) {
                        continue;
                    }
                    let Ok(Shape::Canvas(new)) = serde_json::from_value::<Shape>(p.new.clone())
                    else {
                        return false;
                    };
                    let Ok(Shape::Canvas(old)) = serde_json::from_value::<Shape>(p.old.clone())
                    else {
                        return false;
                    };
                    if old.size != new.size || old.pixel_ratio != new.pixel_ratio {
                        entry.reset |= !new.safe_resize;
                        // Sticky even when a safe A→B→A sequence ends at the
                        // original config before the next resource preparation.
                        entry.path_reset = true;
                        if let Ok([w, h]) = new.pixel_size() {
                            entry.crop[0] = entry.crop[0].min(w);
                            entry.crop[1] = entry.crop[1].min(h);
                        }
                    }
                }
                true
            });
        })?;
        Ok(Self {
            entries,
            observer: Some(observer),
        })
    }
    pub fn retain_live(&self, engine: &SceneEngine) {
        self.entries.borrow_mut().retain(|handle, entry| {
            engine.instance(*handle).is_ok_and(|i| {
                i.state == InstanceState::Attached
                    && i.attachment_epoch == entry.ticket.attachment_epoch
            }) && engine
                .object(*handle)
                .is_ok_and(|o| matches!(o.shape, Shape::Canvas(_)))
        });
    }
    pub fn ensure(
        &self,
        ticket: ResourceTicket,
        id: gpui_flow_scene::DisplayId,
        config: &Canvas,
    ) -> Result<(), CompositeError> {
        let mut entries = self.entries.borrow_mut();
        if entries
            .get(&ticket.handle)
            .is_some_and(|e| e.ticket == ticket && e.id == id)
        {
            return Ok(());
        }
        let retained: u64 = entries.values().map(|e| e.surface.byte_len() * 2).sum();
        let retained_paths: u64 = entries
            .values()
            .map(|e| e.surface.current_path_byte_len())
            .sum();
        let [w, h] = config
            .pixel_size()
            .map_err(CompositeError::InvalidGeometry)?;
        if retained.saturating_add(w as u64 * h as u64 * 8) > Self::MAX_BYTES {
            return Err(CompositeError::WorkingSetLimit);
        }
        if !entries.contains_key(&ticket.handle) && entries.len() >= Self::MAX_ENTRIES {
            return Err(CompositeError::InputLimit);
        }
        let surface = if let Some(old) = entries.get(&ticket.handle).filter(|e| {
            e.id == id && e.ticket.attachment_epoch == ticket.attachment_epoch && !e.reset
        }) {
            let mut surface = old.surface.clone();
            surface.reconfigure(config.clone())?;
            if old.path_reset {
                surface.reset_current_path();
            }
            surface.retain_pixels(old.crop[0], old.crop[1])?;
            surface
        } else {
            CanvasSurface::new(config.clone(), CompositeLimits::default())?
        };
        let added_path_bytes = entries.get(&ticket.handle).map_or_else(
            || surface.current_path_byte_len(),
            |old| surface.additional_current_path_bytes(&old.surface),
        );
        if retained_paths.saturating_add(added_path_bytes) > Self::MAX_PATH_BYTES {
            return Err(CompositeError::WorkingSetLimit);
        }
        // The id is stored independently from the serializable Canvas config.
        let image = upload(&surface)?;
        let crop = [surface.snapshot().width(), surface.snapshot().height()];
        entries.insert(
            ticket.handle,
            Entry {
                id,
                ticket,
                surface,
                image,
                reset: false,
                path_reset: false,
                crop,
            },
        );
        Ok(())
    }
    pub fn image(&self, ticket: ResourceTicket) -> Option<(Arc<RenderImage>, u64)> {
        self.entries
            .borrow()
            .get(&ticket.handle)
            .filter(|e| e.ticket == ticket)
            .map(|e| (e.image.clone(), e.surface.generation()))
    }
    pub fn snapshot(
        &self,
        ticket: ResourceTicket,
    ) -> Option<Arc<gpui_flow_composite::StaticImage>> {
        self.entries
            .borrow()
            .get(&ticket.handle)
            .filter(|e| e.ticket == ticket)
            .map(|e| e.surface.snapshot())
    }
    pub fn edit(
        &self,
        ticket: ResourceTicket,
        edit: impl FnOnce(&mut CanvasSurface) -> Result<(), CompositeError>,
    ) -> Result<(), CompositeError> {
        let mut entries = self.entries.borrow_mut();
        let retained: u64 = entries.values().map(|e| e.surface.byte_len() * 2).sum();
        let retained_paths: u64 = entries
            .values()
            .map(|e| e.surface.current_path_byte_len())
            .sum();
        let entry = entries
            .get_mut(&ticket.handle)
            .filter(|e| e.ticket == ticket)
            .ok_or(CompositeError::Superseded)?;
        if retained.saturating_add(entry.surface.byte_len() * 2) > Self::MAX_BYTES {
            return Err(CompositeError::WorkingSetLimit);
        }
        // The compiler charges its immutable starting path itself. Other live
        // surfaces consume the remaining aggregate budget; a cheap shared clone
        // must not double-charge its baseline or prevent geometry-only clearing.
        let available_path_bytes = Self::MAX_PATH_BYTES
            .checked_sub(retained_paths)
            .ok_or(CompositeError::WorkingSetLimit)?
            .saturating_add(entry.surface.current_path_byte_len());
        let mut surface = entry.surface.clone();
        surface.with_current_path_budget(available_path_bytes, edit)?;
        if surface.config() != entry.surface.config() {
            return Err(CompositeError::InvalidGeometry(
                "resize through the scene component, not pixel edit".into(),
            ));
        }
        // Check actual retained allocation before committing either geometry or
        // pixels. Path budget lowering is temporary and cannot authorize a larger
        // final candidate, including one replaced by an arbitrary host closure.
        if retained_paths.saturating_add(surface.additional_current_path_bytes(&entry.surface))
            > Self::MAX_PATH_BYTES
        {
            return Err(CompositeError::WorkingSetLimit);
        }
        if surface.generation() == entry.surface.generation()
            && Arc::ptr_eq(&surface.snapshot(), &entry.surface.snapshot())
        {
            // A valid path-only edit publishes context state, not pixel content.
            // Keep the existing immutable snapshot generation and GPUI upload.
            entry.surface = surface;
            return Ok(());
        }
        let image = upload(&surface)?;
        entry.surface = surface;
        entry.image = image;
        Ok(())
    }
    pub fn dispose(&mut self, engine: &mut SceneEngine) {
        self.entries.borrow_mut().clear();
        if let Some(observer) = self.observer.take() {
            engine.off_change(observer);
        }
    }
}
fn upload(surface: &CanvasSurface) -> Result<Arc<RenderImage>, CompositeError> {
    let pixels = surface.snapshot();
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(pixels.byte_len() as usize)
        .map_err(|_| CompositeError::AllocationFailed)?;
    bytes.extend_from_slice(pixels.premultiplied_rgba());
    for pixel in bytes.as_chunks_mut::<4>().0 {
        swap_rgba_pa_to_bgra(pixel);
    }
    let buffer = image::RgbaImage::from_raw(pixels.width(), pixels.height(), bytes)
        .ok_or(CompositeError::InvalidImage)?;
    Ok(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

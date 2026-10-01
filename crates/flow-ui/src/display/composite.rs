//! Per-SceneView CPU work queue and real GPUI image upload. No global caches.
use gpui::{App, Bounds, Corners, RenderImage, Window, px};
use gpui_flow_composite::{
    CompositeError, CompositeIssue, CompositeLimits, CpuFrame, DomainSource, ImageInputs,
    PixelBudget, RasterViewport, StaticImage, rasterize_with_images,
};
use gpui_flow_engine::{ElementHandle, SceneEngine};
use gpui_flow_scene::DisplayId;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_ENTRIES: usize = 8;
const MAX_RETAINED: u64 = 32 * 1024 * 1024;
const MAX_RETIRED: u64 = 32 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DomainKey {
    pub source: DomainSource,
    identities: Vec<(ElementHandle, u64)>,
    images: BTreeMap<DisplayId, crate::resources::CompositeImageObservation>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RequestKey {
    pub domains: Vec<DomainKey>,
    pub viewport: RasterViewport,
    pub window: u64,
    revision: u64,
    document_epoch: u64,
}
impl RequestKey {
    pub fn new(
        domains: Vec<DomainSource>,
        engine: &SceneEngine,
        viewport: RasterViewport,
        window: u64,
    ) -> Self {
        let domains = domains
            .into_iter()
            .map(|source| {
                let mut ids = source.ids().into_iter().cloned().collect::<Vec<_>>();
                if let Ok(ancestors) = engine.scene().ancestor_path(source.root_id()) {
                    ids.extend(ancestors);
                }
                ids.sort();
                ids.dedup();
                let identities = ids
                    .iter()
                    .filter_map(|id| engine.handle(id))
                    .filter_map(|h| engine.instance(h).ok().map(|i| (h, i.attachment_epoch)))
                    .collect();
                DomainKey {
                    source,
                    identities,
                    images: BTreeMap::new(),
                }
            })
            .collect();
        Self {
            domains,
            viewport,
            window,
            revision: engine.revision(),
            document_epoch: engine.document_epoch(),
        }
    }
    pub fn with_resources(
        mut self,
        engine: &SceneEngine,
        resources: Option<&crate::resources::SceneResourceAdapter>,
    ) -> Self {
        for domain in &mut self.domains {
            for (id, image) in domain.source.images() {
                let observation = resources
                    .map(|r| r.composite_image(engine, id, &image.source))
                    .unwrap_or_else(|| crate::resources::CompositeImageObservation {
                        ticket: None,
                        image: Err(CompositeError::ImagePending(id.clone())),
                    });
                domain.images.insert(id.clone(), observation);
            }
        }
        self
    }
    fn compatible(&self, other: &Self, id: &DisplayId) -> bool {
        self.window == other.window
            && self.viewport == other.viewport
            && self.document_epoch == other.document_epoch
            && self
                .domains
                .iter()
                .find(|d| d.source.root_id() == id)
                .zip(other.domains.iter().find(|d| d.source.root_id() == id))
                .is_some_and(|(a, b)| {
                    a.identities == b.identities
                        && a.images == b.images
                        && a.source.placement_eq(&b.source)
                })
    }
}
#[derive(Clone)]
pub(crate) struct ImageFrame {
    pub image: Arc<RenderImage>,
    pub bounds: gpui_flow_scene::Bounds,
    pub bytes: u64,
}
fn into_image(frame: CpuFrame) -> Result<ImageFrame, CompositeError> {
    let mut data = frame.premul_rgba;
    if data.len() != frame.width as usize * frame.height as usize * 4 {
        return Err(CompositeError::InvalidGeometry("RGBA buffer length".into()));
    }
    for pixel in data.as_chunks_mut::<4>().0 {
        gpui::swap_rgba_pa_to_bgra(pixel);
    }
    let bytes = data.len() as u64;
    let buffer = image::RgbaImage::from_raw(frame.width, frame.height, data)
        .ok_or(CompositeError::AllocationFailed)?;
    Ok(ImageFrame {
        image: Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])),
        bounds: frame.screen_bounds,
        bytes,
    })
}
struct Accepted {
    key: Arc<RequestKey>,
    frame: ImageFrame,
}
pub(crate) struct CompositeController {
    pub requested: Option<Arc<RequestKey>>,
    completed: Option<Arc<RequestKey>>,
    running: bool,
    cancel: Arc<AtomicBool>,
    candidates: BTreeMap<DisplayId, Option<ImageFrame>>,
    accepted: BTreeMap<DisplayId, Accepted>,
    errors: Vec<CompositeIssue>,
    blocked_uploads: BTreeSet<DisplayId>,
    retired_bytes: u64,
    budget_epoch: u64,
    running_budget_epoch: u64,
}
impl Default for CompositeController {
    fn default() -> Self {
        Self {
            requested: None,
            completed: None,
            running: false,
            cancel: Arc::new(AtomicBool::new(false)),
            candidates: BTreeMap::new(),
            accepted: BTreeMap::new(),
            errors: vec![],
            blocked_uploads: BTreeSet::new(),
            retired_bytes: 0,
            budget_epoch: 0,
            running_budget_epoch: 0,
        }
    }
}
pub(crate) struct Work {
    pub key: Arc<RequestKey>,
    pub cancel: Arc<AtomicBool>,
    retained: u64,
    settled: BTreeSet<DisplayId>,
    settled_bytes: u64,
}
pub(crate) type WorkResult = Vec<(DisplayId, Result<Option<CpuFrame>, CompositeError>)>;
// GPUI owns fetching/decoding/cache. Only frame_count == 1 has a truthful
// producer here; native img's private animated frame cursor is not guessed.
fn static_image(
    image: &RenderImage,
    bytes: &mut u64,
    capacity: u64,
) -> Result<Arc<StaticImage>, CompositeError> {
    if image.frame_count() > 1 {
        return Err(CompositeError::UnsupportedAnimatedImage);
    }
    if image.frame_count() != 1 {
        return Err(CompositeError::InvalidImage);
    }
    let size = image.size(0);
    let data = image.as_bytes(0).ok_or(CompositeError::InvalidImage)?;
    let next = bytes
        .checked_add(data.len() as u64)
        .ok_or(CompositeError::WorkingSetLimit)?;
    if next > capacity.min(MAX_RETAINED) {
        return Err(CompositeError::WorkingSetLimit);
    }
    let input = StaticImage::from_straight_bgra(
        size.width.0 as u32,
        size.height.0 as u32,
        data,
        CompositeLimits::default(),
    )?;
    *bytes = next;
    Ok(Arc::new(input))
}
impl Work {
    pub fn run(&self) -> WorkResult {
        let mut images = ImageInputs::default();
        let mut converted: BTreeMap<gpui::ImageId, Result<Arc<StaticImage>, CompositeError>> =
            BTreeMap::new();
        let mut source_bytes = 0u64;
        for domain in &self.key.domains {
            if self.settled.contains(domain.source.root_id()) {
                continue;
            }
            for (id, observation) in &domain.images {
                let input = observation
                    .image
                    .as_ref()
                    .map_err(Clone::clone)
                    .and_then(|image| {
                        if self.cancel.load(Ordering::Relaxed) {
                            return Err(CompositeError::Superseded);
                        }
                        if let Some(input) = converted.get(&image.id) {
                            return input.clone();
                        }
                        let capacity = CompositeLimits::default()
                            .max_surface_bytes
                            .saturating_sub(self.retained);
                        let input = static_image(image, &mut source_bytes, capacity);
                        converted.insert(image.id, input.clone());
                        input
                    });
                images.insert(id.clone(), input);
            }
        }
        // GPUI/cache-owned decoded originals are outside this adapter's surface
        // budget. Our converted copies are charged by rasterize_with_images.
        let mut budget = match PixelBudget::new(CompositeLimits::default(), self.retained) {
            Ok(b) => b,
            Err(e) => {
                return self
                    .key
                    .domains
                    .iter()
                    .filter(|d| !self.settled.contains(d.source.root_id()))
                    .map(|d| (d.source.root_id().clone(), Err(e.clone())))
                    .collect();
            }
        };
        let mut retained = self.settled_bytes;
        self.key
            .domains
            .iter()
            .enumerate()
            .filter(|(_, d)| !self.settled.contains(d.source.root_id()))
            .map(|(index, d)| {
                let result = if index >= MAX_ENTRIES {
                    Err(CompositeError::InputLimit)
                } else {
                    rasterize_with_images(
                        &d.source,
                        &images,
                        self.key.viewport,
                        &mut budget,
                        &self.cancel,
                    )
                    .and_then(|f| {
                        let bytes = f.as_ref().map_or(0, |f| f.premul_rgba.len() as u64);
                        if retained + bytes > MAX_RETAINED {
                            if let Some(frame) = f {
                                budget.discard_frame(frame);
                            }
                            Err(CompositeError::WorkingSetLimit)
                        } else {
                            retained += bytes;
                            Ok(f)
                        }
                    })
                };
                (d.source.root_id().clone(), result)
            })
            .collect()
    }
}
impl CompositeController {
    /// Replaces the one pending request; an old worker is only cooperatively
    /// cancelled and must finish before another worker can start.
    pub fn request(&mut self, key: Arc<RequestKey>) -> Vec<ImageFrame> {
        if self.requested.as_ref() == Some(&key) {
            return vec![];
        }
        self.cancel.store(true, Ordering::Relaxed);
        // A -> B -> A may return to the same presented values/revision after
        // A's unaccepted candidate was cleared. Completion is request-local.
        self.completed = None;
        self.candidates.clear();
        self.errors.clear();
        self.blocked_uploads.clear();
        self.requested = Some(key);
        self.drain_obsolete()
    }
    pub fn drain_obsolete(&mut self) -> Vec<ImageFrame> {
        let Some(key) = &self.requested else {
            return vec![];
        };
        let mut allowance = MAX_RETIRED.saturating_sub(self.retired_bytes);
        let mut retired = Vec::new();
        self.accepted.retain(|id, old| {
            let obsolete = !key.compatible(&old.key, id)
                || self.candidates.get(id).is_some_and(Option::is_none);
            if obsolete && old.frame.bytes <= allowance {
                allowance -= old.frame.bytes;
                retired.push(old.frame.clone());
                false
            } else {
                true
            }
        });
        retired
    }
    fn held_bytes(&self) -> u64 {
        self.retired_bytes
            + self.accepted.values().map(|v| v.frame.bytes).sum::<u64>()
            + self
                .candidates
                .values()
                .flatten()
                .map(|v| v.bytes)
                .sum::<u64>()
    }
    pub fn take_work(&mut self) -> Option<Work> {
        let key = self.requested.as_ref()?;
        if self.running || self.completed.as_ref() == Some(key) {
            return None;
        }
        if key.domains.is_empty() {
            self.completed = Some(key.clone());
            return None;
        }
        // A budget retry must preserve successful domains, including candidates
        // awaiting upload. Re-publishing them would retire their current images,
        // and that release could trigger an endless success/failure retry loop.
        let mut settled = BTreeSet::new();
        let mut settled_bytes = 0;
        for domain in &key.domains {
            let id = domain.source.root_id();
            if let Some(candidate) = self.candidates.get(id) {
                settled.insert(id.clone());
                settled_bytes += candidate.as_ref().map_or(0, |frame| frame.bytes);
            } else if let Some(accepted) = self.accepted.get(id).filter(|a| a.key == *key) {
                settled.insert(id.clone());
                settled_bytes += accepted.frame.bytes;
            } else if self.blocked_uploads.contains(id)
                || self.errors.iter().any(|issue| {
                    &issue.object == id && issue.error != CompositeError::WorkingSetLimit
                })
            {
                // Only budget failures may change after a byte release. Preserve
                // terminal diagnostics and never bypass an upload block here.
                settled.insert(id.clone());
            }
        }
        self.running = true;
        self.running_budget_epoch = self.budget_epoch;
        self.cancel = Arc::new(AtomicBool::new(false));
        Some(Work {
            key: key.clone(),
            cancel: self.cancel.clone(),
            retained: self.held_bytes(),
            settled,
            settled_bytes,
        })
    }
    pub fn finish(&mut self, key: Arc<RequestKey>, result: WorkResult) {
        self.running = false;
        if self.requested.as_ref() != Some(&key) || self.cancel.load(Ordering::Relaxed) {
            return;
        }
        self.completed = Some(key);
        for (id, result) in result {
            self.errors.retain(|issue| issue.object != id);
            match result.and_then(|frame| frame.map(into_image).transpose()) {
                Ok(frame) => {
                    self.candidates.insert(id, frame);
                }
                Err(error) => self.errors.push(CompositeIssue { object: id, error }),
            }
        }
        // A release can happen while the worker is using its older, more
        // conservative retained-byte snapshot. Retry once at the new epoch.
        if self.budget_epoch != self.running_budget_epoch && self.memory_limited() {
            self.completed = None;
        }
    }
    fn memory_limited(&self) -> bool {
        self.errors
            .iter()
            .any(|issue| issue.error == CompositeError::WorkingSetLimit)
    }
    fn release_retired(&mut self, bytes: u64) {
        let released = bytes.min(self.retired_bytes);
        self.retired_bytes -= released;
        self.budget_released(released);
    }
    fn budget_released(&mut self, released: u64) {
        if released > 0 {
            self.budget_epoch = self.budget_epoch.wrapping_add(1);
            if self.memory_limited() {
                self.completed = None;
            }
        }
    }
    pub fn diagnostics(&self) -> Vec<CompositeIssue> {
        let mut out = self.errors.clone();
        if let Some(key) = &self.requested {
            for (id, old) in &self.accepted {
                if old.key != *key && key.compatible(&old.key, id) {
                    out.push(CompositeIssue {
                        object: id.clone(),
                        error: CompositeError::StaleFrame,
                    });
                }
            }
        }
        out
    }
    pub fn pending(&self) -> bool {
        self.requested.as_ref() != self.completed.as_ref()
            || self.requested.as_ref().is_some_and(|k| {
                k.domains.iter().any(|d| {
                    d.images
                        .values()
                        .any(|i| matches!(i.image, Err(CompositeError::ImagePending(_))))
                })
            })
    }
    fn can_accept(&self, id: &DisplayId, candidate: &ImageFrame) -> bool {
        let old_bytes = self.accepted.get(id).map_or(0, |v| v.frame.bytes);
        self.retired_bytes + old_bytes <= MAX_RETIRED
            && self.accepted.values().map(|a| a.frame.bytes).sum::<u64>() - old_bytes
                + candidate.bytes
                <= MAX_RETAINED
    }
    pub fn frame(&self, id: &DisplayId) -> Option<(ImageFrame, bool)> {
        if self.blocked_uploads.contains(id) {
            return None;
        }
        match self.candidates.get(id) {
            Some(Some(frame)) if self.can_accept(id, frame) => Some((frame.clone(), true)),
            Some(Some(_)) => self.old_frame(id).map(|v| (v, false)),
            Some(None) => None,
            None => self.old_frame(id).map(|v| (v, false)),
        }
    }
    /// Accept only after paint_image reports upload success. The old complete
    /// frame survives errors and remains available at the exact old placement.
    pub fn accepted(
        &mut self,
        key: &Arc<RequestKey>,
        id: &DisplayId,
        image: &Arc<RenderImage>,
    ) -> Option<ImageFrame> {
        if self.requested.as_ref() != Some(key) {
            return None;
        }
        let Some(Some(candidate)) = self.candidates.get(id) else {
            return None;
        };
        if !Arc::ptr_eq(&candidate.image, image) {
            return None;
        }
        if !self.can_accept(id, candidate) {
            return None;
        }
        let Some(Some(frame)) = self.candidates.remove(id) else {
            return None;
        };
        self.errors.retain(|e| &e.object != id);
        self.accepted
            .insert(
                id.clone(),
                Accepted {
                    key: key.clone(),
                    frame,
                },
            )
            .map(|a| a.frame)
    }
    pub fn upload_failed(&mut self, key: &Arc<RequestKey>, id: &DisplayId, error: String) {
        if self.requested.as_ref() != Some(key) {
            return;
        }
        let released = self
            .candidates
            .remove(id)
            .flatten()
            .map_or(0, |frame| frame.bytes);
        self.budget_released(released);
        self.errors.retain(|e| &e.object != id);
        self.errors.push(CompositeIssue {
            object: id.clone(),
            error: CompositeError::UploadFailed(error),
        });
    }
    pub fn block_upload(&mut self, id: DisplayId) {
        self.blocked_uploads.insert(id);
    }
    pub fn old_frame(&self, id: &DisplayId) -> Option<ImageFrame> {
        self.accepted
            .get(id)
            .filter(|a| {
                self.requested
                    .as_ref()
                    .is_some_and(|k| k.compatible(&a.key, id))
            })
            .map(|a| a.frame.clone())
    }
    pub fn retire(
        &mut self,
        frame: ImageFrame,
        window: &Window,
        entity: gpui::WeakEntity<crate::scene_view::SceneView>,
    ) {
        self.retired_bytes += frame.bytes;
        window.on_next_frame(move |window, cx| {
            cx.drop_image(frame.image, Some(window));
            let _ = entity.update(cx, |this, cx| {
                this.compositing.release_retired(frame.bytes);
                cx.notify();
            });
        });
    }
    pub fn dispose(&mut self, cx: &mut App) {
        self.cancel.store(true, Ordering::Relaxed);
        for (_, a) in std::mem::take(&mut self.accepted) {
            cx.drop_image(a.frame.image, None);
        }
        for (_, c) in std::mem::take(&mut self.candidates) {
            if let Some(c) = c {
                cx.drop_image(c.image, None);
            }
        }
    }
}
pub(crate) fn paint(frame: &ImageFrame, window: &mut Window) -> Result<(), String> {
    let b = frame.bounds;
    let b = Bounds::new(
        gpui::point(px(b.origin.x), px(b.origin.y)),
        gpui::size(px(b.size.width), px(b.size.height)),
    );
    window
        .paint_image(b, b, Corners::all(px(0.)), frame.image.clone(), 0, false)
        .map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn premultiplied_rgba_upload_uses_straight_bgra_once() {
        let frame = CpuFrame {
            screen_bounds: Default::default(),
            width: 3,
            height: 1,
            premul_rgba: vec![128, 0, 0, 128, 0, 0, 64, 64, 0, 0, 0, 0],
        };
        let frame = into_image(frame).unwrap();
        assert_eq!(
            frame.image.as_bytes(0),
            // Locked GPUI conversion truncates the floating unpremultiply by 1.
            Some(&[0, 0, 254, 128, 254, 0, 0, 64, 0, 0, 0, 0][..])
        );
    }

    fn engine() -> SceneEngine {
        use gpui_flow_scene::{BlendMode, DisplayObject, Scene, Style, display};
        let mut s = Scene::new();
        s.insert(
            DisplayObject::new("g", display::Group::new()).with_style(Style {
                blend_mode: BlendMode::Normal,
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
        s.insert(
            DisplayObject::new("r", display::Rect::new(20., 20.)),
            Some("g".into()),
            None,
        )
        .unwrap();
        SceneEngine::new(s).unwrap()
    }
    fn key(e: &SceneEngine) -> Arc<RequestKey> {
        use gpui_flow_scene::{Affine, Bounds, Point, Size};
        let domains = gpui_flow_composite::scene_units(e.scene(), CompositeLimits::default())
            .unwrap()
            .into_iter()
            .filter_map(|u| {
                if let gpui_flow_composite::SceneUnit::Domain(d) = u {
                    Some(*d)
                } else {
                    None
                }
            })
            .collect();
        let b = Bounds::new(Point::ZERO, Size::new(100., 100.));
        Arc::new(RequestKey::new(
            domains,
            e,
            RasterViewport {
                world_to_screen: Affine::IDENTITY,
                canvas_bounds: b,
                content_mask: b,
                device_scale: 1.,
            },
            1,
        ))
    }
    fn ready(c: &mut CompositeController, k: Arc<RequestKey>) -> ImageFrame {
        c.request(k.clone());
        let work = c.take_work().unwrap();
        let result = work.run();
        c.finish(k, result);
        c.frame(&"g".into()).unwrap().0
    }
    #[test]
    fn one_worker_keeps_only_latest_request_and_discards_late_completion() {
        let mut e = engine();
        let mut c = CompositeController::default();
        let first = key(&e);
        c.request(first.clone());
        let work = c.take_work().unwrap();
        e.set_property(
            e.handle(&"r".into()).unwrap(),
            gpui_flow_engine::PropertyKey::Opacity,
            serde_json::json!(0.5),
        )
        .unwrap();
        let second = key(&e);
        c.request(second.clone());
        assert!(work.cancel.load(Ordering::Relaxed));
        assert!(c.take_work().is_none());
        c.finish(first, work.run());
        assert!(c.frame(&"g".into()).is_none());
        let next = c.take_work().unwrap();
        assert_eq!(next.key, second);
    }
    #[test]
    fn upload_acceptance_is_explicit_and_failed_candidate_preserves_complete_old_frame() {
        let mut e = engine();
        let mut c = CompositeController::default();
        let first = key(&e);
        let image = ready(&mut c, first.clone());
        assert!(c.accepted.is_empty());
        c.accepted(&first, &"g".into(), &image.image);
        assert_eq!(c.accepted.len(), 1);
        e.set_property(
            e.handle(&"r".into()).unwrap(),
            gpui_flow_engine::PropertyKey::Opacity,
            serde_json::json!(0.5),
        )
        .unwrap();
        let second = key(&e);
        let candidate = ready(&mut c, second.clone());
        assert!(!Arc::ptr_eq(&image.image, &candidate.image));
        c.upload_failed(&second, &"g".into(), "simulated atlas failure".into());
        let old = c.frame(&"g".into()).unwrap();
        assert!(!old.1);
        assert!(Arc::ptr_eq(&old.0.image, &image.image));
        assert!(
            c.diagnostics()
                .iter()
                .any(|e| e.error == CompositeError::StaleFrame)
        );
    }
    #[test]
    fn geometry_document_window_and_attachment_changes_cannot_reuse_old_frame() {
        let mut e = engine();
        let first = key(&e);
        let mut changed = (*first).clone();
        changed.viewport.device_scale = 2.;
        assert!(!first.compatible(&changed, &"g".into()));
        changed = (*first).clone();
        changed.window = 2;
        assert!(!first.compatible(&changed, &"g".into()));
        e.set_property(
            e.handle(&"r".into()).unwrap(),
            gpui_flow_engine::PropertyKey::Transform,
            serde_json::to_value(gpui_flow_scene::Affine::translation(2., 3.)).unwrap(),
        )
        .unwrap();
        assert!(!first.compatible(&key(&e), &"g".into()));
        let doc = e.scene().clone();
        e.reset_scene(doc).unwrap();
        assert!(!first.compatible(&key(&e), &"g".into()));
        let mut different_owner = engine();
        assert!(!key(&different_owner).compatible(&key(&engine()), &"g".into()));
        let before = key(&different_owner);
        let (subtree, _) = different_owner
            .detach(different_owner.handle(&"g".into()).unwrap())
            .unwrap();
        different_owner.attach(&subtree, None, None).unwrap();
        assert!(!before.compatible(&key(&different_owner), &"g".into()));
    }
    #[test]
    fn retirement_pressure_blocks_candidate_before_any_upload_and_preserves_budget() {
        let e = engine();
        let mut c = CompositeController::default();
        let first = key(&e);
        let image = ready(&mut c, first.clone());
        c.accepted(&first, &"g".into(), &image.image);
        let mut next = (*first).clone();
        next.revision += 1;
        let candidate = ready(&mut c, Arc::new(next));
        c.retired_bytes = MAX_RETIRED;
        let frame = c.frame(&"g".into()).unwrap();
        assert!(!frame.1);
        assert!(!Arc::ptr_eq(&frame.0.image, &candidate.image));
        assert!(c.held_bytes() <= 128 * 1024 * 1024);
    }

    #[test]
    fn returning_to_same_revision_request_after_supersession_starts_fresh_work() {
        let e = engine();
        let mut c = CompositeController::default();
        let a = key(&e);
        let candidate_a = ready(&mut c, a.clone());
        assert!(c.accepted.is_empty());
        let mut other = (*a).clone();
        other.viewport.world_to_screen.tx = 3.;
        let b = Arc::new(other);
        assert_eq!(a.revision, b.revision);
        c.request(b.clone());
        let work_b = c.take_work().unwrap();
        c.request(a.clone());
        assert!(c.pending());
        assert!(c.frame(&"g".into()).is_none());
        assert!(c.take_work().is_none());
        c.finish(b, work_b.run());
        let work_a = c
            .take_work()
            .expect("return to A must reraster cleared candidates");
        assert_eq!(work_a.key, a);
        let result = work_a.run();
        c.finish(a.clone(), result);
        let new = c.frame(&"g".into()).unwrap().0;
        assert!(!Arc::ptr_eq(&candidate_a.image, &new.image));
        c.accepted(&a, &"g".into(), &new.image);
        assert!(!c.pending());
        assert!(c.frame(&"g".into()).is_some());
    }
    #[test]
    fn returning_after_failure_clears_failed_completion_and_upload_block() {
        let e = engine();
        let mut c = CompositeController::default();
        let a = key(&e);
        let old = ready(&mut c, a.clone());
        c.accepted(&a, &"g".into(), &old.image);
        c.upload_failed(&a, &"g".into(), "failure".into());
        c.block_upload("g".into());
        assert!(c.frame(&"g".into()).is_none());
        let mut other = (*a).clone();
        other.viewport.device_scale = 2.;
        let b = Arc::new(other);
        let removed = c.request(b.clone());
        assert_eq!(removed.len(), 1);
        c.request(a.clone());
        assert!(c.errors.is_empty());
        assert!(c.blocked_uploads.is_empty());
        let work = c
            .take_work()
            .expect("failed A may retry after a changed request");
        let result = work.run();
        c.finish(a, result);
        assert!(c.frame(&"g".into()).unwrap().1);
    }
    fn input_image(frames: usize) -> Arc<RenderImage> {
        Arc::new(RenderImage::new(
            (0..frames)
                .map(|_| {
                    image::Frame::new(
                        image::RgbaImage::from_raw(1, 1, vec![0, 0, 255, 128]).unwrap(),
                    )
                })
                .collect::<Vec<_>>(),
        ))
    }
    #[test]
    fn static_gpui_adapter_rejects_animated_empty_and_over_budget_before_copy() {
        let mut bytes = 0;
        assert!(matches!(
            static_image(&input_image(2), &mut bytes, MAX_RETAINED),
            Err(CompositeError::UnsupportedAnimatedImage)
        ));
        assert!(matches!(
            static_image(&input_image(0), &mut bytes, MAX_RETAINED),
            Err(CompositeError::InvalidImage)
        ));
        assert!(matches!(
            static_image(&input_image(1), &mut bytes, 3),
            Err(CompositeError::WorkingSetLimit)
        ));
        assert_eq!(bytes, 0);
        assert_eq!(
            static_image(&input_image(1), &mut bytes, 4)
                .unwrap()
                .premultiplied_rgba(),
            &[128, 0, 0, 128]
        );
        assert_eq!(bytes, 4);
    }
    #[test]
    fn cancelled_running_a_cannot_settle_a_after_a_b_a() {
        let e = engine();
        let mut c = CompositeController::default();
        let a = key(&e);
        c.request(a.clone());
        let work = c.take_work().unwrap();
        let mut b = (*a).clone();
        b.viewport.device_scale = 2.;
        c.request(Arc::new(b));
        c.request(a.clone());
        c.finish(a.clone(), work.run());
        assert!(c.pending());
        let replacement = c
            .take_work()
            .expect("cancelled A must not mark returned A completed");
        c.finish(a, replacement.run());
        assert!(c.frame(&"g".into()).is_some());
    }
    #[test]
    fn image_content_identity_and_pending_failure_never_reuse_old_pixels() {
        let mut e = engine();
        e.transaction(|s| {
            s.update_object(&"r".into(), |o| {
                o.shape = gpui_flow_scene::display::Image::new(
                    "fixture.png",
                    gpui_flow_scene::Size::new(20., 20.),
                )
                .into()
            })
        })
        .unwrap();
        let mut first = (*key(&e)).clone();
        first.domains[0].images.insert(
            "r".into(),
            crate::resources::CompositeImageObservation {
                ticket: None,
                image: Ok(input_image(1)),
            },
        );
        let first = Arc::new(first);
        let mut c = CompositeController::default();
        let frame = ready(&mut c, first.clone());
        c.accepted(&first, &"g".into(), &frame.image);
        for state in [
            Ok(input_image(1)),
            Err(CompositeError::ImagePending("r".into())),
            Err(CompositeError::ImageUnavailable {
                image: "r".into(),
                reason: "not authorized".into(),
            }),
        ] {
            let mut next = (*first).clone();
            next.domains[0].images.get_mut(&"r".into()).unwrap().image = state;
            assert_ne!(next, *first);
            assert!(!first.compatible(&next, &"g".into()));
            c.request(Arc::new(next));
            assert!(c.old_frame(&"g".into()).is_none());
        }
    }

    #[test]
    fn shared_source_and_dropped_output_do_not_starve_later_domains() {
        use gpui_flow_scene::{
            BlendMode, Bounds, DisplayObject, Point, Scene, Size, Style, display,
        };
        let mut scene = Scene::new();
        for (index, (width, height)) in [
            (2047., 2047.),
            (1023., 1023.),
            (3199., 1023.),
            (3071., 1023.),
        ]
        .into_iter()
        .enumerate()
        {
            let group = format!("g{index}");
            scene
                .insert(
                    DisplayObject::new(group.clone(), display::Group::new()).with_style(Style {
                        blend_mode: BlendMode::Normal,
                        ..Default::default()
                    }),
                    None,
                    None,
                )
                .unwrap();
            let mut image = display::Image::new("shared.png", Size::new(width, height));
            image.fit = display::ImageFit::Fill;
            scene
                .insert(
                    DisplayObject::new(format!("i{index}"), image),
                    Some(group.into()),
                    None,
                )
                .unwrap();
        }
        let e = SceneEngine::new(scene).unwrap();
        let mut request = (*key(&e)).clone();
        let bounds = Bounds::new(Point::ZERO, Size::new(4096., 4096.));
        request.viewport.canvas_bounds = bounds;
        request.viewport.content_mask = bounds;
        let shared = Arc::new(RenderImage::new(vec![image::Frame::new(
            image::RgbaImage::new(2048, 1024),
        )]));
        for (index, domain) in request.domains.iter_mut().enumerate() {
            domain.images.insert(
                format!("i{index}").into(),
                crate::resources::CompositeImageObservation {
                    ticket: None,
                    image: Ok(shared.clone()),
                },
            );
        }
        let work = Work {
            key: Arc::new(request),
            cancel: Arc::new(AtomicBool::new(false)),
            retained: 64 * 1024 * 1024,
            settled: BTreeSet::new(),
            settled_bytes: 0,
        };
        let result = work.run();
        assert_eq!(result.len(), 4);
        assert_eq!(
            result[0]
                .1
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .premul_rgba
                .len(),
            16 * 1024 * 1024
        );
        assert_eq!(
            result[1]
                .1
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .premul_rgba
                .len(),
            4 * 1024 * 1024
        );
        assert!(matches!(result[2].1, Err(CompositeError::WorkingSetLimit)));
        assert_eq!(
            result[3]
                .1
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .premul_rgba
                .len(),
            12 * 1024 * 1024
        );
    }

    #[test]
    fn budget_retry_preserves_upload_failure_and_observes_candidate_release() {
        for release_while_running in [false, true] {
            let e = engine();
            let mut request = (*key(&e)).clone();
            // Use two real domains, then simulate the platform-only upload
            // failure and a worker's conservative memory-limit result.
            let mut scene = e.scene().clone();
            scene
                .insert(
                    gpui_flow_scene::DisplayObject::new(
                        "g2",
                        gpui_flow_scene::display::Group::new(),
                    )
                    .with_style(gpui_flow_scene::Style {
                        blend_mode: gpui_flow_scene::BlendMode::Normal,
                        ..Default::default()
                    }),
                    None,
                    None,
                )
                .unwrap();
            let e = SceneEngine::new(scene).unwrap();
            request.domains = key(&e).domains.clone();
            let request = Arc::new(request);
            let mut c = CompositeController::default();
            c.request(request.clone());
            let work = c.take_work().unwrap();
            let mut result = work.run();
            result[1].1 = Err(CompositeError::WorkingSetLimit);
            c.finish(request.clone(), result);
            assert!(c.frame(&"g".into()).is_some());
            let running = if release_while_running {
                c.retired_bytes = 4;
                c.release_retired(4);
                Some(c.take_work().unwrap())
            } else {
                None
            };
            c.upload_failed(&request, &"g".into(), "atlas failure".into());
            c.block_upload("g".into());
            if let Some(work) = running {
                assert!(work.settled.contains(&"g".into()));
                c.finish(
                    request.clone(),
                    vec![("g2".into(), Err(CompositeError::WorkingSetLimit))],
                );
            }
            let retry = c
                .take_work()
                .expect("candidate release creates a new budget epoch");
            assert!(retry.settled.contains(&"g".into()));
            let result = retry.run();
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].0.as_str(), "g2");
            c.finish(request.clone(), result);
            assert!(c.frame(&"g".into()).is_none());
            assert!(
                c.diagnostics()
                    .iter()
                    .any(|issue| issue.object.as_str() == "g"
                        && matches!(issue.error, CompositeError::UploadFailed(_)))
            );
            assert!(c.take_work().is_none());
        }
    }

    #[test]
    fn real_mixed_budget_retry_preserves_success_without_retirement_loop() {
        use gpui_flow_scene::{BlendMode, DisplayObject, Size, Style, display};
        for accept_before_retry in [false, true] {
            let mut e = engine();
            let mut c = CompositeController::default();
            let previous = key(&e);
            let previous_image = ready(&mut c, previous.clone());
            c.accepted(&previous, &"g".into(), &previous_image.image);
            e.transaction(|s| {
                s.insert(
                    DisplayObject::new("g2", display::Group::new()).with_style(Style {
                        blend_mode: BlendMode::Normal,
                        ..Default::default()
                    }),
                    None,
                    None,
                )?;
                s.insert(
                    DisplayObject::new(
                        "large",
                        display::Image::new("large.png", Size::new(20., 20.)),
                    ),
                    Some("g2".into()),
                    None,
                )
            })
            .unwrap();
            let mut current = (*key(&e)).clone();
            current
                .domains
                .iter_mut()
                .find(|d| d.source.root_id().as_str() == "g2")
                .unwrap()
                .images
                .insert(
                    "large".into(),
                    crate::resources::CompositeImageObservation {
                        ticket: None,
                        // A real decoded input exceeding the static conversion cap.
                        // This domain remains memory-limited even after all retired
                        // images have been released; its failure is not mocked.
                        image: Ok(Arc::new(RenderImage::new(vec![image::Frame::new(
                            image::RgbaImage::new(3000, 3000),
                        )]))),
                    },
                );
            let current = Arc::new(current);
            let good = ready(&mut c, current.clone());
            assert!(
                c.errors.iter().any(
                    |i| i.object.as_str() == "g2" && i.error == CompositeError::WorkingSetLimit
                )
            );
            if accept_before_retry {
                let old = c.accepted(&current, &"g".into(), &good.image).unwrap();
                assert!(Arc::ptr_eq(&old.image, &previous_image.image));
                c.retired_bytes += old.bytes;
                c.release_retired(old.bytes);
            } else {
                // Another earlier frame's real release can arrive before g is
                // uploaded. Its candidate must survive this retry as well.
                c.retired_bytes = previous_image.bytes;
                c.release_retired(previous_image.bytes);
            }
            let retry = c
                .take_work()
                .expect("released budget retries the failed domain");
            let result = retry.run();
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].0.as_str(), "g2");
            assert!(matches!(result[0].1, Err(CompositeError::WorkingSetLimit)));
            c.finish(current.clone(), result);
            let (preserved, candidate) = c.frame(&"g".into()).unwrap();
            assert!(Arc::ptr_eq(&preserved.image, &good.image));
            assert_eq!(candidate, !accept_before_retry);
            if !accept_before_retry {
                let old = c.accepted(&current, &"g".into(), &preserved.image).unwrap();
                c.retired_bytes += old.bytes;
                c.release_retired(old.bytes);
                let final_retry = c.take_work().unwrap();
                c.finish(current.clone(), final_retry.run());
                assert!(Arc::ptr_eq(
                    &c.frame(&"g".into()).unwrap().0.image,
                    &good.image
                ));
            }
            assert!(!c.pending());
            assert!(
                c.take_work().is_none(),
                "successful domains cannot manufacture new retirements"
            );
        }
    }

    #[test]
    fn retirement_budget_failure_retries_only_after_a_real_release() {
        for release_before_finish in [false, true] {
            let e = engine();
            let mut c = CompositeController::default();
            let k = key(&e);
            c.retired_bytes = 4;
            c.request(k.clone());
            let _work = c.take_work().unwrap();
            if release_before_finish {
                c.release_retired(4);
            }
            c.finish(
                k.clone(),
                vec![("g".into(), Err(CompositeError::WorkingSetLimit))],
            );
            if !release_before_finish {
                assert!(!c.pending());
                assert!(c.take_work().is_none());
                c.release_retired(0);
                assert!(!c.pending());
                c.release_retired(4);
            }
            let retry = c.take_work().expect("real budget release should retry");
            c.finish(
                k.clone(),
                vec![("g".into(), Err(CompositeError::WorkingSetLimit))],
            );
            assert!(!c.pending());
            assert!(c.take_work().is_none());
            drop(retry);
        }
    }
}

//! Per-SceneView CPU work queue and real GPUI image upload. No global caches.
use gpui::{App, Bounds, Corners, RenderImage, Window, px};
use gpui_flow_composite::{
    CompositeError, CompositeIssue, CompositeLimits, CpuFrame, DomainSource, PixelBudget,
    RasterViewport, rasterize,
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
                DomainKey { source, identities }
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
                    a.identities == b.identities && a.source.placement_eq(&b.source)
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
        }
    }
}
pub(crate) struct Work {
    pub key: Arc<RequestKey>,
    pub cancel: Arc<AtomicBool>,
    retained: u64,
}
pub(crate) type WorkResult = Vec<(DisplayId, Result<Option<CpuFrame>, CompositeError>)>;
impl Work {
    pub fn run(&self) -> WorkResult {
        let mut budget = match PixelBudget::new(CompositeLimits::default(), self.retained) {
            Ok(b) => b,
            Err(e) => {
                return self
                    .key
                    .domains
                    .iter()
                    .map(|d| (d.source.root_id().clone(), Err(e.clone())))
                    .collect();
            }
        };
        let mut retained = 0;
        self.key
            .domains
            .iter()
            .enumerate()
            .map(|(index, d)| {
                let result = if index >= MAX_ENTRIES {
                    Err(CompositeError::InputLimit)
                } else {
                    rasterize(&d.source, self.key.viewport, &mut budget, &self.cancel).and_then(
                        |f| {
                            let bytes = f.as_ref().map_or(0, |f| f.premul_rgba.len() as u64);
                            if retained + bytes > MAX_RETAINED {
                                Err(CompositeError::WorkingSetLimit)
                            } else {
                                retained += bytes;
                                Ok(f)
                            }
                        },
                    )
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
        self.running = true;
        self.cancel = Arc::new(AtomicBool::new(false));
        Some(Work {
            key: key.clone(),
            cancel: self.cancel.clone(),
            retained: self.held_bytes(),
        })
    }
    pub fn finish(&mut self, key: Arc<RequestKey>, result: WorkResult) {
        self.running = false;
        if self.requested.as_ref() != Some(&key) {
            return;
        }
        self.completed = Some(key);
        self.candidates.clear();
        self.errors.clear();
        for (id, result) in result {
            match result.and_then(|frame| frame.map(into_image).transpose()) {
                Ok(frame) => {
                    self.candidates.insert(id, frame);
                }
                Err(error) => self.errors.push(CompositeIssue { object: id, error }),
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
        self.candidates.remove(id);
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
                this.compositing.retired_bytes =
                    this.compositing.retired_bytes.saturating_sub(frame.bytes);
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
}

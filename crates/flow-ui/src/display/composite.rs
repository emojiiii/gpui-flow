//! Per-SceneView CPU work queue and real GPUI image upload. No global caches.
use gpui::{App, Bounds, Corners, RenderImage, Window, px};
use gpui_flow_composite::{
    CompositeError, CompositeIssue, CompositeLimits, CpuFrame, DomainSource, ImageInputs,
    PixelBudget, RasterViewport, StaticImage, TextInputs, rasterize_with_inputs,
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

use super::composite_text::{TextBackendSnapshot, TextPreparation};

const MAX_ENTRIES: usize = 8;
const MAX_FAILED_SAMPLES: usize = 128;
const MAX_RETAINED: u64 = 32 * 1024 * 1024;
const MAX_RETIRED: u64 = 32 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DomainKey {
    pub source: DomainSource,
    identities: Vec<(ElementHandle, u64)>,
    text_tickets: Vec<Option<gpui_flow_engine::ResourceTicket>>,
    images: BTreeMap<DisplayId, crate::resources::CompositeImageObservation>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RequestKey {
    pub domains: Vec<DomainKey>,
    pub viewport: RasterViewport,
    pub window: u64,
    revision: u64,
    document_epoch: u64,
    text_backend: Option<TextBackendSnapshot>,
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
                    text_tickets: vec![],
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
            text_backend: None,
        }
    }
    pub fn with_resources(
        mut self,
        engine: &SceneEngine,
        resources: Option<&crate::resources::SceneResourceAdapter>,
    ) -> Self {
        for domain in &mut self.domains {
            domain.text_tickets = domain
                .source
                .texts()
                .iter()
                .map(|(object, _)| {
                    engine
                        .handle(&object.id)
                        .and_then(|h| resources?.tracker().ticket(engine, h))
                })
                .collect();
            for (id, canvas) in domain.source.canvases() {
                let observation = resources
                    .map(|r| r.composite_canvas(engine, id, canvas))
                    .unwrap_or_else(|| crate::resources::CompositeImageObservation {
                        ticket: None,
                        image: Err(CompositeError::ImagePending(id.clone())),
                        frame_index: 0,
                        playback_generation: 0,
                    });
                domain.images.insert(id.clone(), observation);
            }
            for (id, image) in domain.source.images() {
                let observation = resources
                    .map(|r| r.composite_image(engine, id, &image.source))
                    .unwrap_or_else(|| crate::resources::CompositeImageObservation {
                        ticket: None,
                        image: Err(CompositeError::ImagePending(id.clone())),
                        frame_index: 0,
                        playback_generation: 0,
                    });
                domain.images.insert(id.clone(), observation);
            }
        }
        self
    }
    pub fn with_text_backend(
        mut self,
        bridge: Option<&Arc<gpui_flow_text::GpuiTextRasterizer>>,
    ) -> Self {
        if self.domains.iter().any(|d| !d.source.texts().is_empty()) {
            self.text_backend = bridge.map(TextBackendSnapshot::new);
        }
        self
    }
    pub fn text_current(&self) -> bool {
        self.text_backend
            .as_ref()
            .is_none_or(TextBackendSnapshot::current)
    }
    // Frame selection is soft invalidation. Everything else is semantic and
    // cancels stale work, including a seek/pause generation and A/B/A resources.
    fn animation_only(&self, other: &Self) -> bool {
        self.text_backend == other.text_backend
            && self.window == other.window
            && self.viewport == other.viewport
            && self.document_epoch == other.document_epoch
            && self.revision == other.revision
            && self.domains.len() == other.domains.len()
            && self.domains.iter().zip(&other.domains).all(|(a, b)| {
                a.source == b.source
                    && a.text_tickets == b.text_tickets
                    && a.identities == b.identities
                    && same_resources(&a.images, &b.images)
            })
    }
    fn sample(&self, id: &DisplayId) -> Vec<usize> {
        self.domains
            .iter()
            .find(|d| d.source.root_id() == id)
            .map(|d| d.images.values().map(|i| i.frame_index).collect())
            .unwrap_or_default()
    }
    fn domain_semantic_eq(&self, other: &Self, id: &DisplayId) -> bool {
        self.text_backend == other.text_backend
            && self
                .domains
                .iter()
                .find(|d| d.source.root_id() == id)
                .zip(other.domains.iter().find(|d| d.source.root_id() == id))
                .is_some_and(|(a, b)| {
                    a.source == b.source
                        && a.text_tickets == b.text_tickets
                        && a.identities == b.identities
                        && same_resources(&a.images, &b.images)
                })
    }
    fn same_domain_sample(&self, other: &Self, id: &DisplayId) -> bool {
        self.text_backend == other.text_backend
            && self.window == other.window
            && self.viewport == other.viewport
            && self.document_epoch == other.document_epoch
            && self.domains.iter().find(|d| d.source.root_id() == id)
                == other.domains.iter().find(|d| d.source.root_id() == id)
    }
    fn compatible(&self, other: &Self, id: &DisplayId) -> bool {
        self.text_backend == other.text_backend
            && self.window == other.window
            && self.viewport == other.viewport
            && self.document_epoch == other.document_epoch
            && self
                .domains
                .iter()
                .find(|d| d.source.root_id() == id)
                .zip(other.domains.iter().find(|d| d.source.root_id() == id))
                .is_some_and(|(a, b)| {
                    a.text_tickets == b.text_tickets
                        && a.identities == b.identities
                        && same_resources(&a.images, &b.images)
                        && a.source.placement_eq(&b.source)
                })
    }
}
fn same_resources(
    a: &BTreeMap<DisplayId, crate::resources::CompositeImageObservation>,
    b: &BTreeMap<DisplayId, crate::resources::CompositeImageObservation>,
) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|((id, a), (other, b))| id == other && a.resource_eq(b))
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
    // Frame-local failures are memoized by selected-frame vector, not promoted
    // to resource-wide failure. Metadata stays bounded even for many images.
    failed_samples: BTreeMap<DisplayId, BTreeMap<Vec<usize>, (u64, CompositeError)>>,
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
            failed_samples: BTreeMap::new(),
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
// Convert only the admitted frame snapshot. GPUI still owns decoded originals;
// never clone all frames, and share a (resource, frame) conversion within a job.
fn selected_image(
    image: &RenderImage,
    frame_index: usize,
    bytes: &mut u64,
    capacity: u64,
) -> Result<Arc<StaticImage>, CompositeError> {
    if frame_index >= image.frame_count() {
        return Err(CompositeError::InvalidImage);
    }
    let size = image.size(frame_index);
    let data = image
        .as_bytes(frame_index)
        .ok_or(CompositeError::InvalidImage)?;
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
        let mut converted: BTreeMap<
            (gpui::ImageId, usize),
            Result<Arc<StaticImage>, CompositeError>,
        > = BTreeMap::new();
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
                        let snapshot = (image.id, observation.frame_index);
                        if let Some(input) = converted.get(&snapshot) {
                            return input.clone();
                        }
                        let capacity = CompositeLimits::default()
                            .max_surface_bytes
                            .saturating_sub(self.retained);
                        let input = selected_image(
                            image,
                            observation.frame_index,
                            &mut source_bytes,
                            capacity,
                        );
                        converted.insert(snapshot, input.clone());
                        input
                    });
                images.insert(id.clone(), input);
            }
        }
        let mut texts = TextInputs::default();
        let mut remaining_work = CompositeLimits::default().max_surface_writes;
        {
            let mut producer = TextPreparation {
                backend: self.key.text_backend.as_ref(),
                viewport: self.key.viewport,
                cancelled: &self.cancel,
                retained: self.retained,
                source_bytes: &mut source_bytes,
                remaining_work: &mut remaining_work,
            };
            for domain in &self.key.domains {
                if !self.settled.contains(domain.source.root_id()) {
                    producer.domain(&domain.source, &mut texts);
                }
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
        // Glyph production and compositor writes share the request work ceiling.
        if let Err(e) =
            budget.charge_input_work(CompositeLimits::default().max_surface_writes - remaining_work)
        {
            return self
                .key
                .domains
                .iter()
                .filter(|d| !self.settled.contains(d.source.root_id()))
                .map(|d| (d.source.root_id().clone(), Err(e.clone())))
                .collect();
        }
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
                    rasterize_with_inputs(
                        &d.source,
                        &images,
                        &texts,
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
fn frame_local_error(error: &CompositeError) -> bool {
    matches!(
        error,
        CompositeError::WorkingSetLimit | CompositeError::InvalidImage | CompositeError::TooLarge
    )
}
impl CompositeController {
    /// Replaces the one pending request; an old worker is only cooperatively
    /// cancelled and must finish before another worker can start.
    pub fn request(&mut self, key: Arc<RequestKey>) -> Vec<ImageFrame> {
        if self.requested.as_ref() == Some(&key) {
            return vec![];
        }
        let animation_only = self
            .requested
            .as_ref()
            .is_some_and(|old| key.animation_only(old));
        if animation_only && (self.running || self.candidates.values().any(Option::is_some)) {
            // One admitted snapshot can finish and upload even when every newer
            // vsync chooses another frame. The next render samples latest time;
            // there is no per-tick queue and no starvation of a slow worker.
            return vec![];
        }
        self.cancel.store(true, Ordering::Relaxed);
        // A -> B -> A may return to the same presented values/revision after
        // A's unaccepted candidate was cleared. Completion is request-local.
        self.completed = None;
        self.candidates.clear();
        if !animation_only {
            self.errors.clear();
            self.blocked_uploads.clear();
            self.failed_samples.clear();
        } else {
            self.errors.retain(|issue| !frame_local_error(&issue.error));
            for domain in &key.domains {
                let id = domain.source.root_id();
                if let Some((epoch, error)) = self
                    .failed_samples
                    .get(id)
                    .and_then(|f| f.get(&key.sample(id)))
                    && (*error != CompositeError::WorkingSetLimit || *epoch == self.budget_epoch)
                {
                    self.errors.push(CompositeIssue {
                        object: id.clone(),
                        error: error.clone(),
                    });
                }
            }
        }
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
        if !key.text_current() || self.running || self.completed.as_ref() == Some(key) {
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
            } else if let Some(accepted) = self
                .accepted
                .get(id)
                .filter(|a| key.same_domain_sample(&a.key, id))
            {
                settled.insert(id.clone());
                settled_bytes += accepted.frame.bytes;
            } else if self.blocked_uploads.contains(id)
                || self.errors.iter().any(|issue| {
                    &issue.object == id
                        && (issue.error != CompositeError::WorkingSetLimit
                            || self
                                .failed_samples
                                .get(id)
                                .and_then(|f| f.get(&key.sample(id)))
                                .is_some_and(|(epoch, _)| *epoch == self.budget_epoch))
                })
            {
                // Only budget failures may change after a byte release. Preserve
                // terminal diagnostics and never bypass an upload block here.
                settled.insert(id.clone());
            }
        }
        if settled.len() == key.domains.len() {
            self.completed = Some(key.clone());
            return None;
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
        if !key.text_current()
            || self.requested.as_ref() != Some(&key)
            || self.cancel.load(Ordering::Relaxed)
        {
            return;
        }
        self.completed = Some(key.clone());
        for (id, result) in result {
            self.errors.retain(|issue| issue.object != id);

            match result.and_then(|frame| frame.map(into_image).transpose()) {
                Ok(frame) => {
                    self.candidates.insert(id, frame);
                }
                Err(error) => {
                    let mut error = error;
                    if frame_local_error(&error) {
                        let failures = self.failed_samples.entry(id.clone()).or_default();
                        let sample = key.sample(&id);
                        if failures.len() >= MAX_FAILED_SAMPLES && !failures.contains_key(&sample) {
                            error = CompositeError::InputLimit;
                        } else {
                            failures.insert(sample, (self.running_budget_epoch, error.clone()));
                        }
                    }
                    self.errors.push(CompositeIssue { object: id, error });
                }
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
    pub(crate) fn can_animate_domain(&self, id: &DisplayId) -> bool {
        !self.blocked_uploads.contains(id)
            && !self
                .errors
                .iter()
                .any(|issue| &issue.object == id && !frame_local_error(&issue.error))
    }
    pub fn diagnostics(&self) -> Vec<CompositeIssue> {
        let mut out = self.errors.clone();
        if let Some(key) = &self.requested {
            for (id, old) in &self.accepted {
                if old.key != *key
                    && key.compatible(&old.key, id)
                    && !key.domain_semantic_eq(&old.key, id)
                {
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
    /// Background refresh is not a loading interruption when every domain can
    /// still paint a compatible frame. Keep `pending` available for scheduling
    /// and callers, but do not flash a loading badge on every animation tick.
    /// `frame` enforces resource, font, placement and upload invalidation; this
    /// must never resurrect pixels that those checks deliberately withdrew.
    pub fn loading_visible(&self) -> bool {
        self.pending()
            && self.requested.as_ref().is_some_and(|key| {
                key.domains
                    .iter()
                    .any(|domain| self.frame(domain.source.root_id()).is_none())
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
        if self.requested.as_ref().is_some_and(|k| !k.text_current())
            || self.blocked_uploads.contains(id)
        {
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
        if !key.text_current() || self.requested.as_ref() != Some(key) {
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
        // An accepted domain must not retain decoded images from unrelated,
        // later removed/revoked domains in the same request batch.
        let mut domain_key = (**key).clone();
        domain_key.domains.retain(|d| d.source.root_id() == id);
        self.accepted
            .insert(
                id.clone(),
                Accepted {
                    key: Arc::new(domain_key),
                    frame,
                },
            )
            .map(|a| a.frame)
    }
    pub fn upload_failed(&mut self, key: &Arc<RequestKey>, id: &DisplayId, error: String) {
        if !key.text_current() || self.requested.as_ref() != Some(key) {
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
    fn root_and_nested_container_order_edits_hide_accepted_frames_and_reject_late_work() {
        use gpui_flow_engine::PropertyKey;
        use gpui_flow_scene::{Color, ContainerPaintOrder, DisplayObject, Shape, Stroke, display};

        for target in ["g", "r"] {
            let mut e = engine();
            e.transaction(|scene| {
                scene.update_object(&"g".into(), |object| {
                    object.shape = display::Frame::new(30., 30.).into();
                    object.style.stroke = Some(Stroke::new(Color::rgb(0, 255, 0), 8.));
                })?;
                scene.update_object(&"r".into(), |object| {
                    object.shape = display::Box::new(20., 20.).into();
                    object.style.stroke = Some(Stroke::new(Color::WHITE, 6.));
                })?;
                scene.insert(
                    DisplayObject::new("child", display::Rect::new(20., 20.)),
                    Some("r".into()),
                    None,
                )
            })
            .unwrap();
            let mut c = CompositeController::default();
            let first = key(&e);
            let old = ready(&mut c, first.clone());
            let domain: DisplayId = "g".into();
            c.accepted(&first, &domain, &old.image);
            assert_eq!(c.accepted.len(), 1);

            // Admit a paint-only refresh while the old complete image is valid.
            e.set_property(
                e.handle(&"child".into()).unwrap(),
                PropertyKey::Fill,
                serde_json::to_value(gpui_flow_scene::Paint::solid(Color::rgb(0, 0, 255))).unwrap(),
            )
            .unwrap();
            let pending = key(&e);
            assert!(pending.compatible(&first, &domain));
            assert!(c.request(pending.clone()).is_empty());
            let work = c.take_work().unwrap();
            assert!(Arc::ptr_eq(
                &c.old_frame(&domain).unwrap().image,
                &old.image
            ));
            // Finish the pixels before cancellation, but delay delivery so this
            // exercises rejection of a successful obsolete result, not an error.
            let late_result = work.run();
            assert!(matches!(
                late_result.as_slice(),
                [(id, Ok(Some(_)))] if id == &domain
            ));

            let bounds = e.visible_bounds().unwrap();
            let handle = e.handle(&target.into()).unwrap();
            let identity = e.instance(handle).unwrap();
            let mut shape = e.object(handle).unwrap().shape.clone();
            match &mut shape {
                Shape::Box(shape) => shape.paint_order = ContainerPaintOrder::StrokeAfterChildren,
                Shape::Frame(shape) => shape.paint_order = ContainerPaintOrder::StrokeAfterChildren,
                _ => unreachable!(),
            }
            e.set_property(
                handle,
                PropertyKey::Shape,
                serde_json::to_value(shape).unwrap(),
            )
            .unwrap();
            let current = key(&e);
            assert_eq!(e.visible_bounds().unwrap(), bounds);
            assert_eq!(e.instance(handle).unwrap(), identity);
            assert_eq!(current.viewport, pending.viewport);
            assert_eq!(current.domains[0].identities, pending.domains[0].identities);
            assert!(!current.compatible(&first, &domain));
            assert!(!current.compatible(&pending, &domain));
            let retired = c.request(current.clone());
            assert_eq!(retired.len(), 1);
            assert!(Arc::ptr_eq(&retired[0].image, &old.image));
            assert!(work.cancel.load(Ordering::Relaxed));
            assert!(c.old_frame(&domain).is_none());
            assert!(c.frame(&domain).is_none());
            assert!(c.loading_visible());
            assert!(c.take_work().is_none());

            c.finish(pending, late_result);
            assert!(c.candidates.is_empty());
            assert!(c.frame(&domain).is_none());
            assert!(c.loading_visible());
            let fresh = c.take_work().unwrap();
            assert_eq!(fresh.key, current);
            let result = fresh.run();
            c.finish(current.clone(), result);
            let candidate = c.frame(&domain).unwrap();
            assert!(candidate.1);
            assert!(!Arc::ptr_eq(&candidate.0.image, &old.image));
            c.accepted(&current, &domain, &candidate.0.image);
            assert!(Arc::ptr_eq(
                &c.old_frame(&domain).unwrap().image,
                &candidate.0.image
            ));
            assert!(!c.pending());
            assert!(!c.loading_visible());
        }
    }
    #[test]
    fn root_and_nested_container_shadow_edits_hide_accepted_frames_and_reject_late_work() {
        use gpui_flow_engine::PropertyKey;
        use gpui_flow_scene::{Color, DisplayObject, Shadow, Stroke, display};

        for target in ["g", "r"] {
            let mut e = engine();
            e.transaction(|scene| {
                scene.update_object(&"g".into(), |object| {
                    object.shape = display::Frame::new(30., 30.).into();
                    object.style.stroke = Some(Stroke::new(Color::rgb(0, 255, 0), 8.));
                    object.style.shadow = Some(Shadow::new(1., 1., 1., Color::BLACK));
                })?;
                scene.update_object(&"r".into(), |object| {
                    object.shape = display::Box::new(20., 20.).into();
                    object.style.stroke = Some(Stroke::new(Color::WHITE, 6.));
                    object.style.shadow = Some(Shadow::new(1., 1., 1., Color::BLACK));
                })?;
                scene.insert(
                    DisplayObject::new("child", display::Rect::new(20., 20.)),
                    Some("r".into()),
                    None,
                )
            })
            .unwrap();
            let mut c = CompositeController::default();
            let first = key(&e);
            let old = ready(&mut c, first.clone());
            let domain: DisplayId = "g".into();
            c.accepted(&first, &domain, &old.image);
            assert_eq!(c.accepted.len(), 1);

            // Admit a paint-only refresh while the old complete image is valid.
            e.set_property(
                e.handle(&"child".into()).unwrap(),
                PropertyKey::Fill,
                serde_json::to_value(gpui_flow_scene::Paint::solid(Color::rgb(0, 0, 255))).unwrap(),
            )
            .unwrap();
            let pending = key(&e);
            assert!(pending.compatible(&first, &domain));
            assert!(c.request(pending.clone()).is_empty());
            let work = c.take_work().unwrap();
            assert!(Arc::ptr_eq(
                &c.old_frame(&domain).unwrap().image,
                &old.image
            ));
            // Finish the pixels before cancellation, but delay delivery so this
            // exercises rejection of a successful obsolete result, not an error.
            let late_result = work.run();
            assert!(matches!(
                late_result.as_slice(),
                [(id, Ok(Some(_)))] if id == &domain
            ));

            let bounds = e.visible_bounds().unwrap();
            let handle = e.handle(&target.into()).unwrap();
            let identity = e.instance(handle).unwrap();
            let mut shadow = e.object(handle).unwrap().style.shadow.unwrap();
            shadow.x = 2.;
            e.set_property(
                handle,
                PropertyKey::Shadow,
                serde_json::to_value(shadow).unwrap(),
            )
            .unwrap();
            let current = key(&e);
            assert_eq!(e.visible_bounds().unwrap(), bounds);
            assert_eq!(e.instance(handle).unwrap(), identity);
            assert_eq!(current.viewport, pending.viewport);
            assert_eq!(current.domains[0].identities, pending.domains[0].identities);
            assert!(!current.compatible(&first, &domain));
            assert!(!current.compatible(&pending, &domain));
            let retired = c.request(current.clone());
            assert_eq!(retired.len(), 1);
            assert!(Arc::ptr_eq(&retired[0].image, &old.image));
            assert!(work.cancel.load(Ordering::Relaxed));
            assert!(c.old_frame(&domain).is_none());
            assert!(c.frame(&domain).is_none());
            assert!(c.loading_visible());
            assert!(c.take_work().is_none());

            c.finish(pending, late_result);
            assert!(c.candidates.is_empty());
            assert!(c.frame(&domain).is_none());
            assert!(c.loading_visible());
            let fresh = c.take_work().unwrap();
            assert_eq!(fresh.key, current);
            let result = fresh.run();
            c.finish(current.clone(), result);
            let candidate = c.frame(&domain).unwrap();
            assert!(candidate.1);
            assert!(!Arc::ptr_eq(&candidate.0.image, &old.image));
            c.accepted(&current, &domain, &candidate.0.image);
            assert!(Arc::ptr_eq(
                &c.old_frame(&domain).unwrap().image,
                &candidate.0.image
            ));
            assert!(!c.pending());
            assert!(!c.loading_visible());
        }
    }
    #[test]
    fn additional_container_shadow_layers_hide_accepted_frames_and_reject_late_work() {
        use gpui_flow_engine::PropertyKey;
        use gpui_flow_scene::{Color, DisplayObject, Shadow, Stroke, display};

        for target in ["g", "r"] {
            let mut e = engine();
            e.transaction(|scene| {
                scene.update_object(&"g".into(), |object| {
                    object.shape = display::Frame::new(30., 30.).into();
                    object.style.stroke = Some(Stroke::new(Color::rgb(0, 255, 0), 8.));
                    object.style.shadow = Some(Shadow::new(1., 1., 1., Color::BLACK));
                })?;
                scene.update_object(&"r".into(), |object| {
                    object.shape = display::Box::new(20., 20.).into();
                    object.style.stroke = Some(Stroke::new(Color::WHITE, 6.));
                    object.style.shadow = Some(Shadow::new(1., 1., 1., Color::BLACK));
                })?;
                scene.insert(
                    DisplayObject::new("child", display::Rect::new(20., 20.)),
                    Some("r".into()),
                    None,
                )
            })
            .unwrap();
            let mut c = CompositeController::default();
            let first = key(&e);
            let old = ready(&mut c, first.clone());
            let domain: DisplayId = "g".into();
            c.accepted(&first, &domain, &old.image);
            assert_eq!(c.accepted.len(), 1);

            // Admit a paint-only refresh while the old complete image is valid.
            e.set_property(
                e.handle(&"child".into()).unwrap(),
                PropertyKey::Fill,
                serde_json::to_value(gpui_flow_scene::Paint::solid(Color::rgb(0, 0, 255))).unwrap(),
            )
            .unwrap();
            let pending = key(&e);
            assert!(pending.compatible(&first, &domain));
            assert!(c.request(pending.clone()).is_empty());
            let work = c.take_work().unwrap();
            assert!(Arc::ptr_eq(
                &c.old_frame(&domain).unwrap().image,
                &old.image
            ));
            // Finish the pixels before cancellation, but delay delivery so this
            // exercises rejection of a successful obsolete result, not an error.
            let late_result = work.run();
            assert!(matches!(
                late_result.as_slice(),
                [(id, Ok(Some(_)))] if id == &domain
            ));

            let bounds = e.visible_bounds().unwrap();
            let handle = e.handle(&target.into()).unwrap();
            let identity = e.instance(handle).unwrap();
            let shadows = vec![
                Shadow::new(2., 1., 1., Color::rgba(255, 0, 0, 150)),
                Shadow::new(0., 3., 0., Color::BLACK),
            ];
            e.set_property(
                handle,
                PropertyKey::ShadowLayers,
                serde_json::to_value(shadows).unwrap(),
            )
            .unwrap();
            let current = key(&e);
            assert_eq!(e.visible_bounds().unwrap(), bounds);
            assert_eq!(e.instance(handle).unwrap(), identity);
            assert_eq!(current.viewport, pending.viewport);
            assert_eq!(current.domains[0].identities, pending.domains[0].identities);
            assert!(!current.compatible(&first, &domain));
            assert!(!current.compatible(&pending, &domain));
            let retired = c.request(current.clone());
            assert_eq!(retired.len(), 1);
            assert!(Arc::ptr_eq(&retired[0].image, &old.image));
            assert!(work.cancel.load(Ordering::Relaxed));
            assert!(c.old_frame(&domain).is_none());
            assert!(c.frame(&domain).is_none());
            assert!(c.loading_visible());
            assert!(c.take_work().is_none());

            c.finish(pending, late_result);
            assert!(c.candidates.is_empty());
            assert!(c.frame(&domain).is_none());
            assert!(c.loading_visible());
            let fresh = c.take_work().unwrap();
            assert_eq!(fresh.key, current);
            let result = fresh.run();
            c.finish(current.clone(), result);
            let candidate = c.frame(&domain).unwrap();
            assert!(candidate.1);
            assert!(!Arc::ptr_eq(&candidate.0.image, &old.image));
            c.accepted(&current, &domain, &candidate.0.image);
            assert!(Arc::ptr_eq(
                &c.old_frame(&domain).unwrap().image,
                &candidate.0.image
            ));
            assert!(!c.pending());
            assert!(!c.loading_visible());
        }
    }
    #[test]
    fn inner_layer_edit_retires_accepted_frame_and_rejects_late_success() {
        use gpui_flow_engine::PropertyKey;
        use gpui_flow_scene::{Color, Shadow};
        let mut e = engine();
        let mut c = CompositeController::default();
        let first = key(&e);
        let old = ready(&mut c, first.clone());
        let domain: DisplayId = "g".into();
        c.accepted(&first, &domain, &old.image);
        let handle = e.handle(&"r".into()).unwrap();
        e.set_property(
            handle,
            PropertyKey::Fill,
            serde_json::to_value(gpui_flow_scene::Paint::solid(Color::rgb(0, 0, 255))).unwrap(),
        )
        .unwrap();
        let pending = key(&e);
        assert!(pending.compatible(&first, &domain));
        c.request(pending.clone());
        let work = c.take_work().unwrap();
        let late = work.run();
        let bounds = e.visible_bounds().unwrap();
        e.set_property(
            handle,
            PropertyKey::InnerShadowLayers,
            serde_json::to_value([Shadow::new(2., 0., 0., Color::rgb(255, 0, 0))]).unwrap(),
        )
        .unwrap();
        let current = key(&e);
        assert_eq!(bounds, e.visible_bounds().unwrap());
        assert!(!current.compatible(&first, &domain));
        assert_eq!(c.request(current.clone()).len(), 1);
        assert!(c.old_frame(&domain).is_none());
        assert!(work.cancel.load(Ordering::Relaxed));
        c.finish(pending, late);
        assert!(c.frame(&domain).is_none());
        let fresh = c.take_work().unwrap();
        let result = fresh.run();
        c.finish(current.clone(), result);
        let candidate = c.frame(&domain).unwrap();
        assert!(!Arc::ptr_eq(&candidate.0.image, &old.image));
        c.accepted(&current, &domain, &candidate.0.image);
        assert!(!c.pending());
    }
    #[test]
    fn container_inner_edit_retires_accepted_frame_and_rejects_late_success() {
        use gpui_flow_engine::PropertyKey;
        use gpui_flow_scene::{Color, Shadow};
        let mut e = engine();
        let mut c = CompositeController::default();
        let first = key(&e);
        let old = ready(&mut c, first.clone());
        let domain: DisplayId = "g".into();
        c.accepted(&first, &domain, &old.image);
        let handle = e.handle(&"r".into()).unwrap();
        e.set_property(
            handle,
            PropertyKey::Fill,
            serde_json::to_value(gpui_flow_scene::Paint::solid(Color::rgb(0, 0, 255))).unwrap(),
        )
        .unwrap();
        let pending = key(&e);
        assert!(pending.compatible(&first, &domain));
        c.request(pending.clone());
        let work = c.take_work().unwrap();
        let late = work.run();
        let bounds = e.visible_bounds().unwrap();
        e.set_property(
            e.handle(&"g".into()).unwrap(),
            PropertyKey::InnerShadowLayers,
            serde_json::to_value([Shadow::new(2., 0., 0., Color::rgb(255, 0, 0))]).unwrap(),
        )
        .unwrap();
        let current = key(&e);
        assert_eq!(bounds, e.visible_bounds().unwrap());
        assert!(!current.compatible(&first, &domain));
        assert_eq!(c.request(current.clone()).len(), 1);
        assert!(c.old_frame(&domain).is_none());
        assert!(work.cancel.load(Ordering::Relaxed));
        c.finish(pending, late);
        assert!(c.frame(&domain).is_none());
        let fresh = c.take_work().unwrap();
        let result = fresh.run();
        c.finish(current.clone(), result);
        let candidate = c.frame(&domain).unwrap();
        assert!(!Arc::ptr_eq(&candidate.0.image, &old.image));
        c.accepted(&current, &domain, &candidate.0.image);
        assert!(!c.pending());
    }
    #[test]
    fn retirement_pressure_blocks_candidate_before_any_upload_and_preserves_budget() {
        let mut e = engine();
        let mut c = CompositeController::default();
        let first = key(&e);
        let image = ready(&mut c, first.clone());
        c.accepted(&first, &"g".into(), &image.image);
        e.set_property(
            e.handle(&"r".into()).unwrap(),
            gpui_flow_engine::PropertyKey::Opacity,
            serde_json::json!(0.5),
        )
        .unwrap();
        let candidate = ready(&mut c, key(&e));
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
    fn selected_gpui_adapter_validates_index_empty_and_budget_before_copy() {
        let mut bytes = 0;
        assert!(matches!(
            selected_image(&input_image(2), 2, &mut bytes, MAX_RETAINED),
            Err(CompositeError::InvalidImage)
        ));
        assert!(matches!(
            selected_image(&input_image(0), 0, &mut bytes, MAX_RETAINED),
            Err(CompositeError::InvalidImage)
        ));
        assert!(matches!(
            selected_image(&input_image(1), 0, &mut bytes, 3),
            Err(CompositeError::WorkingSetLimit)
        ));
        assert_eq!(bytes, 0);
        assert_eq!(
            selected_image(&input_image(1), 0, &mut bytes, 4)
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
                frame_index: 0,
                playback_generation: 0,
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
                    frame_index: 0,
                    playback_generation: 0,
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
                s.update_object(&"r".into(), |o| o.style.opacity = 0.5)?;
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
                        frame_index: 0,
                        playback_generation: 0,
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
    fn animated_key() -> Arc<RequestKey> {
        let mut e = engine();
        e.transaction(|s| {
            s.update_object(&"r".into(), |o| {
                o.shape = gpui_flow_scene::display::Image::new(
                    "animated.gif",
                    gpui_flow_scene::Size::new(20., 20.),
                )
                .into();
            })
        })
        .unwrap();
        let image = Arc::new(RenderImage::new(vec![
            image::Frame::new(image::RgbaImage::from_pixel(
                1,
                1,
                image::Rgba([0, 0, 255, 255]),
            )),
            image::Frame::new(image::RgbaImage::from_pixel(
                2,
                1,
                image::Rgba([0, 255, 0, 255]),
            )),
        ]));
        let mut k = (*key(&e)).clone();
        k.domains[0].images.insert(
            "r".into(),
            crate::resources::CompositeImageObservation {
                ticket: None,
                image: Ok(image),
                frame_index: 0,
                playback_generation: 0,
            },
        );
        Arc::new(k)
    }
    fn next_frame(k: &Arc<RequestKey>, index: usize) -> Arc<RequestKey> {
        let mut k = (**k).clone();
        k.domains[0]
            .images
            .get_mut(&"r".into())
            .unwrap()
            .frame_index = index;
        Arc::new(k)
    }
    #[test]
    fn slow_animation_worker_publishes_admitted_pixels_before_latest_sample() {
        let first = animated_key();
        let next = next_frame(&first, 1);
        let mut c = CompositeController::default();
        c.request(first.clone());
        let work = c.take_work().unwrap();
        for _ in 0..20 {
            c.request(next.clone());
            assert!(!work.cancel.load(Ordering::Relaxed));
            assert!(c.take_work().is_none());
        }
        c.finish(first.clone(), work.run());
        let red = c.frame(&"g".into()).unwrap().0;
        assert_eq!(red.image.as_bytes(0).unwrap()[0..4], [0, 0, 255, 255]);
        c.request(next.clone()); // Preserve candidate until real upload acceptance.
        assert_eq!(c.requested, Some(first.clone()));
        c.accepted(&first, &"g".into(), &red.image);
        c.request(next.clone());
        assert!(c.old_frame(&"g".into()).is_some());
        assert!(
            !c.diagnostics()
                .iter()
                .any(|i| i.error == CompositeError::StaleFrame)
        );
        let work = c.take_work().unwrap();
        assert_eq!(work.key, next);
        c.finish(next.clone(), work.run());
        let green = c.frame(&"g".into()).unwrap().0;
        assert_eq!(
            green.image.as_bytes(0).unwrap()[(10 * 20 + 10) * 4..(10 * 20 + 10) * 4 + 4],
            [0, 255, 0, 255]
        );
        assert!(c.accepted(&next, &"g".into(), &green.image).is_some());
    }
    #[test]
    fn loading_badge_distinguishes_first_load_from_presentable_animation_refresh() {
        let first = animated_key();
        let mut c = CompositeController::default();
        c.request(first.clone());
        assert!(c.loading_visible());
        let work = c.take_work().unwrap();
        assert!(c.loading_visible());
        c.finish(first.clone(), work.run());
        let image = c.frame(&"g".into()).unwrap().0;
        c.accepted(&first, &"g".into(), &image.image);
        c.request(next_frame(&first, 1));
        assert!(c.pending(), "refresh still needs scheduling");
        assert!(!c.loading_visible(), "accepted frame remains paintable");
        let _work = c.take_work().unwrap();
        assert!(!c.loading_visible());
    }
    #[test]
    fn loading_badge_returns_when_geometry_invalidates_the_presented_frame() {
        let first = animated_key();
        let mut c = CompositeController::default();
        let image = ready(&mut c, first.clone());
        c.accepted(&first, &"g".into(), &image.image);
        let mut changed = (*first).clone();
        changed.viewport.device_scale = 2.;
        c.request(Arc::new(changed));
        assert!(c.frame(&"g".into()).is_none());
        assert!(c.loading_visible());
    }
    #[test]
    fn animation_seek_resource_and_geometry_changes_hard_cancel_and_hide_old_pixels() {
        for change in 0..3 {
            let first = animated_key();
            let mut c = CompositeController::default();
            let old = ready(&mut c, first.clone());
            c.accepted(&first, &"g".into(), &old.image);
            let next = next_frame(&first, 1);
            c.request(next.clone());
            let work = c.take_work().unwrap();
            let mut hard = (*next).clone();
            match change {
                0 => {
                    hard.domains[0]
                        .images
                        .get_mut(&"r".into())
                        .unwrap()
                        .playback_generation += 1
                }
                1 => {
                    hard.domains[0].images.get_mut(&"r".into()).unwrap().image = Ok(input_image(2))
                }
                _ => hard.viewport.device_scale = 2.,
            }
            c.request(Arc::new(hard));
            assert!(work.cancel.load(Ordering::Relaxed));
            assert!(c.old_frame(&"g".into()).is_none());
            c.finish(next, work.run());
            assert!(c.frame(&"g".into()).is_none());
            assert!(c.take_work().is_some());
        }
    }
    #[test]
    fn animation_ticks_memoize_each_failed_sample_without_blocking_other_frames() {
        let first = animated_key();
        let next = next_frame(&first, 1);
        let mut c = CompositeController::default();
        c.request(first.clone());
        c.take_work().unwrap();
        c.finish(
            first.clone(),
            vec![("g".into(), Err(CompositeError::WorkingSetLimit))],
        );
        c.request(next.clone());
        let work = c.take_work().expect("another selected frame can fit");
        c.finish(next.clone(), work.run());
        let green = c.frame(&"g".into()).unwrap().0;
        c.accepted(&next, &"g".into(), &green.image);
        for index in [0, 1, 0, 1] {
            c.request(next_frame(&first, index));
            assert!(c.take_work().is_none());
        }
        c.request(first.clone());
        c.retired_bytes = 4;
        c.release_retired(4);
        assert!(c.take_work().is_some());
        let mut c = CompositeController::default();
        c.request(first.clone());
        c.take_work().unwrap();
        c.finish(
            first.clone(),
            vec![("g".into(), Err(CompositeError::UploadFailed("test".into())))],
        );
        for index in [1, 0, 1, 0] {
            c.request(next_frame(&first, index));
            assert!(c.take_work().is_none());
        }
        assert!(!c.can_animate_domain(&"g".into()));
    }

    #[test]
    fn selected_frames_have_independent_dimensions_and_conversion_budget() {
        let k = animated_key();
        let image = k.domains[0].images[&"r".into()].image.as_ref().unwrap();
        let mut bytes = 0;
        let a = selected_image(image, 0, &mut bytes, 12).unwrap();
        let b = selected_image(image, 1, &mut bytes, 12).unwrap();
        assert_eq!((a.width(), b.width(), bytes), (1, 2, 12));
        assert_eq!(a.premultiplied_rgba(), &[255, 0, 0, 255]);
        assert_eq!(b.premultiplied_rgba(), &[0, 255, 0, 255, 0, 255, 0, 255]);
        assert!(matches!(
            selected_image(image, 1, &mut bytes, 12),
            Err(CompositeError::WorkingSetLimit)
        ));
    }

    #[test]
    fn accepted_other_domain_does_not_pin_removed_decoded_animation() {
        let mut c = CompositeController::default();
        let mut first = (*animated_key()).clone();
        let image = first.domains[0].images[&"r".into()].image.as_ref().unwrap();
        let weak = Arc::downgrade(image);
        // Domain identities are deliberately opaque; construct a second source
        // from a genuinely independent scene with its own root ID.
        let mut scene = gpui_flow_scene::Scene::new();
        scene
            .insert(
                gpui_flow_scene::DisplayObject::new(
                    "other",
                    gpui_flow_scene::display::Rect::new(10., 10.),
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
        first.domains.extend(key(&e).domains.clone());
        let first = Arc::new(first);
        c.request(first.clone());
        let work = c.take_work().unwrap();
        c.finish(first.clone(), work.run());
        drop(work);
        for id in ["g", "other"] {
            let frame = c.frame(&id.into()).unwrap().0;
            c.accepted(&first, &id.into(), &frame.image);
        }
        let mut next = (*first).clone();
        next.domains
            .retain(|d| d.source.root_id().as_str() == "other");
        next.revision += 1;
        let removed = c.request(Arc::new(next));
        drop(removed);
        drop(first);
        assert!(c.take_work().is_none());
        assert!(
            weak.upgrade().is_none(),
            "surviving accepted domain must not retain the removed source"
        );
    }
    #[test]
    fn semantic_recovery_reusing_a_frame_reenables_animation_without_a_worker() {
        let first = animated_key();
        let mut controller = CompositeController::default();
        let frame = ready(&mut controller, first.clone());
        controller.accepted(&first, &"g".into(), &frame.image);
        let mut changed = (*first).clone();
        changed.revision += 1;
        let changed = Arc::new(changed);
        controller.request(changed.clone());
        controller.upload_failed(&changed, &"g".into(), "simulated paint failure".into());
        assert!(!controller.can_animate_domain(&"g".into()));
        let image = first.domains[0].images[&"r".into()].image.as_ref().unwrap();
        let mut playback = crate::image_playback::Playback::new(
            image.id,
            Arc::new(crate::image_playback::Timeline::new(image).unwrap()),
        );
        let now = std::time::Duration::from_millis(5);
        assert!(!playback.sample(now, controller.can_animate_domain(&"g".into())));
        controller.request(first);
        assert!(
            controller.take_work().is_none(),
            "recovery reuses an existing accepted frame"
        );
        // SceneView resamples at exactly the same timestamp after request reset.
        assert!(playback.sample(now, controller.can_animate_domain(&"g".into())));
        assert!(controller.errors.is_empty());
    }

    #[test]
    fn shared_decoded_image_at_different_playheads_keeps_distinct_worker_pixels() {
        use gpui_flow_scene::{BlendMode, DisplayObject, Scene, Size, Style, display};
        let mut scene = Scene::new();
        for (group, id) in [("a", "ia"), ("b", "ib")] {
            scene
                .insert(
                    DisplayObject::new(group, display::Group::new()).with_style(Style {
                        blend_mode: BlendMode::Normal,
                        ..Default::default()
                    }),
                    None,
                    None,
                )
                .unwrap();
            let mut image = display::Image::new("shared.gif", Size::new(20., 20.));
            image.fit = display::ImageFit::Fill;
            scene
                .insert(DisplayObject::new(id, image), Some(group.into()), None)
                .unwrap();
        }
        let e = SceneEngine::new(scene).unwrap();
        let mut k = (*key(&e)).clone();
        let source = animated_key().domains[0].images[&"r".into()]
            .image
            .as_ref()
            .unwrap()
            .clone();
        for (index, domain) in k.domains.iter_mut().enumerate() {
            let id = if index == 0 { "ia" } else { "ib" };
            domain.images.insert(
                id.into(),
                crate::resources::CompositeImageObservation {
                    ticket: None,
                    image: Ok(source.clone()),
                    frame_index: index,
                    playback_generation: 0,
                },
            );
        }
        let mut controller = CompositeController::default();
        let k = Arc::new(k);
        controller.request(k.clone());
        let work = controller.take_work().unwrap();
        let result = work.run();
        assert_eq!(
            result[0].1.as_ref().unwrap().as_ref().unwrap().premul_rgba[..4],
            [255, 0, 0, 255]
        );
        assert_eq!(
            result[1].1.as_ref().unwrap().as_ref().unwrap().premul_rgba[..4],
            [0, 255, 0, 255]
        );
        controller.finish(k, result);
        assert!(controller.errors.is_empty());
    }
    #[test]
    fn text_resource_generation_is_semantic_and_missing_bridge_rejects_whole_domain() {
        use gpui_flow_scene::{Color, DisplayObject, Size, Style, display};
        let mut e = engine();
        e.transaction(|scene| {
            scene.insert(
                DisplayObject::new(
                    "text",
                    display::Text::new("same native glyphs", Size::new(80., 30.)),
                )
                .with_style(Style::fill(Color::WHITE)),
                Some("g".into()),
                None,
            )
        })
        .unwrap();
        let mut resources = gpui_flow_engine::SceneResources::new(&mut e).unwrap();
        resources.synchronize(&mut e).unwrap();
        let handle = e.handle(&"text".into()).unwrap();
        let mut first_key = (*key(&e)).clone();
        first_key.domains[0].text_tickets = vec![resources.ticket(&e, handle)];
        let first = Arc::new(first_key);
        assert!(first.domains[0].text_tickets[0].is_some());
        let mut controller = CompositeController::default();
        controller.request(first.clone());
        let work = controller.take_work().unwrap();
        let result = work.run();
        assert!(matches!(
            &result[0].1,
            Err(CompositeError::TextBackendRequired(_))
        ));
        resources
            .invalidate(&e, gpui_flow_engine::ResourceKind::Text)
            .unwrap();
        resources.synchronize(&mut e).unwrap();
        let mut second_key = (*key(&e)).clone();
        second_key.domains[0].text_tickets = vec![resources.ticket(&e, handle)];
        let second = Arc::new(second_key);
        assert!(!first.compatible(&second, &"g".into()));
        assert!(!first.animation_only(&second));
        assert!(!first.same_domain_sample(&second, &"g".into()));
        controller.request(second);
        controller.finish(first, result);
        assert!(controller.frame(&"g".into()).is_none());
        assert!(controller.take_work().is_some());
    }

    #[test]
    fn same_backend_text_font_epoch_and_capability_withdrawal_reject_real_stale_work() {
        use gpui_flow_scene::{Color, DisplayObject, Size, Style, display};
        let (app, bridge) = gpui_flow_text::GpuiTextRasterizer::application(
            gpui_kit::platform::current_platform(true),
        );
        assert!(bridge.matches_text_system(&app.text_system()));
        let other = gpui_kit::platform::headless();
        assert!(!bridge.matches_text_system(&other.text_system()));
        let mut e = engine();
        e.transaction(|scene| {
            scene.insert(
                DisplayObject::new("text", display::Text::new("A ffi", Size::new(80., 30.)))
                    .with_style(Style::fill(Color::WHITE)),
                Some("g".into()),
                None,
            )
        })
        .unwrap();
        let first = Arc::new((*key(&e)).clone().with_text_backend(Some(&bridge)));
        let mut controller = CompositeController::default();
        controller.request(first.clone());
        let work = controller.take_work().unwrap();
        let result = work.run();
        assert!(result[0].1.as_ref().unwrap().is_some());
        bridge.invalidate();
        assert!(!first.text_current());
        controller.finish(first.clone(), result);
        assert!(controller.candidates.is_empty());
        let fresh = Arc::new((*key(&e)).clone().with_text_backend(Some(&bridge)));
        assert!(!fresh.compatible(&first, &"g".into()));
        assert!(!fresh.animation_only(&first));
        let frame = ready(&mut controller, fresh.clone());
        controller.accepted(&fresh, &"g".into(), &frame.image);
        let withdrawn = key(&e);
        assert!(!withdrawn.compatible(&fresh, &"g".into()));
        let retired = controller.request(withdrawn.clone());
        assert_eq!(retired.len(), 1);
        assert!(controller.frame(&"g".into()).is_none());
        let work = controller.take_work().unwrap();
        controller.finish(withdrawn, work.run());
        assert!(matches!(
            controller.errors[0].error,
            CompositeError::TextBackendRequired(_)
        ));
        let restored = Arc::new((*key(&e)).clone().with_text_backend(Some(&bridge)));
        assert!(ready(&mut controller, restored).bytes > 0);
    }
    #[test]
    fn real_text_bitmap_survives_fractional_dpr_compositor_placement() {
        use gpui_flow_scene::{
            Affine, BlendMode, Color, DisplayObject, Scene, Size, Style, display,
        };
        let (_app, bridge) = gpui_flow_text::GpuiTextRasterizer::application(
            gpui_kit::platform::current_platform(true),
        );
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("g", display::Group::new()).with_style(Style {
                    blend_mode: BlendMode::Normal,
                    ..Default::default()
                }),
                None,
                None,
            )
            .unwrap();
        let mut text = display::Text::new("A ffi", Size::new(60., 30.));
        text.font_family = "DejaVu Sans".into();
        text.font_size = 20.;
        text.line_height = 1.4;
        scene
            .insert(
                DisplayObject::new("text", text)
                    .with_transform(Affine::translation(10.25, 7.5) * Affine::scale(1.25, 1.25))
                    .with_style(Style::fill(Color::WHITE)),
                Some("g".into()),
                None,
            )
            .unwrap();
        let e = SceneEngine::new(scene).unwrap();
        let mut k = (*key(&e)).clone().with_text_backend(Some(&bridge));
        k.viewport.device_scale = 1.25;
        let k = Arc::new(k);
        let mut controller = CompositeController::default();
        controller.request(k);
        let work = controller.take_work().unwrap();
        let mut result = work.run();
        let actual = result.remove(0).1.unwrap().unwrap();
        let request = gpui_flow_text::TextRasterRequest {
            vertical_align: gpui_flow_text::VerticalAlign::Top,
            text: gpui_flow_text::TextMeasureRequest {
                content: "A ffi".into(),
                font_family: "DejaVu Sans".into(),
                font_size: 25.,
                font_weight: 400,
                line_height: 35.,
                wrap_width: Some(75.),
            },
            align: gpui::TextAlign::Left,
            bounds: [10.25, 7.5, 75., 37.5],
            clip: [0., 0., 100., 100.],
            device_scale: 1.25,
            color: [255; 4],
        };
        let mut remaining = 32_000_000;
        let expected = bridge
            .rasterize(
                &request,
                bridge.epoch(),
                &AtomicBool::new(false),
                32 * 1024 * 1024,
                &mut remaining,
            )
            .unwrap()
            .unwrap();
        let origin = [
            (actual.screen_bounds.origin.x * 1.25).round() as i32,
            (actual.screen_bounds.origin.y * 1.25).round() as i32,
        ];
        let mut ink = 0;
        for y in 0..actual.height {
            for x in 0..actual.width {
                let ex = x as i32 + origin[0] - expected.origin[0];
                let ey = y as i32 + origin[1] - expected.origin[1];
                let reference = if ex >= 0
                    && ey >= 0
                    && ex < expected.width as i32
                    && ey < expected.height as i32
                {
                    expected.premul_rgba
                        [((ey as u32 * expected.width + ex as u32) * 4 + 3) as usize]
                } else {
                    0
                };
                let alpha = actual.premul_rgba[((y * actual.width + x) * 4 + 3) as usize];
                assert!(
                    alpha.abs_diff(reference) <= 1,
                    "pixel ({x},{y}): {alpha} vs {reference}"
                );
                if alpha > 0 {
                    ink += 1;
                }
            }
        }
        assert!(ink > 0);
    }
    fn canvas_key(e: &SceneEngine, color: gpui_flow_scene::Color, grid: bool) -> Arc<RequestKey> {
        let domain = gpui_flow_composite::owned_canvas(
            e.scene(),
            gpui_flow_composite::CanvasBackdrop {
                color,
                grid: grid.then_some(gpui_flow_composite::CanvasGrid {
                    color: gpui_flow_scene::Color::BLACK,
                    spacing: 12.,
                    offset: gpui_flow_scene::Point::ZERO,
                }),
            },
            CompositeLimits::default(),
        )
        .unwrap();
        let mut k = (*key(e)).clone();
        k.domains = RequestKey::new(vec![domain], e, k.viewport, k.window).domains;
        Arc::new(k)
    }
    #[test]
    fn owned_canvas_theme_grid_mode_and_viewport_are_hard_invalidation() {
        use gpui_flow_scene::Color;
        let e = engine();
        let first = canvas_key(&e, Color::WHITE, false);
        for next in [
            canvas_key(&e, Color::BLACK, false),
            canvas_key(&e, Color::WHITE, true),
            key(&e),
        ] {
            let mut c = CompositeController::default();
            c.request(first.clone());
            let work = c.take_work().unwrap();
            c.request(next.clone());
            assert!(work.cancel.load(Ordering::Relaxed));
            c.finish(first.clone(), work.run());
            assert!(c.frame(&DisplayId::default()).is_none());
            assert_eq!(c.take_work().unwrap().key, next);
        }
        let mut c = CompositeController::default();
        c.request(first.clone());
        let work = c.take_work().unwrap();
        c.finish(first.clone(), work.run());
        let frame = c.frame(&DisplayId::default()).unwrap().0;
        c.accepted(&first, &DisplayId::default(), &frame.image);
        let retired = c.request(canvas_key(&e, Color::BLACK, false));
        assert_eq!(retired.len(), 1);
        assert!(c.old_frame(&DisplayId::default()).is_none());
        assert_eq!(retired[0].image.id, frame.image.id);
        for kind in 0..4 {
            let mut other = (*first).clone();
            match kind {
                0 => other.viewport.world_to_screen.tx += 1.,
                1 => other.viewport.canvas_bounds.size.width += 1.,
                2 => other.viewport.device_scale = 1.5,
                _ => other.viewport.content_mask.size.width -= 1.,
            }
            assert!(!first.compatible(&other, &DisplayId::default()));
        }
    }
    #[test]
    fn owned_canvas_animation_coalesces_but_theme_change_cancels_immediately() {
        use gpui_flow_scene::{Color, Size, display};
        let mut e = engine();
        e.transaction(|s| {
            s.update_object(&"r".into(), |o| {
                o.shape = display::Image::new("animated.gif", Size::new(20., 20.)).into();
            })
        })
        .unwrap();
        let mut first = (*canvas_key(&e, Color::WHITE, false)).clone();
        first.domains[0].images = animated_key().domains[0].images.clone();
        let first = Arc::new(first);
        let next = next_frame(&first, 1);
        let id = DisplayId::default();
        let mut c = CompositeController::default();
        c.request(first.clone());
        let work = c.take_work().unwrap();
        c.request(next.clone());
        assert!(!work.cancel.load(Ordering::Relaxed));
        c.finish(first.clone(), work.run());
        let frame = c.frame(&id).unwrap().0;
        c.accepted(&first, &id, &frame.image);
        c.request(next.clone());
        assert!(c.old_frame(&id).is_some());
        let work = c.take_work().unwrap();
        let mut theme = (*canvas_key(&e, Color::BLACK, false)).clone();
        theme.domains[0].images = next.domains[0].images.clone();
        c.request(Arc::new(theme));
        assert!(work.cancel.load(Ordering::Relaxed));
        assert!(c.old_frame(&id).is_none());
    }
    #[test]
    fn canvas_pixel_generation_rejects_late_cpu_success_without_document_changes() {
        use gpui_flow_scene::{Bounds, Color, DisplayObject, Point, Scene, Size, Style, display};
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("c", display::Canvas::new(20., 20.)).with_style(Style {
                    fill: None,
                    ..Default::default()
                }),
                None,
                None,
            )
            .unwrap();
        let mut e = SceneEngine::new(scene).unwrap();
        let h = e.handle(&"c".into()).unwrap();
        let mut resources = crate::resources::SceneResourceAdapter::new(&mut e).unwrap();
        let bounds = Bounds::new(Point::ZERO, Size::new(20., 20.));
        resources
            .edit_canvas(&mut e, h, |s| s.fill_rect(bounds, Color::hex(0xff0000)))
            .unwrap();
        let revision = e.revision();
        let first = Arc::new((*key(&e)).clone().with_resources(&e, Some(&resources)));
        let mut c = CompositeController::default();
        c.request(first.clone());
        let work = c.take_work().unwrap();
        let late = work.run();
        assert!(matches!(late.as_slice(), [(_, Ok(Some(_)))]));
        resources
            .edit_canvas(&mut e, h, |s| s.fill_rect(bounds, Color::hex(0x0000ff)))
            .unwrap();
        assert_eq!(revision, e.revision());
        let second = Arc::new((*key(&e)).clone().with_resources(&e, Some(&resources)));
        assert!(!second.compatible(&first, &"c".into()));
        c.request(second.clone());
        assert!(work.cancel.load(Ordering::Relaxed));
        c.finish(first, late);
        assert!(c.frame(&"c".into()).is_none());
        let fresh = c.take_work().unwrap();
        let result = fresh.run();
        let [(_, Ok(Some(frame)))] = result.as_slice() else {
            panic!("complete canvas frame expected")
        };
        let offset = ((10 * frame.width + 10) * 4) as usize;
        assert_eq!(&frame.premul_rgba[offset..offset + 4], &[0, 0, 255, 255]);
        c.finish(second.clone(), result);
        let image = c.frame(&"c".into()).unwrap().0;
        c.accepted(&second, &"c".into(), &image.image);
        resources.edit_canvas(&mut e, h, |s| s.clear()).unwrap();
        let third = Arc::new((*key(&e)).clone().with_resources(&e, Some(&resources)));
        assert_eq!(c.request(third).len(), 1);
        assert!(c.old_frame(&"c".into()).is_none());
    }
    #[test]
    fn text_box_actual_glyph_pixels_follow_vertical_alignment_and_padding() {
        use display::TextVerticalAlign;
        use gpui_flow_scene::{BlendMode, Color, DisplayObject, Scene, Size, Style, display};
        let (_app, bridge) = gpui_flow_text::GpuiTextRasterizer::application(
            gpui_kit::platform::current_platform(true),
        );
        let render = |align, content: &str, height| {
            let mut scene = Scene::new();
            scene
                .insert(
                    DisplayObject::new("g", display::Group::new()).with_style(Style {
                        blend_mode: BlendMode::Normal,
                        ..Default::default()
                    }),
                    None,
                    None,
                )
                .unwrap();
            let mut text = display::Text::new(content, Size::new(100., height));
            text.font_family = "DejaVu Sans".into();
            text.font_size = 16.;
            text.line_height = 1.25;
            text.padding = [10.; 4];
            text.vertical_align = align;
            scene
                .insert(
                    DisplayObject::new("text", text).with_style(Style::fill(Color::WHITE)),
                    Some("g".into()),
                    None,
                )
                .unwrap();
            let e = SceneEngine::new(scene).unwrap();
            let request = Arc::new((*key(&e)).clone().with_text_backend(Some(&bridge)));
            let mut controller = CompositeController::default();
            controller.request(request);
            controller
                .take_work()
                .unwrap()
                .run()
                .remove(0)
                .1
                .unwrap()
                .unwrap()
        };
        let ink = |frame: &CpuFrame| {
            let mut min = [f32::INFINITY; 2];
            let mut max = [f32::NEG_INFINITY; 2];
            let mut count = 0;
            for y in 0..frame.height {
                for x in 0..frame.width {
                    if frame.premul_rgba[((y * frame.width + x) * 4 + 3) as usize] > 0 {
                        count += 1;
                        let p = [
                            frame.screen_bounds.origin.x + x as f32,
                            frame.screen_bounds.origin.y + y as f32,
                        ];
                        for i in 0..2 {
                            min[i] = min[i].min(p[i]);
                            max[i] = max[i].max(p[i]);
                        }
                    }
                }
            }
            assert!(count > 0);
            (min, max, count)
        };
        let top = render(TextVerticalAlign::Top, "A\nB", 100.);
        let middle = render(TextVerticalAlign::Middle, "A\nB", 100.);
        let bottom = render(TextVerticalAlign::Bottom, "A\nB", 100.);
        let (a, amax, count) = ink(&top);
        let (b, _, count_b) = ink(&middle);
        let (c, cmax, count_c) = ink(&bottom);
        assert_eq!(b[1] - a[1], 20.);
        assert_eq!(c[1] - a[1], 40.);
        assert_eq!(a[0], b[0]);
        assert_eq!(a[0], c[0]);
        assert_eq!(count, count_b);
        assert_eq!(count, count_c);
        assert!(a[0] >= 10. && amax[0] < 90. && a[1] >= 10. && cmax[1] < 90.);
        // More line boxes than the inner height: clipped overflow stays at top.
        let top = render(TextVerticalAlign::Top, "A\nB\nC", 40.);
        let bottom = render(TextVerticalAlign::Bottom, "A\nB\nC", 40.);
        assert_eq!(top.screen_bounds, bottom.screen_bounds);
        assert_eq!(top.premul_rgba, bottom.premul_rgba);
    }
    #[test]
    fn animated_image_corner_edits_reject_late_frames_and_repaint_actual_geometry() {
        use gpui_flow_scene::{Shape, Size, display};
        let mut e = engine();
        let mut image =
            display::Image::new("animated.gif", Size::new(20., 20.)).with_corners([8., 0., 0., 0.]);
        image.fit = display::ImageFit::Fill;
        e.transaction(|s| s.update_object(&"r".into(), |o| o.shape = image.clone().into()))
            .unwrap();
        let mut first = (*key(&e)).clone();
        first.domains[0].images = animated_key().domains[0].images.clone();
        let first = Arc::new(first);
        let mut c = CompositeController::default();
        let frame = ready(&mut c, first.clone());
        c.accepted(&first, &"g".into(), &frame.image);
        let next = next_frame(&first, 1);
        assert!(next.animation_only(&first));
        c.request(next.clone());
        let work = c.take_work().unwrap();
        let late = work.run();
        assert!(matches!(late.as_slice(), [(_, Ok(Some(_)))]));
        image.corner_radii = Some([0., 8., 0., 0.].into());
        let h = e.handle(&"r".into()).unwrap();
        e.set_property(
            h,
            gpui_flow_engine::PropertyKey::Shape,
            serde_json::to_value(Shape::from(image)).unwrap(),
        )
        .unwrap();
        let mut fresh = (*key(&e)).clone();
        fresh.domains[0].images = next.domains[0].images.clone();
        let fresh = Arc::new(fresh);
        assert!(!fresh.compatible(&next, &"g".into()));
        assert_eq!(c.request(fresh.clone()).len(), 1);
        assert!(work.cancel.load(Ordering::Relaxed));
        c.finish(next, late);
        assert!(c.frame(&"g".into()).is_none());
        let work = c.take_work().unwrap();
        let result = work.run();
        let [(_, Ok(Some(pixels)))] = result.as_slice() else {
            panic!("image frame expected")
        };
        assert_eq!(&pixels.premul_rgba[..4], &[0, 255, 0, 255]);
        let top_right = ((19) * 4) as usize;
        assert_eq!(&pixels.premul_rgba[top_right..top_right + 4], &[0, 0, 0, 0]);
        c.finish(fresh, result);
        assert!(c.frame(&"g".into()).is_some());
    }
}

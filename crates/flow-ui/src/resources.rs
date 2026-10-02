//! GPUI producer for the engine's runtime resource lifecycle. This adapter uses
//! GPUI's public asset subscription/cache and the existing shared text shaper.
//! It does not fetch, decode, or poll private cache state itself.
mod canvas;
use gpui::{AnyImageCache, App, ImgResourceLoader, RenderImage, Resource, Window};
use gpui_flow_engine::{
    ElementHandle, ResourceCancellation, ResourceCompletion, ResourceError, ResourceEvent,
    ResourceFrame, ResourceKind, ResourceOutcome, ResourceState, ResourceTicket, ResourceViewEvent,
    SceneEngine, SceneResources,
};
use gpui_flow_scene::{Shape, display::Text};
use gpui_flow_text::TextMeasureRequest;
use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

/// A host's explicit authorization/mapping decision. Return None to deny a
/// reference. The default resolver permits filesystem paths, never URI schemes.
/// A resolver may deliberately map an approved source to Resource::Uri; GPUI
/// remains responsible for HTTP, decoding, cache sharing and completion wakeup.
pub type SceneImageResolver = Rc<dyn Fn(&str) -> Option<Resource>>;

#[derive(Clone)]
pub enum SceneImagePresentation {
    Pending,
    Loaded(Arc<RenderImage>),
    Failed(String),
    Cancelled,
}
/// Immutable observation bound to a live resource generation, never only a path.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CompositeImageObservation {
    pub ticket: Option<ResourceTicket>,
    pub image: Result<Arc<RenderImage>, gpui_flow_composite::CompositeError>,
    pub frame_index: usize,
    pub playback_generation: u64,
}
impl CompositeImageObservation {
    pub(crate) fn resource_eq(&self, other: &Self) -> bool {
        self.ticket == other.ticket
            && self.image == other.image
            && self.playback_generation == other.playback_generation
    }
}
/// Runtime-only state; no decoded image or authorization enters Scene JSON.
#[derive(Debug)]
pub enum CanvasEditError {
    Resource(ResourceError),
    Pixels(gpui_flow_composite::CompositeError),
}
impl std::fmt::Display for CanvasEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resource(e) => e.fmt(f),
            Self::Pixels(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for CanvasEditError {}

pub struct SceneResourceAdapter {
    canvases: canvas::CanvasResources,
    tracker: SceneResources,
    resolver: SceneImageResolver,
    cache: Option<AnyImageCache>,
    resolved: BTreeMap<ResourceTicket, Resource>,
    images: BTreeMap<ResourceTicket, SceneImagePresentation>,
    playbacks: BTreeMap<ResourceTicket, crate::image_playback::Playback>,
    timelines: BTreeMap<
        gpui::ImageId,
        Result<Arc<crate::image_playback::Timeline>, gpui_flow_composite::CompositeError>,
    >,
}
impl SceneResourceAdapter {
    pub fn new(engine: &mut SceneEngine) -> Result<Self, ResourceError> {
        let mut tracker = SceneResources::new(engine)?;
        let canvases = match canvas::CanvasResources::new(engine) {
            Ok(canvases) => canvases,
            Err(error) => {
                let _ = tracker.dispose(engine);
                return Err(error);
            }
        };
        Ok(Self {
            tracker,
            canvases,
            resolver: Rc::new(local_image_resource),
            cache: None,
            resolved: BTreeMap::new(),
            images: BTreeMap::new(),
            playbacks: BTreeMap::new(),
            timelines: BTreeMap::new(),
        })
    }
    /// Synchronize resource identity and allocate/resize Canvas surfaces without
    /// requiring a native Window. URL images and text remain pending here.
    pub fn prepare_canvases(
        &mut self,
        engine: &mut SceneEngine,
    ) -> Result<Vec<ResourceEvent>, ResourceError> {
        let mut events = self.tracker.synchronize(engine)?;
        self.canvases.retain_live(engine);
        let requests = engine
            .scene()
            .nodes()
            .filter_map(|node| {
                let Shape::Canvas(config) = &node.object.shape else {
                    return None;
                };
                let handle = engine.handle(&node.object.id)?;
                let ticket = self.tracker.ticket(engine, handle)?;
                (self.tracker.state(engine, ticket) == Some(ResourceState::Pending))
                    .then(|| (ticket, node.object.id.clone(), config.clone()))
            })
            .collect::<Vec<_>>();
        for (ticket, id, config) in requests {
            if self.tracker.ticket(engine, ticket.handle) != Some(ticket) {
                continue;
            }
            let outcome = match self.canvases.ensure(ticket, id, &config) {
                Ok(()) => ResourceOutcome::Loaded,
                Err(error) => ResourceOutcome::Failed(error.to_string()),
            };
            self.settle(engine, ticket, outcome, None, &mut events)?;
        }
        Ok(events)
    }
    /// Explicit retry after a host releases capacity or changes resource limits.
    /// Existing live pixel content is retained; failed producers get fresh tickets.
    pub fn retry_canvases(
        &mut self,
        engine: &mut SceneEngine,
    ) -> Result<Vec<ResourceEvent>, ResourceError> {
        self.tracker.invalidate(engine, ResourceKind::Canvas)?;
        self.prepare_canvases(engine)
    }
    /// Atomic imperative drawing. Scene/Undo contains only the Canvas config;
    /// pixel edits are runtime-only and must be followed by a host repaint.
    pub fn edit_canvas(
        &mut self,
        engine: &mut SceneEngine,
        handle: ElementHandle,
        edit: impl FnOnce(
            &mut gpui_flow_composite::CanvasSurface,
        ) -> Result<(), gpui_flow_composite::CompositeError>,
    ) -> Result<Vec<ResourceEvent>, CanvasEditError> {
        let events = self
            .prepare_canvases(engine)
            .map_err(CanvasEditError::Resource)?;
        let ticket = self
            .tracker
            .ticket(engine, handle)
            .filter(|t| t.kind == ResourceKind::Canvas)
            .ok_or(CanvasEditError::Pixels(
                gpui_flow_composite::CompositeError::Superseded,
            ))?;
        self.canvases
            .edit(ticket, edit)
            .map_err(CanvasEditError::Pixels)?;
        Ok(events)
    }
    pub fn canvas_snapshot(
        &self,
        engine: &SceneEngine,
        handle: ElementHandle,
    ) -> Option<Arc<gpui_flow_composite::StaticImage>> {
        self.canvases.snapshot(self.tracker.ticket(engine, handle)?)
    }
    pub(crate) fn composite_canvas(
        &self,
        engine: &SceneEngine,
        id: &gpui_flow_scene::DisplayId,
        expected: &gpui_flow_scene::display::Canvas,
    ) -> CompositeImageObservation {
        let ticket = engine
            .handle(id)
            .and_then(|h| self.tracker.ticket(engine, h));
        let canonical = engine
            .scene()
            .get(id)
            .is_some_and(|n| matches!(&n.object.shape, Shape::Canvas(c) if c == expected));
        let input = canonical
            .then(|| ticket.and_then(|t| self.canvases.image(t)))
            .flatten();
        let (image, generation) = match input {
            Some((image, generation)) => (Ok(image), generation),
            None => (
                Err(gpui_flow_composite::CompositeError::ImageUnavailable {
                    image: id.clone(),
                    reason: ticket
                        .and_then(|t| self.tracker.state(engine, t))
                        .map_or_else(
                            || "Canvas has no current owned surface".into(),
                            |state| format!("Canvas resource: {state:?}"),
                        ),
                }),
                0,
            ),
        };
        CompositeImageObservation {
            ticket,
            image,
            frame_index: 0,
            playback_generation: generation,
        }
    }
    pub fn tracker(&self) -> &SceneResources {
        &self.tracker
    }
    /// Applies only to this view; invalidates previous image observations.
    pub fn set_image_resolver(
        &mut self,
        engine: &SceneEngine,
        resolver: SceneImageResolver,
    ) -> Result<(), ResourceError> {
        self.tracker.invalidate(engine, ResourceKind::Image)?;
        self.resolver = resolver;
        self.resolved.clear();
        self.images.clear();
        self.playbacks.clear();
        self.timelines.clear();
        Ok(())
    }
    /// Optional host-owned GPUI ImageCache. No independent image cache is
    /// implemented here, and disposal never evicts a shared backend asset.
    pub fn set_image_cache(
        &mut self,
        engine: &SceneEngine,
        cache: Option<AnyImageCache>,
    ) -> Result<(), ResourceError> {
        self.tracker.invalidate(engine, ResourceKind::Image)?;
        self.cache = cache;
        self.resolved.clear();
        self.images.clear();
        self.playbacks.clear();
        self.timelines.clear();
        Ok(())
    }
    /// Font installation/replacement has no public GPUI resource notification;
    /// hosts explicitly request a new shaping generation after that operation.
    pub fn invalidate_text(&mut self, engine: &SceneEngine) -> Result<(), ResourceError> {
        self.tracker.invalidate(engine, ResourceKind::Text)
    }
    pub fn prepare(
        &mut self,
        engine: &mut SceneEngine,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<Vec<ResourceEvent>, ResourceError> {
        let mut events = self.prepare_canvases(engine)?;
        self.resolved
            .retain(|ticket, _| self.tracker.state(engine, *ticket).is_some());
        self.images
            .retain(|ticket, _| self.tracker.state(engine, *ticket).is_some());
        // Clone canonical resource requests before invoking host/GPUI code.
        // State-style placeholders are intentionally not canonical font inputs.
        let requests = engine
            .scene()
            .nodes()
            .filter_map(|node| {
                let handle = engine.handle(&node.object.id)?;
                let ticket = self.tracker.ticket(engine, handle)?;
                (self.tracker.state(engine, ticket) == Some(ResourceState::Pending))
                    .then(|| (ticket, node.object.shape.clone()))
            })
            .collect::<Vec<_>>();
        for (ticket, shape) in requests {
            let (outcome, presentation) = match shape {
                Shape::Image(image) => {
                    if !self.resolved.contains_key(&ticket) {
                        let resource =
                            catch_unwind(AssertUnwindSafe(|| (self.resolver)(&image.source)));
                        match resource {
                            Ok(Some(resource)) => {
                                self.resolved.insert(ticket, resource);
                            }
                            Ok(None) => {
                                self.settle(
                                    engine,
                                    ticket,
                                    ResourceOutcome::Cancelled(ResourceCancellation::NotAuthorized),
                                    Some(SceneImagePresentation::Cancelled),
                                    &mut events,
                                )?;
                                continue;
                            }
                            Err(_) => {
                                self.settle(
                                    engine,
                                    ticket,
                                    ResourceOutcome::Failed("host image resolver panicked".into()),
                                    Some(SceneImagePresentation::Failed(
                                        "host image resolver panicked".into(),
                                    )),
                                    &mut events,
                                )?;
                                continue;
                            }
                        }
                    }
                    // Revalidate after the host resolver, before invoking a loader.
                    if self.tracker.state(engine, ticket) != Some(ResourceState::Pending) {
                        continue;
                    }
                    let resource = self.resolved.get(&ticket).expect("authorized resource");
                    let result = if let Some(cache) = &self.cache {
                        cache.load(resource, window, cx)
                    } else {
                        // Public subscription: GPUI invalidates this view on
                        // completion. There is no timer or internal-state poll.
                        window.use_asset::<ImgResourceLoader>(resource, cx)
                    };
                    match result {
                        None => {
                            self.images.insert(ticket, SceneImagePresentation::Pending);
                            continue;
                        }
                        Some(Ok(image)) => (
                            ResourceOutcome::Loaded,
                            Some(SceneImagePresentation::Loaded(image)),
                        ),
                        Some(Err(error)) => {
                            let error = error.to_string();
                            (
                                ResourceOutcome::Failed(error.clone()),
                                Some(SceneImagePresentation::Failed(error)),
                            )
                        }
                    }
                }
                Shape::Text(text) => {
                    let request = text_request(&text);
                    let shaped = catch_unwind(AssertUnwindSafe(|| {
                        if text.decoration != gpui_flow_scene::display::TextDecoration::None {
                            request.validate()?;
                        }
                        let shaped = gpui_flow_text::gpui_overflow::shape_text_in_box(
                            window.text_system(),
                            &request,
                            gpui::Hsla::default(),
                            crate::text_measure::text_overflow(text.overflow),
                            text.content_bounds().size.width,
                            text.content_bounds().size.height,
                        )?;
                        gpui_flow_text::gpui_decoration::layout_decorations(
                            &shaped,
                            &gpui_flow_text::gpui_decoration::DecorationRequest {
                                decoration: crate::text_measure::text_decoration(text.decoration),
                                font_size: request.font_size,
                                line_height: request.line_height,
                                bounds: [
                                    0.,
                                    0.,
                                    text.content_bounds().size.width,
                                    text.content_bounds().size.height,
                                ],
                                align: match text.align {
                                    gpui_flow_scene::display::TextAlign::Start => {
                                        gpui::TextAlign::Left
                                    }
                                    gpui_flow_scene::display::TextAlign::Center => {
                                        gpui::TextAlign::Center
                                    }
                                    gpui_flow_scene::display::TextAlign::End => {
                                        gpui::TextAlign::Right
                                    }
                                },
                                vertical_align: crate::text_measure::vertical_align(
                                    text.vertical_align,
                                ),
                                device_scale: 1.,
                            },
                            usize::MAX,
                        )?;
                        Ok::<_, String>(shaped)
                    }))
                    .unwrap_or_else(|_| Err("native text shaping panicked".into()));
                    (
                        match shaped {
                            Ok(_) => ResourceOutcome::Loaded,
                            Err(error) => ResourceOutcome::Failed(error),
                        },
                        None,
                    )
                }
                _ => continue,
            };
            self.settle(engine, ticket, outcome, presentation, &mut events)?;
        }
        self.synchronize_playbacks();
        Ok(events)
    }
    fn settle(
        &mut self,
        engine: &mut SceneEngine,
        ticket: ResourceTicket,
        outcome: ResourceOutcome,
        image: Option<SceneImagePresentation>,
        events: &mut Vec<ResourceEvent>,
    ) -> Result<(), ResourceError> {
        let (accepted, event) = self.tracker.complete(engine, ticket, outcome)?;
        if accepted == ResourceCompletion::Accepted {
            if let Some(image) = image {
                self.images.insert(ticket, image);
            }
            if let Some(event) = event {
                events.push(event);
            }
        }
        Ok(())
    }
    fn synchronize_playbacks(&mut self) {
        use crate::image_playback::{MAX_TIMELINE_FRAMES, Playback, Timeline};
        let live = self
            .images
            .iter()
            .filter_map(|(ticket, presentation)| {
                if let SceneImagePresentation::Loaded(image) = presentation {
                    Some((*ticket, image.clone()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let image_ids = live
            .iter()
            .map(|(_, image)| image.id)
            .collect::<BTreeSet<_>>();
        let old_frames = self
            .timelines
            .values()
            .filter_map(|t| t.as_ref().ok())
            .map(|t| t.frames())
            .sum::<usize>();
        self.timelines.retain(|id, _| image_ids.contains(id));
        let retained_frames = self
            .timelines
            .values()
            .filter_map(|t| t.as_ref().ok())
            .map(|t| t.frames())
            .sum::<usize>();
        if retained_frames < old_frames {
            // Aggregate admission can recover only after a real schedule release.
            // Permanent errors may be rechecked once too; no polling retry loop.
            self.timelines.retain(|_, timeline| timeline.is_ok());
        }
        let live_tickets = live
            .iter()
            .map(|(ticket, _)| *ticket)
            .collect::<BTreeSet<_>>();
        self.playbacks
            .retain(|ticket, _| live_tickets.contains(ticket));
        let mut count = self
            .timelines
            .values()
            .filter_map(|t| t.as_ref().ok())
            .map(|t| t.frames())
            .sum::<usize>();
        for (ticket, image) in live {
            let timeline = self.timelines.entry(image.id).or_insert_with(|| {
                if image.frame_count() > MAX_TIMELINE_FRAMES.saturating_sub(count) {
                    return Err(gpui_flow_composite::CompositeError::InputLimit);
                }
                let timeline = Timeline::new(&image)?;
                count += timeline.frames();
                Ok(Arc::new(timeline))
            });
            if let Ok(timeline) = timeline {
                self.playbacks
                    .entry(ticket)
                    .and_modify(|playback| {
                        if playback.image != image.id {
                            *playback = Playback::new(image.id, timeline.clone());
                        }
                    })
                    .or_insert_with(|| Playback::new(image.id, timeline.clone()));
            }
        }
    }
    /// Select one immutable frame per live instance from a shared monotonic time.
    /// Hidden/rejected nodes do not drive frames and resume from their frozen phase.
    pub(crate) fn sample_images(
        &mut self,
        engine: &SceneEngine,
        visible: &BTreeSet<gpui_flow_scene::DisplayId>,
        now: Duration,
        active: bool,
    ) -> bool {
        let tickets = visible
            .iter()
            .filter_map(|id| engine.handle(id))
            .filter_map(|h| self.tracker.ticket(engine, h))
            .collect::<BTreeSet<_>>();
        let mut animate = false;
        for (ticket, playback) in &mut self.playbacks {
            animate |= playback.sample(now, active && tickets.contains(ticket));
        }
        animate
    }
    pub(crate) fn freeze_images(&mut self, now: Duration) {
        for playback in self.playbacks.values_mut() {
            playback.freeze(now);
        }
    }
    /// Returns false until this handle's current image generation is loaded.
    pub(crate) fn set_image_playing(
        &mut self,
        engine: &SceneEngine,
        handle: ElementHandle,
        now: Duration,
        playing: bool,
    ) -> bool {
        let Some(playback) = self
            .tracker
            .ticket(engine, handle)
            .and_then(|t| self.playbacks.get_mut(&t))
        else {
            return false;
        };
        playback.set_playing(now, playing);
        true
    }
    pub(crate) fn seek_image(
        &mut self,
        engine: &SceneEngine,
        handle: ElementHandle,
        time: Duration,
    ) -> bool {
        let Some(playback) = self
            .tracker
            .ticket(engine, handle)
            .and_then(|t| self.playbacks.get_mut(&t))
        else {
            return false;
        };
        playback.seek(time);
        true
    }
    pub(crate) fn image_frame(&self, engine: &SceneEngine, handle: ElementHandle) -> usize {
        self.tracker
            .ticket(engine, handle)
            .and_then(|t| self.playbacks.get(&t))
            .map_or(0, |p| p.index())
    }
    pub fn image(&self, engine: &SceneEngine, handle: ElementHandle) -> SceneImagePresentation {
        let image = self
            .tracker
            .ticket(engine, handle)
            .and_then(|ticket| self.images.get(&ticket).cloned())
            .unwrap_or(SceneImagePresentation::Pending);
        if let SceneImagePresentation::Loaded(image) = &image
            && let Some(Err(error)) = self.timelines.get(&image.id)
        {
            return SceneImagePresentation::Failed(error.to_string());
        }
        image
    }
    pub(crate) fn composite_image(
        &self,
        engine: &SceneEngine,
        id: &gpui_flow_scene::DisplayId,
        expected_source: &str,
    ) -> CompositeImageObservation {
        use gpui_flow_composite::CompositeError;
        let ticket = engine
            .handle(id)
            .and_then(|h| self.tracker.ticket(engine, h));
        let unavailable = |reason: String| CompositeError::ImageUnavailable {
            image: id.clone(),
            reason,
        };
        let canonical_matches = engine.scene().get(id).is_some_and(
            |n| matches!(&n.object.shape, Shape::Image(i) if i.source == expected_source),
        );
        let image = if !canonical_matches {
            Err(unavailable(
                "presented source has no current canonical resource producer".into(),
            ))
        } else {
            match ticket.and_then(|t| self.images.get(&t)) {
                Some(SceneImagePresentation::Loaded(image)) => Ok(image.clone()),
                Some(SceneImagePresentation::Failed(error)) => Err(unavailable(error.clone())),
                Some(SceneImagePresentation::Cancelled) => {
                    Err(unavailable("not authorized by host".into()))
                }
                _ => Err(CompositeError::ImagePending(id.clone())),
            }
        };
        let image = image.and_then(|image| {
            if let Some(Err(error)) = self.timelines.get(&image.id) {
                Err(error.clone())
            } else {
                Ok(image)
            }
        });
        let playback = ticket.and_then(|t| self.playbacks.get(&t));
        CompositeImageObservation {
            ticket,
            image,
            frame_index: playback.map_or(0, |p| p.index()),
            playback_generation: playback.map_or(0, |p| p.generation),
        }
    }
    pub fn frame(&self, engine: &SceneEngine) -> Option<ResourceFrame> {
        self.tracker.frame(engine)
    }
    pub fn layout_ready(&mut self, engine: &SceneEngine) -> Option<ResourceViewEvent> {
        self.tracker.layout_ready(engine)
    }
    pub fn frame_drawn(
        &mut self,
        engine: &SceneEngine,
        frame: ResourceFrame,
    ) -> Vec<ResourceViewEvent> {
        self.tracker.frame_drawn(engine, frame)
    }
    pub fn dispose(
        &mut self,
        engine: &mut SceneEngine,
    ) -> Result<Vec<ResourceEvent>, ResourceError> {
        self.canvases.dispose(engine);
        self.resolved.clear();
        self.images.clear();
        self.playbacks.clear();
        self.timelines.clear();
        self.tracker.dispose(engine)
    }
}
fn text_request(text: &Text) -> TextMeasureRequest {
    TextMeasureRequest {
        content: text.content.clone(),
        font_family: text.font_family.clone(),
        font_size: text.font_size,
        font_weight: text.font_weight,
        italic: text.italic,
        text_case: crate::text_measure::text_case(text.text_case),
        para_spacing: text.para_spacing,
        line_height: text.font_size * text.line_height,
        wrap_width: text.wrap.then_some(text.content_bounds().size.width),
    }
}
/// The URI check prevents accidental delegation of imported remote references.
/// Local paths are always passed as Resource::Path, never GPUI's str conversion.
pub fn local_image_resource(source: &str) -> Option<Resource> {
    if source.is_empty() {
        return None;
    }
    // UNC/network shares and Windows device/NT namespaces are paths, not URIs,
    // but need explicit host authorization instead of the local-file default.
    if source.starts_with(r"\\") || source.starts_with("//") || source.starts_with(r"\??\") {
        return None;
    }
    let bytes = source.as_bytes();
    // Keep native Windows drive paths local even when reviewed on another OS.
    // Resource::Path prevents GPUI from interpreting this as a URI in either case.
    let drive_path = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    let uri = !drive_path
        && source.split_once(':').is_some_and(|(scheme, _)| {
            !scheme.is_empty()
                && scheme.as_bytes()[0].is_ascii_alphabetic()
                && scheme
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
        });
    (!uri).then(|| Resource::from(std::path::PathBuf::from(source)))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn default_policy_is_local_only_and_never_uses_string_uri_conversion() {
        for source in [
            "https://example.test/a.png",
            "HTTP://example.test/a.png",
            "data:image/png;base64,AA",
            "file:///tmp/a",
            "custom:asset",
            "",
        ] {
            assert!(local_image_resource(source).is_none(), "{source}");
        }
        assert!(matches!(
            local_image_resource("assets/a.png"),
            Some(Resource::Path(_))
        ));
        assert!(matches!(
            local_image_resource("/tmp/a.png"),
            Some(Resource::Path(_))
        ));
        assert!(matches!(
            local_image_resource(r"C:\images\a.png"),
            Some(Resource::Path(_))
        ));
        assert!(matches!(
            local_image_resource("C:/images/a.png"),
            Some(Resource::Path(_))
        ));
    }
    #[::core::prelude::v1::test]
    fn network_and_device_paths_require_explicit_host_authorization() {
        for source in [
            r"\\server\share\image.png",
            "//server/share/image.png",
            r"\\?\C:\images\a.png",
            r"\\?\UNC\server\share\a.png",
            r"\\.\device",
            r"\??\C:\images\a.png",
            "//?/C:/images/a.png",
        ] {
            assert!(local_image_resource(source).is_none(), "{source}");
            // A host can intentionally authorize the exact path without URI parsing.
            let allowed = Resource::from(std::path::PathBuf::from(source));
            assert!(matches!(allowed, Resource::Path(_)));
        }
    }
    #[::core::prelude::v1::test]
    fn text_request_preserves_canonical_shaping_inputs() {
        let mut text = Text::new("", gpui_flow_scene::Size::new(84., 30.));
        text.font_family = "Test Font".into();
        text.font_weight = 650;
        text.text_case = gpui_flow_scene::display::TextCase::Lower;
        text.para_spacing = 12.;
        text.font_size = 18.;
        text.line_height = 1.5;
        let request = text_request(&text);
        assert_eq!(request.content, "");
        assert_eq!(request.font_family, "Test Font");
        assert_eq!(request.font_weight, 650);
        assert_eq!(request.text_case, gpui_flow_text::TextCase::Lower);
        assert_eq!(request.para_spacing, 12.);
        assert_eq!(request.line_height, 27.);
        assert_eq!(request.wrap_width, Some(84.));
        text.wrap = false;
        assert_eq!(text_request(&text).wrap_width, None);
    }
    fn image_engine() -> (SceneEngine, SceneResourceAdapter) {
        let mut s = gpui_flow_scene::Scene::new();
        s.insert(
            gpui_flow_scene::DisplayObject::new(
                "image",
                gpui_flow_scene::display::Image::new("a.png", gpui_flow_scene::Size::new(10., 10.)),
            ),
            None,
            None,
        )
        .unwrap();
        let mut e = SceneEngine::new(s).unwrap();
        let mut r = SceneResourceAdapter::new(&mut e).unwrap();
        r.tracker.synchronize(&mut e).unwrap();
        (e, r)
    }
    fn loaded_image() -> SceneImagePresentation {
        SceneImagePresentation::Loaded(Arc::new(RenderImage::new(vec![image::Frame::new(
            image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 255, 255])),
        )])))
    }
    #[::core::prelude::v1::test]
    fn composite_observation_rejects_stale_source_a_b_a_completion() {
        let (mut e, mut r) = image_engine();
        let id = "image".into();
        let a = r.composite_image(&e, &id, "a.png");
        assert!(matches!(
            a.image,
            Err(gpui_flow_composite::CompositeError::ImagePending(_))
        ));
        let ticket = a.ticket.unwrap();
        for source in ["b.png", "a.png"] {
            e.transaction(|s| {
                s.update_object(&id, |o| {
                    if let Shape::Image(i) = &mut o.shape {
                        i.source = source.into()
                    }
                })
            })
            .unwrap();
        }
        r.settle(
            &mut e,
            ticket,
            ResourceOutcome::Loaded,
            Some(loaded_image()),
            &mut vec![],
        )
        .unwrap();
        r.tracker.synchronize(&mut e).unwrap();
        let current = r.composite_image(&e, &id, "a.png");
        assert_ne!(a.ticket, current.ticket);
        assert!(current.image.is_err());
        r.settle(
            &mut e,
            current.ticket.unwrap(),
            ResourceOutcome::Loaded,
            Some(loaded_image()),
            &mut vec![],
        )
        .unwrap();
        assert!(r.composite_image(&e, &id, "a.png").image.is_ok());
        assert!(matches!(
            r.composite_image(&e, &id, "b.png").image,
            Err(gpui_flow_composite::CompositeError::ImageUnavailable { .. })
        ));
    }
    #[::core::prelude::v1::test]
    fn composite_observation_resolver_revocation_cannot_retain_loaded_content() {
        let (mut e, mut r) = image_engine();
        let id = "image".into();
        let ticket = r.composite_image(&e, &id, "a.png").ticket.unwrap();
        r.settle(
            &mut e,
            ticket,
            ResourceOutcome::Loaded,
            Some(loaded_image()),
            &mut vec![],
        )
        .unwrap();
        let loaded = r.composite_image(&e, &id, "a.png");
        assert!(loaded.image.is_ok());
        r.set_image_resolver(&e, Rc::new(|_| None)).unwrap();
        r.tracker.synchronize(&mut e).unwrap();
        let pending = r.composite_image(&e, &id, "a.png");
        assert_ne!(loaded.ticket, pending.ticket);
        assert!(pending.image.is_err());
        r.settle(
            &mut e,
            pending.ticket.unwrap(),
            ResourceOutcome::Cancelled(ResourceCancellation::NotAuthorized),
            Some(SceneImagePresentation::Cancelled),
            &mut vec![],
        )
        .unwrap();
        assert!(matches!(
            r.composite_image(&e, &id, "a.png").image,
            Err(gpui_flow_composite::CompositeError::ImageUnavailable { .. })
        ));
    }
    #[::core::prelude::v1::test]
    fn playback_is_ticket_bound_and_seek_source_reset_cannot_reuse_old_samples() {
        let (mut e, mut r) = image_engine();
        let id = "image".into();
        let handle = e.handle(&id).unwrap();
        let ticket = r.composite_image(&e, &id, "a.png").ticket.unwrap();
        let image = Arc::new(RenderImage::new(vec![
            image::Frame::from_parts(
                image::RgbaImage::new(1, 1),
                0,
                0,
                image::Delay::from_numer_denom_ms(20, 1),
            ),
            image::Frame::from_parts(
                image::RgbaImage::new(1, 1),
                0,
                0,
                image::Delay::from_numer_denom_ms(80, 1),
            ),
        ]));
        r.settle(
            &mut e,
            ticket,
            ResourceOutcome::Loaded,
            Some(SceneImagePresentation::Loaded(image)),
            &mut vec![],
        )
        .unwrap();
        r.synchronize_playbacks();
        let visible = BTreeSet::from([id.clone()]);
        assert!(r.sample_images(&e, &visible, Duration::ZERO, true));
        r.sample_images(&e, &visible, Duration::from_millis(20), true);
        let playing = r.composite_image(&e, &id, "a.png");
        assert_eq!(playing.frame_index, 1);
        assert_eq!(r.image_frame(&e, handle), playing.frame_index);
        assert!(r.set_image_playing(&e, handle, Duration::from_millis(30), false));
        assert!(!r.sample_images(&e, &visible, Duration::from_secs(30), true));
        assert_eq!(r.image_frame(&e, handle), 1);
        assert!(r.seek_image(&e, handle, Duration::ZERO));
        let sought = r.composite_image(&e, &id, "a.png");
        assert_eq!(sought.frame_index, 0);
        assert!(!playing.resource_eq(&sought));
        r.set_image_resolver(&e, Rc::new(|_| None)).unwrap();
        r.tracker.synchronize(&mut e).unwrap();
        assert!(r.playbacks.is_empty());
        assert_ne!(r.composite_image(&e, &id, "a.png").ticket, Some(ticket));
        assert!(!r.seek_image(&e, handle, Duration::ZERO));
    }

    #[::core::prelude::v1::test]
    fn released_timeline_capacity_recovers_a_still_loaded_rejected_image() {
        let mut scene = gpui_flow_scene::Scene::new();
        for i in 0..17 {
            scene
                .insert(
                    gpui_flow_scene::DisplayObject::new(
                        format!("i{i}"),
                        gpui_flow_scene::display::Image::new(
                            format!("{i}.gif"),
                            gpui_flow_scene::Size::new(1., 1.),
                        ),
                    ),
                    None,
                    None,
                )
                .unwrap();
        }
        let mut engine = SceneEngine::new(scene).unwrap();
        let mut r = SceneResourceAdapter::new(&mut engine).unwrap();
        r.tracker.synchronize(&mut engine).unwrap();
        let mut tickets = vec![];
        for i in 0..17 {
            let handle = engine.handle(&format!("i{i}").into()).unwrap();
            let ticket = r.tracker.ticket(&engine, handle).unwrap();
            tickets.push(ticket);
            let image = Arc::new(RenderImage::new(
                (0..crate::image_playback::MAX_IMAGE_FRAMES)
                    .map(|_| image::Frame::new(image::RgbaImage::new(1, 1)))
                    .collect::<Vec<_>>(),
            ));
            r.settle(
                &mut engine,
                ticket,
                ResourceOutcome::Loaded,
                Some(SceneImagePresentation::Loaded(image)),
                &mut vec![],
            )
            .unwrap();
        }
        r.synchronize_playbacks();
        assert_eq!(r.playbacks.len(), 16);
        let rejected = *tickets
            .iter()
            .find(|t| !r.playbacks.contains_key(t))
            .unwrap();
        let admitted = *r.playbacks.keys().next().unwrap();
        r.images.remove(&admitted);
        r.synchronize_playbacks();
        assert!(r.playbacks.contains_key(&rejected));
        assert_eq!(r.playbacks.len(), 16);
    }
}

#[cfg(test)]
mod canvas_tests;

#[cfg(test)]
mod image_hit_tests;

#[cfg(test)]
mod text_box_tests {
    use super::*;
    #[test]
    fn text_box_resource_requests_use_inner_wrap_width() {
        let mut text = Text::new("source", gpui_flow_scene::Size::new(100., 60.));
        text.padding = [3., 5., 7., 11.];
        assert_eq!(text_request(&text).wrap_width, Some(84.));
        text.wrap = false;
        assert_eq!(text_request(&text).wrap_width, None);
    }
}

//! GPUI producer for the engine's runtime resource lifecycle. This adapter uses
//! GPUI's public asset subscription/cache and the existing shared text shaper.
//! It does not fetch, decode, or poll private cache state itself.
use gpui::{AnyImageCache, App, ImgResourceLoader, RenderImage, Resource, Window};
use gpui_flow_engine::{
    ElementHandle, ResourceCancellation, ResourceCompletion, ResourceError, ResourceEvent,
    ResourceFrame, ResourceKind, ResourceOutcome, ResourceState, ResourceTicket, ResourceViewEvent,
    SceneEngine, SceneResources,
};
use gpui_flow_scene::{Shape, display::Text};
use gpui_flow_text::TextMeasureRequest;
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::Arc,
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
}
/// Runtime-only state; no decoded image or authorization enters Scene JSON.
pub struct SceneResourceAdapter {
    tracker: SceneResources,
    resolver: SceneImageResolver,
    cache: Option<AnyImageCache>,
    resolved: BTreeMap<ResourceTicket, Resource>,
    images: BTreeMap<ResourceTicket, SceneImagePresentation>,
}
impl SceneResourceAdapter {
    pub fn new(engine: &mut SceneEngine) -> Result<Self, ResourceError> {
        Ok(Self {
            tracker: SceneResources::new(engine)?,
            resolver: Rc::new(local_image_resource),
            cache: None,
            resolved: BTreeMap::new(),
            images: BTreeMap::new(),
        })
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
        let mut events = self.tracker.synchronize(engine)?;
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
                        crate::text_measure::shape_scene_text(
                            window.text_system(),
                            &request,
                            gpui::Hsla::default(),
                        )
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
    pub fn image(&self, engine: &SceneEngine, handle: ElementHandle) -> SceneImagePresentation {
        self.tracker
            .ticket(engine, handle)
            .and_then(|ticket| self.images.get(&ticket).cloned())
            .unwrap_or(SceneImagePresentation::Pending)
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
        CompositeImageObservation { ticket, image }
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
        self.resolved.clear();
        self.images.clear();
        self.tracker.dispose(engine)
    }
}
fn text_request(text: &Text) -> TextMeasureRequest {
    TextMeasureRequest {
        content: text.content.clone(),
        font_family: text.font_family.clone(),
        font_size: text.font_size,
        font_weight: text.font_weight,
        line_height: text.font_size * text.line_height,
        wrap_width: text.wrap.then_some(text.size.width),
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
        text.font_size = 18.;
        text.line_height = 1.5;
        let request = text_request(&text);
        assert_eq!(request.content, "");
        assert_eq!(request.font_family, "Test Font");
        assert_eq!(request.font_weight, 650);
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
}

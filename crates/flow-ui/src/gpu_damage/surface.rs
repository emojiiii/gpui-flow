use super::*;
use gpui_flow_composite::RasterViewport;
use gpui_flow_core::Viewport;
use gpui_flow_scene::{Affine, Bounds as WorldBounds, Point as WorldPoint, Size as WorldSize};
pub(crate) struct SurfaceElement {
    pub child: AnyElement,
    pub surface: CompositionSurfaceId,
    pub input: Rc<RefCell<Input>>,
    pub key: FrameKey,
    pub viewport: Viewport,
    pub eligible: bool,
}
impl Element for SurfaceElement {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let mask = window.content_mask().bounds;
        let world = |b: Bounds<Pixels>| {
            WorldBounds::new(
                WorldPoint::new(b.origin.x.into(), b.origin.y.into()),
                WorldSize::new(b.size.width.into(), b.size.height.into()),
            )
        };
        self.key.viewport = RasterViewport {
            world_to_screen: Affine::translation(bounds.origin.x.into(), bounds.origin.y.into())
                * Affine::translation(self.viewport.pan.x, self.viewport.pan.y)
                * Affine::scale(self.viewport.zoom, self.viewport.zoom),
            canvas_bounds: world(bounds),
            content_mask: world(mask),
            device_scale: window.scale_factor(),
        };
        self.input.borrow_mut().request = Some(Request {
            key: self.key.clone(),
            eligible: self.eligible,
        });
        // This composition surface is pre-created and remains live until its
        // owning binding is disposed. Failure is visible and uses full painting.
        let mut painted = false;
        let result = window.with_composition_surface(self.surface, |window| {
            self.child.paint(window, cx);
            painted = true;
        });
        if result.is_err() && !painted {
            self.input.borrow_mut().cache.invalidate();
            self.child.paint(window, cx);
        }
    }
}
impl Binding {
    pub(crate) fn element(
        &self,
        child: AnyElement,
        engine: &SceneEngine,
        viewport: Viewport,
        background: u32,
        grid: Option<u32>,
        eligible: bool,
    ) -> AnyElement {
        SurfaceElement {
            child,
            surface: self.surface.id(),
            input: self.input.clone(),
            key: FrameKey {
                engine: engine.engine_id(),
                document: engine.document_epoch(),
                revision: engine.revision(),
                viewport: RasterViewport {
                    world_to_screen: Affine::IDENTITY,
                    canvas_bounds: WorldBounds::default(),
                    content_mask: WorldBounds::default(),
                    device_scale: 1.,
                },
                background,
                grid,
            },
            viewport,
            eligible: eligible && self.matches(engine),
        }
        .into_any_element()
    }
}

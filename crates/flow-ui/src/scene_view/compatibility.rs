//! Explicit construction helpers for a bounded, pinned Leafer behavior subset.
//! No profile is inferred from document JSON, and existing Flow defaults remain.
use super::*;
use gpui_flow_interaction::scene::{
    PointerGestureProfile, SceneEventRouter, TextEscapeBehavior,
    plugins::{SceneDragPlugin, SceneResizePlugin, TextResizeMode},
};
use gpui_flow_layout::{ContentBoundsPolicy, FlowStyle, WrapGapPolicy};
use gpui_flow_scene::{ContainerPaintOrder, Shape};

/// Supported upstream defaults, pinned by docs/leafer-task-ledger.json.
/// See docs/leafer-compatibility-profile.md for the deliberate remaining boundaries.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LeaferCompatibilityProfile;

/// Verified plain-container conditions supplied by the host. Use only for
/// ordinary Box/Frame content without masks, erasers or effects. This describes
/// upstream __single / __clipAfterFill conditions, not a general mask adapter.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LeaferPlainContainerContext {
    pub has_children: bool,
    pub children_overflow: bool,
    pub isolated: bool,
}
impl LeaferCompatibilityProfile {
    pub fn text_escape_behavior(self) -> TextEscapeBehavior {
        TextEscapeBehavior::Finish
    }
    /// Intrinsic resizing for existing admitted single-selection built-ins;
    /// plain Text resizes its box; Groups scale child transforms while keeping
    /// their own pose. Multi-selection retains the shared-transform fallback.
    pub fn resize_plugin(self) -> SceneResizePlugin {
        SceneResizePlugin::default()
            .with_builtin_resize(true)
            .with_text_resize_mode(TextResizeMode::Box)
    }
    pub fn drag_plugin(self) -> SceneDragPlugin {
        let mut plugin = SceneDragPlugin::default();
        plugin.threshold_pixels = 2.0;
        plugin
    }
    pub fn event_router(self) -> SceneEventRouter {
        SceneEventRouter::with_gesture_profile(PointerGestureProfile::LeaferMouse)
    }
    /// Apply only the two layout compatibility policies; preserve authored gaps,
    /// direction, sizing and all finite-value/constraint validation.
    pub fn configure_flow_style(self, style: &mut FlowStyle) {
        style.content_bounds_policy = ContentBoundsPolicy::LeaferShrink;
        style.wrap_gap_policy = WrapGapPolicy::LeaferExcludePendingGap;
    }
    pub fn flow_style(self) -> FlowStyle {
        let mut style = FlowStyle::default();
        self.configure_flow_style(&mut style);
        style
    }
    /// Explicit helper for supported plain containers. The host must verify the
    /// context above; arbitrary masks/effects cannot be inferred from Shape.
    /// Returns false for a non-container and leaves it unchanged.
    pub fn configure_plain_container_paint(
        self,
        shape: &mut Shape,
        context: LeaferPlainContainerContext,
    ) -> bool {
        let (clips, order) = match shape {
            Shape::Box(shape) => (shape.clip_children, &mut shape.paint_order),
            Shape::Frame(shape) => (shape.clip_children, &mut shape.paint_order),
            _ => return false,
        };
        *order = if context.isolated || (clips && context.has_children && context.children_overflow)
        {
            ContainerPaintOrder::StrokeAfterChildren
        } else {
            ContainerPaintOrder::BeforeChildren
        };
        true
    }
}
impl SceneView {
    /// Construct an opt-in compatible view. Layout and paint authoring helpers
    /// are explicit: opening a document never silently rewrites its contents.
    pub fn new_with_profile(
        scene: Scene,
        profile: LeaferCompatibilityProfile,
        cx: &mut Context<Self>,
    ) -> Result<Self, SceneError> {
        Ok(Self::from_engine_with_profile(
            gpui_flow_engine::SceneEngine::new(scene)?,
            profile,
            cx,
        ))
    }
    pub fn from_engine_with_profile(
        engine: gpui_flow_engine::SceneEngine,
        profile: LeaferCompatibilityProfile,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::from_engine(engine, cx);
        view.interactions
            .replace(profile.resize_plugin(), &mut view.editor)
            .expect("fresh default host has resize plugin");
        view.interactions
            .replace(profile.drag_plugin(), &mut view.editor)
            .expect("fresh default host has drag plugin");
        view.router = profile.event_router();
        view.text_escape_behavior = profile.text_escape_behavior();
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn helpers_are_opt_in_and_preserve_unrelated_authored_values() {
        let profile = LeaferCompatibilityProfile;
        let mut style = FlowStyle {
            gap: 12.0,
            wrap: true,
            ..Default::default()
        };
        assert_eq!(style.wrap_gap_policy, WrapGapPolicy::IncludePendingGap);
        profile.configure_flow_style(&mut style);
        assert_eq!(style.gap, 12.0);
        assert!(style.wrap);
        assert_eq!(
            style.content_bounds_policy,
            ContentBoundsPolicy::LeaferShrink
        );
        assert_eq!(
            style.wrap_gap_policy,
            WrapGapPolicy::LeaferExcludePendingGap
        );
        assert_eq!(profile.text_escape_behavior(), TextEscapeBehavior::Finish);
        assert_eq!(
            profile.event_router().gesture_profile(),
            PointerGestureProfile::LeaferMouse
        );
        assert_eq!(profile.drag_plugin().threshold_pixels, 2.0);
    }
    #[::core::prelude::v1::test]
    fn profile_resizes_actual_rect_and_text_boxes_without_scaling_their_matrix() {
        use gpui_flow_interaction::{CanvasEvent, HitTarget, PointerInput, scene::ScenePluginHost};
        use gpui_flow_scene::{Affine, DisplayObject, Point, Size, display};
        for shape in [
            Shape::Rect(display::Rect::new(100.0, 50.0)),
            Shape::Text(display::Text::new("text", Size::new(100.0, 50.0))),
        ] {
            let mut scene = Scene::new();
            scene
                .insert(DisplayObject::new("s", shape), None, None)
                .unwrap();
            let mut editor = SceneEditor::new(scene).unwrap();
            editor.selection.select("s".into(), false);
            let mut host = ScenePluginHost::new();
            host.install(LeaferCompatibilityProfile.resize_plugin())
                .unwrap();
            let down = PointerInput::left(
                Point::new(100.0, 50.0),
                HitTarget::ObjectResizeHandle {
                    id: "s".into(),
                    handle: gpui_flow_editor::ResizeHandle::BottomRight,
                },
            );
            let moved = PointerInput::left(Point::new(200.0, 100.0), HitTarget::Background);
            for event in [
                CanvasEvent::PointerDown(down),
                CanvasEvent::PointerMove(moved.clone()),
                CanvasEvent::PointerUp(moved),
            ] {
                let report = host.dispatch(&mut editor, event);
                assert!(report.errors.is_empty(), "{:?}", report.errors);
            }
            let object = &editor.scene().get(&"s".into()).unwrap().object;
            assert_eq!(object.transform, Affine::IDENTITY);
            assert_eq!(
                object.shape.local_bounds().unwrap().size,
                Size::new(200.0, 100.0)
            );
            assert!(editor.undo());
            assert_eq!(
                editor
                    .scene()
                    .get(&"s".into())
                    .unwrap()
                    .object
                    .shape
                    .local_bounds()
                    .unwrap()
                    .size,
                Size::new(100.0, 50.0)
            );
            assert!(!editor.undo());
        }
    }
    #[::core::prelude::v1::test]
    fn plain_container_paint_follows_explicit_clip_overflow_or_isolation() {
        for frame in [false, true] {
            for has_children in [false, true] {
                for children_overflow in [false, true] {
                    for isolated in [false, true] {
                        let mut shape = if frame {
                            Shape::Frame(gpui_flow_scene::display::Frame::new(40.0, 40.0))
                        } else {
                            Shape::Box(gpui_flow_scene::display::Box::new(40.0, 40.0))
                        };
                        let context = LeaferPlainContainerContext {
                            has_children,
                            children_overflow,
                            isolated,
                        };
                        assert!(
                            LeaferCompatibilityProfile
                                .configure_plain_container_paint(&mut shape, context)
                        );
                        let actual = match shape {
                            Shape::Box(shape) => shape.paint_order,
                            Shape::Frame(shape) => shape.paint_order,
                            _ => unreachable!(),
                        };
                        assert_eq!(
                            actual,
                            if isolated || (frame && has_children && children_overflow) {
                                ContainerPaintOrder::StrokeAfterChildren
                            } else {
                                ContainerPaintOrder::BeforeChildren
                            }
                        );
                    }
                }
            }
        }
        let mut shape = Shape::Rect(gpui_flow_scene::display::Rect::new(40.0, 40.0));
        let before = shape.clone();
        assert!(
            !LeaferCompatibilityProfile
                .configure_plain_container_paint(&mut shape, Default::default())
        );
        assert_eq!(shape, before);
    }
}

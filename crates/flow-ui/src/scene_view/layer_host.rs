//! Private bridge: layered surfaces use the exact standalone hit/router/render path.
use super::*;
use gpui_flow_engine::{ElementHandle, EngineError};
use gpui_flow_interaction::{CanvasEvent, HitTarget, scene::SceneEventRouter};

#[derive(Default)]
pub(super) enum LayerHostState {
    #[default]
    Standalone,
    Managed(u64),
    Destroyed,
}
impl SceneView {
    /// Prefer this checked setter over direct field assignment. Overlapping
    /// layers cannot own an opaque whole-canvas compositing backdrop.
    pub fn set_compositing_mode(
        &mut self,
        mode: SceneCompositing,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        check_compositing_mode(&self.layer_host, mode)?;
        if self.compositing_mode != mode {
            self.compositing_mode = mode;
            cx.notify();
        }
        Ok(())
    }
    /// Preserve the legacy public field while rejecting an unsupported direct
    /// assignment explicitly at the next render, rather than dropping its backdrop.
    pub(crate) fn layer_reconcile_mode(&mut self) {
        if self.is_layer_managed() && self.compositing_mode == SceneCompositing::OwnedCanvas {
            self.compositing_mode = SceneCompositing::Domains;
            self.interaction_error = Some("OwnedCanvas rejected: detach this layer before enabling an opaque whole-canvas backdrop; restored Domains".into());
        }
    }
    pub(crate) fn is_layer_managed(&self) -> bool {
        !matches!(self.layer_host, LayerHostState::Standalone)
    }
    pub(crate) fn layer_mount(&mut self, owner: u64, cx: &mut Context<Self>) -> bool {
        #[cfg(all(feature = "gpu-damage", target_os = "linux"))]
        if self.gpu_damage.is_some() {
            return false;
        }
        if !mountable(&self.layer_host, self.compositing_mode)
            || self.inner_tool_active()
            || self.has_tool_widget()
            || self.image_crop_editor_active()
            || self.pending_flow_edits() > 0
        {
            return false;
        }
        self.focus_lost(cx);
        self.router = SceneEventRouter::with_gesture_profile(self.router.gesture_profile());
        self.pending_native_focus = false;
        self.subscriptions.clear();
        self.interaction_window = None;
        self.layer_host = LayerHostState::Managed(owner);
        cx.notify();
        true
    }
    pub(crate) fn layer_unmount(&mut self, owner: u64, cx: &mut Context<Self>) {
        if !matches!(self.layer_host, LayerHostState::Managed(id) if id == owner) {
            return;
        }
        let _ = self.cancel_flow_frame(cx);
        self.focus_lost(cx);
        // Close outstanding action senders. Reattachment cannot replay old input.
        self.router = SceneEventRouter::with_gesture_profile(self.router.gesture_profile());
        self.pending_native_focus = false;
        self.layer_host = LayerHostState::Standalone;
        cx.notify();
    }
    pub(crate) fn layer_destroy(&mut self, owner: u64, cx: &mut Context<Self>) {
        if !matches!(self.layer_host, LayerHostState::Managed(id) if id == owner) {
            return;
        }
        self.navigation_frames.cancel();
        let _ = self.set_flow_frame_queue(None, cx);
        self.focus_lost(cx);
        self.router = SceneEventRouter::with_gesture_profile(self.router.gesture_profile());
        if let Some(mut tools) = self.tools.take() {
            let _ = tools.dispose(&mut self.editor, &mut self.interactions);
        }
        self.text_input = None;
        self.dispose_tool_widgets(cx);
        self.image_crop = None;
        self.compositing.dispose(cx);
        self.presentations = Default::default();
        release_composite_keys(&mut self.compositing);
        if let Some(mut resources) = self.resources.take() {
            let _ = resources.dispose(&mut self.editor.engine);
        }
        self.subscriptions.clear();
        self.interaction_window = None;
        // Drop the old engine, including detached trees, scoped listeners and
        // disposers, even when a trusted host retained a clone of this Entity.
        self.editor = SceneEditor::new(Scene::new()).expect("empty scene is valid");
        self.state_styles =
            gpui_flow_interaction::scene::SceneStateStyles::new(&self.editor.engine);
        self.interactions = gpui_flow_interaction::scene::ScenePluginHost::new();
        self.image_alpha_hits = Default::default();
        self.renderers = Default::default();
        self.diagnostics.clear();
        self.gradient_diagnostics.clear();
        self.composite_diagnostics.clear();
        self.compositing_pending = false;
        self.composite_geometry = None;
        self.text_rasterizer = None;
        self.resource_error = None;
        self.interaction_error = None;
        self.state_style_error = None;
        self.pending_native_focus = false;
        self.layer_host = LayerHostState::Destroyed;
        cx.notify();
    }
    pub(crate) fn layer_cursor(&self) -> CursorStyle {
        self.input_cursor()
    }
    pub(crate) fn layer_hit(&self, local: WorldPoint) -> HitTarget {
        self.input_hit(local, false)
    }
    pub(crate) fn layer_dispatch(&mut self, event: CanvasEvent, cx: &mut Context<Self>) {
        if matches!(event, CanvasEvent::FocusLost)
            && let Some(resources) = &mut self.resources
        {
            resources.freeze_images(self.state_clock.elapsed());
        }
        self.plugin_event(event, cx);
        // LayeredApp owns native focus. The child router still owns all logical
        // Focus/Blur notifications, including queued element focus requests.
        self.pending_native_focus = false;
    }
    pub(crate) fn layer_focus(
        &mut self,
        handle: ElementHandle,
        cx: &mut Context<Self>,
    ) -> Result<bool, EngineError> {
        let report =
            self.router
                .set_focus(&mut self.editor, &mut self.interactions, Some(handle))?;
        self.apply_routed_report(report, cx);
        self.pending_native_focus = false;
        Ok(self.router.focused(&self.editor.engine) == Some(handle))
    }
}

fn check_compositing_mode(state: &LayerHostState, mode: SceneCompositing) -> Result<(), String> {
    match state {
        LayerHostState::Destroyed => Err("destroyed layer cannot change compositing mode".into()),
        LayerHostState::Managed(_) if mode == SceneCompositing::OwnedCanvas => Err(
            "OwnedCanvas is unsupported while mounted in LayeredApp; detach the layer first".into(),
        ),
        _ => Ok(()),
    }
}
// dispose retires native images; replacing also releases requested/completed
// domain keys and their decoded image/text/source snapshots.
fn release_composite_keys(controller: &mut crate::display::composite::CompositeController) {
    *controller = Default::default();
}
fn mountable(state: &LayerHostState, mode: SceneCompositing) -> bool {
    matches!(state, LayerHostState::Standalone) && mode == SceneCompositing::Domains
}
#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn managed_compositing_setter_rejects_owned_mode_and_destroyed_views() {
        assert!(
            check_compositing_mode(&LayerHostState::Standalone, SceneCompositing::OwnedCanvas)
                .is_ok()
        );
        assert!(
            check_compositing_mode(&LayerHostState::Managed(1), SceneCompositing::Domains).is_ok()
        );
        assert!(
            check_compositing_mode(&LayerHostState::Managed(1), SceneCompositing::OwnedCanvas)
                .is_err()
        );
        assert!(
            check_compositing_mode(&LayerHostState::Destroyed, SceneCompositing::Domains).is_err()
        );
    }
    #[::core::prelude::v1::test]
    fn terminal_composite_reset_drops_retained_request_snapshot() {
        use crate::display::composite::{CompositeController, RequestKey};
        use gpui_flow_composite::RasterViewport;
        use gpui_flow_scene::{Affine, Bounds, Point, Size};
        let engine = gpui_flow_engine::SceneEngine::new(Scene::new()).unwrap();
        let bounds = Bounds::new(Point::ZERO, Size::new(10., 10.));
        let key = std::sync::Arc::new(RequestKey::new(
            vec![],
            &engine,
            RasterViewport {
                world_to_screen: Affine::IDENTITY,
                canvas_bounds: bounds,
                content_mask: bounds,
                device_scale: 1.,
            },
            1,
        ));
        let weak = std::sync::Arc::downgrade(&key);
        let mut controller = CompositeController::default();
        controller.requested = Some(key);
        assert!(weak.upgrade().is_some());
        release_composite_keys(&mut controller);
        assert!(weak.upgrade().is_none());
        assert!(controller.requested.is_none());
    }
    #[::core::prelude::v1::test]
    fn rejects_owned_canvas_and_live_or_destroyed_owners() {
        assert!(mountable(
            &LayerHostState::Standalone,
            SceneCompositing::Domains
        ));
        assert!(!mountable(
            &LayerHostState::Standalone,
            SceneCompositing::OwnedCanvas
        ));
        assert!(!mountable(
            &LayerHostState::Managed(1),
            SceneCompositing::Domains
        ));
        assert!(!mountable(
            &LayerHostState::Destroyed,
            SceneCompositing::Domains
        ));
    }
}

use super::SceneWheelPolicy;
use crate::scene::SceneCommand;
use crate::*;
use gpui_flow_editor::SceneEditor;
pub struct SceneZoomPlugin {
    pub sensitivity: f32,
    wheel: SceneWheelPolicy,
}
impl Default for SceneZoomPlugin {
    fn default() -> Self {
        Self {
            sensitivity: 0.002,
            wheel: SceneWheelPolicy::default(),
        }
    }
}
impl SceneZoomPlugin {
    pub fn with_wheel_policy(mut self, policy: SceneWheelPolicy) -> Self {
        self.wheel = policy;
        self
    }
}
impl CanvasPlugin<SceneEditor, SceneCommand> for SceneZoomPlugin {
    fn id(&self) -> &str {
        "zoom"
    }
    fn priority(&self) -> i32 {
        40
    }
    fn is_default_action(&self) -> bool {
        true
    }
    fn on_event(
        &mut self,
        event: &CanvasEvent,
        cx: &PluginContext<'_, SceneEditor>,
        out: &mut PluginOutput<SceneCommand>,
    ) {
        if !cx.allows(Capability::Navigation) {
            return;
        }
        if let CanvasEvent::Wheel {
            position,
            delta,
            modifiers,
        } = event
            && self.wheel.zooms(modifiers.command())
            && self.sensitivity.is_finite()
            && self.sensitivity >= 0.0
            && delta.is_finite()
        {
            let zoom = (cx.editor.viewport.zoom as f64
                * (-(delta.y as f64) * self.sensitivity as f64).exp())
            .clamp(
                cx.editor.viewport.min_zoom as f64,
                cx.editor.viewport.max_zoom as f64,
            ) as f32;
            out.command(SceneCommand::Zoom {
                position: *position,
                zoom,
            });
        }
    }
}

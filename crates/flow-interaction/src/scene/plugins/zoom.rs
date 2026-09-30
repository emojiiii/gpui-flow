use crate::scene::SceneCommand;
use crate::*;
use gpui_flow_editor::SceneEditor;
pub struct SceneZoomPlugin {
    pub sensitivity: f32,
}
impl Default for SceneZoomPlugin {
    fn default() -> Self {
        Self { sensitivity: 0.002 }
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
            && modifiers.command()
        {
            out.command(SceneCommand::Zoom {
                position: *position,
                zoom: (cx.editor.viewport.zoom * (-delta.y * self.sensitivity).exp())
                    .clamp(cx.editor.viewport.min_zoom, cx.editor.viewport.max_zoom),
            });
        }
    }
}

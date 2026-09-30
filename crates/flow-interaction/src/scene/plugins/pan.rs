use crate::scene::SceneCommand;
use crate::*;
use gpui_flow_core::Point;
use gpui_flow_editor::SceneEditor;
#[derive(Default)]
pub struct ScenePanPlugin {
    last: Option<Point>,
}
impl CanvasPlugin<SceneEditor, SceneCommand> for ScenePanPlugin {
    fn id(&self) -> &str {
        "pan"
    }
    fn priority(&self) -> i32 {
        -20
    }
    fn is_default_action(&self) -> bool {
        true
    }
    fn on_lifecycle(&mut self, event: PluginLifecycle) {
        if matches!(
            event,
            PluginLifecycle::Cancelled | PluginLifecycle::Disabled | PluginLifecycle::Uninstalled
        ) {
            self.last = None;
        }
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
        match event {
            CanvasEvent::PointerDown(p)
                if p.hit != HitTarget::Control
                    && (p.button == Some(PointerButton::Middle)
                        || (p.button == Some(PointerButton::Left)
                            && (p.modifiers.space
                                || (p.hit == HitTarget::Background && !p.modifiers.shift)))) =>
            {
                self.last = Some(p.position);
                out.command(SceneCommand::CapturePointer {
                    pointer_id: p.pointer_id,
                });
                out.effect(CanvasEffect::Cursor(CanvasCursor::Grabbing));
            }
            CanvasEvent::PointerMove(p) => {
                if let Some(last) = self.last.replace(p.position) {
                    out.command(SceneCommand::Pan {
                        delta: p.position - last,
                    });
                } else {
                    self.last = None;
                }
            }
            CanvasEvent::PointerUp(_) => {
                if self.last.take().is_some() {
                    out.command(SceneCommand::ReleasePointer);
                    out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
                }
            }
            CanvasEvent::Wheel {
                delta, modifiers, ..
            } if !modifiers.command() => out.command(SceneCommand::Pan { delta: *delta }),
            _ => {}
        }
    }
}

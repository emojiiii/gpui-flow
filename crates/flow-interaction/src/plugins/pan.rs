use crate::*;
use gpui_flow_core::Point;
#[derive(Default)]
pub struct PanPlugin {
    last: Option<Point>,
}
impl CanvasPlugin for PanPlugin {
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
    fn on_event(&mut self, event: &CanvasEvent, cx: &PluginContext<'_>, out: &mut PluginOutput) {
        if !cx.allows(Capability::Navigation) {
            return;
        }
        match event {
            CanvasEvent::PointerDown(p)
                if p.hit != HitTarget::Control
                    && (p.button == Some(PointerButton::Middle)
                        || (p.button == Some(PointerButton::Left) && p.modifiers.space)) =>
            {
                self.last = Some(p.position);
                out.command(CanvasCommand::CapturePointer {
                    pointer_id: p.pointer_id,
                });
                out.effect(CanvasEffect::Cursor(CanvasCursor::Grabbing));
            }
            CanvasEvent::PointerMove(p) => {
                if let Some(last) = self.last.replace(p.position) {
                    out.command(CanvasCommand::Pan {
                        delta: p.position - last,
                    });
                } else {
                    self.last = None;
                }
            }
            // Releasing the navigation modifier finishes the pan, rather than
            // leaving a capture that a later focus/lost-button event rolls back.
            CanvasEvent::KeyUp {
                key: KeyCode::Space,
                ..
            } if cx.capture_owner == Some("pan") => {
                if self.last.take().is_some() {
                    out.command(CanvasCommand::ReleasePointer);
                    out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
                }
            }
            CanvasEvent::PointerUp(_) => {
                if self.last.take().is_some() {
                    out.command(CanvasCommand::ReleasePointer);
                    out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
                }
            }
            CanvasEvent::Wheel {
                delta, modifiers, ..
            } if !modifiers.command() => out.command(CanvasCommand::Pan { delta: *delta }),
            _ => {}
        }
    }
}

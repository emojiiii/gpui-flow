use crate::*;
use gpui_flow_core::ConnectionDrag;
#[derive(Default)]
pub struct ConnectionPlugin {
    drag: ConnectionDrag,
}
impl CanvasPlugin for ConnectionPlugin {
    fn id(&self) -> &str {
        "connection"
    }
    fn priority(&self) -> i32 {
        10
    }
    fn is_default_action(&self) -> bool {
        true
    }
    fn on_lifecycle(&mut self, event: PluginLifecycle) {
        if matches!(
            event,
            PluginLifecycle::Cancelled | PluginLifecycle::Disabled | PluginLifecycle::Uninstalled
        ) {
            self.drag.cancel();
        }
    }
    fn on_event(&mut self, event: &CanvasEvent, cx: &PluginContext<'_>, out: &mut PluginOutput) {
        if !cx.allows(Capability::Edit) {
            return;
        }
        match event {
            CanvasEvent::PointerDown(p)
                if p.button == Some(PointerButton::Left) && !p.modifiers.space =>
            {
                if let HitTarget::Port(from) = &p.hit {
                    match self.drag.begin(cx.editor.graph(), from.clone(), p.position) {
                        Ok(()) => {
                            out.command(CanvasCommand::CapturePointer {
                                pointer_id: p.pointer_id,
                            });
                            out.command(CanvasCommand::PreviewConnection {
                                from: from.clone(),
                                position: p.position,
                                hovered: None,
                            });
                        }
                        Err(error) => {
                            out.effect(CanvasEffect::ConnectionRejected(error.to_string()))
                        }
                    }
                }
            }
            CanvasEvent::PointerMove(p) => {
                if let Some(from) = self.drag.source().cloned() {
                    let hovered = match &p.hit {
                        HitTarget::Port(port) => Some(port.clone()),
                        _ => None,
                    };
                    self.drag.move_to_exact(
                        cx.editor.graph(),
                        p.position,
                        hovered.clone(),
                        p.button == Some(PointerButton::Left),
                    );
                    if self.drag.active() {
                        out.command(CanvasCommand::PreviewConnection {
                            from,
                            position: p.position,
                            hovered,
                        });
                    } else {
                        out.command(CanvasCommand::ClearConnection);
                        out.command(CanvasCommand::ReleasePointer);
                    }
                }
            }
            CanvasEvent::PointerUp(p) => {
                if let Some(from) = self.drag.source().cloned() {
                    let hovered = match &p.hit {
                        HitTarget::Port(port) => Some(port.clone()),
                        _ => None,
                    };
                    out.command(CanvasCommand::PreviewConnection {
                        from: from.clone(),
                        position: p.position,
                        hovered,
                    });
                    out.command(CanvasCommand::CompleteConnection { from });
                    out.command(CanvasCommand::ReleasePointer);
                    out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
                    self.drag.cancel();
                }
            }
            _ => {}
        }
    }
}

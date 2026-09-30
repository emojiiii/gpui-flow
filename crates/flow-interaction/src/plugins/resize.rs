use crate::*;
use gpui_flow_core::{NodeId, Point, Size};
struct Resize {
    node: NodeId,
    start: Point,
    size: Size,
}
pub struct ResizePlugin {
    pub minimum_size: Size,
    resize: Option<Resize>,
}
impl Default for ResizePlugin {
    fn default() -> Self {
        Self {
            minimum_size: Size::new(80.0, 48.0),
            resize: None,
        }
    }
}
impl CanvasPlugin for ResizePlugin {
    fn id(&self) -> &str {
        "resize"
    }
    fn priority(&self) -> i32 {
        20
    }
    fn is_default_action(&self) -> bool {
        true
    }
    fn on_lifecycle(&mut self, event: PluginLifecycle) {
        if matches!(
            event,
            PluginLifecycle::Cancelled | PluginLifecycle::Disabled | PluginLifecycle::Uninstalled
        ) {
            self.resize = None;
        }
    }
    fn on_event(&mut self, event: &CanvasEvent, cx: &PluginContext<'_>, out: &mut PluginOutput) {
        if !cx.allows(Capability::Edit) {
            return;
        }
        match event {
            CanvasEvent::PointerDown(p) if p.button == Some(PointerButton::Left) => {
                if let HitTarget::ResizeHandle(node) = &p.hit
                    && let Some(target) = cx.editor.graph().nodes.get(node)
                {
                    if target.locked {
                        return;
                    }
                    self.resize = Some(Resize {
                        node: node.clone(),
                        start: cx.editor.viewport.screen_to_world(p.position),
                        size: target.size,
                    });
                    out.command(CanvasCommand::BeginGesture {
                        label: "Resize node".into(),
                    });
                    out.command(CanvasCommand::CapturePointer {
                        pointer_id: p.pointer_id,
                    });
                    out.effect(CanvasEffect::Cursor(CanvasCursor::Resize));
                }
            }
            CanvasEvent::PointerMove(p) => {
                if let Some(resize) = &self.resize {
                    let delta = cx.editor.viewport.screen_to_world(p.position) - resize.start;
                    out.command(CanvasCommand::ResizeNode {
                        node: resize.node.clone(),
                        size: Size::new(
                            (resize.size.width + delta.x).max(self.minimum_size.width),
                            (resize.size.height + delta.y).max(self.minimum_size.height),
                        ),
                    });
                }
            }
            CanvasEvent::PointerUp(_) if self.resize.take().is_some() => {
                out.command(CanvasCommand::CommitGesture);
                out.command(CanvasCommand::ReleasePointer);
                out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
            }
            _ => {}
        }
    }
}

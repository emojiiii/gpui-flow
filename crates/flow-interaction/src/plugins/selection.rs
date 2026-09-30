use crate::*;
use gpui_flow_core::{Point, Rect};
#[derive(Default)]
pub struct SelectionPlugin {
    marquee: Option<(Point, bool)>,
}
impl CanvasPlugin for SelectionPlugin {
    fn id(&self) -> &str {
        "selection"
    }
    fn priority(&self) -> i32 {
        0
    }
    fn is_default_action(&self) -> bool {
        true
    }
    fn on_lifecycle(&mut self, event: PluginLifecycle) {
        if matches!(
            event,
            PluginLifecycle::Cancelled | PluginLifecycle::Disabled | PluginLifecycle::Uninstalled
        ) {
            self.marquee = None;
        }
    }
    fn on_event(&mut self, event: &CanvasEvent, cx: &PluginContext<'_>, out: &mut PluginOutput) {
        if !cx.allows(Capability::Selection) {
            return;
        }
        match event {
            CanvasEvent::PointerDown(p)
                if p.button == Some(PointerButton::Left) && !p.modifiers.space =>
            {
                let mode = if p.modifiers.shift {
                    SelectionMode::Toggle
                } else {
                    SelectionMode::Replace
                };
                match &p.hit {
                    HitTarget::Node(node) => {
                        if p.modifiers.shift
                            || !cx
                                .editor
                                .selection
                                .expanded_nodes(cx.editor.graph())
                                .contains(node)
                        {
                            out.command(CanvasCommand::SelectNode {
                                node: node.clone(),
                                mode,
                            });
                        }
                    }
                    HitTarget::ResizeHandle(node) => {
                        if !cx.editor.selection.nodes.contains(node) {
                            out.command(CanvasCommand::SelectNode {
                                node: node.clone(),
                                mode: SelectionMode::Replace,
                            });
                        }
                    }
                    HitTarget::Edge(edge) => out.command(CanvasCommand::SelectEdge {
                        edge: edge.clone(),
                        mode,
                    }),
                    HitTarget::Group(group) => {
                        if p.modifiers.shift || !cx.editor.selection.groups.contains(group) {
                            out.command(CanvasCommand::SelectGroup {
                                group: group.clone(),
                                mode,
                            });
                        }
                    }
                    HitTarget::Background => {
                        let start = cx.editor.viewport.screen_to_world(p.position);
                        self.marquee = Some((start, p.modifiers.shift));
                        out.command(CanvasCommand::CapturePointer {
                            pointer_id: p.pointer_id,
                        });
                        if !p.modifiers.shift {
                            out.command(CanvasCommand::ClearSelection);
                        }
                        out.effect(CanvasEffect::Marquee(Some(Rect::from_points(start, start))));
                    }
                    _ => {}
                }
            }
            CanvasEvent::PointerMove(p) => {
                if let Some((start, _)) = self.marquee {
                    out.effect(CanvasEffect::Marquee(Some(Rect::from_points(
                        start,
                        cx.editor.viewport.screen_to_world(p.position),
                    ))));
                }
            }
            CanvasEvent::PointerUp(p) => {
                if let Some((start, additive)) = self.marquee.take() {
                    out.command(CanvasCommand::SelectRect {
                        rect: Rect::from_points(
                            start,
                            cx.editor.viewport.screen_to_world(p.position),
                        ),
                        additive,
                        fully_contained: false,
                    });
                    out.command(CanvasCommand::ReleasePointer);
                    out.effect(CanvasEffect::Marquee(None));
                }
            }
            _ => {}
        }
    }
}

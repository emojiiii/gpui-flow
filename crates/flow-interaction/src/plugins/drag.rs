use crate::*;
use gpui_flow_core::{GroupId, NodeId, Point};
use std::collections::BTreeMap;
struct Drag {
    screen_start: Point,
    world_start: Point,
    nodes: BTreeMap<NodeId, Point>,
    groups: BTreeMap<GroupId, Point>,
    started: bool,
}
pub struct DragPlugin {
    pub threshold_pixels: f32,
    drag: Option<Drag>,
}
impl Default for DragPlugin {
    fn default() -> Self {
        Self {
            threshold_pixels: 3.0,
            drag: None,
        }
    }
}
impl CanvasPlugin for DragPlugin {
    fn id(&self) -> &str {
        "drag"
    }
    fn priority(&self) -> i32 {
        30
    }
    fn is_default_action(&self) -> bool {
        true
    }
    fn on_lifecycle(&mut self, event: PluginLifecycle) {
        if matches!(
            event,
            PluginLifecycle::Cancelled | PluginLifecycle::Disabled | PluginLifecycle::Uninstalled
        ) {
            self.drag = None;
        }
    }
    fn on_event(&mut self, event: &CanvasEvent, cx: &PluginContext<'_>, out: &mut PluginOutput) {
        if !cx.allows(Capability::Edit) {
            return;
        }
        match event {
            CanvasEvent::PointerDown(p)
                if p.button == Some(PointerButton::Left)
                    && !p.modifiers.space
                    && !p.modifiers.shift =>
            {
                let selected = cx.editor.selection.expanded_nodes(cx.editor.graph());
                let eligible = match &p.hit {
                    HitTarget::Node(id) => selected.contains(id),
                    HitTarget::Group(id) => cx.editor.selection.groups.contains(id),
                    _ => false,
                };
                if !eligible {
                    return;
                }
                if selected.iter().any(|id| {
                    cx.editor
                        .graph()
                        .nodes
                        .get(id)
                        .is_some_and(|node| node.locked)
                }) || cx.editor.selection.groups.iter().any(|id| {
                    cx.editor
                        .graph()
                        .groups
                        .get(id)
                        .is_some_and(|group| group.locked)
                }) {
                    return;
                }
                let nodes = selected
                    .iter()
                    .filter_map(|id| {
                        cx.editor
                            .graph()
                            .nodes
                            .get(id)
                            .map(|node| (id.clone(), node.position))
                    })
                    .collect();
                let groups = cx
                    .editor
                    .selection
                    .groups
                    .iter()
                    .filter_map(|id| {
                        cx.editor
                            .graph()
                            .groups
                            .get(id)
                            .map(|group| (id.clone(), group.bounds.origin))
                    })
                    .collect();
                self.drag = Some(Drag {
                    screen_start: p.position,
                    world_start: cx.editor.viewport.screen_to_world(p.position),
                    nodes,
                    groups,
                    started: false,
                });
                out.command(CanvasCommand::CapturePointer {
                    pointer_id: p.pointer_id,
                });
                out.effect(CanvasEffect::Cursor(CanvasCursor::Grab));
            }
            CanvasEvent::PointerMove(p) => {
                if let Some(drag) = &mut self.drag {
                    if !drag.started
                        && p.position.distance(drag.screen_start) < self.threshold_pixels.max(0.0)
                    {
                        return;
                    }
                    if !drag.started {
                        out.command(CanvasCommand::BeginGesture {
                            label: "Drag selection".into(),
                        });
                        drag.started = true;
                    }
                    let delta = cx.editor.viewport.screen_to_world(p.position) - drag.world_start;
                    out.command(CanvasCommand::SetPositions {
                        nodes: drag
                            .nodes
                            .iter()
                            .map(|(id, position)| (id.clone(), *position + delta))
                            .collect(),
                        groups: drag
                            .groups
                            .iter()
                            .map(|(id, position)| (id.clone(), *position + delta))
                            .collect(),
                    });
                    out.effect(CanvasEffect::Cursor(CanvasCursor::Grabbing));
                }
            }
            CanvasEvent::PointerUp(_) => {
                if let Some(drag) = self.drag.take() {
                    if drag.started {
                        out.command(CanvasCommand::CommitGesture);
                    }
                    out.command(CanvasCommand::ReleasePointer);
                    out.effect(CanvasEffect::Guides(vec![]));
                    out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
                }
            }
            _ => {}
        }
    }
}

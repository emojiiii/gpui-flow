use crate::scene::{SceneCommand, object_editable, selection_bounds};
use crate::*;
use gpui_flow_core::Point;
use gpui_flow_editor::SceneEditor;
use gpui_flow_scene::DisplayId;
struct Drag {
    screen_start: Point,
    world_start: Point,
    anchor: Point,
    started: bool,
}
pub struct SceneDragPlugin {
    pub threshold_pixels: f32,
    drag: Option<Drag>,
}
impl Default for SceneDragPlugin {
    fn default() -> Self {
        Self {
            threshold_pixels: 3.0,
            drag: None,
        }
    }
}
impl CanvasPlugin<SceneEditor, SceneCommand> for SceneDragPlugin {
    fn id(&self) -> &str {
        "drag"
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
            self.drag = None;
        }
    }
    fn on_event(
        &mut self,
        event: &CanvasEvent,
        cx: &PluginContext<'_, SceneEditor>,
        out: &mut PluginOutput<SceneCommand>,
    ) {
        if !cx.allows(Capability::Edit) {
            return;
        }
        match event {
            CanvasEvent::PointerDown(p)
                if p.button == Some(PointerButton::Left)
                    && !p.modifiers.space
                    && !p.modifiers.shift =>
            {
                let HitTarget::Object(id) = &p.hit else {
                    return;
                };
                let id = DisplayId::from(id.clone());
                let selected =
                    cx.editor.scene().ancestor_path(&id).is_ok_and(|path| {
                        path.iter().any(|id| cx.editor.selection.ids.contains(id))
                    });
                if !selected
                    || cx
                        .editor
                        .selection
                        .roots(cx.editor.scene())
                        .iter()
                        .any(|id| !object_editable(cx.editor.scene(), id))
                {
                    return;
                }
                let Ok(Some(bounds)) = selection_bounds(cx.editor) else {
                    return;
                };
                self.drag = Some(Drag {
                    screen_start: p.position,
                    world_start: cx.editor.viewport.screen_to_world(p.position),
                    anchor: bounds.origin,
                    started: false,
                });
                out.command(SceneCommand::CapturePointerForSelection {
                    pointer_id: p.pointer_id,
                    targets: cx
                        .editor
                        .selection
                        .roots(cx.editor.scene())
                        .iter()
                        .filter_map(|id| cx.editor.engine.handle(id))
                        .collect(),
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
                        out.command(SceneCommand::BeginGesture);
                        drag.started = true;
                    }
                    out.command(SceneCommand::MoveSelectionTo {
                        anchor: drag.anchor + cx.editor.viewport.screen_to_world(p.position)
                            - drag.world_start,
                    });
                    out.effect(CanvasEffect::Cursor(CanvasCursor::Grabbing));
                }
            }
            CanvasEvent::PointerUp(_) => {
                if let Some(drag) = self.drag.take() {
                    if drag.started {
                        out.command(SceneCommand::CommitGesture);
                    }
                    out.command(SceneCommand::ReleasePointer);
                    out.effect(CanvasEffect::Guides(vec![]));
                    out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
                }
            }
            _ => {}
        }
    }
}

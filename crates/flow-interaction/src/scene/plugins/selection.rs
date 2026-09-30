use crate::scene::SceneCommand;
use crate::*;
use gpui_flow_core::{Point, Rect};
use gpui_flow_editor::SceneEditor;
use gpui_flow_scene::DisplayId;
#[derive(Default)]
pub struct SceneSelectionPlugin {
    marquee: Option<(Point, bool)>,
}
impl CanvasPlugin<SceneEditor, SceneCommand> for SceneSelectionPlugin {
    fn id(&self) -> &str {
        "selection"
    }
    fn priority(&self) -> i32 {
        -30
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
    fn on_event(
        &mut self,
        event: &CanvasEvent,
        cx: &PluginContext<'_, SceneEditor>,
        out: &mut PluginOutput<SceneCommand>,
    ) {
        if !cx.allows(Capability::Selection) {
            return;
        }
        match event {
            CanvasEvent::PointerDown(p)
                if p.button == Some(PointerButton::Left) && !p.modifiers.space =>
            {
                match &p.hit {
                    HitTarget::Object(id) | HitTarget::ObjectResize(id) => {
                        let mut id = DisplayId::from(id.clone());
                        if p.modifiers.control
                            && let Some(parent) =
                                cx.editor.scene().get(&id).and_then(|n| n.parent.clone())
                        {
                            id = parent;
                        }
                        let mode = if p.modifiers.shift {
                            SelectionMode::Toggle
                        } else {
                            SelectionMode::Replace
                        };
                        if p.modifiers.shift || !cx.editor.selection.ids.contains(&id) {
                            out.command(SceneCommand::Select { id, mode });
                        }
                    }
                    HitTarget::Background if p.modifiers.shift => {
                        let start = cx.editor.viewport.screen_to_world(p.position);
                        self.marquee = Some((start, true));
                        out.command(SceneCommand::CapturePointer {
                            pointer_id: p.pointer_id,
                        });
                        out.effect(CanvasEffect::Marquee(Some(Rect::from_points(start, start))));
                    }
                    HitTarget::Background => out.command(SceneCommand::ClearSelection),
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
                    out.command(SceneCommand::SelectRect {
                        rect: Rect::from_points(
                            start,
                            cx.editor.viewport.screen_to_world(p.position),
                        ),
                        additive,
                    });
                    out.command(SceneCommand::ReleasePointer);
                    out.effect(CanvasEffect::Marquee(None));
                }
            }
            _ => {}
        }
    }
}

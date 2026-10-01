use crate::scene::{LineEndpoint, SceneCommand, SceneLineGeometry};
use crate::*;
use gpui_flow_core::Point;
use gpui_flow_editor::SceneEditor;
use gpui_flow_scene::DisplayId;

struct Drag {
    geometry: SceneLineGeometry,
    endpoint: LineEndpoint,
    start: Point,
}
/// Target-aware outer editing of a single built-in Line's true endpoints.
#[derive(Default)]
pub struct SceneLineEndpointPlugin {
    drag: Option<Drag>,
}
impl CanvasPlugin<SceneEditor, SceneCommand> for SceneLineEndpointPlugin {
    fn id(&self) -> &str {
        "line-endpoints"
    }
    fn priority(&self) -> i32 {
        12
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
                    && !cx.editor.editing() =>
            {
                let HitTarget::LineEndpoint { id, endpoint } = &p.hit else {
                    return;
                };
                let Some(geometry) = SceneLineGeometry::for_selection(cx.editor) else {
                    return;
                };
                if cx.editor.engine.handle(&DisplayId::from(id.clone())) != Some(geometry.target) {
                    return;
                }
                let start = cx.editor.viewport.screen_to_world(p.position);
                if !start.is_finite() {
                    return;
                }
                out.command(SceneCommand::BeginGesture);
                out.command(SceneCommand::CapturePointerForSelection {
                    pointer_id: p.pointer_id,
                    targets: vec![geometry.target],
                });
                self.drag = Some(Drag {
                    geometry,
                    endpoint: *endpoint,
                    start,
                });
                out.effect(CanvasEffect::Cursor(CanvasCursor::Crosshair));
            }
            CanvasEvent::PointerMove(p) => {
                if let Some(drag) = &self.drag {
                    let current = cx.editor.viewport.screen_to_world(p.position);
                    if !current.is_finite() {
                        out.command(SceneCommand::CancelGesture);
                        return;
                    }
                    if let Some(position) = drag.geometry.endpoint_position(
                        drag.endpoint,
                        drag.start,
                        current,
                        p.modifiers.shift,
                    ) {
                        out.command(SceneCommand::SetLineEndpoint {
                            target: drag.geometry.target,
                            endpoint: drag.endpoint,
                            position,
                        });
                    }
                } else if cx.capture_owner.is_none()
                    && !matches!(
                        p.hit,
                        HitTarget::ObjectResize(_)
                            | HitTarget::ObjectResizeHandle { .. }
                            | HitTarget::SelectionResizeHandle { .. }
                            | HitTarget::SelectionRotateHandle
                    )
                {
                    let cursor = if matches!(p.hit, HitTarget::LineEndpoint { .. })
                        && SceneLineGeometry::for_selection(cx.editor).is_some()
                    {
                        CanvasCursor::Crosshair
                    } else {
                        CanvasCursor::Default
                    };
                    out.effect(CanvasEffect::Cursor(cursor));
                }
            }
            CanvasEvent::PointerUp(_) if self.drag.take().is_some() => {
                out.command(SceneCommand::CommitGesture);
                out.command(SceneCommand::ReleasePointer);
                out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
            }
            CanvasEvent::PointerLeave if self.drag.is_none() && cx.capture_owner.is_none() => {
                out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
            }
            _ => {}
        }
    }
}

use crate::{
    scene::{
        GEOMETRY_TOOLS_PLUGIN, GeometryControl, GeometryToolMode, SceneCommand, SceneGeometryTools,
    },
    *,
};
use gpui_flow_editor::SceneEditor;
use gpui_flow_scene::{DisplayId, Point};

struct Drag {
    geometry: SceneGeometryTools,
    control: GeometryControl,
    start: Point,
}
/// One opt-in controller for all three bounded Flow geometry extensions.
pub struct SceneGeometryToolsPlugin {
    mode: GeometryToolMode,
    drag: Option<Drag>,
}
impl SceneGeometryToolsPlugin {
    pub fn new(mode: GeometryToolMode) -> Self {
        Self { mode, drag: None }
    }
}
impl CanvasPlugin<SceneEditor, SceneCommand> for SceneGeometryToolsPlugin {
    fn id(&self) -> &str {
        GEOMETRY_TOOLS_PLUGIN
    }
    fn priority(&self) -> i32 {
        13
    }
    fn is_default_action(&self) -> bool {
        true
    }
    fn geometry_tool_mode(&self) -> Option<GeometryToolMode> {
        Some(self.mode)
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
                let HitTarget::GeometryControl { id, control } = &p.hit else {
                    return;
                };
                if control.mode() != self.mode {
                    return;
                }
                let Some(geometry) =
                    SceneGeometryTools::for_selection(cx.editor, self.mode, cx.transforms)
                else {
                    return;
                };
                if cx.editor.engine.handle(&DisplayId::from(id.clone())) != Some(geometry.target) {
                    return;
                }
                let start = cx.editor.viewport.screen_to_world(p.position);
                // Reject stale or invented control IDs before reserving history/capture.
                if geometry.drag(*control, start, start, false).is_none() {
                    return;
                }
                out.command(SceneCommand::BeginGesture);
                out.command(SceneCommand::CapturePointerForSelection {
                    pointer_id: p.pointer_id,
                    targets: vec![geometry.target],
                });
                self.drag = Some(Drag {
                    geometry,
                    control: *control,
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
                    if let Some(edit) =
                        drag.geometry
                            .drag(drag.control, drag.start, current, p.modifiers.shift)
                    {
                        out.command(SceneCommand::SetGeometry {
                            target: drag.geometry.target,
                            edit,
                        });
                    }
                } else if cx.capture_owner.is_none()
                    && matches!(&p.hit, HitTarget::GeometryControl { control, .. } if control.mode() == self.mode)
                {
                    out.effect(CanvasEffect::Cursor(CanvasCursor::Crosshair));
                }
            }
            CanvasEvent::PointerUp(_) if self.drag.take().is_some() => {
                out.command(SceneCommand::CommitGesture);
                out.command(SceneCommand::ReleasePointer);
                out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
            }
            _ => {}
        }
    }
}

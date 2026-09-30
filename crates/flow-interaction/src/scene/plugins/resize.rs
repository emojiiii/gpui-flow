use crate::scene::{SceneCommand, object_editable};
use crate::*;
use gpui_flow_core::{Point, Rect};
use gpui_flow_editor::{SceneEditor, resize_transform};
use gpui_flow_scene::{Affine, DisplayId, Shape};
struct Resize {
    id: DisplayId,
    inverse: Affine,
    original: Affine,
    bounds: Rect,
    start_local: Point,
    uniform: bool,
}
#[derive(Default)]
pub struct SceneResizePlugin {
    resize: Option<Resize>,
}
impl CanvasPlugin<SceneEditor, SceneCommand> for SceneResizePlugin {
    fn id(&self) -> &str {
        "resize"
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
            self.resize = None;
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
            CanvasEvent::PointerDown(p) if p.button == Some(PointerButton::Left) => {
                let HitTarget::ObjectResize(id) = &p.hit else {
                    return;
                };
                let id = DisplayId::from(id.clone());
                if !object_editable(cx.editor.scene(), &id) {
                    return;
                }
                let Some(node) = cx.editor.scene().get(&id) else {
                    return;
                };
                let Ok(Some(bounds)) = cx.editor.scene().local_bounds(&id) else {
                    return;
                };
                let Some(inverse) = cx
                    .editor
                    .scene()
                    .world_transform(&id)
                    .ok()
                    .and_then(Affine::inverse)
                else {
                    return;
                };
                self.resize = Some(Resize {
                    id,
                    inverse,
                    original: node.object.transform,
                    bounds,
                    start_local: inverse
                        .transform_point(cx.editor.viewport.screen_to_world(p.position)),
                    uniform: matches!(node.object.shape, Shape::Text(_) | Shape::Image(_)),
                });
                out.command(SceneCommand::BeginGesture);
                out.command(SceneCommand::CapturePointer {
                    pointer_id: p.pointer_id,
                });
                out.effect(CanvasEffect::Cursor(CanvasCursor::Resize));
            }
            CanvasEvent::PointerMove(p) => {
                if let Some(resize) = &self.resize {
                    let mut transform = resize_transform(
                        resize.original,
                        resize.bounds,
                        resize.start_local,
                        resize
                            .inverse
                            .transform_point(cx.editor.viewport.screen_to_world(p.position)),
                    );
                    if (resize.uniform || p.modifiers.shift)
                        && let Some(inverse) = resize.original.inverse()
                    {
                        let relative = inverse * transform;
                        let factor = relative.a.max(relative.d);
                        transform = resize.original
                            * Affine::scale(factor, factor).around(resize.bounds.origin);
                    }
                    out.command(SceneCommand::SetTransform {
                        id: resize.id.clone(),
                        transform,
                    });
                }
            }
            CanvasEvent::PointerUp(_) if self.resize.take().is_some() => {
                out.command(SceneCommand::CommitGesture);
                out.command(SceneCommand::ReleasePointer);
                out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
            }
            _ => {}
        }
    }
}

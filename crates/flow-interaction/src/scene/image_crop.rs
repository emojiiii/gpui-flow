//! Opt-in source-frame handles reuse the ordinary host capture and edit history.
use super::{SceneCommand, object_editable};
use crate::*;
use gpui_flow_editor::{ImageCropGeometry, ImageCropPart, ResizeHandle, SceneEditor};
use gpui_flow_engine::{ElementHandle, InstanceState};
use gpui_flow_scene::{Affine, DisplayId, Point, Shape};

pub const IMAGE_CROP_PLUGIN: &str = "image-crop";
pub const IMAGE_CROP_HANDLE_RADIUS: f32 = 9.;
#[derive(Debug, Clone)]
pub struct SceneImageCropGeometry {
    pub target: ElementHandle,
    pub geometry: ImageCropGeometry,
    pub source_crop: Option<gpui_flow_scene::Bounds>,
    pub world: Affine,
    pub inverse: Affine,
}
impl SceneImageCropGeometry {
    pub fn for_selection(editor: &SceneEditor) -> Option<Self> {
        if editor.selection.ids.len() != 1 {
            return None;
        }
        let id = editor.selection.ids.iter().next()?;
        if !object_editable(editor.scene(), id) {
            return None;
        }
        let target = editor.engine.handle(id)?;
        if editor.engine.instance(target).ok()?.state != InstanceState::Attached {
            return None;
        }
        let node = editor.scene().get(id)?;
        let Shape::Image(image) = &node.object.shape else {
            return None;
        };
        if !node.children.is_empty() {
            return None;
        }
        let world = editor.scene().world_transform(id).ok()?;
        Some(Self {
            target,
            geometry: ImageCropGeometry::new(image).ok()?,
            source_crop: image.source_crop,
            world,
            inverse: world.inverse()?,
        })
    }
    pub fn screen_handles(&self, editor: &SceneEditor) -> Option<[(ResizeHandle, Point); 8]> {
        if !editor.viewport.is_valid() {
            return None;
        }
        let handles = ResizeHandle::ALL.map(|handle| {
            (
                handle,
                editor.viewport.world_to_screen(
                    self.world
                        .transform_point(handle.position(self.geometry.frame())),
                ),
            )
        });
        handles
            .iter()
            .all(|(_, p)| p.is_finite())
            .then_some(handles)
    }
    pub fn hit(&self, editor: &SceneEditor, screen: Point) -> Option<ImageCropPart> {
        let handles = self.screen_handles(editor)?;
        // Closest handle wins on small frames; ties retain clockwise order.
        if let Some((handle, _)) = handles
            .iter()
            .filter_map(|(h, p)| {
                let distance = (p.x - screen.x).hypot(p.y - screen.y);
                (distance <= IMAGE_CROP_HANDLE_RADIUS).then_some((*h, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
        {
            return Some(ImageCropPart::Handle(handle));
        }
        let local = self
            .inverse
            .transform_point(editor.viewport.screen_to_world(screen));
        self.geometry
            .frame()
            .contains(local)
            .then_some(ImageCropPart::Move)
    }
}
struct Drag {
    geometry: SceneImageCropGeometry,
    part: ImageCropPart,
    start: Point,
}
#[derive(Default)]
pub struct SceneImageCropPlugin {
    drag: Option<Drag>,
}
impl CanvasPlugin<SceneEditor, SceneCommand> for SceneImageCropPlugin {
    fn id(&self) -> &str {
        IMAGE_CROP_PLUGIN
    }
    fn priority(&self) -> i32 {
        5
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
                let HitTarget::ImageCrop { id, part } = &p.hit else {
                    return;
                };
                let Some(geometry) = SceneImageCropGeometry::for_selection(cx.editor) else {
                    return;
                };
                if cx.editor.engine.handle(&DisplayId::from(id.clone())) != Some(geometry.target) {
                    return;
                }
                let start = geometry
                    .inverse
                    .transform_point(cx.editor.viewport.screen_to_world(p.position));
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
                    part: *part,
                    start,
                });
                out.effect(CanvasEffect::Cursor(CanvasCursor::Crosshair));
            }
            CanvasEvent::PointerMove(p) => {
                if let Some(drag) = &self.drag {
                    let current = drag
                        .geometry
                        .inverse
                        .transform_point(cx.editor.viewport.screen_to_world(p.position));
                    if !current.is_finite() {
                        out.command(SceneCommand::CancelGesture);
                        return;
                    }
                    if let Some(crop) = drag.geometry.geometry.drag(
                        drag.part,
                        drag.start,
                        current,
                        p.modifiers.shift,
                    ) {
                        out.command(SceneCommand::SetImageCrop {
                            target: drag.geometry.target,
                            crop: if crop == drag.geometry.geometry.crop {
                                drag.geometry.source_crop
                            } else {
                                Some(crop)
                            },
                        });
                    }
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

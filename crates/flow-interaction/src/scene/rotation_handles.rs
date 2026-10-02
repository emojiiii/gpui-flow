//! Center-pivot pointer rotation reusing the existing frozen selection roots.
use super::{SceneResizeGeometry, SceneSelectionResizeGeometry};
use gpui_flow_core::Point;
use gpui_flow_editor::{ResizeHandle, SceneEditor};
use gpui_flow_scene::{DisplayId, Scene, Shape};

pub const ROTATION_HANDLE_OFFSET: f32 = 28.;
pub const ROTATION_HANDLE_RADIUS: f32 = 5.;
pub const ROTATION_HANDLE_HIT_RADIUS: f32 = 10.;

#[derive(Debug, Clone)]
pub struct SceneRotationGeometry {
    pub selection: SceneSelectionResizeGeometry,
    pub pivot: Point,
    /// World-space top midpoint, before the constant screen-space stem offset.
    pub stem: Point,
}
#[derive(Debug, Clone, Copy)]
pub struct SceneRotationHandle {
    pub position: Point,
    pub stem: Point,
}
impl SceneRotationGeometry {
    pub fn for_selection(editor: &SceneEditor) -> Option<Self> {
        let selection = SceneSelectionResizeGeometry::for_nonempty_selection(editor)?;
        for id in editor.selection.roots(editor.scene()) {
            if !Self::accepts_subtree(editor.scene(), &id) {
                return None;
            }
        }
        let (pivot, stem) = if editor.selection.ids.len() == 1 {
            let id = editor.selection.ids.iter().next()?;
            let geometry = SceneResizeGeometry::for_target(editor, id)?;
            (
                geometry.world.transform_point(geometry.bounds.center()),
                geometry
                    .world
                    .transform_point(ResizeHandle::Top.position(geometry.bounds)),
            )
        } else {
            let bounds = selection.geometry.bounds;
            (bounds.center(), ResizeHandle::Top.position(bounds))
        };
        (pivot.is_finite() && stem.is_finite()).then_some(Self {
            selection,
            pivot,
            stem,
        })
    }
    /// Text and custom producers still have no arbitrary-transform contract.
    /// Images use exact geometry-driven CPU domains when native painting cannot
    /// represent the pose. Reject the complete unsupported subtree.
    pub(crate) fn accepts_subtree(scene: &Scene, id: &DisplayId) -> bool {
        if !scene
            .local_bounds(id)
            .ok()
            .flatten()
            .is_some_and(|bounds| bounds.is_valid() && bounds.size.is_positive())
        {
            return false;
        }
        let mut pending = vec![id];
        while let Some(id) = pending.pop() {
            let Some(node) = scene.get(id) else {
                return false;
            };
            if matches!(node.object.shape, Shape::Text(_) | Shape::Custom(_)) {
                return false;
            }
            pending.extend(&node.children);
        }
        true
    }
    pub fn screen_handle(&self, editor: &SceneEditor) -> Option<SceneRotationHandle> {
        if !editor.viewport.is_valid() {
            return None;
        }
        let stem = editor.viewport.world_to_screen(self.stem);
        let pivot = editor.viewport.world_to_screen(self.pivot);
        let dx = stem.x as f64 - pivot.x as f64;
        let dy = stem.y as f64 - pivot.y as f64;
        let length = dx.hypot(dy);
        if !length.is_finite() || length == 0. {
            return None;
        }
        let position = Point::new(
            (stem.x as f64 + dx / length * ROTATION_HANDLE_OFFSET as f64) as f32,
            (stem.y as f64 + dy / length * ROTATION_HANDLE_OFFSET as f64) as f32,
        );
        (stem.is_finite() && position.is_finite()).then_some(SceneRotationHandle { position, stem })
    }
}
pub fn hit_rotation_handle(handle: SceneRotationHandle, position: Point) -> bool {
    position.is_finite()
        && (position.x as f64 - handle.position.x as f64)
            .hypot(position.y as f64 - handle.position.y as f64)
            <= ROTATION_HANDLE_HIT_RADIUS as f64
}

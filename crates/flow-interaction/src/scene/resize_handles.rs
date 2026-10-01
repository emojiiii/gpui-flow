//! Shared bounded outer-handle geometry for native paint, picking and input.
use super::object_editable;
use crate::CanvasCursor;
use gpui_flow_core::{Point, Rect};
use gpui_flow_editor::{ResizeHandle, SceneEditor};
use gpui_flow_engine::{ElementHandle, InstanceState};
use gpui_flow_scene::{Affine, DisplayId, Scene, Shape};

pub const RESIZE_HANDLE_HIT_RADIUS: f32 = 10.;
pub const RESIZE_HANDLE_PAINT_SIZE: f32 = 8.;

#[derive(Debug, Clone, Copy)]
pub struct SceneResizeGeometry {
    pub bounds: Rect,
    pub original: Affine,
    pub world: Affine,
    pub inverse: Affine,
    pub minimum_scale: Point,
    pub uniform: bool,
}
impl SceneResizeGeometry {
    /// One live target with positive-area paint bounds. Native descendants
    /// require a currently supported positive uniform world transform and
    /// force uniform resizing of the complete subtree. Custom renderers have
    /// no transform contract here, so Custom subtrees are explicitly excluded.
    pub fn for_target(editor: &SceneEditor, id: &DisplayId) -> Option<Self> {
        if !object_editable(editor.scene(), id) {
            return None;
        }
        let handle = editor.engine.handle(id)?;
        if editor.engine.instance(handle).ok()?.state != InstanceState::Attached {
            return None;
        }
        let node = editor.scene().get(id)?;
        let bounds = editor.engine.local_bounds(id).ok()??;
        if !bounds.is_valid() || !bounds.size.is_positive() {
            return None;
        }
        let world = editor.engine.world_transform(id).ok()?;
        let inverse = world.inverse()?;
        if !Self::accepts_transform(editor, id, node.object.transform) {
            return None;
        }
        let mut pending = vec![id];
        let mut uniform = false;
        while let Some(next) = pending.pop() {
            let node = editor.scene().get(next)?;
            match node.object.shape {
                Shape::Custom(_) => return None,
                Shape::Text(_) | Shape::Image(_) => {
                    let matrix = editor.engine.world_transform(next).ok()?;
                    if !native_transform_supported(matrix) {
                        return None;
                    }
                    uniform = true;
                }
                _ => {}
            }
            pending.extend(&node.children);
        }
        // Lengths of transformed local axes, never the rotation-dependent AABB
        // or screen pixels. Tiny existing objects can stay tiny or grow, but
        // cannot shrink further; pressing a handle alone changes nothing.
        let x = (world.a as f64).hypot(world.b as f64) * bounds.size.width as f64;
        let y = (world.c as f64).hypot(world.d as f64) * bounds.size.height as f64;
        if !x.is_finite() || !y.is_finite() || x <= 0. || y <= 0. {
            return None;
        }
        let floor = |length: f64| (1. / length.max(1.)).clamp(0.05, 1.) as f32;
        Some(Self {
            bounds,
            original: node.object.transform,
            world,
            inverse,
            minimum_scale: Point::new(floor(x), floor(y)),
            uniform,
        })
    }
    pub fn screen_handles(self, editor: &SceneEditor) -> Option<[SceneResizeHandle; 8]> {
        if !editor.viewport.is_valid() {
            return None;
        }
        let handles = ResizeHandle::ALL.map(|handle| {
            let position = handle.position(self.bounds);
            let direction = self
                .world
                .transform_vector(position - handle.anchor(self.bounds));
            SceneResizeHandle {
                handle,
                position: editor
                    .viewport
                    .world_to_screen(self.world.transform_point(position)),
                cursor: resize_cursor(direction),
            }
        });
        handles
            .iter()
            .all(|h| h.position.is_finite())
            .then_some(handles)
    }
    /// Check the actual proposed matrices as well, including floating-point
    /// composition through ancestors. Never turn a working native descendant
    /// into an unsupported rotated/skewed/nonuniform presentation.
    pub fn accepts_transform(editor: &SceneEditor, id: &DisplayId, transform: Affine) -> bool {
        Self::validate_subtree(editor.scene(), id, transform, false)
    }
    pub(crate) fn accepts_scene_transform(
        scene: &Scene,
        id: &DisplayId,
        transform: Affine,
    ) -> bool {
        Self::validate_subtree(scene, id, transform, true)
    }
    fn validate_subtree(scene: &Scene, id: &DisplayId, transform: Affine, selection: bool) -> bool {
        let Some(node) = scene.get(id) else {
            return false;
        };
        let parent = match &node.parent {
            Some(parent) => match scene.world_transform(parent) {
                Ok(matrix) => matrix,
                Err(_) => return false,
            },
            None => Affine::IDENTITY,
        };
        let mut pending = vec![(id, parent * transform)];
        while let Some((next, world)) = pending.pop() {
            let Some(node) = scene.get(next) else {
                return false;
            };
            if (selection && (node.object.locked || matches!(node.object.shape, Shape::Custom(_))))
                || !world.is_finite()
                || world.inverse().is_none()
                || (matches!(node.object.shape, Shape::Text(_) | Shape::Image(_))
                    && !native_transform_supported(world))
            {
                return false;
            }
            for child in &node.children {
                let Some(node) = scene.get(child) else {
                    return false;
                };
                pending.push((child, world * node.object.transform));
            }
        }
        true
    }
}
fn native_transform_supported(m: Affine) -> bool {
    m.is_finite() && m.b == 0. && m.c == 0. && m.a > 0. && m.a == m.d
}

#[derive(Debug, Clone, Copy)]
pub struct SceneResizeHandle {
    pub handle: ResizeHandle,
    pub position: Point,
    pub cursor: CanvasCursor,
}
/// Nearest visible center wins when small-object hit circles overlap. Equal
/// distances keep the documented clockwise handle order.
pub fn hit_resize_handle(
    handles: &[SceneResizeHandle],
    position: Point,
) -> Option<SceneResizeHandle> {
    if !position.is_finite() {
        return None;
    }
    handles
        .iter()
        .copied()
        .filter_map(|handle| {
            let dx = handle.position.x as f64 - position.x as f64;
            let dy = handle.position.y as f64 - position.y as f64;
            let distance = dx * dx + dy * dy;
            (distance <= (RESIZE_HANDLE_HIT_RADIUS as f64).powi(2)).then_some((handle, distance))
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(handle, _)| handle)
}
fn resize_cursor(direction: Point) -> CanvasCursor {
    let octant = ((direction.y as f64).atan2(direction.x as f64) / std::f64::consts::FRAC_PI_4)
        .round() as i32;
    match octant.rem_euclid(4) {
        0 => CanvasCursor::ResizeHorizontal,
        1 => CanvasCursor::Resize,
        2 => CanvasCursor::ResizeVertical,
        _ => CanvasCursor::ResizeOtherDiagonal,
    }
}

/// Frozen root matrices for one shared world-axis resize. The bounds union uses
/// cached geometric subtree bounds (stroke and internal clipping included),
/// never visual effect/shadow bounds, and never an arbitrary primary element.
#[derive(Debug, Clone)]
pub struct SceneSelectionResizeGeometry {
    pub geometry: SceneResizeGeometry,
    pub roots: Vec<SceneResizeRoot>,
}
#[derive(Debug, Clone, Copy)]
pub struct SceneResizeRoot {
    pub target: ElementHandle,
    pub original: Affine,
    pub parent: Affine,
    pub parent_inverse: Affine,
}
impl SceneSelectionResizeGeometry {
    pub fn for_selection(editor: &SceneEditor) -> Option<Self> {
        if editor.selection.ids.len() < 2 {
            return None;
        }
        Self::for_nonempty_selection(editor)
    }
    /// Shared frozen-root machinery also serves single-object pointer rotation.
    pub(crate) fn for_nonempty_selection(editor: &SceneEditor) -> Option<Self> {
        if editor.selection.ids.is_empty()
            || editor
                .selection
                .ids
                .iter()
                .any(|id| editor.engine.handle(id).is_none())
        {
            return None;
        }
        let mut bounds: Option<Rect> = None;
        let mut uniform = false;
        let mut roots = Vec::new();
        for id in editor.selection.roots(editor.scene()) {
            let geometry = SceneResizeGeometry::for_target(editor, &id)?;
            if !SceneResizeGeometry::accepts_scene_transform(editor.scene(), &id, geometry.original)
            {
                return None;
            }
            let world_bounds = editor.engine.world_bounds(&id).ok()??;
            if !world_bounds.is_valid() || !world_bounds.size.is_positive() {
                return None;
            }
            bounds = Some(bounds.map_or(world_bounds, |old| old.union(world_bounds)));
            uniform |= geometry.uniform;
            let node = editor.scene().get(&id)?;
            let parent = match &node.parent {
                Some(id) => editor.engine.world_transform(id).ok()?,
                None => Affine::IDENTITY,
            };
            roots.push(SceneResizeRoot {
                target: editor.engine.handle(&id)?,
                original: geometry.original,
                parent,
                parent_inverse: parent.inverse()?,
            });
        }
        let bounds = bounds?;
        if !bounds.is_valid() || !bounds.size.is_positive() {
            return None;
        }
        let floor = |length: f32| (1. / (length as f64).max(1.)).clamp(0.05, 1.) as f32;
        Some(Self {
            geometry: SceneResizeGeometry {
                bounds,
                original: Affine::IDENTITY,
                world: Affine::IDENTITY,
                inverse: Affine::IDENTITY,
                minimum_scale: Point::new(floor(bounds.size.width), floor(bounds.size.height)),
                uniform,
            },
            roots,
        })
    }
    pub fn screen_handles(&self, editor: &SceneEditor) -> Option<[SceneResizeHandle; 8]> {
        self.geometry.screen_handles(editor)
    }
    /// Apply one world delta to every frozen root. Identity bypass avoids
    /// rounding drift (and Redo invalidation) in P^-1 * P at zero motion.
    pub fn transforms(&self, delta: Affine) -> Option<Vec<(ElementHandle, Affine)>> {
        self.roots
            .iter()
            .map(|root| {
                let transform = if delta == Affine::IDENTITY {
                    root.original
                } else {
                    root.parent_inverse * delta * root.parent * root.original
                };
                (transform.is_finite() && transform.inverse().is_some())
                    .then_some((root.target, transform))
            })
            .collect()
    }
}

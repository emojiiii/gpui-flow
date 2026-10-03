//! Two true endpoints of a built-in straight Line, not Path control points.
use super::object_editable;
use gpui_flow_core::Point;
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ElementHandle, InstanceState};
use gpui_flow_scene::{Affine, DisplayId, Scene, Shape, display::Line};

pub const LINE_HANDLE_PAINT_SIZE: f32 = 8.;
pub const LINE_HANDLE_HIT_RADIUS: f32 = 10.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEndpoint {
    Start,
    End,
}
impl LineEndpoint {
    pub fn position(self, line: &Line) -> Point {
        match self {
            Self::Start => line.start,
            Self::End => line.end,
        }
    }
    pub fn opposite(self) -> Self {
        match self {
            Self::Start => Self::End,
            Self::End => Self::Start,
        }
    }
    pub(crate) fn set(self, line: &mut Line, position: Point) {
        match self {
            Self::Start => line.start = position,
            Self::End => line.end = position,
        }
    }
}
#[derive(Debug, Clone)]
pub struct SceneLineGeometry {
    pub target: ElementHandle,
    pub line: Line,
    pub world: Affine,
    pub inverse: Affine,
    /// One world unit, or the initial length for an existing shorter line.
    pub minimum_length: f64,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneLineHandle {
    pub endpoint: LineEndpoint,
    pub position: Point,
}
impl SceneLineGeometry {
    pub fn for_selection(editor: &SceneEditor) -> Option<Self> {
        if editor.selection.ids.len() != 1 {
            return None;
        }
        Self::for_target(editor, editor.selection.ids.iter().next()?)
    }
    pub fn for_selection_with_geometry(editor: &SceneEditor, geometry: &Scene) -> Option<Self> {
        if editor.selection.ids.len() != 1 {
            return None;
        }
        Self::for_target_with_geometry(editor, editor.selection.ids.iter().next()?, geometry)
    }
    pub fn for_target(editor: &SceneEditor, id: &DisplayId) -> Option<Self> {
        editor.scene().validate_view(editor.viewport.zoom).ok()?;
        Self::for_target_with_geometry(editor, id, editor.scene())
    }
    fn for_target_with_geometry(
        editor: &SceneEditor,
        id: &DisplayId,
        geometry: &Scene,
    ) -> Option<Self> {
        let target = editor.engine.handle(id)?;
        if editor.engine.instance(target).ok()?.state != InstanceState::Attached {
            return None;
        }
        let (line, world) = Self::scene_line(geometry, id)?;
        let minimum_length = Self::world_length(&line, world)?.min(1.);
        Some(Self {
            target,
            line,
            world,
            inverse: world.inverse()?,
            minimum_length,
        })
    }
    pub(crate) fn scene_line(scene: &Scene, id: &DisplayId) -> Option<(Line, Affine)> {
        if !object_editable(scene, id) {
            return None;
        }
        let node = scene.get(id)?;
        let Shape::Line(line) = &node.object.shape else {
            return None;
        };
        let world = scene.world_transform(id).ok()?;
        if !node.children.is_empty()
            || world.inverse().is_none()
            || !world.transform_point(line.start).is_finite()
            || !world.transform_point(line.end).is_finite()
        {
            return None;
        }
        Self::world_length(line, world)?;
        Some((line.clone(), world))
    }
    /// Endpoint distance through the complete linear world transform. Wide
    /// differences avoid local subtraction overflow; translation/AABBs and
    /// viewport zoom do not affect the minimum-length contract.
    pub(crate) fn world_length(line: &Line, world: Affine) -> Option<f64> {
        if !line.start.is_finite() || !line.end.is_finite() || !world.is_finite() {
            return None;
        }
        let x = line.end.x as f64 - line.start.x as f64;
        let y = line.end.y as f64 - line.start.y as f64;
        let length = (world.a as f64 * x + world.c as f64 * y)
            .hypot(world.b as f64 * x + world.d as f64 * y);
        (length.is_finite() && length > 0.).then_some(length)
    }
    pub fn screen_handles(&self, editor: &SceneEditor) -> Option<[SceneLineHandle; 2]> {
        if !editor.viewport.is_valid() {
            return None;
        }
        let handles = [LineEndpoint::Start, LineEndpoint::End].map(|endpoint| SceneLineHandle {
            endpoint,
            position: editor
                .viewport
                .world_to_screen(self.world.transform_point(endpoint.position(&self.line))),
        });
        handles
            .iter()
            .all(|h| h.position.is_finite())
            .then_some(handles)
    }
    /// Frozen-world pointer delta, rather than absolute pointer position, keeps
    /// off-center presses stable. Shift constrains local movement, not line angle.
    /// Too-short samples are rejected and leave the last accepted geometry alone.
    pub fn endpoint_position(
        &self,
        endpoint: LineEndpoint,
        start_world: Point,
        current_world: Point,
        constrain: bool,
    ) -> Option<Point> {
        if !start_world.is_finite() || !current_world.is_finite() {
            return None;
        }
        let original = endpoint.position(&self.line);
        if start_world == current_world {
            return Some(original);
        }
        let dx = current_world.x as f64 - start_world.x as f64;
        let dy = current_world.y as f64 - start_world.y as f64;
        let mut x = self.inverse.a as f64 * dx + self.inverse.c as f64 * dy;
        let mut y = self.inverse.b as f64 * dx + self.inverse.d as f64 * dy;
        if constrain {
            if x.abs() > y.abs() {
                y = 0.;
            } else {
                x = 0.;
            }
        }
        let position = Point::new(
            (original.x as f64 + x) as f32,
            (original.y as f64 + y) as f32,
        );
        let mut candidate = self.line.clone();
        endpoint.set(&mut candidate, position);
        (Self::world_length(&candidate, self.world)? >= self.minimum_length).then_some(position)
    }
}
/// Nearest center wins; an exact tie selects Start, even at tiny zoom levels.
pub fn hit_line_handle(handles: &[SceneLineHandle], position: Point) -> Option<SceneLineHandle> {
    if !position.is_finite() {
        return None;
    }
    handles
        .iter()
        .copied()
        .filter_map(|handle| {
            let distance = (handle.position.x as f64 - position.x as f64)
                .hypot(handle.position.y as f64 - position.y as f64);
            (distance <= LINE_HANDLE_HIT_RADIUS as f64).then_some((handle, distance))
        })
        .min_by(|(a, da), (b, db)| {
            da.total_cmp(db).then_with(|| {
                let rank = |endpoint| {
                    if endpoint == LineEndpoint::Start {
                        0
                    } else {
                        1
                    }
                };
                rank(a.endpoint).cmp(&rank(b.endpoint))
            })
        })
        .map(|(handle, _)| handle)
}

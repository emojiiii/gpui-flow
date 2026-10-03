//! Flow extension: bounded Path/Pen point, local shear and editor-pivot tools.
use super::{SceneResizeGeometry, SceneRotationGeometry, object_editable};
use crate::TransformCapabilities;
use gpui_flow_editor::{
    PathControlGeometry, SceneEditor, bounded_point, drag_local_point, skew_levers,
};
pub use gpui_flow_editor::{PathPointId, PathPointRole, SkewAxis};
use gpui_flow_engine::{ElementHandle, InstanceState, SceneEngine};
use gpui_flow_scene::{Affine, Point};

pub const GEOMETRY_TOOLS_PLUGIN: &str = "geometry-tools";
pub const GEOMETRY_HANDLE_SIZE: f32 = 8.;
pub const GEOMETRY_HANDLE_HIT_RADIUS: f32 = 10.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryToolMode {
    Path,
    Skew,
    Pivot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryControl {
    PathPoint(PathPointId),
    Skew(SkewAxis),
    Pivot,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GeometryEdit {
    PathPoint {
        point: PathPointId,
        position: Point,
    },
    /// Cumulative local shear from this gesture's frozen transform (maximum 16).
    Skew {
        axis: SkewAxis,
        shear: f32,
    },
    Pivot {
        position: Option<Point>,
    },
}
impl GeometryEdit {
    pub fn control(self) -> GeometryControl {
        match self {
            Self::PathPoint { point, .. } => GeometryControl::PathPoint(point),
            Self::Skew { axis, .. } => GeometryControl::Skew(axis),
            Self::Pivot { .. } => GeometryControl::Pivot,
        }
    }
    pub fn mode(self) -> GeometryToolMode {
        self.control().mode()
    }
}
impl GeometryControl {
    pub fn mode(self) -> GeometryToolMode {
        match self {
            Self::PathPoint(_) => GeometryToolMode::Path,
            Self::Skew(_) => GeometryToolMode::Skew,
            Self::Pivot => GeometryToolMode::Pivot,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryHandle {
    pub control: GeometryControl,
    pub position: Point,
}

#[derive(Clone, Debug)]
pub struct SceneGeometryTools {
    pub target: ElementHandle,
    pub mode: GeometryToolMode,
    pub world: Affine,
    pub original: Affine,
    pub pivot: Point,
    pub original_pivot: Option<Point>,
    inverse: Affine,
    path: Option<PathControlGeometry>,
    levers: Option<[Point; 2]>,
}
impl SceneGeometryTools {
    pub fn for_selection(
        editor: &SceneEditor,
        mode: GeometryToolMode,
        capabilities: TransformCapabilities,
    ) -> Option<Self> {
        let resolved;
        let geometry = if editor.scene().has_scale_fixed_strokes() {
            resolved = editor.engine.resolved_for_view(editor.viewport.zoom).ok()?;
            &resolved
        } else {
            &editor.engine
        };
        Self::for_selection_with_geometry(editor, mode, capabilities, geometry)
    }
    pub fn for_selection_with_geometry(
        editor: &SceneEditor,
        mode: GeometryToolMode,
        capabilities: TransformCapabilities,
        geometry: &SceneEngine,
    ) -> Option<Self> {
        if editor.selection.ids.len() != 1 || !editor.viewport.is_valid() {
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
        let node = geometry.scene().get(id)?;
        let world = geometry.world_transform(id).ok()?;
        let inverse = world.inverse()?;
        let bounds = geometry.local_bounds(id).ok()??;
        let pivot = node.object.pivot.unwrap_or(bounds.center());
        if !bounded_point(pivot) || !world.transform_point(pivot).is_finite() {
            return None;
        }
        let path = if mode == GeometryToolMode::Path {
            if !node.children.is_empty() {
                return None;
            }
            Some(PathControlGeometry::new(&node.object.shape, world).ok()?)
        } else {
            None
        };
        let levers = if mode == GeometryToolMode::Skew {
            SceneResizeGeometry::for_target_with_geometry(editor, id, capabilities, geometry)?;
            if !SceneRotationGeometry::accepts_subtree(editor.scene(), id, capabilities) {
                return None;
            }
            Some(skew_levers(bounds)?)
        } else {
            None
        };
        Some(Self {
            target,
            mode,
            world,
            original: node.object.transform,
            pivot,
            original_pivot: node.object.pivot,
            inverse,
            path,
            levers,
        })
    }
    pub fn screen_handles(&self, editor: &SceneEditor) -> Option<Vec<GeometryHandle>> {
        if !editor.viewport.is_valid() {
            return None;
        }
        let local: Vec<_> = match self.mode {
            GeometryToolMode::Path => self.path.as_ref()?.points().iter()
                .map(|p| (GeometryControl::PathPoint(p.id), p.local)).collect(),
            GeometryToolMode::Pivot => vec![(GeometryControl::Pivot, self.pivot)],
            GeometryToolMode::Skew => self.levers?.into_iter().zip([SkewAxis::X, SkewAxis::Y])
                .filter(|(lever, axis)| match axis { SkewAxis::X => (lever.y - self.pivot.y).abs(), SkewAxis::Y => (lever.x - self.pivot.x).abs() } >= 0.0001)
                .map(|(p, axis)| (GeometryControl::Skew(axis), p)).collect(),
        };
        let handles: Vec<_> = local
            .into_iter()
            .map(|(control, local)| GeometryHandle {
                control,
                position: editor
                    .viewport
                    .world_to_screen(self.world.transform_point(local)),
            })
            .collect();
        handles
            .iter()
            .all(|h| h.position.is_finite())
            .then_some(handles)
    }
    pub fn screen_segments(&self, editor: &SceneEditor) -> Vec<(Point, Point)> {
        self.path.as_ref().map_or_else(Vec::new, |path| {
            path.control_segments()
                .into_iter()
                .map(|(a, b)| {
                    (
                        editor
                            .viewport
                            .world_to_screen(self.world.transform_point(a)),
                        editor
                            .viewport
                            .world_to_screen(self.world.transform_point(b)),
                    )
                })
                .collect()
        })
    }
    pub fn drag(
        &self,
        control: GeometryControl,
        start: Point,
        current: Point,
        constrain: bool,
    ) -> Option<GeometryEdit> {
        if control.mode() != self.mode {
            return None;
        }
        match control {
            GeometryControl::PathPoint(point) => Some(GeometryEdit::PathPoint {
                point,
                position: drag_local_point(
                    self.path.as_ref()?.point(point)?,
                    self.inverse,
                    start,
                    current,
                    constrain,
                )
                .ok()?,
            }),
            GeometryControl::Pivot => {
                let position =
                    drag_local_point(self.pivot, self.inverse, start, current, constrain).ok()?;
                Some(GeometryEdit::Pivot {
                    position: if position == self.pivot {
                        self.original_pivot
                    } else {
                        Some(position)
                    },
                })
            }
            GeometryControl::Skew(axis) => {
                let delta =
                    drag_local_point(Point::ZERO, self.inverse, start, current, false).ok()?;
                let levers = self.levers?;
                let (amount, distance) = match axis {
                    SkewAxis::X => (delta.x, levers[0].y - self.pivot.y),
                    SkewAxis::Y => (delta.y, levers[1].x - self.pivot.x),
                };
                if distance.abs() < 0.0001 {
                    return None;
                }
                let shear = amount / distance;
                gpui_flow_editor::apply_skew(self.original, self.pivot, shear, axis)?;
                Some(GeometryEdit::Skew { axis, shear })
            }
        }
    }
}
/// Stable nearest-handle selection, including overlapping controls at low zoom.
pub fn hit_geometry_handle(handles: &[GeometryHandle], position: Point) -> Option<GeometryHandle> {
    if !position.is_finite() {
        return None;
    }
    handles
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, handle)| {
            let distance = (handle.position.x as f64 - position.x as f64)
                .hypot(handle.position.y as f64 - position.y as f64);
            (distance <= GEOMETRY_HANDLE_HIT_RADIUS as f64).then_some((index, handle, distance))
        })
        .min_by(|(a, _, da), (b, _, db)| da.total_cmp(db).then(a.cmp(b)))
        .map(|(_, h, _)| h)
}

impl super::ScenePluginHost {
    pub fn geometry_tool_mode(&self) -> Option<GeometryToolMode> {
        self.enabled_geometry_tool_mode()
    }
    pub fn geometry_tools(
        &self,
        editor: &SceneEditor,
        geometry: &SceneEngine,
    ) -> Option<SceneGeometryTools> {
        SceneGeometryTools::for_selection_with_geometry(
            editor,
            self.geometry_tool_mode()?,
            self.transform_capabilities(),
            geometry,
        )
    }
}

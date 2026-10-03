//! Direct editing of stored Path/Pen points, with no flattening or inferred smoothing.
use gpui_flow_scene::{
    Affine, Bounds, Point, Shape,
    display::{Path, PathCommand},
};
pub const MAX_PATH_EDIT_COMMANDS: usize = 512;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PathPointRole {
    Endpoint,
    Control1,
    Control2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PathPointId {
    pub command: usize,
    pub role: PathPointRole,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathControlPoint {
    pub id: PathPointId,
    pub local: Point,
}
#[derive(Clone, Debug)]
pub struct PathControlGeometry {
    shape: Shape,
    pub world: Affine,
    inverse: Affine,
    points: Vec<PathControlPoint>,
}
fn path(shape: &Shape) -> Option<&Path> {
    match shape {
        Shape::Path(p) => Some(p),
        Shape::Pen(p) => Some(&p.path),
        _ => None,
    }
}
fn coordinate(command: &mut PathCommand, role: PathPointRole) -> Option<&mut Point> {
    match (command, role) {
        (
            PathCommand::MoveTo { to }
            | PathCommand::LineTo { to }
            | PathCommand::QuadraticTo { to, .. }
            | PathCommand::CubicTo { to, .. },
            PathPointRole::Endpoint,
        ) => Some(to),
        (PathCommand::QuadraticTo { control, .. }, PathPointRole::Control1) => Some(control),
        (PathCommand::CubicTo { control1, .. }, PathPointRole::Control1) => Some(control1),
        (PathCommand::CubicTo { control2, .. }, PathPointRole::Control2) => Some(control2),
        _ => None,
    }
}
impl PathControlGeometry {
    pub fn new(shape: &Shape, world: Affine) -> Result<Self, String> {
        let p = path(shape).ok_or("point editing requires Path or leaf Pen")?;
        if p.commands.len() > MAX_PATH_EDIT_COMMANDS {
            return Err("path editing command limit exceeded".into());
        }
        p.validate()?;
        let inverse = world.inverse().ok_or("path transform is singular")?;
        let mut points = Vec::new();
        for (index, command) in p.commands.iter().enumerate() {
            for role in [
                PathPointRole::Endpoint,
                PathPointRole::Control1,
                PathPointRole::Control2,
            ] {
                if let Some(position) = coordinate(&mut command.clone(), role).copied() {
                    if !bounded_point(position) || !world.transform_point(position).is_finite() {
                        return Err("path handle has nonfinite world coordinates".into());
                    }
                    points.push(PathControlPoint {
                        id: PathPointId {
                            command: index,
                            role,
                        },
                        local: position,
                    });
                }
            }
        }
        Ok(Self {
            shape: shape.clone(),
            world,
            inverse,
            points,
        })
    }
    pub fn points(&self) -> &[PathControlPoint] {
        &self.points
    }
    pub fn point(&self, id: PathPointId) -> Option<Point> {
        self.points
            .iter()
            .find(|point| point.id == id)
            .map(|p| p.local)
    }
    /// Patch one stored coordinate, preserving Path/Pen identity and every other field.
    pub fn set_point(shape: &mut Shape, id: PathPointId, position: Point) -> Result<(), String> {
        if !bounded_point(position) {
            return Err("path point exceeds the finite coordinate budget".into());
        }
        let p = match shape {
            Shape::Path(p) => p,
            Shape::Pen(p) => &mut p.path,
            _ => return Err("point editing requires Path or leaf Pen".into()),
        };
        if p.commands.len() > MAX_PATH_EDIT_COMMANDS {
            return Err("path editing command limit exceeded".into());
        }
        let slot = coordinate(
            p.commands.get_mut(id.command).ok_or("stale path point")?,
            id.role,
        )
        .ok_or("stale path point role")?;
        let previous = *slot;
        *slot = position;
        if let Err(error) = p.validate() {
            // This public helper also accepts untrusted, not-yet-admitted
            // Shapes. Preserve them exactly when validation fails.
            *coordinate(&mut p.commands[id.command], id.role).expect("validated point role") =
                previous;
            return Err(error);
        }
        Ok(())
    }
    /// The actual Bezier control polygon, used only for the editing overlay.
    pub fn control_segments(&self) -> Vec<(Point, Point)> {
        let mut segments = Vec::new();
        let mut previous = Point::ZERO;
        let mut start = Point::ZERO;
        for command in &path(&self.shape).expect("admitted path").commands {
            match *command {
                PathCommand::MoveTo { to } => {
                    start = to;
                    previous = to;
                }
                PathCommand::LineTo { to } => previous = to,
                PathCommand::QuadraticTo { control, to } => {
                    segments.extend([(previous, control), (control, to)]);
                    previous = to;
                }
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    segments.extend([(previous, control1), (control2, to)]);
                    previous = to;
                }
                PathCommand::Close => previous = start,
            }
        }
        segments
    }
    pub fn drag(
        &self,
        id: PathPointId,
        start_world: Point,
        current_world: Point,
        constrain: bool,
    ) -> Result<Shape, String> {
        let point = self
            .points
            .iter()
            .find(|p| p.id == id)
            .ok_or("unknown path point")?;
        let position = drag_local_point(
            point.local,
            self.inverse,
            start_world,
            current_world,
            constrain,
        )?;
        let mut shape = self.shape.clone();
        Self::set_point(&mut shape, id, position)?;
        Ok(shape)
    }
}
pub fn bounded_point(point: Point) -> bool {
    point.is_finite() && point.x.abs() <= 1_000_000. && point.y.abs() <= 1_000_000.
}
/// Frozen inverse-linear deltas preserve off-center presses and avoid subtracting
/// separately transformed large absolute positions. Returning to start is exact.
pub fn drag_local_point(
    original: Point,
    inverse: Affine,
    start: Point,
    current: Point,
    constrain: bool,
) -> Result<Point, String> {
    if !bounded_point(original)
        || !inverse.is_finite()
        || !start.is_finite()
        || !current.is_finite()
    {
        return Err("nonfinite pointer".into());
    }
    if start == current {
        return Ok(original);
    }
    let dx = current.x as f64 - start.x as f64;
    let dy = current.y as f64 - start.y as f64;
    let mut x = inverse.a as f64 * dx + inverse.c as f64 * dy;
    let mut y = inverse.b as f64 * dx + inverse.d as f64 * dy;
    if constrain {
        if x.abs() > y.abs() {
            y = 0.;
        } else {
            x = 0.;
        }
    }
    let point = Point::new(
        (original.x as f64 + x) as f32,
        (original.y as f64 + y) as f32,
    );
    if !bounded_point(point) {
        return Err("local point budget exceeded".into());
    }
    Ok(point)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkewAxis {
    X,
    Y,
}
/// One bounded local shear about an explicit pivot; no implicit angle spelling.
pub fn skew_transform(
    original: Affine,
    pivot: Point,
    lever: Point,
    delta: Point,
    axis: SkewAxis,
) -> Option<Affine> {
    original.inverse()?;
    if !pivot.is_finite() || !lever.is_finite() || !delta.is_finite() {
        return None;
    }
    let (amount, denominator) = match axis {
        SkewAxis::X => (delta.x as f64, lever.y as f64 - pivot.y as f64),
        SkewAxis::Y => (delta.y as f64, lever.x as f64 - pivot.x as f64),
    };
    if denominator.abs() < 0.0001 {
        return None;
    }
    let shear = amount / denominator;
    apply_skew(original, pivot, shear as f32, axis)
}
pub fn apply_skew(original: Affine, pivot: Point, shear: f32, axis: SkewAxis) -> Option<Affine> {
    original.inverse()?;
    if !bounded_point(pivot) || !shear.is_finite() || shear.abs() > 16. {
        return None;
    }
    if shear == 0. {
        return Some(original);
    }
    let local = match axis {
        SkewAxis::X => Affine {
            c: shear,
            ..Affine::IDENTITY
        },
        SkewAxis::Y => Affine {
            b: shear,
            ..Affine::IDENTITY
        },
    };
    let result = original * local.around(pivot);
    result.inverse()?;
    Some(result)
}
pub fn skew_levers(bounds: Bounds) -> Option<[Point; 2]> {
    if !bounds.is_valid() || !bounds.size.is_positive() {
        return None;
    }
    Some([
        Point::new(
            bounds.center().x,
            bounds.min_y() - bounds.size.height * 0.25,
        ),
        Point::new(bounds.max_x() + bounds.size.width * 0.25, bounds.center().y),
    ])
}

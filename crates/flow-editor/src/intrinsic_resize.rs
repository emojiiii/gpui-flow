//! Bounded built-in sizing policies. Document styling and runtime pixels are
//! deliberately outside these geometry-only patches.
use crate::{ResizeOptions, resize_transform_with_options};
use gpui_flow_scene::{
    Affine, Bounds, DisplayId, DisplayObject, Point, Scene, Shape, Size, display::PathCommand,
};

pub const MAX_INTRINSIC_RESIZE_DIMENSION: f32 = 1_000_000.;
pub const MAX_INTRINSIC_RESIZE_POINTS: usize = 65_536;
pub const MAX_INTRINSIC_RESIZE_CHILDREN: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinResizePolicy {
    /// Own nominal width/height; corner radii, paint and child geometry stay fixed.
    Size,
    /// Rewrite coordinates/control points, retaining the shape and curve commands.
    Coordinates,
    /// A Star retains its regular radial definition and therefore scales uniformly.
    Radial,
    /// A Group has no size; scale its direct children's transforms in group space.
    Children,
}

#[derive(Debug, Clone)]
pub struct BuiltinResizeGeometry {
    shape: Shape,
    bounds: Bounds,
    children: Vec<(DisplayId, Affine)>,
    policy: BuiltinResizePolicy,
}

/// A typed patch: applying it copies only the fields owned by the kind's policy.
/// Constructors validate the finite geometry and budgets before producing it.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinResize {
    shape: Shape,
    pub transform: Affine,
    children: Vec<(DisplayId, Affine)>,
}

impl BuiltinResizeGeometry {
    /// Text uses the separate Box/FontSize policies; Custom uses its registered
    /// protocol. Neither can silently enter this built-in dispatcher.
    pub fn new(scene: &Scene, id: &DisplayId) -> Result<Self, String> {
        let node = scene.get(id).ok_or("missing intrinsic resize target")?;
        let shape = &node.object.shape;
        validate_budget(shape)?;
        let policy = match shape {
            Shape::Rect(_)
            | Shape::Ellipse(_)
            | Shape::Image(_)
            | Shape::Canvas(_)
            | Shape::Box(_)
            | Shape::Frame(_) => BuiltinResizePolicy::Size,
            Shape::Line(_) | Shape::Polygon(_) | Shape::Path(_) | Shape::Pen(_) => {
                BuiltinResizePolicy::Coordinates
            }
            Shape::Star(_) => BuiltinResizePolicy::Radial,
            Shape::Group(_) => BuiltinResizePolicy::Children,
            Shape::Text(_) | Shape::Custom(_) => {
                return Err("separate Text/Custom resize policy required".into());
            }
        };
        let bounds = match shape {
            // Partial ellipses edit the complete nominal sizing box, not the arc's AABB.
            Shape::Ellipse(s) => Some(s.bounds()),
            Shape::Group(_) => scene.local_bounds(id).map_err(|e| e.to_string())?,
            _ => shape.local_bounds(),
        }
        .ok_or("intrinsic resize requires geometry")?;
        validate_bounds(bounds)?;
        let children = if policy == BuiltinResizePolicy::Children {
            if node.children.len() > MAX_INTRINSIC_RESIZE_CHILDREN {
                return Err("intrinsic resize child budget exceeded".into());
            }
            node.children
                .iter()
                .map(|id| {
                    let child = scene.get(id).ok_or("missing intrinsic resize child")?;
                    Ok((id.clone(), child.object.transform))
                })
                .collect::<Result<_, String>>()?
        } else {
            Vec::new()
        };
        Ok(Self {
            shape: shape.clone(),
            bounds,
            children,
            policy,
        })
    }
    pub fn bounds(&self) -> Bounds {
        self.bounds
    }
    pub fn policy(&self) -> BuiltinResizePolicy {
        self.policy
    }

    /// Uses frozen local pointer coordinates. The opposite sizing-box anchor
    /// stays fixed, including nonzero origins and rotated/reflected ancestors.
    pub fn resize(
        &self,
        original: Affine,
        start: Point,
        current: Point,
        mut options: ResizeOptions,
    ) -> Option<BuiltinResize> {
        original.inverse()?;
        options.uniform |= self.policy == BuiltinResizePolicy::Radial;
        let delta =
            resize_transform_with_options(Affine::IDENTITY, self.bounds, start, current, options)?;
        let mut shape = self.shape.clone();
        let mut children = self.children.clone();
        let transform = if self.policy == BuiltinResizePolicy::Children {
            validate_bounds(delta.transform_bounds(self.bounds)).ok()?;
            // A structural Group delegates its scale to content, retaining its own pose.
            for (_, child) in &mut children {
                *child = delta * *child;
                child.inverse()?;
            }
            original
        } else {
            let origin = self.bounds.origin;
            let content = Affine::scale(delta.a, delta.d).around(origin);
            let size = Size::new(
                self.bounds.size.width * delta.a,
                self.bounds.size.height * delta.d,
            );
            match &mut shape {
                Shape::Rect(s) => s.size = size,
                Shape::Ellipse(s) => s.size = size,
                Shape::Image(s) => s.size = size,
                Shape::Canvas(s) => s.size = size,
                Shape::Box(s) => s.size = size,
                Shape::Frame(s) => s.size = size,
                Shape::Line(s) => {
                    s.start = content.transform_point(s.start);
                    s.end = content.transform_point(s.end);
                }
                Shape::Polygon(s) => {
                    for point in &mut s.points {
                        *point = content.transform_point(*point);
                    }
                }
                Shape::Path(s) => transform_path(&mut s.commands, content),
                Shape::Pen(s) => transform_path(&mut s.path.commands, content),
                Shape::Star(s) => {
                    s.center = content.transform_point(s.center);
                    s.outer_radius *= delta.a;
                    s.inner_radius *= delta.a;
                }
                _ => return None,
            }
            if delta == Affine::IDENTITY {
                original
            } else {
                let moved = delta.transform_point(origin);
                original * Affine::translation(moved.x - origin.x, moved.y - origin.y)
            }
        };
        validate_budget(&shape).ok()?;
        if !matches!(shape, Shape::Group(_)) {
            validate_bounds(shape.local_bounds()?).ok()?;
        }
        transform.inverse()?;
        Some(BuiltinResize {
            shape,
            transform,
            children,
        })
    }
}

impl BuiltinResize {
    pub fn child_transforms(&self) -> &[(DisplayId, Affine)] {
        &self.children
    }
    /// Preserve current non-geometry fields, including image source/crop/fit,
    /// Canvas DPR/safe-resize, polygon closure, and Star count/angle.
    pub fn apply_to(&self, object: &DisplayObject) -> Result<DisplayObject, String> {
        let mut object = object.clone();
        match (&mut object.shape, &self.shape) {
            (Shape::Rect(a), Shape::Rect(b)) => a.size = b.size,
            (Shape::Ellipse(a), Shape::Ellipse(b)) => a.size = b.size,
            (Shape::Image(a), Shape::Image(b)) => a.size = b.size,
            (Shape::Canvas(a), Shape::Canvas(b)) => a.size = b.size,
            (Shape::Box(a), Shape::Box(b)) => a.size = b.size,
            (Shape::Frame(a), Shape::Frame(b)) => a.size = b.size,
            (Shape::Line(a), Shape::Line(b)) => {
                a.start = b.start;
                a.end = b.end;
            }
            (Shape::Polygon(a), Shape::Polygon(b)) => a.points.clone_from(&b.points),
            (Shape::Path(a), Shape::Path(b)) => a.commands.clone_from(&b.commands),
            (Shape::Pen(a), Shape::Pen(b)) => a.path.commands.clone_from(&b.path.commands),
            (Shape::Star(a), Shape::Star(b)) => {
                a.center = b.center;
                a.outer_radius = b.outer_radius;
                a.inner_radius = b.inner_radius;
            }
            (Shape::Group(_), Shape::Group(_)) => {}
            _ => return Err("intrinsic resize kind changed".into()),
        }
        object.transform = self.transform;
        validate_budget(&object.shape)?;
        Ok(object)
    }
}

fn validate_bounds(bounds: Bounds) -> Result<(), String> {
    if !bounds.is_valid()
        || !bounds.size.is_positive()
        || [
            bounds.origin.x,
            bounds.origin.y,
            bounds.max_x(),
            bounds.max_y(),
            bounds.size.width,
            bounds.size.height,
        ]
        .iter()
        .any(|v| v.abs() > MAX_INTRINSIC_RESIZE_DIMENSION)
    {
        return Err("intrinsic resize requires bounded positive-area geometry".into());
    }
    Ok(())
}
fn validate_budget(shape: &Shape) -> Result<(), String> {
    let point = |p: Point| {
        p.is_finite()
            && p.x.abs() <= MAX_INTRINSIC_RESIZE_DIMENSION
            && p.y.abs() <= MAX_INTRINSIC_RESIZE_DIMENSION
    };
    let path = match shape {
        Shape::Path(s) => Some(s),
        Shape::Pen(s) => Some(&s.path),
        _ => None,
    };
    if let Some(path) = path {
        if path.commands.len() > MAX_INTRINSIC_RESIZE_POINTS / 3 {
            return Err("intrinsic resize path budget exceeded".into());
        }
        for command in &path.commands {
            let valid = match *command {
                PathCommand::MoveTo { to } | PathCommand::LineTo { to } => point(to),
                PathCommand::QuadraticTo { control, to } => point(control) && point(to),
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => point(control1) && point(control2) && point(to),
                PathCommand::Close => true,
            };
            if !valid {
                return Err("intrinsic resize coordinate budget exceeded".into());
            }
        }
    }
    if let Shape::Polygon(s) = shape
        && (s.points.len() > MAX_INTRINSIC_RESIZE_POINTS || !s.points.iter().all(|p| point(*p)))
    {
        return Err("intrinsic resize polygon budget exceeded".into());
    }
    shape.validate()
}
fn transform_path(commands: &mut [PathCommand], transform: Affine) {
    for command in commands {
        match command {
            PathCommand::MoveTo { to } | PathCommand::LineTo { to } => {
                *to = transform.transform_point(*to)
            }
            PathCommand::QuadraticTo { control, to } => {
                *control = transform.transform_point(*control);
                *to = transform.transform_point(*to);
            }
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                *control1 = transform.transform_point(*control1);
                *control2 = transform.transform_point(*control2);
                *to = transform.transform_point(*to);
            }
            PathCommand::Close => {}
        }
    }
}

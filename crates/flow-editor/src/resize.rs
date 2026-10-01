use gpui_flow_core::{Point, Rect};
use gpui_flow_scene::Affine;

/// Clockwise positions on the initial local paint bounds. These are transform
/// handles, not edits to a shape's width/height or path control points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeHandle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}
impl ResizeHandle {
    pub const ALL: [Self; 8] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Right,
        Self::BottomRight,
        Self::Bottom,
        Self::BottomLeft,
        Self::Left,
    ];
    fn axes(self) -> (f64, f64) {
        match self {
            Self::TopLeft => (-1., -1.),
            Self::Top => (0., -1.),
            Self::TopRight => (1., -1.),
            Self::Right => (1., 0.),
            Self::BottomRight => (1., 1.),
            Self::Bottom => (0., 1.),
            Self::BottomLeft => (-1., 1.),
            Self::Left => (-1., 0.),
        }
    }
    pub fn position(self, bounds: Rect) -> Point {
        let (x, y) = self.axes();
        Point::new(
            (bounds.min_x() as f64 + (x + 1.) * 0.5 * bounds.size.width as f64) as f32,
            (bounds.min_y() as f64 + (y + 1.) * 0.5 * bounds.size.height as f64) as f32,
        )
    }
    pub fn anchor(self, bounds: Rect) -> Point {
        let (x, y) = self.axes();
        Point::new(
            (bounds.min_x() as f64 + (1. - x) * 0.5 * bounds.size.width as f64) as f32,
            (bounds.min_y() as f64 + (1. - y) * 0.5 * bounds.size.height as f64) as f32,
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ResizeOptions {
    pub handle: ResizeHandle,
    pub uniform: bool,
    /// Positive lower factors, at most 1. A host can derive these from the
    /// initial local-axis world lengths without introducing zoom/AABB drift.
    pub minimum_scale: Point,
}
impl Default for ResizeOptions {
    fn default() -> Self {
        Self {
            handle: ResizeHandle::BottomRight,
            uniform: false,
            minimum_scale: Point::new(0.05, 0.05),
        }
    }
}

/// Resize one initial transform around the opposite corner/edge midpoint.
/// Uniform corners project signed local motion onto the initial diagonal;
/// uniform sides use their active axis. Positive factors prevent new flips.
/// Degenerate, non-finite or overflowing geometry is rejected atomically.
pub fn resize_transform_with_options(
    original: Affine,
    bounds: Rect,
    start: Point,
    current: Point,
    options: ResizeOptions,
) -> Option<Affine> {
    if !bounds.is_valid()
        || !bounds.size.is_positive()
        || original.inverse().is_none()
        || !start.is_finite()
        || !current.is_finite()
        || !options.minimum_scale.is_finite()
        || !(0.05..=1.).contains(&options.minimum_scale.x)
        || !(0.05..=1.).contains(&options.minimum_scale.y)
    {
        return None;
    }
    if start == current {
        return Some(original);
    }
    let dx = current.x as f64 - start.x as f64;
    let dy = current.y as f64 - start.y as f64;
    let (x, y) = options.handle.axes();
    let width = bounds.size.width as f64;
    let height = bounds.size.height as f64;
    let mut sx = 1. + x * dx / width;
    let mut sy = 1. + y * dy / height;
    if options.uniform {
        let factor = if x == 0. {
            sy
        } else if y == 0. {
            sx
        } else {
            1. + (dx * x * width + dy * y * height) / (width * width + height * height)
        };
        sx = factor.clamp(
            options.minimum_scale.x.max(options.minimum_scale.y) as f64,
            100.,
        );
        sy = sx;
    } else {
        sx = sx.clamp(options.minimum_scale.x as f64, 100.);
        sy = sy.clamp(options.minimum_scale.y as f64, 100.);
    }
    if sx == 1. && sy == 1. {
        return Some(original);
    }
    let transform =
        original * Affine::scale(sx as f32, sy as f32).around(options.handle.anchor(bounds));
    (transform.is_finite() && transform.inverse().is_some()).then_some(transform)
}
/// Resize from a pointer delta in the object's initial local coordinate space.
/// Using the full paint bounds and the actual initial pointer avoids a jump for
/// negative-origin paths, stars, strokes, and off-center handle presses.
pub fn resize_transform(original: Affine, bounds: Rect, start: Point, current: Point) -> Affine {
    let delta = current - start;
    let x = if bounds.size.width > 0. {
        (1. + delta.x / bounds.size.width).clamp(0.05, 100.)
    } else {
        1.
    };
    let y = if bounds.size.height > 0. {
        (1. + delta.y / bounds.size.height).clamp(0.05, 100.)
    } else {
        1.
    };
    original * Affine::scale(x, y).around(bounds.origin)
}
#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_core::Size;
    #[test]
    fn no_pointer_motion_does_not_jump() {
        for bounds in [
            Rect::new(Point::new(-50., -50.), Size::new(100., 100.)),
            Rect::new(Point::new(-3., -3.), Size::new(106., 86.)),
        ] {
            let original = Affine::translation(30., 80.) * Affine::rotation(15.);
            let start = Point::new(bounds.max_x() + 2., bounds.max_y() - 1.);
            assert_eq!(resize_transform(original, bounds, start, start), original);
        }
    }
    #[test]
    fn offset_origin_is_fixed() {
        let b = Rect::new(Point::new(-50., -50.), Size::new(100., 100.));
        let m = resize_transform(
            Affine::IDENTITY,
            b,
            Point::new(50., 50.),
            Point::new(150., 150.),
        );
        assert_eq!(m.transform_point(b.origin), b.origin);
        assert_eq!(
            m.transform_point(Point::new(50., 50.)),
            Point::new(150., 150.)
        );
    }
}

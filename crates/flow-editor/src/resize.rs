use gpui_flow_core::{Point, Rect};
use gpui_flow_scene::Affine;
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

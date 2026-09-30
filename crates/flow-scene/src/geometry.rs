use crate::{Bounds, FillRule, Point, Size};
use serde::{Deserialize, Serialize};

/// One ordered polyline or closed contour in local coordinates.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contour {
    pub points: Vec<Point>,
    pub closed: bool,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlattenOptions {
    pub tolerance: f32,
    pub max_depth: u8,
}
impl Default for FlattenOptions {
    fn default() -> Self {
        Self {
            tolerance: 0.25,
            max_depth: 12,
        }
    }
}
impl FlattenOptions {
    pub(crate) fn normalized(self) -> Self {
        Self {
            tolerance: if self.tolerance.is_finite() && self.tolerance > 0.0 {
                self.tolerance.max(0.0001)
            } else {
                0.25
            },
            max_depth: self.max_depth.min(16),
        }
    }
}
pub(crate) fn point_bounds(points: impl IntoIterator<Item = Point>) -> Option<Bounds> {
    let mut points = points.into_iter();
    let first = points.next()?;
    let invalid = || {
        Bounds::new(
            Point::new(f32::NAN, f32::NAN),
            Size::new(f32::NAN, f32::NAN),
        )
    };
    if !first.is_finite() {
        return Some(invalid());
    }
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (first.x, first.y, first.x, first.y);
    for p in points {
        if !p.is_finite() {
            return Some(invalid());
        }
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    Some(Bounds::new(
        Point::new(min_x, min_y),
        Size::new(max_x - min_x, max_y - min_y),
    ))
}
pub(crate) fn intersection(a: Bounds, b: Bounds) -> Option<Bounds> {
    let (x, y) = (a.min_x().max(b.min_x()), a.min_y().max(b.min_y()));
    let (r, bottom) = (a.max_x().min(b.max_x()), a.max_y().min(b.max_y()));
    (r >= x && bottom >= y).then(|| Bounds::new(Point::new(x, y), Size::new(r - x, bottom - y)))
}
pub(crate) fn distance_to_segment(p: Point, a: Point, b: Point) -> f32 {
    let (dx, dy) = (b.x as f64 - a.x as f64, b.y as f64 - a.y as f64);
    let l = dx * dx + dy * dy;
    let t = if l == 0.0 {
        0.0
    } else {
        (((p.x as f64 - a.x as f64) * dx + (p.y as f64 - a.y as f64) * dy) / l).clamp(0.0, 1.0)
    };
    ((p.x as f64 - a.x as f64 - t * dx).hypot(p.y as f64 - a.y as f64 - t * dy)) as f32
}
pub(crate) fn stroke_hit(contours: &[Contour], p: Point, radius: f32) -> bool {
    if radius < 0.0 || !radius.is_finite() {
        return false;
    }
    contours.iter().any(|c| {
        if c.points.len() == 1 {
            return c.points[0].distance(p) <= radius;
        }
        c.points
            .windows(2)
            .any(|w| distance_to_segment(p, w[0], w[1]) <= radius)
            || (c.closed
                && c.points.len() > 1
                && distance_to_segment(p, *c.points.last().unwrap(), c.points[0]) <= radius)
    })
}
pub(crate) fn fill_hit(contours: &[Contour], p: Point, rule: FillRule) -> bool {
    let mut winding = 0i64;
    let mut parity = false;
    for contour in contours {
        if contour.points.len() < 3 {
            continue;
        }
        // Fill implicitly closes an open path, matching vector drawing semantics.
        for i in 0..contour.points.len() {
            let a = contour.points[i];
            let b = contour.points[(i + 1) % contour.points.len()];
            if distance_to_segment(p, a, b) <= 1e-5 {
                return true;
            }
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x as f64
                    + (p.y as f64 - a.y as f64) * (b.x as f64 - a.x as f64)
                        / (b.y as f64 - a.y as f64);
                if x > p.x as f64 {
                    parity = !parity;
                    if b.y > a.y {
                        winding += 1;
                    } else {
                        winding -= 1;
                    }
                }
            }
        }
    }
    match rule {
        FillRule::EvenOdd => parity,
        FillRule::NonZero => winding != 0,
    }
}
pub(crate) fn rectangle(size: Size, radius: f32, tolerance: f32) -> Contour {
    let r = radius.max(0.0).min(size.width.min(size.height) * 0.5);
    if r <= 0.0 {
        return Contour {
            points: vec![
                Point::ZERO,
                Point::new(size.width, 0.0),
                Point::new(size.width, size.height),
                Point::new(0.0, size.height),
            ],
            closed: true,
        };
    }
    let count = (arc_segments(r, tolerance) / 4).max(2);
    let mut points = Vec::with_capacity(4 * (count + 1));
    for (center, start) in [
        (Point::new(size.width - r, r), -90.0f32),
        (Point::new(size.width - r, size.height - r), 0.0),
        (Point::new(r, size.height - r), 90.0),
        (Point::new(r, r), 180.0),
    ] {
        for i in 0..=count {
            let angle = (start + 90.0 * i as f32 / count as f32).to_radians();
            points.push(center + Point::new(angle.cos() * r, angle.sin() * r));
        }
    }
    Contour {
        points,
        closed: true,
    }
}
pub(crate) fn rounded_contains(bounds: Bounds, radius: f32, p: Point) -> bool {
    if !bounds.contains(p) {
        return false;
    }
    let r = radius
        .max(0.0)
        .min(bounds.size.width.min(bounds.size.height) * 0.5);
    if r == 0.0 {
        return true;
    }
    let x = p.x.clamp(bounds.min_x() + r, bounds.max_x() - r);
    let y = p.y.clamp(bounds.min_y() + r, bounds.max_y() - r);
    (p.x - x).hypot(p.y - y) <= r
}
pub(crate) fn arc_segments(radius: f32, tolerance: f32) -> usize {
    let tolerance = if tolerance.is_finite() && tolerance > 0.0 {
        tolerance
    } else {
        0.25
    };
    if radius <= tolerance {
        return 12;
    }
    let step = 2.0
        * (1.0 - tolerance as f64 / radius as f64)
            .clamp(-1.0, 1.0)
            .acos();
    if step <= 0.0 {
        return 4096;
    }
    (std::f64::consts::TAU / step).ceil().clamp(12.0, 4096.0) as usize
}
pub(crate) fn lerp(a: Point, b: Point, t: f32) -> Point {
    Point::new(
        (a.x as f64 + (b.x as f64 - a.x as f64) * t as f64) as f32,
        (a.y as f64 + (b.y as f64 - a.y as f64) * t as f64) as f32,
    )
}
pub(crate) fn quadratic(a: Point, b: Point, c: Point, t: f32) -> Point {
    lerp(lerp(a, b, t), lerp(b, c, t), t)
}
pub(crate) fn cubic(a: Point, b: Point, c: Point, d: Point, t: f32) -> Point {
    lerp(quadratic(a, b, c, t), quadratic(b, c, d, t), t)
}
pub(crate) fn flatten_quadratic(
    a: Point,
    b: Point,
    c: Point,
    o: FlattenOptions,
    depth: u8,
    out: &mut Vec<Point>,
) {
    if depth >= o.max_depth || distance_to_segment(b, a, c) <= o.tolerance {
        out.push(c);
        return;
    }
    let ab = lerp(a, b, 0.5);
    let bc = lerp(b, c, 0.5);
    let mid = lerp(ab, bc, 0.5);
    flatten_quadratic(a, ab, mid, o, depth + 1, out);
    flatten_quadratic(mid, bc, c, o, depth + 1, out);
}
pub(crate) fn flatten_cubic(
    a: Point,
    b: Point,
    c: Point,
    d: Point,
    o: FlattenOptions,
    depth: u8,
    out: &mut Vec<Point>,
) {
    if depth >= o.max_depth
        || distance_to_segment(b, a, d).max(distance_to_segment(c, a, d)) <= o.tolerance
    {
        out.push(d);
        return;
    }
    let ab = lerp(a, b, 0.5);
    let bc = lerp(b, c, 0.5);
    let cd = lerp(c, d, 0.5);
    let abc = lerp(ab, bc, 0.5);
    let bcd = lerp(bc, cd, 0.5);
    let mid = lerp(abc, bcd, 0.5);
    flatten_cubic(a, ab, abc, mid, o, depth + 1, out);
    flatten_cubic(mid, bcd, cd, d, o, depth + 1, out);
}

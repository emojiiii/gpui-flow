use crate::geometry::{cubic, flatten_cubic, flatten_quadratic, point_bounds, quadratic};
use crate::{Bounds, Contour, FlattenOptions, Point};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum PathCommand {
    MoveTo {
        to: Point,
    },
    LineTo {
        to: Point,
    },
    QuadraticTo {
        control: Point,
        to: Point,
    },
    CubicTo {
        control1: Point,
        control2: Point,
        to: Point,
    },
    Close,
}
/// Absolute vector commands. Curves retain their control points; flattening is
/// adaptive and only performed at the rendering/picking boundary.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Path {
    pub commands: Vec<PathCommand>,
}
impl Path {
    pub fn new(commands: Vec<PathCommand>) -> Self {
        Self { commands }
    }
    pub fn validate(&self) -> Result<(), String> {
        let mut started = false;
        for command in &self.commands {
            let points: Vec<Point> = match *command {
                PathCommand::MoveTo { to } => {
                    started = true;
                    vec![to]
                }
                PathCommand::LineTo { to } => vec![to],
                PathCommand::QuadraticTo { control, to } => vec![control, to],
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => vec![control1, control2, to],
                PathCommand::Close => vec![],
            };
            if !started {
                return Err("path must begin with MoveTo".into());
            }
            if points.iter().any(|p| !p.is_finite()) {
                return Err("path coordinates must be finite".into());
            }
        }
        if self.bounds().is_some_and(|b| !b.is_valid()) {
            return Err("path bounds overflow".into());
        }
        Ok(())
    }
    pub fn flatten(&self, tolerance: f32) -> Vec<Contour> {
        self.flatten_with(FlattenOptions {
            tolerance,
            ..FlattenOptions::default()
        })
    }
    pub fn flatten_with(&self, options: FlattenOptions) -> Vec<Contour> {
        let options = options.normalized();
        let mut result = Vec::new();
        let mut contour = Contour::default();
        let mut current = Point::ZERO;
        let mut start = Point::ZERO;
        for command in &self.commands {
            match *command {
                PathCommand::MoveTo { to } => {
                    if !contour.points.is_empty() {
                        result.push(contour);
                    }
                    contour = Contour {
                        points: vec![to],
                        closed: false,
                    };
                    current = to;
                    start = to;
                }
                PathCommand::LineTo { to } => {
                    if contour.points.is_empty() {
                        contour.points.push(current);
                    }
                    contour.points.push(to);
                    current = to;
                }
                PathCommand::QuadraticTo { control, to } => {
                    if contour.points.is_empty() {
                        contour.points.push(current);
                    }
                    flatten_quadratic(current, control, to, options, 0, &mut contour.points);
                    current = to;
                }
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    if contour.points.is_empty() {
                        contour.points.push(current);
                    }
                    flatten_cubic(
                        current,
                        control1,
                        control2,
                        to,
                        options,
                        0,
                        &mut contour.points,
                    );
                    current = to;
                }
                PathCommand::Close => {
                    if !contour.points.is_empty() {
                        contour.closed = true;
                        result.push(contour);
                        contour = Contour::default();
                    }
                    current = start;
                }
            }
        }
        if !contour.points.is_empty() {
            result.push(contour);
        }
        result
    }
    /// Tight local axis-aligned bounds from analytic Bézier extrema; not just
    /// the control-point hull or a sampling approximation.
    pub fn bounds(&self) -> Option<Bounds> {
        let mut points = Vec::new();
        let mut current = Point::ZERO;
        let mut start = Point::ZERO;
        for command in &self.commands {
            match *command {
                PathCommand::MoveTo { to } => {
                    points.push(to);
                    current = to;
                    start = to;
                }
                PathCommand::LineTo { to } => {
                    points.extend([current, to]);
                    current = to;
                }
                PathCommand::QuadraticTo { control, to } => {
                    points.extend([current, to]);
                    for (a, b, c) in [(current.x, control.x, to.x), (current.y, control.y, to.y)] {
                        let denominator = a as f64 - 2.0 * b as f64 + c as f64;
                        if denominator != 0.0 {
                            let t = (a as f64 - b as f64) / denominator;
                            if t > 0.0 && t < 1.0 {
                                points.push(quadratic(current, control, to, t as f32));
                            }
                        }
                    }
                    current = to;
                }
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    points.extend([current, to]);
                    for (p0, p1, p2, p3) in [
                        (current.x, control1.x, control2.x, to.x),
                        (current.y, control1.y, control2.y, to.y),
                    ] {
                        let p0 = p0 as f64;
                        let p1 = p1 as f64;
                        let p2 = p2 as f64;
                        let p3 = p3 as f64;
                        let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
                        let b = 2.0 * (p0 - 2.0 * p1 + p2);
                        let c = p1 - p0;
                        for t in roots(a, b, c) {
                            if t > 0.0 && t < 1.0 {
                                points.push(cubic(current, control1, control2, to, t as f32));
                            }
                        }
                    }
                    current = to;
                }
                PathCommand::Close => {
                    points.extend([current, start]);
                    current = start;
                }
            }
        }
        point_bounds(points)
    }
}
fn roots(a: f64, b: f64, c: f64) -> Vec<f64> {
    if a.abs() <= f64::EPSILON * (b.abs() + c.abs()).max(1.0) {
        return if b == 0.0 { vec![] } else { vec![-c / b] };
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return vec![];
    }
    let q = -0.5 * (b + discriminant.sqrt().copysign(b));
    if q == 0.0 {
        vec![-b / (2.0 * a)]
    } else {
        vec![q / a, c / q]
    }
}

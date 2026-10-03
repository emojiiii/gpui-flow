use crate::geometry::{flatten_cubic, flatten_quadratic, point_bounds};
use crate::{Bounds, Contour, FlattenOptions, Point};
use kurbo::{CubicBez, ParamCurveExtrema, QuadBez};
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
        self.flatten_impl(options, None)
            .expect("unbounded flattening cannot exhaust its point budget")
    }
    /// Advanced-stroke adapter entry point. Curve subdivision checks the
    /// remaining point allowance before every output point allocation.
    pub(crate) fn flatten_bounded(
        &self,
        options: FlattenOptions,
        max_points: usize,
    ) -> Result<Vec<Contour>, String> {
        if self.commands.len() > max_points {
            return Err("stroke path command budget exceeded".into());
        }
        self.flatten_impl(options, Some(max_points))
    }
    fn flatten_impl(
        &self,
        options: FlattenOptions,
        max_points: Option<usize>,
    ) -> Result<Vec<Contour>, String> {
        let options = options.normalized();
        let mut result = Vec::new();
        let mut contour = Contour::default();
        let mut completed_points = 0usize;
        let mut current = Point::ZERO;
        let mut start = Point::ZERO;
        for command in &self.commands {
            let added = match command {
                PathCommand::MoveTo { .. } | PathCommand::LineTo { .. } => {
                    1 + usize::from(
                        contour.points.is_empty() && matches!(command, PathCommand::LineTo { .. }),
                    )
                }
                PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. } => {
                    usize::from(contour.points.is_empty())
                }
                PathCommand::Close => 0,
            };
            if max_points.is_some_and(|limit| {
                completed_points
                    .saturating_add(contour.points.len())
                    .saturating_add(added)
                    > limit
            }) {
                return Err("stroke flattened point budget exceeded".into());
            }
            match *command {
                PathCommand::MoveTo { to } => {
                    if !contour.points.is_empty() {
                        completed_points += contour.points.len();
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
                    flatten_quadratic(
                        current,
                        control,
                        to,
                        options,
                        0,
                        &mut contour.points,
                        max_points
                            .unwrap_or(usize::MAX)
                            .saturating_sub(completed_points),
                    )?;
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
                        max_points
                            .unwrap_or(usize::MAX)
                            .saturating_sub(completed_points),
                    )?;
                    current = to;
                }
                PathCommand::Close => {
                    if !contour.points.is_empty() {
                        contour.closed = true;
                        completed_points += contour.points.len();
                        result.push(contour);
                        contour = Contour::default();
                    }
                    current = start;
                }
            }
            if max_points.is_some_and(|limit| completed_points + contour.points.len() > limit) {
                return Err("stroke flattened point budget exceeded".into());
            }
        }
        if !contour.points.is_empty() {
            result.push(contour);
        }
        Ok(result)
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
                    let controls = [current, control, to];
                    if controls.iter().any(|p| !p.is_finite()) {
                        return point_bounds(controls);
                    }
                    let [from, control, end] = controls.map(curve_point);
                    extend_curve_bounds(
                        &mut points,
                        QuadBez::new(from, control, end).bounding_box(),
                    );
                    current = to;
                }
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    let controls = [current, control1, control2, to];
                    if controls.iter().any(|p| !p.is_finite()) {
                        return point_bounds(controls);
                    }
                    let [from, control1, control2, end] = controls.map(curve_point);
                    extend_curve_bounds(
                        &mut points,
                        CubicBez::new(from, control1, control2, end).bounding_box(),
                    );
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
// Keep curve arithmetic wide until the scene boundary: finite f32 controls can
// overflow f32 derivative coefficients even when the curve bounds are valid.
fn curve_point(point: Point) -> kurbo::Point {
    kurbo::Point::new(point.x as f64, point.y as f64)
}

fn extend_curve_bounds(points: &mut Vec<Point>, bounds: kurbo::Rect) {
    // Use the same scene-coordinate conversion and final point_bounds check as
    // lines and moves, including rejection of unrepresentable f32 extents.
    points.extend([
        Point::new(bounds.x0 as f32, bounds.y0 as f32),
        Point::new(bounds.x1 as f32, bounds.y1 as f32),
    ]);
}

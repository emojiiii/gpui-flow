use super::{Path, PathCommand};
use crate::{Bounds, Contour, FillRule, FlattenOptions, Point, Size};
use kurbo::{Arc, PathEl, Shape as _};
use serde::{Deserialize, Serialize};

/// Ellipse in a local (0,0,width,height) box, optionally an arc, pie or ring.
/// Outer arcs sweep clockwise in scene coordinates; 0 degrees points right.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ellipse {
    pub size: Size,
    #[serde(default)]
    pub start_angle: f32,
    #[serde(default)]
    pub end_angle: f32,
    /// Ratio of inner to outer radii, from 0 through 1.
    #[serde(default)]
    pub inner_radius: f32,
    #[serde(default = "closed_default")]
    pub closed: bool,
}
fn closed_default() -> bool {
    true
}
impl Ellipse {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            size: Size::new(width, height),
            start_angle: 0.,
            end_angle: 0.,
            inner_radius: 0.,
            closed: true,
        }
    }
    pub fn circle(radius: f32) -> Self {
        Self::new(radius * 2., radius * 2.)
    }
    pub fn arc(mut self, start_angle: f32, end_angle: f32) -> Self {
        self.start_angle = start_angle;
        self.end_angle = end_angle;
        self.closed = false;
        self
    }
    pub fn sector(mut self, start_angle: f32, end_angle: f32) -> Self {
        self.start_angle = start_angle;
        self.end_angle = end_angle;
        self.closed = true;
        self
    }
    pub fn ring(mut self, inner_radius: f32) -> Self {
        self.inner_radius = inner_radius;
        self
    }
    /// Nominal sizing box. `geometry_bounds` reports actual arc/sector geometry.
    pub fn bounds(&self) -> Bounds {
        Bounds::new(Point::ZERO, self.size)
    }
    /// Preserve the original full-oval analytic/flattening paths exactly.
    pub fn is_default_oval(&self) -> bool {
        self.start_angle == 0. && self.end_angle == 0. && self.inner_radius == 0.
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.size.is_positive() {
            return Err("ellipse size must be finite and positive".into());
        }
        if !self.inner_radius.is_finite() || !(0. ..=1.).contains(&self.inner_radius) {
            return Err("ellipse inner radius ratio must be in 0..=1".into());
        }
        if !self.start_angle.is_finite()
            || !self.end_angle.is_finite()
            || self.start_angle.abs() > 1_000_000.
            || self.end_angle.abs() > 1_000_000.
            || (self.end_angle as f64 - self.start_angle as f64).abs() > 360.
        {
            return Err(
                "ellipse angles require finite +/-1000000 degrees and at most one revolution"
                    .into(),
            );
        }
        if !self.is_default_oval()
            && (self.size.width > 1_000_000. || self.size.height > 1_000_000.)
        {
            return Err("arc/ring ellipse size exceeds 1000000 local units".into());
        }
        Ok(())
    }
    /// Canonical curve geometry shared by bounds, pixels, hit testing and strokes.
    /// Uses Kurbo's cubic arc conversion at 0.01 local-unit tolerance. Both zero
    /// angles mean a full oval; other equal angles mean empty geometry. Signed
    /// spans of exactly +/-360 canonicalize to one clockwise full revolution.
    pub fn to_path(&self) -> Result<Path, String> {
        self.validate()?;
        if self.size.width > 1_000_000. || self.size.height > 1_000_000. {
            return Err("ellipse path conversion size exceeds 1000000 local units".into());
        }
        let delta = self.end_angle as f64 - self.start_angle as f64;
        let sweep = if (self.start_angle == 0. && self.end_angle == 0.) || delta.abs() == 360. {
            360.
        } else {
            delta.rem_euclid(360.)
        };
        if sweep == 0. {
            return Ok(Path::default());
        }
        let start = (self.start_angle as f64).rem_euclid(360.).to_radians();
        let full = sweep == 360.;
        let rx = self.size.width as f64 * 0.5;
        let ry = self.size.height as f64 * 0.5;
        let outer = Arc::new((rx, ry), (rx, ry), start, sweep.to_radians(), 0.);
        let mut commands = Vec::new();
        let inner = self.inner_radius as f64;
        if inner == 0. {
            if !full && self.closed {
                commands.push(PathCommand::MoveTo {
                    to: Point::new(rx as f32, ry as f32),
                });
            }
            let connect = !commands.is_empty();
            append_arc(&mut commands, outer, connect)?;
            if full || self.closed {
                commands.push(PathCommand::Close);
            }
        } else {
            append_arc(&mut commands, outer, false)?;
            if inner < 1. || self.closed {
                if full {
                    commands.push(PathCommand::Close);
                }
                let arc = Arc::new(
                    (rx, ry),
                    (rx * inner, ry * inner),
                    start,
                    sweep.to_radians(),
                    0.,
                )
                .reversed();
                append_arc(&mut commands, arc, !full)?;
                commands.push(PathCommand::Close);
            } else if full {
                commands.push(PathCommand::Close);
            }
        }
        if commands.len() > 64 {
            return Err("ellipse curve command budget exceeded".into());
        }
        let path = Path::new(commands);
        path.validate()?;
        Ok(path)
    }
    pub fn geometry_bounds(&self) -> Option<Bounds> {
        if self.is_default_oval() {
            Some(self.bounds())
        } else {
            self.to_path().ok()?.bounds()
        }
    }
    /// Every contour is returned, including an oppositely wound inner ring.
    /// Generated curves retain a seven-level subdivision bound (at most 8192
    /// points); the original full oval retains its existing 4096-point cap.
    pub fn flatten(&self, tolerance: f32) -> Vec<Contour> {
        if self.validate().is_err() {
            return Vec::new();
        }
        if self.is_default_oval() {
            let count = crate::geometry::arc_segments(
                self.size.width.max(self.size.height) * 0.5,
                tolerance,
            );
            let points = (0..count)
                .map(|i| {
                    let angle = std::f32::consts::TAU * i as f32 / count as f32;
                    Point::new(
                        (1. + angle.cos()) * self.size.width * 0.5,
                        (1. + angle.sin()) * self.size.height * 0.5,
                    )
                })
                .collect();
            return vec![Contour {
                points,
                closed: true,
            }];
        }
        self.to_path().map_or_else(
            |_| Vec::new(),
            |path| {
                path.flatten_with(FlattenOptions {
                    tolerance,
                    max_depth: 7,
                })
            },
        )
    }
    pub fn contains(&self, point: Point) -> bool {
        self.contains_with_rule(point, FillRule::NonZero)
    }
    pub fn contains_with_rule(&self, point: Point, rule: FillRule) -> bool {
        if !point.is_finite() || self.validate().is_err() {
            return false;
        }
        if self.is_default_oval() {
            let x = (point.x - self.size.width * 0.5) / (self.size.width * 0.5);
            let y = (point.y - self.size.height * 0.5) / (self.size.height * 0.5);
            x * x + y * y <= 1.
        } else {
            crate::geometry::fill_hit(&self.flatten(0.1), point, rule)
        }
    }
}
fn append_arc(commands: &mut Vec<PathCommand>, arc: Arc, connect: bool) -> Result<(), String> {
    for element in arc.path_elements(0.01) {
        if commands.len() >= 64 {
            return Err("ellipse curve command budget exceeded".into());
        }
        let point = |p: kurbo::Point| Point::new(p.x as f32, p.y as f32);
        commands.push(match element {
            PathEl::MoveTo(to) if connect => PathCommand::LineTo { to: point(to) },
            PathEl::MoveTo(to) => PathCommand::MoveTo { to: point(to) },
            PathEl::LineTo(to) => PathCommand::LineTo { to: point(to) },
            PathEl::QuadTo(control, to) => PathCommand::QuadraticTo {
                control: point(control),
                to: point(to),
            },
            PathEl::CurveTo(control1, control2, to) => PathCommand::CubicTo {
                control1: point(control1),
                control2: point(control2),
                to: point(to),
            },
            PathEl::ClosePath => PathCommand::Close,
        });
    }
    Ok(())
}

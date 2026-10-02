//! Bounded curve-based path measurement. Kurbo performs curve integration and
//! distance inversion; display flattening and its tolerance are not reused.
use super::{Path, PathCommand};
use crate::{Affine, Point};
use kurbo::{
    CubicBez, Line, ParamCurve, ParamCurveArclen, ParamCurveDeriv, ParamCurveExtrema, PathSeg,
    QuadBez, Vec2,
};

/// Position and forward unit tangent on a measured path. Rotation is in degrees
/// in the same coordinate system as `point` (positive toward positive y).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathSample {
    pub point: Point,
    pub tangent: Point,
    pub rotation_degrees: f32,
}
impl PathSample {
    /// Transform position and direction, then renormalize the tangent. Handles
    /// reflection, skew and nonuniform scale; rejects a collapsed direction.
    pub fn transformed(self, transform: Affine) -> Result<Self, String> {
        if !transform.is_finite() || !self.point.is_finite() || !self.tangent.is_finite() {
            return Err("path sample transform must be finite".into());
        }
        let point = transform.transform_point(self.point);
        let tangent = Vec2::new(
            transform.a as f64 * self.tangent.x as f64 + transform.c as f64 * self.tangent.y as f64,
            transform.b as f64 * self.tangent.x as f64 + transform.d as f64 * self.tangent.y as f64,
        );
        sample(point, tangent)
    }
}

#[derive(Debug, Clone)]
struct MeasuredSegment {
    curve: PathSeg,
    start: f64,
    end: f64,
    length: f64,
}
/// Immutable local-space measurement snapshot. Moves add no distance; closes
/// add their real closing edge. Retain this snapshot for repeated sampling, or
/// measure again after changing path data. No scene identity/cache is captured.
#[derive(Debug, Clone)]
pub struct PathMeasure {
    segments: Vec<MeasuredSegment>,
    total: f64,
    accuracy: f64,
}
impl Path {
    /// Measure the bounded Path/Pen data subset accepted by `to_svg`.
    /// Kurbo's requested integration error totals at most 0.001 local units
    /// across all segments (subject to floating-point roundoff).
    pub fn measure(&self) -> Result<PathMeasure, String> {
        self.to_svg()?;
        let accuracy = 0.001 / (5 * self.commands.len().max(1)) as f64;
        let mut result = PathMeasure {
            segments: Vec::new(),
            total: 0.,
            accuracy,
        };
        let mut current = kurbo::Point::ORIGIN;
        let mut start = current;
        for command in &self.commands {
            let curve = match *command {
                PathCommand::MoveTo { to } => {
                    current = wide(to);
                    start = current;
                    continue;
                }
                PathCommand::LineTo { to } => PathSeg::Line(Line::new(current, wide(to))),
                PathCommand::QuadraticTo { control, to } => {
                    // Exact degree elevation avoids the pinned Kurbo quadratic
                    // length formula's coincident-control/collinear-cusp edge
                    // cases. Cubic integration retains the original curve.
                    PathSeg::Cubic(QuadBez::new(current, wide(control), wide(to)).raise())
                }
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => PathSeg::Cubic(CubicBez::new(
                    current,
                    wide(control1),
                    wide(control2),
                    wide(to),
                )),
                PathCommand::Close => PathSeg::Line(Line::new(current, start)),
            };
            current = curve.end();
            // Kurbo supplies at most five monotone pieces. Inversion must not
            // straddle a stationary cusp, where a tangent is not unique.
            for range in curve.extrema_ranges() {
                let curve = curve.subsegment(range);
                let length = curve.arclen(accuracy);
                if !length.is_finite() || length < 0. {
                    return Err("path length is not finite".into());
                }
                if length == 0. {
                    continue;
                }
                let end = result.total + length;
                if !end.is_finite() || end <= result.total {
                    return Err("path length exceeds supported numeric precision".into());
                }
                result.segments.push(MeasuredSegment {
                    curve,
                    start: result.total,
                    end,
                    length,
                });
                result.total = end;
            }
        }
        Ok(result)
    }
}
impl PathMeasure {
    pub fn total_length(&self) -> f64 {
        self.total
    }

    /// Clamp a finite distance to [0, total]. Empty/move-only/zero-length paths
    /// return None. At a segment/subpath boundary use the preceding endpoint;
    /// immediately after a disjoint move sampling continues on the next subpath.
    /// Stationary cusps are boundaries too: use the incoming tangent at the
    /// cusp, and the outgoing tangent just after it.
    pub fn sample(&self, distance: f64) -> Result<Option<PathSample>, String> {
        if !distance.is_finite() {
            return Err("path distance must be finite".into());
        }
        if self.segments.is_empty() {
            return Ok(None);
        }
        let distance = distance.clamp(0., self.total);
        let index = self
            .segments
            .partition_point(|segment| segment.end < distance);
        let segment = &self.segments[index.min(self.segments.len() - 1)];
        let local = (distance - segment.start).clamp(0., segment.length);
        let t = if local == 0. {
            0.
        } else if distance == segment.end {
            1.
        } else {
            segment
                .curve
                .inv_arclen(local, self.accuracy.min(segment.length * 1e-6))
        };
        if !t.is_finite() || !(0. ..=1.).contains(&t) {
            return Err("path distance inversion failed".into());
        }
        let position = segment.curve.eval(t);
        // Raising a quadratic preserves its parameter. The line-to-cubic
        // helper changes speed but preserves direction, which is all we use.
        let derivative = segment.curve.to_cubic().deriv();
        let epsilon = derivative
            .p0
            .to_vec2()
            .hypot()
            .max(derivative.p1.to_vec2().hypot())
            .max(derivative.p2.to_vec2().hypot())
            * f64::EPSILON
            * 64.;
        let mut tangent = derivative.eval(t).to_vec2();
        if tangent.hypot() <= epsilon {
            let second = derivative.deriv();
            tangent = second.eval(t).to_vec2() * if t == 1. { -1. } else { 1. };
            if tangent.hypot() <= epsilon {
                tangent = second.deriv().eval(t).to_vec2();
            }
        }
        sample(Point::new(position.x as f32, position.y as f32), tangent).map(Some)
    }

    /// Sample a finite fraction, clamped to [0, 1], in local arc-length space.
    pub fn sample_fraction(&self, fraction: f64) -> Result<Option<PathSample>, String> {
        if !fraction.is_finite() {
            return Err("path fraction must be finite".into());
        }
        self.sample(fraction.clamp(0., 1.) * self.total)
    }
}
fn wide(point: Point) -> kurbo::Point {
    kurbo::Point::new(point.x as f64, point.y as f64)
}
fn sample(point: Point, tangent: Vec2) -> Result<PathSample, String> {
    let length = tangent.hypot();
    if !point.is_finite() || !length.is_finite() || length == 0. {
        return Err("path sample has no finite nonzero tangent".into());
    }
    let tangent = tangent / length;
    Ok(PathSample {
        point,
        tangent: Point::new(tangent.x as f32, tangent.y as f32),
        rotation_degrees: tangent.y.atan2(tangent.x).to_degrees() as f32,
    })
}

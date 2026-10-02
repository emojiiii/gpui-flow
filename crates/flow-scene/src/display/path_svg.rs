//! Bounded SVG `d` adapter. Syntax comes from svgtypes; ellipse conversion from
//! Kurbo. Stored commands stay backend-independent and use the existing path
//! bounds, picking, stroke and compositor implementations.
use super::{Path, PathCommand};
use crate::Point;
use kurbo::{Arc, Point as KPoint, SvgArc, Vec2};
use std::fmt::Write;
use svgtypes::{PathParser, PathSegment as S};

const MAX_BYTES: usize = 1_048_576;
const MAX_COMMANDS: usize = 16_384;
const MAX_COORDINATE: f64 = 1_000_000.;

fn scalar(value: f64) -> Result<f64, String> {
    if value.is_finite() && value.abs() <= MAX_COORDINATE {
        Ok(value)
    } else {
        Err("SVG path coordinate exceeds finite +/-1000000 range".into())
    }
}
fn point(x: f64, y: f64) -> Result<KPoint, String> {
    Ok(KPoint::new(scalar(x)?, scalar(y)?))
}
fn stored(p: KPoint) -> Result<Point, String> {
    let p = point(p.x, p.y)?;
    Ok(Point::new(p.x as f32, p.y as f32))
}
fn push(out: &mut Vec<PathCommand>, command: PathCommand) -> Result<(), String> {
    if out.len() >= MAX_COMMANDS {
        return Err("SVG path exceeds 16384 normalized commands".into());
    }
    out.push(command);
    Ok(())
}

impl Path {
    /// Export normalized absolute M/L/Q/C/Z path data, never an SVG document.
    /// Validates before returning any data. The SVG adapter's 1 MiB, 16384
    /// command and +/-1e6 coordinate limits also apply to typed path updates.
    /// Arcs imported by `from_svg` remain cubic curves; source spelling and
    /// shorthand commands are not retained. Negative zero is written as zero.
    pub fn to_svg(&self) -> Result<String, String> {
        if self.commands.len() > MAX_COMMANDS {
            return Err("SVG path exceeds 16384 normalized commands".into());
        }
        self.validate()?;
        let number = |value: f32| -> Result<f32, String> {
            scalar(value as f64)?;
            Ok(if value == 0. { 0. } else { value })
        };
        let mut out = String::new();
        for command in &self.commands {
            if !out.is_empty() {
                out.push(' ');
            }
            match *command {
                PathCommand::MoveTo { to } => {
                    write!(out, "M{} {}", number(to.x)?, number(to.y)?).unwrap();
                }
                PathCommand::LineTo { to } => {
                    write!(out, "L{} {}", number(to.x)?, number(to.y)?).unwrap();
                }
                PathCommand::QuadraticTo { control, to } => {
                    write!(
                        out,
                        "Q{} {} {} {}",
                        number(control.x)?,
                        number(control.y)?,
                        number(to.x)?,
                        number(to.y)?
                    )
                    .unwrap();
                }
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    write!(
                        out,
                        "C{} {} {} {} {} {}",
                        number(control1.x)?,
                        number(control1.y)?,
                        number(control2.x)?,
                        number(control2.y)?,
                        number(to.x)?,
                        number(to.y)?
                    )
                    .unwrap();
                }
                PathCommand::Close => out.push('Z'),
            }
            if out.len() > MAX_BYTES {
                return Err("SVG path exceeds 1 MiB output limit".into());
            }
        }
        Ok(out)
    }
    /// Import SVG path data (`d`), not an SVG document or external resource.
    /// Supports M/L/H/V/C/S/Q/T/A/Z, both absolute and relative. Elliptical
    /// arcs become cubic curves at Kurbo's 0.1 local-unit tolerance; no exact
    /// arc/source-string round trip is promised. Invalid input is atomic.
    /// Limits: 1 MiB input, 16384 input/output commands, +/-1e6 coordinates.
    pub fn from_svg(data: &str) -> Result<Self, String> {
        if data.len() > MAX_BYTES {
            return Err("SVG path exceeds 1 MiB input limit".into());
        }
        let mut out = Vec::new();
        let mut current = KPoint::ORIGIN;
        let mut start = current;
        let mut cubic: Option<KPoint> = None;
        let mut quadratic: Option<KPoint> = None;
        let mut closed = false;
        for (index, segment) in PathParser::from(data).enumerate() {
            if index >= MAX_COMMANDS {
                return Err("SVG path exceeds 16384 input commands".into());
            }
            let segment = segment.map_err(|e| format!("invalid SVG path: {e}"))?;
            if closed && !matches!(segment, S::MoveTo { .. } | S::ClosePath { .. }) {
                push(
                    &mut out,
                    PathCommand::MoveTo {
                        to: stored(current)?,
                    },
                )?;
                closed = false;
            }
            let resolve = |abs: bool, x: f64, y: f64| {
                scalar(x)?;
                scalar(y)?;
                point(
                    x + if abs { 0. } else { current.x },
                    y + if abs { 0. } else { current.y },
                )
            };
            let mut next_cubic = None;
            let mut next_quadratic = None;
            match segment {
                S::MoveTo { abs, x, y } => {
                    current = resolve(abs, x, y)?;
                    start = current;
                    closed = false;
                    push(
                        &mut out,
                        PathCommand::MoveTo {
                            to: stored(current)?,
                        },
                    )?;
                }
                S::LineTo { abs, x, y } => {
                    current = resolve(abs, x, y)?;
                    push(
                        &mut out,
                        PathCommand::LineTo {
                            to: stored(current)?,
                        },
                    )?;
                }
                S::HorizontalLineTo { abs, x } => {
                    current = point(scalar(x)? + if abs { 0. } else { current.x }, current.y)?;
                    push(
                        &mut out,
                        PathCommand::LineTo {
                            to: stored(current)?,
                        },
                    )?;
                }
                S::VerticalLineTo { abs, y } => {
                    current = point(current.x, scalar(y)? + if abs { 0. } else { current.y })?;
                    push(
                        &mut out,
                        PathCommand::LineTo {
                            to: stored(current)?,
                        },
                    )?;
                }
                S::CurveTo {
                    abs,
                    x1,
                    y1,
                    x2,
                    y2,
                    x,
                    y,
                } => {
                    let a = resolve(abs, x1, y1)?;
                    let b = resolve(abs, x2, y2)?;
                    let to = resolve(abs, x, y)?;
                    push(
                        &mut out,
                        PathCommand::CubicTo {
                            control1: stored(a)?,
                            control2: stored(b)?,
                            to: stored(to)?,
                        },
                    )?;
                    next_cubic = Some(b);
                    current = to;
                }
                S::SmoothCurveTo { abs, x2, y2, x, y } => {
                    let a = cubic.map_or(current, |c| current + (current - c));
                    let b = resolve(abs, x2, y2)?;
                    let to = resolve(abs, x, y)?;
                    push(
                        &mut out,
                        PathCommand::CubicTo {
                            control1: stored(a)?,
                            control2: stored(b)?,
                            to: stored(to)?,
                        },
                    )?;
                    next_cubic = Some(b);
                    current = to;
                }
                S::Quadratic { abs, x1, y1, x, y } => {
                    let control = resolve(abs, x1, y1)?;
                    let to = resolve(abs, x, y)?;
                    push(
                        &mut out,
                        PathCommand::QuadraticTo {
                            control: stored(control)?,
                            to: stored(to)?,
                        },
                    )?;
                    next_quadratic = Some(control);
                    current = to;
                }
                S::SmoothQuadratic { abs, x, y } => {
                    let control = quadratic.map_or(current, |c| current + (current - c));
                    let to = resolve(abs, x, y)?;
                    push(
                        &mut out,
                        PathCommand::QuadraticTo {
                            control: stored(control)?,
                            to: stored(to)?,
                        },
                    )?;
                    next_quadratic = Some(control);
                    current = to;
                }
                S::EllipticalArc {
                    abs,
                    rx,
                    ry,
                    x_axis_rotation,
                    large_arc,
                    sweep,
                    x,
                    y,
                } => {
                    let to = resolve(abs, x, y)?;
                    let radii = Vec2::new(scalar(rx)?.abs(), scalar(ry)?.abs());
                    let rotation = scalar(x_axis_rotation)?.to_radians();
                    // Coincident endpoints omit the arc; zero radius is a line.
                    if current != to {
                        if radii.x != 0.
                            && radii.y != 0.
                            && (radii.x <= 1e-5 || radii.y <= 1e-5 || current.distance(to) < 1e-9)
                        {
                            return Err("SVG arc is below supported numerical scale".into());
                        }
                        if let Some(arc) = Arc::from_svg_arc(&SvgArc {
                            from: current,
                            to,
                            radii,
                            x_rotation: rotation,
                            large_arc,
                            sweep,
                        }) {
                            // Radius correction can magnify extreme aspect
                            // ratios. Bound before subdivision, not afterwards.
                            point(arc.center.x, arc.center.y)?;
                            scalar(arc.radii.x)?;
                            scalar(arc.radii.y)?;
                            if !arc.start_angle.is_finite() || !arc.sweep_angle.is_finite() {
                                return Err("SVG arc numerical overflow".into());
                            }
                            let before = out.len();
                            let mut error = None;
                            arc.to_cubic_beziers(0.1, |a, b, end| {
                                if error.is_none() {
                                    let result = (|| {
                                        push(
                                            &mut out,
                                            PathCommand::CubicTo {
                                                control1: stored(a)?,
                                                control2: stored(b)?,
                                                to: stored(end)?,
                                            },
                                        )
                                    })();
                                    error = result.err();
                                }
                            });
                            if let Some(error) = error {
                                return Err(error);
                            }
                            if out.len() == before {
                                return Err("SVG arc produced no finite curve".into());
                            }
                            // Numerical ellipse conversion must not drift the
                            // canonical endpoint used by the following segment.
                            if let Some(PathCommand::CubicTo { to: end, .. }) = out.last_mut() {
                                *end = stored(to)?;
                            }
                        } else {
                            push(&mut out, PathCommand::LineTo { to: stored(to)? })?;
                        }
                    }
                    current = to;
                }
                S::ClosePath { .. } => {
                    if !closed {
                        push(&mut out, PathCommand::Close)?;
                    }
                    current = start;
                    closed = true;
                }
            }
            cubic = next_cubic;
            quadratic = next_quadratic;
        }
        let path = Self::new(out);
        path.validate()?;
        Ok(path)
    }
}

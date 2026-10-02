//! Persistent geometry, frozen in logical Canvas space at append time.
use super::{CanvasCommand, clip::Budget};
use crate::{CompositeError, CompositeLimits};
use gpui_flow_scene::{
    Affine, Point,
    display::{Path, PathCommand},
};
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub(crate) struct CurrentPath {
    path: Arc<Path>,
    start: Option<Point>,
    closed: bool,
    has_segment: bool,
}
impl CurrentPath {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn byte_len(&self) -> u64 {
        (self.path.commands.capacity() * size_of::<PathCommand>()) as u64
    }
    pub fn additional_bytes(&self, baseline: &Self) -> u64 {
        if Arc::ptr_eq(&self.path, &baseline.path) {
            0
        } else {
            self.byte_len()
        }
    }
    pub fn snapshot(&self) -> Arc<Path> {
        self.path.clone()
    }
    fn append(
        &mut self,
        commands: &[PathCommand],
        reserve: usize,
        budget: &mut Budget,
        limits: CompositeLimits,
    ) -> Result<(), CompositeError> {
        let len = self.path.commands.len().saturating_add(commands.len());
        budget.input_count(commands.len(), limits)?;
        if len > limits.max_points {
            return Err(CompositeError::InputLimit);
        }
        if Arc::get_mut(&mut self.path).is_none() || self.path.commands.capacity() < len {
            let capacity = self
                .path
                .commands
                .len()
                .saturating_add(reserve)
                .min(limits.max_points)
                .max(len);
            // Copy-on-write snapshots never observe later path mutations. Both
            // the previous allocation and its candidate are admitted first.
            budget.input_count(self.path.commands.len(), limits)?;
            budget.path_allocation(capacity, limits)?;
            let mut copy = Vec::new();
            copy.try_reserve_exact(capacity)
                .map_err(|_| CompositeError::AllocationFailed)?;
            copy.extend_from_slice(&self.path.commands);
            self.path = Arc::new(Path::new(copy));
        }
        Arc::get_mut(&mut self.path)
            .expect("unique candidate")
            .commands
            .extend_from_slice(commands);
        Ok(())
    }
    pub(super) fn command(
        &mut self,
        command: &CanvasCommand,
        transform: Affine,
        reserve: usize,
        budget: &mut Budget,
        limits: CompositeLimits,
    ) -> Result<bool, CompositeError> {
        let (start, item) = match *command {
            CanvasCommand::BeginPath => {
                *self = Self::default();
                return Ok(true);
            }
            CanvasCommand::MoveTo { to } => {
                let to = point(transform, to)?;
                self.append(&[PathCommand::MoveTo { to }], reserve, budget, limits)?;
                self.start = Some(to);
                self.closed = false;
                self.has_segment = false;
                return Ok(true);
            }
            CanvasCommand::LineTo { to } => {
                let to = point(transform, to)?;
                (to, PathCommand::LineTo { to })
            }
            CanvasCommand::QuadraticCurveTo { control, to } => {
                let control = point(transform, control)?;
                (
                    control,
                    PathCommand::QuadraticTo {
                        control,
                        to: point(transform, to)?,
                    },
                )
            }
            CanvasCommand::BezierCurveTo {
                control1,
                control2,
                to,
            } => {
                let control1 = point(transform, control1)?;
                (
                    control1,
                    PathCommand::CubicTo {
                        control1,
                        control2: point(transform, control2)?,
                        to: point(transform, to)?,
                    },
                )
            }
            CanvasCommand::ClosePath => {
                if self.has_segment && !self.closed {
                    self.append(&[PathCommand::Close], reserve, budget, limits)?;
                    self.closed = true;
                }
                return Ok(true);
            }
            _ => return Ok(false),
        };
        if self.start.is_none() {
            // HTML's implicit first point is the line endpoint or first control.
            self.append(
                &[PathCommand::MoveTo { to: start }],
                reserve,
                budget,
                limits,
            )?;
            self.start = Some(start);
            if matches!(item, PathCommand::LineTo { .. }) {
                return Ok(true);
            }
        } else if self.closed {
            // Explicit restart makes continuation after Close consistent in all
            // existing fill, stroke, bounds and curve preparation adapters.
            self.append(
                &[PathCommand::MoveTo {
                    to: self.start.unwrap(),
                }],
                reserve,
                budget,
                limits,
            )?;
        }
        self.append(&[item], reserve, budget, limits)?;
        self.closed = false;
        self.has_segment = true;
        Ok(true)
    }
    pub(super) fn local_snapshot(
        &self,
        transform: Affine,
        budget: &mut Budget,
        limits: CompositeLimits,
    ) -> Result<Path, CompositeError> {
        let inverse = transform
            .inverse()
            .ok_or(CompositeError::UnsupportedTransform)?;
        if [
            inverse.a, inverse.b, inverse.c, inverse.d, inverse.tx, inverse.ty,
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return Err(CompositeError::UnsupportedTransform);
        }
        budget.path_allocation(self.path.commands.len(), limits)?;
        let mut commands = Vec::new();
        commands
            .try_reserve_exact(self.path.commands.len())
            .map_err(|_| CompositeError::AllocationFailed)?;
        for command in &self.path.commands {
            commands.push(match *command {
                PathCommand::MoveTo { to } => PathCommand::MoveTo {
                    to: point(inverse, to)?,
                },
                PathCommand::LineTo { to } => PathCommand::LineTo {
                    to: point(inverse, to)?,
                },
                PathCommand::QuadraticTo { control, to } => PathCommand::QuadraticTo {
                    control: point(inverse, control)?,
                    to: point(inverse, to)?,
                },
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => PathCommand::CubicTo {
                    control1: point(inverse, control1)?,
                    control2: point(inverse, control2)?,
                    to: point(inverse, to)?,
                },
                PathCommand::Close => PathCommand::Close,
            });
        }
        let path = Path::new(commands);
        path.validate().map_err(CompositeError::InvalidGeometry)?;
        Ok(path)
    }
}
fn point(transform: Affine, value: Point) -> Result<Point, CompositeError> {
    if !value.is_finite() {
        return Err(CompositeError::InvalidGeometry(
            "Canvas path coordinates must be finite".into(),
        ));
    }
    // Affine uses wide intermediates; reject unrepresentable stored f32s.
    let point = transform.transform_point(value);
    if point.is_finite() {
        Ok(point)
    } else {
        Err(CompositeError::InvalidGeometry(
            "Canvas transformed path coordinates overflow".into(),
        ))
    }
}

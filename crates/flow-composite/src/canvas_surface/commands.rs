//! A bounded, transaction-local vector context over the existing compositor.
use super::CanvasSurface;
use crate::{CompositeError, CompositeLimits, DomainNode, DomainSource, ImageInputs, TextInputs};
use gpui_flow_scene::{
    Affine, DisplayObject, FillRule, Paint, Stroke, StrokeAlignment, Style,
    display::{Group, Path},
};

/// Maximum operations, including state operations, in one atomic batch.
pub const MAX_CANVAS_COMMANDS: usize = 256;
/// Save/restore state never survives a batch or enters a scene document.
pub const MAX_CANVAS_STATE_DEPTH: usize = 16;

/// Owned paths are evaluated under the current transform at the draw command.
/// Each draw is source-over; alpha applies to that draw, not the whole batch.
/// State starts at identity/1 and the save stack must finish balanced.
#[derive(Clone, Debug, PartialEq)]
pub enum CanvasCommand {
    Save,
    Restore,
    /// Postmultiply the current local-to-Canvas transform.
    Transform(Affine),
    SetTransform(Affine),
    SetOpacity(f32),
    FillPath {
        path: Path,
        paint: Paint,
        rule: FillRule,
    },
    /// Reuses Flow's centered local-space cap/join/dash stroke geometry.
    StrokePath {
        path: Path,
        stroke: Stroke,
    },
}
#[derive(Clone, Copy)]
struct State {
    transform: Affine,
    opacity: f32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            transform: Affine::IDENTITY,
            opacity: 1.,
        }
    }
}
fn matrix(value: Affine) -> Result<Affine, CompositeError> {
    if value.inverse().is_none()
        || [value.a, value.b, value.c, value.d, value.tx, value.ty]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
    {
        Err(CompositeError::UnsupportedTransform)
    } else {
        Ok(value)
    }
}
fn paint_budget(paint: &Paint) -> Result<(), CompositeError> {
    if matches!(paint, Paint::LinearGradient { stops, .. } | Paint::RadialGradient { stops, .. } if stops.len() > 256)
    {
        Err(CompositeError::InputLimit)
    } else {
        Ok(())
    }
}
fn source(
    commands: &[CanvasCommand],
    limits: CompositeLimits,
) -> Result<Option<DomainSource>, CompositeError> {
    if commands.len() > MAX_CANVAS_COMMANDS {
        return Err(CompositeError::InputLimit);
    }
    let mut state = State::default();
    let mut stack = Vec::new();
    let mut children = Vec::new();
    let mut draws = 0usize;
    let mut points = 0usize;
    for command in commands {
        let (path, style) = match command {
            CanvasCommand::Save => {
                if stack.len() >= MAX_CANVAS_STATE_DEPTH.min(limits.max_depth) {
                    return Err(CompositeError::NestingLimit);
                }
                stack.push(state);
                continue;
            }
            CanvasCommand::Restore => {
                state = stack.pop().ok_or_else(|| {
                    CompositeError::InvalidGeometry("Canvas restore without save".into())
                })?;
                continue;
            }
            CanvasCommand::Transform(value) => {
                state.transform = matrix(state.transform * matrix(*value)?)?;
                continue;
            }
            CanvasCommand::SetTransform(value) => {
                state.transform = matrix(*value)?;
                continue;
            }
            CanvasCommand::SetOpacity(value) => {
                if !value.is_finite() || !(0. ..=1.).contains(value) {
                    return Err(CompositeError::InvalidGeometry(
                        "Canvas opacity must be in [0,1]".into(),
                    ));
                }
                state.opacity = *value;
                continue;
            }
            CanvasCommand::FillPath { path, paint, rule } => {
                paint_budget(paint)?;
                (
                    path,
                    Style {
                        fill: Some(paint.clone()),
                        fill_rule: *rule,
                        opacity: state.opacity,
                        ..Default::default()
                    },
                )
            }
            CanvasCommand::StrokePath { path, stroke } => {
                if stroke.alignment != StrokeAlignment::Center
                    || stroke.dash_pattern.len() > gpui_flow_scene::MAX_STROKE_DASH_ENTRIES
                {
                    return Err(CompositeError::InputLimit);
                }
                paint_budget(&stroke.paint)?;
                (
                    path,
                    Style {
                        fill: None,
                        stroke: Some(stroke.clone()),
                        opacity: state.opacity,
                        ..Default::default()
                    },
                )
            }
        };
        // Admission precedes cloning or validation's geometry preparation.
        draws += 1;
        points = points.saturating_add(path.commands.len());
        if draws + 1 > limits.max_nodes
            || path.commands.len() > limits.max_points
            || points > limits.max_total_points
        {
            return Err(CompositeError::InputLimit);
        }
        let object = DisplayObject::new(format!("canvas-command-{draws}"), path.clone())
            .with_transform(state.transform)
            .with_style(style);
        object.validate().map_err(CompositeError::InvalidGeometry)?;
        // Compile directly to the existing domain IR: repeated Scene::insert
        // would clone/revalidate all preceding paths for every command.
        children.push(DomainNode {
            object,
            world_transform: state.transform,
            children: vec![],
        });
    }
    if !stack.is_empty() {
        return Err(CompositeError::InvalidGeometry(
            "unbalanced Canvas save stack".into(),
        ));
    }
    if draws == 0 {
        return Ok(None);
    }
    Ok(Some(DomainSource {
        root: DomainNode {
            object: DisplayObject::new("canvas-command-root", Group::new()),
            world_transform: Affine::IDENTITY,
            children,
        },
        clips: vec![],
        canvas: None,
    }))
}
impl CanvasSurface {
    /// Render an ordered vector batch and publish at most one immutable generation.
    /// Any invalid command, stack, geometry or budget leaves all pixels untouched.
    /// Returns false when there is no visible output. Coordinates are logical;
    /// the existing surface DPR mapping is applied once after the command state.
    /// No context state, current path or command history is retained afterward.
    pub fn draw_commands(&mut self, commands: &[CanvasCommand]) -> Result<bool, CompositeError> {
        let Some(source) = source(commands, self.limits)? else {
            return Ok(false);
        };
        let before = self.generation();
        self.draw(
            &source,
            Affine::IDENTITY,
            &ImageInputs::default(),
            &TextInputs::default(),
        )?;
        Ok(self.generation() != before)
    }
}

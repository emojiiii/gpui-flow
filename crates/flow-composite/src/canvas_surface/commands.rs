//! A bounded, transaction-local vector context over the existing compositor.
mod clip;
mod current_path;
pub(crate) use current_path::CurrentPath;
mod image;
mod pattern;
pub use pattern::{CanvasPathTarget, CanvasPattern, CanvasPatternRepeat, CanvasStrokeOptions};
mod stroke_text;
mod text;
use super::CanvasSurface;
use crate::{CompositeError, CompositeLimits, DomainNode, DomainSource, ImageInputs, TextInputs};
use clip::{Budget, Clip};
use gpui_flow_scene::{
    Affine, Bounds, Color, DisplayId, DisplayObject, FillRule, Paint, Point, Stroke,
    StrokeAlignment, Style,
    display::{Group, Path, Text},
};
use std::{
    borrow::Cow,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
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
    /// Clear only the persistent current geometry; state and pixels survive.
    BeginPath,
    MoveTo {
        to: Point,
    },
    LineTo {
        to: Point,
    },
    QuadraticCurveTo {
        control: Point,
        to: Point,
    },
    BezierCurveTo {
        control1: Point,
        control2: Point,
        to: Point,
    },
    ClosePath,
    /// Draw a frozen snapshot without consuming the persistent path.
    FillCurrentPath {
        paint: Paint,
        rule: FillRule,
    },
    StrokeCurrentPath {
        stroke: Stroke,
    },
    /// Freeze the current geometry in this batch's clip state.
    ClipCurrentPath {
        rule: FillRule,
    },
    /// Intersect the current clip with this path, frozen at the current transform.
    /// Opacity does not affect clipping. Save/Restore includes the clip state.
    ClipPath {
        path: Path,
        rule: FillRule,
    },
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
    /// Runtime image pattern using the current path or an explicit local path.
    FillPattern {
        path: CanvasPathTarget,
        pattern: CanvasPattern,
        rule: FillRule,
    },
    StrokePattern {
        path: CanvasPathTarget,
        pattern: CanvasPattern,
        options: CanvasStrokeOptions,
    },
    /// Immutable decoded input, never a URL. Source is in physical image pixels;
    /// destination is in logical Canvas units before the current transform.
    DrawImage {
        image: DisplayId,
        source: Option<Bounds>,
        destination: Bounds,
    },
    /// Solid text in an explicit local box. Position anchors its top-left,
    /// not an HTML alphabetic baseline. Fonts/ink are supplied by the host.
    FillTextBox {
        text: Text,
        position: Point,
        color: Color,
    },
    /// Actual host-supplied glyph outlines, stroked once as a local triangle union.
    /// Uses the same top-left text box and content clip as FillTextBox.
    StrokeTextBox {
        text: Text,
        position: Point,
        color: Color,
        options: CanvasStrokeOptions,
    },
}
#[derive(Clone)]
struct State {
    transform: Affine,
    opacity: f32,
    clips: Vec<Clip>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            transform: Affine::IDENTITY,
            opacity: 1.,
            clips: Vec::new(),
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
struct Compilation {
    drawing: Option<(DomainSource, ImageInputs)>,
    current_path: CurrentPath,
    strokes: Vec<stroke_text::PendingStroke>,
    budget: Budget,
}
fn source(
    commands: &[CanvasCommand],
    limits: CompositeLimits,
    inputs: &ImageInputs,
    smooth: bool,
    current_path: &CurrentPath,
    retained_paths: &[Arc<super::PathBudgetScope>],
) -> Result<Compilation, CompositeError> {
    if commands.len() > MAX_CANVAS_COMMANDS {
        return Err(CompositeError::InputLimit);
    }
    let mut state = State::default();
    let mut stack = Vec::new();
    let mut children = Vec::new();
    let mut draws = 0usize;
    let mut budget = Budget::default();
    budget.retained_path_bytes(current_path.byte_len(), limits)?;
    for (index, scope) in retained_paths.iter().enumerate() {
        let path = &scope.baseline;
        if path.additional_bytes(current_path) != 0
            && retained_paths[..index]
                .iter()
                .all(|earlier| path.additional_bytes(&earlier.baseline) != 0)
        {
            budget.retained_path_bytes(path.byte_len(), limits)?;
        }
    }
    let mut current_path = current_path.clone();
    let reserve = commands.len().saturating_mul(2);
    let mut changed_path = false;
    let mut images = ImageInputs::default();
    let mut text_bytes = 0usize;
    let mut strokes = Vec::new();
    for command in commands {
        if current_path.command(command, state.transform, reserve, &mut budget, limits)? {
            changed_path = true;
            continue;
        }
        if let CanvasCommand::FillPattern { path, pattern, .. }
        | CanvasCommand::StrokePattern { path, pattern, .. } = command
        {
            let (rule, stroke) = match command {
                CanvasCommand::FillPattern { rule, .. } => (*rule, None),
                CanvasCommand::StrokePattern { options, .. } => (FillRule::NonZero, Some(options)),
                _ => unreachable!(),
            };
            draws += 1;
            children.push(
                pattern::Compiler {
                    images: &mut images,
                    budget: &mut budget,
                    smooth,
                    limits,
                }
                .compile(
                    path,
                    pattern,
                    rule,
                    stroke,
                    &current_path,
                    &state,
                    draws,
                )?,
            );
            continue;
        }
        let mut path_input_admitted = false;
        let (path, style) = match command {
            CanvasCommand::StrokeTextBox {
                text,
                position,
                color,
                options,
            } => {
                draws += 1;
                let (node, pending) = stroke_text::compile(
                    text,
                    *position,
                    *color,
                    options,
                    &state,
                    &mut budget,
                    &mut text_bytes,
                    draws,
                    limits,
                )?;
                children.push(node);
                strokes.push(pending);
                continue;
            }
            CanvasCommand::FillTextBox {
                text,
                position,
                color,
            } => {
                draws += 1;
                children.push(text::compile(
                    text,
                    *position,
                    *color,
                    &state,
                    &mut budget,
                    &mut text_bytes,
                    draws,
                    limits,
                )?);
                continue;
            }
            CanvasCommand::DrawImage {
                image,
                source,
                destination,
            } => {
                draws += 1;
                children.push(
                    image::Compiler {
                        inputs,
                        images: &mut images,
                        budget: &mut budget,
                        smooth,
                        limits,
                    }
                    .compile(image, *source, *destination, &state, draws)?,
                );
                continue;
            }
            CanvasCommand::Save => {
                if stack.len() >= MAX_CANVAS_STATE_DEPTH.min(limits.max_depth) {
                    return Err(CompositeError::NestingLimit);
                }
                stack.push(state.clone());
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
            CanvasCommand::ClipCurrentPath { rule } => {
                budget.input(current_path.path(), limits)?;
                if state.clips.len() + 2 > limits.max_depth
                    || state.clips.len() >= MAX_CANVAS_STATE_DEPTH
                {
                    return Err(CompositeError::NestingLimit);
                }
                current_path
                    .path()
                    .validate()
                    .map_err(CompositeError::InvalidGeometry)?;
                state.clips.push(Clip {
                    path: current_path.snapshot(),
                    rule: *rule,
                    transform: Affine::IDENTITY,
                });
                continue;
            }
            CanvasCommand::FillCurrentPath { paint, rule } => {
                paint_budget(paint)?;
                budget.input(current_path.path(), limits)?;
                path_input_admitted = true;
                (
                    Cow::Owned(current_path.local_snapshot(
                        state.transform,
                        &mut budget,
                        limits,
                    )?),
                    Style {
                        fill: Some(paint.clone()),
                        fill_rule: *rule,
                        opacity: state.opacity,
                        ..Default::default()
                    },
                )
            }
            CanvasCommand::StrokeCurrentPath { stroke } => {
                if stroke.alignment != StrokeAlignment::Center
                    || stroke.dash_pattern.len() > gpui_flow_scene::MAX_STROKE_DASH_ENTRIES
                {
                    return Err(CompositeError::InputLimit);
                }
                paint_budget(&stroke.paint)?;
                budget.input(current_path.path(), limits)?;
                path_input_admitted = true;
                (
                    Cow::Owned(current_path.local_snapshot(
                        state.transform,
                        &mut budget,
                        limits,
                    )?),
                    Style {
                        fill: None,
                        stroke: Some(stroke.clone()),
                        opacity: state.opacity,
                        ..Default::default()
                    },
                )
            }
            CanvasCommand::FillPattern { .. } | CanvasCommand::StrokePattern { .. } => {
                unreachable!("patterns handled above")
            }
            CanvasCommand::BeginPath
            | CanvasCommand::MoveTo { .. }
            | CanvasCommand::LineTo { .. }
            | CanvasCommand::QuadraticCurveTo { .. }
            | CanvasCommand::BezierCurveTo { .. }
            | CanvasCommand::ClosePath => unreachable!("path construction handled above"),
            CanvasCommand::ClipPath { path, rule } => {
                budget.input(path, limits)?;
                if state.clips.len() + 2 > limits.max_depth
                    || state.clips.len() >= MAX_CANVAS_STATE_DEPTH
                {
                    return Err(CompositeError::NestingLimit);
                }
                budget.path_allocation(path.commands.len(), limits)?;
                path.validate().map_err(CompositeError::InvalidGeometry)?;
                state.clips.push(Clip {
                    path: Arc::new(path.clone()),
                    rule: *rule,
                    transform: state.transform,
                });
                continue;
            }
            CanvasCommand::FillPath { path, paint, rule } => {
                paint_budget(paint)?;
                (
                    Cow::Borrowed(path),
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
                    Cow::Borrowed(path),
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
        if !path_input_admitted {
            budget.input(&path, limits)?;
        }
        budget.draw(path.commands.len(), &state.clips, limits)?;
        let object = DisplayObject::new(format!("canvas-command-{draws}"), path.into_owned())
            .with_transform(state.transform)
            .with_style(style);
        object.validate().map_err(CompositeError::InvalidGeometry)?;
        // Compile directly to the existing domain IR: repeated Scene::insert
        // would clone/revalidate all preceding paths for every command.
        children.push(clip::wrap(
            DomainNode {
                stroke_scale: gpui_flow_scene::StrokeScaleData::IDENTITY,
                object,
                world_transform: state.transform,
                children: vec![],
            },
            &state.clips,
            draws,
        ));
    }
    if !stack.is_empty() {
        return Err(CompositeError::InvalidGeometry(
            "unbalanced Canvas save stack".into(),
        ));
    }
    if changed_path {
        current_path
            .path()
            .validate()
            .map_err(CompositeError::InvalidGeometry)?;
    }
    if draws == 0 {
        return Ok(Compilation {
            drawing: None,
            current_path,
            strokes,
            budget,
        });
    }
    Ok(Compilation {
        current_path,
        strokes,
        budget,
        drawing: Some((
            DomainSource {
                root: DomainNode {
                    stroke_scale: gpui_flow_scene::StrokeScaleData::IDENTITY,
                    object: DisplayObject::new("canvas-command-root", Group::new()),
                    world_transform: Affine::IDENTITY,
                    children,
                },
                clips: vec![],
                canvas: None,
                lowered_geometry: None,
                resolved_camera: None,
                logical_camera: None,
            },
            images,
        )),
    })
}
impl CanvasSurface {
    /// Render an ordered drawing batch and publish at most one immutable generation.
    /// Any invalid command, stack, geometry or budget leaves all pixels untouched.
    /// Returns whether a generation was published (a clipped transparent result
    /// can still publish). Coordinates are logical;
    /// the existing surface DPR mapping is applied once after the command state.
    /// Current geometry persists; transforms, opacity, clips and the state stack
    /// remain batch-local. Geometry-only success does not publish pixels.
    pub fn draw_commands(&mut self, commands: &[CanvasCommand]) -> Result<bool, CompositeError> {
        self.draw_commands_with_images(commands, &ImageInputs::default())
    }
    /// Atomic vector/pattern/image batch using host-supplied immutable decoded inputs.
    /// Input keys are scoped to this call; only referenced buffers are retained
    /// and charged, with shared buffers counted once by the compositor.
    pub fn draw_commands_with_images(
        &mut self,
        commands: &[CanvasCommand],
        inputs: &ImageInputs,
    ) -> Result<bool, CompositeError> {
        self.draw_commands_with_text(commands, inputs, |_| Ok(CanvasTextOutput::default()))
    }
    /// Invoke an explicit host font producer after command admission and before
    /// pixel mutation. Raster work and glyph buffers share composition budgets.
    pub fn draw_commands_with_text(
        &mut self,
        commands: &[CanvasCommand],
        inputs: &ImageInputs,
        prepare: impl FnOnce(CanvasTextRequest<'_>) -> Result<CanvasTextOutput, CompositeError>,
    ) -> Result<bool, CompositeError> {
        if commands
            .iter()
            .any(|command| matches!(command, CanvasCommand::StrokeTextBox { .. }))
        {
            return Err(CompositeError::UnsupportedTextStyle(
                "canvas-stroke-text".into(),
            ));
        }
        self.draw_commands_with_fonts(commands, inputs, |request| {
            Ok(CanvasFontOutput {
                fills: prepare(request.fills)?,
                strokes: vec![],
                outline_work: 0,
            })
        })
    }
    /// Prepare raster fill text and real glyph outline paths in one bounded batch.
    pub fn draw_commands_with_fonts(
        &mut self,
        commands: &[CanvasCommand],
        inputs: &ImageInputs,
        prepare: impl FnOnce(CanvasFontRequest<'_>) -> Result<CanvasFontOutput, CompositeError>,
    ) -> Result<bool, CompositeError> {
        self.draw_commands_with_fonts_guarded(
            commands,
            inputs,
            &AtomicBool::new(false),
            prepare,
            || Ok(()),
        )
    }
    /// Guarded publication. The validator runs before host preparation, before
    /// rasterization, and immediately before pixels, generation and current path
    /// become current together. Cancellation is checked at each boundary and by
    /// the rasterizer. This is a commit validation point, not a synchronization
    /// primitive for concurrent changes after the validator returns.
    pub fn draw_commands_with_fonts_guarded(
        &mut self,
        commands: &[CanvasCommand],
        inputs: &ImageInputs,
        cancelled: &AtomicBool,
        prepare: impl FnOnce(CanvasFontRequest<'_>) -> Result<CanvasFontOutput, CompositeError>,
        validate: impl Fn() -> Result<(), CompositeError>,
    ) -> Result<bool, CompositeError> {
        let check = || {
            if cancelled.load(Ordering::Acquire) {
                return Err(CompositeError::Superseded);
            }
            validate()?;
            if cancelled.load(Ordering::Acquire) {
                return Err(CompositeError::Superseded);
            }
            Ok(())
        };
        let scopes: Vec<_> = self
            .path_budget_scopes
            .iter()
            .filter_map(std::sync::Weak::upgrade)
            .collect();
        let mut limits = self.limits;
        for scope in &scopes {
            limits.max_path_bytes = limits.max_path_bytes.min(scope.max_bytes);
        }
        let mut compiled = source(
            commands,
            limits,
            inputs,
            self.config.smooth,
            &self.current_path,
            &scopes,
        )?;
        check()?;
        let Some((mut source, images)) = compiled.drawing else {
            check()?;
            self.current_path = compiled.current_path;
            return Ok(false);
        };
        let jobs: Vec<_> = compiled
            .strokes
            .iter()
            .map(|pending| pending.job.clone())
            .collect();
        let output = prepare(CanvasFontRequest {
            fills: CanvasTextRequest {
                source: &source,
                viewport: self.raster_viewport(),
                limits,
                retained_bytes: self
                    .byte_len()
                    .saturating_mul(2)
                    .saturating_add(images.byte_len()),
            },
            strokes: &jobs,
            max_outline_commands: compiled.budget.remaining_points(limits),
            max_outline_path_bytes: compiled.budget.remaining_path_bytes(limits),
        })?;
        stroke_text::lower(
            &mut source,
            compiled.strokes,
            output.strokes,
            &mut compiled.budget,
            limits,
        )?;
        check()?;
        // A private cheap clone receives every intermediate write. A late host
        // epoch change or cancellation cannot alter the live context or pixels.
        let mut candidate = self.clone();
        candidate.draw_with_preparation_work_guarded(
            &source,
            Affine::IDENTITY,
            &images,
            &output.fills.inputs,
            output
                .fills
                .pixel_work
                .checked_add(output.outline_work)
                .ok_or(CompositeError::WorkLimit)?,
            cancelled,
            limits,
        )?;
        candidate.current_path = compiled.current_path;
        check()?;
        let published = candidate.generation != self.generation;
        self.pixels = candidate.pixels;
        self.generation = candidate.generation;
        self.current_path = candidate.current_path;
        Ok(published)
    }
}

/// Domain IDs are batch-local; never cache font inputs by ID alone.
pub struct CanvasTextRequest<'a> {
    pub source: &'a DomainSource,
    pub viewport: crate::RasterViewport,
    pub limits: CompositeLimits,
    pub retained_bytes: u64,
}
#[derive(Default)]
pub struct CanvasTextOutput {
    pub inputs: TextInputs,
    /// Actual glyph raster writes, charged before compositing can publish.
    pub pixel_work: u64,
}

/// Canvas-only host font work. Generic scene Text and export contracts stay unchanged.
pub struct CanvasFontRequest<'a> {
    pub fills: CanvasTextRequest<'a>,
    pub strokes: &'a [CanvasStrokeTextJob],
    /// Shared remaining allowance for all returned Path command counts.
    pub max_outline_commands: usize,
    /// Shared remaining allowance for all returned Path vector capacities.
    /// The host must also account its shaping/conversion scratch before allocation.
    pub max_outline_path_bytes: u64,
}
#[derive(Clone, Debug)]
pub struct CanvasStrokeTextJob {
    pub id: DisplayId,
    pub text: Text,
    pub world_transform: Affine,
}
#[derive(Default)]
pub struct CanvasFontOutput {
    pub fills: CanvasTextOutput,
    pub strokes: Vec<CanvasStrokeTextOutput>,
    /// Outline extraction, validation and conversion work, shared by the whole
    /// batch and charged together with fill raster writes before publication.
    pub outline_work: u64,
}
/// Exactly one result per job, including empty text. All glyph contours belong
/// to one path in text-box local logical coordinates, before world/DPR mapping.
pub struct CanvasStrokeTextOutput {
    pub id: DisplayId,
    pub path: Path,
}

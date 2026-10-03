//! Only compiler-owned Canvas placeholders may become outline-stroked paths.
use super::{CanvasStrokeOptions, CanvasStrokeTextJob, CanvasStrokeTextOutput, State, clip, text};
use crate::{CompositeError, CompositeLimits, DomainNode, DomainSource};
use gpui_flow_scene::{
    Color, DisplayObject, FillRule, Point, Shape, StrokeGeometryBudget, Style,
    display::{Group, Path, PathCommand, Text, TextDecoration},
};
use std::sync::Arc;

pub(super) struct PendingStroke {
    pub job: CanvasStrokeTextJob,
    placeholder: DisplayObject,
    style: Style,
}
#[allow(clippy::too_many_arguments)]
pub(super) fn compile(
    text_value: &Text,
    position: Point,
    color: Color,
    options: &CanvasStrokeOptions,
    state: &State,
    budget: &mut clip::Budget,
    bytes: &mut usize,
    index: usize,
    limits: CompositeLimits,
) -> Result<(DomainNode, PendingStroke), CompositeError> {
    let world = text::admit(text_value, position, state, bytes)?;
    let id = format!("canvas-stroke-text-{index}");
    if text_value.decoration != TextDecoration::None {
        return Err(CompositeError::UnsupportedTextStyle(id.into()));
    }
    if options.dash_pattern.len() > gpui_flow_scene::MAX_STROKE_DASH_ENTRIES {
        return Err(CompositeError::InputLimit);
    }
    // Pending and lowered styles own the dash array; validate-only extrusion
    // also briefly materializes its even f64 dash pattern.
    budget.retained_path_bytes(
        (options.dash_pattern.len() as u64)
            .saturating_mul(2 * size_of::<f32>() as u64 + 4 * size_of::<f64>() as u64),
        limits,
    )?;
    let mut stroke = options.stroke()?;
    stroke.paint = color.into();
    // The scene stroke validator is private; an empty vector validates the
    // options without touching any supplied glyph geometry.
    Shape::Path(Path::default())
        .stroke_mesh_with_rule(&stroke, FillRule::NonZero)
        .map_err(CompositeError::InvalidGeometry)?;
    let bounds = text_value.content_bounds();
    let clip_path = Path::new(vec![
        PathCommand::MoveTo { to: bounds.origin },
        PathCommand::LineTo {
            to: Point::new(bounds.max_x(), bounds.min_y()),
        },
        PathCommand::LineTo {
            to: Point::new(bounds.max_x(), bounds.max_y()),
        },
        PathCommand::LineTo {
            to: Point::new(bounds.min_x(), bounds.max_y()),
        },
        PathCommand::Close,
    ]);
    let mut clips = state.clips.clone();
    clips.push(clip::Clip {
        path: Arc::new(clip_path),
        rule: FillRule::NonZero,
        transform: world,
    });
    budget.draw(0, &clips, limits)?;
    let placeholder = DisplayObject::new(id, Group::new()).with_transform(world);
    let pending = PendingStroke {
        job: CanvasStrokeTextJob {
            id: placeholder.id.clone(),
            text: text_value.clone(),
            world_transform: world,
        },
        placeholder: placeholder.clone(),
        style: Style {
            fill: None,
            stroke: Some(stroke),
            opacity: state.opacity,
            ..Default::default()
        },
    };
    Ok((
        clip::wrap(
            DomainNode {
                object: placeholder,
                world_transform: world,
                children: vec![],
            },
            &clips,
            index,
        ),
        pending,
    ))
}

pub(super) fn lower(
    source: &mut DomainSource,
    pending: Vec<PendingStroke>,
    mut output: Vec<CanvasStrokeTextOutput>,
    budget: &mut clip::Budget,
    limits: CompositeLimits,
) -> Result<(), CompositeError> {
    if pending.len() != output.len() {
        return Err(mismatch());
    }
    if pending.is_empty() {
        return Ok(());
    }
    // Admission covers every result, including hidden, empty and repeated-ID
    // responses, before geometry preparation can allocate generated outlines.
    for result in &output {
        if result.id.0.len() > 512 {
            return Err(CompositeError::InputLimit);
        }
        budget.lowered_path(&result.path, limits)?;
        result
            .path
            .validate()
            .map_err(CompositeError::InvalidGeometry)?;
        for command in &result.path.commands {
            let points: &[Point] = match command {
                PathCommand::MoveTo { to } | PathCommand::LineTo { to } => std::slice::from_ref(to),
                PathCommand::QuadraticTo { control, to } => {
                    if !bounded(*control) || !bounded(*to) {
                        return Err(mismatch());
                    }
                    &[]
                }
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    if !bounded(*control1) || !bounded(*control2) || !bounded(*to) {
                        return Err(mismatch());
                    }
                    &[]
                }
                PathCommand::Close => &[],
            };
            if points.iter().any(|p| !bounded(*p)) {
                return Err(mismatch());
            }
        }
    }
    for expected in pending {
        let matches: Vec<_> = output
            .iter()
            .enumerate()
            .filter(|(_, result)| result.id == expected.job.id)
            .map(|(index, _)| index)
            .collect();
        if matches.len() != 1 {
            return Err(mismatch());
        }
        let result = output.swap_remove(matches[0]);
        let mut found = 0;
        replace(
            &mut source.root,
            &expected,
            &mut Some(result.path),
            &mut found,
        )?;
        if found != 1 {
            return Err(mismatch());
        }
    }
    if !output.is_empty() {
        return Err(mismatch());
    }
    source.lowered_geometry = Some(StrokeGeometryBudget::new(
        limits.max_points,
        budget.remaining_points(limits),
        limits.max_triangles,
        budget.remaining_path_bytes(limits),
    ));
    Ok(())
}
fn replace(
    node: &mut DomainNode,
    expected: &PendingStroke,
    path: &mut Option<Path>,
    found: &mut usize,
) -> Result<(), CompositeError> {
    if node.object.id == expected.job.id {
        if node.object != expected.placeholder
            || !node.children.is_empty()
            || node.world_transform != expected.job.world_transform
        {
            return Err(mismatch());
        }
        node.object.shape = Shape::Path(path.take().ok_or_else(mismatch)?);
        node.object.style = expected.style.clone();
        *found += 1;
    }
    for child in &mut node.children {
        replace(child, expected, path, found)?;
    }
    Ok(())
}
fn bounded(point: Point) -> bool {
    point.is_finite() && point.x.abs().max(point.y.abs()) <= 1_000_000.
}
fn mismatch() -> CompositeError {
    CompositeError::InvalidGeometry(
        "Canvas stroke text outline/job mismatch or invalid geometry".into(),
    )
}

//! The same-App shaped producer returns pre-DPR raster-logical glyph geometry.
//! Undo only its isotropic raster matrix; the Canvas compositor owns world/DPR.
use super::*;
use gpui_flow_scene::{
    DisplayId,
    display::{Path, PathCommand, Text},
};
use gpui_flow_text::{
    OutlinedText, TextOutlineError,
    gpui_outlines::{charge_outline_budget, release_outline_bytes},
    outline_registration::OutlineBindingError,
    static_outlines::{OutlineBudget, OutlineCommand, OutlineError},
};
use std::mem::size_of;

pub(super) fn prepare(
    request: &CanvasFontRequest<'_>,
    backend: &TextBackendSnapshot,
    owner: &Arc<gpui::TextSystem>,
    cancelled: &AtomicBool,
) -> Result<PreparedStrokes, CompositeError> {
    let limits = request.fills.limits.bounded();
    let mut budget = batch_budget(request);
    let initial = budget.remaining();
    let first = &request.strokes[0].id;
    let mut outputs =
        reserved_vec::<CanvasStrokeTextOutput>(request.strokes.len(), &mut budget, first)?;
    for job in request.strokes {
        check(Some(backend), cancelled)?;
        let content = job.text.content_bounds();
        // Do not turn an invalid/empty content box into successful identity-free
        // output. Empty *text* still goes through the real outline producer.
        if !content.is_valid() || !content.size.is_positive() {
            return Err(CompositeError::TextRasterization {
                text: job.id.clone(),
                reason: "Canvas stroke text requires a finite positive content box".into(),
            });
        }
        let space = transform::space(
            request.fills.viewport.world_to_screen * job.world_transform,
            content,
            request.fills.viewport,
            true,
        )?
        .ok_or(CompositeError::UnsupportedTransform)?;
        let raster_request = request_for(
            &job.text,
            &space,
            request.fills.viewport.device_scale,
            &mut budget,
            &job.id,
        )?;
        let request_bytes =
            raster_request.text.content.capacity() + raster_request.text.font_family.capacity();
        let before = budget.remaining();
        let mut job_budget = OutlineBudget::new(
            before[0],
            before[1].min(limits.max_points),
            before[2],
            before[3],
        );
        let job_commands = job_budget.remaining()[1];
        let outlined = backend
            .bridge
            .outline_text(
                owner,
                &raster_request,
                backend.epoch,
                cancelled,
                &mut job_budget,
            )
            .map_err(|error| map_error(&job.id, error))?;
        let after = job_budget.remaining();
        budget = OutlineBudget::new(
            after[0],
            before[1] - (job_commands - after[1]),
            after[2],
            after[3],
        );
        // The request copies are no longer live during command conversion.
        drop(raster_request);
        release_outline_bytes(&mut budget, request_bytes);
        let path = local_path(
            outlined,
            space.matrix,
            &mut budget,
            &job.id,
            limits.max_points,
            || check(Some(backend), cancelled),
        )?;
        let id = DisplayId(copy_string(&job.id.0, &mut |bytes, work| {
            charge(&mut budget, bytes, work, &job.id)
        })?);
        outputs.push(CanvasStrokeTextOutput { id, path });
    }
    check(Some(backend), cancelled)?;
    Ok(PreparedStrokes {
        outputs,
        retained_bytes: (initial[2] - budget.remaining()[2]) as u64,
        work: (initial[3] - budget.remaining()[3]) as u64,
    })
}

fn batch_budget(request: &CanvasFontRequest<'_>) -> OutlineBudget {
    let limits = request.fills.limits.bounded();
    let bytes = request
        .max_outline_path_bytes
        .min(limits.max_path_bytes)
        .min(
            limits
                .max_surface_bytes
                .saturating_sub(request.fills.retained_bytes),
        )
        .min(32 * 1024 * 1024);
    OutlineBudget::new(
        8192,
        request.max_outline_commands.min(limits.max_total_points),
        usize::try_from(bytes).unwrap_or(usize::MAX),
        usize::try_from(limits.max_surface_writes).unwrap_or(usize::MAX),
    )
}

fn request_for(
    text: &Text,
    space: &transform::RasterSpace,
    device_scale: f32,
    budget: &mut OutlineBudget,
    id: &DisplayId,
) -> Result<TextRasterRequest, CompositeError> {
    let m = space.matrix;
    let content = text.content_bounds();
    let mut reserve = |bytes, work| charge(budget, bytes, work, id);
    Ok(TextRasterRequest {
        text: TextMeasureRequest {
            content: copy_string(&text.content, &mut reserve)?,
            font_family: copy_string(&text.font_family, &mut reserve)?,
            font_size: text.font_size * m.d,
            font_weight: text.font_weight,
            italic: text.italic,
            text_case: crate::text_measure::text_case(text.text_case),
            para_spacing: text.para_spacing * m.d,
            letter_spacing: text.letter_spacing * m.d,
            word_spacing: text.word_spacing * m.d,
            para_indent: text.para_indent * m.d,
            line_height: text.font_size * text.line_height * m.d,
            wrap_width: text.wrap.then_some(content.size.width * m.a),
        },
        overflow: crate::text_measure::text_overflow(text.overflow),
        decoration: crate::text_measure::text_decoration(text.decoration),
        vertical_align: crate::text_measure::vertical_align(text.vertical_align),
        align: match text.align {
            TextAlign::Start => gpui::TextAlign::Left,
            TextAlign::Center => gpui::TextAlign::Center,
            TextAlign::End => gpui::TextAlign::Right,
        },
        bounds: [
            m.tx + content.origin.x * m.a,
            m.ty + content.origin.y * m.d,
            content.size.width * m.a,
            content.size.height * m.d,
        ],
        clip: [
            space.clip.origin.x,
            space.clip.origin.y,
            space.clip.size.width,
            space.clip.size.height,
        ],
        device_scale,
        color: [0, 0, 0, 255],
    })
}

fn reserved_vec<T>(
    count: usize,
    budget: &mut OutlineBudget,
    id: &DisplayId,
) -> Result<Vec<T>, CompositeError> {
    let bytes = count
        .checked_mul(size_of::<T>())
        .ok_or(CompositeError::WorkingSetLimit)?;
    charge(budget, bytes, 0, id)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| CompositeError::AllocationFailed)?;
    let actual = output
        .capacity()
        .checked_mul(size_of::<T>())
        .ok_or(CompositeError::WorkingSetLimit)?;
    charge(budget, actual - bytes, 0, id)?;
    Ok(output)
}

fn local_path(
    outlined: OutlinedText,
    raster: Affine,
    budget: &mut OutlineBudget,
    id: &DisplayId,
    max_points: usize,
    validate: impl Fn() -> Result<(), CompositeError>,
) -> Result<Path, CompositeError> {
    let mut pending = budget.clone();
    charge(&mut pending, 0, outlined.glyphs.len(), id)?;
    let count = outlined
        .glyphs
        .iter()
        .try_fold(0usize, |count, glyph| {
            count.checked_add(glyph.commands.len())
        })
        .ok_or(CompositeError::InputLimit)?;
    if count > max_points {
        return Err(CompositeError::InputLimit);
    }
    // Both the original glyph vectors and the final concatenated vector are
    // simultaneously live. Never release the former before conversion finishes.
    charge(
        &mut pending,
        0,
        count.checked_mul(8).ok_or(CompositeError::WorkLimit)?,
        id,
    )?;
    let mut commands = reserved_vec::<PathCommand>(count, &mut pending, id)?;
    for glyph in &outlined.glyphs {
        for command in &glyph.commands {
            validate()?;
            commands.push(local_command(*command, raster)?);
        }
    }
    let bytes = outlined.allocated_bytes;
    drop(outlined);
    release_outline_bytes(&mut pending, bytes);
    validate()?;
    *budget = pending;
    Ok(Path::new(commands))
}

fn local_command(command: OutlineCommand, m: Affine) -> Result<PathCommand, CompositeError> {
    if !positive_uniform(m) {
        return Err(CompositeError::UnsupportedTransform);
    }
    let point = |p: [f32; 2]| {
        let p = Point::new((p[0] - m.tx) / m.a, (p[1] - m.ty) / m.d);
        if !p.is_finite() || p.x.abs().max(p.y.abs()) > 1_000_000. {
            Err(CompositeError::InvalidGeometry(
                "Canvas text outline coordinates are out of range".into(),
            ))
        } else {
            Ok(p)
        }
    };
    Ok(match command {
        OutlineCommand::MoveTo(p) => PathCommand::MoveTo { to: point(p)? },
        OutlineCommand::LineTo(p) => PathCommand::LineTo { to: point(p)? },
        OutlineCommand::QuadTo(c, p) => PathCommand::QuadraticTo {
            control: point(c)?,
            to: point(p)?,
        },
        OutlineCommand::CurveTo(c1, c2, p) => PathCommand::CubicTo {
            control1: point(c1)?,
            control2: point(c2)?,
            to: point(p)?,
        },
        OutlineCommand::Close => PathCommand::Close,
    })
}

fn charge(
    budget: &mut OutlineBudget,
    bytes: usize,
    work: usize,
    id: &DisplayId,
) -> Result<(), CompositeError> {
    charge_outline_budget(budget, bytes, work).map_err(|error| map_error(id, error.into()))
}
fn map_error(id: &DisplayId, error: TextOutlineError) -> CompositeError {
    match error {
        TextOutlineError::Binding(OutlineBindingError::StaleSnapshot) => CompositeError::Superseded,
        TextOutlineError::UnsupportedDecoration => CompositeError::UnsupportedTextStyle(id.clone()),
        TextOutlineError::Outline(OutlineError::Allocation) => CompositeError::AllocationFailed,
        TextOutlineError::Outline(OutlineError::Limit("job work" | "glyph or job work")) => {
            CompositeError::WorkLimit
        }
        TextOutlineError::Outline(OutlineError::Limit(
            "job storage" | "line-offset storage" | "outline allocation capacity",
        )) => CompositeError::WorkingSetLimit,
        TextOutlineError::Outline(OutlineError::Limit(_)) => CompositeError::InputLimit,
        error => CompositeError::TextRasterization {
            text: id.clone(),
            reason: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests;

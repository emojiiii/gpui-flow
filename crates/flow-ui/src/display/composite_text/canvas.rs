//! Canvas-only font admission. The ordinary scene/export producer is unchanged.
use super::*;
use gpui_flow_composite::{
    CanvasFontOutput, CanvasFontRequest, CanvasStrokeTextOutput, CanvasTextOutput,
};
use std::sync::atomic::Ordering;

#[cfg(feature = "canvas-stroke-text")]
mod outlines;

#[derive(Default)]
struct PreparedStrokes {
    outputs: Vec<CanvasStrokeTextOutput>,
    retained_bytes: u64,
    work: u64,
}

/// The owner comes from the consuming App, never from a numeric font ID or the
/// bridge itself. Defer its retrieval until a stroke job actually needs it.
pub(crate) fn canvas_font_inputs(
    request: CanvasFontRequest<'_>,
    backend: Option<&TextBackendSnapshot>,
    owner: impl FnOnce() -> Arc<gpui::TextSystem>,
    cancelled: &AtomicBool,
) -> Result<CanvasFontOutput, CompositeError> {
    check(backend, cancelled)?;
    let strokes = if let Some(first) = request.strokes.first() {
        #[cfg(feature = "canvas-stroke-text")]
        {
            let backend =
                backend.ok_or_else(|| CompositeError::TextBackendRequired(first.id.clone()))?;
            outlines::prepare(&request, backend, &owner(), cancelled)?
        }
        #[cfg(not(feature = "canvas-stroke-text"))]
        {
            let _ = owner;
            return Err(CompositeError::TextRasterization {
                text: first.id.clone(),
                reason: "Canvas stroke text requires the canvas-stroke-text feature and a registered static-outline backend".into(),
            });
        }
    } else {
        PreparedStrokes::default()
    };
    let limits = request.fills.limits.bounded();
    let mut source_bytes = strokes.retained_bytes;
    let mut remaining_work = limits
        .max_surface_writes
        .checked_sub(strokes.work)
        .ok_or(CompositeError::WorkLimit)?;
    let mut inputs = TextInputs::default();
    TextPreparation {
        backend,
        viewport: request.fills.viewport,
        cancelled,
        retained: request.fills.retained_bytes,
        source_bytes: &mut source_bytes,
        remaining_work: &mut remaining_work,
        limits,
    }
    .domain_inner(request.fills.source, &mut inputs, true, false);
    check(backend, cancelled)?;
    Ok(CanvasFontOutput {
        fills: CanvasTextOutput {
            inputs,
            pixel_work: limits.max_surface_writes - strokes.work - remaining_work,
        },
        strokes: strokes.outputs,
        outline_work: strokes.work,
    })
}

pub(super) fn check(
    backend: Option<&TextBackendSnapshot>,
    cancelled: &AtomicBool,
) -> Result<(), CompositeError> {
    if cancelled.load(Ordering::Acquire) || backend.is_some_and(|b| !b.current()) {
        Err(CompositeError::Superseded)
    } else {
        Ok(())
    }
}

/// Reserve before allocating, then account any allocator-reported extra capacity
/// before exposing the string. Only the caller's private pending budget mutates.
pub(super) fn copy_string(
    value: &str,
    charge: &mut impl FnMut(usize, usize) -> Result<(), CompositeError>,
) -> Result<String, CompositeError> {
    charge(value.len(), value.len())?;
    let mut result = String::new();
    result
        .try_reserve_exact(value.len())
        .map_err(|_| CompositeError::AllocationFailed)?;
    charge(result.capacity() - value.len(), 0)?;
    result.push_str(value);
    Ok(result)
}

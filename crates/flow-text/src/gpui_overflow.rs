//! Fixed-box end ellipsis using GPUI shaping/wrap boundaries and Unicode graphemes.
use crate::{TextMeasureRequest, TextOverflow, gpui_backend::shape_text_checked};
use gpui::{Hsla, WindowTextSystem, WrappedLine, px};
use unicode_segmentation::UnicodeSegmentation;

/// Same shaped rows for native painting and CPU glyph rasterization.
pub struct TextBoxShape {
    pub lines: Vec<WrappedLine>,
    /// Top of each shaped line relative to the block. An unwrapped GPUI line
    /// represents one hard-newline paragraph; after ellipsis it can instead
    /// represent a visual row, whose original offset must remain unchanged.
    pub line_offsets: Vec<f32>,
    /// Original row height keeps vertically overflowing text top aligned even
    /// when ellipsis removes the hidden rows before paint.
    pub block_height: f32,
    /// Bounded owned native-layout copies retained for spacing/indent.
    pub layout_bytes: usize,
}
/// Reserve all offsets only after admitting their storage beside offsets that
/// are still live. Callers fill at most `count` entries so the Vec never grows.
fn reserve_line_offsets(
    count: usize,
    already_live_bytes: usize,
    admit: &mut impl FnMut(usize) -> Result<(), String>,
) -> Result<Vec<f32>, String> {
    let total_bytes = |capacity: usize| {
        capacity
            .checked_mul(std::mem::size_of::<f32>())
            .and_then(|bytes| already_live_bytes.checked_add(bytes))
            .ok_or_else(|| "text line-offset storage overflow".to_owned())
    };
    let planned_bytes = total_bytes(count)?;
    // In particular, a rejecting quota is consulted before try_reserve_exact,
    // even for a count that the allocator could not possibly represent.
    admit(planned_bytes)?;
    let mut offsets = Vec::new();
    offsets
        .try_reserve_exact(count)
        .map_err(|_| "text line-offset allocation failed")?;
    // Vec exposes actual capacity; do not silently assume reserve_exact's
    // minimum capacity is an exact allocator guarantee.
    let actual_bytes = total_bytes(offsets.capacity())?;
    if actual_bytes != planned_bytes {
        admit(actual_bytes)?;
    }
    Ok(offsets)
}
fn paragraph_layout(
    lines: &[WrappedLine],
    request: &TextMeasureRequest,
    admit: &mut impl FnMut(usize) -> Result<(), String>,
) -> Result<(Vec<f32>, f32), String> {
    let mut offsets = reserve_line_offsets(lines.len(), 0, admit)?;
    let mut rows = 0;
    for (paragraph, line) in lines.iter().enumerate() {
        let top = rows as f32 * request.line_height + paragraph as f32 * request.para_spacing;
        rows += line.wrap_boundaries.len() + 1;
        offsets.push(top);
    }
    Ok((offsets, request.block_height(rows)))
}
fn plain(request: &TextMeasureRequest, content: &str) -> TextMeasureRequest {
    TextMeasureRequest {
        content: content.into(),
        wrap_width: None,
        // These rows already contain presentation text from the original shape.
        text_case: crate::TextCase::None,
        para_spacing: 0.,
        ..request.clone()
    }
}
fn fits_width(line: &WrappedLine, available: f32) -> Result<bool, String> {
    let mut start = 0.;
    for boundary in &line.wrap_boundaries {
        let end = line
            .unwrapped_layout
            .runs
            .get(boundary.run_ix)
            .and_then(|run| run.glyphs.get(boundary.glyph_ix))
            .ok_or("invalid native wrap boundary")?;
        let end = f32::from(end.position.x);
        if end - start > available {
            return Ok(false);
        }
        start = end;
    }
    Ok(f32::from(line.unwrapped_layout.width) - start <= available)
}
fn width(
    system: &WindowTextSystem,
    request: &TextMeasureRequest,
    content: &str,
    color: Hsla,
    live_offset_bytes: usize,
    check: &mut impl FnMut(&[WrappedLine], usize) -> Result<(), String>,
) -> Result<f32, String> {
    let mut copied_bytes = 0;
    let lines = shape_text_checked(system, &plain(request, content), color, &mut |bytes| {
        copied_bytes = bytes;
        check(&[], live_offset_bytes + bytes)
    })?;
    check(&lines, live_offset_bytes + copied_bytes)?;
    Ok(lines
        .iter()
        .map(|l| f32::from(l.size(px(request.line_height)).width))
        .fold(0., f32::max))
}
struct EllipsisRow {
    available: f32,
    force: bool,
    live_offset_bytes: usize,
}
fn ellipsize(
    system: &WindowTextSystem,
    request: &TextMeasureRequest,
    row: &str,
    color: Hsla,
    geometry: EllipsisRow,
    check: &mut impl FnMut(&[WrappedLine], usize) -> Result<(), String>,
) -> Result<String, String> {
    let EllipsisRow {
        available,
        force,
        live_offset_bytes,
    } = geometry;
    if !force && width(system, request, row, color, live_offset_bytes, check)? <= available {
        return Ok(row.into());
    }
    let marker = "...";
    if width(system, request, marker, color, live_offset_bytes, check)? > available {
        return Ok(marker.into());
    }
    let boundaries = std::iter::once(0)
        .chain(row.grapheme_indices(true).map(|(i, g)| i + g.len()))
        .collect::<Vec<_>>();
    let mut low = 0;
    let mut high = boundaries.len();
    // At most 17 probes for the admitted 64KiB source. GPUI measures the whole
    // candidate including marker, kerning and fallback; never sum char widths.
    while low + 1 < high {
        let mid = low + (high - low) / 2;
        let candidate = format!("{}...", &row[..boundaries[mid]]);
        if width(system, request, &candidate, color, live_offset_bytes, check)? <= available {
            low = mid;
        } else {
            high = mid;
        }
    }
    Ok(format!("{}...", &row[..boundaries[low]]))
}
/// End ellipsis is opt-in; Clip preserves the existing unbounded explicit-box
/// native path. Ellipsis admits 64KiB UTF-8 and at most 8192 source glyphs.
pub fn shape_text_in_box(
    system: &WindowTextSystem,
    request: &TextMeasureRequest,
    color: Hsla,
    overflow: TextOverflow,
    box_width: f32,
    box_height: f32,
) -> Result<TextBoxShape, String> {
    shape_text_in_box_checked(
        system,
        request,
        color,
        overflow,
        box_width,
        box_height,
        &mut |_, _| Ok(()),
    )
}

/// The checker sees every actual source/probe/output shape, including hidden
/// rows. `live_offset_bytes` counts the simultaneously live Flow line-offset
/// vector capacities. An empty run slice also requests storage-only admission
/// before either offset Vec is allocated. GPUI-owned shape/font caches have
/// separate host ownership.
pub(crate) fn shape_text_in_box_checked(
    system: &WindowTextSystem,
    request: &TextMeasureRequest,
    color: Hsla,
    overflow: TextOverflow,
    box_width: f32,
    box_height: f32,
    check: &mut impl FnMut(&[WrappedLine], usize) -> Result<(), String>,
) -> Result<TextBoxShape, String> {
    if !box_width.is_finite() || !box_height.is_finite() || box_width <= 0. || box_height <= 0. {
        return Err("invalid text box".into());
    }
    if overflow == TextOverflow::Ellipsis {
        request.validate()?;
    }
    let mut layout_bytes = 0;
    let original = shape_text_checked(system, request, color, &mut |bytes| {
        layout_bytes = bytes;
        check(&[], bytes)
    })?;
    let (line_offsets, block_height) = paragraph_layout(&original, request, &mut |bytes| {
        check(&[], layout_bytes + bytes)
    })?;
    let offset_bytes = line_offsets.capacity() * std::mem::size_of::<f32>() + layout_bytes;
    check(&original, offset_bytes)?;
    if !block_height.is_finite() || line_offsets.iter().any(|offset| !offset.is_finite()) {
        return Err("text line height overflow".into());
    }
    if overflow == TextOverflow::Clip {
        return Ok(TextBoxShape {
            lines: original,
            line_offsets,
            block_height,
            layout_bytes,
        });
    }
    let glyphs: usize = original
        .iter()
        .flat_map(|l| &l.unwrapped_layout.runs)
        .map(|r| r.glyphs.len())
        .sum();
    if glyphs > 8192 {
        return Err("ellipsis source glyph limit (8192)".into());
    }
    let count: usize = original.iter().map(|l| l.wrap_boundaries.len() + 1).sum();
    if count > 8192 {
        return Err("ellipsis source row limit (8192)".into());
    }
    let fits_horizontally = original.iter().try_fold(true, |fits, line| {
        fits_width(line, box_width).map(|row_fits| fits && row_fits)
    })?;
    if block_height <= box_height && fits_horizontally {
        return Ok(TextBoxShape {
            lines: original,
            line_offsets,
            block_height,
            layout_bytes,
        });
    }
    let mut rows = Vec::new();
    for (line, top) in original.iter().zip(&line_offsets) {
        let boundaries = line
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain(std::iter::once(line.text.len()))
            .collect::<Vec<_>>();
        let mut start = 0;
        let mut row_index = 0;
        for wrap in &line.wrap_boundaries {
            let end = line
                .unwrapped_layout
                .runs
                .get(wrap.run_ix)
                .and_then(|r| r.glyphs.get(wrap.glyph_ix))
                .ok_or("invalid native wrap boundary")?
                .index;
            if end < start || end > line.text.len() || boundaries.binary_search(&end).is_err() {
                return Err("unsupported native wrap order".into());
            }
            rows.push((
                &line.text[start..end],
                *top + row_index as f32 * request.line_height,
                if row_index == 0 {
                    request.para_indent
                } else {
                    0.
                },
            ));
            row_index += 1;
            start = end;
        }
        rows.push((
            &line.text[start..],
            *top + row_index as f32 * request.line_height,
            if row_index == 0 {
                request.para_indent
            } else {
                0.
            },
        ));
    }
    let visible = rows
        .iter()
        .take_while(|(_, top, _)| *top as f64 + request.line_height as f64 <= box_height as f64)
        .count()
        .max(1);
    let vertical_overflow = rows.len() > visible;
    let mut clipped = Vec::with_capacity(visible);
    let mut changed = vertical_overflow;
    for (index, (row, _, indent)) in rows.iter().take(visible).enumerate() {
        let mut row_request = request.clone();
        row_request.para_indent = *indent;
        let value = ellipsize(
            system,
            &row_request,
            row,
            color,
            EllipsisRow {
                available: box_width,
                force: vertical_overflow && index + 1 == visible,
                live_offset_bytes: offset_bytes,
            },
            check,
        )?;
        changed |= value != *row;
        clipped.push((value, *indent));
    }
    if !changed {
        return Ok(TextBoxShape {
            lines: original,
            line_offsets,
            block_height,
            layout_bytes,
        });
    }
    let mut lines = Vec::with_capacity(clipped.len());
    let mut output_layout_bytes = 0;
    for (content, indent) in &clipped {
        let mut row_request = plain(request, content);
        row_request.para_indent = *indent;
        let mut row_bytes = 0;
        lines.extend(shape_text_checked(
            system,
            &row_request,
            color,
            &mut |bytes| {
                row_bytes = bytes;
                check(&[], offset_bytes + output_layout_bytes + bytes)
            },
        )?);
        output_layout_bytes += row_bytes;
    }
    let output_glyphs: usize = lines
        .iter()
        .flat_map(|l| &l.unwrapped_layout.runs)
        .map(|r| r.glyphs.len())
        .sum();
    if output_glyphs > 8192 {
        return Err("ellipsis output glyph limit (8192)".into());
    }
    let output_count = visible.min(rows.len());
    let mut output_offsets = reserve_line_offsets(
        output_count,
        offset_bytes + output_layout_bytes,
        &mut |bytes| check(&[], bytes),
    )?;
    for (_, top, _) in rows.iter().take(visible) {
        output_offsets.push(*top);
    }
    check(
        &lines,
        offset_bytes + output_layout_bytes + output_offsets.capacity() * std::mem::size_of::<f32>(),
    )?;
    Ok(TextBoxShape {
        lines,
        line_offsets: output_offsets,
        block_height,
        layout_bytes: output_layout_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::reserve_line_offsets;

    #[test]
    fn line_offset_quota_rejects_before_impossible_allocation() {
        // This count has a checked usize byte length, but exceeds Vec's isize
        // allocation limit. The admission error must win before any allocation.
        let impossible_count = isize::MAX as usize / std::mem::size_of::<f32>() + 1;
        let mut calls = 0;
        let error = reserve_line_offsets(impossible_count, 12, &mut |bytes| {
            calls += 1;
            assert!(bytes > isize::MAX as usize);
            Err("rejected before allocation".into())
        })
        .unwrap_err();
        assert_eq!(error, "rejected before allocation");
        assert_eq!(calls, 1);
    }

    #[test]
    fn line_offset_admission_includes_both_live_vectors_and_checked_sizes() {
        let mut original = reserve_line_offsets(3, 0, &mut |bytes| {
            assert_eq!(bytes, 12);
            Ok(())
        })
        .unwrap();
        original.extend([0., 24., 48.]);
        let live_bytes = original.capacity() * std::mem::size_of::<f32>();
        let error = reserve_line_offsets(1, live_bytes, &mut |bytes| {
            assert_eq!(bytes, live_bytes + 4);
            Err("combined offset quota".into())
        })
        .unwrap_err();
        assert_eq!(error, "combined offset quota");
        assert_eq!(original, [0., 24., 48.]);
        for (count, live) in [(usize::MAX, 0), (1, usize::MAX)] {
            let error = reserve_line_offsets(count, live, &mut |_| {
                panic!("overflow must fail before admission or allocation")
            })
            .unwrap_err();
            assert_eq!(error, "text line-offset storage overflow");
        }
    }
}

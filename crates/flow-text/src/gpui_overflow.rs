//! Fixed-box end ellipsis using GPUI shaping/wrap boundaries and Unicode graphemes.
use crate::{TextMeasureRequest, TextOverflow, gpui_backend::shape_text};
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
}
fn paragraph_layout(lines: &[WrappedLine], request: &TextMeasureRequest) -> (Vec<f32>, f32) {
    let mut rows = 0;
    let offsets = lines
        .iter()
        .enumerate()
        .map(|(paragraph, line)| {
            let top = rows as f32 * request.line_height + paragraph as f32 * request.para_spacing;
            rows += line.wrap_boundaries.len() + 1;
            top
        })
        .collect();
    (offsets, request.block_height(rows))
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
fn width(
    system: &WindowTextSystem,
    request: &TextMeasureRequest,
    content: &str,
    color: Hsla,
) -> Result<f32, String> {
    Ok(shape_text(system, &plain(request, content), color)?
        .iter()
        .map(|l| f32::from(l.size(px(request.line_height)).width))
        .fold(0., f32::max))
}
fn ellipsize(
    system: &WindowTextSystem,
    request: &TextMeasureRequest,
    row: &str,
    color: Hsla,
    available: f32,
    force: bool,
) -> Result<String, String> {
    if !force && width(system, request, row, color)? <= available {
        return Ok(row.into());
    }
    let marker = "...";
    if width(system, request, marker, color)? > available {
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
        if width(system, request, &candidate, color)? <= available {
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
    if !box_width.is_finite() || !box_height.is_finite() || box_width <= 0. || box_height <= 0. {
        return Err("invalid text box".into());
    }
    if overflow == TextOverflow::Ellipsis {
        request.validate()?;
    }
    let original = shape_text(system, request, color)?;
    let (line_offsets, block_height) = paragraph_layout(&original, request);
    if !block_height.is_finite() || line_offsets.iter().any(|offset| !offset.is_finite()) {
        return Err("text line height overflow".into());
    }
    if overflow == TextOverflow::Clip {
        return Ok(TextBoxShape {
            lines: original,
            line_offsets,
            block_height,
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
    if block_height <= box_height
        && original
            .iter()
            .all(|l| f32::from(l.size(px(request.line_height)).width) <= box_width)
    {
        return Ok(TextBoxShape {
            lines: original,
            line_offsets,
            block_height,
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
            ));
            row_index += 1;
            start = end;
        }
        rows.push((
            &line.text[start..],
            *top + row_index as f32 * request.line_height,
        ));
    }
    let visible = rows
        .iter()
        .take_while(|(_, top)| *top as f64 + request.line_height as f64 <= box_height as f64)
        .count()
        .max(1);
    let vertical_overflow = rows.len() > visible;
    let mut clipped = Vec::with_capacity(visible);
    let mut changed = vertical_overflow;
    for (index, (row, _)) in rows.iter().take(visible).enumerate() {
        let value = ellipsize(
            system,
            request,
            row,
            color,
            box_width,
            vertical_overflow && index + 1 == visible,
        )?;
        changed |= value != *row;
        clipped.push(value);
    }
    if !changed {
        return Ok(TextBoxShape {
            lines: original,
            line_offsets,
            block_height,
        });
    }
    let lines = shape_text(system, &plain(request, &clipped.join("\n")), color)?;
    let output_glyphs: usize = lines
        .iter()
        .flat_map(|l| &l.unwrapped_layout.runs)
        .map(|r| r.glyphs.len())
        .sum();
    if output_glyphs > 8192 {
        return Err("ellipsis output glyph limit (8192)".into());
    }
    Ok(TextBoxShape {
        lines,
        line_offsets: rows.iter().take(visible).map(|(_, top)| *top).collect(),
        block_height,
    })
}

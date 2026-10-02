//! Fixed-box end ellipsis using GPUI shaping/wrap boundaries and Unicode graphemes.
use crate::{TextMeasureRequest, TextOverflow, gpui_backend::shape_text};
use gpui::{Hsla, WindowTextSystem, WrappedLine, px};
use unicode_segmentation::UnicodeSegmentation;

/// Same shaped rows for native painting and CPU glyph rasterization.
pub struct TextBoxShape {
    pub lines: Vec<WrappedLine>,
    /// Original row height keeps vertically overflowing text top aligned even
    /// when ellipsis removes the hidden rows before paint.
    pub block_height: f32,
}
fn row_height(lines: &[WrappedLine], height: f32) -> f32 {
    lines
        .iter()
        .map(|l| l.wrap_boundaries.len() + 1)
        .sum::<usize>() as f32
        * height
}
fn plain(request: &TextMeasureRequest, content: &str) -> TextMeasureRequest {
    TextMeasureRequest {
        content: content.into(),
        wrap_width: None,
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
    let block_height = row_height(&original, request.line_height);
    if overflow == TextOverflow::Clip {
        return Ok(TextBoxShape {
            lines: original,
            block_height,
        });
    }
    let glyphs: usize = original
        .iter()
        .flat_map(|l| &l.unwrapped_layout.runs)
        .map(|r| r.glyphs.len())
        .sum();
    if !block_height.is_finite() {
        return Err("ellipsis line height overflow".into());
    }
    if glyphs > 8192 {
        return Err("ellipsis source glyph limit (8192)".into());
    }
    let max_rows = ((box_height as f64 / request.line_height as f64)
        .floor()
        .max(1.) as usize)
        .min(8192);
    let count: usize = original.iter().map(|l| l.wrap_boundaries.len() + 1).sum();
    if count > 8192 {
        return Err("ellipsis source row limit (8192)".into());
    }
    if count <= max_rows
        && original
            .iter()
            .all(|l| f32::from(l.size(px(request.line_height)).width) <= box_width)
    {
        return Ok(TextBoxShape {
            lines: original,
            block_height,
        });
    }
    let mut rows = Vec::new();
    for line in &original {
        let boundaries = line
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain(std::iter::once(line.text.len()))
            .collect::<Vec<_>>();
        let mut start = 0;
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
            rows.push(&line.text[start..end]);
            start = end;
        }
        rows.push(&line.text[start..]);
    }
    let vertical_overflow = rows.len() > max_rows;
    let visible = rows.len().min(max_rows);
    let mut clipped = Vec::with_capacity(visible);
    let mut changed = vertical_overflow;
    for (index, row) in rows.iter().take(visible).enumerate() {
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
        block_height,
    })
}

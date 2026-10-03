//! One unsnapped paragraph/wrap/baseline traversal for bitmap and vector consumers.
use crate::{TextRasterRequest, gpui_overflow::TextBoxShape};
use gpui::{FontId, GlyphId, Pixels, TextAlign};

#[derive(Clone, Copy, Debug)]
pub(crate) struct GlyphPlacement {
    pub font_id: FontId,
    pub glyph_id: GlyphId,
    pub font_size: Pixels,
    /// Isotropic raster-logical baseline origin, including the native glyph offset.
    pub origin: [f32; 2],
    pub is_emoji: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PlacementStats {
    pub glyphs: usize,
    pub rows: usize,
}
pub(crate) fn counts(shape: &TextBoxShape) -> Result<PlacementStats, String> {
    if shape.lines.len() != shape.line_offsets.len() {
        return Err("text line offsets disagree with shaped paragraphs".into());
    }
    let mut stats = PlacementStats::default();
    for line in &shape.lines {
        stats.rows = stats
            .rows
            .checked_add(line.wrap_boundaries.len() + 1)
            .ok_or("text row limit (8192)")?;
        for run in &line.unwrapped_layout.runs {
            stats.glyphs = stats
                .glyphs
                .checked_add(run.glyphs.len())
                .ok_or("text glyph limit (8192)")?;
        }
    }
    // Fill historically limits glyphs, but permits empty paragraph rows beyond
    // that limit. The outline consumer applies its own bounded row admission.
    if stats.glyphs > 8192 {
        return Err("text glyph limit (8192)".into());
    }
    Ok(stats)
}
pub(crate) fn visit_glyphs<E: From<String>>(
    shape: &TextBoxShape,
    r: &TextRasterRequest,
    mut visitor: impl FnMut(GlyphPlacement) -> Result<(), E>,
) -> Result<PlacementStats, E> {
    let stats = counts(shape)?;
    let block_y = r.bounds[1] + r.vertical_align.offset(r.bounds[3], shape.block_height);
    for (line, offset) in shape.lines.iter().zip(&shape.line_offsets) {
        let layout = &line.unwrapped_layout;
        let baseline = (r.text.line_height - f32::from(layout.ascent) - f32::from(layout.descent))
            / 2.
            + f32::from(layout.ascent);
        // Validate before indexing; no malformed boundary may silently disappear.
        let mut previous_boundary = None;
        for boundary in &line.wrap_boundaries {
            if layout
                .runs
                .get(boundary.run_ix)
                .and_then(|run| run.glyphs.get(boundary.glyph_ix))
                .is_none()
                || previous_boundary.is_some_and(|previous| previous >= *boundary)
            {
                return Err(String::from("invalid native wrap boundary").into());
            }
            previous_boundary = Some(*boundary);
        }
        let mut wraps = line.wrap_boundaries.iter().peekable();
        let aligned = |start: f32, next: Option<&&gpui::WrapBoundary>| {
            let end = next.map_or(f32::from(layout.width), |w| {
                f32::from(layout.runs[w.run_ix].glyphs[w.glyph_ix].position.x)
            });
            aligned_x(r.bounds[0], r.bounds[2], end - start, r.align)
        };
        let mut x = aligned(0., wraps.peek());
        let mut y = block_y + offset;
        let mut previous_x = 0.;
        for (run_ix, run) in layout.runs.iter().enumerate() {
            for (glyph_ix, glyph) in run.glyphs.iter().enumerate() {
                let glyph_x = f32::from(glyph.position.x);
                x += glyph_x - previous_x;
                if wraps.peek() == Some(&&gpui::WrapBoundary { run_ix, glyph_ix }) {
                    wraps.next();
                    x = aligned(glyph_x, wraps.peek());
                    y += r.text.line_height;
                }
                previous_x = glyph_x;
                let origin = [x, y + baseline + f32::from(glyph.position.y)];
                if origin.iter().any(|v| !v.is_finite())
                    || !f32::from(layout.font_size).is_finite()
                    || f32::from(layout.font_size) <= 0.
                {
                    return Err(String::from("invalid native glyph position").into());
                }
                visitor(GlyphPlacement {
                    font_id: run.font_id,
                    glyph_id: glyph.id,
                    font_size: layout.font_size,
                    origin,
                    is_emoji: glyph.is_emoji,
                })?;
            }
        }
    }
    Ok(stats)
}
pub(crate) fn aligned_x(x: f32, width: f32, line: f32, align: TextAlign) -> f32 {
    match align {
        TextAlign::Left => x,
        TextAlign::Center => (x * 2. + width - line) / 2.,
        TextAlign::Right => x + width - line,
    }
}

//! Shared device-snapped decoration geometry on actual GPUI visual rows.
use crate::{TextDecoration, VerticalAlign, gpui_overflow::TextBoxShape};
use gpui::TextAlign;

pub struct DecorationRequest {
    pub decoration: TextDecoration,
    pub font_size: f32,
    pub line_height: f32,
    pub bounds: [f32; 4],
    pub align: TextAlign,
    pub vertical_align: VerticalAlign,
    pub device_scale: f32,
}
pub struct DecorationLayout {
    /// Screen-logical x/y/width/height, snapped through the requested DPR.
    pub rects: Vec<[f32; 4]>,
    pub allocated_bytes: usize,
}
pub fn layout_decorations(
    shaped: &TextBoxShape,
    r: &DecorationRequest,
    max_bytes: usize,
) -> Result<DecorationLayout, String> {
    use TextDecoration::*;
    if r.decoration == None {
        return Ok(DecorationLayout {
            rects: vec![],
            allocated_bytes: 0,
        });
    }
    if !r.font_size.is_finite()
        || r.font_size <= 0.
        || !r.line_height.is_finite()
        || r.line_height <= 0.
        || !r.device_scale.is_finite()
        || r.device_scale <= 0.
        || r.bounds
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
        || r.bounds[2] <= 0.
        || r.bounds[3] <= 0.
    {
        return Err("invalid text decoration geometry".into());
    }
    let glyphs: usize = shaped
        .lines
        .iter()
        .flat_map(|l| &l.unwrapped_layout.runs)
        .map(|r| r.glyphs.len())
        .sum();
    if glyphs > 8192 {
        return Err("text decoration glyph limit (8192)".into());
    }
    let rows = shaped
        .lines
        .iter()
        .map(|l| l.wrap_boundaries.len() + 1)
        .sum::<usize>();
    if rows > 8192 {
        return Err("text decoration row limit (8192)".into());
    }
    let count = rows
        * if r.decoration == UnderlineStrikethrough {
            2
        } else {
            1
        };
    let allocated_bytes = count * std::mem::size_of::<[f32; 4]>();
    if allocated_bytes > max_bytes {
        return Err("text working-set limit".into());
    }
    let mut rects = Vec::new();
    rects
        .try_reserve_exact(count)
        .map_err(|_| "text decoration allocation")?;
    let block_y = r.bounds[1] + r.vertical_align.offset(r.bounds[3], shaped.block_height);
    let thickness = (r.font_size / 11. * r.device_scale).round().max(1.) / r.device_scale;
    let snap = |v: f32| (v * r.device_scale).round() / r.device_scale;
    if shaped.lines.len() != shaped.line_offsets.len() {
        return Err("text line offsets disagree with shaped paragraphs".into());
    }
    for (line, offset) in shaped.lines.iter().zip(&shaped.line_offsets) {
        let mut y = block_y + offset;
        let layout = &line.unwrapped_layout;
        let baseline = (r.line_height - f32::from(layout.ascent) - f32::from(layout.descent)) * 0.5
            + f32::from(layout.ascent);
        let mut start = 0.;
        // Indentation precedes the first glyph only. It participates in row
        // alignment, but is not decorated; empty indented rows have no ink.
        let mut leading = layout
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .next()
            .map_or(f32::from(layout.width), |glyph| f32::from(glyph.position.x))
            .max(0.);
        let ends = line
            .wrap_boundaries
            .iter()
            .map(|w| {
                line.unwrapped_layout
                    .runs
                    .get(w.run_ix)
                    .and_then(|run| run.glyphs.get(w.glyph_ix))
                    .map(|glyph| f32::from(glyph.position.x))
                    .ok_or("invalid text decoration wrap boundary")
            })
            .chain(std::iter::once(Ok(f32::from(layout.width))));
        for end in ends {
            let end = end?;
            let advance = end - start;
            let width = advance - leading;
            if !width.is_finite() || width < 0. {
                return Err("unsupported text decoration row".into());
            }
            if width > 0. {
                let x = crate::gpui_raster::aligned_x(r.bounds[0], r.bounds[2], advance, r.align)
                    + leading;
                let left = snap(x);
                let right = snap(x + width);
                for offset in [
                    matches!(r.decoration, Underline | UnderlineStrikethrough)
                        .then_some(r.font_size * 0.15),
                    matches!(r.decoration, Strikethrough | UnderlineStrikethrough)
                        .then_some(-r.font_size * 0.35),
                ]
                .into_iter()
                .flatten()
                {
                    let rect = [left, snap(y + baseline + offset), right - left, thickness];
                    if rect.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.) {
                        return Err("unsupported text decoration placement".into());
                    }
                    if rect[2] > 0. {
                        rects.push(rect);
                    }
                }
            }
            start = end;
            leading = 0.;
            y += r.line_height;
        }
    }
    Ok(DecorationLayout {
        rects,
        allocated_bytes,
    })
}

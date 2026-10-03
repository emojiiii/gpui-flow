//! Bounded spacing over native shaping clusters. Cached GPUI layouts stay immutable.
use crate::TextMeasureRequest;
use gpui::{LineLayout, WrapBoundary, WrappedLine, WrappedLineLayout, px};
use std::{mem::size_of, ops::DerefMut, sync::Arc};

/// Conservative admission for the owned copies and their wrap-boundary capacity.
/// GPUI's original cached layouts retain their separate host ownership.
pub(crate) fn storage(lines: &[WrappedLine]) -> Result<usize, String> {
    let mut bytes = 0usize;
    let mut glyphs = 0usize;
    if lines.len() > 8192 {
        return Err("text inline layout row limit (8192)".into());
    }
    for line in lines {
        let layout = &line.unwrapped_layout;
        bytes += size_of::<LineLayout>()
            + size_of::<WrappedLineLayout>()
            + 4 * size_of::<usize>()
            + layout.runs.len() * size_of::<gpui::ShapedRun>();
        for run in &layout.runs {
            glyphs += run.glyphs.len();
            if glyphs > 8192 {
                return Err("text inline layout glyph limit (8192)".into());
            }
            bytes += run.glyphs.len() * size_of::<gpui::ShapedGlyph>();
        }
        // SmallVec may grow geometrically; reserve this upper bound before copying.
        let count: usize = layout.runs.iter().map(|r| r.glyphs.len()).sum();
        bytes += count.max(1).next_power_of_two() * size_of::<WrapBoundary>();
    }
    Ok(bytes)
}

pub(crate) fn apply(lines: &mut [WrappedLine], request: &TextMeasureRequest) -> Result<(), String> {
    for line in lines {
        let original = &line.unwrapped_layout;
        let mut layout = LineLayout {
            font_size: original.font_size,
            width: original.width,
            ascent: original.ascent,
            descent: original.descent,
            runs: original.runs.clone(),
            len: original.len,
        };
        let mut extra = request.para_indent;
        let mut previous = None;
        let mut previous_x = f32::NEG_INFINITY;
        for glyph in layout.runs.iter_mut().flat_map(|r| &mut r.glyphs) {
            let x = f32::from(glyph.position.x);
            if !line.text.is_char_boundary(glyph.index)
                || glyph.index >= line.text.len()
                || previous.is_some_and(|index| index > glyph.index)
                || !x.is_finite()
                || (previous != Some(glyph.index) && x < previous_x)
            {
                return Err("spacing requires monotonic native shaping clusters".into());
            }
            if previous != Some(glyph.index) {
                if let Some(index) = previous {
                    extra += request.letter_spacing;
                    if line.text[index..].starts_with(' ') {
                        extra += request.word_spacing;
                    }
                }
                previous = Some(glyph.index);
            }
            previous_x = previous_x.max(x);
            glyph.position.x += px(extra);
        }
        if let Some(index) = previous {
            extra += request.letter_spacing;
            if line.text[index..].starts_with(' ') {
                extra += request.word_spacing;
            }
        }
        layout.width += px(extra);
        if !f32::from(layout.width).is_finite() {
            return Err("text inline layout width overflow".into());
        }
        let mut wrapped = WrappedLineLayout {
            unwrapped_layout: Arc::new(layout),
            wrap_boundaries: Default::default(),
            wrap_width: request.wrap_width.map(px),
        };
        if let Some(width) = request.wrap_width {
            wrap(&line.text, &mut wrapped, width)?;
        }
        *line.deref_mut() = Arc::new(wrapped);
    }
    Ok(())
}

fn word_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, '\u{00c0}'..='\u{024f}' | '\u{0400}'..='\u{04ff}'
            | '\u{1e00}'..='\u{1eff}' | '\u{0300}'..='\u{036f}' | '\u{0980}'..='\u{09ff}')
        || matches!(
            c,
            '-' | '_'
                | '.'
                | '\''
                | '’'
                | '‘'
                | '$'
                | '%'
                | '@'
                | '#'
                | '^'
                | '~'
                | ','
                | '='
                | ':'
                | ';'
                | '!'
                | ')'
                | ']'
                | '}'
                | '"'
                | '”'
                | '»'
                | '…'
                | '⋯'
                | '\u{202f}'
                | '\u{00a0}'
                | '\u{2011}'
        )
}

/// Same greedy native-cluster wrap rule used by the pinned GPUI LineLayout,
/// with advances after spacing and an indent reserved only in the first row.
fn wrap(text: &str, line: &mut WrappedLineLayout, width: f32) -> Result<(), String> {
    let layout = &line.unwrapped_layout;
    let mut glyphs = layout
        .runs
        .iter()
        .enumerate()
        .flat_map(|(run_ix, run)| {
            run.glyphs
                .iter()
                .enumerate()
                .map(move |(glyph_ix, glyph)| (WrapBoundary { run_ix, glyph_ix }, glyph))
        })
        .peekable();
    let mut first_non_space = false;
    let mut candidate = None;
    let mut row_start = WrapBoundary {
        run_ix: 0,
        glyph_ix: 0,
    };
    let mut row_x = 0.;
    let mut previous = '\0';
    let mut cluster = None;
    let mut cluster_boundary = row_start;
    let mut cluster_x = 0.;
    while let Some((boundary, glyph)) = glyphs.next() {
        let ch = text[glyph.index..]
            .chars()
            .next()
            .ok_or("invalid text cluster")?;
        let x = f32::from(glyph.position.x);
        if cluster != Some(glyph.index) {
            cluster = Some(glyph.index);
            cluster_boundary = boundary;
            cluster_x = x;
            if ch != ' ' && first_non_space && (!word_char(ch) || previous == ' ') {
                candidate = Some((boundary, x));
            }
        }
        first_non_space |= ch != ' ';
        // Never split two glyphs belonging to the same native shaping cluster.
        if glyphs
            .peek()
            .is_some_and(|(_, next)| next.index == glyph.index)
        {
            continue;
        }
        let next_x = glyphs
            .peek()
            .map_or(f32::from(layout.width), |(_, g)| f32::from(g.position.x));
        if next_x - row_x > width && cluster_boundary > row_start {
            let (next, next_x) = candidate
                .take()
                .filter(|(b, _)| *b > row_start)
                .unwrap_or((cluster_boundary, cluster_x));
            line.wrap_boundaries.push(next);
            row_start = next;
            row_x = next_x;
        }
        previous = ch;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{FontId, GlyphId, ShapedGlyph, ShapedRun, point};
    fn request() -> TextMeasureRequest {
        TextMeasureRequest {
            content: "ab".into(),
            font_family: "test".into(),
            font_size: 10.,
            font_weight: 400,
            italic: false,
            line_height: 12.,
            text_case: Default::default(),
            para_spacing: 0.,
            letter_spacing: 2.,
            word_spacing: 0.,
            para_indent: 3.,
            wrap_width: Some(16.),
        }
    }
    fn line(indices: &[usize]) -> WrappedLine {
        let mut line = WrappedLine::default();
        line.text = "ab".into();
        *line.deref_mut() = Arc::new(WrappedLineLayout {
            unwrapped_layout: Arc::new(LineLayout {
                width: px(20.),
                font_size: px(10.),
                len: 2,
                runs: vec![ShapedRun {
                    font_id: FontId(0),
                    glyphs: indices
                        .iter()
                        .enumerate()
                        .map(|(i, index)| ShapedGlyph {
                            id: GlyphId(i as u32),
                            position: point(px(if *index == 0 { 0. } else { 10. }), px(0.)),
                            index: *index,
                            is_emoji: false,
                        })
                        .collect(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        });
        line
    }
    #[test]
    fn inline_layout_wrap_never_splits_a_multi_glyph_cluster() {
        let mut lines = vec![line(&[0, 0, 1])];
        apply(&mut lines, &request()).unwrap();
        assert_eq!(
            lines[0].wrap_boundaries.as_slice(),
            &[WrapBoundary {
                run_ix: 0,
                glyph_ix: 2
            }]
        );
        assert_eq!(f32::from(lines[0].unwrapped_layout.width), 27.);
        assert_eq!(
            lines[0].runs()[0].glyphs[0].position.x,
            lines[0].runs()[0].glyphs[1].position.x
        );
    }
    #[test]
    fn inline_layout_reverse_cluster_order_rejects_and_default_layout_is_unmodified() {
        let mut lines = vec![line(&[1, 0])];
        let before = lines[0].unwrapped_layout.clone();
        assert!(
            apply(&mut lines, &request())
                .unwrap_err()
                .contains("monotonic")
        );
        assert!(Arc::ptr_eq(&before, &lines[0].unwrapped_layout));
        assert_eq!(f32::from(before.width), 20.);
    }
}

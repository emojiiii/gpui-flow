//! Upright cluster layout for Flow's explicit axis modes; no synthetic glyphs.
use crate::gpui_placement::{GlyphPlacement, PlacementStats};
use crate::{
    TextMeasureRequest, TextMetrics, UnsupportedWritingMode, WritingMode, WritingModeError,
};
use gpui::{Hsla, WindowTextSystem, WrappedLine};
use std::mem::size_of;

const MAX_ITEMS: usize = 8192;

#[derive(Clone, Copy, Debug)]
struct Row {
    start: usize,
    end: usize,
    inline: f32,
    cross: f32,
}
#[derive(Clone, Copy)]
struct Cluster {
    start: usize,
    end: usize,
    source_index: usize,
    x: f32,
    width: f32,
}

/// A bounded owned plan of real native glyphs. `wrap_width` in the request is
/// the available INLINE length (height for Y modes); output metrics are physical.
pub struct AxisLayout {
    glyphs: Vec<GlyphPlacement>,
    rows: Vec<Row>,
    mode: WritingMode,
    metrics: TextMetrics,
    allocated_bytes: usize,
}
impl AxisLayout {
    pub fn metrics(&self) -> &TextMetrics {
        &self.metrics
    }
    pub fn allocated_bytes(&self) -> usize {
        self.allocated_bytes
    }
    pub(crate) fn visit<E: From<String>>(
        &self,
        bounds: [f32; 4],
        align: gpui::TextAlign,
        cross_align: crate::VerticalAlign,
        mut visitor: impl FnMut(GlyphPlacement) -> Result<(), E>,
    ) -> Result<PlacementStats, E> {
        let vertical = self.mode.is_vertical();
        let available_inline = bounds[if vertical { 3 } else { 2 }];
        let available_cross = bounds[if vertical { 2 } else { 3 }];
        let block_cross = if vertical {
            self.metrics.width
        } else {
            self.metrics.height
        };
        let cross = cross_align.offset(available_cross, block_cross);
        for row in &self.rows {
            let free = available_inline - row.inline;
            let inline = match (align, self.mode.is_reverse()) {
                (gpui::TextAlign::Center, _) => free * 0.5,
                (gpui::TextAlign::Left, false) | (gpui::TextAlign::Right, true) => 0.,
                _ => free,
            };
            for glyph in &self.glyphs[row.start..row.end] {
                let mut glyph = *glyph;
                glyph.origin[0] += bounds[0] + if vertical { cross } else { inline };
                glyph.origin[1] += bounds[1] + if vertical { inline } else { cross };
                if glyph.origin.iter().any(|n| !n.is_finite()) {
                    return Err(String::from("invalid axis glyph origin").into());
                }
                visitor(glyph)?;
            }
        }
        Ok(PlacementStats {
            glyphs: self.glyphs.len(),
            rows: self.rows.len(),
        })
    }
}

pub fn shape_axis(
    system: &WindowTextSystem,
    request: &TextMeasureRequest,
    mode: WritingMode,
) -> Result<AxisLayout, WritingModeError> {
    shape_axis_checked(system, request, mode, &mut |_| Ok(()))
}

pub(crate) fn shape_axis_checked(
    system: &WindowTextSystem,
    request: &TextMeasureRequest,
    mode: WritingMode,
    admit: &mut impl FnMut(usize) -> Result<(), String>,
) -> Result<AxisLayout, WritingModeError> {
    if request.content.len() > crate::MAX_TEXT_BYTES || request.font_family.len() > 512 {
        return Err("axis shaping allows at most 64KiB UTF-8 content and 512-byte family".into());
    }
    let paragraphs = request.paragraph_breaks() + 1;
    if paragraphs > MAX_ITEMS {
        return Err("text axis layout paragraph limit (8192)".into());
    }
    let presentation_bytes: usize = match request.text_case {
        crate::TextCase::None => request.content.len(),
        crate::TextCase::Upper => request
            .content
            .chars()
            .flat_map(char::to_uppercase)
            .map(char::len_utf8)
            .sum(),
        crate::TextCase::Lower => request
            .content
            .chars()
            .flat_map(char::to_lowercase)
            .map(char::len_utf8)
            .sum(),
    };
    if presentation_bytes > crate::MAX_TEXT_BYTES {
        return Err("case conversion output exceeds 64KiB UTF-8".into());
    }
    // Locked GPUI shape_text retains per-paragraph SharedStrings, one four-slot
    // DecorationRun Vec each, and geometric SmallVec line capacity. Admit those
    // wrappers and both transient content copies before validation/case conversion.
    // Native glyph/font layout caches retain their separate host ownership.
    let source_bytes = request.content.len()
        + 2 * presentation_bytes
        + 2 * request.font_family.len()
        + paragraphs.next_power_of_two() * size_of::<WrappedLine>()
        + paragraphs * (4 * size_of::<gpui::DecorationRun>() + 4 * size_of::<usize>());
    admit(source_bytes)?;
    request.validate()?;
    let mut native = request.clone();
    native.wrap_width = None;
    native.letter_spacing = 0.;
    native.word_spacing = 0.;
    native.para_indent = 0.;
    let lines = crate::gpui_backend::shape_text(system, &native, Hsla::default())?;
    from_lines(&lines, request, mode, source_bytes, admit)
}

fn from_lines(
    lines: &[WrappedLine],
    request: &TextMeasureRequest,
    mode: WritingMode,
    source_bytes: usize,
    admit: &mut impl FnMut(usize) -> Result<(), String>,
) -> Result<AxisLayout, WritingModeError> {
    let count: usize = lines
        .iter()
        .flat_map(|l| &l.unwrapped_layout.runs)
        .map(|r| r.glyphs.len())
        .sum();
    if count > MAX_ITEMS || lines.len() > MAX_ITEMS {
        return Err("text axis layout glyph/paragraph limit (8192)"
            .to_owned()
            .into());
    }
    let rows_capacity = (count + lines.len()).min(MAX_ITEMS);
    let planned = count * (size_of::<GlyphPlacement>() + size_of::<Cluster>())
        + rows_capacity * size_of::<Row>()
        + source_bytes;
    admit(planned)?;
    let mut glyphs = Vec::<GlyphPlacement>::new();
    let mut clusters = Vec::<Cluster>::new();
    let mut rows = Vec::<Row>::new();
    glyphs
        .try_reserve_exact(count)
        .map_err(|_| "text axis allocation failed".to_owned())?;
    clusters
        .try_reserve_exact(count)
        .map_err(|_| "text axis allocation failed".to_owned())?;
    rows.try_reserve_exact(rows_capacity)
        .map_err(|_| "text axis allocation failed".to_owned())?;
    let actual = glyphs.capacity() * size_of::<GlyphPlacement>()
        + clusters.capacity() * size_of::<Cluster>()
        + rows.capacity() * size_of::<Row>()
        + source_bytes;
    if actual != planned {
        admit(actual)?;
    }
    let mut max_inline = 0f32;
    for (paragraph, line) in lines.iter().enumerate() {
        let mut cross =
            rows.len() as f32 * request.line_height + paragraph as f32 * request.para_spacing;
        clusters.clear();
        let layout = &line.unwrapped_layout;
        let baseline = ((if mode.is_vertical() {
            request.font_size
        } else {
            request.line_height
        }) - f32::from(layout.ascent)
            - f32::from(layout.descent))
            * 0.5
            + f32::from(layout.ascent);
        if !baseline.is_finite()
            || !f32::from(layout.width).is_finite()
            || !f32::from(layout.font_size).is_finite()
            || f32::from(layout.font_size) <= 0.
        {
            return Err("invalid native axis metrics".to_owned().into());
        }
        for run in &layout.runs {
            for glyph in &run.glyphs {
                let x = f32::from(glyph.position.x);
                if !line.text.is_char_boundary(glyph.index)
                    || glyph.index >= line.text.len()
                    || !x.is_finite()
                    || !f32::from(glyph.position.y).is_finite()
                {
                    return Err("invalid native axis cluster".to_owned().into());
                }
                if let Some(previous) = clusters.last()
                    && (glyph.index < previous.source_index
                        || (glyph.index != previous.source_index && x < previous.x))
                {
                    return Err(WritingModeError::Unsupported(
                        UnsupportedWritingMode::NativeClusterOrder,
                    ));
                }
                if clusters
                    .last()
                    .is_none_or(|c| c.source_index != glyph.index)
                {
                    if let Some(previous) = clusters.last_mut() {
                        previous.width = x - previous.x;
                    }
                    clusters.push(Cluster {
                        start: glyphs.len(),
                        end: glyphs.len(),
                        source_index: glyph.index,
                        x,
                        width: 0.,
                    });
                }
                glyphs.push(GlyphPlacement {
                    font_id: run.font_id,
                    glyph_id: glyph.id,
                    font_size: layout.font_size,
                    origin: [x, f32::from(glyph.position.y) + baseline],
                    is_emoji: glyph.is_emoji,
                });
                clusters.last_mut().expect("created cluster").end = glyphs.len();
            }
        }
        if let Some(last) = clusters.last_mut() {
            last.width = f32::from(layout.width) - last.x;
            if last.width < 0. {
                return Err(WritingModeError::Unsupported(
                    UnsupportedWritingMode::NativeClusterOrder,
                ));
            }
        }
        let mut row_start = glyphs.len() - clusters.iter().map(|c| c.end - c.start).sum::<usize>();
        let mut row_clusters = 0;
        let mut inline = request.para_indent;
        for cluster in &clusters {
            let extent = if mode.is_vertical() {
                request.font_size
            } else {
                cluster.width
            };
            let advance = extent
                + request.letter_spacing
                + if line.text[cluster.source_index..].starts_with(' ') {
                    request.word_spacing
                } else {
                    0.
                };
            if request
                .wrap_width
                .is_some_and(|limit| inline + advance > limit)
                && row_clusters > 0
            {
                finish_row(
                    &mut rows,
                    &mut glyphs,
                    Row {
                        start: row_start,
                        end: cluster.start,
                        inline,
                        cross,
                    },
                    mode,
                )?;
                max_inline = max_inline.max(inline);
                cross = rows.len() as f32 * request.line_height
                    + paragraph as f32 * request.para_spacing;
                row_start = cluster.start;
                row_clusters = 0;
                inline = 0.;
            }
            for glyph in &mut glyphs[cluster.start..cluster.end] {
                let local_x = glyph.origin[0] - cluster.x;
                if mode.is_vertical() {
                    glyph.origin[0] = cross + (request.line_height - cluster.width) * 0.5 + local_x;
                    glyph.origin[1] += if mode.is_reverse() {
                        -inline - extent
                    } else {
                        inline
                    };
                } else {
                    glyph.origin[0] = local_x
                        + if mode.is_reverse() {
                            -inline - extent
                        } else {
                            inline
                        };
                    glyph.origin[1] += cross;
                }
            }
            inline += advance;
            row_clusters += 1;
        }
        finish_row(
            &mut rows,
            &mut glyphs,
            Row {
                start: row_start,
                end: count_so_far(&clusters, row_start),
                inline,
                cross,
            },
            mode,
        )?;
        max_inline = max_inline.max(inline);
    }
    let cross = request.block_height(rows.len());
    let metrics = TextMetrics {
        width: if mode.is_vertical() {
            cross
        } else {
            max_inline
        },
        height: if mode.is_vertical() {
            max_inline.max(request.font_size)
        } else {
            cross
        },
        line_height: request.line_height,
        lines: rows.len(),
    };
    metrics.validate()?;
    let allocated_bytes =
        glyphs.capacity() * size_of::<GlyphPlacement>() + rows.capacity() * size_of::<Row>();
    Ok(AxisLayout {
        glyphs,
        rows,
        mode,
        metrics,
        allocated_bytes,
    })
}
fn count_so_far(clusters: &[Cluster], empty: usize) -> usize {
    clusters.last().map_or(empty, |c| c.end)
}
fn finish_row(
    rows: &mut Vec<Row>,
    glyphs: &mut [GlyphPlacement],
    row: Row,
    mode: WritingMode,
) -> Result<(), WritingModeError> {
    if rows.len() >= MAX_ITEMS || !row.inline.is_finite() || !row.cross.is_finite() {
        return Err("text axis row/geometry limit (8192)".to_owned().into());
    }
    if mode.is_reverse() {
        for glyph in &mut glyphs[row.start..row.end] {
            glyph.origin[usize::from(mode.is_vertical())] += row.inline;
        }
    }
    rows.push(row);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{FontId, GlyphId, LineLayout, ShapedGlyph, ShapedRun, WrappedLineLayout, point, px};
    use std::{ops::DerefMut, sync::Arc};
    fn request() -> TextMeasureRequest {
        TextMeasureRequest {
            content: "ab".into(),
            font_family: "geometry-fixture".into(),
            font_size: 10.,
            font_weight: 400,
            italic: false,
            line_height: 14.,
            text_case: Default::default(),
            para_spacing: 7.,
            letter_spacing: 2.,
            word_spacing: 0.,
            para_indent: 4.,
            wrap_width: None,
        }
    }
    // Deliberately specified cluster geometry tests placement only. Native font
    // shaping and real pixels are tested by flow-ui's writing_mode_tests.
    fn line() -> WrappedLine {
        let mut line = WrappedLine::default();
        line.text = "ab".into();
        *line.deref_mut() = Arc::new(WrappedLineLayout {
            unwrapped_layout: Arc::new(LineLayout {
                font_size: px(10.),
                width: px(22.),
                ascent: px(8.),
                descent: px(2.),
                len: 2,
                runs: vec![ShapedRun {
                    font_id: FontId(3),
                    glyphs: [(0, 0.), (0, 2.), (1, 10.)]
                        .into_iter()
                        .enumerate()
                        .map(|(id, (index, x))| ShapedGlyph {
                            id: GlyphId(id as u32),
                            position: point(px(x), px(0.)),
                            index,
                            is_emoji: false,
                        })
                        .collect(),
                }],
            }),
            ..Default::default()
        });
        line
    }
    fn layout(mode: WritingMode, width: Option<f32>) -> AxisLayout {
        let mut request = request();
        request.wrap_width = width;
        from_lines(&[line()], &request, mode, 0, &mut |_| Ok(())).unwrap()
    }
    #[test]
    fn axis_geometry_keeps_native_cluster_members_upright_and_reverses_cluster_advance() {
        let x = layout(WritingMode::XReverse, None);
        assert_eq!(
            x.glyphs.iter().map(|g| g.origin).collect::<Vec<_>>(),
            [[16., 10.], [18., 10.], [2., 10.]]
        );
        let y = layout(WritingMode::Y, None);
        assert_eq!(
            y.glyphs.iter().map(|g| g.origin).collect::<Vec<_>>(),
            [[2., 12.], [4., 12.], [1., 24.]]
        );
        let reverse = layout(WritingMode::YReverse, None);
        assert_eq!(
            reverse.glyphs.iter().map(|g| g.origin).collect::<Vec<_>>(),
            [[2., 22.], [4., 22.], [1., 10.]]
        );
        for plan in [&x, &y, &reverse] {
            assert_eq!(
                plan.glyphs.iter().map(|g| g.glyph_id).collect::<Vec<_>>(),
                [GlyphId(0), GlyphId(1), GlyphId(2)]
            );
            assert!(
                plan.glyphs
                    .iter()
                    .all(|g| g.font_id == FontId(3) && g.font_size == px(10.))
            );
        }
    }
    #[test]
    fn vertical_wrap_uses_inline_height_and_keeps_indent_only_on_paragraph_first_column() {
        let plan = layout(WritingMode::Y, Some(24.));
        assert_eq!(plan.metrics.width, 28.);
        assert_eq!(plan.metrics.height, 16.);
        assert_eq!(plan.metrics.lines, 2);
        assert_eq!(
            plan.glyphs.iter().map(|g| g.origin).collect::<Vec<_>>(),
            [[2., 12.], [4., 12.], [15., 8.]]
        );
        let mut paragraphs = request();
        paragraphs.content = "ab\nab".into();
        let plan = from_lines(
            &[line(), line()],
            &paragraphs,
            WritingMode::Y,
            0,
            &mut |_| Ok(()),
        )
        .unwrap();
        assert_eq!(plan.metrics.width, 35.);
        assert_eq!(plan.glyphs[3].origin, [23., 12.]);
    }
    #[test]
    fn reverse_start_and_end_align_to_opposite_inline_edges_without_reflecting_glyphs() {
        let plan = layout(WritingMode::YReverse, None);
        let mut start = vec![];
        let mut end = vec![];
        plan.visit(
            [5., 7., 80., 100.],
            gpui::TextAlign::Left,
            crate::VerticalAlign::Middle,
            |g| {
                start.push(g.origin);
                Ok::<_, String>(())
            },
        )
        .unwrap();
        plan.visit(
            [5., 7., 80., 100.],
            gpui::TextAlign::Right,
            crate::VerticalAlign::Middle,
            |g| {
                end.push(g.origin);
                Ok::<_, String>(())
            },
        )
        .unwrap();
        assert_eq!(start[0], [40., 101.]);
        assert_eq!(end[0], [40., 29.]);
        assert_eq!(start[1][0] - start[0][0], 2.);
    }
    #[test]
    fn axis_admission_and_native_reverse_order_fail_before_owned_plan_can_escape() {
        let error = from_lines(&[line()], &request(), WritingMode::Y, 0, &mut |_| {
            Err("quota".into())
        })
        .err()
        .unwrap();
        assert_eq!(error, WritingModeError::Invalid("quota".into()));
        let mut native = line();
        let layout = Arc::get_mut(native.deref_mut()).unwrap();
        Arc::get_mut(&mut layout.unwrapped_layout).unwrap().runs[0].glyphs[2].index = 0;
        Arc::get_mut(&mut layout.unwrapped_layout).unwrap().runs[0].glyphs[0].index = 1;
        assert!(matches!(
            from_lines(&[native], &request(), WritingMode::Y, 0, &mut |_| Ok(())),
            Err(WritingModeError::Unsupported(
                UnsupportedWritingMode::NativeClusterOrder
            ))
        ));
    }
    #[test]
    fn fractional_line_boxes_use_the_same_count_formula_as_measured_layout() {
        let mut r = request();
        r.content = std::iter::repeat_n("ab", 2048)
            .collect::<Vec<_>>()
            .join("\n");
        r.line_height = 16.8;
        r.para_spacing = 0.3;
        let lines = (0..2048).map(|_| line()).collect::<Vec<_>>();
        let plan = from_lines(&lines, &r, WritingMode::Y, 0, &mut |_| Ok(())).unwrap();
        assert_eq!(plan.metrics.width, r.block_height(2048));
        assert_eq!(
            plan.rows[2047].cross,
            2047. * r.line_height + 2047. * r.para_spacing
        );
    }
}

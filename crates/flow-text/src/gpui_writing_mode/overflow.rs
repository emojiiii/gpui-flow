//! End ellipsis preserves retained native source clusters and shapes only its marker.
use super::*;
use crate::TextOverflow;

/// Shape axis text for a physical box. Clip retains the full plan; ellipsis
/// trims whole native clusters and keeps the original cross-axis block extent.
pub fn shape_axis_in_box(
    system: &WindowTextSystem,
    request: &TextMeasureRequest,
    mode: WritingMode,
    overflow: TextOverflow,
    width: f32,
    height: f32,
) -> Result<AxisLayout, WritingModeError> {
    shape_axis_in_box_checked(
        system,
        request,
        mode,
        overflow,
        [width, height],
        &mut |_| Ok(()),
    )
}

pub(crate) fn shape_axis_in_box_checked(
    system: &WindowTextSystem,
    request: &TextMeasureRequest,
    mode: WritingMode,
    overflow: TextOverflow,
    size: [f32; 2],
    admit: &mut impl FnMut(usize) -> Result<(), String>,
) -> Result<AxisLayout, WritingModeError> {
    if size.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err("invalid text box".into());
    }
    let original = shape_axis_checked(system, request, mode, admit)?;
    let inline = size[usize::from(mode.is_vertical())];
    let cross = size[usize::from(!mode.is_vertical())];
    let visible = visible_rows(&original, cross);
    if overflow == TextOverflow::Clip
        || (visible == original.rows.len() && original.rows.iter().all(|r| r.inline <= inline))
    {
        return Ok(original);
    }
    // Match horizontal ellipsis: a native "..." is retained even when the marker
    // plus indentation exceeds the inline box; physical clipping then limits ink.
    admit(original.allocated_bytes + request.font_family.len() + 3)?;
    let marker_request = TextMeasureRequest {
        content: "...".into(),
        font_family: request.font_family.clone(),
        font_size: request.font_size,
        font_weight: request.font_weight,
        italic: request.italic,
        line_height: request.line_height,
        text_case: crate::TextCase::None,
        para_spacing: 0.,
        letter_spacing: request.letter_spacing,
        word_spacing: request.word_spacing,
        para_indent: 0.,
        wrap_width: None,
    };
    let marker = shape_axis_checked(system, &marker_request, mode, &mut |bytes| {
        admit(original.allocated_bytes + bytes)
    })?;
    drop(marker_request);
    ellipsize(&original, &marker, inline, visible, admit)
}

fn visible_rows(plan: &AxisLayout, cross: f32) -> usize {
    plan.rows
        .iter()
        .take_while(|row| row.cross_end <= cross)
        .count()
        .max(1)
}

#[derive(Clone, Copy)]
struct Cut {
    glyph_end: usize,
    cluster_end: usize,
    inline: f32,
    marker: bool,
}

fn cutoff(plan: &AxisLayout, row: &Row, marker: f32, available: f32, force: bool) -> Cut {
    if !force && row.inline <= available {
        return Cut {
            glyph_end: row.end,
            cluster_end: row.cluster_end,
            inline: row.inline,
            marker: false,
        };
    }
    let mut cut = Cut {
        glyph_end: row.start,
        cluster_end: row.cluster_start,
        inline: row.indent,
        marker: true,
    };
    for (index, cluster) in plan.cluster_ends[row.cluster_start..row.cluster_end]
        .iter()
        .enumerate()
    {
        if cluster.inline + marker > available {
            break;
        }
        cut.glyph_end = cluster.glyph_end;
        cut.cluster_end = row.cluster_start + index + 1;
        cut.inline = cluster.inline;
    }
    cut
}

fn ellipsize(
    original: &AxisLayout,
    marker: &AxisLayout,
    available: f32,
    visible: usize,
    admit: &mut impl FnMut(usize) -> Result<(), String>,
) -> Result<AxisLayout, WritingModeError> {
    if marker.rows.len() != 1 || marker.glyphs.is_empty() {
        return Err("invalid native ellipsis marker".into());
    }
    let marker_inline = marker.rows[0].inline;
    let hidden = original.rows.len() > visible;
    let cut_row = |index, row: &Row| {
        cutoff(
            original,
            row,
            marker_inline,
            available,
            hidden && index + 1 == visible,
        )
    };
    let mut glyph_count = 0;
    let mut cluster_count = 0;
    for (index, row) in original.rows.iter().take(visible).enumerate() {
        let cut = cut_row(index, row);
        glyph_count += cut.glyph_end - row.start + if cut.marker { marker.glyphs.len() } else { 0 };
        cluster_count += cut.cluster_end - row.cluster_start
            + if cut.marker {
                marker.cluster_ends.len()
            } else {
                0
            };
    }
    if glyph_count > MAX_ITEMS || cluster_count > MAX_ITEMS {
        return Err("text axis ellipsis output glyph/cluster limit (8192)".into());
    }
    let live = original.allocated_bytes + marker.allocated_bytes;
    let planned = glyph_count * size_of::<GlyphPlacement>()
        + visible * size_of::<Row>()
        + cluster_count * size_of::<ClusterEnd>();
    admit(live + planned)?;
    let mut glyphs = Vec::<GlyphPlacement>::new();
    let mut rows = Vec::<Row>::new();
    let mut cluster_ends = Vec::<ClusterEnd>::new();
    glyphs
        .try_reserve_exact(glyph_count)
        .map_err(|_| "text axis allocation failed")?;
    rows.try_reserve_exact(visible)
        .map_err(|_| "text axis allocation failed")?;
    cluster_ends
        .try_reserve_exact(cluster_count)
        .map_err(|_| "text axis allocation failed")?;
    let allocated_bytes = glyphs.capacity() * size_of::<GlyphPlacement>()
        + rows.capacity() * size_of::<Row>()
        + cluster_ends.capacity() * size_of::<ClusterEnd>();
    if allocated_bytes != planned {
        admit(live + allocated_bytes)?;
    }
    let inline_axis = usize::from(original.mode.is_vertical());
    let cross_axis = 1 - inline_axis;
    let mut max_inline = 0f32;
    for (index, row) in original.rows.iter().take(visible).enumerate() {
        // Also gives long multirow output construction a cancellation checkpoint.
        admit(live + allocated_bytes)?;
        let cut = cut_row(index, row);
        let inline = cut.inline + if cut.marker { marker_inline } else { 0. };
        let baseline = if cut.marker && cut.glyph_end == row.start {
            marker.rows[0].baseline
        } else {
            row.baseline
        };
        let start = glyphs.len();
        let cluster_start = cluster_ends.len();
        for source in &original.glyphs[row.start..cut.glyph_end] {
            let mut glyph = *source;
            if original.mode.is_reverse() {
                glyph.origin[inline_axis] += inline - row.inline;
            }
            glyphs.push(glyph);
        }
        for cluster in &original.cluster_ends[row.cluster_start..cut.cluster_end] {
            cluster_ends.push(ClusterEnd {
                glyph_end: start + cluster.glyph_end - row.start,
                inline: cluster.inline,
            });
        }
        if cut.marker {
            let marker_start = glyphs.len();
            for source in &marker.glyphs {
                let mut glyph = *source;
                glyph.origin[cross_axis] += row.cross;
                // Keep the source paragraph's native baseline even when its
                // fallback runs have different metrics from the marker's font.
                glyph.origin[1] += baseline - marker.rows[0].baseline;
                if !original.mode.is_reverse() {
                    glyph.origin[inline_axis] += cut.inline;
                }
                glyphs.push(glyph);
            }
            for cluster in &marker.cluster_ends {
                cluster_ends.push(ClusterEnd {
                    glyph_end: marker_start + cluster.glyph_end,
                    inline: cut.inline + cluster.inline,
                });
            }
        }
        rows.push(Row {
            start,
            end: glyphs.len(),
            inline,
            cluster_start,
            cluster_end: cluster_ends.len(),
            baseline,
            ..*row
        });
        max_inline = max_inline.max(inline);
    }
    // Preserve the original cross extent so clipping does not recenter a block
    // after hidden rows/columns have been removed.
    let mut metrics = original.metrics.clone();
    if original.mode.is_vertical() {
        metrics.height = max_inline.max(f32::from(marker.glyphs[0].font_size));
    } else {
        metrics.width = max_inline;
    }
    metrics.lines = rows.len();
    metrics.validate()?;
    Ok(AxisLayout {
        glyphs,
        rows,
        cluster_ends,
        mode: original.mode,
        metrics,
        allocated_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{GlyphId, point, px};
    use std::{ops::DerefMut, sync::Arc};

    fn marker(mode: WritingMode) -> AxisLayout {
        let mut line = super::super::tests::line();
        line.text = "...".into();
        let wrapped = Arc::get_mut(line.deref_mut()).unwrap();
        let native = Arc::get_mut(&mut wrapped.unwrapped_layout).unwrap();
        native.width = px(9.);
        native.ascent = px(6.); // Marker metrics deliberately differ from the source.
        native.len = 3;
        for (index, glyph) in native.runs[0].glyphs.iter_mut().enumerate() {
            glyph.id = GlyphId(100 + index as u32);
            glyph.index = index;
            glyph.position = point(px(index as f32 * 3.), px(0.));
        }
        let mut request = super::super::tests::request();
        request.content = "...".into();
        request.para_indent = 0.;
        from_lines(&[line], &request, mode, 0, &mut |_| Ok(())).unwrap()
    }

    #[test]
    fn ellipsis_keeps_cluster_members_and_places_native_marker_at_inline_end() {
        for mode in [WritingMode::XReverse, WritingMode::Y, WritingMode::YReverse] {
            let mut request = super::super::tests::request();
            request.content = "ab\nab".into();
            let original = from_lines(
                &[super::super::tests::line(), super::super::tests::line()],
                &request,
                mode,
                0,
                &mut |_| Ok(()),
            )
            .unwrap();
            let marker = marker(mode);
            let available = 16. + marker.rows[0].inline;
            let output = ellipsize(&original, &marker, available, 1, &mut |_| Ok(())).unwrap();
            assert_eq!(
                output.glyphs.iter().map(|g| g.glyph_id).collect::<Vec<_>>(),
                [
                    GlyphId(0),
                    GlyphId(1),
                    GlyphId(100),
                    GlyphId(101),
                    GlyphId(102)
                ]
            );
            assert_eq!(output.glyphs[1].origin[0] - output.glyphs[0].origin[0], 2.);
            assert_eq!(output.glyphs[1].origin[1] - output.glyphs[0].origin[1], 0.);
            assert_eq!(
                output.glyphs[2].origin[1],
                match mode {
                    WritingMode::XReverse => 10.,
                    WritingMode::Y => 24.,
                    WritingMode::YReverse => 34.,
                    WritingMode::X => unreachable!(),
                }
            );
            assert_eq!(output.rows[0].inline, available);
            assert_eq!(output.rows[0].indent, 4.);
            assert_eq!(output.metrics.lines, 1);
            assert_eq!(
                if mode.is_vertical() {
                    output.metrics.width
                } else {
                    output.metrics.height
                },
                35.
            );
            let inline = usize::from(mode.is_vertical());
            if mode.is_reverse() {
                assert!(output.glyphs[2].origin[inline] < output.glyphs[0].origin[inline]);
            } else {
                assert!(output.glyphs[2].origin[inline] > output.glyphs[0].origin[inline]);
            }
            let narrow = ellipsize(&original, &marker, 1., 1, &mut |_| Ok(())).unwrap();
            assert_eq!(narrow.glyphs.len(), marker.glyphs.len());
            assert_eq!(narrow.rows[0].baseline, marker.rows[0].baseline);
            assert_eq!(narrow.rows[0].inline, 4. + marker.rows[0].inline);
            assert!(narrow.glyphs.iter().all(|glyph| glyph.glyph_id.0 >= 100));
        }
    }

    #[test]
    fn ellipsis_marks_an_empty_visible_paragraph_and_retains_hard_newline_gaps() {
        let mut request = super::super::tests::request();
        request.content = "ab\n\n".into();
        let blank = || {
            let mut line = super::super::tests::line();
            line.text = "".into();
            let native =
                Arc::get_mut(&mut Arc::get_mut(line.deref_mut()).unwrap().unwrapped_layout)
                    .unwrap();
            native.runs.clear();
            native.width = px(0.);
            native.len = 0;
            line
        };
        let original = from_lines(
            &[super::super::tests::line(), blank(), blank()],
            &request,
            WritingMode::Y,
            0,
            &mut |_| Ok(()),
        )
        .unwrap();
        assert_eq!(visible_rows(&original, 35.), 2);
        let marker = marker(WritingMode::Y);
        let output = ellipsize(&original, &marker, 100., 2, &mut |_| Ok(())).unwrap();
        assert_eq!(output.rows[1].cross, 21.);
        assert_eq!(output.rows[1].baseline, marker.rows[0].baseline);
        assert_eq!(output.rows[1].indent, 4.);
        assert_eq!(output.rows[1].inline, 4. + marker.rows[0].inline);
        assert_eq!(output.metrics.width, 56.);
        assert_eq!(output.glyphs[3].glyph_id, GlyphId(100));
    }

    #[test]
    fn ellipsis_preserves_wrap_cross_offsets_and_charges_simultaneous_owned_plans() {
        let mut request = super::super::tests::request();
        request.content = "ab\nab".into();
        request.wrap_width = Some(16.);
        let original = from_lines(
            &[super::super::tests::line(), super::super::tests::line()],
            &request,
            WritingMode::YReverse,
            0,
            &mut |_| Ok(()),
        )
        .unwrap();
        assert_eq!(visible_rows(&original, 28.), 2);
        assert_eq!(visible_rows(&original, 1.), 1);
        let marker = marker(WritingMode::YReverse);
        let output = ellipsize(&original, &marker, 64., 2, &mut |_| Ok(())).unwrap();
        assert_eq!(
            output
                .rows
                .iter()
                .map(|row| (row.cross, row.indent))
                .collect::<Vec<_>>(),
            [(0., 4.), (14., 0.)]
        );
        assert_eq!(output.metrics.width, original.metrics.width);
        assert_eq!(output.glyphs[2].glyph_id, GlyphId(2));
        assert_eq!(output.glyphs[3].glyph_id, GlyphId(100));
        let live = original.allocated_bytes + marker.allocated_bytes;
        let error = ellipsize(&original, &marker, 64., 2, &mut |bytes| {
            assert!(bytes > live);
            Err("combined axis quota".into())
        })
        .err()
        .unwrap();
        assert_eq!(
            error,
            WritingModeError::Invalid("combined axis quota".into())
        );
    }

    #[test]
    fn fractional_cross_endpoints_keep_exact_fits_and_reject_one_ulp_less() {
        for mode in [WritingMode::XReverse, WritingMode::Y, WritingMode::YReverse] {
            for (paragraphs, line_height, para_spacing, wrap_width) in [
                (4, 24.1, 0., None),
                (7, 24.1, 0., None),
                (7, 16.8, 0.3, Some(16.)),
            ] {
                let mut request = super::super::tests::request();
                request.content = std::iter::repeat_n("ab", paragraphs)
                    .collect::<Vec<_>>()
                    .join("\n");
                request.line_height = line_height;
                request.para_spacing = para_spacing;
                request.wrap_width = wrap_width;
                let lines = (0..paragraphs)
                    .map(|_| super::super::tests::line())
                    .collect::<Vec<_>>();
                let plan = from_lines(&lines, &request, mode, 0, &mut |_| Ok(())).unwrap();
                let rows_per_paragraph = if wrap_width.is_some() { 2 } else { 1 };
                assert_eq!(plan.rows.len(), paragraphs * rows_per_paragraph);
                for (index, row) in plan.rows.iter().enumerate() {
                    let expected = (index + 1) as f32 * line_height
                        + (index / rows_per_paragraph) as f32 * para_spacing;
                    assert_eq!(row.cross_end, expected);
                    assert_eq!(visible_rows(&plan, expected), index + 1);
                    assert_eq!(visible_rows(&plan, expected.next_down()), index.max(1));
                }
                assert_eq!(
                    plan.rows.last().unwrap().cross_end,
                    request.block_height(plan.rows.len())
                );
            }
        }
    }
}

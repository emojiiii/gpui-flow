use gpui::{Hsla, TextAlign, WindowTextSystem, px};
use gpui_flow_text::{
    GpuiTextMeasurer, GpuiTextRasterizer, TextCase, TextDecoration, TextMeasureRequest,
    TextMeasurer, TextOverflow, TextRasterRequest, VerticalAlign,
    gpui_backend::shape_text,
    gpui_decoration::{DecorationRequest, layout_decorations},
    gpui_overflow::shape_text_in_box,
};
use std::sync::atomic::AtomicBool;
fn request(content: &str) -> TextMeasureRequest {
    TextMeasureRequest {
        content: content.into(),
        font_family: "DejaVu Sans".into(),
        font_size: 18.,
        font_weight: 400,
        italic: false,
        text_case: TextCase::None,
        para_spacing: 9.,
        line_height: 24.,
        wrap_width: None,
    }
}
#[test]
fn unicode_presentation_matches_native_shaping_and_measurement_without_source_mutation() {
    let app = gpui_kit::platform::headless();
    let system = WindowTextSystem::new(app.text_system());
    let measure = GpuiTextMeasurer::new(app.text_system());
    for case in [TextCase::Upper, TextCase::Lower] {
        let mut r = request("Straße İ ΟΣ\nffi e\u{301}\n");
        r.text_case = case;
        let before = r.clone();
        let mut expected = r.clone();
        expected.content = case.presentation(&r.content).unwrap().into_owned();
        expected.text_case = TextCase::None;
        let shaped = shape_text(&system, &r, Hsla::default()).unwrap();
        let plain = shape_text(&system, &expected, Hsla::default()).unwrap();
        for (a, b) in shaped.iter().zip(&plain) {
            assert_eq!(a.text, b.text);
            assert_eq!(a.size(px(24.)), b.size(px(24.)));
        }
        assert_eq!(
            measure.measure(&r).unwrap(),
            measure.measure(&expected).unwrap()
        );
        assert_eq!(measure.measure(&r).unwrap().height, 90.);
        assert_eq!(r, before);
    }
}
#[test]
fn hard_breaks_empty_paragraphs_and_trailing_newline_have_exact_offsets() {
    let app = gpui_kit::platform::headless();
    let system = WindowTextSystem::new(app.text_system());
    for (content, offsets) in [
        ("", vec![0.]),
        ("A\n", vec![0., 33.]),
        ("A\n\nB\n", vec![0., 33., 66., 99.]),
    ] {
        let r = request(content);
        let s = shape_text_in_box(&system, &r, Hsla::default(), TextOverflow::Clip, 300., 300.)
            .unwrap();
        assert_eq!(s.line_offsets, offsets);
        assert_eq!(
            s.block_height,
            24. * s.lines.len() as f32 + 9. * (s.lines.len() - 1) as f32
        );
        assert_eq!(
            GpuiTextMeasurer::new(app.text_system())
                .measure(&r)
                .unwrap()
                .height,
            s.block_height
        );
    }
}
#[test]
fn wrapping_adds_no_paragraph_gap_and_ellipsis_preserves_original_row_positions() {
    let app = gpui_kit::platform::headless();
    let system = WindowTextSystem::new(app.text_system());
    let mut r = request("one two three four five six seven eight nine\nfinal paragraph");
    r.wrap_width = Some(95.);
    r.para_spacing = 19.;
    r.text_case = TextCase::Upper;
    let full =
        shape_text_in_box(&system, &r, Hsla::default(), TextOverflow::Clip, 95., 500.).unwrap();
    let rows = full.lines[0].wrap_boundaries.len() + 1;
    assert!(rows > 2);
    assert_eq!(full.line_offsets[1], rows as f32 * 24. + 19.);
    let clipped = shape_text_in_box(
        &system,
        &r,
        Hsla::default(),
        TextOverflow::Ellipsis,
        95.,
        48.,
    )
    .unwrap();
    assert_eq!(clipped.line_offsets, vec![0., 24.]);
    assert_eq!(clipped.lines.len(), 2);
    assert!(clipped.lines[1].text.ends_with("..."));
    assert_eq!(clipped.block_height, full.block_height);
}
#[test]
fn ellipsis_gap_visibility_and_decorations_share_the_same_line_offsets() {
    let app = gpui_kit::platform::headless();
    let system = WindowTextSystem::new(app.text_system());
    let mut r = request("A\nB\nC");
    r.para_spacing = 20.;
    for (height, offsets, ending) in [(60., vec![0.], "A..."), (70., vec![0., 44.], "B...")] {
        let s = shape_text_in_box(
            &system,
            &r,
            Hsla::default(),
            TextOverflow::Ellipsis,
            150.,
            height,
        )
        .unwrap();
        assert_eq!(s.line_offsets, offsets);
        assert_eq!(s.lines.last().unwrap().text.as_ref(), ending);
        let d = layout_decorations(
            &s,
            &DecorationRequest {
                decoration: TextDecoration::Underline,
                font_size: 18.,
                line_height: 24.,
                bounds: [0., 0., 150., height],
                align: TextAlign::Left,
                vertical_align: VerticalAlign::Bottom,
                device_scale: 1.,
            },
            1_000_000,
        )
        .unwrap();
        assert_eq!(d.rects.len(), offsets.len());
        if offsets.len() == 2 {
            assert_eq!(d.rects[1][1] - d.rects[0][1], 44.);
        }
    }
}
#[test]
fn cpu_glyph_pixels_match_explicit_transformed_text_at_dpr_and_alignment_variants() {
    let (_app, bridge) =
        GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    for case in [TextCase::Upper, TextCase::Lower] {
        for (scale, align, vertical) in [
            (1., TextAlign::Left, VerticalAlign::Top),
            (2., TextAlign::Right, VerticalAlign::Bottom),
        ] {
            let mut r = TextRasterRequest {
                text: request("Straße İ\n\nffi end"),
                align,
                vertical_align: vertical,
                overflow: TextOverflow::Clip,
                decoration: TextDecoration::Underline,
                bounds: [0., 0., 200., 160.],
                clip: [0., 0., 200., 160.],
                device_scale: scale,
                color: [230, 180, 90, 255],
            };
            r.text.text_case = case;
            let render = |r: &TextRasterRequest| {
                bridge
                    .rasterize(
                        r,
                        bridge.epoch(),
                        &AtomicBool::new(false),
                        8_000_000,
                        &mut 8_000_000,
                    )
                    .unwrap()
                    .unwrap()
            };
            let styled = render(&r);
            r.text.content = case.presentation(&r.text.content).unwrap().into_owned();
            r.text.text_case = TextCase::None;
            let explicit = render(&r);
            assert_eq!(styled.premul_rgba, explicit.premul_rgba);
            assert_eq!(styled.rows, 3);
            assert!(styled.glyphs > 0);
        }
    }
}
#[test]
fn paragraph_and_case_errors_preserve_raster_budgets_cancellation_and_input() {
    let (app, bridge) = GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let mut r = TextRasterRequest {
        text: request("a\nb"),
        align: TextAlign::Left,
        vertical_align: VerticalAlign::Top,
        overflow: TextOverflow::Ellipsis,
        decoration: TextDecoration::None,
        bounds: [0., 0., 100., 100.],
        clip: [0., 0., 100., 100.],
        device_scale: 1.,
        color: [255; 4],
    };
    let before = r.text.clone();
    assert!(
        bridge
            .rasterize(
                &r,
                bridge.epoch(),
                &AtomicBool::new(true),
                1_000_000,
                &mut 1_000_000
            )
            .is_err()
    );
    assert!(
        bridge
            .rasterize(
                &r,
                bridge.epoch(),
                &AtomicBool::new(false),
                10,
                &mut 1_000_000
            )
            .is_err()
    );
    assert!(
        bridge
            .rasterize(
                &r,
                bridge.epoch(),
                &AtomicBool::new(false),
                1_000_000,
                &mut 1
            )
            .is_err()
    );
    assert_eq!(r.text, before);
    r.text.content = "İ".repeat(32_768);
    r.text.text_case = TextCase::Lower;
    assert!(
        shape_text(
            &WindowTextSystem::new(app.text_system()),
            &r.text,
            Hsla::default()
        )
        .is_err()
    );
    assert!(
        bridge
            .rasterize(
                &r,
                bridge.epoch(),
                &AtomicBool::new(false),
                1_000_000,
                &mut 1_000_000
            )
            .is_err()
    );
}

#[test]
fn paragraph_offset_storage_is_charged_to_the_raster_working_set() {
    let (_app, bridge) =
        GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let r = TextRasterRequest {
        text: request("\n\n"),
        align: TextAlign::Left,
        vertical_align: VerticalAlign::Top,
        overflow: TextOverflow::Clip,
        decoration: TextDecoration::None,
        bounds: [0., 0., 100., 100.],
        clip: [0., 0., 100., 100.],
        device_scale: 1.,
        color: [255; 4],
    };
    // Empty lines consume no glyph bitmap; the only extra retained allocation
    // beyond the output pixels is exactly the three paragraph offsets.
    assert!(
        bridge
            .rasterize(
                &r,
                bridge.epoch(),
                &AtomicBool::new(false),
                40_000,
                &mut 1_000_000
            )
            .is_err()
    );
    let result = bridge
        .rasterize(
            &r,
            bridge.epoch(),
            &AtomicBool::new(false),
            40_012,
            &mut 1_000_000,
        )
        .unwrap()
        .unwrap();
    assert_eq!(result.peak_bytes, 40_012);
    assert_eq!(result.rows, 3);
}

#[test]
fn cpu_paragraph_gap_moves_glyph_ink_by_exact_hard_break_offset() {
    let (_app, bridge) =
        GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let mut r = TextRasterRequest {
        text: request("A"),
        align: TextAlign::Left,
        vertical_align: VerticalAlign::Top,
        overflow: TextOverflow::Clip,
        decoration: TextDecoration::None,
        bounds: [0., 0., 100., 100.],
        clip: [0., 0., 100., 100.],
        device_scale: 1.,
        color: [255; 4],
    };
    let render = |r: &TextRasterRequest| {
        bridge
            .rasterize(
                r,
                bridge.epoch(),
                &AtomicBool::new(false),
                1_000_000,
                &mut 1_000_000,
            )
            .unwrap()
            .unwrap()
    };
    let single = render(&r);
    r.text.content = "A\nA".into();
    let paired = render(&r);
    let stride = 100 * 4;
    let mut expected = single.premul_rgba.clone();
    for y in 0..(100 - 33) {
        for x in 0..stride {
            expected[(y + 33) * stride + x] =
                expected[(y + 33) * stride + x].max(single.premul_rgba[y * stride + x]);
        }
    }
    assert_eq!(paired.premul_rgba, expected);
    assert_eq!(paired.rows, 2);
}

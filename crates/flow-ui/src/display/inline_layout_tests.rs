use gpui::{Hsla, TextAlign, WindowTextSystem};
use gpui_flow_text::{
    GpuiTextMeasurer, GpuiTextRasterizer, TextMeasureRequest, TextMeasurer, TextOverflow,
    TextRasterRequest, gpui_backend::shape_text, gpui_overflow::shape_text_in_box,
};
use std::sync::atomic::AtomicBool;
fn request(content: &str) -> TextMeasureRequest {
    TextMeasureRequest {
        content: content.into(),
        font_family: "DejaVu Sans".into(),
        font_size: 18.,
        font_weight: 400,
        italic: false,
        line_height: 24.,
        text_case: Default::default(),
        para_spacing: 0.,
        letter_spacing: 0.,
        word_spacing: 0.,
        para_indent: 0.,
        wrap_width: None,
    }
}
#[test]
fn inline_layout_uses_native_clusters_and_never_mutates_cached_layouts() {
    let app = gpui::Application::with_platform(crate::display::native_text_test_platform());
    let system = WindowTextSystem::new(app.text_system());
    let plain = request("office a e\u{301}");
    let original = shape_text(&system, &plain, Hsla::default()).unwrap();
    let original_glyphs: Vec<_> = original[0].runs().iter().flat_map(|r| &r.glyphs).collect();
    let mut styled = plain.clone();
    styled.letter_spacing = 2.;
    styled.word_spacing = 5.;
    styled.para_indent = 11.;
    let changed = shape_text(&system, &styled, Hsla::default()).unwrap();
    let changed_glyphs: Vec<_> = changed[0].runs().iter().flat_map(|r| &r.glyphs).collect();
    assert_eq!(original_glyphs.len(), changed_glyphs.len());
    let mut extra = 11.;
    let mut previous = None;
    for (before, after) in original_glyphs.iter().zip(&changed_glyphs) {
        if previous != Some(before.index) {
            if let Some(index) = previous {
                extra += 2.
                    + if plain.content[index..].starts_with(' ') {
                        5.
                    } else {
                        0.
                    };
            }
            previous = Some(before.index);
        }
        assert_eq!(before.id, after.id);
        assert_eq!(before.index, after.index);
        assert!((f32::from(after.position.x - before.position.x) - extra).abs() < 0.001);
    }
    extra += 2.;
    assert!(
        (f32::from(changed[0].unwrapped_layout.width - original[0].unwrapped_layout.width) - extra)
            .abs()
            < 0.001
    );
    let again = shape_text(&system, &plain, Hsla::default()).unwrap();
    assert_eq!(
        again[0].unwrapped_layout.width,
        original[0].unwrapped_layout.width
    );
    let measured = GpuiTextMeasurer::new(app.text_system())
        .measure(&styled)
        .unwrap();
    assert_eq!(measured.width, f32::from(changed[0].unwrapped_layout.width));
}
#[test]
fn inline_layout_indent_changes_wrap_and_ellipsis_keeps_only_paragraph_starts_indented() {
    let app = gpui::Application::with_platform(crate::display::native_text_test_platform());
    let system = WindowTextSystem::new(app.text_system());
    let mut r = request("alpha beta gamma delta epsilon\nsecond paragraph");
    r.wrap_width = Some(100.);
    r.para_indent = 20.;
    r.letter_spacing = 1.;
    r.word_spacing = 3.;
    r.para_spacing = 9.;
    let full =
        shape_text_in_box(&system, &r, Hsla::default(), TextOverflow::Clip, 100., 500.).unwrap();
    assert!(!full.lines[0].wrap_boundaries.is_empty());
    assert_eq!(f32::from(full.lines[0].runs()[0].glyphs[0].position.x), 20.);
    let clipped = shape_text_in_box(
        &system,
        &r,
        Hsla::default(),
        TextOverflow::Ellipsis,
        100.,
        48.,
    )
    .unwrap();
    assert_eq!(clipped.lines.len(), 2);
    assert_eq!(clipped.line_offsets, [0., 24.]);
    assert_eq!(
        f32::from(clipped.lines[0].runs()[0].glyphs[0].position.x),
        20.
    );
    assert_eq!(
        f32::from(clipped.lines[1].runs()[0].glyphs[0].position.x),
        0.
    );
    assert!(clipped.lines[1].text.ends_with("..."));
    assert!(clipped.layout_bytes > 0);
    assert!(full.line_offsets[1] >= 48. + 9.);
    let mut overwide = request("A");
    overwide.wrap_width = Some(100.);
    overwide.para_indent = 95.;
    let clipped = shape_text_in_box(
        &system,
        &overwide,
        Hsla::default(),
        TextOverflow::Ellipsis,
        100.,
        24.,
    )
    .unwrap();
    assert_eq!(clipped.lines[0].text.as_ref(), "...");
}
fn raster_request(content: &str) -> TextRasterRequest {
    TextRasterRequest {
        text: request(content),
        align: TextAlign::Left,
        vertical_align: Default::default(),
        overflow: TextOverflow::Clip,
        decoration: Default::default(),
        bounds: [0., 0., 180., 50.],
        clip: [0., 0., 180., 50.],
        device_scale: 1.,
        color: [255; 4],
    }
}
#[test]
fn inline_layout_cpu_indent_matches_exact_glyph_translation_and_budget_admission() {
    let (_app, bridge) =
        GpuiTextRasterizer::application(crate::display::native_text_test_platform());
    let mut r = raster_request("AB");
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
    let plain = render(&r);
    r.text.para_indent = 12.;
    let indented = render(&r);
    for y in 0..50usize {
        for x in 0..168usize {
            assert_eq!(
                &plain.premul_rgba[(y * 180 + x) * 4..(y * 180 + x + 1) * 4],
                &indented.premul_rgba[(y * 180 + x + 12) * 4..(y * 180 + x + 13) * 4]
            );
        }
    }
    let mut empty = raster_request("");
    empty.text.letter_spacing = 1.;
    assert!(
        bridge
            .rasterize(
                &empty,
                bridge.epoch(),
                &AtomicBool::new(false),
                36_004,
                &mut 1_000_000
            )
            .unwrap_err()
            .contains("working-set")
    );
    assert!(
        bridge
            .rasterize(
                &r,
                bridge.epoch(),
                &AtomicBool::new(true),
                1_000_000,
                &mut 1_000_000
            )
            .unwrap_err()
            .contains("superseded")
    );
    r.text.content = "a".repeat(8193);
    assert!(
        bridge
            .rasterize(
                &r,
                bridge.epoch(),
                &AtomicBool::new(false),
                1_000_000,
                &mut 1_000_000
            )
            .unwrap_err()
            .contains("glyph limit")
    );
}
#[test]
fn inline_layout_rejects_invalid_style_dimensions() {
    let app = gpui::Application::with_platform(crate::display::native_text_test_platform());
    let system = WindowTextSystem::new(app.text_system());
    let mut r = request("A");
    for bad in [-1., f32::NAN, f32::INFINITY, 1_000_001.] {
        r.word_spacing = bad;
        assert!(shape_text(&system, &r, Hsla::default()).is_err());
    }
    r.word_spacing = 0.;
    r.letter_spacing = 1.;
    r.content = "a".repeat(65_537);
    assert!(shape_text(&system, &r, Hsla::default()).is_err());
}

#[test]
fn inline_layout_ellipsis_reports_transient_owned_shape_peak() {
    let (_app, bridge) =
        GpuiTextRasterizer::application(crate::display::native_text_test_platform());
    let mut r = raster_request(&"a ".repeat(200));
    r.text.letter_spacing = 1.;
    r.text.para_indent = 10.;
    r.overflow = TextOverflow::Ellipsis;
    let render = |max_bytes| {
        bridge.rasterize(
            &r,
            bridge.epoch(),
            &AtomicBool::new(false),
            max_bytes,
            &mut 1_000_000,
        )
    };
    let result = render(1_000_000).unwrap().unwrap();
    assert!(result.peak_bytes > result.premul_rgba.len() + 10_000);
    let exact = render(result.peak_bytes).unwrap().unwrap();
    assert_eq!(exact.premul_rgba, result.premul_rgba);
    assert!(
        render(result.peak_bytes - 1)
            .unwrap_err()
            .contains("working-set")
    );
}

#[test]
fn inline_layout_decoration_excludes_indent_and_empty_paragraphs() {
    use gpui_flow_text::{
        TextDecoration,
        gpui_decoration::{DecorationRequest, layout_decorations},
    };
    let app = gpui::Application::with_platform(crate::display::native_text_test_platform());
    let system = WindowTextSystem::new(app.text_system());
    let mut r = request("A\n\nB");
    r.para_indent = 20.;
    let shaped =
        shape_text_in_box(&system, &r, Hsla::default(), TextOverflow::Clip, 180., 100.).unwrap();
    let decorations = layout_decorations(
        &shaped,
        &DecorationRequest {
            decoration: TextDecoration::Underline,
            font_size: r.font_size,
            line_height: r.line_height,
            bounds: [0., 0., 180., 100.],
            align: TextAlign::Left,
            vertical_align: Default::default(),
            device_scale: 1.,
        },
        1_000_000,
    )
    .unwrap();
    assert_eq!(decorations.rects.len(), 2);
    for rect in decorations.rects {
        assert_eq!(rect[0], 20.);
        assert!(rect[2] > 0. && rect[2] < 20.);
    }
}

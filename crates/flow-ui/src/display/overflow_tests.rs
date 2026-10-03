use gpui::{Hsla, WindowTextSystem, px};
use gpui_flow_text::{
    TextMeasureRequest, TextOverflow,
    gpui_backend::shape_text,
    gpui_overflow::{TextBoxShape, shape_text_in_box},
};
fn request(text: &str, wrap: Option<f32>) -> TextMeasureRequest {
    TextMeasureRequest {
        content: text.into(),
        font_family: "DejaVu Sans".into(),
        font_size: 18.,
        font_weight: 400,
        italic: false,
        text_case: Default::default(),
        para_spacing: 0.,
        letter_spacing: 0.,
        word_spacing: 0.,
        para_indent: 0.,
        line_height: 24.,
        wrap_width: wrap,
    }
}
fn shaped(r: &TextMeasureRequest, w: f32, h: f32) -> TextBoxShape {
    let app = gpui_kit::platform::headless();
    shape_text_in_box(
        &WindowTextSystem::new(app.text_system()),
        r,
        Hsla::default(),
        TextOverflow::Ellipsis,
        w,
        h,
    )
    .unwrap()
}
#[test]
fn ellipsis_nowrap_shapes_marker_inside_width_without_rewriting_source() {
    let r = request("The quick brown fox jumps", None);
    let before = r.clone();
    let s = shaped(&r, 110., 24.);
    assert_eq!(s.lines.len(), 1);
    assert!(s.lines[0].text.ends_with("..."));
    assert!(f32::from(s.lines[0].size(px(24.)).width) <= 110.);
    assert_eq!(r, before);
}
#[test]
fn ellipsis_wrap_uses_native_rows_and_marks_only_last_visible_row() {
    let r = request("One two three four five six seven eight nine", Some(110.));
    let s = shaped(&r, 110., 48.);
    assert_eq!(s.lines.len(), 2);
    assert!(!s.lines[0].text.ends_with("..."));
    assert!(s.lines[1].text.ends_with("..."));
    assert!(s.block_height > 48.);
    for l in &s.lines {
        assert!(f32::from(l.size(px(24.)).width) <= 110.);
    }
}
#[test]
fn ellipsis_vertical_overflow_keeps_original_height_for_top_alignment() {
    let s = shaped(&request("A\nB\nC", None), 100., 50.);
    assert_eq!(s.lines.len(), 2);
    assert_eq!(s.lines[0].text.as_ref(), "A");
    assert_eq!(s.lines[1].text.as_ref(), "B...");
    assert_eq!(s.block_height, 72.);
    assert_eq!(
        gpui_flow_text::VerticalAlign::Bottom.offset(50., s.block_height),
        0.
    );
}
#[test]
fn ellipsis_fitting_text_empty_rows_and_trailing_newline_preserve_native_layout() {
    for text in ["Fits", "", "A\n", "A\n\nB"] {
        let r = request(text, None);
        let s = shaped(&r, 300., 200.);
        let app = gpui_kit::platform::headless();
        let raw = shape_text(
            &WindowTextSystem::new(app.text_system()),
            &r,
            Hsla::default(),
        )
        .unwrap();
        assert_eq!(s.lines.len(), raw.len());
        for (a, b) in s.lines.iter().zip(raw.iter()) {
            assert_eq!(a.text, b.text);
            assert_eq!(a.size(px(24.)), b.size(px(24.)));
        }
    }
}
#[test]
fn ellipsis_tiny_boxes_keep_marker_for_existing_clip_mask() {
    let s = shaped(&request("content\nhidden", None), 1., 1.);
    assert_eq!(s.lines.len(), 1);
    assert_eq!(s.lines[0].text.as_ref(), "...");
    assert_eq!(s.block_height, 48.);
}
#[test]
fn ellipsis_never_cuts_combining_or_zwj_graphemes() {
    let family = "👩‍👩‍👧‍👦";
    for atom in ["e\u{301}", family, "🇨🇦"] {
        let source = atom.repeat(12);
        let r = request(&source, None);
        let s = shaped(&r, 110., 24.);
        let prefix = s.lines[0].text.strip_suffix("...").unwrap();
        assert_eq!(prefix.len() % atom.len(), 0, "{prefix:?}");
        assert!(source.starts_with(prefix));
    }
}
#[test]
fn ellipsis_rejects_source_and_geometry_budgets_explicitly() {
    let app = gpui_kit::platform::headless();
    let system = WindowTextSystem::new(app.text_system());
    for (r, w, h) in [
        (request(&"x".repeat(65537), None), 100., 24.),
        (request(&"x".repeat(8193), None), 100., 24.),
        (request("ok", None), f32::NAN, 24.),
        (request("ok", None), 100., 0.),
    ] {
        assert!(
            shape_text_in_box(&system, &r, Hsla::default(), TextOverflow::Ellipsis, w, h).is_err()
        );
    }
}

#[test]
fn ellipsis_cpu_raster_uses_real_marker_glyphs_and_shared_box_rows() {
    use std::sync::atomic::AtomicBool;
    let (app, bridge) =
        gpui_flow_text::GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let system = WindowTextSystem::new(app.text_system());
    let text = request("One two three four five six seven", Some(110.));
    let shaped = shape_text_in_box(
        &system,
        &text,
        Hsla::default(),
        TextOverflow::Ellipsis,
        110.,
        48.,
    )
    .unwrap();
    let mut r = gpui_flow_text::TextRasterRequest {
        text,
        align: gpui::TextAlign::Left,
        vertical_align: gpui_flow_text::VerticalAlign::Top,
        overflow: TextOverflow::Ellipsis,
        decoration: gpui_flow_text::TextDecoration::None,
        bounds: [0., 0., 110., 48.],
        clip: [0., 0., 110., 48.],
        device_scale: 1.,
        color: [255; 4],
    };
    let raster = |r: &gpui_flow_text::TextRasterRequest| {
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
    let actual = raster(&r);
    r.text.content = shaped
        .lines
        .iter()
        .map(|l| l.text.as_ref())
        .collect::<Vec<_>>()
        .join("\n");
    r.text.wrap_width = None;
    r.overflow = TextOverflow::Clip;
    let expected = raster(&r);
    assert_eq!(actual.premul_rgba, expected.premul_rgba);
    assert_eq!(actual.rows, 2);
    assert!(actual.glyphs > 3);
    assert!(
        actual
            .premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0)
    );
}

#[test]
fn real_italic_faces_feed_native_shaping_measurement_and_cpu_raster() {
    use gpui_flow_text::TextMeasurer;
    use std::sync::atomic::AtomicBool;
    let (app, bridge) =
        gpui_flow_text::GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let system = WindowTextSystem::new(app.text_system());
    let regular = request("ffi / actual italic face café", None);
    let mut italic = regular.clone();
    italic.italic = true;
    let upright = shape_text(&system, &regular, Hsla::default()).unwrap();
    let slanted = shape_text(&system, &italic, Hsla::default()).unwrap();
    assert_ne!(
        upright[0].unwrapped_layout.runs[0].font_id,
        slanted[0].unwrapped_layout.runs[0].font_id
    );
    let measurer = gpui_flow_text::GpuiTextMeasurer::new(app.text_system());
    for (r, line) in [(&regular, &upright[0]), (&italic, &slanted[0])] {
        let m = measurer.measure(r).unwrap();
        assert_eq!(m.width, f32::from(line.size(px(r.line_height)).width));
    }
    let mut r = gpui_flow_text::TextRasterRequest {
        text: regular,
        align: gpui::TextAlign::Left,
        vertical_align: gpui_flow_text::VerticalAlign::Top,
        overflow: TextOverflow::Ellipsis,
        decoration: gpui_flow_text::TextDecoration::None,
        bounds: [0., 0., 140., 48.],
        clip: [0., 0., 140., 48.],
        device_scale: 1.,
        color: [255; 4],
    };
    let render = |r: &gpui_flow_text::TextRasterRequest| {
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
    let normal = render(&r);
    r.text = italic;
    let italic = render(&r);
    assert_ne!(normal.premul_rgba, italic.premul_rgba);
    assert!(italic.glyphs > 0);
}
#[test]
fn italic_ellipsis_width_uses_selected_face_and_preserves_original_text() {
    let mut r = request("ffi tilted typography with a longer ending", None);
    r.italic = true;
    r.font_weight = 700;
    let before = r.clone();
    let s = shaped(&r, 110., 24.);
    assert!(s.lines[0].text.ends_with("..."));
    assert!(f32::from(s.lines[0].size(px(24.)).width) <= 110.);
    assert_eq!(r, before);
}

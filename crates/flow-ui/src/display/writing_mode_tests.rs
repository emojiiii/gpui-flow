use gpui::{Hsla, TextAlign, WindowTextSystem};
use gpui_flow_text::{
    GpuiTextMeasurer, GpuiTextRasterizer, TextDecoration, TextMeasureRequest, TextMeasurer,
    TextOverflow, TextRasterRequest, WritingMode, WritingModeError,
    gpui_backend::shape_text,
    gpui_writing_mode::{shape_axis, shape_axis_in_box},
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
fn raster_request(content: &str) -> TextRasterRequest {
    TextRasterRequest {
        text: request(content),
        align: TextAlign::Left,
        vertical_align: Default::default(),
        overflow: TextOverflow::Clip,
        decoration: Default::default(),
        bounds: [0., 0., 120., 120.],
        clip: [0., 0., 120., 120.],
        device_scale: 1.,
        color: [12, 34, 56, 255],
    }
}
#[test]
fn writing_mode_measurement_uses_real_native_clusters_and_physical_axes() {
    let app = gpui_kit::platform::headless();
    let system = WindowTextSystem::new(app.text_system());
    let backend = GpuiTextMeasurer::new(app.text_system());
    let r = request("office e\u{301}");
    let native = shape_text(&system, &r, Hsla::default()).unwrap();
    let mut previous = None;
    let clusters = native[0]
        .runs()
        .iter()
        .flat_map(|run| &run.glyphs)
        .filter(|g| {
            let new = previous != Some(g.index);
            previous = Some(g.index);
            new
        })
        .count();
    for mode in [WritingMode::Y, WritingMode::YReverse] {
        let metrics = backend.measure_writing_mode(&r, mode).unwrap();
        assert_eq!(metrics.width, 24.);
        assert_eq!(metrics.height, clusters as f32 * 18.);
        assert_eq!(&metrics, shape_axis(&system, &r, mode).unwrap().metrics());
    }
    let mut blank_columns = request("A\n\n");
    blank_columns.para_spacing = 7.;
    let blank_metrics = backend
        .measure_writing_mode(&blank_columns, WritingMode::Y)
        .unwrap();
    assert_eq!(
        (
            blank_metrics.width,
            blank_metrics.height,
            blank_metrics.lines
        ),
        (86., 18., 3)
    );
    assert_eq!(
        backend.measure(&r).unwrap(),
        backend.measure_writing_mode(&r, WritingMode::X).unwrap()
    );
    let mut wrapped = r.clone();
    wrapped.wrap_width = Some(36.);
    let metrics = backend
        .measure_writing_mode(&wrapped, WritingMode::Y)
        .unwrap();
    assert!(metrics.width > 24. && metrics.height <= 36.);
}
#[test]
fn writing_mode_raster_paints_real_upright_glyphs_and_keeps_cancellation_and_limits() {
    let (_app, bridge) =
        GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let r = raster_request("AB");
    let mut outputs = vec![];
    for mode in [
        WritingMode::X,
        WritingMode::XReverse,
        WritingMode::Y,
        WritingMode::YReverse,
    ] {
        let out = bridge
            .rasterize_writing_mode(
                &r,
                mode,
                bridge.epoch(),
                &AtomicBool::new(false),
                2_000_000,
                &mut 1_000_000,
            )
            .unwrap()
            .unwrap();
        assert_eq!(out.glyphs, 2);
        let ink = out
            .premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] != 0)
            .count();
        assert!(
            ink > 20 && ink < 18 * 18 * 2,
            "actual glyph ink must not become filled cells"
        );
        outputs.push(out.premul_rgba);
    }
    for pair in outputs.windows(2) {
        assert_ne!(pair[0], pair[1]);
    }
    for mode in [WritingMode::XReverse, WritingMode::Y, WritingMode::YReverse] {
        assert_eq!(
            bridge
                .rasterize_writing_mode(
                    &r,
                    mode,
                    bridge.epoch(),
                    &AtomicBool::new(true),
                    2_000_000,
                    &mut 1_000_000
                )
                .unwrap_err(),
            WritingModeError::Invalid("text snapshot superseded".into())
        );
        assert!(
            bridge
                .rasterize_writing_mode(
                    &r,
                    mode,
                    bridge.epoch(),
                    &AtomicBool::new(false),
                    16,
                    &mut 1_000_000
                )
                .is_err()
        );
    }
}

#[test]
fn writing_mode_routes_scene_and_standalone_consumers_without_horizontal_fallback() {
    use gpui_flow_composite::{
        CompositeError, CompositeLimits, RasterViewport, SceneUnit, scene_units,
    };
    use gpui_flow_scene::{
        Affine, Bounds, Color, DisplayObject, Point, Scene, Size, Style, display,
    };
    use std::sync::Arc;
    let (_app, bridge) =
        GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    for mode in [
        display::TextWritingMode::X,
        display::TextWritingMode::Y,
        display::TextWritingMode::YReverse,
        display::TextWritingMode::XReverse,
    ] {
        let mut scene = Scene::new();
        let mut text = display::Text::new("AB e\u{301}\nCD", Size::new(100., 80.));
        text.font_family = "DejaVu Sans".into();
        text.font_size = 18.;
        text.padding = [4.; 4];
        text.writing_mode = mode;
        text.overflow = display::TextOverflow::Ellipsis;
        text.decoration = display::TextDecoration::UnderlineStrikethrough;
        scene
            .insert(
                DisplayObject::new("t", text).with_style(Style::fill(Color::WHITE)),
                None,
                None,
            )
            .unwrap();
        let units = scene_units(&scene, CompositeLimits::default()).unwrap();
        let nondefault = mode != display::TextWritingMode::X;
        assert_eq!(
            units.iter().any(|u| matches!(u, SceneUnit::Domain(_))),
            nondefault
        );
        let paint = scene.paint_list().unwrap();
        let native =
            super::RenderObject::from_paint_item(&paint[0], gpui_flow_core::Viewport::default());
        assert_eq!(native.native_renderable(), !nondefault);
        if !nondefault {
            continue;
        }
        let domains: Vec<_> = units
            .into_iter()
            .filter_map(|u| match u {
                SceneUnit::Domain(d) => Some(*d),
                _ => None,
            })
            .collect();
        let engine = gpui_flow_engine::SceneEngine::new(scene).unwrap();
        let bounds = Bounds::new(Point::ZERO, Size::new(140., 120.));
        let key = super::composite::RequestKey::new(
            domains,
            &engine,
            RasterViewport {
                world_to_screen: Affine::IDENTITY,
                canvas_bounds: bounds,
                content_mask: bounds,
                device_scale: 1.25,
            },
            1,
        );
        let mut controller = super::composite::CompositeController::default();
        controller.request(Arc::new(key.clone()));
        assert!(matches!(
            controller.take_work().unwrap().run().remove(0).1,
            Err(CompositeError::TextBackendRequired(_))
        ));
        let mut controller = super::composite::CompositeController::default();
        controller.request(Arc::new(key.with_text_backend(Some(&bridge))));
        let frame = controller
            .take_work()
            .unwrap()
            .run()
            .remove(0)
            .1
            .unwrap()
            .unwrap();
        let ink = frame
            .premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 0)
            .count();
        assert!(ink > 50 && ink < (frame.width * frame.height) as usize / 2);
    }
}

#[test]
fn writing_mode_ellipsis_keeps_native_ligature_and_combining_clusters_with_real_decorated_ink() {
    let (app, bridge) = GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let system = WindowTextSystem::new(app.text_system());
    let content = "office e\u{301} ABCDEFGHIJKLMN";
    let native = shape_text(&system, &request(content), Hsla::default()).unwrap();
    let mut clusters: Vec<(usize, f32, usize)> = vec![];
    for glyph in native[0].runs().iter().flat_map(|run| &run.glyphs) {
        if let Some(last) = clusters.last_mut().filter(|last| last.0 == glyph.index) {
            last.2 += 1;
        } else {
            clusters.push((glyph.index, f32::from(glyph.position.x), 1));
        }
    }
    let combining = content.find("e\u{301}").unwrap();
    let keep = clusters
        .iter()
        .position(|cluster| cluster.0 == combining)
        .unwrap()
        + 1;
    let kept_glyphs: usize = clusters[..keep].iter().map(|cluster| cluster.2).sum();
    let spaces = clusters[..keep]
        .iter()
        .filter(|cluster| content[cluster.0..].starts_with(' '))
        .count();
    let marker_native = shape_text(&system, &request("..."), Hsla::default()).unwrap();
    let marker_glyphs: usize = marker_native[0]
        .runs()
        .iter()
        .map(|run| run.glyphs.len())
        .sum();
    for mode in [WritingMode::XReverse, WritingMode::Y, WritingMode::YReverse] {
        let mut r = raster_request(content);
        r.text.letter_spacing = 2.;
        r.text.word_spacing = 3.;
        r.text.para_indent = 4.;
        r.overflow = TextOverflow::Ellipsis;
        let mut marker_request = request("...");
        marker_request.letter_spacing = r.text.letter_spacing;
        let marker = shape_axis(&system, &marker_request, mode).unwrap();
        let marker_inline = if mode.is_vertical() {
            marker.metrics().height
        } else {
            marker.metrics().width
        };
        let retained_inline = r.text.para_indent
            + spaces as f32 * r.text.word_spacing
            + keep as f32 * r.text.letter_spacing
            + if mode.is_vertical() {
                keep as f32 * r.text.font_size
            } else {
                clusters[keep].1
            };
        let available = retained_inline + marker_inline + 0.01;
        r.bounds = if mode.is_vertical() {
            [5., 7., 80., available]
        } else {
            [5., 7., available, 80.]
        };
        r.clip = r.bounds;
        let plan = shape_axis_in_box(&system, &r.text, mode, r.overflow, r.bounds[2], r.bounds[3])
            .unwrap();
        assert_eq!(plan.metrics().lines, 1);
        let mut images = vec![];
        for decoration in [
            TextDecoration::None,
            TextDecoration::Underline,
            TextDecoration::Strikethrough,
            TextDecoration::UnderlineStrikethrough,
        ] {
            r.decoration = decoration;
            let out = bridge
                .rasterize_writing_mode(
                    &r,
                    mode,
                    bridge.epoch(),
                    &AtomicBool::new(false),
                    2_000_000,
                    &mut 1_000_000,
                )
                .unwrap()
                .unwrap();
            assert_eq!(
                out.glyphs,
                kept_glyphs + marker_glyphs,
                "{mode:?} {decoration:?}"
            );
            assert_eq!(out.rows, 1);
            assert!(
                out.premul_rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] > 0)
            );
            images.push(out.premul_rgba);
        }
        for image in &images[1..] {
            assert_ne!(
                image, &images[0],
                "{mode:?} decorations must paint actual ink"
            );
        }
        assert_ne!(images[1], images[2]);
        assert_ne!(images[1], images[3]);
        r.device_scale = 1.25;
        r.clip = [
            r.bounds[0] + 1.,
            r.bounds[1] + 2.,
            r.bounds[2] - 2.,
            r.bounds[3] - 4.,
        ];
        let clipped = bridge
            .rasterize_writing_mode(
                &r,
                mode,
                bridge.epoch(),
                &AtomicBool::new(false),
                2_000_000,
                &mut 1_000_000,
            )
            .unwrap()
            .unwrap();
        assert_eq!(clipped.glyphs, kept_glyphs + marker_glyphs);
        for (cancelled, budget, work, error) in [
            (true, 2_000_000, 1_000_000, "text snapshot superseded"),
            (false, 16, 1_000_000, "text working-set limit"),
            (false, 2_000_000, 0, "text pixel work limit"),
        ] {
            assert_eq!(
                bridge
                    .rasterize_writing_mode(
                        &r,
                        mode,
                        bridge.epoch(),
                        &AtomicBool::new(cancelled),
                        budget,
                        &mut { work }
                    )
                    .unwrap_err(),
                WritingModeError::Invalid(error.into())
            );
        }
    }
}

#[test]
fn writing_mode_narrow_ellipsis_preserves_native_marker_then_clips_and_marks_last_visible_column() {
    let (_app, bridge) =
        GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let paint = |r: &TextRasterRequest, mode| {
        bridge
            .rasterize_writing_mode(
                r,
                mode,
                bridge.epoch(),
                &AtomicBool::new(false),
                2_000_000,
                &mut 1_000_000,
            )
            .unwrap()
            .unwrap()
    };
    for mode in [WritingMode::XReverse, WritingMode::Y, WritingMode::YReverse] {
        let mut r = raster_request("AB");
        r.text.para_indent = 2.;
        r.overflow = TextOverflow::Ellipsis;
        r.decoration = TextDecoration::UnderlineStrikethrough;
        r.bounds = if mode.is_vertical() {
            [0., 0., 40., 8.]
        } else {
            [0., 0., 8., 40.]
        };
        r.clip = r.bounds;
        let ellipsis = paint(&r, mode);
        r.text.content = "...".into();
        r.overflow = TextOverflow::Clip;
        let marker = paint(&r, mode);
        assert_eq!(ellipsis.glyphs, marker.glyphs);
        assert_eq!(ellipsis.premul_rgba, marker.premul_rgba);
        r.text.content = "ABCD\nEF".into();
        r.text.para_indent = 0.;
        r.text.wrap_width = Some(36.);
        r.overflow = TextOverflow::Ellipsis;
        r.bounds = if mode.is_vertical() {
            [0., 0., 24., 36.]
        } else {
            [0., 0., 36., 24.]
        };
        r.clip = r.bounds;
        let wrapped = paint(&r, mode);
        assert_eq!(wrapped.rows, 1);
        assert!(wrapped.glyphs >= marker.glyphs);
        assert!(
            wrapped
                .premul_rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] > 0)
        );
    }
}

#[test]
fn writing_mode_fractional_exact_fit_ellipsis_matches_clip_layout_and_pixels() {
    let (app, bridge) = GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let system = WindowTextSystem::new(app.text_system());
    for mode in [WritingMode::XReverse, WritingMode::Y, WritingMode::YReverse] {
        for (paragraphs, line_height, para_spacing, wrap_width) in [
            (4, 24.1, 0., None),
            (7, 24.1, 0., None),
            (7, 16.8, 0.3, Some(36.)),
        ] {
            let content = std::iter::repeat_n("ABAB", paragraphs)
                .collect::<Vec<_>>()
                .join("\n");
            let mut r = raster_request(&content);
            r.text.line_height = line_height;
            r.text.para_spacing = para_spacing;
            r.text.wrap_width = wrap_width;
            r.decoration = TextDecoration::UnderlineStrikethrough;
            let source = shape_axis(&system, &r.text, mode).unwrap();
            r.bounds = [0., 0., source.metrics().width, source.metrics().height];
            r.clip = r.bounds;
            let ellipsis = shape_axis_in_box(
                &system,
                &r.text,
                mode,
                TextOverflow::Ellipsis,
                r.bounds[2],
                r.bounds[3],
            )
            .unwrap();
            assert_eq!(ellipsis.metrics(), source.metrics(), "{mode:?}");
            let paint = |r: &TextRasterRequest| {
                bridge
                    .rasterize_writing_mode(
                        r,
                        mode,
                        bridge.epoch(),
                        &AtomicBool::new(false),
                        2_000_000,
                        &mut 1_000_000,
                    )
                    .unwrap()
                    .unwrap()
            };
            let clip = paint(&r);
            r.overflow = TextOverflow::Ellipsis;
            let ellipsis = paint(&r);
            assert_eq!(ellipsis.rows, clip.rows, "{mode:?}");
            assert_eq!(ellipsis.glyphs, clip.glyphs, "{mode:?}");
            assert_eq!(ellipsis.premul_rgba, clip.premul_rgba, "{mode:?}");
        }
    }
}

#[test]
fn vertical_autoheight_uses_native_unwrapped_height_and_preserves_physical_width() {
    use gpui_flow_layout::{FlowLayout, TextSizing};
    use gpui_flow_scene::{DisplayObject, Point, Scene, Shape, Size, display};
    use std::sync::Arc;
    let app = gpui_kit::platform::headless();
    let backend = Arc::new(GpuiTextMeasurer::new(app.text_system()));
    for (scene_mode, mode) in [
        (display::TextWritingMode::Y, WritingMode::Y),
        (display::TextWritingMode::YReverse, WritingMode::YReverse),
    ] {
        for (content, expected_height, expected_cross) in [("", 18., 18.), ("AB\n", 36., 43.)] {
            let mut r = request(content);
            r.line_height = 18.;
            r.para_spacing = 7.;
            let native = backend.measure_writing_mode(&r, mode).unwrap();
            assert_eq!(native.height, expected_height);
            assert_eq!(native.width, expected_cross);
            let mut text = display::Text::new(content, Size::new(32., 100.));
            text.font_family = r.font_family;
            text.font_size = 18.;
            text.line_height = 1.;
            text.para_spacing = 7.;
            text.padding = [3., 5., 7., 11.];
            text.wrap = false;
            text.writing_mode = scene_mode;
            text.auto_size_align = true;
            text.align = display::TextAlign::Center;
            text.vertical_align = display::TextVerticalAlign::Bottom;
            let mut scene = Scene::new();
            scene
                .insert(DisplayObject::new("t", text), None, None)
                .unwrap();
            let mut layout = FlowLayout::default();
            layout.texts.insert("t".into(), TextSizing::AutoHeight);
            let measured = layout.with_text_measurer(backend.clone());
            measured.apply_to_scene(&mut scene).unwrap();
            let Shape::Text(text) = &scene.get(&"t".into()).unwrap().object.shape else {
                panic!()
            };
            assert_eq!(text.size, Size::new(32., expected_height + 10.));
            assert_eq!(
                text.layout_origin,
                Point::new(0., -expected_height / 2. - 3.)
            );
            assert_eq!(text.content_bounds().size, Size::new(16., native.height));
            assert_eq!(text.content, content);
            assert!(measured.arranged(&scene).unwrap().1.changed.is_empty());
        }
    }
}

#[test]
fn vertical_autowidth_uses_native_wrapped_columns_and_keeps_physical_height() {
    use gpui_flow_layout::{FlowLayout, TextSizing};
    use gpui_flow_scene::{DisplayObject, Point, Scene, Shape, Size, display};
    use std::sync::Arc;
    let app = gpui_kit::platform::headless();
    let backend = Arc::new(GpuiTextMeasurer::new(app.text_system()));
    for (scene_mode, mode) in [
        (display::TextWritingMode::Y, WritingMode::Y),
        (display::TextWritingMode::YReverse, WritingMode::YReverse),
    ] {
        for (content, wrap, expected_width, expected_height) in [
            ("", true, 18., 18.),
            ("AB\n", true, 43., 36.),
            ("ABAB\n", true, 61., 36.),
            ("ABAB\n", false, 43., 72.),
        ] {
            let mut r = request(content);
            r.line_height = 18.;
            r.para_spacing = 7.;
            r.wrap_width = wrap.then_some(36.);
            let native = backend.measure_writing_mode(&r, mode).unwrap();
            assert_eq!(
                (native.width, native.height),
                (expected_width, expected_height)
            );
            let mut text = display::Text::new(content, Size::new(100., 46.));
            text.font_family = r.font_family;
            text.font_size = 18.;
            text.line_height = 1.;
            text.para_spacing = 7.;
            text.padding = [3., 5., 7., 11.];
            text.wrap = wrap;
            text.writing_mode = scene_mode;
            text.auto_size_align = true;
            text.align = display::TextAlign::End;
            text.vertical_align = display::TextVerticalAlign::Middle;
            let mut scene = Scene::new();
            scene
                .insert(DisplayObject::new("t", text), None, None)
                .unwrap();
            let mut layout = FlowLayout::default();
            layout.texts.insert("t".into(), TextSizing::AutoWidth);
            let measured = layout.with_text_measurer(backend.clone());
            measured.apply_to_scene(&mut scene).unwrap();
            let Shape::Text(text) = &scene.get(&"t".into()).unwrap().object.shape else {
                panic!()
            };
            assert_eq!(text.size, Size::new(expected_width + 16., 46.));
            assert_eq!(
                text.layout_origin,
                Point::new(-expected_width / 2. - 11., 0.)
            );
            assert_eq!(text.content_bounds().size, Size::new(native.width, 36.));
            assert_eq!(text.content, content);
            assert!(measured.arranged(&scene).unwrap().1.changed.is_empty());
        }
    }
}

#[test]
fn vertical_baseline_matches_same_app_native_metrics_and_arranged_world_baselines() {
    use gpui_flow_layout::{
        AlignItems, AxisSizing, FlowDirection, FlowLayout, FlowStyle, TextSizing,
    };
    use gpui_flow_scene::{DisplayObject, Scene, Shape, Size, display};
    use gpui_flow_text::InlineAlign;
    use std::sync::Arc;
    let app = gpui_kit::platform::headless();
    let system = WindowTextSystem::new(app.text_system());
    let backend = Arc::new(GpuiTextMeasurer::new(app.text_system()));
    for (scene_mode, mode) in [
        (display::TextWritingMode::Y, WritingMode::Y),
        (display::TextWritingMode::YReverse, WritingMode::YReverse),
    ] {
        for sizing in [
            TextSizing::Natural,
            TextSizing::AutoHeight,
            TextSizing::AutoWidth,
        ] {
            for content in ["AB\nABCDE", "ABCDE\nAB", "\nAB", ""] {
                for (align, scene_align) in [
                    (InlineAlign::Start, display::TextAlign::Start),
                    (InlineAlign::Center, display::TextAlign::Center),
                    (InlineAlign::End, display::TextAlign::End),
                ] {
                    let mut r = request(content);
                    r.para_indent = if content.is_empty() { 0. } else { 4. };
                    r.letter_spacing = 2.;
                    r.para_spacing = 7.;
                    r.wrap_width = (sizing == TextSizing::AutoWidth).then_some(70.);
                    let metrics = backend.measure_writing_mode(&r, mode).unwrap();
                    let available = if sizing == TextSizing::AutoWidth {
                        70.
                    } else {
                        metrics.height
                    };
                    let mut native_request = r.clone();
                    native_request.wrap_width = None;
                    native_request.letter_spacing = 0.;
                    native_request.para_indent = 0.;
                    let native = shape_text(&system, &native_request, Hsla::default()).unwrap();
                    let first = &native[0].unwrapped_layout;
                    let cell = (r.font_size - f32::from(first.ascent) - f32::from(first.descent))
                        * 0.5
                        + f32::from(first.ascent);
                    let mut first_advance = r.para_indent;
                    let mut last_index = None;
                    let mut count = 0;
                    for glyph in first.runs.iter().flat_map(|run| &run.glyphs) {
                        if last_index == Some(glyph.index) {
                            continue;
                        }
                        let advance = r.font_size + r.letter_spacing;
                        if r.wrap_width
                            .is_some_and(|limit| first_advance + advance > limit)
                            && count > 0
                        {
                            break;
                        }
                        first_advance += advance;
                        last_index = Some(glyph.index);
                        count += 1;
                    }
                    if count == 0 {
                        first_advance = r.para_indent + r.font_size;
                    }
                    let slack = match (align, mode.is_reverse()) {
                        (InlineAlign::Center, _) => (available - first_advance) * 0.5,
                        (InlineAlign::Start, false) | (InlineAlign::End, true) => 0.,
                        _ => available - first_advance,
                    };
                    let mut expected = cell
                        + if mode.is_reverse() {
                            -r.para_indent - r.font_size
                        } else {
                            r.para_indent
                        };
                    if mode.is_reverse() {
                        expected += first_advance;
                    }
                    expected += slack;
                    assert_eq!(
                        backend
                            .first_baseline_writing_mode(&r, mode, available, align)
                            .unwrap(),
                        expected
                    );
                    assert_eq!(
                        shape_axis(&system, &r, mode)
                            .unwrap()
                            .first_baseline(available, align)
                            .unwrap(),
                        expected
                    );
                    for direction in [FlowDirection::Row, FlowDirection::RowReverse] {
                        let mut scene = Scene::new();
                        scene
                            .insert(
                                DisplayObject::new("p", display::Frame::new(500., 200.)),
                                None,
                                None,
                            )
                            .unwrap();
                        for id in ["vertical", "horizontal"] {
                            let vertical = id == "vertical";
                            let mut text = display::Text::new(
                                if vertical { content } else { "A" },
                                Size::new(140., 80.),
                            );
                            text.font_family = r.font_family.clone();
                            text.font_size = r.font_size;
                            text.line_height = r.line_height / r.font_size;
                            text.wrap = vertical && sizing == TextSizing::AutoWidth;
                            text.auto_size_align = true;
                            text.padding = [3., 5., 7., 11.];
                            if vertical {
                                text.writing_mode = scene_mode;
                                text.align = scene_align;
                                text.vertical_align = display::TextVerticalAlign::Middle;
                                text.para_indent = r.para_indent;
                                text.letter_spacing = r.letter_spacing;
                                text.para_spacing = r.para_spacing;
                            }
                            scene
                                .insert(DisplayObject::new(id, text), Some("p".into()), None)
                                .unwrap();
                        }
                        let mut layout = FlowLayout::default();
                        layout.containers.insert(
                            "p".into(),
                            FlowStyle {
                                direction,
                                align_items: AlignItems::Baseline,
                                height_sizing: AxisSizing::content(),
                                ..Default::default()
                            },
                        );
                        layout.texts.insert("vertical".into(), sizing);
                        layout
                            .texts
                            .insert("horizontal".into(), TextSizing::Natural);
                        let measured = layout.with_text_measurer(backend.clone());
                        measured.apply_to_scene(&mut scene).unwrap();
                        let world_baseline = |id: &str, baseline: f32| {
                            let object = &scene.get(&id.into()).unwrap().object;
                            let Shape::Text(text) = &object.shape else {
                                panic!()
                            };
                            object.transform.ty + text.content_bounds().origin.y + baseline
                        };
                        let horizontal = backend.first_baseline(&request("A")).unwrap();
                        assert!(
                            (world_baseline("vertical", expected)
                                - world_baseline("horizontal", horizontal))
                            .abs()
                                < 0.00002
                        );
                        assert!(measured.arranged(&scene).unwrap().1.changed.is_empty());
                    }
                }
            }
        }
    }
}

#[test]
fn vertical_first_baseline_places_real_first_cluster_at_horizontal_reference_pixels() {
    use gpui_flow_text::InlineAlign;
    let (app, bridge) = GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let system = WindowTextSystem::new(app.text_system());
    let backend = GpuiTextMeasurer::new(app.text_system());
    let native = shape_text(&system, &request("AB"), Hsla::default()).unwrap();
    let a = shape_text(&system, &request("A"), Hsla::default()).unwrap();
    let glyphs: Vec<_> = native[0]
        .runs()
        .iter()
        .flat_map(|run| &run.glyphs)
        .collect();
    assert_eq!(glyphs.len(), 2);
    assert_eq!(
        f32::from(glyphs[1].position.x),
        f32::from(a[0].unwrapped_layout.width)
    );
    let cell = (18.
        - f32::from(native[0].unwrapped_layout.ascent)
        - f32::from(native[0].unwrapped_layout.descent))
        * 0.5
        + f32::from(native[0].unwrapped_layout.ascent);
    for mode in [WritingMode::Y, WritingMode::YReverse] {
        for (align, native_align) in [
            (InlineAlign::Start, TextAlign::Left),
            (InlineAlign::Center, TextAlign::Center),
            (InlineAlign::End, TextAlign::Right),
        ] {
            let mut r = raster_request("AB\nABCDE");
            r.bounds = [10., 10., 60., 120.];
            r.text.para_indent = 4.;
            r.text.letter_spacing = 2.;
            r.align = native_align;
            let baseline = backend
                .first_baseline_writing_mode(&r.text, mode, 120., align)
                .unwrap();
            let cell_top = r.bounds[1] + baseline - cell;
            r.clip = [r.bounds[0], cell_top, r.text.line_height, r.text.font_size];
            let mut reference = r.clone();
            reference.text = request("A");
            reference.text.line_height = 18.;
            reference.bounds = r.clip;
            reference.align = TextAlign::Center;
            let paint = |request: &TextRasterRequest, mode| {
                bridge
                    .rasterize_writing_mode(
                        request,
                        mode,
                        bridge.epoch(),
                        &AtomicBool::new(false),
                        2_000_000,
                        &mut 1_000_000,
                    )
                    .unwrap()
                    .unwrap()
            };
            let actual = paint(&r, mode);
            let expected = paint(&reference, WritingMode::X);
            assert_eq!(
                (actual.origin, actual.width, actual.height),
                (expected.origin, expected.width, expected.height)
            );
            assert_eq!(actual.premul_rgba, expected.premul_rgba);
            assert!(
                actual
                    .premul_rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|pixel| pixel[3] != 0)
                    .count()
                    > 20
            );
        }
    }
}

#[test]
fn vertical_empty_first_paragraph_indent_rejection_is_atomic_with_real_fonts() {
    use gpui_flow_editor::SceneEditor;
    use gpui_flow_layout::{AlignItems, FlowLayout, FlowStyle, TextSizing};
    use gpui_flow_scene::{DisplayObject, Scene, Shape, Size, display};
    use std::sync::Arc;
    let app = gpui_kit::platform::headless();
    let backend = Arc::new(GpuiTextMeasurer::new(app.text_system()));
    for mode in [
        display::TextWritingMode::Y,
        display::TextWritingMode::YReverse,
    ] {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("p", display::Frame::new(200., 200.)),
                None,
                None,
            )
            .unwrap();
        let mut text = display::Text::new("\nAB", Size::new(80., 40.));
        text.font_family = "DejaVu Sans".into();
        text.font_size = 18.;
        text.writing_mode = mode;
        text.wrap = true;
        scene
            .insert(DisplayObject::new("t", text), Some("p".into()), None)
            .unwrap();
        let mut layout = FlowLayout::default();
        layout.containers.insert(
            "p".into(),
            FlowStyle {
                align_items: AlignItems::Baseline,
                ..Default::default()
            },
        );
        layout.texts.insert("t".into(), TextSizing::AutoWidth);
        let measured = layout.with_text_measurer(backend.clone());
        measured.apply_to_scene(&mut scene).unwrap();
        let mut editor = SceneEditor::new(scene.clone()).unwrap();
        measured.install_reactive(&mut editor.engine).unwrap();
        let revision = editor.engine.revision();
        assert!(
            editor
                .transaction(|s| s.update_object(&"t".into(), |object| {
                    let Shape::Text(text) = &mut object.shape else {
                        panic!()
                    };
                    text.para_indent = 100.;
                }))
                .unwrap_err()
                .to_string()
                .contains("content box")
        );
        assert_eq!(editor.scene(), &scene);
        assert_eq!(editor.engine.revision(), revision);
        assert!(!editor.undo());
    }
}

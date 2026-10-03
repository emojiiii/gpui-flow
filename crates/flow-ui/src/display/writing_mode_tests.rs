use gpui::{Hsla, TextAlign, WindowTextSystem};
use gpui_flow_text::{
    GpuiTextMeasurer, GpuiTextRasterizer, TextMeasureRequest, TextMeasurer, TextOverflow,
    TextRasterRequest, UnsupportedWritingMode, WritingMode, WritingModeError,
    gpui_backend::shape_text, gpui_writing_mode::shape_axis,
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
        let mut invalid = r.clone();
        invalid.overflow = TextOverflow::Ellipsis;
        assert_eq!(
            bridge
                .rasterize_writing_mode(
                    &invalid,
                    mode,
                    bridge.epoch(),
                    &AtomicBool::new(false),
                    2_000_000,
                    &mut 1_000_000
                )
                .unwrap_err(),
            WritingModeError::Unsupported(UnsupportedWritingMode::Ellipsis)
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

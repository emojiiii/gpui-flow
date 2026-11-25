use gpui::{Hsla, TextAlign, WindowTextSystem};
use gpui_flow_text::{
    GpuiTextRasterizer, TextDecoration as D, TextMeasureRequest, TextOverflow, TextRasterRequest,
    VerticalAlign,
    gpui_decoration::{DecorationRequest, layout_decorations},
    gpui_overflow::shape_text_in_box,
};
use std::sync::{Arc, atomic::AtomicBool};
fn request(decoration: D) -> TextRasterRequest {
    TextRasterRequest {
        text: TextMeasureRequest {
            content: "A A\nA A".into(),
            font_family: "DejaVu Sans".into(),
            font_size: 20.,
            font_weight: 400,
            italic: false,
            text_case: Default::default(),
            para_spacing: 0.,
            letter_spacing: 0.,
            word_spacing: 0.,
            para_indent: 0.,
            line_height: 28.,
            wrap_width: None,
        },
        align: TextAlign::Left,
        vertical_align: VerticalAlign::Top,
        overflow: TextOverflow::Clip,
        decoration,
        bounds: [4., 6., 160., 90.],
        clip: [4., 6., 160., 90.],
        device_scale: 1.,
        color: [255; 4],
    }
}
fn plan(r: &TextRasterRequest) -> gpui_flow_text::gpui_decoration::DecorationLayout {
    let app = gpui::Application::with_platform(crate::display::native_text_test_platform());
    let system = WindowTextSystem::new(app.text_system());
    let shaped = shape_text_in_box(
        &system,
        &r.text,
        Hsla::default(),
        r.overflow,
        r.bounds[2],
        r.bounds[3],
    )
    .unwrap();
    layout_decorations(
        &shaped,
        &DecorationRequest {
            decoration: r.decoration,
            font_size: r.text.font_size,
            line_height: r.text.line_height,
            bounds: r.bounds,
            align: r.align,
            vertical_align: r.vertical_align,
            device_scale: r.device_scale,
        },
        usize::MAX,
    )
    .unwrap()
}
fn raster(
    bridge: &Arc<GpuiTextRasterizer>,
    r: &TextRasterRequest,
) -> gpui_flow_text::RasterizedText {
    bridge
        .rasterize(
            r,
            bridge.epoch(),
            &AtomicBool::new(false),
            2_000_000,
            &mut 2_000_000,
        )
        .unwrap()
        .unwrap()
}
#[test]
fn decoration_rows_share_width_offsets_and_device_snapping() {
    let under = plan(&request(D::Underline));
    let strike = plan(&request(D::Strikethrough));
    let both = plan(&request(D::UnderlineStrikethrough));
    assert_eq!(under.rects.len(), 2);
    assert_eq!(strike.rects.len(), 2);
    assert_eq!(both.rects.len(), 4);
    assert_eq!(under.rects[1][1] - under.rects[0][1], 28.);
    assert_eq!(under.rects[0][1] - strike.rects[0][1], 10.);
    assert_eq!(under.rects[0][2], strike.rects[0][2]);
    let mut fractional = request(D::Underline);
    fractional.device_scale = 1.5;
    fractional.bounds[0] = 3.25;
    fractional.bounds[1] = 5.5;
    for r in plan(&fractional).rects {
        for value in r {
            assert!((value * 1.5 - (value * 1.5).round()).abs() < 0.001);
        }
    }
}
#[test]
fn decoration_alignment_and_overflow_follow_presented_rows() {
    let mut r = request(D::Underline);
    let first = plan(&r).rects[0];
    r.align = TextAlign::Right;
    let right = plan(&r).rects[0];
    assert_eq!(right[0] + right[2], 164.);
    r.align = TextAlign::Left;
    r.vertical_align = VerticalAlign::Bottom;
    assert_eq!(plan(&r).rects[0][1] - first[1], 34.);
    r.text.content = "One two three four five six seven".into();
    r.text.wrap_width = Some(110.);
    r.bounds[2] = 110.;
    r.bounds[3] = 40.;
    r.overflow = TextOverflow::Ellipsis;
    let bottom = plan(&r);
    r.vertical_align = VerticalAlign::Top;
    assert_eq!(bottom.rects, plan(&r).rects);
    assert_eq!(bottom.rects.len(), 1);
    assert!(bottom.rects[0][2] <= 110.);
}
#[test]
fn decoration_raster_adds_real_ink_and_empty_rows_stay_empty() {
    let (_app, bridge) =
        GpuiTextRasterizer::application(crate::display::native_text_test_platform());
    let plain = raster(&bridge, &request(D::None));
    let under = raster(&bridge, &request(D::Underline));
    let both = raster(&bridge, &request(D::UnderlineStrikethrough));
    let ink = |r: &gpui_flow_text::RasterizedText| {
        r.premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| u64::from(p[3]))
            .sum::<u64>()
    };
    assert!(ink(&under) > ink(&plain));
    assert!(ink(&both) > ink(&under));
    assert!(both.pixel_writes > plain.pixel_writes);
    let mut empty = request(D::UnderlineStrikethrough);
    empty.text.content.clear();
    assert!(plan(&empty).rects.is_empty());
    assert_eq!(ink(&raster(&bridge, &empty)), 0);
    empty.text.content = "   ".into();
    assert_eq!(plan(&empty).rects.len(), 2);
    assert!(ink(&raster(&bridge, &empty)) > 0);
}
#[test]
fn decoration_plan_memory_and_pixel_work_fail_without_a_partial_result() {
    let (app, bridge) =
        GpuiTextRasterizer::application(crate::display::native_text_test_platform());
    let r = request(D::UnderlineStrikethrough);
    let shaped = shape_text_in_box(
        &WindowTextSystem::new(app.text_system()),
        &r.text,
        Hsla::default(),
        r.overflow,
        r.bounds[2],
        r.bounds[3],
    )
    .unwrap();
    let spec = DecorationRequest {
        decoration: r.decoration,
        font_size: r.text.font_size,
        line_height: r.text.line_height,
        bounds: r.bounds,
        align: r.align,
        vertical_align: r.vertical_align,
        device_scale: r.device_scale,
    };
    assert!(layout_decorations(&shaped, &spec, 0).is_err());
    let good = raster(&bridge, &r);
    assert!(good.peak_bytes > good.premul_rgba.len());
    let mut short = good.pixel_writes - 1;
    assert!(
        bridge
            .rasterize(
                &r,
                bridge.epoch(),
                &AtomicBool::new(false),
                2_000_000,
                &mut short
            )
            .is_err()
    );
    assert!(
        bridge
            .rasterize(
                &r,
                bridge.epoch(),
                &AtomicBool::new(true),
                2_000_000,
                &mut 2_000_000
            )
            .is_err()
    );
}
#[test]
fn decoration_limits_apply_to_rows_and_glyphs_before_geometry_allocation() {
    let app = gpui::Application::with_platform(crate::display::native_text_test_platform());
    let system = WindowTextSystem::new(app.text_system());
    for text in ["\n".repeat(8192), "a".repeat(8193)] {
        let mut r = request(D::Underline);
        r.text.content = text;
        let shaped = shape_text_in_box(
            &system,
            &r.text,
            Hsla::default(),
            r.overflow,
            r.bounds[2],
            r.bounds[3],
        )
        .unwrap();
        assert!(
            layout_decorations(
                &shaped,
                &DecorationRequest {
                    decoration: r.decoration,
                    font_size: r.text.font_size,
                    line_height: r.text.line_height,
                    bounds: r.bounds,
                    align: r.align,
                    vertical_align: r.vertical_align,
                    device_scale: 1.
                },
                usize::MAX
            )
            .is_err()
        );
    }
}
#[test]
fn decoration_alpha_requires_a_domain_but_opaque_native_text_stays_fast() {
    use gpui_flow_composite::{CompositeLimits, SceneUnit, scene_units};
    use gpui_flow_scene::{Color, DisplayObject, Scene, Style, display};
    for mode in 0..3 {
        let mut s = Scene::new();
        let mut t = display::Text::new("decorated", gpui_flow_scene::Size::new(90., 50.));
        t.decoration = display::TextDecoration::Underline;
        s.insert(
            DisplayObject::new("t", t).with_style(Style {
                fill: Some(
                    Color {
                        r: 255,
                        g: 255,
                        b: 255,
                        a: if mode == 2 { 128 } else { 255 },
                    }
                    .into(),
                ),
                opacity: if mode == 1 { 0.5 } else { 1. },
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
        let units = scene_units(&s, CompositeLimits::default()).unwrap();
        assert_eq!(
            units.iter().any(|u| matches!(u, SceneUnit::Domain(_))),
            mode != 0
        );
        let list = s.paint_list().unwrap();
        let object =
            super::RenderObject::from_paint_item(&list[0], gpui_flow_core::Viewport::default());
        assert_eq!(object.native_renderable(), mode == 0);
    }
}

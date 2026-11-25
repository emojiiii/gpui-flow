use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, ContainerPaintOrder, DisplayObject, FillRule, GradientStop,
    MaskMode, Paint, Point, Scene, Shadow, Size, Stroke, StrokeAlignment, StrokeCap, StrokeJoin,
    Style, display,
};
use std::{
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds {
    Bounds::new(Point::new(x, y), Size::new(width, height))
}
fn request() -> SceneExportRequest {
    SceneExportRequest {
        world_bounds: bounds(0., 0., 20., 20.),
        scale: 1.,
        pixel_ratio: 1.,
        background: Color::TRANSPARENT,
        format: SceneExportFormat::Svg,
    }
}
fn plan(scene: &Scene) -> SceneExportPlan {
    SceneExportPlan::new(scene, request(), CompositeLimits::default()).unwrap()
}
fn encode(
    plan: &SceneExportPlan,
    images: &ImageInputs,
    texts: &TextInputs,
) -> Result<SceneExport, CompositeError> {
    plan.encode_with_inputs(
        images,
        texts,
        SceneExportPreparation::default(),
        &AtomicBool::new(false),
    )
}
fn svg(scene: &Scene) -> String {
    String::from_utf8(
        encode(
            &plan(scene),
            &ImageInputs::default(),
            &TextInputs::default(),
        )
        .unwrap()
        .bytes,
    )
    .unwrap()
}
fn xml(svg: &str) -> roxmltree::Document<'_> {
    roxmltree::Document::parse(svg).unwrap()
}
fn render(svg: &str) -> tiny_skia::Pixmap {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).unwrap();
    let size = tree.size().to_int_size();
    let mut output = tiny_skia::Pixmap::new(size.width(), size.height()).unwrap();
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut output.as_mut(),
    );
    output
}
fn at(image: &tiny_skia::Pixmap, x: u32, y: u32) -> [u8; 4] {
    let p = image.pixel(x, y).unwrap().demultiply();
    [p.red(), p.green(), p.blue(), p.alpha()]
}
fn rect(id: &str, x: f32, y: f32, width: f32, height: f32, color: Color) -> DisplayObject {
    DisplayObject::new(id, display::Rect::new(width, height))
        .with_transform(Affine::translation(x, y))
        .with_style(Style::fill(color))
}
fn sample_scene() -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            rect("red", 2., 3., 8., 6., Color::hex(0xff0000)),
            None,
            None,
        )
        .unwrap();
    scene
}
fn image(width: u32, height: u32, bgra: &[u8]) -> Arc<StaticImage> {
    Arc::new(
        StaticImage::from_straight_bgra(width, height, bgra, CompositeLimits::default()).unwrap(),
    )
}
fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut decoder = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut rgba = vec![0; decoder.output_buffer_size().unwrap()];
    let info = decoder.next_frame(&mut rgba).unwrap();
    rgba.truncate(info.buffer_size());
    (info.width, info.height, rgba)
}

#[test]
fn svg_is_vector_xml_and_renders_without_an_embedded_scene_bitmap() {
    let output = svg(&sample_scene());
    let document = xml(&output);
    let root = document.root_element();
    assert_eq!(root.tag_name().name(), "svg");
    assert_eq!(
        root.tag_name().namespace(),
        Some("http://www.w3.org/2000/svg")
    );
    assert_eq!(root.attribute("viewBox"), Some("0 0 20 20"));
    assert_eq!(root.attribute("width"), Some("20"));
    assert_eq!(SceneExportFormat::Svg.mime_type(), "image/svg+xml");
    assert!(!document.descendants().any(|n| n.has_tag_name("image")));
    let path = document
        .descendants()
        .find(|n| n.has_tag_name("path") && n.attribute("fill").is_some())
        .unwrap();
    let geometry = display::Path::from_svg(path.attribute("d").unwrap()).unwrap();
    assert_eq!(geometry.bounds(), Some(bounds(0., 0., 8., 6.)));
    let rendered = render(&output);
    assert_eq!(at(&rendered, 3, 4), [255, 0, 0, 255]);
    assert_eq!(at(&rendered, 11, 4), [0; 4]);
}

#[test]
fn svg_crop_scale_dpr_and_fractional_extent_do_not_stretch_geometry() {
    let plan = SceneExportPlan::new(
        &Scene::new(),
        SceneExportRequest {
            world_bounds: bounds(5., -2., 2.5, 1.5),
            background: Color::WHITE,
            ..request()
        },
        CompositeLimits::default(),
    )
    .unwrap();
    let output = String::from_utf8(
        encode(&plan, &ImageInputs::default(), &TextInputs::default())
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert_eq!(plan.pixel_size(), [3, 2]);
    assert_eq!(
        xml(&output).root_element().attribute("viewBox"),
        Some("0 0 3 2")
    );
    assert!(output.contains("matrix(1 0 0 1 -5 2)"));
    let p = render(&output);
    assert_eq!(at(&p, 0, 0), [255; 4]);
    assert!((120..=136).contains(&at(&p, 2, 0)[3]), "{:?}", at(&p, 2, 0));
    assert!((56..=72).contains(&at(&p, 2, 1)[3]), "{:?}", at(&p, 2, 1));
    let scaled = SceneExportPlan::new(
        &sample_scene(),
        SceneExportRequest {
            world_bounds: bounds(2., 3., 4., 3.),
            scale: 2.,
            pixel_ratio: 1.5,
            ..request()
        },
        CompositeLimits::default(),
    )
    .unwrap();
    assert_eq!(scaled.pixel_size(), [12, 9]);
    let s = String::from_utf8(
        encode(&scaled, &ImageInputs::default(), &TextInputs::default())
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert!(s.contains("matrix(3 0 0 3 -6 -9)"));
    assert_eq!(at(&render(&s), 11, 8), [255, 0, 0, 255]);
}

#[test]
fn svg_empty_scene_keeps_alpha_background_and_complete_extent() {
    for color in [
        Color::TRANSPARENT,
        Color::WHITE,
        Color::rgba(255, 0, 0, 128),
    ] {
        let p = SceneExportPlan::new(
            &Scene::new(),
            SceneExportRequest {
                background: color,
                ..request()
            },
            CompositeLimits::default(),
        )
        .unwrap();
        let s = String::from_utf8(
            encode(&p, &ImageInputs::default(), &TextInputs::default())
                .unwrap()
                .bytes,
        )
        .unwrap();
        let rgba = at(&render(&s), 10, 10);
        assert_eq!(rgba, [color.r, color.g, color.b, color.a]);
    }
}

#[test]
fn svg_hierarchy_local_affine_order_and_group_opacity_survive_readback() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("group", display::Group::new())
                .with_transform(Affine::translation(12., 2.) * Affine::rotation(90.))
                .with_style(Style {
                    opacity: 0.5,
                    ..Style::default()
                }),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            rect("back", 0., 0., 10., 8., Color::hex(0xff0000)),
            Some("group".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            rect("front", 2., 2., 6., 4., Color::hex(0x0000ff)),
            Some("group".into()),
            None,
        )
        .unwrap();
    let s = svg(&scene);
    let document = xml(&s);
    let group = document
        .descendants()
        .find(|n| n.attribute("data-flow-id") == Some("group"))
        .unwrap();
    assert_eq!(group.attribute("opacity"), Some("0.5"));
    let ids: Vec<_> = group
        .children()
        .filter_map(|n| n.attribute("data-flow-id"))
        .collect();
    assert_eq!(ids, ["back", "front"]);
    let output = render(&s);
    assert_eq!(at(&output, 8, 6), [0, 0, 255, 128]);
    assert_eq!(at(&output, 11, 3), [255, 0, 0, 128]);
}

#[test]
fn svg_rounded_container_clip_and_after_children_stroke_have_distinct_phases() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new(
                "frame",
                display::Frame::new(14., 14.)
                    .with_corners([4., 1., 3., 2.])
                    .with_paint_order(ContainerPaintOrder::StrokeAfterChildren),
            )
            .with_transform(Affine::translation(2., 2.))
            .with_style(Style {
                fill: Some(Color::WHITE.into()),
                stroke: Some(Stroke::new(Color::BLACK, 2.)),
                ..Style::default()
            }),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            rect("child", -5., -5., 30., 30., Color::hex(0xff0000)),
            Some("frame".into()),
            None,
        )
        .unwrap();
    let s = svg(&scene);
    let doc = xml(&s);
    assert!(
        doc.descendants()
            .filter(|n| n.has_tag_name("clipPath"))
            .count()
            >= 2
    );
    let output = render(&s);
    assert_eq!(at(&output, 9, 9), [255, 0, 0, 255]);
    assert_eq!(at(&output, 2, 8), [0, 0, 0, 255]);
    assert_eq!(at(&output, 0, 0), [0; 4]);
    assert_eq!(at(&output, 19, 8), [0; 4]);
}

#[test]
fn svg_gradients_layers_and_evenodd_holes_remain_vector() {
    let mut scene = Scene::new();
    let path = display::Path::from_svg("M0 0H18V18H0Z M6 6H12V12H6Z").unwrap();
    scene
        .insert(
            DisplayObject::new("gradient", path).with_style(Style {
                fill: Some(Paint::LinearGradient {
                    start: Point::ZERO,
                    end: Point::new(18., 0.),
                    stops: vec![
                        GradientStop {
                            offset: 0.,
                            color: Color::hex(0xff0000),
                        },
                        GradientStop {
                            offset: 1.,
                            color: Color::hex(0x0000ff),
                        },
                    ],
                }),
                fill_rule: FillRule::EvenOdd,
                ..Style::default()
            }),
            None,
            None,
        )
        .unwrap();
    let s = svg(&scene);
    let doc = xml(&s);
    let gradient = doc
        .descendants()
        .find(|n| n.has_tag_name("linearGradient"))
        .unwrap();
    assert_eq!(gradient.attribute("gradientUnits"), Some("userSpaceOnUse"));
    let out = render(&s);
    assert_eq!(at(&out, 9, 9), [0; 4]);
    assert!(at(&out, 2, 2)[0] > 180);
    assert!(at(&out, 16, 2)[2] > 180);
    assert!(!s.contains("data:image"));
}

#[test]
fn svg_radial_gradient_and_multiple_paints_preserve_alpha_order() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("layers", display::Rect::new(20., 20.)).with_style(Style {
                fill: Some(Color::hex(0x0000ff).into()),
                fill_layers: vec![Paint::RadialGradient {
                    center: Point::new(10., 10.),
                    radius: 8.,
                    stops: vec![
                        GradientStop {
                            offset: 0.,
                            color: Color::rgba(255, 0, 0, 128),
                        },
                        GradientStop {
                            offset: 1.,
                            color: Color::TRANSPARENT,
                        },
                    ],
                }],
                ..Style::default()
            }),
            None,
            None,
        )
        .unwrap();
    let s = svg(&scene);
    assert_eq!(
        xml(&s)
            .descendants()
            .filter(|n| n.has_tag_name("radialGradient"))
            .count(),
        1
    );
    let out = render(&s);
    assert_eq!(at(&out, 0, 0), [0, 0, 255, 255]);
    let center = at(&out, 10, 10);
    assert!(
        center[0] > 100 && center[2] < 155 && center[3] == 255,
        "{center:?}"
    );
}

#[test]
fn svg_path_pen_and_primitive_curves_roundtrip_canonical_geometry() {
    let primitives: Vec<gpui_flow_scene::Shape> = vec![
        display::Pen::new()
            .move_to(Point::new(1., 1.))
            .quadratic_to(Point::new(5., 8.), Point::new(9., 1.))
            .line_to(Point::new(1., 1.))
            .close()
            .into(),
        display::Rect::new(8., 6.)
            .with_corners([1., 2., 0., 3.])
            .into(),
        display::Ellipse::new(8., 8.).ring(0.5).into(),
        display::Ellipse::new(8., 8.).sector(15., 190.).into(),
        display::Star::new(5, 4., 2.).into(),
        display::Polygon::new(vec![Point::ZERO, Point::new(6., 0.), Point::new(3., 5.)]).into(),
        display::Path::from_svg("M1 1Q4 8 8 1C9 2 10 3 8 4Z")
            .unwrap()
            .into(),
    ];
    for shape in primitives {
        let mut scene = Scene::new();
        scene
            .insert(DisplayObject::new("primitive", shape.clone()), None, None)
            .unwrap();
        let s = svg(&scene);
        let doc = xml(&s);
        let data = doc
            .descendants()
            .find(|n| n.has_tag_name("path") && n.attribute("fill").is_some())
            .unwrap()
            .attribute("d")
            .unwrap();
        let path = display::Path::from_svg(data).unwrap();
        assert!(path.bounds().is_some());
        for p in [Point::new(2., 2.), Point::new(4., 4.), Point::new(19., 19.)] {
            let expected = shape.contains_geometry(p, FillRule::NonZero);
            let actual =
                gpui_flow_scene::Shape::Path(path.clone()).contains_geometry(p, FillRule::NonZero);
            assert_eq!(actual, expected, "{} {p:?}", shape.tag());
        }
    }
}

#[test]
fn svg_dashed_aligned_strokes_reuse_triangle_union_and_stay_vector() {
    let mut scene = Scene::new();
    let mut stroke = Stroke::new(Color::hex(0xff0000), 2.)
        .with_alignment(StrokeAlignment::Inside)
        .with_cap(StrokeCap::Butt)
        .with_join(StrokeJoin::Miter);
    stroke.dash_pattern = vec![3., 2.];
    scene
        .insert(
            DisplayObject::new("stroke", display::Rect::new(16., 16.)).with_style(Style {
                fill: None,
                stroke: Some(stroke),
                ..Style::default()
            }),
            None,
            None,
        )
        .unwrap();
    let s = svg(&scene);
    let out = render(&s);
    assert_eq!(at(&out, 1, 1), [255, 0, 0, 255]);
    assert_eq!(at(&out, 8, 8), [0; 4]);
    assert!(!s.contains("data:image"));
    assert!(s.contains("fill-rule=\"nonzero\""));
}

#[test]
fn svg_line_stroke_is_geometry_without_an_accidental_fill() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new(
                "line",
                display::Line::new(Point::new(2., 8.), Point::new(18., 8.)),
            )
            .with_style(Style::stroke(Color::BLACK, 4.)),
            None,
            None,
        )
        .unwrap();
    let out = render(&svg(&scene));
    assert_eq!(at(&out, 10, 8), [0, 0, 0, 255]);
    assert_eq!(at(&out, 10, 12), [0; 4]);
}

#[test]
fn svg_xml_escapes_identifiers_and_rejects_forbidden_characters_atomically() {
    let id = "引号 & <tag> \" ' \n\t\r";
    let mut scene = Scene::new();
    scene
        .insert(rect(id, 0., 0., 4., 4., Color::BLACK), None, None)
        .unwrap();
    let s = svg(&scene);
    let doc = xml(&s);
    assert_eq!(
        doc.descendants().find_map(|n| n.attribute("data-flow-id")),
        Some(id)
    );
    assert!(!doc.descendants().any(|n| n.has_tag_name("tag")));
    for id in ["bad\0id", "bad\u{1f}id", "bad\u{fffe}id"] {
        let mut scene = Scene::new();
        scene
            .insert(rect(id, 0., 0., 4., 4., Color::BLACK), None, None)
            .unwrap();
        let before = scene.to_json().unwrap();
        assert!(matches!(
            encode(
                &plan(&scene),
                &ImageInputs::default(),
                &TextInputs::default()
            ),
            Err(CompositeError::InvalidGeometry(_))
        ));
        assert_eq!(scene.to_json().unwrap(), before);
    }
}

#[test]
fn svg_embeds_decoded_image_only_without_resource_url_or_credentials() {
    let mut scene = sample_scene();
    scene
        .insert(
            DisplayObject::new(
                "image",
                display::Image::new(
                    "https://user:secret@example.invalid/image?token=secret",
                    Size::new(4., 2.),
                ),
            )
            .with_transform(Affine::translation(12., 2.)),
            None,
            None,
        )
        .unwrap();
    let mut images = ImageInputs::default();
    images.insert(
        "image".into(),
        Ok(image(2, 1, &[0, 0, 255, 255, 255, 0, 0, 255])),
    );
    let s = String::from_utf8(
        encode(&plan(&scene), &images, &TextInputs::default())
            .unwrap()
            .bytes,
    )
    .unwrap();
    let doc = xml(&s);
    let node = doc.descendants().find(|n| n.has_tag_name("image")).unwrap();
    let href = node
        .attribute(("http://www.w3.org/1999/xlink", "href"))
        .unwrap();
    let png = STANDARD
        .decode(href.strip_prefix("data:image/png;base64,").unwrap())
        .unwrap();
    assert_eq!(
        decode_png(&png),
        (2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255])
    );
    assert!(!s.contains("secret") && !s.contains("example.invalid"));
    let out = render(&s);
    assert!(at(&out, 12, 2)[0] > 180);
    assert!(at(&out, 15, 2)[2] > 180);
    assert_eq!(at(&out, 3, 4), [255, 0, 0, 255]);
}

#[test]
fn svg_image_source_crop_and_rounded_box_clip_use_existing_mapping() {
    let mut scene = Scene::new();
    let mut shape = display::Image::new("private", Size::new(12., 12.)).rounded(4.);
    shape.source_crop = Some(bounds(0.5, 0., 0.5, 1.));
    shape.fit = display::ImageFit::Fill;
    scene
        .insert(
            DisplayObject::new("image", shape).with_transform(Affine::translation(2., 2.)),
            None,
            None,
        )
        .unwrap();
    let mut images = ImageInputs::default();
    images.insert(
        "image".into(),
        Ok(image(2, 1, &[0, 0, 255, 255, 255, 0, 0, 255])),
    );
    let s = String::from_utf8(
        encode(&plan(&scene), &images, &TextInputs::default())
            .unwrap()
            .bytes,
    )
    .unwrap();
    let out = render(&s);
    assert_eq!(at(&out, 11, 8), [0, 0, 255, 255]);
    assert_eq!(at(&out, 2, 2), [0; 4]);
    assert_eq!(at(&out, 15, 8), [0; 4]);
}

#[test]
fn svg_canvas_pixels_and_paints_are_distinct_clipped_contributors() {
    let mut scene = Scene::new();
    let mut canvas = display::Canvas::new(10., 10.).rounded(3.);
    canvas.smooth = false;
    scene
        .insert(
            DisplayObject::new("canvas", canvas).with_style(Style::fill(Color::hex(0x0000ff))),
            None,
            None,
        )
        .unwrap();
    let mut images = ImageInputs::default();
    images.insert("canvas".into(), Ok(image(1, 1, &[0, 0, 255, 128])));
    let s = String::from_utf8(
        encode(&plan(&scene), &images, &TextInputs::default())
            .unwrap()
            .bytes,
    )
    .unwrap();
    let doc = xml(&s);
    let image = doc.descendants().find(|n| n.has_tag_name("image")).unwrap();
    assert_eq!(image.attribute("image-rendering"), Some("optimizeSpeed"));
    assert_eq!(image.attribute("data-flow-raster"), Some("canvas"));
    assert_eq!(at(&render(&s), 5, 5), [128, 0, 127, 255]);
}

#[test]
fn svg_text_is_explicit_host_ink_not_substituted_fonts_or_a_layout_rectangle() {
    let mut scene = sample_scene();
    scene
        .insert(
            DisplayObject::new(
                "text",
                display::Text::new("private source & <text>", Size::new(8., 8.)),
            )
            .with_transform(Affine::translation(10., 8.)),
            None,
            None,
        )
        .unwrap();
    let pixels = image(2, 2, &[0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255]);
    let mut text = TextInputs::default();
    text.insert_clipped(
        "text".into(),
        Ok(Some(Arc::new(TextImage {
            image: pixels,
            local_bounds: bounds(0., 0., 8., 8.),
        }))),
        bounds(0., 0., 6., 8.),
    );
    let s = String::from_utf8(
        encode(&plan(&scene), &ImageInputs::default(), &text)
            .unwrap()
            .bytes,
    )
    .unwrap();
    let doc = xml(&s);
    assert!(!doc.descendants().any(|n| n.has_tag_name("text")));
    assert_eq!(
        doc.descendants()
            .filter(|n| n.attribute("data-flow-raster") == Some("text-ink"))
            .count(),
        1
    );
    assert!(!s.contains("private source") && !s.contains("font-family"));
    let out = render(&s);
    assert!(at(&out, 11, 9)[3] > 200);
    assert_eq!(at(&out, 16, 14), [0; 4]);
    assert!(at(&out, 13, 9)[3] < 255);
}

#[test]
fn svg_requires_current_image_canvas_and_text_inputs_even_outside_crop() {
    let shapes: [gpui_flow_scene::Shape; 3] = [
        display::Image::new("private", Size::new(2., 2.)).into(),
        display::Canvas::new(2., 2.).into(),
        display::Text::new("T", Size::new(2., 2.)).into(),
    ];
    for shape in shapes {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("missing", shape)
                    .with_transform(Affine::translation(500., 500.)),
                None,
                None,
            )
            .unwrap();
        let error = encode(
            &plan(&scene),
            &ImageInputs::default(),
            &TextInputs::default(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            CompositeError::ImagePending(_) | CompositeError::TextBackendRequired(_)
        ));
    }
}

#[test]
fn svg_failed_resources_and_bad_ink_geometry_reject_without_mutation() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("text", display::Text::new("T", Size::new(2., 2.))),
            None,
            None,
        )
        .unwrap();
    let before = scene.to_json().unwrap();
    let mut texts = TextInputs::default();
    texts.insert("text".into(), Err(CompositeError::Superseded));
    assert_eq!(
        encode(&plan(&scene), &ImageInputs::default(), &texts).unwrap_err(),
        CompositeError::Superseded
    );
    texts.insert(
        "text".into(),
        Ok(Some(Arc::new(TextImage {
            image: image(1, 1, &[0; 4]),
            local_bounds: bounds(f32::NAN, 0., 1., 1.),
        }))),
    );
    assert!(matches!(
        encode(&plan(&scene), &ImageInputs::default(), &texts),
        Err(CompositeError::InvalidGeometry(_))
    ));
    assert_eq!(scene.to_json().unwrap(), before);
}

#[test]
fn svg_hidden_producers_and_explicit_empty_text_do_not_require_pixels() {
    let mut scene = Scene::new();
    let mut hidden =
        DisplayObject::new("hidden", display::Image::new("private", Size::new(2., 2.)));
    hidden.visible = false;
    scene.insert(hidden, None, None).unwrap();
    scene
        .insert(
            DisplayObject::new("empty", display::Text::new("", Size::new(2., 2.))),
            None,
            None,
        )
        .unwrap();
    let mut texts = TextInputs::default();
    texts.insert("empty".into(), Ok(None));
    let s = String::from_utf8(
        encode(&plan(&scene), &ImageInputs::default(), &texts)
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert!(!s.contains("<image"));
    assert!(render(&s).data().iter().all(|b| *b == 0));
}

#[test]
fn svg_unsupported_effects_and_hidden_mask_markers_reject_at_plan_admission() {
    let styles = [
        Style {
            blend_mode: BlendMode::Multiply,
            ..Style::default()
        },
        Style {
            shadow: Some(Shadow::new(1., 1., 1., Color::BLACK)),
            ..Style::default()
        },
        Style {
            inner_shadow_layers: vec![Shadow::new(1., 1., 1., Color::BLACK)],
            ..Style::default()
        },
    ];
    for style in styles {
        let mut scene = Scene::new();
        scene
            .insert(
                rect("effect", 0., 0., 4., 4., Color::BLACK).with_style(style),
                None,
                None,
            )
            .unwrap();
        assert!(SceneExportPlan::new(&scene, request(), CompositeLimits::default()).is_err());
    }
    let mut scene = Scene::new();
    let mut mask = rect("mask", 0., 0., 4., 4., Color::BLACK).with_mask(MaskMode::Alpha);
    mask.visible = false;
    scene
        .insert(
            DisplayObject::new("masks", display::Group::new()),
            None,
            None,
        )
        .unwrap();
    scene.insert(mask, Some("masks".into()), None).unwrap();
    assert!(matches!(
        SceneExportPlan::new(&scene, request(), CompositeLimits::default()),
        Err(CompositeError::UnsupportedShape(_))
    ));
}

#[test]
fn svg_snapshot_is_repeatable_and_scene_json_is_immutable_on_success_and_failure() {
    let mut scene = sample_scene();
    let before = scene.to_json().unwrap();
    let p = plan(&scene);
    let a = encode(&p, &ImageInputs::default(), &TextInputs::default()).unwrap();
    assert_eq!(before, scene.to_json().unwrap());
    scene
        .insert(rect("later", 0., 0., 20., 20., Color::WHITE), None, None)
        .unwrap();
    assert_eq!(
        a.bytes,
        encode(&p, &ImageInputs::default(), &TextInputs::default())
            .unwrap()
            .bytes
    );
    assert_ne!(a.bytes, svg(&scene).as_bytes());
}

#[test]
fn svg_format_has_no_full_frame_raster_or_fixed_codec_reservation_for_vector_only() {
    let p = SceneExportPlan::new(
        &Scene::new(),
        request(),
        CompositeLimits {
            max_surface_bytes: 2048,
            ..CompositeLimits::default()
        },
    )
    .unwrap();
    let r = p.text_request(&ImageInputs::default(), 0).unwrap();
    assert_eq!(r.retained_bytes, 0);
    assert_eq!(r.viewport, p.viewport());
    assert!(encode(&p, &ImageInputs::default(), &TextInputs::default()).is_ok());
}

#[test]
fn svg_work_output_geometry_retained_and_embedded_codec_budgets_are_atomic() {
    let empty = Scene::new();
    for (limits, expected) in [
        (
            CompositeLimits {
                max_surface_bytes: 128,
                ..CompositeLimits::default()
            },
            CompositeError::WorkingSetLimit,
        ),
        (
            CompositeLimits {
                max_surface_writes: 8,
                ..CompositeLimits::default()
            },
            CompositeError::WorkLimit,
        ),
    ] {
        let p = SceneExportPlan::new(&empty, request(), limits).unwrap();
        assert_eq!(
            encode(&p, &ImageInputs::default(), &TextInputs::default()).unwrap_err(),
            expected
        );
    }
    let p = SceneExportPlan::new(
        &sample_scene(),
        request(),
        CompositeLimits {
            max_total_points: 2,
            ..CompositeLimits::default()
        },
    )
    .unwrap();
    assert_eq!(
        encode(&p, &ImageInputs::default(), &TextInputs::default()).unwrap_err(),
        CompositeError::InputLimit
    );
    let p = plan(&empty);
    assert_eq!(
        p.encode_with_inputs(
            &ImageInputs::default(),
            &TextInputs::default(),
            SceneExportPreparation {
                retained_bytes: CompositeLimits::default().max_surface_bytes,
                pixel_work: 0
            },
            &AtomicBool::new(false)
        )
        .unwrap_err(),
        CompositeError::WorkingSetLimit
    );
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("image", display::Image::new("private", Size::new(2., 2.))),
            None,
            None,
        )
        .unwrap();
    let p = SceneExportPlan::new(
        &scene,
        request(),
        CompositeLimits {
            max_surface_bytes: 1024 * 1024,
            ..CompositeLimits::default()
        },
    )
    .unwrap();
    let mut images = ImageInputs::default();
    images.insert("image".into(), Ok(image(1, 1, &[0; 4])));
    assert_eq!(
        encode(&p, &images, &TextInputs::default()).unwrap_err(),
        CompositeError::WorkingSetLimit
    );
}

#[test]
fn svg_combines_host_pixel_work_with_serialization_and_png_work() {
    let p = plan(&Scene::new());
    assert_eq!(
        p.encode_with_inputs(
            &ImageInputs::default(),
            &TextInputs::default(),
            SceneExportPreparation {
                pixel_work: p.limits().max_surface_writes,
                retained_bytes: 0
            },
            &AtomicBool::new(false)
        )
        .unwrap_err(),
        CompositeError::WorkLimit
    );
}

#[test]
fn svg_cancel_before_encode_and_after_text_preparation_publishes_nothing() {
    let p = plan(&sample_scene());
    let cancel = AtomicBool::new(true);
    assert_eq!(
        p.encode_with_inputs(
            &ImageInputs::default(),
            &TextInputs::default(),
            SceneExportPreparation::default(),
            &cancel
        )
        .unwrap_err(),
        CompositeError::Superseded
    );
    cancel.store(false, Ordering::Relaxed);
    let error = p
        .encode_with_text(&ImageInputs::default(), 0, &cancel, |_| {
            cancel.store(true, Ordering::Relaxed);
            Ok(CanvasTextOutput {
                inputs: TextInputs::default(),
                pixel_work: 0,
            })
        })
        .unwrap_err();
    assert_eq!(error, CompositeError::Superseded);
}

#[test]
fn svg_rejects_unsafe_crop_scale_and_geometry_without_nonfinite_xml() {
    for r in [
        SceneExportRequest {
            scale: f32::NAN,
            ..request()
        },
        SceneExportRequest {
            pixel_ratio: f32::INFINITY,
            ..request()
        },
        SceneExportRequest {
            world_bounds: bounds(0., 0., 0., 1.),
            ..request()
        },
    ] {
        assert!(SceneExportPlan::new(&Scene::new(), r, CompositeLimits::default()).is_err());
    }
    let mut scene = Scene::new();
    scene
        .insert(
            rect("large", 0., 0., 1_000_001., 2., Color::BLACK),
            None,
            None,
        )
        .unwrap();
    assert!(matches!(
        encode(
            &plan(&scene),
            &ImageInputs::default(),
            &TextInputs::default()
        ),
        Err(CompositeError::InvalidGeometry(_))
    ));
}

#[test]
fn svg_root_z_order_is_stable_and_ignores_names_and_arbitrary_metadata() {
    let mut scene = Scene::new();
    let mut first = rect("first", 0., 0., 10., 10., Color::hex(0xff0000));
    first.z_index = 5;
    first.name = "unrelated private name".into();
    first.data = serde_json::json!({"credentials": "private metadata"});
    scene.insert(first, None, None).unwrap();
    scene
        .insert(
            rect("second", 0., 0., 10., 10., Color::hex(0x0000ff)),
            None,
            None,
        )
        .unwrap();
    let s = svg(&scene);
    let doc = xml(&s);
    let ids: Vec<_> = doc
        .descendants()
        .filter_map(|n| n.attribute("data-flow-id"))
        .collect();
    assert_eq!(ids, ["second", "first"]);
    assert_eq!(at(&render(&s), 5, 5), [255, 0, 0, 255]);
    assert!(!s.contains("private") && !s.contains("credentials"));
}

#[test]
fn svg_failed_image_and_custom_producer_never_fall_back_to_geometry() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("image", display::Image::new("private", Size::new(2., 2.))),
            None,
            None,
        )
        .unwrap();
    let mut images = ImageInputs::default();
    let failure = CompositeError::ImageUnavailable {
        image: "image".into(),
        reason: "failed current frame".into(),
    };
    images.insert("image".into(), Err(failure.clone()));
    assert_eq!(
        encode(&plan(&scene), &images, &TextInputs::default()).unwrap_err(),
        failure
    );
    let mut custom = Scene::new();
    custom
        .insert(
            DisplayObject::new(
                "custom",
                display::Custom::new("test", bounds(0., 0., 4., 4.), vec![]),
            ),
            None,
            None,
        )
        .unwrap();
    assert!(matches!(
        SceneExportPlan::new(&custom, request(), CompositeLimits::default()),
        Err(CompositeError::UnsupportedCustomExport(_))
    ));
}

#[test]
fn svg_stroke_triangle_and_scene_node_budgets_remain_enforced() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("stroke", display::Rect::new(8., 8.))
                .with_style(Style::stroke(Color::BLACK, 2.)),
            None,
            None,
        )
        .unwrap();
    let p = SceneExportPlan::new(
        &scene,
        request(),
        CompositeLimits {
            max_triangles: 0,
            ..CompositeLimits::default()
        },
    )
    .unwrap();
    assert_eq!(
        encode(&p, &ImageInputs::default(), &TextInputs::default()).unwrap_err(),
        CompositeError::InputLimit
    );
    assert!(matches!(
        SceneExportPlan::new(
            &scene,
            request(),
            CompositeLimits {
                max_nodes: 1,
                ..CompositeLimits::default()
            }
        ),
        Err(CompositeError::InputLimit)
    ));
}

#[test]
fn svg_open_aligned_strokes_remain_vectors_without_implicit_closure_ink() {
    for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("open", display::Path::from_svg("M4 4H16V16H4").unwrap())
                    .with_style(Style {
                        fill: None,
                        stroke: Some(
                            Stroke::new(Color::rgba(255, 0, 0, 128), 2.)
                                .with_alignment(alignment)
                                .with_cap(StrokeCap::Butt)
                                .with_join(StrokeJoin::Miter),
                        ),
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
        let output = svg(&scene);
        let document = xml(&output);
        assert!(!document.descendants().any(|n| n.has_tag_name("image")));
        assert!(
            document
                .descendants()
                .any(|n| n.has_tag_name("path") && n.attribute("fill-rule") == Some("nonzero"))
        );
        let out = render(&output);
        let outside = alignment == StrokeAlignment::Outside;
        assert_eq!(at(&out, 9, if outside { 2 } else { 5 }), [255, 0, 0, 128]);
        assert_eq!(at(&out, 9, if outside { 5 } else { 2 }), [0; 4]);
        for x in [3, 4, 5] {
            assert_eq!(at(&out, x, 10), [0; 4]);
        }
        assert_eq!(
            at(&out, if outside { 17 } else { 15 }, 10),
            [255, 0, 0, 128]
        );
    }
}

#[test]
fn svg_degenerate_open_alignment_uses_empty_inside_and_single_alpha_outside() {
    for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            let mut scene = Scene::new();
            scene
                .insert(
                    DisplayObject::new("line", display::Path::from_svg("M4 10H16H4H16").unwrap())
                        .with_style(Style {
                            fill: None,
                            fill_rule: rule,
                            stroke: Some(
                                Stroke::new(Color::rgba(255, 0, 0, 128), 2.)
                                    .with_alignment(alignment)
                                    .with_cap(StrokeCap::Butt),
                            ),
                            ..Default::default()
                        }),
                    None,
                    None,
                )
                .unwrap();
            let output = svg(&scene);
            assert!(!xml(&output).descendants().any(|n| n.has_tag_name("image")));
            let out = render(&output);
            let expected = if alignment == StrokeAlignment::Outside {
                [255, 0, 0, 128]
            } else {
                [0; 4]
            };
            assert_eq!(at(&out, 10, 10), expected);
            assert_eq!(at(&out, 10, 8), expected);
            assert_eq!(at(&out, 10, 13), [0; 4]);
        }
    }
}

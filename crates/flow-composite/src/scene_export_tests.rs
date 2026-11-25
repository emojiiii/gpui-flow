use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, DisplayObject, MaskMode, Point, Scene, Size, Style, display,
};
use std::{
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds {
    Bounds::new(Point::new(x, y), Size::new(w, h))
}
fn request() -> SceneExportRequest {
    SceneExportRequest {
        world_bounds: bounds(0., 0., 8., 6.),
        scale: 1.,
        pixel_ratio: 1.,
        background: Color::TRANSPARENT,
        format: SceneExportFormat::Png,
    }
}
fn plan(scene: &Scene, request: SceneExportRequest) -> SceneExportPlan {
    SceneExportPlan::new(scene, request, CompositeLimits::default()).unwrap()
}
fn encode(plan: &SceneExportPlan) -> Result<SceneExport, CompositeError> {
    plan.encode_with_inputs(
        &ImageInputs::default(),
        &TextInputs::default(),
        SceneExportPreparation::default(),
        &AtomicBool::new(false),
    )
}
fn png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut decoder = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut rgba = vec![0; decoder.output_buffer_size().unwrap()];
    let info = decoder.next_frame(&mut rgba).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    rgba.truncate(info.buffer_size());
    (info.width, info.height, rgba)
}
fn pixels(plan: &SceneExportPlan) -> Vec<u8> {
    png(&encode(plan).unwrap().bytes).2
}
fn at(rgba: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    rgba[((y * w + x) * 4) as usize..][..4].try_into().unwrap()
}
fn rect(id: &str, x: f32, y: f32, w: f32, h: f32, color: Color) -> DisplayObject {
    DisplayObject::new(id, display::Rect::new(w, h))
        .with_transform(Affine::translation(x, y))
        .with_style(Style::fill(color))
}
fn scene() -> Scene {
    let mut s = Scene::new();
    s.insert(
        rect("red", 1., 1., 3., 2., Color::hex(0xff0000)),
        None,
        None,
    )
    .unwrap();
    s
}

#[test]
fn scene_png_has_fixed_extent_world_crop_scale_and_dpr() {
    let s = scene();
    let p = plan(
        &s,
        SceneExportRequest {
            world_bounds: bounds(1., 1., 4., 3.),
            scale: 2.,
            pixel_ratio: 1.5,
            ..request()
        },
    );
    assert_eq!(p.pixel_size(), [12, 9]);
    assert_eq!(
        p.viewport()
            .world_to_screen
            .transform_point(Point::new(1., 1.)),
        Point::ZERO
    );
    assert_eq!(p.viewport().device_scale, 1.);
    let result = encode(&p).unwrap();
    assert_eq!((result.width, result.height), (12, 9));
    let (w, h, rgba) = png(&result.bytes);
    assert_eq!((w, h), (12, 9));
    assert_eq!(at(&rgba, w, 0, 0), [255, 0, 0, 255]);
    assert_eq!(at(&rgba, w, 8, 5), [255, 0, 0, 255]);
    assert_eq!(at(&rgba, w, 9, 5), [0; 4]);
    assert_eq!(at(&rgba, w, 0, 6), [0; 4]);
}
#[test]
fn empty_scene_preserves_explicit_alpha_and_does_not_autotrim() {
    for color in [
        Color::TRANSPARENT,
        Color::rgba(255, 0, 0, 128),
        Color::WHITE,
    ] {
        let p = plan(
            &Scene::new(),
            SceneExportRequest {
                background: color,
                ..request()
            },
        );
        let rgba = pixels(&p);
        assert_eq!(rgba.len(), 8 * 6 * 4);
        assert!(
            rgba.as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [color.r, color.g, color.b, color.a])
        );
    }
}
#[test]
fn fractional_crop_has_ceil_dimensions_and_exact_edge_coverage() {
    let p = plan(
        &Scene::new(),
        SceneExportRequest {
            world_bounds: bounds(-2., -1., 2.5, 1.5),
            background: Color::hex(0xff0000),
            ..request()
        },
    );
    assert_eq!(p.pixel_size(), [3, 2]);
    let rgba = pixels(&p);
    assert_eq!(at(&rgba, 3, 0, 0), [255, 0, 0, 255]);
    assert!((120..=136).contains(&at(&rgba, 3, 2, 0)[3]));
    assert!((56..=72).contains(&at(&rgba, 3, 2, 1)[3]));
}
#[test]
fn scene_snapshot_and_encode_leave_canonical_state_unchanged() {
    let mut s = scene();
    let json = s.to_json().unwrap();
    let p = plan(&s, request());
    let old = encode(&p).unwrap().bytes;
    assert_eq!(s.to_json().unwrap(), json);
    s.insert(rect("new", 0., 0., 8., 6., Color::WHITE), None, None)
        .unwrap();
    assert_eq!(encode(&p).unwrap().bytes, old);
    assert_ne!(encode(&plan(&s, request())).unwrap().bytes, old);
}
#[test]
fn scene_export_keeps_opaque_only_public_owned_canvas_contract() {
    assert_eq!(
        owned_canvas(
            &Scene::new(),
            CanvasBackdrop {
                color: Color::TRANSPARENT,
                grid: None
            },
            CompositeLimits::default()
        )
        .unwrap_err(),
        CompositeError::UnsupportedExternalBackdrop
    );
    assert!(encode(&plan(&Scene::new(), request())).is_ok());
}
#[test]
fn roots_blend_against_explicit_background_and_keep_z_order() {
    let mut s = Scene::new();
    let mut red = rect("red", 0., 0., 8., 6., Color::hex(0xff0000));
    red.style.blend_mode = BlendMode::Multiply;
    s.insert(red, None, None).unwrap();
    assert_eq!(at(&pixels(&plan(&s, request())), 8, 2, 2), [255, 0, 0, 255]);
    assert_eq!(
        at(
            &pixels(&plan(
                &s,
                SceneExportRequest {
                    background: Color::hex(0x00ffff),
                    ..request()
                }
            )),
            8,
            2,
            2
        ),
        [0, 0, 0, 255]
    );
    let mut blue = rect("blue", 0., 0., 8., 6., Color::hex(0x0000ff));
    blue.z_index = -1;
    s.insert(blue, None, None).unwrap();
    assert_eq!(at(&pixels(&plan(&s, request())), 8, 2, 2), [0, 0, 0, 255]);
}
#[test]
fn existing_group_opacity_mask_and_parent_clip_are_used() {
    let mut s = Scene::new();
    let mut frame = DisplayObject::new("frame", display::Frame::new(4., 4.));
    frame.style.opacity = 0.5;
    frame.style.fill = None;
    s.insert(frame, None, None).unwrap();
    s.insert(
        DisplayObject::new("group", display::Group::new()),
        Some("frame".into()),
        None,
    )
    .unwrap();
    let mut mask = rect("mask", 0., 0., 2., 4., Color::WHITE);
    mask.mask = MaskMode::Alpha;
    s.insert(mask, Some("group".into()), None).unwrap();
    s.insert(
        rect("red", 0., 0., 6., 6., Color::hex(0xff0000)),
        Some("group".into()),
        None,
    )
    .unwrap();
    let rgba = pixels(&plan(&s, request()));
    assert_eq!(at(&rgba, 8, 1, 1), [255, 0, 0, 128]);
    assert_eq!(at(&rgba, 8, 3, 1), [0; 4]);
    assert_eq!(at(&rgba, 8, 1, 5), [0; 4]);
}
#[test]
fn missing_image_even_outside_crop_rejects_whole_export() {
    let mut s = scene();
    s.insert(
        DisplayObject::new(
            "image",
            display::Image::new("unresolved", Size::new(2., 2.)),
        )
        .with_transform(Affine::translation(100., 100.)),
        None,
        None,
    )
    .unwrap();
    let json = s.to_json().unwrap();
    let p = plan(&s, request());
    assert_eq!(
        encode(&p).unwrap_err(),
        CompositeError::ImagePending("image".into())
    );
    let mut images = ImageInputs::default();
    let error = CompositeError::ImageUnavailable {
        image: "image".into(),
        reason: "failed current ticket".into(),
    };
    images.insert("image".into(), Err(error.clone()));
    assert_eq!(
        p.encode_with_inputs(
            &images,
            &TextInputs::default(),
            SceneExportPreparation::default(),
            &AtomicBool::new(false)
        )
        .unwrap_err(),
        error
    );
    assert_eq!(s.to_json().unwrap(), json);
}
#[test]
fn current_image_and_canvas_pixels_use_shared_input_path() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("image", display::Image::new("current", Size::new(2., 2.))),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("canvas", display::Canvas::new(2., 2.))
            .with_transform(Affine::translation(3., 0.))
            .with_style(Style {
                fill: None,
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    let p = plan(&s, request());
    assert_eq!(p.source().images().len(), 1);
    assert_eq!(p.source().canvases().len(), 1);
    let mut images = ImageInputs::default();
    let image = Arc::new(
        StaticImage::from_straight_bgra(1, 1, &[0, 0, 255, 128], CompositeLimits::default())
            .unwrap(),
    );
    images.insert("image".into(), Ok(image.clone()));
    images.insert("canvas".into(), Ok(image));
    assert_eq!(images.byte_len(), 4);
    let output = p
        .encode_with_inputs(
            &images,
            &TextInputs::default(),
            SceneExportPreparation::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
    let (_, _, rgba) = png(&output.bytes);
    assert_eq!(at(&rgba, 8, 0, 0), [255, 0, 0, 128]);
    assert_eq!(at(&rgba, 8, 3, 0), [255, 0, 0, 128]);
}
#[test]
fn missing_text_rejects_and_host_ink_with_preparation_is_used() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("text", display::Text::new("ink", Size::new(2., 1.)))
            .with_style(Style::fill(Color::WHITE)),
        None,
        None,
    )
    .unwrap();
    let p = plan(&s, request());
    assert_eq!(
        encode(&p).unwrap_err(),
        CompositeError::TextBackendRequired("text".into())
    );
    let result = p
        .encode_with_text(&ImageInputs::default(), 4, &AtomicBool::new(false), |r| {
            assert_eq!(r.source.text_sources().len(), 1);
            assert_eq!(r.viewport, p.viewport());
            assert!(r.retained_bytes >= 4 + 8 * 6 * 4);
            assert!(r.limits.max_surface_writes < p.limits().max_surface_writes);
            let mut inputs = TextInputs::default();
            inputs.insert(
                "text".into(),
                Ok(Some(Arc::new(TextImage {
                    image: Arc::new(
                        StaticImage::from_premultiplied_rgba(
                            2,
                            1,
                            vec![255, 255, 255, 255, 0, 0, 0, 0],
                            CompositeLimits::default(),
                        )
                        .unwrap(),
                    ),
                    local_bounds: bounds(0., 0., 2., 1.),
                }))),
            );
            Ok(CanvasTextOutput {
                inputs,
                pixel_work: 2,
            })
        })
        .unwrap();
    let (_, _, rgba) = png(&result.bytes);
    assert_eq!(at(&rgba, 8, 0, 0), [255; 4]);
    assert_eq!(at(&rgba, 8, 1, 0), [0; 4]);
}
#[test]
fn visible_custom_rejects_atomic_capture_hidden_custom_is_not_a_producer() {
    let mut s = scene();
    let custom = DisplayObject::new(
        "custom",
        display::Custom::new("native", bounds(0., 0., 1., 1.), vec![]),
    );
    s.insert(custom.clone(), None, None).unwrap();
    let json = s.to_json().unwrap();
    assert_eq!(
        SceneExportPlan::new(&s, request(), CompositeLimits::default()).unwrap_err(),
        CompositeError::UnsupportedCustomExport("custom".into())
    );
    assert_eq!(s.to_json().unwrap(), json);
    let mut hidden = custom;
    hidden.visible = false;
    let mut s = scene();
    s.insert(hidden, None, None).unwrap();
    assert!(encode(&plan(&s, request())).is_ok());
}
#[test]
fn lossless_webp_decodes_to_same_straight_rgba_as_png() {
    let mut s = scene();
    s.insert(
        rect("half", 5., 1., 2., 3., Color::rgba(0, 0, 255, 128)),
        None,
        None,
    )
    .unwrap();
    let expected = pixels(&plan(&s, request()));
    let result = encode(&plan(
        &s,
        SceneExportRequest {
            format: SceneExportFormat::WebPLossless,
            ..request()
        },
    ))
    .unwrap();
    assert_eq!(&result.bytes[..4], b"RIFF");
    assert_eq!(&result.bytes[8..12], b"WEBP");
    let decoded = ::image::load_from_memory_with_format(&result.bytes, ::image::ImageFormat::WebP)
        .unwrap()
        .to_rgba8();
    assert_eq!(decoded.dimensions(), (8, 6));
    assert_eq!(decoded.into_raw(), expected);
}
#[test]
fn jpeg_requires_opaque_background_and_valid_quality() {
    for (quality, background) in [
        (0, Color::WHITE),
        (101, Color::WHITE),
        (80, Color::TRANSPARENT),
        (80, Color::rgba(255, 255, 255, 254)),
    ] {
        assert!(matches!(
            SceneExportPlan::new(
                &scene(),
                SceneExportRequest {
                    format: SceneExportFormat::Jpeg { quality },
                    background,
                    ..request()
                },
                CompositeLimits::default()
            ),
            Err(CompositeError::InvalidGeometry(_))
        ));
    }
}
#[test]
fn jpeg_decode_has_fixed_size_and_explicit_fractional_edge_matte() {
    let p = plan(
        &Scene::new(),
        SceneExportRequest {
            world_bounds: bounds(0., 0., 15.5, 15.5),
            background: Color::rgb(30, 90, 160),
            format: SceneExportFormat::Jpeg { quality: 100 },
            ..request()
        },
    );
    let result = encode(&p).unwrap();
    assert_eq!(&result.bytes[..2], &[0xff, 0xd8]);
    let decoded = ::image::load_from_memory_with_format(&result.bytes, ::image::ImageFormat::Jpeg)
        .unwrap()
        .to_rgb8();
    assert_eq!(decoded.dimensions(), (16, 16));
    for pixel in decoded.pixels() {
        for (actual, expected) in pixel.0.iter().zip([30u8, 90, 160]) {
            assert!(actual.abs_diff(expected) <= 3);
        }
    }
}
#[test]
fn jpeg_composites_scene_alpha_against_the_selected_background() {
    let mut s = Scene::new();
    s.insert(
        rect("half", 0., 0., 16., 16., Color::rgba(255, 0, 0, 128)),
        None,
        None,
    )
    .unwrap();
    let p = plan(
        &s,
        SceneExportRequest {
            world_bounds: bounds(0., 0., 16., 16.),
            background: Color::WHITE,
            format: SceneExportFormat::Jpeg { quality: 100 },
            ..request()
        },
    );
    let result = encode(&p).unwrap();
    let decoded = ::image::load_from_memory_with_format(&result.bytes, ::image::ImageFormat::Jpeg)
        .unwrap()
        .to_rgb8();
    assert_eq!(result.request.format.mime_type(), "image/jpeg");
    for pixel in decoded.pixels() {
        for (actual, expected) in pixel.0.iter().zip([255u8, 127, 127]) {
            assert!(actual.abs_diff(expected) <= 3);
        }
    }
}
#[test]
fn geometry_limits_reject_zero_nonfinite_huge_and_invalid_scales() {
    for r in [
        SceneExportRequest {
            world_bounds: bounds(0., 0., 0., 1.),
            ..request()
        },
        SceneExportRequest {
            world_bounds: bounds(f32::INFINITY, 0., 1., 1.),
            ..request()
        },
        SceneExportRequest {
            world_bounds: bounds(0., 0., 4097., 1.),
            ..request()
        },
        SceneExportRequest {
            scale: f32::NAN,
            ..request()
        },
        SceneExportRequest {
            scale: -1.,
            ..request()
        },
        SceneExportRequest {
            pixel_ratio: 0.,
            ..request()
        },
        SceneExportRequest {
            pixel_ratio: f32::MAX,
            ..request()
        },
    ] {
        assert!(SceneExportPlan::new(&scene(), r, CompositeLimits::default()).is_err());
    }
    assert_eq!(
        SceneExportPlan::new(
            &scene(),
            request(),
            CompositeLimits {
                max_pixels: 47,
                ..Default::default()
            }
        )
        .unwrap_err(),
        CompositeError::TooLarge
    );
    assert_eq!(
        SceneExportPlan::new(
            &scene(),
            request(),
            CompositeLimits {
                max_nodes: 1,
                ..Default::default()
            }
        )
        .unwrap_err(),
        CompositeError::InputLimit
    );
}
#[test]
fn encoder_scratch_and_output_caps_fail_without_scene_mutation() {
    let s = scene();
    let json = s.to_json().unwrap();
    for format in [
        SceneExportFormat::Png,
        SceneExportFormat::Jpeg { quality: 90 },
        SceneExportFormat::WebPLossless,
    ] {
        let r = SceneExportRequest {
            format,
            background: Color::WHITE,
            ..request()
        };
        assert_eq!(
            SceneExportPlan::new(
                &s,
                r,
                CompositeLimits {
                    max_surface_bytes: 1024,
                    ..Default::default()
                }
            )
            .unwrap_err(),
            CompositeError::WorkingSetLimit
        );
        let extra = if format == SceneExportFormat::WebPLossless {
            8 * 6 * 4 * 2 + (8 * 6 * 8 + 65536) * 3
        } else {
            0
        };
        let p = SceneExportPlan::new(
            &s,
            r,
            CompositeLimits {
                max_surface_bytes: 8 * 1024 * 1024 + 8 * 6 * 4 + extra + 8,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(encode(&p).unwrap_err(), CompositeError::WorkingSetLimit);
    }
    assert_eq!(s.to_json().unwrap(), json);
}
#[test]
fn aggregate_work_includes_text_render_and_codec_not_independent_caps() {
    let s = scene();
    let p = SceneExportPlan::new(
        &s,
        request(),
        CompositeLimits {
            max_surface_writes: 8 * 6 * 3,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(encode(&p).unwrap_err(), CompositeError::WorkLimit);
    let p = plan(&s, request());
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
    assert_eq!(
        p.encode_with_inputs(
            &ImageInputs::default(),
            &TextInputs::default(),
            SceneExportPreparation {
                pixel_work: 0,
                retained_bytes: u64::MAX
            },
            &AtomicBool::new(false)
        )
        .unwrap_err(),
        CompositeError::WorkingSetLimit
    );
}
#[test]
fn input_bytes_remain_charged_during_encoding() {
    let p = plan(&scene(), request());
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "unused-retained".into(),
        Ok(Arc::new(
            StaticImage::from_straight_bgra(1, 1, &[255; 4], CompositeLimits::default()).unwrap(),
        )),
    );
    let r = SceneExportPreparation {
        pixel_work: 0,
        retained_bytes: p.limits().max_surface_bytes - 8 * 1024 * 1024 - 8 * 6 * 4 - 2,
    };
    assert_eq!(
        p.encode_with_inputs(&inputs, &TextInputs::default(), r, &AtomicBool::new(false))
            .unwrap_err(),
        CompositeError::WorkingSetLimit
    );
}
#[test]
fn cancellation_before_and_during_host_preparation_never_returns_bytes() {
    let p = plan(&scene(), request());
    assert_eq!(
        p.encode_with_text(
            &ImageInputs::default(),
            0,
            &AtomicBool::new(true),
            |_| panic!("must not prepare cancelled export")
        )
        .unwrap_err(),
        CompositeError::Superseded
    );
    let cancelled = AtomicBool::new(false);
    assert_eq!(
        p.encode_with_text(&ImageInputs::default(), 0, &cancelled, |_| {
            cancelled.store(true, Ordering::Relaxed);
            Ok(CanvasTextOutput::default())
        })
        .unwrap_err(),
        CompositeError::Superseded
    );
}
#[test]
fn repeated_png_and_webp_exports_are_deterministic() {
    for format in [SceneExportFormat::Png, SceneExportFormat::WebPLossless] {
        let p = plan(
            &scene(),
            SceneExportRequest {
                format,
                ..request()
            },
        );
        assert_eq!(encode(&p).unwrap().bytes, encode(&p).unwrap().bytes);
        assert_eq!(
            format.mime_type(),
            if format == SceneExportFormat::Png {
                "image/png"
            } else {
                "image/webp"
            }
        );
    }
}

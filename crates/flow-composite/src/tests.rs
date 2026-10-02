use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, DisplayObject, MaskMode, Point, Scene, Size, Style, display,
};
use std::sync::atomic::AtomicBool;
fn group(id: &str, opacity: f32) -> DisplayObject {
    DisplayObject::new(id, display::Group::new()).with_style(Style {
        opacity,
        blend_mode: BlendMode::Normal,
        ..Default::default()
    })
}
fn rect(id: &str, x: f32, color: Color) -> DisplayObject {
    DisplayObject::new(id, display::Rect::new(20., 20.))
        .with_transform(Affine::translation(x, 0.))
        .with_style(Style::fill(color))
}
fn source(scene: &Scene) -> DomainSource {
    match scene_units(scene, CompositeLimits::default())
        .unwrap()
        .remove(0)
    {
        SceneUnit::Domain(d) => *d,
        u => panic!("{u:?}"),
    }
}
fn viewport() -> RasterViewport {
    RasterViewport {
        world_to_screen: Affine::IDENTITY,
        canvas_bounds: Bounds::new(Point::ZERO, Size::new(100., 100.)),
        content_mask: Bounds::new(Point::ZERO, Size::new(100., 100.)),
        device_scale: 1.,
    }
}
fn render(scene: &Scene) -> CpuFrame {
    rasterize(
        &source(scene),
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}
fn at(f: &CpuFrame, x: u32, y: u32) -> [u8; 4] {
    let x = x - (f.screen_bounds.origin.x as u32);
    let y = y - (f.screen_bounds.origin.y as u32);
    f.premul_rgba[((y * f.width + x) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}
fn insert(scene: &mut Scene, o: DisplayObject, parent: &str) {
    scene.insert(o, Some(parent.into()), None).unwrap();
}
#[test]
fn group_opacity_is_once_across_overlapping_children() {
    let mut s = Scene::new();
    s.insert(group("g", 0.5), None, None).unwrap();
    insert(&mut s, rect("a", 0., Color::hex(0xff0000)), "g");
    insert(&mut s, rect("b", 10., Color::hex(0x0000ff)), "g");
    let f = render(&s);
    assert_eq!(at(&f, 5, 5), [128, 0, 0, 128]);
    assert_eq!(at(&f, 15, 5), [0, 0, 128, 128]);
}
#[test]
fn svg_arc_path_uses_real_curve_pixels_inside_composite_domain() {
    let mut s = Scene::new();
    s.insert(group("g", 0.5), None, None).unwrap();
    let path = display::Path::from_svg("M40 20 A20 20 0 1 1 0 20 A20 20 0 1 1 40 20Z").unwrap();
    insert(
        &mut s,
        DisplayObject::new("arc", path).with_style(Style::fill(Color::hex(0xff0000))),
        "g",
    );
    let frame = render(&s);
    assert_eq!(at(&frame, 20, 20), [128, 0, 0, 128]);
    assert_eq!(at(&frame, 1, 1), [0, 0, 0, 0]);
}
#[test]
fn nested_opacity_multiplies_once_per_group() {
    let mut s = Scene::new();
    s.insert(group("a", 0.5), None, None).unwrap();
    insert(&mut s, group("b", 0.5), "a");
    insert(&mut s, rect("r", 0., Color::WHITE), "b");
    assert_eq!(at(&render(&s), 5, 5), [64, 64, 64, 64]);
}
#[test]
fn multiply_and_screen_read_the_internal_backdrop() {
    for (blend, expected) in [
        (BlendMode::Multiply, [0, 0, 0, 255]),
        (BlendMode::Screen, [255, 0, 255, 255]),
    ] {
        let mut s = Scene::new();
        s.insert(group("g", 1.), None, None).unwrap();
        insert(&mut s, rect("a", 0., Color::hex(0xff0000)), "g");
        let mut b = rect("b", 0., Color::hex(0x0000ff));
        b.style.blend_mode = blend;
        insert(&mut s, b, "g");
        assert_eq!(at(&render(&s), 5, 5), expected);
    }
}
fn artistic_modes() -> [BlendMode; 15] {
    use BlendMode::*;
    [
        Multiply, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight, SoftLight,
        Difference, Exclusion, Hue, Saturation, Color, Luminosity,
    ]
}
fn blended_pixel(mode: BlendMode, background: u32, foreground: u32, alpha: f32) -> [u8; 4] {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    insert(&mut s, rect("back", 0., Color::hex(background)), "g");
    let mut front = rect("front", 0., Color::hex(foreground));
    front.style.blend_mode = mode;
    front.style.opacity = alpha;
    insert(&mut s, front, "g");
    at(&render(&s), 5, 5)
}
#[test]
fn separable_artistic_modes_match_independent_channel_equations() {
    fn equation(mode: BlendMode, b: f64, s: f64) -> f64 {
        use BlendMode::*;
        match mode {
            Multiply => b * s,
            Screen => b + s - b * s,
            Overlay => {
                if b <= 0.5 {
                    2. * b * s
                } else {
                    1. - 2. * (1. - b) * (1. - s)
                }
            }
            Darken => b.min(s),
            Lighten => b.max(s),
            ColorDodge => {
                if s == 1. {
                    1.
                } else {
                    (b / (1. - s)).min(1.)
                }
            }
            ColorBurn => {
                if s == 0. {
                    0.
                } else {
                    1. - ((1. - b) / s).min(1.)
                }
            }
            HardLight => {
                if s <= 0.5 {
                    2. * b * s
                } else {
                    1. - 2. * (1. - b) * (1. - s)
                }
            }
            SoftLight => {
                if s <= 0.5 {
                    b - (1. - 2. * s) * b * (1. - b)
                } else {
                    let d = if b <= 0.25 {
                        ((16. * b - 12.) * b + 4.) * b
                    } else {
                        b.sqrt()
                    };
                    b + (2. * s - 1.) * (d - b)
                }
            }
            Difference => (b - s).abs(),
            Exclusion => b + s - 2. * b * s,
            _ => unreachable!(),
        }
    }
    for mode in artistic_modes().into_iter().take(11) {
        for alpha in [1., 0.5] {
            let pixel = blended_pixel(mode, 0x33b366, 0xcc4d99, alpha);
            for (i, (b, s)) in [51., 179., 102.]
                .into_iter()
                .zip([204., 77., 153.])
                .enumerate()
            {
                let expected = ((1. - alpha as f64) * b
                    + alpha as f64 * 255. * equation(mode, b / 255., s / 255.))
                .round() as i16;
                assert!(
                    (pixel[i] as i16 - expected).abs() <= 2,
                    "{mode:?} alpha {alpha} channel {i}: {} != {expected}",
                    pixel[i]
                );
            }
            assert_eq!(pixel[3], 255);
        }
    }
}
#[test]
fn nonseparable_modes_preserve_the_expected_color_components() {
    // Red source, blue destination. Rec.601 luminance is .30/.11 respectively.
    for (mode, expected) in [
        (BlendMode::Hue, [94, 0, 0, 255]),
        (BlendMode::Saturation, [0, 0, 255, 255]),
        (BlendMode::Color, [94, 0, 0, 255]),
        (BlendMode::Luminosity, [54, 54, 255, 255]),
    ] {
        let actual = blended_pixel(mode, 0x0000ff, 0xff0000, 1.);
        for i in 0..4 {
            assert!(
                (actual[i] as i16 - expected[i]).abs() <= 2,
                "{mode:?}: {actual:?}"
            );
        }
    }
}
#[test]
fn every_artistic_mode_requires_a_real_backdrop_and_roundtrips() {
    for mode in artistic_modes() {
        assert!(mode.requires_backdrop());
        let encoded = serde_json::to_string(&mode).unwrap();
        assert_eq!(serde_json::from_str::<BlendMode>(&encoded).unwrap(), mode);
        let mut s = Scene::new();
        let mut o = rect("external", 0., Color::WHITE);
        o.style.blend_mode = mode;
        s.insert(o, None, None).unwrap();
        let units = scene_units(&s, CompositeLimits::default()).unwrap();
        assert!(
            matches!(&units[0],SceneUnit::Rejected(e) if e.error==CompositeError::UnsupportedExternalBackdrop)
        );
    }
    assert!(!BlendMode::Normal.requires_backdrop());
    assert!(!BlendMode::PassThrough.requires_backdrop());
}
#[test]
fn alpha_mask_is_hidden_and_masks_following_siblings_once() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    let mut mask = rect("mask", 0., Color::WHITE).with_mask(MaskMode::Alpha);
    mask.style.opacity = 0.5;
    insert(&mut s, mask, "g");
    insert(&mut s, rect("a", 10., Color::hex(0xff0000)), "g");
    insert(&mut s, rect("b", 10., Color::hex(0x0000ff)), "g");
    let f = render(&s);
    assert_eq!(at(&f, 5, 5), [0, 0, 0, 0]);
    assert_eq!(at(&f, 15, 5), [0, 0, 128, 128]);
    assert_eq!(at(&f, 25, 5), [0, 0, 0, 0]);
}
#[test]
fn external_blend_rejected_and_unsupported_descendant_rejects_whole_domain() {
    let mut s = Scene::new();
    let mut r = rect("r", 0., Color::WHITE);
    r.style.blend_mode = BlendMode::Multiply;
    s.insert(r, None, None).unwrap();
    assert!(matches!(
        &scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::UnsupportedExternalBackdrop,
            ..
        })
    ));
    let mut s = Scene::new();
    s.insert(group("g", 0.5), None, None).unwrap();
    insert(
        &mut s,
        DisplayObject::new("t", display::Text::new("hello", Size::new(40., 20.))),
        "g",
    );
    assert!(matches!(
        rasterize(
            &source(&s),
            viewport(),
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false)
        ),
        Err(CompositeError::TextBackendRequired(_))
    ));
}
#[test]
fn pixel_budget_failure_returns_no_partial_frame() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    insert(&mut s, rect("r", 0., Color::WHITE), "g");
    let limits = CompositeLimits {
        max_surface_bytes: 100,
        ..Default::default()
    };
    let mut budget = PixelBudget::new(limits, 0).unwrap();
    assert!(matches!(
        rasterize(
            &source(&s),
            viewport(),
            &mut budget,
            &AtomicBool::new(false)
        ),
        Err(CompositeError::WorkingSetLimit)
    ));
    assert_eq!(budget.retained_bytes(), 0);
}
#[test]
fn normal_group_isolates_while_pass_through_shares_backdrop() {
    for (blend, expected) in [
        (BlendMode::Normal, [0, 0, 255, 255]),
        (BlendMode::PassThrough, [0, 0, 0, 255]),
    ] {
        let mut s = Scene::new();
        s.insert(group("outer", 1.), None, None).unwrap();
        insert(&mut s, rect("red", 0., Color::hex(0xff0000)), "outer");
        let mut inner = group("inner", 1.);
        inner.style.blend_mode = blend;
        insert(&mut s, inner, "outer");
        let mut blue = rect("blue", 0., Color::hex(0x0000ff));
        blue.style.blend_mode = BlendMode::Multiply;
        insert(&mut s, blue, "inner");
        assert_eq!(at(&render(&s), 5, 5), expected);
    }
}
#[test]
fn opaque_mask_blend_flush_uses_parent_backdrop_but_translucent_mask_isolates_run() {
    for (opacity, expected) in [(1., [0, 0, 0, 255]), (0.5, [127, 0, 128, 255])] {
        let mut s = Scene::new();
        s.insert(group("g", 1.), None, None).unwrap();
        insert(&mut s, rect("back", 0., Color::hex(0xff0000)), "g");
        let mut mask = rect("m", 0., Color::WHITE).with_mask(MaskMode::Alpha);
        mask.style.opacity = opacity;
        insert(&mut s, mask, "g");
        let mut blue = rect("blue", 0., Color::hex(0x0000ff));
        blue.style.blend_mode = BlendMode::Multiply;
        insert(&mut s, blue, "g");
        assert_eq!(at(&render(&s), 5, 5), expected);
    }
}
#[test]
fn new_mask_ends_previous_run_and_equal_z_preserves_insertion_order() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    insert(
        &mut s,
        rect("m1", 0., Color::WHITE).with_mask(MaskMode::Alpha),
        "g",
    );
    insert(&mut s, rect("red", 0., Color::hex(0xff0000)), "g");
    insert(
        &mut s,
        rect("m2", 10., Color::WHITE).with_mask(MaskMode::Alpha),
        "g",
    );
    insert(&mut s, rect("blue", 10., Color::hex(0x0000ff)), "g");
    let f = render(&s);
    assert_eq!(at(&f, 5, 5), [255, 0, 0, 255]);
    assert_eq!(at(&f, 15, 5), [0, 0, 255, 255]);
    assert_eq!(at(&f, 25, 5), [0, 0, 255, 255]);
}
#[test]
fn primitive_layer_alpha_is_once() {
    let mut s = Scene::new();
    let mut r = rect("r", 0., Color::hex(0xff0000));
    r.style.fill_layers.push(Color::hex(0x0000ff).into());
    r.style.opacity = 0.5;
    s.insert(r, None, None).unwrap();
    assert_eq!(at(&render(&s), 5, 5), [0, 0, 128, 128]);
}
#[test]
fn fractional_dpr_grid_affine_and_inherited_rounded_clips() {
    let mut s = Scene::new();
    let mut frame = DisplayObject::new("frame", display::Frame::new(40., 40.));
    if let gpui_flow_scene::Shape::Frame(f) = &mut frame.shape {
        f.corner_radius = 12.;
    }
    s.insert(frame, None, None).unwrap();
    insert(&mut s, group("g", 0.5), "frame");
    let mut r = rect("r", 0., Color::WHITE);
    r.transform = Affine::translation(30., 0.) * Affine::scale(-2., 2.);
    insert(&mut s, r, "g");
    let unit = scene_units(&s, CompositeLimits::default())
        .unwrap()
        .into_iter()
        .find_map(|u| {
            if let SceneUnit::Domain(d) = u {
                Some(d)
            } else {
                None
            }
        })
        .unwrap();
    let mut v = viewport();
    v.device_scale = 1.5;
    v.world_to_screen = Affine::translation(0.3, 0.2);
    let f = rasterize(
        &unit,
        v,
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!((f.screen_bounds.origin.x * 1.5).fract(), 0.);
    assert_eq!((f.screen_bounds.origin.y * 1.5).fract(), 0.);
    assert_eq!(&f.premul_rgba[..4], &[0, 0, 0, 0]);
    assert!(f.premul_rgba.as_chunks::<4>().0.iter().any(|p| p[3] == 128));
}
#[test]
fn gradient_mask_consumes_pixel_alpha() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    let mut m = rect("m", 0., Color::WHITE).with_mask(MaskMode::Alpha);
    m.style.fill = Some(gpui_flow_scene::Paint::LinearGradient {
        start: Point::ZERO,
        end: Point::new(20., 0.),
        stops: vec![
            gpui_flow_scene::GradientStop {
                offset: 0.,
                color: Color::TRANSPARENT,
            },
            gpui_flow_scene::GradientStop {
                offset: 1.,
                color: Color::WHITE,
            },
        ],
    });
    insert(&mut s, m, "g");
    insert(&mut s, rect("r", 0., Color::WHITE), "g");
    let f = render(&s);
    assert!(at(&f, 2, 5)[3] < at(&f, 17, 5)[3]);
}
#[test]
fn empty_offscreen_cancelled_and_work_limited_outputs_are_atomic() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    let mut b = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    assert!(
        rasterize(&source(&s), viewport(), &mut b, &AtomicBool::new(false))
            .unwrap()
            .is_none()
    );
    insert(&mut s, rect("r", 200., Color::WHITE), "g");
    assert!(
        rasterize(&source(&s), viewport(), &mut b, &AtomicBool::new(false))
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        rasterize(&source(&s), viewport(), &mut b, &AtomicBool::new(true)),
        Err(CompositeError::Superseded)
    ));
    let mut v = viewport();
    v.world_to_screen = Affine::translation(-200., 0.);
    let mut b = PixelBudget::new(
        CompositeLimits {
            max_surface_writes: 1,
            ..Default::default()
        },
        0,
    )
    .unwrap();
    assert!(matches!(
        rasterize(&source(&s), v, &mut b, &AtomicBool::new(false)),
        Err(CompositeError::WorkLimit)
    ));
    assert_eq!(b.retained_bytes(), 0);
}
#[test]
fn zero_scale_and_excessive_control_points_are_explicit_errors() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    insert(&mut s, rect("r", 0., Color::WHITE), "g");
    let mut v = viewport();
    v.device_scale = 0.;
    let mut b = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    assert!(matches!(
        rasterize(&source(&s), v, &mut b, &AtomicBool::new(false)),
        Err(CompositeError::UnsupportedTransform)
    ));
    let units = scene_units(
        &s,
        CompositeLimits {
            max_nodes: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        &units[0],
        SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::InputLimit,
            ..
        })
    ));
}
#[test]
fn domain_key_equality_sees_child_paint_and_mask_reordering() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    insert(&mut s, rect("a", 0., Color::WHITE), "g");
    let first = source(&s);
    s.update_object(&"a".into(), |o| o.style.fill = Some(Color::BLACK.into()))
        .unwrap();
    assert_ne!(first, source(&s));
}
#[test]
fn pass_through_mask_group_does_not_fabricate_an_external_blend_backdrop() {
    let mut s = Scene::new();
    s.insert(rect("red", 0., Color::hex(0xff0000)), None, None)
        .unwrap();
    let mut g = group("g", 1.);
    g.style.blend_mode = BlendMode::PassThrough;
    s.insert(g, None, None).unwrap();
    insert(
        &mut s,
        rect("mask", 0., Color::WHITE).with_mask(MaskMode::Alpha),
        "g",
    );
    let mut blue = rect("blue", 0., Color::hex(0x0000ff));
    blue.style.blend_mode = BlendMode::Multiply;
    insert(&mut s, blue, "g");
    let units = scene_units(&s, CompositeLimits::default()).unwrap();
    assert!(matches!(
        &units[1],
        SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::UnsupportedExternalBackdrop,
            ..
        })
    ));
    s.update_object(&"mask".into(), |o| o.style.opacity = 0.5)
        .unwrap();
    let units = scene_units(&s, CompositeLimits::default()).unwrap();
    assert!(matches!(&units[1], SceneUnit::Domain(_)));
}
#[test]
fn tiny_skia_rejected_inverse_never_becomes_a_successful_blank_gradient() {
    use gpui_flow_scene::{GradientStop, Paint};
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    let mut r = DisplayObject::new("r", display::Rect::new(20_000_000., 20_000_000.));
    r.transform = Affine::new(1e-6, 1e-6, 0., 1e-6, 0., 0.);
    r.style.fill = Some(Paint::LinearGradient {
        start: Point::ZERO,
        end: Point::new(20_000_000., 0.),
        stops: vec![
            GradientStop {
                offset: 0.,
                color: Color::WHITE,
            },
            GradientStop {
                offset: 1.,
                color: Color::BLACK,
            },
        ],
    });
    insert(&mut s, r, "g");
    let result = rasterize(
        &source(&s),
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    );
    assert!(matches!(result, Err(CompositeError::UnsupportedTransform)));
}
#[test]
fn excess_domains_reject_before_cloning_another_domain_snapshot() {
    let mut s = Scene::new();
    for n in 0..9 {
        s.insert(group(&format!("g{n}"), 1.), None, None).unwrap();
    }
    let units = scene_units(&s, CompositeLimits::default()).unwrap();
    assert_eq!(
        units
            .iter()
            .filter(|u| matches!(u, SceneUnit::Domain(_)))
            .count(),
        8
    );
    assert!(matches!(
        &units[8],
        SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::InputLimit,
            ..
        })
    ));
}

fn static_pixels(width: u32, height: u32, bgra: &[u8]) -> std::sync::Arc<StaticImage> {
    std::sync::Arc::new(
        StaticImage::from_straight_bgra(width, height, bgra, CompositeLimits::default()).unwrap(),
    )
}
fn image_scene(fit: display::ImageFit, opacity: f32) -> Scene {
    let mut s = Scene::new();
    s.insert(group("g", opacity), None, None).unwrap();
    let mut image = display::Image::new("fixture.png", Size::new(20., 20.));
    image.fit = fit;
    // Deliberately wrong: native GPUI uses decoded image dimensions for fit.
    image.natural_size = Some(Size::new(1., 8.));
    insert(&mut s, DisplayObject::new("image", image), "g");
    s
}
fn render_pixels(s: &Scene, pixels: std::sync::Arc<StaticImage>) -> CpuFrame {
    let mut images = ImageInputs::default();
    images.insert("image".into(), Ok(pixels));
    rasterize_with_images(
        &source(s),
        &images,
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}
#[test]
fn static_bgra_input_validates_and_premultiplies_channels_once() {
    let i = static_pixels(3, 1, &[0, 0, 255, 128, 255, 0, 0, 64, 99, 88, 77, 0]);
    assert_eq!(
        i.premultiplied_rgba(),
        &[128, 0, 0, 128, 0, 0, 64, 64, 0, 0, 0, 0]
    );
    assert!(matches!(
        StaticImage::from_straight_bgra(1, 1, &[0; 3], CompositeLimits::default()),
        Err(CompositeError::InvalidImage)
    ));
    assert!(matches!(
        StaticImage::from_straight_bgra(0, 1, &[], CompositeLimits::default()),
        Err(CompositeError::InvalidImage)
    ));
    assert!(matches!(
        StaticImage::from_straight_bgra(u32::MAX, u32::MAX, &[], CompositeLimits::default()),
        Err(CompositeError::TooLarge)
    ));
}
#[test]
fn static_image_fit_uses_decoded_aspect_and_clips_cover_to_layout_box() {
    let pixels = static_pixels(2, 1, &[0, 0, 255, 255, 255, 0, 0, 255]);
    for fit in [
        display::ImageFit::Contain,
        display::ImageFit::Cover,
        display::ImageFit::Fill,
    ] {
        let f = render_pixels(&image_scene(fit, 1.), pixels.clone());
        assert_eq!(at(&f, 1, 10)[3], 255);
        assert_eq!(at(&f, 19, 10)[3], 255);
        assert_eq!(at(&f, 20, 10), [0; 4]);
        if fit == display::ImageFit::Contain {
            assert_eq!(f.screen_bounds.origin.y, 4.);
            assert_eq!(at(&f, 10, 4), [0; 4]);
            assert_eq!(at(&f, 10, 15), [0; 4]);
        } else {
            assert_eq!(at(&f, 10, 1)[3], 255);
        }
    }
}
#[test]
fn static_image_opacity_and_native_style_semantics_match() {
    let mut s = image_scene(display::ImageFit::Fill, 0.5);
    s.update_object(&"image".into(), |o| {
        o.style.opacity = 0.5;
        o.style.fill = Some(gpui_flow_scene::Paint::Solid {
            color: Color::hex(0x00ff00),
        });
    })
    .unwrap();
    let f = render_pixels(&s, static_pixels(1, 1, &[0, 0, 255, 128]));
    assert_eq!(at(&f, 10, 10), [32, 0, 0, 32]);
}
#[test]
fn static_image_is_real_alpha_mask_not_its_bounds() {
    let mut s = image_scene(display::ImageFit::Fill, 1.);
    s.update_object(&"image".into(), |o| o.mask = MaskMode::Alpha)
        .unwrap();
    insert(&mut s, rect("blue", 0., Color::hex(0x0000ff)), "g");
    let f = render_pixels(&s, static_pixels(2, 1, &[0, 0, 255, 0, 0, 0, 255, 128]));
    assert_eq!(at(&f, 1, 10), [0; 4]);
    assert_eq!(at(&f, 18, 10), [0, 0, 128, 128]);
}
#[test]
fn static_image_blends_with_actual_vector_backdrop() {
    for (blend, expected) in [
        (BlendMode::Multiply, [0, 0, 0, 255]),
        (BlendMode::Screen, [255, 0, 255, 255]),
    ] {
        let mut s = image_scene(display::ImageFit::Fill, 1.);
        let mut back = rect("back", 0., Color::hex(0xff0000));
        back.z_index = -1;
        insert(&mut s, back, "g");
        s.update_object(&"image".into(), |o| o.style.blend_mode = blend)
            .unwrap();
        assert_eq!(
            at(
                &render_pixels(&s, static_pixels(1, 1, &[255, 0, 0, 255])),
                10,
                10
            ),
            expected
        );
    }
}
#[test]
fn static_image_pattern_follows_full_affine_without_double_transform() {
    let mut s = image_scene(display::ImageFit::Fill, 1.);
    s.update_object(&"image".into(), |o| {
        o.transform = Affine::new(0., 1., -1., 0., 40., 0.)
    })
    .unwrap();
    let f = render_pixels(&s, static_pixels(2, 1, &[0, 0, 255, 255, 255, 0, 0, 255]));
    assert_eq!(at(&f, 30, 1), [255, 0, 0, 255]);
    assert_eq!(at(&f, 30, 18), [0, 0, 255, 255]);
}
#[test]
fn missing_or_failed_static_image_rejects_entire_domain() {
    let mut s = image_scene(display::ImageFit::Fill, 1.);
    insert(&mut s, rect("vector", 0., Color::WHITE), "g");
    assert!(matches!(
        rasterize(
            &source(&s),
            viewport(),
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false)
        ),
        Err(CompositeError::ImagePending(_))
    ));
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "image".into(),
        Err(CompositeError::UnsupportedAnimatedImage),
    );
    assert!(matches!(
        rasterize_with_images(
            &source(&s),
            &inputs,
            viewport(),
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false)
        ),
        Err(CompositeError::UnsupportedAnimatedImage)
    ));
}
#[test]
fn static_image_inputs_share_the_working_set_budget_and_deduplicate() {
    let pixels = static_pixels(1, 1, &[0, 0, 255, 255]);
    let mut inputs = ImageInputs::default();
    inputs.insert("image".into(), Ok(pixels.clone()));
    inputs.insert("duplicate".into(), Ok(pixels));
    assert_eq!(inputs.byte_len(), 4);
    let mut budget = PixelBudget::new(
        CompositeLimits {
            max_surface_bytes: 3,
            ..Default::default()
        },
        0,
    )
    .unwrap();
    assert!(matches!(
        rasterize_with_images(
            &source(&image_scene(display::ImageFit::Fill, 1.)),
            &inputs,
            viewport(),
            &mut budget,
            &AtomicBool::new(false)
        ),
        Err(CompositeError::WorkingSetLimit)
    ));
    assert_eq!(budget.retained_bytes(), 0);
}

#[test]
fn hidden_images_and_ancestors_do_not_need_pixels_but_hidden_masks_still_mask() {
    for zero_opacity in [false, true] {
        let mut s = image_scene(display::ImageFit::Fill, 1.);
        s.update_object(&"image".into(), |o| {
            if zero_opacity {
                o.style.opacity = 0.
            } else {
                o.visible = false
            }
        })
        .unwrap();
        insert(&mut s, rect("white", 0., Color::WHITE), "g");
        assert!(source(&s).images().is_empty());
        assert_eq!(at(&render(&s), 10, 10), [255; 4]);
        s.update_object(&"image".into(), |o| o.mask = MaskMode::Alpha)
            .unwrap();
        assert_eq!(at(&render(&s), 10, 10), [0; 4]);
    }
    let mut s = image_scene(display::ImageFit::Fill, 1.);
    insert(&mut s, group("hidden", 0.), "g");
    s.reparent(&"image".into(), Some("hidden".into()), None, false)
        .unwrap();
    insert(&mut s, rect("white", 0., Color::WHITE), "g");
    assert!(source(&s).images().is_empty());
    assert_eq!(at(&render(&s), 10, 10), [255; 4]);
}

#[test]
fn decoded_aspect_fit_overflow_is_an_explicit_error() {
    let mut s = image_scene(display::ImageFit::Cover, 1.);
    s.update_object(&"image".into(), |o| {
        if let gpui_flow_scene::Shape::Image(i) = &mut o.shape {
            i.size = Size::new(f32::MAX, f32::MAX);
            i.natural_size = None;
        }
    })
    .unwrap();
    let mut images = ImageInputs::default();
    images.insert("image".into(), Ok(static_pixels(2, 1, &[0; 8])));
    assert!(matches!(
        rasterize_with_images(
            &source(&s),
            &images,
            viewport(),
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false)
        ),
        Err(CompositeError::InvalidImage)
    ));
}

#[test]
fn static_pixels_respect_inherited_rounded_clip_after_transform() {
    let mut scene = image_scene(display::ImageFit::Fill, 0.5);
    scene
        .insert(
            DisplayObject::new("frame", display::Frame::new(20., 20.).rounded(8.))
                .with_transform(Affine::translation(5., 5.)),
            None,
            None,
        )
        .unwrap();
    scene
        .reparent(&"g".into(), Some("frame".into()), None, false)
        .unwrap();
    let domain = scene_units(&scene, CompositeLimits::default())
        .unwrap()
        .into_iter()
        .find_map(|unit| {
            if let SceneUnit::Domain(domain) = unit {
                Some(domain)
            } else {
                None
            }
        })
        .unwrap();
    let mut images = ImageInputs::default();
    images.insert("image".into(), Ok(static_pixels(1, 1, &[0, 0, 255, 128])));
    let mut budget = PixelBudget::new(CompositeLimits::default(), 16).unwrap();
    let frame = rasterize_with_images(
        &domain,
        &images,
        viewport(),
        &mut budget,
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(at(&frame, 5, 5), [0; 4]);
    assert_eq!(at(&frame, 15, 15), [64, 0, 0, 64]);
    assert_eq!(at(&frame, 24, 5), [0; 4]);
    let peak = budget.peak_bytes();
    let writes = budget.surface_writes();
    assert!(budget.retained_bytes() > 16);
    budget.discard_frame(frame);
    assert_eq!(budget.retained_bytes(), 16);
    assert_eq!(budget.peak_bytes(), peak);
    assert_eq!(budget.surface_writes(), writes);
}

#[test]
fn hidden_blend_image_cannot_reject_visible_masked_siblings() {
    for zero_opacity in [false, true] {
        let mut scene = image_scene(display::ImageFit::Fill, 1.);
        scene
            .update_object(&"g".into(), |o| o.style.blend_mode = BlendMode::PassThrough)
            .unwrap();
        let mut mask = rect("mask", 0., Color::WHITE).with_mask(MaskMode::Alpha);
        mask.z_index = -1;
        insert(&mut scene, mask, "g");
        scene
            .update_object(&"image".into(), |o| {
                o.style.blend_mode = BlendMode::Multiply;
                if zero_opacity {
                    o.style.opacity = 0.;
                } else {
                    o.visible = false;
                }
            })
            .unwrap();
        insert(&mut scene, rect("red", 0., Color::hex(0xff0000)), "g");
        assert!(source(&scene).images().is_empty());
        assert_eq!(at(&render(&scene), 10, 10), [255, 0, 0, 255]);
    }
}

#[test]
fn hidden_mask_preserves_zero_alpha_without_requiring_external_backdrop() {
    let mut scene = Scene::new();
    let mut root = group("g", 1.);
    root.style.blend_mode = BlendMode::PassThrough;
    scene.insert(root, None, None).unwrap();
    let mut mask = rect("mask", 0., Color::WHITE).with_mask(MaskMode::Alpha);
    mask.visible = false;
    insert(&mut scene, mask, "g");
    let mut red = rect("red", 0., Color::hex(0xff0000));
    red.style.blend_mode = BlendMode::Multiply;
    insert(&mut scene, red, "g");
    assert_eq!(at(&render(&scene), 10, 10), [0; 4]);
}

#[test]
fn hidden_mask_retains_zero_alpha_run_without_external_backdrop_dependency() {
    let mut scene = image_scene(display::ImageFit::Fill, 1.);
    scene
        .update_object(&"g".into(), |o| o.style.blend_mode = BlendMode::PassThrough)
        .unwrap();
    scene
        .update_object(&"image".into(), |o| {
            o.style.blend_mode = BlendMode::Multiply
        })
        .unwrap();
    let mut mask = rect("mask", 0., Color::WHITE).with_mask(MaskMode::Alpha);
    mask.z_index = -1;
    mask.visible = false;
    insert(&mut scene, mask, "g");
    let frame = render_pixels(&scene, static_pixels(1, 1, &[0, 0, 255, 255]));
    assert_eq!(at(&frame, 10, 10), [0; 4]);
}

fn unpainted_container(id: &str, shape: impl Into<gpui_flow_scene::Shape>) -> DisplayObject {
    DisplayObject::new(id, shape).with_style(Style {
        fill: None,
        ..Default::default()
    })
}

fn container_scene(shape: gpui_flow_scene::Shape) -> Scene {
    let mut s = Scene::new();
    let mut c = unpainted_container("container", shape)
        .with_transform(Affine::translation(10., 10.))
        .with_style(Style::fill(Color::hex(0xff0000)));
    c.style.opacity = 0.5;
    c.style.stroke = Some(gpui_flow_scene::Stroke::new(Color::hex(0x00ff00), 8.));
    s.insert(c, None, None).unwrap();
    insert(&mut s, rect("child", 0., Color::hex(0x0000ff)), "container");
    s
}

#[test]
fn containers_paint_parent_fill_stroke_then_children_with_opacity_once() {
    for shape in [
        display::Box::new(30., 30.).into(),
        display::Frame::new(30., 30.).into(),
    ] {
        let s = container_scene(shape);
        assert_eq!(
            s.paint_order().unwrap(),
            vec!["container".into(), "child".into()]
        );
        let f = render(&s);
        assert_eq!(at(&f, 12, 15), [0, 0, 128, 128]); // child covers inner stroke
        assert_eq!(at(&f, 8, 15), [0, 128, 0, 128]); // own stroke is not child-clipped
        assert_eq!(at(&f, 34, 25), [128, 0, 0, 128]); // own background
        assert_eq!(at(&f, 25, 25), [0, 0, 128, 128]); // overlap doesn't double alpha
    }
}

#[test]
fn box_overflow_and_frame_default_clip_have_distinct_real_pixels() {
    for (shape, outside) in [
        (
            gpui_flow_scene::Shape::Box(display::Box::new(20., 20.)),
            true,
        ),
        (display::Box::new(20., 20.).clipped(true).into(), false),
        (display::Frame::new(20., 20.).into(), false),
    ] {
        let mut s = Scene::new();
        s.insert(group("g", 1.), None, None).unwrap();
        insert(
            &mut s,
            rect("back", 0., Color::WHITE).with_transform(Affine::scale(3., 3.)),
            "g",
        );
        insert(&mut s, unpainted_container("container", shape), "g");
        insert(
            &mut s,
            rect("child", 10., Color::hex(0x0000ff)),
            "container",
        );
        let f = render(&s);
        assert_eq!(at(&f, 15, 5), [0, 0, 255, 255]);
        assert_eq!(
            at(&f, 25, 5),
            if outside { [0, 0, 255, 255] } else { [255; 4] }
        );
    }
}

#[test]
fn clipped_pass_through_container_keeps_internal_backdrop_and_isolation_is_explicit() {
    for (blend, opacity, expected) in [
        (BlendMode::PassThrough, 1., [0, 0, 0, 255]),
        (BlendMode::Normal, 1., [0, 0, 255, 255]),
        (BlendMode::PassThrough, 0.5, [128, 0, 128, 255]),
    ] {
        let mut s = Scene::new();
        s.insert(group("g", 1.), None, None).unwrap();
        insert(&mut s, rect("red", 0., Color::hex(0xff0000)), "g");
        let mut c = unpainted_container("container", display::Frame::new(16., 16.).rounded(4.));
        c.style.blend_mode = blend;
        c.style.opacity = opacity;
        insert(&mut s, c, "g");
        let mut child = rect("blue", 0., Color::hex(0x0000ff));
        child.style.blend_mode = BlendMode::Multiply;
        insert(&mut s, child, "container");
        let f = render(&s);
        assert_eq!(at(&f, 8, 8), expected);
        assert_eq!(at(&f, 19, 8), [255, 0, 0, 255]);
    }
}

#[test]
fn rounded_nested_container_clips_transform_and_preserve_sibling_order() {
    let mut s = Scene::new();
    s.insert(group("g", 0.5), None, None).unwrap();
    insert(
        &mut s,
        unpainted_container("outer", display::Frame::new(30., 30.).rounded(10.))
            .with_transform(Affine::translation(40., 10.) * Affine::scale(-1., 1.)),
        "g",
    );
    insert(
        &mut s,
        unpainted_container(
            "inner",
            display::Box::new(20., 40.).rounded(6.).clipped(true),
        )
        .with_transform(Affine::translation(5., -5.)),
        "outer",
    );
    let mut a = rect("a", -10., Color::hex(0xff0000)).with_transform(Affine::scale(3., 3.));
    a.z_index = 2;
    insert(&mut s, a, "inner");
    let mut b = rect("b", -10., Color::hex(0x0000ff)).with_transform(Affine::scale(3., 3.));
    b.z_index = 1;
    insert(&mut s, b, "inner");
    let f = render(&s);
    assert_eq!(at(&f, 25, 25), [128, 0, 0, 128]);
    assert_eq!(at(&f, 15, 10), [0; 4]);
    assert_eq!(at(&f, 35, 25), [0; 4]);
    s.update_object(&"b".into(), |o| o.z_index = 2).unwrap();
    assert_eq!(at(&render(&s), 25, 25), [0, 0, 128, 128]);
}

#[test]
fn container_alpha_masks_include_children_and_clip_without_drawing_mask_surface() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    let mut mask = unpainted_container("mask", display::Frame::new(20., 20.).rounded(8.))
        .with_mask(MaskMode::Alpha);
    mask.style.opacity = 0.5;
    insert(&mut s, mask, "g");
    insert(&mut s, rect("mask-content", 5., Color::WHITE), "mask");
    insert(
        &mut s,
        rect("content", 0., Color::hex(0x0000ff)).with_transform(Affine::scale(2., 2.)),
        "g",
    );
    let f = render(&s);
    assert_eq!(at(&f, 10, 10), [0, 0, 128, 128]);
    assert_eq!(at(&f, 2, 10), [0; 4]);
    assert_eq!(at(&f, 22, 10), [0; 4]);
    s.update_object(&"mask".into(), |o| o.visible = false)
        .unwrap();
    assert!(render(&s).premul_rgba.iter().all(|b| *b == 0));
}

#[test]
fn masks_inside_clipped_container_use_real_parent_backdrop_once() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    insert(&mut s, rect("red", 0., Color::hex(0xff0000)), "g");
    insert(
        &mut s,
        unpainted_container("frame", display::Frame::new(15., 20.)),
        "g",
    );
    insert(
        &mut s,
        unpainted_container("masks", display::Group::new()),
        "frame",
    );
    insert(
        &mut s,
        rect("mask", 0., Color::WHITE).with_mask(MaskMode::Alpha),
        "masks",
    );
    let mut blue = rect("blue", 0., Color::hex(0x0000ff));
    blue.style.blend_mode = BlendMode::Multiply;
    insert(&mut s, blue, "masks");
    let f = render(&s);
    assert_eq!(at(&f, 10, 10), [0, 0, 0, 255]);
    assert_eq!(at(&f, 17, 10), [255, 0, 0, 255]);
    s.update_object(&"mask".into(), |o| o.style.opacity = 0.5)
        .unwrap();
    assert_eq!(at(&render(&s), 10, 10), [127, 0, 128, 255]);
}

#[test]
fn clipped_container_bounds_reduce_allocation_but_never_clip_own_stroke() {
    let mut s = container_scene(display::Frame::new(30., 30.).into());
    s.update_object(&"child".into(), |o| {
        o.shape = display::Rect::new(10_000., 10_000.).into()
    })
    .unwrap();
    let mut v = viewport();
    v.canvas_bounds.size = Size::new(20_000., 20_000.);
    v.content_mask = v.canvas_bounds;
    let f = rasterize(
        &source(&s),
        v,
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert!(f.width <= 40 && f.height <= 40);
    assert_eq!(at(&f, 8, 15), [0, 128, 0, 128]);
    s.update_object(&"container".into(), |o| {
        o.shape = display::Box::new(30., 30.).into()
    })
    .unwrap();
    assert!(matches!(
        rasterize(
            &source(&s),
            v,
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false)
        ),
        Err(CompositeError::TooLarge)
    ));
}

#[test]
fn container_clipping_budget_and_work_errors_release_every_temporary() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    insert(
        &mut s,
        unpainted_container("outer", display::Frame::new(20., 20.)),
        "g",
    );
    insert(
        &mut s,
        unpainted_container("inner", display::Frame::new(20., 20.)),
        "outer",
    );
    insert(&mut s, rect("child", 0., Color::WHITE), "inner");
    for limits in [
        CompositeLimits {
            max_surface_bytes: 4000,
            ..Default::default()
        },
        CompositeLimits {
            max_surface_writes: 500,
            ..Default::default()
        },
        CompositeLimits {
            max_points: 0,
            ..Default::default()
        },
    ] {
        let mut budget = PixelBudget::new(limits, 16).unwrap();
        let err = rasterize(
            &source(&s),
            viewport(),
            &mut budget,
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            CompositeError::WorkingSetLimit
                | CompositeError::WorkLimit
                | CompositeError::InputLimit
        ));
        assert_eq!(budget.retained_bytes(), 16);
    }
}

#[test]
fn container_unsupported_descendants_and_external_backdrop_remain_explicit_errors() {
    let mut s = container_scene(display::Frame::new(30., 30.).into());
    insert(
        &mut s,
        unpainted_container(
            "text",
            display::Text::new("unsupported", Size::new(20., 20.)),
        ),
        "container",
    );
    assert!(matches!(
        rasterize(
            &source(&s),
            viewport(),
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false)
        ),
        Err(CompositeError::TextBackendRequired(_))
    ));
    let mut s = Scene::new();
    let mut f = unpainted_container("frame", display::Frame::new(30., 30.));
    f.style.blend_mode = BlendMode::Multiply;
    s.insert(f, None, None).unwrap();
    assert!(matches!(
        &scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::UnsupportedExternalBackdrop,
            ..
        })
    ));
}

#[test]
fn container_clip_shape_changes_invalidate_placement_but_opacity_only_can_reuse() {
    let mut s = container_scene(display::Frame::new(30., 30.).into());
    let a = source(&s);
    s.update_object(&"container".into(), |o| o.style.opacity = 0.6)
        .unwrap();
    let b = source(&s);
    assert_ne!(a, b);
    assert!(a.placement_eq(&b));
    s.update_object(&"container".into(), |o| {
        if let gpui_flow_scene::Shape::Frame(f) = &mut o.shape {
            f.clip_children = false;
        }
    })
    .unwrap();
    assert!(!b.placement_eq(&source(&s)));
}

#[test]
fn rounded_clip_of_isolated_child_applies_coverage_once_at_fractional_dpr() {
    let mut s = Scene::new();
    s.insert(group("g", 0.5), None, None).unwrap();
    insert(
        &mut s,
        unpainted_container("frame", display::Frame::new(25., 25.).rounded(8.))
            .with_transform(Affine::translation(10.2, 10.3) * Affine::rotation(0.12)),
        "g",
    );
    insert(&mut s, group("isolated", 1.), "frame");
    insert(
        &mut s,
        rect("child", 0., Color::WHITE)
            .with_transform(Affine::translation(-10., -10.) * Affine::scale(3., 3.)),
        "isolated",
    );
    let mut expected = Scene::new();
    expected.insert(group("g", 0.5), None, None).unwrap();
    insert(
        &mut expected,
        DisplayObject::new("rounded", display::Rect::new(25., 25.).rounded(8.))
            .with_transform(Affine::translation(10.2, 10.3) * Affine::rotation(0.12))
            .with_style(Style::fill(Color::WHITE)),
        "g",
    );
    let mut v = viewport();
    v.device_scale = 1.5;
    let raster = |s: &Scene| {
        rasterize(
            &source(s),
            v,
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap()
    };
    let a = raster(&s);
    let b = raster(&expected);
    assert_eq!(a.screen_bounds, b.screen_bounds);
    assert!(
        a.premul_rgba
            .iter()
            .zip(&b.premul_rgba)
            .all(|(a, b)| a.abs_diff(*b) <= 1)
    );
    assert!(
        a.premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0 && p[3] < 128)
    );
}

#[test]
fn static_image_inside_rounded_frame_uses_clip_and_hidden_container_skips_dependency() {
    let mut s = image_scene(display::ImageFit::Fill, 0.5);
    insert(
        &mut s,
        unpainted_container("frame", display::Frame::new(15., 15.).rounded(6.)),
        "g",
    );
    s.reparent(&"image".into(), Some("frame".into()), None, false)
        .unwrap();
    let f = render_pixels(&s, static_pixels(1, 1, &[255, 0, 0, 255]));
    assert_eq!(at(&f, 8, 8), [0, 0, 128, 128]);
    assert_eq!(at(&f, 0, 0), [0; 4]);
    assert!(f.width <= 16);
    s.update_object(&"frame".into(), |o| o.visible = false)
        .unwrap();
    assert!(source(&s).images().is_empty());
    assert!(
        rasterize(
            &source(&s),
            viewport(),
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false)
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn large_world_translation_cancelled_by_viewport_preserves_small_container_clip() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    insert(
        &mut s,
        unpainted_container("frame", display::Frame::new(1., 1.))
            .with_transform(Affine::translation(16_777_216., 0.)),
        "g",
    );
    insert(&mut s, rect("white", 0., Color::WHITE), "frame");
    let mut v = viewport();
    v.world_to_screen = Affine::translation(-16_777_216., 0.);
    let f = rasterize(
        &source(&s),
        v,
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(at(&f, 0, 0), [255; 4]);
}

#[test]
fn nested_clip_screen_blend_keeps_parent_pixels_and_releases_temporary_masks() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    insert(&mut s, rect("red", 0., Color::hex(0xff0000)), "g");
    insert(
        &mut s,
        unpainted_container("outer", display::Frame::new(20., 20.).rounded(4.)),
        "g",
    );
    insert(
        &mut s,
        unpainted_container("inner", display::Frame::new(15., 15.)),
        "outer",
    );
    let mut child = rect("blue", 0., Color::hex(0x0000ff));
    child.style.blend_mode = BlendMode::Screen;
    insert(&mut s, child, "inner");
    let mut budget = PixelBudget::new(CompositeLimits::default(), 16).unwrap();
    let f = rasterize(
        &source(&s),
        viewport(),
        &mut budget,
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(at(&f, 8, 8), [255, 0, 255, 255]);
    assert_eq!(at(&f, 18, 8), [255, 0, 0, 255]);
    assert_eq!(budget.retained_bytes(), 16 + f.premul_rgba.len() as u64);
    assert!(budget.peak_bytes() > budget.retained_bytes());
}

fn text_fixture() -> TextInputs {
    // Synthetic A8-like ink fixture exercises composition independently of fonts.
    // Real glyph production is covered by text_raster_probe, never substituted here.
    let mut rgba = vec![0u8; 20 * 20 * 4];
    for y in 5..15 {
        for x in 5..15 {
            rgba[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4].copy_from_slice(&[0, 0, 255, 255]);
        }
    }
    let image = std::sync::Arc::new(
        StaticImage::from_premultiplied_rgba(20, 20, rgba, CompositeLimits::default()).unwrap(),
    );
    let mut inputs = TextInputs::default();
    inputs.insert(
        "text".into(),
        Ok(Some(std::sync::Arc::new(TextImage {
            image,
            local_bounds: Bounds::new(Point::ZERO, Size::new(20., 20.)),
        }))),
    );
    inputs
}
fn text_scene() -> Scene {
    let mut scene = Scene::new();
    scene.insert(group("g", 0.5), None, None).unwrap();
    insert(
        &mut scene,
        DisplayObject::new("text", display::Text::new("ink", Size::new(20., 20.)))
            .with_style(Style::fill(Color::WHITE)),
        "g",
    );
    scene
}
fn render_text_fixture(
    scene: &Scene,
    inputs: &TextInputs,
) -> Result<Option<CpuFrame>, CompositeError> {
    rasterize_with_inputs(
        &source(scene),
        &ImageInputs::default(),
        inputs,
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
}
#[test]
fn text_pixels_have_real_transparent_gaps_and_group_alpha_is_applied_once() {
    let frame = render_text_fixture(&text_scene(), &text_fixture())
        .unwrap()
        .unwrap();
    assert_eq!(at(&frame, 10, 10), [0, 0, 128, 128]);
    assert_eq!(at(&frame, 2, 2), [0, 0, 0, 0]);
}
#[test]
fn text_mask_uses_glyph_alpha_and_never_its_layout_rectangle() {
    let mut s = text_scene();
    s.update_object(&"text".into(), |o| o.mask = MaskMode::Alpha)
        .unwrap();
    insert(&mut s, rect("red", 0., Color::hex(0xff0000)), "g");
    let frame = render_text_fixture(&s, &text_fixture()).unwrap().unwrap();
    assert_eq!(at(&frame, 10, 10), [128, 0, 0, 128]);
    assert_eq!(at(&frame, 2, 2), [0, 0, 0, 0]);
}
#[test]
fn text_internal_multiply_sees_preceding_siblings_and_keeps_ink_holes() {
    let mut s = text_scene();
    s.update_object(&"g".into(), |o| o.style.opacity = 1.)
        .unwrap();
    s.update_object(&"text".into(), |o| {
        o.style.blend_mode = BlendMode::Multiply;
        o.z_index = 1;
    })
    .unwrap();
    insert(&mut s, rect("red", 0., Color::hex(0xff0000)), "g");
    let frame = render_text_fixture(&s, &text_fixture()).unwrap().unwrap();
    assert_eq!(at(&frame, 10, 10), [0, 0, 0, 255]);
    assert_eq!(at(&frame, 2, 2), [255, 0, 0, 255]);
}
#[test]
fn text_empty_hidden_unsupported_and_bad_pixel_inputs_fail_or_skip_explicitly() {
    let mut s = text_scene();
    let mut none = TextInputs::default();
    none.insert("text".into(), Ok(None));
    assert!(render_text_fixture(&s, &none).unwrap().is_none());
    s.update_object(&"text".into(), |o| o.visible = false)
        .unwrap();
    assert!(
        render_text_fixture(&s, &TextInputs::default())
            .unwrap()
            .is_none()
    );
    s.update_object(&"text".into(), |o| {
        o.visible = true;
        o.style.stroke = Some(gpui_flow_scene::Stroke::new(Color::WHITE, 1.));
    })
    .unwrap();
    assert!(matches!(
        render_text_fixture(&s, &text_fixture()),
        Err(CompositeError::UnsupportedTextStyle(_))
    ));
    assert!(
        StaticImage::from_premultiplied_rgba(1, 1, vec![255, 0, 0, 20], CompositeLimits::default())
            .is_err()
    );
}
#[test]
fn text_pixels_share_working_set_budget_and_failed_domain_leaves_no_partial_output() {
    let s = text_scene();
    let inputs = text_fixture();
    assert_eq!(inputs.byte_len(), 1600);
    let mut budget = PixelBudget::new(
        CompositeLimits {
            max_surface_bytes: 2000,
            ..Default::default()
        },
        20,
    )
    .unwrap();
    assert!(matches!(
        rasterize_with_inputs(
            &source(&s),
            &ImageInputs::default(),
            &inputs,
            viewport(),
            &mut budget,
            &AtomicBool::new(false)
        ),
        Err(CompositeError::WorkingSetLimit)
    ));
    assert_eq!(budget.retained_bytes(), 20);
    let mut budget = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    budget.charge_input_work(32_000_000).unwrap();
    assert!(matches!(
        rasterize_with_inputs(
            &source(&s),
            &ImageInputs::default(),
            &inputs,
            viewport(),
            &mut budget,
            &AtomicBool::new(false)
        ),
        Err(CompositeError::WorkLimit)
    ));
    assert_eq!(budget.retained_bytes(), 0);
}

#[test]
fn aligned_stroke_layers_use_fill_rule_order_and_apply_alpha_once() {
    use gpui_flow_scene::{FillRule, GradientStop, Paint, Stroke, StrokeAlignment, StrokeJoin};
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    let blue = Color::rgba(0, 0, 255, 128);
    let gradient = Paint::LinearGradient {
        start: Point::ZERO,
        end: Point::new(20., 0.),
        stops: vec![
            GradientStop {
                offset: 0.,
                color: blue,
            },
            GradientStop {
                offset: 1.,
                color: blue,
            },
        ],
    };
    insert(
        &mut s,
        DisplayObject::new("r", display::Rect::new(20., 20.))
            .with_transform(Affine::translation(10., 10.))
            .with_style(Style {
                fill: None,
                stroke: Some(
                    Stroke::new(Color::rgba(255, 0, 0, 128), 4.)
                        .with_join(StrokeJoin::Miter)
                        .with_alignment(StrokeAlignment::Inside),
                ),
                stroke_layers: vec![
                    Stroke {
                        paint: gradient,
                        ..Stroke::new(blue, 4.)
                            .with_join(StrokeJoin::Miter)
                            .with_alignment(StrokeAlignment::Outside)
                    },
                    Stroke::new(Color::rgba(0, 255, 0, 128), 2.)
                        .with_join(StrokeJoin::Miter)
                        .with_alignment(StrokeAlignment::Outside),
                ],
                ..Default::default()
            }),
        "g",
    );
    let frame = render(&s);
    assert_eq!(at(&frame, 11, 11), [128, 0, 0, 128]);
    assert_eq!(at(&frame, 7, 7), [0, 0, 128, 128]);
    assert_eq!(at(&frame, 9, 20), [0, 128, 64, 192]);
    assert_eq!(at(&frame, 20, 20), [0, 0, 0, 0]);
    let first = source(&s);
    s.update_object(&"r".into(), |o| o.style.fill_rule = FillRule::EvenOdd)
        .unwrap();
    let second = source(&s);
    assert!(!first.placement_eq(&second));
    s.update_object(&"r".into(), |o| o.style.stroke_layers.reverse())
        .unwrap();
    assert_eq!(at(&render(&s), 9, 20), [0, 64, 128, 192]);
}
#[test]
fn aligned_compound_fill_rule_changes_pixels_and_forbids_stale_placement() {
    use gpui_flow_scene::{FillRule, Stroke, StrokeAlignment, StrokeJoin};
    for data in [
        "M0 0H40V40H0Z M10 10H30V30H10Z",
        "M0 0H40V40H0 M10 10H30V30H10Z",
    ] {
        let mut s = Scene::new();
        s.insert(group("g", 1.), None, None).unwrap();
        insert(
            &mut s,
            DisplayObject::new("p", display::Path::from_svg(data).unwrap())
                .with_transform(Affine::translation(10., 10.))
                .with_style(Style {
                    fill: None,
                    stroke: Some(
                        Stroke::new(Color::rgba(255, 0, 0, 128), 4.)
                            .with_join(StrokeJoin::Miter)
                            .with_alignment(StrokeAlignment::Outside),
                    ),
                    ..Default::default()
                }),
            "g",
        );
        let a = source(&s);
        assert_eq!(at(&render(&s), 21, 30), [0, 0, 0, 0]);
        s.update_object(&"p".into(), |o| o.style.fill_rule = FillRule::EvenOdd)
            .unwrap();
        assert!(!a.placement_eq(&source(&s)));
        assert_eq!(at(&render(&s), 21, 30), [128, 0, 0, 128]);
        // The outside corner is a union of triangles, not accumulated alpha.
        assert_eq!(at(&render(&s), 8, 8), [128, 0, 0, 128]);
    }
}
#[test]
fn aligned_stroke_pixels_match_picking_under_reflection_scale_rotation_and_nested_clips() {
    use gpui_flow_scene::{FillRule, HitTestOptions, Stroke, StrokeAlignment};
    for data in [
        "M0 0H40V40H0Z M10 10H30V30H10Z",
        "M0 0H40V40H0",
        "M0 0H40V40H0 M10 10H30V30H10Z",
    ] {
        for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
            let mut s = Scene::new();
            s.insert(group("g", 1.), None, None).unwrap();
            insert(
                &mut s,
                unpainted_container("outer", display::Frame::new(68., 64.).rounded(14.))
                    .with_transform(Affine::translation(10., 10.)),
                "g",
            );
            insert(
                &mut s,
                unpainted_container("inner", display::Frame::new(58., 56.).rounded(8.))
                    .with_transform(Affine::translation(4., 3.)),
                "outer",
            );
            insert(
                &mut s,
                DisplayObject::new("p", display::Path::from_svg(data).unwrap())
                    .with_transform(
                        Affine::translation(44., 5.)
                            * Affine::rotation(13.)
                            * Affine::scale(-1.1, 1.2),
                    )
                    .with_style(Style {
                        fill: None,
                        fill_rule: FillRule::EvenOdd,
                        stroke: Some(
                            Stroke::new(Color::rgba(255, 0, 0, 128), 4.).with_alignment(alignment),
                        ),
                        ..Default::default()
                    }),
                "inner",
            );
            let frame = render(&s);
            // Build the shared geometry once for the dense pixel oracle; separately
            // check Scene picking on a grid so the test does not benchmark repeated
            // scene validation/tessellation tens of thousands of times.
            let item = s
                .paint_list()
                .unwrap()
                .into_iter()
                .find(|p| p.object.id.as_str() == "p")
                .unwrap();
            let inverse = item.world_transform.inverse().unwrap();
            let mesh = item
                .object
                .shape
                .stroke_mesh_with_rule(
                    item.object.style.stroke.as_ref().unwrap(),
                    item.object.style.fill_rule,
                )
                .unwrap();
            let painted = |p| {
                mesh.hit_test(inverse.transform_point(p), 0.)
                    && item.clips.iter().all(|clip| clip.contains(p))
            };
            let mut verified = 0;
            for y in 12..72 {
                for x in 12..75 {
                    let p = Point::new(x as f32 + 0.5, y as f32 + 0.5);
                    let expected = painted(p);
                    if x % 8 == 0 && y % 8 == 0 {
                        assert_eq!(
                            s.hit_test(p, HitTestOptions::default())
                                .unwrap()
                                .is_some_and(|h| h.id.as_str() == "p"),
                            expected
                        );
                    }
                    // Compare full-coverage pixels away from stroke/clip AA boundaries.
                    // Four corners alone can miss a thin clipped wedge inside a pixel.
                    if [-1.2, -0.6, 0.0, 0.6, 1.2].into_iter().all(|dx| {
                        [-1.2, -0.6, 0.0, 0.6, 1.2]
                            .into_iter()
                            .all(|dy| painted(p + Point::new(dx, dy)) == expected)
                    }) {
                        let alpha = if frame.screen_bounds.contains(p) {
                            at(&frame, x, y)[3]
                        } else {
                            0
                        };
                        assert_eq!(alpha, if expected { 128 } else { 0 }, "{p:?}");
                        verified += 1;
                    }
                }
            }
            assert!(verified > 1000);
        }
    }
}
#[test]
fn aligned_open_strokes_have_no_closure_ink_and_union_retraced_alpha_once() {
    use gpui_flow_scene::{FillRule, Stroke, StrokeAlignment, StrokeCap, StrokeJoin};
    for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
        let mut s = Scene::new();
        s.insert(group("g", 1.), None, None).unwrap();
        insert(
            &mut s,
            DisplayObject::new("p", display::Path::from_svg("M10 10H50V50H10").unwrap())
                .with_style(Style {
                    fill: None,
                    stroke: Some(
                        Stroke::new(Color::rgba(255, 0, 0, 128), 4.)
                            .with_alignment(alignment)
                            .with_cap(StrokeCap::Butt)
                            .with_join(StrokeJoin::Miter),
                    ),
                    ..Default::default()
                }),
            "g",
        );
        let f = render(&s);
        let outside = alignment == StrokeAlignment::Outside;
        assert_eq!(
            at(&f, 25, 12),
            if outside { [0; 4] } else { [128, 0, 0, 128] }
        );
        assert_eq!(at(&f, 11, 30), [0; 4]);
        if outside {
            assert_eq!(at(&f, 25, 7), [128, 0, 0, 128]);
            assert_eq!(at(&f, 52, 8), [128, 0, 0, 128]);
        }
        let first = source(&s);
        s.update_object(&"p".into(), |o| {
            o.shape = display::Path::from_svg("M10 10H50V50H10Z").unwrap().into();
        })
        .unwrap();
        assert!(!first.placement_eq(&source(&s)));
        assert_eq!(
            at(&render(&s), if outside { 8 } else { 11 }, 30),
            [128, 0, 0, 128]
        );
    }
    // The open fill mask of a retraced line is empty under either rule.
    // Outside must union overlapping doubled-stroke triangles with NonZero.
    for rule in [FillRule::NonZero, FillRule::EvenOdd] {
        let mut s = Scene::new();
        s.insert(group("g", 1.), None, None).unwrap();
        insert(
            &mut s,
            DisplayObject::new("p", display::Path::from_svg("M10 30H50H10H50").unwrap())
                .with_style(Style {
                    fill: None,
                    fill_rule: rule,
                    stroke: Some(
                        Stroke::new(Color::rgba(255, 0, 0, 128), 4.)
                            .with_alignment(StrokeAlignment::Outside)
                            .with_cap(StrokeCap::Round),
                    ),
                    ..Default::default()
                }),
            "g",
        );
        let f = render(&s);
        for (x, y) in [(25, 30), (25, 27), (8, 30), (51, 30)] {
            assert_eq!(at(&f, x, y), [128, 0, 0, 128], "{rule:?} ({x}, {y})");
        }
    }
}

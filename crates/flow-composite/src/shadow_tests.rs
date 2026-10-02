use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, DisplayObject, MaskMode, Point, Scene, Shadow, Size, Style,
    display,
};
use std::sync::{Arc, atomic::AtomicBool};
fn scene(blur: f32) -> Scene {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("r", display::Rect::new(10., 10.))
            .with_transform(Affine::translation(20., 20.))
            .with_style(Style {
                fill: Some(Color::hex(0x0000ff).into()),
                shadow: Some(Shadow::new(20., 0., blur, Color::hex(0xff0000))),
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    s
}
fn source(s: &Scene) -> DomainSource {
    match scene_units(s, CompositeLimits::default())
        .unwrap()
        .remove(0)
    {
        SceneUnit::Domain(s) => *s,
        other => panic!("{other:?}"),
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
fn render(s: &Scene) -> CpuFrame {
    rasterize(
        &source(s),
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}
fn pixel(f: &CpuFrame, x: f32, y: f32, dpr: f32) -> [u8; 4] {
    let ix = ((x - f.screen_bounds.origin.x) * dpr).floor() as i64;
    let iy = ((y - f.screen_bounds.origin.y) * dpr).floor() as i64;
    if ix < 0 || iy < 0 || ix >= f.width as i64 || iy >= f.height as i64 {
        return [0; 4];
    }
    f.premul_rgba[((iy as u32 * f.width + ix as u32) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}
#[test]
fn shadow_native_leaf_is_promoted_and_zero_blur_offsets_real_alpha() {
    let s = scene(0.);
    let f = render(&s);
    assert_eq!(pixel(&f, 25., 25., 1.), [0, 0, 255, 255]);
    assert_eq!(pixel(&f, 45., 25., 1.), [255, 0, 0, 255]);
    assert_eq!(pixel(&f, 35., 25., 1.), [0; 4]);
    assert!(f.screen_bounds.max_x() >= 50.);
}
#[test]
fn gaussian_has_symmetric_falloff_and_transparent_truncation_halo() {
    let f = render(&scene(4.));
    let at = |x| pixel(&f, x, 25., 1.)[3];
    assert!(at(38.) > 0 && at(38.) < at(40.) && at(40.) < at(44.));
    assert!((at(38.) as i32 - at(51.) as i32).abs() <= 1);
    assert_eq!(at(60.), 0);
    assert_eq!(pixel(&f, 25., 25., 1.), [0, 0, 255, 255]);
}
#[test]
fn zero_shadow_alpha_skips_filter_work_and_keeps_source_pixels() {
    let mut s = scene(256.);
    s.update_object(&"r".into(), |o| {
        o.style.shadow.as_mut().unwrap().color = Color::TRANSPARENT
    })
    .unwrap();
    let f = render(&s);
    assert_eq!(pixel(&f, 25., 25., 1.), [0, 0, 255, 255]);
    assert_eq!(pixel(&f, 45., 25., 1.), [0; 4]);
    s.update_object(&"r".into(), |o| {
        o.style.fill = Some(Color::TRANSPARENT.into())
    })
    .unwrap();
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
fn translucent_multi_paint_source_and_node_opacity_are_applied_once() {
    let mut s = scene(0.);
    s.update_object(&"r".into(), |o| {
        o.style.fill = Some(Color::rgba(0, 0, 255, 128).into());
        o.style.fill_layers.push(Color::rgba(0, 0, 255, 128).into());
        o.style.opacity = 0.5;
        o.style.shadow.as_mut().unwrap().x = 5.;
    })
    .unwrap();
    let f = render(&s);
    // Source alpha is 192, shadow alpha 192, overlap alpha 240, then node 50%.
    assert!((pixel(&f, 28., 25., 1.)[3] as i32 - 120).abs() <= 1);
    assert!((pixel(&f, 33., 25., 1.)[3] as i32 - 96).abs() <= 1);
}
#[test]
fn image_and_text_shadow_preserve_transparent_ink_holes() {
    for text in [false, true] {
        let mut rgba = vec![0; 12 * 12 * 4];
        for y in 0..12 {
            for x in 0..12 {
                if !(4..8).contains(&x) || !(4..8).contains(&y) {
                    rgba[(y * 12 + x) * 4..(y * 12 + x) * 4 + 4].copy_from_slice(&[0, 0, 255, 255]);
                }
            }
        }
        let image = Arc::new(
            StaticImage::from_premultiplied_rgba(12, 12, rgba, CompositeLimits::default()).unwrap(),
        );
        let mut s = Scene::new();
        let shape: gpui_flow_scene::Shape = if text {
            display::Text::new("ink", Size::new(12., 12.)).into()
        } else {
            display::Image::new("local.png", Size::new(12., 12.)).into()
        };
        s.insert(
            DisplayObject::new("ink", shape)
                .with_transform(Affine::translation(20., 20.))
                .with_style(Style {
                    shadow: Some(Shadow::new(20., 0., 0., Color::hex(0xff0000))),
                    shadow_layers: vec![Shadow::new(40., 0., 0., Color::hex(0x00ff00))],
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
        let mut images = ImageInputs::default();
        let mut texts = TextInputs::default();
        if text {
            texts.insert(
                "ink".into(),
                Ok(Some(Arc::new(TextImage {
                    image,
                    local_bounds: Bounds::new(Point::ZERO, Size::new(12., 12.)),
                }))),
            );
        } else {
            images.insert("ink".into(), Ok(image));
        }
        let f = rasterize_with_inputs(
            &source(&s),
            &images,
            &texts,
            viewport(),
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        assert_eq!(pixel(&f, 42., 22., 1.), [255, 0, 0, 255]);
        assert_eq!(pixel(&f, 46., 26., 1.), [0; 4]);
        assert_eq!(pixel(&f, 26., 26., 1.), [0; 4]);
        assert_eq!(pixel(&f, 62., 22., 1.), [0, 255, 0, 255]);
        assert_eq!(pixel(&f, 66., 26., 1.), [0; 4]);
        assert_eq!(pixel(&f, 82., 22., 1.), [0; 4]);
    }
}
#[test]
fn source_outside_viewport_still_casts_visible_shadow() {
    let mut s = scene(4.);
    s.update_object(&"r".into(), |o| {
        o.transform.tx = -15.;
        o.style.shadow.as_mut().unwrap().x = 30.;
    })
    .unwrap();
    let f = render(&s);
    assert!(pixel(&f, 20., 25., 1.)[3] > 240);
    assert_eq!(pixel(&f, 2., 25., 1.), [0; 4]);
}
#[test]
fn shadow_offset_rotates_and_uniform_scale_dpr_follow_the_same_geometry() {
    let mut s = scene(0.);
    s.update_object(&"r".into(), |o| {
        o.transform = Affine::translation(60., 20.) * Affine::rotation(90.)
    })
    .unwrap();
    let f = render(&s);
    assert_eq!(pixel(&f, 55., 45., 1.), [255, 0, 0, 255]);
    for zoom in [0.5, 1., 2.] {
        for dpr in [1., 1.25, 2.] {
            let s = scene(0.);
            let mut v = viewport();
            v.device_scale = dpr;
            v.world_to_screen = Affine::scale(zoom, zoom);
            let f = rasterize(
                &source(&s),
                v,
                &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
                &AtomicBool::new(false),
            )
            .unwrap()
            .unwrap();
            assert_eq!(pixel(&f, 45. * zoom, 25. * zoom, dpr), [255, 0, 0, 255]);
            assert_eq!(pixel(&f, 25. * zoom, 25. * zoom, dpr), [0, 0, 255, 255]);
        }
    }
}
#[test]
fn inherited_rounded_clip_cuts_shadow_after_blur_not_before_source_sampling() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("frame", display::Frame::new(40., 40.).rounded(10.)).with_style(Style {
            fill: None,
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    let mut o = scene(4.).get(&"r".into()).unwrap().object.clone();
    o.transform = Affine::translation(42., 15.);
    o.style.shadow.as_mut().unwrap().x = -20.;
    s.insert(o, Some("frame".into()), None).unwrap();
    let domain = scene_units(&s, CompositeLimits::default())
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
    let f = rasterize(
        &domain,
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert!(pixel(&f, 27., 20., 1.)[3] > 240);
    assert_eq!(pixel(&f, 43., 20., 1.), [0; 4]);
    assert_eq!(pixel(&f, 1., 1., 1.), [0; 4]);
}
#[test]
fn artistic_blend_in_owned_canvas_applies_to_shadow_and_source_together() {
    let mut s = scene(0.);
    s.update_object(&"r".into(), |o| o.style.blend_mode = BlendMode::Multiply)
        .unwrap();
    assert!(matches!(
        &scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::UnsupportedExternalBackdrop,
            ..
        })
    ));
    let d = owned_canvas(
        &s,
        CanvasBackdrop {
            color: Color::hex(0xffff00),
            grid: None,
        },
        CompositeLimits::default(),
    )
    .unwrap();
    let f = rasterize(
        &d,
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(pixel(&f, 45., 25., 1.), [255, 0, 0, 255]);
    assert_eq!(pixel(&f, 25., 25., 1.), [0, 0, 0, 255]);
}
#[test]
fn unsupported_shadow_shapes_and_transforms_reject_without_native_fallback() {
    for a in [
        Affine::scale(1., 2.),
        Affine::skew(5., 0.),
        Affine::scale(-1., 1.),
    ] {
        let mut s = scene(1.);
        s.update_object(&"r".into(), |o| {
            o.transform = Affine::translation(50., 20.) * a
        })
        .unwrap();
        assert!(matches!(
            rasterize(
                &source(&s),
                viewport(),
                &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
                &AtomicBool::new(false)
            ),
            Err(CompositeError::UnsupportedTransform)
        ));
    }
}
#[test]
fn all_shadow_scratch_and_filter_work_are_admitted_before_allocation() {
    for (limits, error) in [
        (
            CompositeLimits {
                max_surface_bytes: 10_000,
                ..Default::default()
            },
            CompositeError::WorkingSetLimit,
        ),
        (
            CompositeLimits {
                max_surface_writes: 100,
                ..Default::default()
            },
            CompositeError::WorkLimit,
        ),
    ] {
        let mut budget = PixelBudget::new(limits, 0).unwrap();
        let result = rasterize(
            &source(&scene(4.)),
            viewport(),
            &mut budget,
            &AtomicBool::new(false),
        );
        assert_eq!(result.unwrap_err(), error);
        assert_eq!(budget.retained_bytes(), 0);
    }
    let mut budget = PixelBudget::new(CompositeLimits::default(), 7).unwrap();
    let f = rasterize(
        &source(&scene(4.)),
        viewport(),
        &mut budget,
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert!(budget.peak_bytes() > budget.retained_bytes());
    assert_eq!(budget.retained_bytes(), f.premul_rgba.len() as u64 + 7);
    budget.discard_frame(f);
    assert_eq!(budget.retained_bytes(), 7);
}
#[test]
fn shadow_device_limits_cancellation_and_cache_placement_are_explicit() {
    for (blur, zoom) in [(256., 1.), (1., 0.0001)] {
        let s = scene(blur);
        let mut v = viewport();
        v.world_to_screen = Affine::scale(zoom, zoom);
        let mut budget = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
        assert_eq!(
            rasterize(&source(&s), v, &mut budget, &AtomicBool::new(false)).unwrap_err(),
            CompositeError::TooLarge
        );
        assert_eq!(budget.retained_bytes(), 0);
    }
    let s = scene(2.);
    let mut budget = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    assert_eq!(
        rasterize(&source(&s), viewport(), &mut budget, &AtomicBool::new(true)).unwrap_err(),
        CompositeError::Superseded
    );
    assert_eq!(budget.peak_bytes(), 0);
    let a = source(&s);
    let mut next = s.clone();
    next.update_object(&"r".into(), |o| o.style.shadow.as_mut().unwrap().x = 21.)
        .unwrap();
    assert!(!a.placement_eq(&source(&next)));
}
#[test]
fn alpha_mask_uses_shadow_coverage_and_group_opacity_is_once() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("g", display::Group::new()).with_style(Style {
            opacity: 0.5,
            blend_mode: BlendMode::Normal,
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    let mut mask = scene(0.).get(&"r".into()).unwrap().object.clone();
    mask.mask = MaskMode::Alpha;
    s.insert(mask, Some("g".into()), None).unwrap();
    s.insert(
        DisplayObject::new("paint", display::Rect::new(80., 80.))
            .with_style(Style::fill(Color::WHITE)),
        Some("g".into()),
        None,
    )
    .unwrap();
    let f = render(&s);
    assert_eq!(pixel(&f, 45., 25., 1.), [128; 4]);
    assert_eq!(pixel(&f, 35., 25., 1.), [0; 4]);
}

#[test]
fn fractional_rounded_child_clip_applies_once_to_shadow_source_overlap() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("g", display::Group::new()).with_style(Style {
            blend_mode: BlendMode::Normal,
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("clip", display::Frame::new(27., 27.).rounded(9.))
            .with_transform(Affine::translation(17.3, 17.3))
            .with_style(Style {
                fill: None,
                ..Default::default()
            }),
        Some("g".into()),
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("ink", display::Rect::new(40., 40.))
            .with_transform(Affine::translation(-5., -5.))
            .with_style(Style {
                fill: Some(Color::hex(0x0000ff).into()),
                shadow: Some(Shadow::new(0., 0., 0., Color::hex(0xff0000))),
                ..Default::default()
            }),
        Some("clip".into()),
        None,
    )
    .unwrap();
    let mut v = viewport();
    v.device_scale = 1.25;
    let render = |s: &Scene| {
        rasterize(
            &source(s),
            v,
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap()
    };
    let f = render(&s);
    assert!(
        f.premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0 && p[3] < 255)
    );
    assert!(
        f.premul_rgba.as_chunks::<4>().0.iter().all(|p| p[0] == 0),
        "red shadow must stay hidden behind opaque blue, even at antialiased clip edges"
    );
    s.update_object(&"ink".into(), |o| o.style.blend_mode = BlendMode::Normal)
        .unwrap();
    let explicit = render(&s);
    assert_eq!(f.screen_bounds, explicit.screen_bounds);
    assert_eq!(f.premul_rgba, explicit.premul_rgba);
}

#[test]
fn blurred_shadow_matches_equivalent_zoom_dpr_and_object_scale() {
    for scale in [0.5, 1.25, 2.] {
        let s = scene(4.);
        let render = |s: &Scene, v| {
            rasterize(
                &source(s),
                v,
                &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
                &AtomicBool::new(false),
            )
            .unwrap()
            .unwrap()
        };
        let mut dpr = viewport();
        dpr.device_scale = scale;
        let physical = render(&s, dpr);
        let mut zoom = viewport();
        zoom.world_to_screen = Affine::scale(scale, scale);
        zoom.canvas_bounds.size = Size::new(100. * scale, 100. * scale);
        zoom.content_mask = zoom.canvas_bounds;
        let logical = render(&s, zoom);
        assert_eq!(
            (physical.width, physical.height),
            (logical.width, logical.height)
        );
        assert_eq!(physical.premul_rgba, logical.premul_rgba);
        let mut object = s.clone();
        object
            .update_object(&"r".into(), |o| {
                o.transform = Affine::scale(scale, scale) * o.transform
            })
            .unwrap();
        let mut object_view = viewport();
        object_view.canvas_bounds = zoom.canvas_bounds;
        object_view.content_mask = zoom.content_mask;
        let transformed = render(&object, object_view);
        assert_eq!(logical.premul_rgba, transformed.premul_rgba);
    }
}

#[test]
fn aligned_stroke_shadow_uses_the_hollow_final_mesh() {
    let mut s = scene(0.);
    s.update_object(&"r".into(), |o| {
        o.style.fill = None;
        o.style.stroke = Some(
            gpui_flow_scene::Stroke::new(Color::WHITE, 2.)
                .with_alignment(gpui_flow_scene::StrokeAlignment::Outside),
        );
    })
    .unwrap();
    let f = render(&s);
    assert_eq!(pixel(&f, 39., 25., 1.), [255, 0, 0, 255]);
    assert_eq!(pixel(&f, 45., 25., 1.), [0; 4]);
    assert_eq!(pixel(&f, 51., 25., 1.), [255, 0, 0, 255]);
}

#[test]
fn fractional_shadow_offset_uses_premultiplied_bilinear_coverage() {
    let mut s = scene(0.);
    s.update_object(&"r".into(), |o| o.style.shadow.as_mut().unwrap().x = 20.5)
        .unwrap();
    let f = render(&s);
    let p = pixel(&f, 40., 25., 1.);
    assert!(
        (p[3] as i32 - 128).abs() <= 1,
        "pixel={p:?}, bounds={:?}, neighbors={:?}",
        f.screen_bounds,
        (38..44)
            .map(|x| pixel(&f, x as f32, 25., 1.))
            .collect::<Vec<_>>()
    );
    assert_eq!(p[0], p[3]);
    assert_eq!(p[1], 0);
    assert_eq!(p[2], 0);
    assert_eq!(pixel(&f, 45., 25., 1.), [255, 0, 0, 255]);
}
#[test]
fn group_shadow_preserves_image_and_text_descendant_ink_holes() {
    for text in [false, true] {
        let mut rgba = vec![0; 12 * 12 * 4];
        for y in 0..12 {
            for x in 0..12 {
                if !(4..8).contains(&x) || !(4..8).contains(&y) {
                    rgba[(y * 12 + x) * 4..(y * 12 + x) * 4 + 4].copy_from_slice(&[0, 0, 255, 255]);
                }
            }
        }
        let image = Arc::new(
            StaticImage::from_premultiplied_rgba(12, 12, rgba, CompositeLimits::default()).unwrap(),
        );
        let mut s = Scene::new();
        s.insert(
            DisplayObject::new("parent", display::Group::new()).with_style(Style {
                shadow: Some(Shadow::new(20., 0., 0., Color::hex(0xff0000))),
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
        let shape: gpui_flow_scene::Shape = if text {
            display::Text::new("ink", Size::new(12., 12.)).into()
        } else {
            display::Image::new("local.png", Size::new(12., 12.)).into()
        };
        s.insert(
            DisplayObject::new("ink", shape)
                .with_transform(Affine::translation(20., 20.))
                .with_style(Style {
                    shadow: None,
                    ..Default::default()
                }),
            Some("parent".into()),
            None,
        )
        .unwrap();
        let mut images = ImageInputs::default();
        let mut texts = TextInputs::default();
        if text {
            texts.insert(
                "ink".into(),
                Ok(Some(Arc::new(TextImage {
                    image,
                    local_bounds: Bounds::new(Point::ZERO, Size::new(12., 12.)),
                }))),
            );
        } else {
            images.insert("ink".into(), Ok(image));
        }
        let f = rasterize_with_inputs(
            &source(&s),
            &images,
            &texts,
            viewport(),
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        assert_eq!(pixel(&f, 42., 22., 1.), [255, 0, 0, 255]);
        assert_eq!(pixel(&f, 46., 26., 1.), [0; 4]);
        assert_eq!(pixel(&f, 26., 26., 1.), [0; 4]);
    }
}

#[test]
fn aligned_open_stroke_shadows_never_add_the_implicit_closing_edge() {
    use gpui_flow_scene::{Stroke, StrokeAlignment, StrokeCap, StrokeJoin};
    for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
        let mut s = scene(0.);
        s.update_object(&"r".into(), |o| {
            o.shape = display::Path::from_svg("M0 0H10V10H0").unwrap().into();
            o.style.fill = None;
            o.style.stroke = Some(
                Stroke::new(Color::WHITE, 2.)
                    .with_alignment(alignment)
                    .with_cap(StrokeCap::Butt)
                    .with_join(StrokeJoin::Miter),
            );
        })
        .unwrap();
        let f = render(&s);
        let outside = alignment == StrokeAlignment::Outside;
        assert_eq!(pixel(&f, if outside { 39. } else { 41. }, 25., 1.), [0; 4]);
        assert_eq!(
            pixel(&f, 45., if outside { 19. } else { 21. }, 1.),
            [255, 0, 0, 255]
        );
        assert_eq!(
            pixel(&f, if outside { 51. } else { 49. }, 25., 1.),
            [255, 0, 0, 255]
        );
    }
}

#[test]
fn aligned_empty_inside_has_neither_source_ink_nor_blurred_shadow() {
    use gpui_flow_scene::{FillRule, Stroke, StrokeAlignment};
    for rule in [FillRule::NonZero, FillRule::EvenOdd] {
        let mut s = scene(4.);
        s.update_object(&"r".into(), |o| {
            o.shape = display::Path::from_svg("M0 0L10 10L20 20L0 0")
                .unwrap()
                .into();
            o.style.fill = None;
            o.style.fill_rule = rule;
            o.style.stroke =
                Some(Stroke::new(Color::WHITE, 4.).with_alignment(StrokeAlignment::Inside));
        })
        .unwrap();
        let frame = rasterize(
            &source(&s),
            viewport(),
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(frame.is_none_or(|frame| frame.premul_rgba.iter().all(|&channel| channel == 0)));
    }
}

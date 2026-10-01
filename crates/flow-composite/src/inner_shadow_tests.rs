use super::*;
use gpui_flow_scene::{display, *};
use std::sync::{Arc, atomic::AtomicBool};

fn source(scene: &Scene) -> DomainSource {
    match scene_units(scene, CompositeLimits::default())
        .unwrap()
        .remove(0)
    {
        SceneUnit::Domain(source) => *source,
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
fn scene(alpha: u8, shadows: Vec<Shadow>) -> Scene {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("r", display::Rect::new(20., 20.))
            .with_transform(Affine::translation(20., 20.))
            .with_style(Style {
                fill: Some(Color::rgba(0, 0, 255, alpha).into()),
                inner_shadow_layers: shadows,
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    s
}
fn red(x: f32, blur: f32) -> Shadow {
    Shadow::new(x, 0., blur, Color::rgb(255, 0, 0))
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
fn pixel(f: &CpuFrame, x: f32, y: f32) -> [u8; 4] {
    let x = (x - f.screen_bounds.origin.x).floor() as i64;
    let y = (y - f.screen_bounds.origin.y).floor() as i64;
    if x < 0 || y < 0 || x >= f.width as i64 || y >= f.height as i64 {
        return [0; 4];
    }
    f.premul_rgba[((y as u32 * f.width + x as u32) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}

#[test]
fn zero_offset_zero_blur_has_explicit_opaque_and_translucent_math() {
    assert_eq!(
        pixel(&render(&scene(255, vec![red(0., 0.)])), 30., 30.),
        [0, 0, 255, 255]
    );
    // A=128; inner=round(128*(255-128)/255)=64. Red over blue:
    // alpha=64+128*(1-64/255)=160, blue=96, before local opacity.
    assert_eq!(
        pixel(&render(&scene(128, vec![red(0., 0.)])), 30., 30.),
        [64, 0, 96, 160]
    );
    let mut s = scene(128, vec![red(0., 0.)]);
    s.update_object(&"r".into(), |o| {
        o.style.opacity = 0.5;
        o.style.inner_shadow_layers[0].color.a = 128;
    })
    .unwrap();
    let p = pixel(&render(&s), 30., 30.);
    assert_eq!(p, [16, 0, 56, 72]);
}

#[test]
fn offset_and_gaussian_tint_only_real_interior_with_no_expanded_bounds() {
    let hard = render(&scene(255, vec![red(4., 0.)]));
    assert_eq!(pixel(&hard, 21., 30.), [255, 0, 0, 255]);
    assert_eq!(pixel(&hard, 26., 30.), [0, 0, 255, 255]);
    assert_eq!(pixel(&hard, 19., 30.), [0; 4]);
    assert_eq!(pixel(&hard, 41., 30.), [0; 4]);
    let soft = render(&scene(255, vec![red(0., 4.)]));
    let edge = pixel(&soft, 20., 30.);
    let center = pixel(&soft, 30., 30.);
    assert!(edge[0] > 50 && edge[0] < 255);
    assert!(center[0] < edge[0]);
    assert_eq!(pixel(&soft, 19., 30.), [0; 4]);
    assert_eq!(soft.screen_bounds, hard.screen_bounds);
}

#[test]
fn inner_layer_colors_are_ordered_and_all_sample_the_original_alpha() {
    let a = Shadow::new(40., 0., 0., Color::rgba(255, 0, 0, 128));
    let b = Shadow::new(40., 0., 0., Color::rgba(0, 255, 0, 128));
    assert_eq!(
        pixel(&render(&scene(255, vec![a, b])), 30., 30.),
        [64, 128, 63, 255]
    );
    assert_eq!(
        pixel(&render(&scene(255, vec![b, a])), 30., 30.),
        [128, 64, 63, 255]
    );
    // Second translucent zero-shift shadow must still use A=128, not the
    // alpha=160 result produced by the first layer.
    let f = render(&scene(
        128,
        vec![red(0., 0.), Shadow::new(0., 0., 0., Color::rgb(0, 255, 0))],
    ));
    assert_eq!(pixel(&f, 30., 30.), [48, 64, 72, 184]);
}

#[test]
fn fractional_offsets_and_edge_antialiasing_survive_inner_clipping() {
    let f = render(&scene(255, vec![red(0.5, 0.)]));
    let edge = pixel(&f, 20., 30.);
    assert!((edge[0] as i32 - 127).abs() <= 1, "{edge:?}");
    assert_eq!(pixel(&f, 21., 30.), [0, 0, 255, 255]);
    let mut s = scene(255, vec![red(40., 0.)]);
    s.update_object(&"r".into(), |o| {
        o.shape = display::Ellipse::circle(10.).into();
        o.transform.tx += 0.25;
    })
    .unwrap();
    let f = render(&s);
    assert_eq!(pixel(&f, 20., 20.), [0; 4]);
    assert!(
        f.premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0] > 0 && p[3] > 0 && p[3] < 255)
    );
    assert!(
        f.premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[0] <= p[3] && p[2] <= p[3])
    );
}

#[test]
fn inner_paints_after_fill_before_strokes_without_self_outer_feedback() {
    let mut s = scene(255, vec![red(40., 0.)]);
    s.update_object(&"r".into(), |o| {
        o.style.stroke = Some(Stroke::new(Color::rgb(0, 255, 0), 6.))
    })
    .unwrap();
    let f = render(&s);
    assert_eq!(pixel(&f, 30., 30.), [255, 0, 0, 255]);
    assert_eq!(pixel(&f, 21., 30.), [0, 255, 0, 255]);
    let mut s = scene(128, vec![red(0., 0.)]);
    s.update_object(&"r".into(), |o| {
        o.style.shadow = Some(Shadow::new(40., 0., 0., Color::BLACK))
    })
    .unwrap();
    let f = render(&s);
    assert_eq!(pixel(&f, 30., 30.), [64, 0, 96, 160]);
    assert_eq!(pixel(&f, 70., 30.), [0, 0, 0, 128]);
    assert_eq!(pixel(&f, 50., 30.), [0; 4]);
}

#[test]
fn image_and_text_holes_and_partial_source_alpha_are_preserved() {
    for text in [false, true] {
        let mut rgba = vec![0; 12 * 12 * 4];
        for y in 0..12 {
            for x in 0..12 {
                if !(4..8).contains(&x) || !(4..8).contains(&y) {
                    rgba[(y * 12 + x) * 4..][..4].copy_from_slice(&[0, 0, 128, 128]);
                }
            }
        }
        let image = Arc::new(
            StaticImage::from_premultiplied_rgba(12, 12, rgba, CompositeLimits::default()).unwrap(),
        );
        let shape: Shape = if text {
            display::Text::new("ring", Size::new(12., 12.)).into()
        } else {
            display::Image::new("ring.png", Size::new(12., 12.)).into()
        };
        let mut s = Scene::new();
        s.insert(
            DisplayObject::new("ink", shape)
                .with_transform(Affine::translation(20., 20.))
                .with_style(Style {
                    inner_shadow_layers: vec![red(0., 0.), red(1., 3.)],
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
        assert_eq!(pixel(&f, 26., 26.), [0; 4]);
        assert_eq!(pixel(&f, 19., 22.), [0; 4]);
        let p = pixel(&f, 22., 22.);
        assert!(p[0] >= 64 && p[3] >= 160 && p[3] < 255, "{p:?}");
    }
}

#[test]
fn parent_outer_source_includes_child_inner_but_never_extends_geometry() {
    let mut s = scene(128, vec![red(0., 0.)]);
    s.insert(
        DisplayObject::new("g", display::Group::new()).with_style(Style {
            shadow: Some(Shadow::new(40., 0., 0., Color::BLACK)),
            ..Default::default()
        }),
        None,
        Some(0),
    )
    .unwrap();
    s.reparent(&"r".into(), Some("g".into()), None, true)
        .unwrap();
    let f = render(&s);
    assert_eq!(pixel(&f, 30., 30.), [64, 0, 96, 160]);
    assert_eq!(pixel(&f, 70., 30.), [0, 0, 0, 160]);
}

#[test]
fn unsupported_container_declarations_reject_including_transparent_effects() {
    for shape in [
        Shape::Group(display::Group::new()),
        display::Box::new(20., 20.).into(),
        display::Frame::new(20., 20.).into(),
        display::Custom::new(
            "custom",
            Bounds::new(Point::ZERO, Size::new(20., 20.)),
            vec![],
        )
        .into(),
    ] {
        for color in [Color::BLACK, Color::TRANSPARENT] {
            let mut s = Scene::new();
            s.insert(
                DisplayObject::new("bad", shape.clone()).with_style(Style {
                    inner_shadow_layers: vec![Shadow::new(0., 0., 0., color)],
                    ..Default::default()
                }),
                None,
                None,
            )
            .unwrap();
            assert!(
                matches!(&scene_units(&s, CompositeLimits::default()).unwrap()[..], [SceneUnit::Rejected(CompositeIssue { error: CompositeError::UnsupportedInnerShadow(id), .. })] if id.as_str() == "bad")
            );
        }
    }
}

#[test]
fn vector_holes_sibling_mask_and_ancestor_clip_preserve_empty_pixels() {
    use display::PathCommand::*;
    let rect = |x: f32, y: f32, w: f32| {
        vec![
            MoveTo {
                to: Point::new(x, y),
            },
            LineTo {
                to: Point::new(x + w, y),
            },
            LineTo {
                to: Point::new(x + w, y + w),
            },
            LineTo {
                to: Point::new(x, y + w),
            },
            Close,
        ]
    };
    let mut commands = rect(0., 0., 20.);
    commands.extend(rect(6., 6., 8.));
    let mut s = scene(255, vec![red(40., 4.)]);
    s.update_object(&"r".into(), |o| {
        o.shape = display::Path::new(commands).into();
        o.style.fill_rule = FillRule::EvenOdd;
    })
    .unwrap();
    let f = render(&s);
    assert_eq!(pixel(&f, 30., 30.), [0; 4]);
    assert_eq!(pixel(&f, 22., 22.), [255, 0, 0, 255]);
    s.insert(
        DisplayObject::new("g", display::Frame::new(14., 20.))
            .with_transform(Affine::translation(20., 20.))
            .with_style(Style {
                fill: None,
                opacity: 0.5,
                ..Default::default()
            }),
        None,
        Some(0),
    )
    .unwrap();
    s.reparent(&"r".into(), Some("g".into()), None, true)
        .unwrap();
    let f = render(&s);
    assert_eq!(pixel(&f, 36., 22.), [0; 4]);
    assert_eq!(pixel(&f, 22., 22.), [128, 0, 0, 128]);
    let mut s = scene(255, vec![red(40., 0.)]);
    s.insert(
        DisplayObject::new("g", display::Group::new()),
        None,
        Some(0),
    )
    .unwrap();
    s.reparent(&"r".into(), Some("g".into()), None, true)
        .unwrap();
    let mut mask = DisplayObject::new("mask", display::Rect::new(10., 20.))
        .with_transform(Affine::translation(20., 20.));
    mask.mask = MaskMode::Alpha;
    s.insert(mask, Some("g".into()), Some(0)).unwrap();
    let f = render(&s);
    assert_eq!(pixel(&f, 25., 25.), [255, 0, 0, 255]);
    assert_eq!(pixel(&f, 35., 25.), [0; 4]);
}

#[test]
fn offscreen_text_alpha_is_requested_for_self_or_ancestor_effects() {
    for ancestor in [false, true] {
        let mut s = Scene::new();
        if ancestor {
            s.insert(
                DisplayObject::new("g", display::Group::new()).with_style(Style {
                    shadow: Some(Shadow::new(20., 0., 0., Color::BLACK)),
                    ..Default::default()
                }),
                None,
                None,
            )
            .unwrap();
        }
        s.insert(
            DisplayObject::new("t", display::Text::new("ink", Size::new(20., 10.)))
                .with_transform(Affine::translation(-10., 20.))
                .with_style(Style {
                    inner_shadow_layers: if ancestor { vec![] } else { vec![red(5., 0.)] },
                    ..Default::default()
                }),
            ancestor.then(|| "g".into()),
            None,
        )
        .unwrap();
        let src = source(&s);
        assert!(src.text_sources()[0].2);
        let mut texts = TextInputs::default();
        let image = Arc::new(
            StaticImage::from_premultiplied_rgba(
                20,
                10,
                [0, 0, 255, 255].repeat(200),
                CompositeLimits::default(),
            )
            .unwrap(),
        );
        texts.insert(
            "t".into(),
            Ok(Some(Arc::new(TextImage {
                image,
                local_bounds: Bounds::new(Point::ZERO, Size::new(20., 10.)),
            }))),
        );
        let f = rasterize_with_inputs(
            &src,
            &ImageInputs::default(),
            &texts,
            viewport(),
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        assert_eq!(pixel(&f, 2., 25.), [0, 0, 255, 255]); // no false viewport-edge inner tint
        if ancestor {
            assert_eq!(pixel(&f, 12., 25.), [0, 0, 0, 255]);
        }
        s.update_object(&if ancestor { "g" } else { "t" }.into(), |o| {
            o.style.shadow = None;
            o.style.inner_shadow_layers.clear();
            o.style.blend_mode = BlendMode::Normal;
        })
        .unwrap();
        assert!(!source(&s).text_sources()[0].2);
    }
}

#[test]
fn deep_outer_replays_of_inner_sources_reject_without_allocating() {
    let mut s = Scene::new();
    for depth in 0..8 {
        s.insert(
            DisplayObject::new(format!("g{depth}"), display::Group::new()).with_style(Style {
                shadow_layers: vec![Shadow::new(0., 0., 0., Color::BLACK); 8],
                ..Default::default()
            }),
            (depth > 0).then(|| format!("g{}", depth - 1).into()),
            None,
        )
        .unwrap();
    }
    s.insert(
        DisplayObject::new("r", display::Rect::new(10., 10.)).with_style(Style {
            inner_shadow_layers: vec![red(1., 0.); 8],
            ..Default::default()
        }),
        Some("g7".into()),
        None,
    )
    .unwrap();
    let mut b = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    assert!(matches!(
        rasterize(&source(&s), viewport(), &mut b, &AtomicBool::new(false)),
        Err(CompositeError::WorkLimit | CompositeError::WorkingSetLimit)
    ));
    assert_eq!(b.peak_bytes(), 0);
    assert_eq!(b.surface_writes(), 0);
}

#[test]
fn ancestor_rounded_clip_is_applied_once_at_fractional_dpr() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("f", display::Frame::new(20., 20.).rounded(8.))
            .with_transform(Affine::translation(20.25, 20.25))
            .with_style(Style {
                fill: None,
                blend_mode: BlendMode::Normal,
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("r", display::Rect::new(50., 50.))
            .with_transform(Affine::translation(-10., -10.))
            .with_style(Style {
                fill: Some(Color::rgb(0, 0, 255).into()),
                inner_shadow_layers: vec![red(100., 0.)],
                ..Default::default()
            }),
        Some("f".into()),
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
    let actual = render(&s);
    s.update_object(&"r".into(), |o| {
        o.style.inner_shadow_layers.clear();
        o.style.fill = Some(Color::rgb(255, 0, 0).into());
    })
    .unwrap();
    let expected = render(&s);
    assert_eq!(actual.screen_bounds, expected.screen_bounds);
    assert_eq!(actual.premul_rgba, expected.premul_rgba);
    assert!(
        actual
            .premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0 && p[3] < 255)
    );
}

#[test]
fn inner_mask_alpha_and_owned_blend_use_final_leaf_result() {
    let mut s = Scene::new();
    s.insert(DisplayObject::new("g", display::Group::new()), None, None)
        .unwrap();
    let mut mask = DisplayObject::new("m", display::Rect::new(20., 20.)).with_style(Style {
        fill: Some(Color::rgba(0, 0, 255, 128).into()),
        inner_shadow_layers: vec![red(0., 0.)],
        ..Default::default()
    });
    mask.mask = MaskMode::Alpha;
    s.insert(mask, Some("g".into()), None).unwrap();
    s.insert(
        DisplayObject::new("r", display::Rect::new(20., 20.)).with_style(Style::fill(Color::WHITE)),
        Some("g".into()),
        None,
    )
    .unwrap();
    assert_eq!(pixel(&render(&s), 10., 10.), [160; 4]);
    let mut s = scene(255, vec![red(40., 0.)]);
    s.update_object(&"r".into(), |o| o.style.blend_mode = BlendMode::Multiply)
        .unwrap();
    let owned = owned_canvas(
        &s,
        CanvasBackdrop {
            color: Color::rgb(128, 128, 128),
            grid: None,
        },
        CompositeLimits::default(),
    )
    .unwrap();
    let f = rasterize(
        &owned,
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(pixel(&f, 30., 30.), [128, 0, 0, 255]);
}

#[test]
fn dormant_unsupported_and_active_transform_rejection_are_explicit() {
    let mut s = Scene::new();
    let mut object = DisplayObject::new("g", display::Group::new()).with_style(Style {
        inner_shadow_layers: vec![red(1., 0.)],
        ..Default::default()
    });
    object.visible = false;
    s.insert(object, None, None).unwrap();
    assert!(
        scene_units(&s, CompositeLimits::default())
            .unwrap()
            .is_empty()
    );
    s.update_object(&"g".into(), |o| o.visible = true).unwrap();
    assert!(matches!(
        &scene_units(&s, CompositeLimits::default()).unwrap()[..],
        [SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::UnsupportedInnerShadow(_),
            ..
        })]
    ));
    let mut s = scene(255, vec![red(1., 0.)]);
    s.update_object(&"r".into(), |o| {
        o.transform = Affine::translation(40., 20.) * Affine::scale(-1., 1.)
    })
    .unwrap();
    let mut b = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    assert!(matches!(
        rasterize(&source(&s), viewport(), &mut b, &AtomicBool::new(false)),
        Err(CompositeError::UnsupportedTransform)
    ));
    assert_eq!(b.peak_bytes(), 0);
    assert_eq!(b.surface_writes(), 0);
}

#[test]
fn transparent_layers_skip_filters_and_later_invalid_layers_reject_before_allocation() {
    let s = scene(255, vec![Shadow::new(4096., 0., 256., Color::TRANSPARENT)]);
    let mut b = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    let f = rasterize(&source(&s), viewport(), &mut b, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(b.peak_bytes(), f.premul_rgba.len() as u64);
    let s = scene(255, vec![red(1., 0.), red(0., 256.)]);
    let mut b = PixelBudget::new(CompositeLimits::default(), 7).unwrap();
    assert!(matches!(
        rasterize(&source(&s), viewport(), &mut b, &AtomicBool::new(false)),
        Err(CompositeError::TooLarge)
    ));
    assert_eq!(b.peak_bytes(), 7);
    assert_eq!(b.retained_bytes(), 7);
    assert_eq!(b.surface_writes(), 0);
}

#[test]
fn inner_admission_matches_runtime_bytes_work_and_depth_before_pixels() {
    let s = scene(128, vec![red(2., 0.), red(-1., 3.)]);
    let src = source(&s);
    let mut b = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    let f = rasterize(&src, viewport(), &mut b, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    let limits = CompositeLimits {
        max_surface_bytes: b.peak_bytes(),
        max_surface_writes: b.surface_writes(),
        ..Default::default()
    };
    assert!(
        rasterize(
            &src,
            viewport(),
            &mut PixelBudget::new(limits, 0).unwrap(),
            &AtomicBool::new(false)
        )
        .unwrap()
        .is_some()
    );
    for limits in [
        CompositeLimits {
            max_surface_bytes: limits.max_surface_bytes - 1,
            ..limits
        },
        CompositeLimits {
            max_surface_writes: limits.max_surface_writes - 1,
            ..limits
        },
        CompositeLimits {
            max_depth: 0,
            ..limits
        },
    ] {
        let mut b = PixelBudget::new(limits, 0).unwrap();
        assert!(matches!(
            rasterize(&src, viewport(), &mut b, &AtomicBool::new(false)),
            Err(CompositeError::WorkingSetLimit
                | CompositeError::WorkLimit
                | CompositeError::NestingLimit)
        ));
        assert_eq!(b.peak_bytes(), 0);
        assert_eq!(b.retained_bytes(), 0);
        assert_eq!(b.surface_writes(), 0);
    }
    assert!(f.premul_rgba.len() > 4);
}

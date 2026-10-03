use super::*;
use gpui_flow_scene::{display, *};
use std::sync::{Arc, atomic::AtomicBool};

fn effect(spread: f32, knockout: bool, fixed: bool) -> Shadow {
    Shadow {
        spread,
        r#box: knockout,
        scale_fixed: fixed,
        ..Shadow::new(0., 0., 0., Color::rgb(255, 0, 0))
    }
}
fn view(zoom: f32, dpr: f32) -> RasterViewport {
    let b = Bounds::new(Point::ZERO, Size::new(180., 180.));
    RasterViewport {
        world_to_screen: Affine::scale(zoom, zoom),
        device_scale: dpr,
        canvas_bounds: b,
        content_mask: b,
    }
}
fn scene(shadow: Shadow) -> Scene {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("r", display::Rect::new(20., 10.))
            .with_transform(Affine::translation(30., 30.))
            .with_style(Style {
                fill: Some(Color::rgba(0, 0, 255, 128).into()),
                shadow: Some(shadow),
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    s
}
fn source(s: &Scene) -> DomainSource {
    capture_subtree(s, &s.roots()[0], CompositeLimits::default()).unwrap()
}
fn render(s: &Scene, v: RasterViewport) -> CpuFrame {
    rasterize(
        &source(s),
        v,
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}
fn pixel(f: &CpuFrame, x: i32, y: i32, dpr: f32) -> [u8; 4] {
    let x = x - (f.screen_bounds.origin.x * dpr).round() as i32;
    let y = y - (f.screen_bounds.origin.y * dpr).round() as i32;
    if x < 0 || y < 0 || x >= f.width as i32 || y >= f.height as i32 {
        return [0; 4];
    }
    f.premul_rgba[((y as u32 * f.width + x as u32) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}
fn plan(
    shadow: Shadow,
    world: Affine,
    zoom: f32,
    dpr: f32,
    inner: bool,
) -> crate::shadow::SourcePlan {
    let a = crate::blur::PhysicalSpace {
        camera: Affine::scale(zoom, zoom),
        scale: dpr as f64,
        logical_camera: Affine::scale(zoom, zoom),
    };
    crate::shadow::SourcePlan::physical(
        crate::shadow::PreparedShadow {
            effect: shadow,
            world,
            ink_bounds: None,
            reference: Some(Bounds::new(Point::ZERO, Size::new(100., 40.))),
        },
        a,
        [20., 20., 120., 60.],
        CompositeLimits::default(),
        inner,
    )
    .unwrap()
}
#[test]
fn arithmetic_preserves_canvas_axes_cap_and_dpr_provenance() {
    for zoom in [0.5, 1., 2.] {
        for dpr in [1., 2.] {
            for fixed in [false, true] {
                let mut e = effect(10., false, fixed);
                e.x = 7.;
                e.y = -3.;
                e.blur = 4.;
                let p = plan(e, Affine::IDENTITY, zoom, dpr, false);
                let m = p.mode.unwrap();
                let f = if fixed {
                    1. / f64::from(zoom).max(1.)
                } else {
                    1.
                };
                assert_eq!(m.factors, [1. + 20. * f / 100., 1. + 20. * f / 40.]);
                assert_eq!(
                    p.filter.offset,
                    [
                        7. * f64::from(zoom * dpr) * f,
                        -3. * f64::from(zoom * dpr) * f
                    ]
                );
                assert_eq!(p.filter.sigma, 4. * zoom * dpr * f as f32 / 2.);
            }
        }
    }
    let e = effect(10., false, true);
    let p = plan(e, Affine::new(0., 1., -1., 0., 0., 0.), 2., 2., false);
    assert_eq!(p.mode.unwrap().factors, [1.1, 1.25]);
    let p = plan(effect(-30., false, false), Affine::IDENTITY, 1., 1., false);
    assert_eq!(p.mode.unwrap().factors, [0.4, -0.5]);
    for inner in [false, true] {
        let p = plan(
            effect(if inner { 20. } else { -20. }, false, false),
            Affine::IDENTITY,
            1.,
            1.,
            inner,
        );
        assert_eq!(
            p.mode.as_ref().unwrap().kind,
            crate::shadow::SpreadMode::Collapsed
        );
        assert_eq!(p.mode.unwrap().output.is_some(), inner);
    }
}
#[test]
fn knockout_uses_original_fractional_alpha_and_inner_box_is_inert() {
    let s = scene(effect(0., true, false));
    let f = render(&s, view(1., 1.));
    // B=A=128; knockout coverage=64, under blue source alpha128.
    assert_eq!(pixel(&f, 35, 35, 1.), [32, 0, 128, 160]);
    let plain = render(&scene(effect(0., false, false)), view(1., 1.));
    assert_eq!(pixel(&plain, 35, 35, 1.), [64, 0, 128, 192]);
    let mut inner = s.clone();
    inner
        .update_object(&"r".into(), |o| {
            o.style.shadow = None;
            o.style.inner_shadow_layers = vec![effect(0., true, false)]
        })
        .unwrap();
    let a = render(&inner, view(1., 1.));
    inner
        .update_object(&"r".into(), |o| {
            o.style.inner_shadow_layers[0].r#box = false
        })
        .unwrap();
    let b = render(&inner, view(1., 1.));
    assert_eq!(a.premul_rgba, b.premul_rgba);
    assert_eq!(pixel(&a, 35, 35, 1.), [64, 0, 96, 160]);
}
#[test]
fn signed_spread_reflects_asymmetric_ink_and_exact_collapse() {
    let mut s = scene(effect(-15., false, false));
    s.update_object(&"r".into(), |o| {
        o.shape = display::Image::new("holes", Size::new(20., 10.)).into();
        o.style.shadow.as_mut().unwrap().x = 40.;
    })
    .unwrap();
    let mut rgba = vec![0; 20 * 10 * 4];
    for y in 1..4 {
        for x in 2..6 {
            rgba[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4].copy_from_slice(&[0, 0, 128, 128]);
        }
    }
    let image = Arc::new(
        StaticImage::from_premultiplied_rgba(20, 10, rgba, CompositeLimits::default()).unwrap(),
    );
    let mut images = ImageInputs::default();
    images.insert("r".into(), Ok(image));
    let f = rasterize_with_images(
        &source(&s),
        &images,
        view(1., 1.),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    // C=(40,35), k=(-.5,-2); left/top ink reflects right/bottom.
    assert_eq!(pixel(&f, 83, 39, 1.), [128, 0, 0, 128]);
    assert_eq!(pixel(&f, 76, 30, 1.), [0; 4]);
    s.update_object(&"r".into(), |o| {
        o.shape = display::Rect::new(20., 10.).into();
        o.style.shadow = Some(effect(-5., false, false));
    })
    .unwrap();
    let f = render(&s, view(1., 1.));
    assert_eq!(pixel(&f, 35, 35, 1.), [0, 0, 128, 128]);
    s.update_object(&"r".into(), |o| {
        o.style.shadow = None;
        o.style.inner_shadow_layers = vec![effect(5., true, false)];
    })
    .unwrap();
    assert_eq!(
        pixel(&render(&s, view(1., 1.)), 35, 35, 1.),
        [128, 0, 64, 192]
    );
}
#[test]
fn projection_oracle_uses_sampled_alpha_before_spread_and_offset() {
    // Independent bilinear arithmetic over an asymmetric, holed alpha array.
    for spread in [4., -3., -13.] {
        for inner in [false, true] {
            let mut e = effect(spread, true, false);
            e.x = 24.25;
            e.y = -0.5;
            let mut s = scene(e);
            s.update_object(&"r".into(), |o| {
                o.shape = display::Image::new("alpha", Size::new(20., 10.)).into();
                if inner {
                    o.style.shadow = None;
                    o.style.inner_shadow_layers = vec![e];
                }
            })
            .unwrap();
            let mut rgba = vec![0; 20 * 10 * 4];
            for y in 0..10 {
                for x in 0..20 {
                    if !(7..12).contains(&x) || !(3..7).contains(&y) {
                        let a = ((x * 11 + y * 7) % 180 + 40) as u8;
                        rgba[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4].copy_from_slice(&[0, 0, a, a]);
                    }
                }
            }
            let image = Arc::new(
                StaticImage::from_premultiplied_rgba(
                    20,
                    10,
                    rgba.clone(),
                    CompositeLimits::default(),
                )
                .unwrap(),
            );
            let mut images = ImageInputs::default();
            images.insert("r".into(), Ok(image));
            let frame = rasterize_with_images(
                &source(&s),
                &images,
                view(1., 1.),
                &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
                &AtomicBool::new(false),
            )
            .unwrap()
            .unwrap();
            let sample = |x: f64, y: f64| -> u8 {
                let x0 = x.floor() as i32;
                let y0 = y.floor() as i32;
                let fx = x - x.floor();
                let fy = y - y.floor();
                let at = |x: i32, y: i32| {
                    if (0..20).contains(&x) && (0..10).contains(&y) {
                        rgba[((y * 20 + x) * 4 + 3) as usize] as f64
                    } else {
                        0.
                    }
                };
                (at(x0, y0) * (1. - fx) * (1. - fy)
                    + at(x0 + 1, y0) * fx * (1. - fy)
                    + at(x0, y0 + 1) * (1. - fx) * fy
                    + at(x0 + 1, y0 + 1) * fx * fy)
                    .round() as u8
            };
            let sign = if inner { -1. } else { 1. };
            let k = [
                1. + sign * 2. * spread as f64 / 20.,
                1. + sign * 2. * spread as f64 / 10.,
            ];
            let spread_at = |x: i32, y: i32| {
                sample(
                    10. + (x as f64 + 0.5 - 40.) / k[0] - 0.5,
                    5. + (y as f64 + 0.5 - 35.) / k[1] - 0.5,
                )
            };
            for y in 15..65 {
                for x in 10..100 {
                    let sx = x as f64 - e.x as f64;
                    let sy = y as f64 - e.y as f64;
                    let ix = sx.floor() as i32;
                    let iy = sy.floor() as i32;
                    let fx = sx - sx.floor();
                    let fy = sy - sy.floor();
                    let b = (f64::from(spread_at(ix, iy)) * (1. - fx) * (1. - fy)
                        + f64::from(spread_at(ix + 1, iy)) * fx * (1. - fy)
                        + f64::from(spread_at(ix, iy + 1)) * (1. - fx) * fy
                        + f64::from(spread_at(ix + 1, iy + 1)) * fx * fy)
                        .round() as u32;
                    let a = sample((x - 30) as f64, (y - 30) as f64) as u32;
                    let c = if inner {
                        (a * (255 - b) + 127) / 255
                    } else {
                        (b * (255 - a) + 127) / 255
                    };
                    let expected = if inner {
                        [
                            c as u8,
                            0,
                            ((a * (255 - c) + 127) / 255) as u8,
                            (c + (a * (255 - c) + 127) / 255) as u8,
                        ]
                    } else {
                        [
                            ((c * (255 - a) + 127) / 255) as u8,
                            0,
                            a as u8,
                            (a + (c * (255 - a) + 127) / 255) as u8,
                        ]
                    };
                    let got = pixel(&frame, x, y, 1.);
                    for channel in 0..4 {
                        assert!(
                            (i32::from(got[channel]) - i32::from(expected[channel])).abs() <= 1,
                            "spread={spread} inner={inner} at{x},{y}: {got:?} != {expected:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn fixed_effect_zoom_dpr_nested_rotation_matches_export_provenance() {
    for zoom in [0.5, 1., 2.] {
        for dpr in [1., 2.] {
            let mut e = effect(3., false, true);
            e.x = 15.;
            e.y = 2.;
            e.blur = 0.004;
            let mut s = Scene::new();
            s.insert(
                DisplayObject::new("g", display::Group::new())
                    .with_transform(Affine::translation(35., 25.) * Affine::scale(1.25, 1.25))
                    .with_style(Style {
                        blend_mode: BlendMode::Normal,
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
            s.insert(
                DisplayObject::new("r", display::Rect::new(12., 6.))
                    .with_transform(
                        Affine::translation(10., 10.) * Affine::new(0., 1., -1., 0., 0., 0.),
                    )
                    .with_style(Style {
                        fill: Some(Color::rgb(0, 0, 255).into()),
                        shadow: Some(e),
                        ..Default::default()
                    }),
                Some("g".into()),
                None,
            )
            .unwrap();
            let export = SceneExportPlan::new(
                &s,
                SceneExportRequest {
                    world_bounds: Bounds::new(Point::ZERO, Size::new(90., 90.)),
                    scale: zoom,
                    pixel_ratio: dpr,
                    background: Color::TRANSPARENT,
                    format: SceneExportFormat::Png,
                },
                CompositeLimits::default(),
            )
            .unwrap();
            let logical_bounds = Bounds::new(
                Point::ZERO,
                Size::new(
                    (90. * zoom * dpr).ceil() / dpr,
                    (90. * zoom * dpr).ceil() / dpr,
                ),
            );
            let direct = RasterViewport {
                world_to_screen: Affine::scale(zoom, zoom),
                device_scale: dpr,
                canvas_bounds: logical_bounds,
                content_mask: logical_bounds,
            };
            let mut direct_source = export.source().clone();
            direct_source.logical_camera = None;
            direct_source.logical_effect_camera = None;
            let a = rasterize(
                &direct_source,
                direct,
                &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
                &AtomicBool::new(false),
            )
            .unwrap()
            .unwrap();
            let b = rasterize(
                export.source(),
                export.viewport(),
                &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
                &AtomicBool::new(false),
            )
            .unwrap()
            .unwrap();
            assert_eq!((a.width, a.height), (b.width, b.height));
            assert_eq!(a.premul_rgba, b.premul_rgba, "zoom{zoom} dpr{dpr}");
            let png = export
                .encode_with_inputs(
                    &ImageInputs::default(),
                    &TextInputs::default(),
                    SceneExportPreparation::default(),
                    &AtomicBool::new(false),
                )
                .unwrap();
            assert_eq!((png.width, png.height), (a.width, a.height));
        }
    }
}
#[test]
fn original_reference_stays_fixed_when_child_halo_enters_paintless_frame() {
    for late in [false, true] {
        let mut s = Scene::new();
        let order = if late {
            ContainerPaintOrder::StrokeAfterChildren
        } else {
            ContainerPaintOrder::BeforeChildren
        };
        let mut e = effect(5., false, false);
        e.x = 35.;
        s.insert(
            DisplayObject::new("f", display::Frame::new(20., 20.).with_paint_order(order))
                .with_transform(Affine::translation(20., 20.))
                .with_style(Style {
                    fill: None,
                    shadow: Some(e),
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
        s.insert(
            DisplayObject::new("r", display::Rect::new(4., 4.))
                .with_transform(Affine::translation(-5., 8.))
                .with_style(Style {
                    fill: Some(Color::rgb(0, 0, 255).into()),
                    blur: 2.,
                    ..Default::default()
                }),
            Some("f".into()),
            None,
        )
        .unwrap();
        let src = source(&s);
        let mut prep = crate::vector::Preparation::default();
        let mut n = prep
            .node(
                &src.root,
                CompositeLimits::default(),
                &ImageInputs::default(),
                &TextInputs::default(),
            )
            .unwrap();
        assert!(n.shadows[0].ink_bounds.is_none());
        crate::blur::resolve(
            &mut n,
            crate::blur::PhysicalSpace {
                camera: Affine::IDENTITY,
                logical_camera: Affine::IDENTITY,
                scale: 1.,
            },
            CompositeLimits::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let mode = n.device.as_ref().unwrap().shadows[0].mode.as_ref().unwrap();
        assert_eq!(mode.center, [30., 30.]);
        assert_eq!(mode.factors, [1.5, 1.5]);
        let f = render(&s, view(1., 1.));
        let mut red = 0;
        for y in 10..50 {
            for x in 45..80 {
                red += u64::from(pixel(&f, x, y, 1.)[0]);
            }
        }
        assert!(red > 0, "clipped descendant halo must project");
    }
}
#[test]
fn full_offscreen_source_and_large_cancelling_translation_match() {
    let mut e = effect(5., true, true);
    e.x = 60.;
    e.blur = 0.002;
    let mut a = scene(e);
    a.update_object(&"r".into(), |o| {
        o.transform = Affine::translation(-25., 30.)
    })
    .unwrap();
    let direct = render(&a, view(1., 2.));
    assert!(pixel(&direct, 80, 65, 2.)[0] > 0);
    let mut b = a.clone();
    b.update_object(&"r".into(), |o| o.transform.tx += 100_000_000.)
        .unwrap();
    let mut v = view(1., 2.);
    v.world_to_screen.tx = -100_000_000.;
    // Use a multiple-of-eight original translation so f32 authored placement
    // itself is representable at this magnitude.
    a.update_object(&"r".into(), |o| o.transform.tx = -24.)
        .unwrap();
    b.update_object(&"r".into(), |o| o.transform.tx = 100_000_000. - 24.)
        .unwrap();
    assert_eq!(
        render(&a, view(1., 2.)).premul_rgba,
        render(&b, v).premul_rgba
    );
}
#[test]
fn mode_admission_exact_peak_work_late_failure_and_retained_atomicity() {
    for inner in [false, true] {
        for spread in [3., -13., -5.] {
            let mut s = scene(effect(spread, true, true));
            s.update_object(&"r".into(), |o| {
                if inner {
                    o.style.inner_shadow_layers = vec![o.style.shadow.take().unwrap()];
                } else {
                    o.style.shadow.as_mut().unwrap().blur = 2.;
                }
            })
            .unwrap();
            let src = source(&s);
            let mut measured = PixelBudget::new(CompositeLimits::default(), 79).unwrap();
            let frame = rasterize(&src, view(1., 1.), &mut measured, &AtomicBool::new(false))
                .unwrap()
                .unwrap();
            let peak = measured.peak_bytes();
            let work = measured.surface_writes();
            for (bytes, writes, ok) in [
                (peak, work, true),
                (peak - 1, work, false),
                (peak, work - 1, false),
            ] {
                let limits = CompositeLimits {
                    max_surface_bytes: bytes,
                    max_surface_writes: writes,
                    ..Default::default()
                };
                let mut budget = PixelBudget::new(limits, 79).unwrap();
                let r = rasterize(&src, view(1., 1.), &mut budget, &AtomicBool::new(false));
                if ok {
                    assert_eq!(r.unwrap().unwrap().premul_rgba, frame.premul_rgba)
                } else {
                    assert!(matches!(
                        r,
                        Err(CompositeError::WorkLimit | CompositeError::WorkingSetLimit)
                    ));
                    assert_eq!(budget.retained_bytes(), 79);
                    assert_eq!(budget.surface_writes(), 0);
                }
            }
        }
    }
    let mut s = scene(effect(2., false, false));
    s.update_object(&"r".into(), |o| {
        o.style.shadow_layers.push(effect(4096., false, false))
    })
    .unwrap();
    let mut budget = PixelBudget::new(CompositeLimits::default(), 37).unwrap();
    assert!(matches!(
        rasterize(
            &source(&s),
            view(1., 1.),
            &mut budget,
            &AtomicBool::new(false)
        ),
        Err(CompositeError::TooLarge)
    ));
    assert_eq!(budget.retained_bytes(), 37);
    assert_eq!(budget.surface_writes(), 0);
    let mut budget = PixelBudget::new(CompositeLimits::default(), 37).unwrap();
    assert!(matches!(
        rasterize(
            &source(&s),
            view(1., 1.),
            &mut budget,
            &AtomicBool::new(true)
        ),
        Err(CompositeError::Superseded)
    ));
    assert_eq!(budget.retained_bytes(), 37);
}

#[test]
fn each_outer_inner_layer_uses_independent_original_alpha() {
    let mut s = scene(effect(2., true, false));
    s.update_object(&"r".into(), |o| {
        let mut green = effect(2., true, false);
        green.color = Color::rgb(0, 255, 0);
        o.style.shadow_layers = vec![green];
        let mut inner = effect(-2., true, false);
        inner.color = Color::rgb(255, 0, 0);
        o.style.inner_shadow_layers = vec![inner];
    })
    .unwrap();
    // Outer layers each have 64 coverage; final source and inner still use A128.
    let compose = |s: [u32; 4], d: [u32; 4]| -> [u32; 4] {
        std::array::from_fn(|i| s[i] + (d[i] * (255 - s[3]) + 127) / 255)
    };
    let expected = compose(
        [64, 0, 0, 64],
        compose([0, 0, 128, 128], compose([0, 64, 0, 64], [64, 0, 0, 64])),
    );
    assert_eq!(
        pixel(&render(&s, view(1., 1.)), 35, 35, 1.),
        expected.map(|v| v as u8)
    );
    let before = source(&s);
    s.update_object(&"r".into(), |o| o.style.shadow_layers[0].r#box = false)
        .unwrap();
    let after = source(&s);
    assert_ne!(before, after);
    assert!(!before.placement_eq(&after));
}
#[test]
fn collapsed_inner_obeys_both_fixed_container_phases() {
    for frame in [false, true] {
        for late in [false, true] {
            let mut s = Scene::new();
            let order = if late {
                ContainerPaintOrder::StrokeAfterChildren
            } else {
                ContainerPaintOrder::BeforeChildren
            };
            let shape: Shape = if frame {
                display::Frame::new(20., 10.).with_paint_order(order).into()
            } else {
                display::Box::new(20., 10.).with_paint_order(order).into()
            };
            s.insert(
                DisplayObject::new("c", shape)
                    .with_transform(Affine::translation(30., 30.))
                    .with_style(Style {
                        fill: None,
                        inner_shadow_layers: vec![effect(5., true, false)],
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
            s.insert(
                DisplayObject::new("r", display::Rect::new(20., 10.))
                    .with_style(Style::fill(Color::rgba(0, 0, 255, 128))),
                Some("c".into()),
                None,
            )
            .unwrap();
            assert_eq!(
                pixel(&render(&s, view(1., 1.)), 35, 35, 1.),
                if late {
                    [128, 0, 64, 192]
                } else {
                    [64, 0, 128, 192]
                }
            );
        }
    }
}
#[test]
fn mode_sources_include_mask_results_and_pixel_path_erasures() {
    for eraser in [EraserMode::Pixel, EraserMode::Path] {
        let mut s = Scene::new();
        let mut shadow = effect(0., true, true);
        shadow.x = 35.;
        s.insert(
            DisplayObject::new("g", display::Group::new())
                .with_transform(Affine::translation(20., 20.))
                .with_style(Style {
                    shadow: Some(shadow),
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
        s.insert(
            DisplayObject::new("mask", display::Rect::new(12., 12.))
                .with_mask(MaskMode::Alpha)
                .with_style(Style::fill(Color::rgba(0, 0, 0, 128))),
            Some("g".into()),
            None,
        )
        .unwrap();
        s.insert(
            DisplayObject::new("r", display::Rect::new(20., 12.))
                .with_style(Style::fill(Color::rgb(0, 0, 255))),
            Some("g".into()),
            None,
        )
        .unwrap();
        s.insert(
            DisplayObject::new("erase", display::Rect::new(4., 4.))
                .with_transform(Affine::translation(4., 4.))
                .with_eraser(eraser)
                .with_style(Style::fill(Color::BLACK)),
            Some("g".into()),
            None,
        )
        .unwrap();
        let f = render(&s, view(1., 1.));
        assert_eq!(pixel(&f, 57, 22, 1.), [128, 0, 0, 128]);
        assert_eq!(
            pixel(&f, 60, 25, 1.),
            if eraser == EraserMode::Pixel {
                [128, 0, 0, 128]
            } else {
                [0; 4]
            }
        );
        s.update_object(&"mask".into(), |o| o.style.opacity = 0.5)
            .unwrap();
        let translucent = render(&s, view(1., 1.));
        assert_eq!(pixel(&translucent, 57, 22, 1.), [64, 0, 0, 64]);
        assert_eq!(pixel(&translucent, 60, 25, 1.), [0; 4]);
        assert_eq!(pixel(&f, 70, 25, 1.), [0; 4]);
    }
}
#[test]
fn nonempty_zero_reference_rejects_before_pixels_and_empty_source_stays_empty() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("g", display::Group::new()).with_style(Style {
            shadow: Some(effect(1., false, false)),
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    assert!(
        rasterize(
            &source(&s),
            view(1., 1.),
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false)
        )
        .unwrap()
        .is_none()
    );
    // Public fixed boxes already reject zero dimensions in Scene validation.
    // The no-pixel source planner independently rejects every invalid reference.
    for reference in [
        None,
        Some(Bounds::new(Point::ZERO, Size::new(0., 10.))),
        Some(Bounds::new(Point::ZERO, Size::new(10., 0.))),
    ] {
        let result = crate::shadow::SourcePlan::physical(
            crate::shadow::PreparedShadow {
                effect: effect(1., false, false),
                world: Affine::IDENTITY,
                ink_bounds: None,
                reference,
            },
            crate::blur::PhysicalSpace {
                camera: Affine::IDENTITY,
                logical_camera: Affine::IDENTITY,
                scale: 1.,
            },
            [0., 0., 10., 10.],
            CompositeLimits::default(),
            false,
        );
        assert!(matches!(
            result,
            Err(CompositeError::InvalidShadowReference)
        ));
    }
}

#[test]
fn far_separated_original_and_projection_do_not_allocate_their_union() {
    let mut e = effect(2., true, false);
    e.x = 4096.;
    let mut s = scene(e);
    s.update_object(&"r".into(), |o| o.transform.tx = -4080.)
        .unwrap();
    assert_eq!(
        pixel(&render(&s, view(1., 1.)), 25, 35, 1.),
        [128, 0, 0, 128]
    );
}
#[test]
fn mode_plans_respect_exact_dimensions_pixels_and_depth() {
    let p = plan(effect(2., true, false), Affine::IDENTITY, 1., 1., false);
    let m = p.mode.as_ref().unwrap();
    let surfaces = m.transformed.iter().chain(m.output.iter());
    let max_dimension = surfaces
        .clone()
        .fold(p.width.max(p.height), |n, s| n.max(s.width).max(s.height));
    let max_pixels = surfaces.fold(p.pixels, |n, s| n.max(s.pixels));
    let build = |limits| {
        crate::shadow::SourcePlan::physical(
            crate::shadow::PreparedShadow {
                effect: effect(2., true, false),
                world: Affine::IDENTITY,
                ink_bounds: None,
                reference: Some(Bounds::new(Point::ZERO, Size::new(100., 40.))),
            },
            crate::blur::PhysicalSpace {
                camera: Affine::IDENTITY,
                logical_camera: Affine::IDENTITY,
                scale: 1.,
            },
            [20., 20., 120., 60.],
            limits,
            false,
        )
    };
    assert!(
        build(CompositeLimits {
            max_dimension,
            max_pixels,
            ..Default::default()
        })
        .is_ok()
    );
    assert!(matches!(
        build(CompositeLimits {
            max_dimension: max_dimension - 1,
            ..Default::default()
        }),
        Err(CompositeError::TooLarge)
    ));
    assert!(matches!(
        build(CompositeLimits {
            max_pixels: max_pixels - 1,
            ..Default::default()
        }),
        Err(CompositeError::TooLarge)
    ));
    for depth in [15, 16, 17] {
        let mut s = Scene::new();
        for i in 0..depth {
            s.insert(
                DisplayObject::new(format!("g{i}"), display::Group::new()).with_style(Style {
                    shadow: Some(effect(-5., false, false)),
                    ..Default::default()
                }),
                if i == 0 {
                    None
                } else {
                    Some(format!("g{}", i - 1).into())
                },
                None,
            )
            .unwrap();
        }
        s.insert(
            DisplayObject::new("r", display::Rect::new(10., 10.))
                .with_style(Style::fill(Color::BLACK)),
            Some(format!("g{}", depth - 1).into()),
            None,
        )
        .unwrap();
        let r = capture_subtree(&s, &"g0".into(), CompositeLimits::default()).and_then(|src| {
            rasterize(
                &src,
                view(1., 1.),
                &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
                &AtomicBool::new(false),
            )
        });
        if depth <= 16 {
            assert!(r.is_ok(), "depth{depth}: {r:?}");
        } else {
            assert!(matches!(r, Err(CompositeError::NestingLimit)));
        }
    }
}
#[test]
fn fixed_stroke_reference_is_resolved_before_spread_and_dpr_does_not_change_it() {
    let mut s = scene(effect(3., false, true));
    s.update_object(&"r".into(), |o| {
        o.style.stroke =
            Some(Stroke::new(Color::BLACK, 4.).with_scale_fixed(StrokeScaleFixed::Fixed))
    })
    .unwrap();
    let src = match scene_units(&s, CompositeLimits::default())
        .unwrap()
        .remove(0)
    {
        SceneUnit::Domain(s) => *s,
        other => panic!("{other:?}"),
    };
    for zoom in [0.5, 1., 2.] {
        for dpr in [1_f32, 2.] {
            let mut prep = crate::vector::Preparation::default();
            prep.logical_camera = Some(zoom);
            let mut node = prep
                .node(
                    &src.root,
                    CompositeLimits::default(),
                    &ImageInputs::default(),
                    &TextInputs::default(),
                )
                .unwrap();
            let expected = s
                .shadow_reference_bounds_for_view(&"r".into(), zoom)
                .unwrap();
            assert_eq!(node.shadows[0].reference, expected);
            crate::blur::resolve(
                &mut node,
                crate::blur::PhysicalSpace {
                    camera: Affine::scale(zoom, zoom),
                    logical_camera: Affine::scale(zoom, zoom),
                    scale: dpr as f64,
                },
                CompositeLimits::default(),
                &AtomicBool::new(false),
            )
            .unwrap();
            let reference = expected.unwrap();
            let f = 1. / f64::from(zoom).max(1.);
            assert_eq!(
                node.device.unwrap().shadows[0]
                    .mode
                    .as_ref()
                    .unwrap()
                    .factors,
                [
                    1. + 6. * f / reference.size.width as f64,
                    1. + 6. * f / reference.size.height as f64
                ]
            );
        }
    }
}

#[test]
fn group_reference_retains_hidden_nested_geometry_and_box_overflow_without_resources() {
    for hidden_by_opacity in [false, true] {
        let mut s = Scene::new();
        s.insert(
            DisplayObject::new("g", display::Group::new()).with_style(Style {
                shadow: Some(effect(5., false, false)),
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
        s.insert(
            DisplayObject::new("box", display::Box::new(10., 10.)).with_style(Style {
                fill: None,
                ..Default::default()
            }),
            Some("g".into()),
            None,
        )
        .unwrap();
        s.insert(
            DisplayObject::new("ink", display::Rect::new(5., 5.))
                .with_transform(Affine::translation(40., 2.))
                .with_style(Style::fill(Color::rgb(0, 0, 255))),
            Some("box".into()),
            None,
        )
        .unwrap();
        let mut hidden = DisplayObject::new("hidden", display::Group::new())
            .with_transform(Affine::translation(80., 0.));
        if hidden_by_opacity {
            hidden.style.opacity = 0.
        } else {
            hidden.visible = false
        }
        s.insert(hidden, Some("g".into()), None).unwrap();
        s.insert(
            DisplayObject::new(
                "missing",
                display::Image::new("never-loaded", Size::new(20., 20.)),
            ),
            Some("hidden".into()),
            None,
        )
        .unwrap();
        let src = source(&s);
        assert!(src.images().is_empty());
        let mut prep = crate::vector::Preparation::default();
        let n = prep
            .node(
                &src.root,
                CompositeLimits::default(),
                &ImageInputs::default(),
                &TextInputs::default(),
            )
            .unwrap();
        assert_eq!(
            n.shadows[0].reference,
            s.shadow_reference_bounds(&"g".into()).unwrap()
        );
        assert_eq!(
            n.shadows[0].reference.unwrap(),
            Bounds::new(Point::ZERO, Size::new(100., 20.))
        );
        assert!(render(&s, view(1., 1.)).premul_rgba.iter().any(|&b| b != 0));
        let old = src;
        s.update_object(&"missing".into(), |o| o.transform.tx = 10.)
            .unwrap();
        let new = source(&s);
        assert_ne!(old, new);
        assert!(!old.placement_eq(&new));
    }
}

#[test]
fn gaussian_filters_the_resampled_a8_before_offset_independent_convolution_oracle() {
    let mut e = effect(4., false, false);
    e.x = 40.;
    e.blur = 2.;
    let mut s = scene(e);
    s.update_object(&"r".into(), |o| {
        o.shape = display::Image::new("alpha", Size::new(20., 10.)).into()
    })
    .unwrap();
    let mut rgba = vec![0; 800];
    for y in 0..10 {
        for x in 0..20 {
            if (x + y) % 4 != 0 {
                let a = (40 + x * 7 + y * 3) as u8;
                rgba[(y * 20 + x) * 4..][..4].copy_from_slice(&[0, 0, a, a]);
            }
        }
    }
    let image = Arc::new(
        StaticImage::from_premultiplied_rgba(20, 10, rgba.clone(), CompositeLimits::default())
            .unwrap(),
    );
    let mut images = ImageInputs::default();
    images.insert("r".into(), Ok(image));
    let f = rasterize_with_images(
        &source(&s),
        &images,
        view(1., 1.),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    let spread = |qx: i32, qy: i32| -> f64 {
        let x = 10. + (f64::from(qx) + 0.5 - 40.) / 1.4 - 0.5;
        let y = 5. + (f64::from(qy) + 0.5 - 35.) / 1.8 - 0.5;
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        let fx = x - x.floor();
        let fy = y - y.floor();
        let at = |x: i32, y: i32| {
            if (0..20).contains(&x) && (0..10).contains(&y) {
                f64::from(rgba[((y * 20 + x) * 4 + 3) as usize])
            } else {
                0.
            }
        };
        (at(ix, iy) * (1. - fx) * (1. - fy)
            + at(ix + 1, iy) * fx * (1. - fy)
            + at(ix, iy + 1) * (1. - fx) * fy
            + at(ix + 1, iy + 1) * fx * fy)
            .round()
    };
    // Locked public sigma-to-kernel mapping gives width5 at sigma1. This
    // scalar double-precision oracle does not call production/image filtering.
    let mut kernel: Vec<f64> = (-2..=2).map(|x| (-f64::from(x * x) / 2.).exp()).collect();
    let sum: f64 = kernel.iter().sum();
    for w in &mut kernel {
        *w /= sum;
    }
    for y in 15..55 {
        for x in 55..105 {
            let mut expected = 0.;
            for dy in -2..=2 {
                for dx in -2..=2 {
                    expected += spread(x - 40 + dx, y + dy)
                        * kernel[(dx + 2) as usize]
                        * kernel[(dy + 2) as usize];
                }
            }
            let got = pixel(&f, x, y, 1.);
            assert!(
                (f64::from(got[0]) - expected.round()).abs() <= 1.,
                "at{x},{y}: {} vs {expected}",
                got[0]
            );
            assert_eq!(got[0], got[3]);
        }
    }
}

fn retained_reference_scene(nested: bool) -> Scene {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("g", display::Group::new()).with_style(Style {
            shadow: Some(effect(1., false, false)),
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("ink", display::Rect::new(8., 8.))
            .with_transform(Affine::translation(20., 20.)),
        Some("g".into()),
        None,
    )
    .unwrap();
    let parent = if nested {
        let mut g = DisplayObject::new("hidden", display::Group::new());
        g.visible = false;
        s.insert(g, Some("g".into()), None).unwrap();
        "hidden"
    } else {
        "g"
    };
    let mut p = DisplayObject::new(
        "polygon",
        display::Polygon::new(vec![
            Point::new(10., 10.),
            Point::new(40., 10.),
            Point::new(10., 40.),
        ]),
    );
    p.visible = false;
    s.insert(p, Some(parent.into()), None).unwrap();
    s
}
#[test]
fn review_hidden_reference_root_points_are_counted_at_capture() {
    let s = retained_reference_scene(false);
    let exact = CompositeLimits {
        max_points: 3,
        max_total_points: 3,
        ..Default::default()
    };
    assert!(capture_subtree(&s, &"g".into(), exact).is_ok());
    for limits in [
        CompositeLimits {
            max_points: 2,
            ..exact
        },
        CompositeLimits {
            max_total_points: 2,
            ..exact
        },
    ] {
        assert!(matches!(
            capture_subtree(&s, &"g".into(), limits),
            Err(CompositeError::InputLimit)
        ));
    }
}
#[test]
fn review_retained_reference_geometry_revalidates_tighter_raster_limits() {
    for nested in [false, true] {
        let s = retained_reference_scene(nested);
        let src = source(&s);
        let exact = CompositeLimits {
            max_nodes: if nested { 4 } else { 3 },
            max_depth: if nested { 2 } else { 1 },
            max_points: 3,
            max_total_points: 3,
            ..Default::default()
        };
        assert!(
            rasterize(
                &src,
                view(1., 1.),
                &mut PixelBudget::new(exact, 31).unwrap(),
                &AtomicBool::new(false)
            )
            .is_ok()
        );
        for limits in [
            CompositeLimits {
                max_nodes: exact.max_nodes - 1,
                ..exact
            },
            CompositeLimits {
                max_depth: exact.max_depth - 1,
                ..exact
            },
            CompositeLimits {
                max_points: 2,
                ..exact
            },
            CompositeLimits {
                max_total_points: 2,
                ..exact
            },
        ] {
            let mut budget = PixelBudget::new(limits, 31).unwrap();
            assert!(matches!(
                rasterize(&src, view(1., 1.), &mut budget, &AtomicBool::new(false)),
                Err(CompositeError::InputLimit | CompositeError::NestingLimit)
            ));
            assert_eq!(budget.retained_bytes(), 31);
            assert_eq!(budget.surface_writes(), 0);
        }
    }
}
#[test]
fn review_positive_sigma_cannot_underflow_to_true_bypass() {
    for active in [false, true] {
        let mut e = effect(0., active, active);
        e.blur = f32::from_bits(1);
        let s = scene(e);
        let mut budget = PixelBudget::new(CompositeLimits::default(), 31).unwrap();
        assert!(matches!(
            rasterize(
                &source(&s),
                view(1., 1.),
                &mut budget,
                &AtomicBool::new(false)
            ),
            Err(CompositeError::TooLarge)
        ));
        assert_eq!(budget.retained_bytes(), 31);
        assert_eq!(budget.surface_writes(), 0);
    }
}
#[test]
fn review_default_physical_parameters_preserve_legacy_matrix_narrowing() {
    let e = Shadow::new(7.3, -2.9, 4.1, Color::BLACK);
    let world = Affine::rotation(17.) * Affine::scale(1.13, 1.13);
    let a = crate::blur::PhysicalSpace {
        camera: Affine::rotation(11.) * Affine::scale(1.27, 1.27),
        logical_camera: Affine::IDENTITY,
        scale: 1.25,
    };
    let prepared = crate::shadow::PreparedShadow {
        effect: e,
        world,
        ink_bounds: None,
        reference: None,
    };
    let old = crate::shadow::FilterPlan::matrix(prepared, a.local(world, [0., 0.])).unwrap();
    let current = crate::shadow::SourcePlan::physical(
        prepared,
        a,
        [20., 20., 120., 60.],
        CompositeLimits::default(),
        false,
    )
    .unwrap();
    assert!(current.mode.is_none());
    assert_eq!(current.filter.offset, old.offset);
    assert_eq!(current.filter.sigma, old.sigma);
}

#[test]
fn review_multiple_hidden_siblings_share_cumulative_reference_point_budget() {
    let mut s = retained_reference_scene(false);
    let mut sibling = DisplayObject::new(
        "sibling",
        display::Polygon::new(vec![
            Point::new(12., 12.),
            Point::new(42., 12.),
            Point::new(12., 42.),
        ]),
    );
    sibling.visible = false;
    s.insert(sibling, Some("g".into()), None).unwrap();
    let exact = CompositeLimits {
        max_points: 3,
        max_total_points: 6,
        ..Default::default()
    };
    let tight = CompositeLimits {
        max_total_points: 5,
        ..exact
    };
    assert!(capture_subtree(&s, &"g".into(), exact).is_ok());
    assert!(matches!(
        capture_subtree(&s, &"g".into(), tight),
        Err(CompositeError::InputLimit)
    ));
    let src = source(&s);
    assert!(
        rasterize(
            &src,
            view(1., 1.),
            &mut PixelBudget::new(exact, 31).unwrap(),
            &AtomicBool::new(false)
        )
        .is_ok()
    );
    let mut budget = PixelBudget::new(tight, 31).unwrap();
    assert!(matches!(
        rasterize(&src, view(1., 1.), &mut budget, &AtomicBool::new(false)),
        Err(CompositeError::InputLimit)
    ));
    assert_eq!(budget.retained_bytes(), 31);
    assert_eq!(budget.surface_writes(), 0);
}

use super::*;
use gpui_flow_scene::{display, *};
use std::sync::{Arc, atomic::AtomicBool};

fn view(x: f32, y: f32, w: f32, h: f32, dpr: f32) -> RasterViewport {
    let b = Bounds::new(Point::new(x, y), Size::new(w, h));
    RasterViewport {
        world_to_screen: Affine::IDENTITY,
        canvas_bounds: b,
        content_mask: b,
        device_scale: dpr,
    }
}
fn rect(id: &str, x: f32, y: f32, w: f32, h: f32, color: Color, blur: f32) -> DisplayObject {
    DisplayObject::new(id, display::Rect::new(w, h))
        .with_transform(Affine::translation(x, y))
        .with_style(Style {
            fill: Some(color.into()),
            blur,
            ..Default::default()
        })
}
fn insert(s: &mut Scene, o: DisplayObject, parent: Option<&str>) {
    s.insert(o, parent.map(Into::into), None).unwrap();
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
fn group(blur: f32) -> Scene {
    let mut s = Scene::new();
    insert(
        &mut s,
        DisplayObject::new("g", display::Group::new()).with_style(Style {
            blur,
            ..Default::default()
        }),
        None,
    );
    s
}
// Independent scalar f64 separable convolution, transparent exterior. Kernel
// extent is the public locked Gaussian mapping, no production/filter helpers.
fn reference(input: &[u8], w: usize, h: usize, sigma: f64) -> Vec<u8> {
    let n = (((((sigma - 0.8) / 0.3) + 1.) * 2.) + 1.).max(3.) as usize | 1;
    let r = n / 2;
    let mut kernel: Vec<_> = (0..n)
        .map(|i| (-((i as f64 - r as f64).powi(2)) / (2. * sigma * sigma)).exp())
        .collect();
    let sum: f64 = kernel.iter().sum();
    for v in &mut kernel {
        *v /= sum;
    }
    let mut horizontal = vec![0.; input.len()];
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                for (i, &k) in kernel.iter().enumerate() {
                    let sx = x as isize + i as isize - r as isize;
                    if sx >= 0 && sx < w as isize {
                        horizontal[(y * w + x) * 4 + c] +=
                            f64::from(input[(y * w + sx as usize) * 4 + c]) * k;
                    }
                }
            }
        }
    }
    let mut output = vec![0; input.len()];
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let mut sum = 0.;
                for (i, &k) in kernel.iter().enumerate() {
                    let sy = y as isize + i as isize - r as isize;
                    if sy >= 0 && sy < h as isize {
                        sum += horizontal[(sy as usize * w + x) * 4 + c] * k;
                    }
                }
                output[(y * w + x) * 4 + c] = sum.round() as u8;
            }
        }
    }
    output
}
#[test]
fn blur_rgba_matches_independent_convolution_ring_and_full_plane() {
    for sigma in [0.001, 0.5, 1., 9.] {
        let mut s = group(sigma);
        insert(
            &mut s,
            rect("r", 42., 42., 8., 8., Color::rgba(255, 0, 0, 128), 0.),
            Some("g"),
        );
        insert(
            &mut s,
            rect("b", 46., 42., 4., 8., Color::rgba(0, 0, 255, 128), 0.),
            Some("g"),
        );
        let f = render(&s, view(-10., -10., 120., 120., 1.));
        let mut original = vec![0; f.premul_rgba.len()];
        let x0 = f.screen_bounds.origin.x as i32;
        let y0 = f.screen_bounds.origin.y as i32;
        for y in 42..50 {
            for x in 42..50 {
                let i = (((y - y0) as u32 * f.width + (x - x0) as u32) * 4) as usize;
                original[i..i + 4].copy_from_slice(if x < 46 {
                    &[128, 0, 0, 128]
                } else {
                    &[64, 0, 128, 192]
                });
            }
        }

        let expected = reference(&original, f.width as usize, f.height as usize, sigma as f64);
        assert!(
            f.premul_rgba
                .iter()
                .zip(&expected)
                .all(|(a, b)| a.abs_diff(*b) <= 1),
            "sigma {sigma}, first mismatch {:?}",
            f.premul_rgba
                .iter()
                .zip(&expected)
                .enumerate()
                .find(|(_, (a, b))| a.abs_diff(**b) > 1)
        );
        assert!(
            f.premul_rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[..3].iter().all(|c| *c <= p[3]))
        );
        if sigma >= 0.5 {
            assert!(pixel(&f, 41, 45, 1.)[3] > 0);
        }
    }
}
#[test]
fn blur_sigma_mapping_rotation_dpr_and_boundaries() {
    for (matrix, scale) in [
        (Affine::IDENTITY, 1.),
        (Affine::rotation(0.7) * Affine::scale(2., 2.), 2.),
    ] {
        let (sigma, _, _) = crate::blur::BlurPlan::parameters(1., matrix).unwrap();
        assert_eq!(sigma, scale);
    }
    for sigma in [0.001, 64.] {
        assert!(crate::blur::BlurPlan::parameters(sigma, Affine::IDENTITY).is_ok());
    }
    for sigma in [0., 0.00099, 64.01] {
        assert!(crate::blur::BlurPlan::parameters(sigma, Affine::IDENTITY).is_err());
    }
    for matrix in [
        Affine::scale(2., 1.),
        Affine::scale(-1., 1.),
        Affine::scale(0., 0.),
        Affine::new(1., 0., 0.2, 1., 0., 0.),
    ] {
        assert_eq!(
            crate::blur::BlurPlan::parameters(1., matrix).unwrap_err(),
            CompositeError::UnsupportedTransform
        );
    }
    let mut s = Scene::new();
    insert(&mut s, rect("r", 20., 20., 1., 1., Color::WHITE, 1.), None);
    let f = render(&s, view(0., 0., 50., 50., 1.));
    // sigma=1 center is ~42, sigma=0.5 center is ~158: discriminates the
    // foreground mapping from the existing shadow half-radius mapping.
    assert!((35..=50).contains(&pixel(&f, 20, 20, 1.)[3]));
}
#[test]
fn blur_zero_and_signed_zero_retain_exact_legacy_bytes_extent_and_work() {
    let mut s = group(0.);
    insert(
        &mut s,
        rect(
            "r",
            10.25,
            11.5,
            12.,
            10.,
            Color::rgba(220, 40, 70, 130),
            0.,
        ),
        Some("g"),
    );
    s.update_object(&"g".into(), |o| {
        o.style.shadow = Some(Shadow::new(3.25, 2.5, 1., Color::BLACK))
    })
    .unwrap();
    let mut outputs = Vec::new();
    for zero in [0., -0.] {
        s.update_object(&"g".into(), |o| o.style.blur = zero)
            .unwrap();
        let mut b = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
        let f = rasterize(
            &source(&s),
            view(0., 0., 50., 50., 1.25),
            &mut b,
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        outputs.push((
            f.screen_bounds,
            f.premul_rgba,
            b.peak_bytes(),
            b.surface_writes(),
        ));
    }
    assert_eq!(outputs[0], outputs[1]);
}
#[test]
fn blur_offscreen_ink_and_nested_shadow_crop_are_lattice_invariant() {
    for dpr in [1., 1.25, 2.] {
        let mut s = group(0.5);
        s.update_object(&"g".into(), |o| {
            o.style.shadow = Some(Shadow::new(3.25, -2.5, 0.5, Color::rgb(0, 0, 255)))
        })
        .unwrap();
        let mut c = rect("r", -5.25, 10.5, 6., 8., Color::rgba(255, 0, 0, 180), 1.);
        c.style
            .inner_shadow_layers
            .push(Shadow::new(1., 0., 0.5, Color::rgb(0, 255, 0)));
        insert(&mut s, c, Some("g"));
        let wide = render(&s, view(-30., -30., 80., 80., dpr));
        for (x, y, w, h) in [
            (0.25, 5.5, 12., 12.),
            (-7.5, 10.25, 5., 4.),
            (-2., 18., 4., 4.),
            (-7., 7., 4., 4.),
        ] {
            let tight = render(&s, view(x, y, w, h, dpr));
            for yy in 0..tight.height {
                for xx in 0..tight.width {
                    let px = (tight.screen_bounds.origin.x * dpr).round() as i32 + xx as i32;
                    let py = (tight.screen_bounds.origin.y * dpr).round() as i32 + yy as i32;
                    assert_eq!(
                        pixel(&tight, px, py, dpr),
                        pixel(&wide, px, py, dpr),
                        "dpr={dpr} {px},{py}"
                    );
                }
            }
        }
        assert!(pixel(&wide, 0, (14. * dpr) as i32, dpr)[3] > 0);
    }
}
fn exact_admission(s: &Scene, v: RasterViewport) {
    let source = source(s);
    let mut reference = PixelBudget::new(CompositeLimits::default(), 7).unwrap();
    let frame = rasterize(&source, v, &mut reference, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    let limits = CompositeLimits {
        max_surface_bytes: reference.peak_bytes(),
        max_surface_writes: reference.surface_writes(),
        ..Default::default()
    };
    let mut exact = PixelBudget::new(limits, 7).unwrap();
    let repeat = rasterize(&source, v, &mut exact, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(frame.premul_rgba, repeat.premul_rgba);
    assert_eq!(
        (exact.peak_bytes(), exact.surface_writes()),
        (reference.peak_bytes(), reference.surface_writes())
    );
    exact.discard_frame(repeat);
    assert_eq!(exact.retained_bytes(), 7);
    for (limits, error) in [
        (
            CompositeLimits {
                max_surface_bytes: limits.max_surface_bytes - 1,
                ..limits
            },
            CompositeError::WorkingSetLimit,
        ),
        (
            CompositeLimits {
                max_surface_writes: limits.max_surface_writes - 1,
                ..limits
            },
            CompositeError::WorkLimit,
        ),
    ] {
        let mut denied = PixelBudget::new(limits, 7).unwrap();
        assert_eq!(
            rasterize(&source, v, &mut denied, &AtomicBool::new(false)).unwrap_err(),
            error
        );
        assert_eq!(
            (
                denied.retained_bytes(),
                denied.peak_bytes(),
                denied.surface_writes()
            ),
            (7, 7, 0)
        );
    }
}
#[test]
fn blur_exact_admission_replays_shadows_clips_masks_erasers_and_both_orders() {
    for order in [
        ContainerPaintOrder::BeforeChildren,
        ContainerPaintOrder::StrokeAfterChildren,
    ] {
        for mask_opacity in [0.5, 1.] {
            let mut s = group(0.3);
            s.update_object(&"g".into(), |o| {
                o.style.shadow = Some(Shadow::new(2., 0., 0.3, Color::BLACK));
                o.style.grayscale = 0.4;
            })
            .unwrap();
            insert(
                &mut s,
                DisplayObject::new(
                    "f",
                    display::Frame::new(20., 20.)
                        .rounded(3.)
                        .with_paint_order(order),
                )
                .with_style(Style {
                    blur: 0.2,
                    stroke: Some(Stroke::new(Color::WHITE, 2.)),
                    inner_shadow_layers: vec![Shadow::new(1., 0., 0.2, Color::BLACK)],
                    ..Default::default()
                }),
                Some("g"),
            );
            insert(
                &mut s,
                DisplayObject::new("m", display::Group::new()),
                Some("f"),
            );
            let mut mask =
                rect("mask", 0., 0., 16., 16., Color::WHITE, 0.3).with_mask(MaskMode::Alpha);
            mask.style.opacity = mask_opacity;
            insert(&mut s, mask, Some("m"));
            insert(
                &mut s,
                rect("r", 1., 1., 10., 10., Color::rgb(200, 30, 90), 0.2),
                Some("m"),
            );
            insert(
                &mut s,
                rect("e", 5., 5., 3., 3., Color::WHITE, 0.3).with_eraser(EraserMode::Pixel),
                Some("m"),
            );
            exact_admission(&s, view(-10., -10., 50., 50., 1.));
        }
    }
}
#[test]
fn blur_path_ignores_filter_and_pixel_softens_only_owned_alpha() {
    let mut s = group(0.);
    insert(
        &mut s,
        rect("r", 10., 10., 20., 20., Color::rgb(0, 0, 255), 0.),
        Some("g"),
    );
    insert(
        &mut s,
        rect("e", 16., 16., 4., 4., Color::WHITE, 0.).with_eraser(EraserMode::Path),
        Some("g"),
    );
    let base = render(&s, view(0., 0., 50., 50., 1.));
    s.update_object(&"e".into(), |o| {
        o.style.blur = 256.;
        o.style.grayscale = 1.;
    })
    .unwrap();
    let ignored = render(&s, view(0., 0., 50., 50., 1.));
    assert_eq!(base.premul_rgba, ignored.premul_rgba);
    s.update_object(&"e".into(), |o| {
        o.eraser = EraserMode::Pixel;
        o.style.blur = 1.;
    })
    .unwrap();
    let soft = render(&s, view(0., 0., 50., 50., 1.));
    assert!(pixel(&soft, 15, 18, 1.)[3] < 255);
    assert!(pixel(&soft, 18, 18, 1.)[3] > 0);
    assert_eq!(pixel(&soft, 12, 12, 1.), [0, 0, 255, 255]);
}
#[test]
fn blur_pass_through_backdrop_rejects_and_normal_boundary_recovers() {
    let mut s = group(1.);
    let mut r = rect("r", 10., 10., 10., 10., Color::WHITE, 0.);
    r.style.blend_mode = BlendMode::Multiply;
    insert(&mut s, r, Some("g"));
    assert_eq!(
        capture_subtree(&s, &"g".into(), CompositeLimits::default()).unwrap_err(),
        CompositeError::UnsupportedBlurBackdrop("g".into())
    );
    assert_eq!(
        owned_canvas(
            &s,
            CanvasBackdrop {
                color: Color::WHITE,
                grid: None
            },
            CompositeLimits::default()
        )
        .unwrap_err(),
        CompositeError::UnsupportedBlurBackdrop("g".into())
    );
    s.update_object(&"g".into(), |o| o.style.blend_mode = BlendMode::Normal)
        .unwrap();
    assert!(
        render(&s, view(0., 0., 40., 40., 1.))
            .premul_rgba
            .iter()
            .any(|b| *b != 0)
    );
}
#[test]
fn blur_full_source_text_image_canvas_and_missing_producer_are_atomic() {
    for producer in [0, 1, 2] {
        let mut s = group(1.);
        let shape: Shape = match producer {
            0 => display::Text::new("ink", Size::new(8., 8.)).into(),
            1 => display::Image::new("a.png", Size::new(8., 8.)).into(),
            _ => display::Canvas::new(8., 8.).into(),
        };
        insert(
            &mut s,
            DisplayObject::new("ink", shape).with_transform(Affine::translation(-5., 5.)),
            Some("g"),
        );
        let src = source(&s);
        if producer == 0 {
            assert!(src.text_sources()[0].2);
        }
        let mut budget = PixelBudget::new(CompositeLimits::default(), 9).unwrap();
        assert!(
            rasterize(
                &src,
                view(0., 0., 30., 30., 1.),
                &mut budget,
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert_eq!(budget.retained_bytes(), 9);
        let image = Arc::new(
            StaticImage::from_premultiplied_rgba(
                8,
                8,
                [128, 0, 0, 128].repeat(64),
                CompositeLimits::default(),
            )
            .unwrap(),
        );
        let mut images = ImageInputs::default();
        let mut texts = TextInputs::default();
        if producer == 0 {
            texts.insert(
                "ink".into(),
                Ok(Some(Arc::new(TextImage {
                    image,
                    local_bounds: Bounds::new(Point::ZERO, Size::new(8., 8.)),
                }))),
            );
        } else {
            images.insert("ink".into(), Ok(image));
        }
        let f = rasterize_with_inputs(
            &src,
            &images,
            &texts,
            view(0., 0., 30., 30., 1.),
            &mut budget,
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        assert!(pixel(&f, 3, 8, 1.)[3] > 0);
    }
}
#[test]
fn blur_huge_sources_and_pre_cancel_reject_without_domain_pixels() {
    let mut s = Scene::new();
    insert(
        &mut s,
        rect("r", -4999., 0., 5000., 2., Color::WHITE, 1.),
        None,
    );
    let src = source(&s);
    let mut b = PixelBudget::new(CompositeLimits::default(), 7).unwrap();
    assert_eq!(
        rasterize(
            &src,
            view(0., 0., 10., 10., 1.),
            &mut b,
            &AtomicBool::new(false)
        )
        .unwrap_err(),
        CompositeError::TooLarge
    );
    assert_eq!(
        (b.retained_bytes(), b.peak_bytes(), b.surface_writes()),
        (7, 7, 0)
    );
    assert_eq!(
        rasterize(
            &src,
            view(0., 0., 10., 10., 1.),
            &mut b,
            &AtomicBool::new(true)
        )
        .unwrap_err(),
        CompositeError::Superseded
    );
}

#[test]
fn blur_child_halo_entering_empty_frame_casts_own_shadows() {
    let mut s = Scene::new();
    insert(
        &mut s,
        DisplayObject::new("g", display::Frame::new(20., 20.)).with_style(Style {
            fill: None,
            shadow: Some(Shadow::new(10., 0., 0., Color::rgb(0, 255, 0))),
            ..Default::default()
        }),
        None,
    );
    insert(
        &mut s,
        rect("r", -5., 5., 4., 8., Color::rgb(255, 0, 0), 1.),
        Some("g"),
    );
    let f = render(&s, view(-10., -10., 50., 50., 1.));
    assert!(pixel(&f, 0, 8, 1.)[0] > 0);
    assert!(
        pixel(&f, 10, 8, 1.)[1] > 0,
        "bounds {:?}, row {:?}",
        f.screen_bounds,
        (-2..25)
            .map(|x| (x, pixel(&f, x, 8, 1.)))
            .collect::<Vec<_>>()
    );
    exact_admission(&s, view(-10., -10., 50., 50., 1.));
    s.update_object(&"g".into(), |o| {
        o.style.shadow = None;
        o.style
            .inner_shadow_layers
            .push(Shadow::new(2., 0., 0., Color::rgb(0, 255, 0)));
    })
    .unwrap();
    assert!(pixel(&render(&s, view(-10., -10., 50., 50., 1.)), 0, 8, 1.)[1] > 0);
}

#[test]
fn blur_large_cancelled_world_camera_translation_preserves_physical_lattice() {
    for dpr in [1., 1.25, 2.] {
        let mut s = Scene::new();
        insert(
            &mut s,
            rect("r", 0., 0., 10., 10., Color::rgba(220, 40, 70, 160), 1.),
            None,
        );
        let base = render(&s, view(-20., -20., 60., 60., dpr));
        s.update_object(&"r".into(), |o| {
            o.transform = Affine::translation(1_000_000_000., 1_000_000_000.)
        })
        .unwrap();
        let mut v = view(-20., -20., 60., 60., dpr);
        v.world_to_screen = Affine::translation(-1_000_000_000., -1_000_000_000.);
        let moved = render(&s, v);
        assert_eq!(base.screen_bounds, moved.screen_bounds);
        assert_eq!(base.premul_rgba, moved.premul_rgba, "dpr {dpr}");
    }
}

#[test]
fn blur_complete_phase_stream_precedes_grayscale_and_whole_node_opacity() {
    for order in [
        ContainerPaintOrder::BeforeChildren,
        ContainerPaintOrder::StrokeAfterChildren,
    ] {
        let mut s = Scene::new();
        insert(
            &mut s,
            DisplayObject::new("g", display::Frame::new(16., 16.).with_paint_order(order))
                .with_transform(Affine::translation(12., 12.))
                .with_style(Style {
                    fill: Some(Color::rgba(255, 0, 0, 140).into()),
                    stroke: Some(Stroke::new(Color::rgba(0, 255, 0, 180), 3.)),
                    shadow: Some(Shadow::new(7., 0., 0.5, Color::rgb(0, 0, 255))),
                    inner_shadow_layers: vec![Shadow::new(
                        3.,
                        0.,
                        0.5,
                        Color::rgba(0, 255, 0, 160),
                    )],
                    ..Default::default()
                }),
            None,
        );
        insert(
            &mut s,
            rect("r", -2., 5., 10., 8., Color::rgba(0, 0, 255, 170), 0.5),
            Some("g"),
        );
        let v = view(-10., -10., 70., 70., 1.);
        let before = render(&s, v);
        s.update_object(&"g".into(), |o| o.style.blur = 1.).unwrap();
        let blurred = render(&s, v);
        let mut input = vec![0; blurred.premul_rgba.len()];
        for y in 0..blurred.height {
            for x in 0..blurred.width {
                let p = pixel(
                    &before,
                    blurred.screen_bounds.origin.x as i32 + x as i32,
                    blurred.screen_bounds.origin.y as i32 + y as i32,
                    1.,
                );
                let i = ((y * blurred.width + x) * 4) as usize;
                input[i..i + 4].copy_from_slice(&p);
            }
        }
        let expected = reference(&input, blurred.width as usize, blurred.height as usize, 1.);
        assert!(
            blurred
                .premul_rgba
                .iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 1),
            "{order:?}"
        );
        s.update_object(&"g".into(), |o| o.style.grayscale = 0.7)
            .unwrap();
        let gray = render(&s, v);
        for (p, q) in blurred
            .premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(gray.premul_rgba.as_chunks::<4>().0)
        {
            let y = (2126. * p[0] as f64 + 7152. * p[1] as f64 + 722. * p[2] as f64) / 10000.;
            for c in 0..3 {
                assert_eq!(
                    q[c],
                    (p[c] as f64 + 0.7f32 as f64 * (y - p[c] as f64))
                        .round()
                        .clamp(0., p[3] as f64) as u8
                );
            }
            assert_eq!(p[3], q[3]);
        }
        s.update_object(&"g".into(), |o| o.style.opacity = 0.5)
            .unwrap();
        let half = render(&s, v);
        assert!(
            half.premul_rgba
                .iter()
                .zip(&gray.premul_rgba)
                .all(|(a, b)| ((i16::from(*a) * 2) - i16::from(*b)).abs() <= 1)
        );
    }
}
#[test]
fn blur_parent_shadow_samples_finished_child_alpha_once() {
    let mut s = group(0.);
    s.update_object(&"g".into(), |o| {
        o.style.shadow = Some(Shadow::new(25., 0., 0., Color::rgb(0, 255, 0)))
    })
    .unwrap();
    insert(
        &mut s,
        rect("r", 5., 5., 8., 8., Color::rgba(255, 0, 0, 160), 1.),
        Some("g"),
    );
    let f = render(&s, view(-10., -10., 70., 50., 1.));
    for y in 0..20 {
        for x in 0..20 {
            let p = pixel(&f, x, y, 1.);
            let q = pixel(&f, x + 25, y, 1.);
            assert_eq!(p[3], q[3]);
            assert_eq!(q[1], q[3]);
        }
    }
}
#[test]
fn blur_child_clip_forms_source_edge_but_inherited_clip_trims_final_halo_once() {
    let mut s = Scene::new();
    insert(
        &mut s,
        DisplayObject::new("f", display::Frame::new(20., 20.).rounded(6.))
            .with_transform(Affine::translation(10.25, 10.5))
            .with_style(Style {
                fill: None,
                blur: 1.,
                ..Default::default()
            }),
        None,
    );
    insert(
        &mut s,
        rect("r", -8., -8., 36., 36., Color::WHITE, 0.),
        Some("f"),
    );
    let v = view(0., 0., 50., 50., 1.25);
    let own = render(&s, v);
    assert!(pixel(&own, 12, 25, 1.25)[3] > 0); // outside own child's clip
    s.update_object(&"f".into(), |o| o.style.blur = 0.).unwrap();
    s.update_object(&"r".into(), |o| o.style.blur = 1.).unwrap();
    let inherited = render(&s, v);
    assert_eq!(pixel(&inherited, 11, 25, 1.25)[3], 0);
    // At the rounded boundary a huge white child is constant through blur;
    // final coverage must equal a single unblurred clip, never squared.
    s.update_object(&"r".into(), |o| o.style.blur = 0.).unwrap();
    let plain = render(&s, v);
    for y in 13..38 {
        for x in 13..38 {
            assert_eq!(pixel(&inherited, x, y, 1.25), pixel(&plain, x, y, 1.25));
        }
    }
}
#[test]
fn blur_alpha_mask_expands_coverage_and_nonseparable_blend_uses_offset_source_indices() {
    let mut s = group(0.);
    insert(
        &mut s,
        rect("m", 12., 12., 5., 5., Color::WHITE, 1.).with_mask(MaskMode::Alpha),
        Some("g"),
    );
    insert(
        &mut s,
        rect("r", 0., 0., 40., 40., Color::rgb(0, 0, 255), 0.),
        Some("g"),
    );
    let f = render(&s, view(0., 0., 40., 40., 1.));
    let m = capture_subtree(&s, &"m".into(), CompositeLimits::default()).unwrap();
    let mask = rasterize(
        &m,
        view(0., 0., 40., 40., 1.),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert!(pixel(&f, 11, 14, 1.)[3] > 0);
    for y in 0..40 {
        for x in 0..40 {
            assert_eq!(pixel(&f, x, y, 1.)[3], pixel(&mask, x, y, 1.)[3]);
        }
    }
    let mut s = group(0.);
    s.update_object(&"g".into(), |o| o.style.blend_mode = BlendMode::Normal)
        .unwrap();
    let backdrop = Color::rgb(60, 130, 210);
    insert(
        &mut s,
        rect("bg", 0., 0., 40., 40., backdrop, 0.),
        Some("g"),
    );
    insert(
        &mut s,
        rect("fg", 12., 12., 5., 5., Color::rgba(230, 40, 80, 170), 1.),
        Some("g"),
    );
    let foreground = capture_subtree(&s, &"fg".into(), CompositeLimits::default()).unwrap();
    let v = view(10., 11., 15., 14., 1.);
    let fg = rasterize(
        &foreground,
        v,
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    s.update_object(&"fg".into(), |o| o.style.blend_mode = BlendMode::Hue)
        .unwrap();
    let f = render(&s, v);
    for y in 11..25 {
        for x in 10..25 {
            let mut expected = [60, 130, 210, 255];
            crate::nonseparable::pixel(
                &mut expected,
                &pixel(&fg, x, y, 1.),
                1.,
                255,
                BlendMode::Hue,
            );
            assert_eq!(pixel(&f, x, y, 1.), expected);
        }
    }
}

#[test]
fn blur_canvas_draw_keeps_snapshot_and_rolls_back_prepared_resource_budget_failure() {
    let mut s = Scene::new();
    insert(
        &mut s,
        rect("r", 0., 0., 8., 8., Color::rgb(255, 0, 0), 1.),
        None,
    );
    let frozen = source(&s);
    let mut canvas =
        CanvasSurface::new(display::Canvas::new(24., 24.), CompositeLimits::default()).unwrap();
    let old = canvas.snapshot();
    s.update_object(&"r".into(), |o| o.style.blur = 0.).unwrap();
    canvas
        .draw(
            &frozen,
            Affine::translation(8., 8.),
            &ImageInputs::default(),
            &TextInputs::default(),
        )
        .unwrap();
    let after = canvas.snapshot();
    assert!(old.premultiplied_rgba().iter().all(|p| *p == 0));
    assert!(after.premultiplied_rgba()[(11 * 24 + 7) * 4 + 3] > 0);
    let mut resources = group(1.);
    insert(
        &mut resources,
        DisplayObject::new("image", display::Image::new("ready.png", Size::new(8., 8.))),
        Some("g"),
    );
    let image = Arc::new(
        StaticImage::from_premultiplied_rgba(
            8,
            8,
            [100, 20, 0, 128].repeat(64),
            CompositeLimits::default(),
        )
        .unwrap(),
    );
    let before_input = image.premultiplied_rgba().to_vec();
    let mut inputs = ImageInputs::default();
    inputs.insert("image".into(), Ok(image.clone()));
    let mut limited = CanvasSurface::new(
        display::Canvas::new(24., 24.),
        CompositeLimits {
            max_surface_writes: 3000,
            ..Default::default()
        },
    )
    .unwrap();
    let before = limited.snapshot();
    let generation = limited.generation();
    assert_eq!(
        limited
            .draw(
                &source(&resources),
                Affine::IDENTITY,
                &inputs,
                &TextInputs::default()
            )
            .unwrap_err(),
        CompositeError::WorkLimit
    );
    assert_eq!(limited.generation(), generation);
    assert!(Arc::ptr_eq(&before, &limited.snapshot()));
    assert_eq!(image.premultiplied_rgba(), before_input);
    let generation = canvas.generation();
    assert!(
        canvas
            .draw(
                &source(&resources),
                Affine::IDENTITY,
                &ImageInputs::default(),
                &TextInputs::default()
            )
            .is_err()
    );
    assert_eq!(canvas.generation(), generation);
    assert!(Arc::ptr_eq(&after, &canvas.snapshot()));
}
fn decode_export(result: &SceneExport) -> Vec<u8> {
    match result.request.format {
        SceneExportFormat::Png => {
            let mut reader = png::Decoder::new(std::io::Cursor::new(&result.bytes))
                .read_info()
                .unwrap();
            let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
            reader.next_frame(&mut bytes).unwrap();
            bytes
        }
        SceneExportFormat::WebPLossless => {
            ::image::load_from_memory_with_format(&result.bytes, ::image::ImageFormat::WebP)
                .unwrap()
                .into_rgba8()
                .into_raw()
        }
        SceneExportFormat::Jpeg { .. } => {
            ::image::load_from_memory_with_format(&result.bytes, ::image::ImageFormat::Jpeg)
                .unwrap()
                .into_rgba8()
                .into_raw()
        }
        _ => unreachable!(),
    }
}
#[test]
fn blur_fixed_fractional_export_crop_matches_full_source_and_lossless_formats() {
    let mut s = Scene::new();
    insert(
        &mut s,
        rect("r", -2., 4., 4., 10., Color::rgb(255, 0, 0), 1.),
        None,
    );
    let crop = Bounds::new(Point::new(0.4, 0.8), Size::new(22.4, 20.8));
    let make = |format| {
        SceneExportPlan::new(
            &s,
            SceneExportRequest {
                world_bounds: crop,
                scale: 1.,
                pixel_ratio: 1.25,
                background: Color::rgb(0, 255, 0),
                format,
            },
            CompositeLimits::default(),
        )
        .unwrap()
    };
    let encode = |p: &SceneExportPlan| {
        p.encode_with_inputs(
            &ImageInputs::default(),
            &TextInputs::default(),
            SceneExportPreparation::default(),
            &AtomicBool::new(false),
        )
        .unwrap()
    };
    let png = encode(&make(SceneExportFormat::Png));
    let webp = encode(&make(SceneExportFormat::WebPLossless));
    assert_eq!((png.width, png.height), (28, 26));
    let lossless = decode_export(&png);
    assert_eq!(lossless, decode_export(&webp));
    assert!(lossless[(8 * 28) * 4] > 0); // entirely off-crop source reaches the crop
    assert_eq!(&lossless[(16 * 28 + 20) * 4..][..4], &[0, 255, 0, 255]);
    let jpg = decode_export(&encode(&make(SceneExportFormat::Jpeg { quality: 100 })));
    for (a, b) in jpg[(16 * 28 + 20) * 4..][..4].iter().zip([0, 255, 0, 255]) {
        assert!(a.abs_diff(b) <= 2);
    }
    let large = SceneExportPlan::new(
        &s,
        SceneExportRequest {
            world_bounds: Bounds::new(Point::new(-7.6, -7.2), Size::new(38.4, 36.8)),
            scale: 1.,
            pixel_ratio: 1.25,
            background: Color::rgb(0, 255, 0),
            format: SceneExportFormat::Png,
        },
        CompositeLimits::default(),
    )
    .unwrap();
    let large = encode(&large);
    let pixels = decode_export(&large);
    for y in 0..26 {
        for x in 0..28 {
            assert_eq!(
                &lossless[(y * 28 + x) * 4..][..4],
                &pixels[((y + 10) * large.width as usize + x + 10) * 4..][..4]
            );
        }
    }
}
#[test]
fn blur_svg_rejects_before_resources_and_hidden_policy_is_unchanged() {
    for shape in [
        Shape::from(display::Image::new("missing.png", Size::new(8., 8.))),
        display::Text::new("missing", Size::new(8., 8.)).into(),
    ] {
        let mut s = Scene::new();
        insert(
            &mut s,
            DisplayObject::new("r", shape).with_style(Style {
                blur: 1.,
                ..Default::default()
            }),
            None,
        );
        let request = SceneExportRequest {
            world_bounds: Bounds::new(Point::ZERO, Size::new(30., 30.)),
            scale: 1.,
            pixel_ratio: 1.,
            background: Color::TRANSPARENT,
            format: SceneExportFormat::Svg,
        };
        assert!(
            matches!(SceneExportPlan::new(&s,request,CompositeLimits::default()),Err(CompositeError::UnsupportedShape(reason)) if reason=="SVG blur")
        );
        for (visible, opacity, blur) in [(false, 1., 1.), (true, 0., 1.), (true, 1., 0.)] {
            s.update_object(&"r".into(), |o| {
                o.visible = visible;
                o.style.opacity = opacity;
                o.style.blur = blur;
            })
            .unwrap();
            assert!(SceneExportPlan::new(&s, request, CompositeLimits::default()).is_ok());
        }
    }
}
#[test]
fn blur_owned_background_and_prior_siblings_are_never_filter_sources() {
    let mut s = Scene::new();
    insert(
        &mut s,
        rect("bg", 0., 0., 40., 40., Color::rgb(0, 255, 0), 0.),
        None,
    );
    insert(
        &mut s,
        rect("fg", 12., 12., 6., 6., Color::rgb(255, 0, 0), 1.),
        None,
    );
    let render_owned = |s: &Scene| {
        let src = owned_canvas(
            s,
            CanvasBackdrop {
                color: Color::rgb(0, 0, 255),
                grid: None,
            },
            CompositeLimits::default(),
        )
        .unwrap();
        rasterize(
            &src,
            view(0., 0., 40., 40., 1.),
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap()
    };
    let f = render_owned(&s);
    assert_eq!(pixel(&f, 1, 1, 1.), [0, 255, 0, 255]);
    // Original background remains opaque green under a separately blurred red
    // source; color/alpha cannot be filtered out of the parent destination.
    let p = pixel(&f, 11, 15, 1.);
    assert!(p[0] > 0 && p[1] > 0);
    assert_eq!(p[3], 255);
    s.update_object(&"fg".into(), |o| o.eraser = EraserMode::Pixel)
        .unwrap();
    let erased = render_owned(&s);
    let edge = pixel(&erased, 11, 15, 1.);
    assert_eq!(edge[0], 0);
    assert!(edge[1] > 0 && edge[2] > 0);
    assert_eq!(edge[3], 255);
    assert_eq!(pixel(&erased, 1, 1, 1.), [0, 255, 0, 255]);
}

#[test]
fn blur_recursive_replay_depth_and_chain_cost_reject_before_output() {
    let mut s = group(0.2);
    let mut parent = "g".to_owned();
    for index in 0..7 {
        let id = format!("g{index}");
        insert(
            &mut s,
            DisplayObject::new(id.clone(), display::Group::new()).with_style(Style {
                blur: 0.2,
                shadow: Some(Shadow::new(1., 0., 0., Color::BLACK)),
                inner_shadow_layers: vec![Shadow::new(1., 0., 0., Color::BLACK)],
                ..Default::default()
            }),
            Some(&parent),
        );
        parent = id;
    }
    insert(
        &mut s,
        rect("r", 0., 0., 4., 4., Color::WHITE, 0.2),
        Some(&parent),
    );
    let src = source(&s);
    for (limits, error) in [
        (CompositeLimits::default(), CompositeError::WorkLimit),
        (
            CompositeLimits {
                max_depth: 1,
                ..Default::default()
            },
            CompositeError::NestingLimit,
        ),
        (
            CompositeLimits {
                max_nodes: 1,
                ..Default::default()
            },
            CompositeError::InputLimit,
        ),
    ] {
        let mut b = PixelBudget::new(limits, 7).unwrap();
        assert_eq!(
            rasterize(
                &src,
                view(-5., -5., 10., 10., 1.),
                &mut b,
                &AtomicBool::new(false)
            )
            .unwrap_err(),
            error
        );
        assert_eq!(
            (b.retained_bytes(), b.peak_bytes(), b.surface_writes()),
            (7, 7, 0)
        );
    }
}

#[test]
fn blur_sources_wholly_outside_each_view_edge_and_corner_keep_visible_halos() {
    for dpr in [1., 1.25, 2.] {
        for (x, y, w, h, px, py) in [
            (-5., 8., 4., 4., 0., 10.),
            (21., 8., 4., 4., 19., 10.),
            (8., -5., 4., 4., 10., 0.),
            (8., 21., 4., 4., 10., 19.),
            (-3., -3., 2., 2., 0., 0.),
            (21., -3., 2., 2., 19., 0.),
            (-3., 21., 2., 2., 0., 19.),
            (21., 21., 2., 2., 19., 19.),
        ] {
            let mut s = Scene::new();
            insert(&mut s, rect("r", x, y, w, h, Color::WHITE, 1.5), None);
            let wide = render(&s, view(-20., -20., 60., 60., dpr));
            let tight = render(&s, view(0., 0., 20., 20., dpr));
            assert!(
                pixel(&tight, (px * dpr) as i32, (py * dpr) as i32, dpr)[3] > 0,
                "{x},{y} dpr {dpr}"
            );
            for yy in 0..tight.height {
                for xx in 0..tight.width {
                    let px = (tight.screen_bounds.origin.x * dpr).round() as i32 + xx as i32;
                    let py = (tight.screen_bounds.origin.y * dpr).round() as i32 + yy as i32;
                    assert_eq!(pixel(&tight, px, py, dpr), pixel(&wide, px, py, dpr));
                }
            }
        }
    }
}

use super::*;
use gpui_flow_scene::{display, *};
use std::sync::{Arc, atomic::AtomicBool};

fn bounds() -> Bounds {
    Bounds::new(Point::ZERO, Size::new(100., 100.))
}
fn viewport() -> RasterViewport {
    RasterViewport {
        world_to_screen: Affine::IDENTITY,
        canvas_bounds: bounds(),
        content_mask: bounds(),
        device_scale: 1.,
    }
}
fn rect(id: &str, color: Color) -> DisplayObject {
    DisplayObject::new(id, display::Rect::new(20., 20.))
        .with_transform(Affine::translation(10., 10.))
        .with_style(Style::fill(color))
}
fn scene(color: Color, amount: f32) -> Scene {
    let mut s = Scene::new();
    let mut r = rect("r", color);
    r.style.grayscale = amount;
    s.insert(r, None, None).unwrap();
    s
}
fn source(s: &Scene) -> DomainSource {
    capture_subtree(s, &s.roots()[0], CompositeLimits::default()).unwrap()
}
fn render_source(source: &DomainSource) -> CpuFrame {
    rasterize(
        source,
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}
fn render(s: &Scene) -> CpuFrame {
    render_source(&source(s))
}
fn pixel(f: &CpuFrame, x: usize, y: usize) -> [u8; 4] {
    let x = x as i64 - f.screen_bounds.origin.x as i64;
    let y = y as i64 - f.screen_bounds.origin.y as i64;
    if x < 0 || y < 0 || x >= f.width as i64 || y >= f.height as i64 {
        return [0; 4];
    }
    f.premul_rgba[((y as u32 * f.width + x as u32) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}
fn filtered(p: [u8; 4], amount: f64) -> [u8; 4] {
    let y = (2126. * p[0] as f64 + 7152. * p[1] as f64 + 722. * p[2] as f64) / 10000.;
    let mut out = p;
    for i in 0..3 {
        out[i] = (p[i] as f64 + amount * (y - p[i] as f64))
            .round()
            .clamp(0., p[3] as f64) as u8;
    }
    out
}
fn assert_alpha_eq(a: &CpuFrame, b: &CpuFrame) {
    assert_eq!(
        (a.screen_bounds, a.width, a.height),
        (b.screen_bounds, b.width, b.height)
    );
    assert!(
        a.premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(b.premul_rgba.as_chunks::<4>().0.iter())
            .all(|(a, b)| a[3] == b[3])
    );
}
fn insert(s: &mut Scene, o: DisplayObject, parent: &str) {
    s.insert(o, Some(parent.into()), None).unwrap();
}
fn group_scene(amount: f32) -> Scene {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("g", display::Group::new()).with_style(Style {
            grayscale: amount,
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    s
}

#[test]
fn grayscale_exact_srgb_primaries_partial_low_alpha_and_neutral_identity() {
    for (color, amount, expected) in [
        (Color::rgb(255, 0, 0), 1., [54, 54, 54, 255]),
        (Color::rgb(0, 255, 0), 1., [182, 182, 182, 255]),
        (Color::rgb(0, 0, 255), 1., [18, 18, 18, 255]),
        (Color::rgba(255, 0, 0, 128), 1., [27, 27, 27, 128]),
        (Color::rgb(255, 0, 0), 0.5, [155, 27, 27, 255]),
        (Color::rgba(255, 128, 0, 128), 1., [73, 73, 73, 128]),
        (Color::rgb(255, 0, 0), 0., [255, 0, 0, 255]),
        (Color::rgb(71, 71, 71), 1., [71, 71, 71, 255]),
        (Color::WHITE, 1., [255; 4]),
    ] {
        assert_eq!(
            pixel(&render(&scene(color, amount)), 20, 20),
            expected,
            "{color:?}/{amount}"
        );
    }
}

#[test]
fn grayscale_nested_groups_filter_composed_overlap_stroke_and_local_opacity_once() {
    let mut s = group_scene(0.5);
    insert(&mut s, rect("r", Color::rgb(255, 0, 0)), "g");
    let mut child = rect("b", Color::rgba(0, 0, 255, 128));
    child.style.grayscale = 0.5;
    child.style.stroke = Some(Stroke::new(Color::rgb(0, 255, 0), 4.));
    insert(&mut s, child, "g");
    let before = render(&s);
    // Child blue -> [5,5,69,128], source-over red -> [132,5,69,255], then parent.
    assert_eq!(pixel(&before, 20, 20), filtered([132, 5, 69, 255], 0.5));
    let stroke = pixel(&before, 9, 20);
    assert_eq!(stroke, filtered(filtered([0, 255, 0, 255], 0.5), 0.5));
    s.update_object(&"g".into(), |o| o.style.opacity = 0.5)
        .unwrap();
    let half = render(&s);
    let overlap = pixel(&half, 20, 20);
    assert_eq!(overlap[3], 128); // complete layer opacity, not each overlapping child
    for (a, b) in overlap[..3].iter().zip(pixel(&before, 20, 20)) {
        assert!((*a as i16 * 2 - b as i16).abs() <= 1);
    }
}

#[test]
fn grayscale_own_and_child_colored_shadows_run_before_filter_in_both_container_orders() {
    for order in [
        ContainerPaintOrder::BeforeChildren,
        ContainerPaintOrder::StrokeAfterChildren,
    ] {
        for frame in [false, true] {
            let mut s = Scene::new();
            let shape: Shape = if frame {
                display::Frame::new(35., 35.).with_paint_order(order).into()
            } else {
                display::Box::new(35., 35.).with_paint_order(order).into()
            };
            s.insert(
                DisplayObject::new("g", shape)
                    .with_transform(Affine::translation(10., 10.))
                    .with_style(Style {
                        fill: Some(Color::rgb(255, 0, 0).into()),
                        stroke: Some(Stroke::new(Color::rgb(0, 255, 0), 3.)),
                        shadow: Some(Shadow::new(45., 0., 0., Color::rgb(0, 0, 255))),
                        inner_shadow_layers: vec![Shadow::new(8., 0., 1., Color::rgb(0, 255, 0))],
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
            let mut child = rect("child", Color::rgba(0, 0, 255, 128));
            child.style.shadow = Some(Shadow::new(6., 0., 0., Color::rgb(255, 0, 0)));
            insert(&mut s, child, "g");
            let before = render(&s);
            assert!(
                before
                    .premul_rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[0] != p[1])
            );
            s.update_object(&"g".into(), |o| o.style.grayscale = 1.)
                .unwrap();
            let after = render(&s);
            assert_alpha_eq(&before, &after);
            for (a, b) in before
                .premul_rgba
                .as_chunks::<4>()
                .0
                .iter()
                .zip(after.premul_rgba.as_chunks::<4>().0.iter())
            {
                assert_eq!(*b, filtered(*a, 1.));
            }
            assert_eq!(pixel(&after, 60, 20), [18, 18, 18, 255]);
        }
    }
}

#[test]
fn grayscale_rounded_ancestor_clip_keeps_fractional_alpha_once() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("clip", display::Frame::new(30., 30.).rounded(9.))
            .with_transform(Affine::translation(10.25, 10.5))
            .with_style(Style {
                fill: None,
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    insert(
        &mut s,
        DisplayObject::new("g", display::Group::new()).with_style(Style {
            blend_mode: BlendMode::Normal,
            ..Default::default()
        }),
        "clip",
    );
    for id in ["a", "b"] {
        insert(
            &mut s,
            DisplayObject::new(id, display::Rect::new(40., 40.))
                .with_style(Style::fill(Color::rgb(255, 0, 0))),
            "g",
        );
    }
    let capture = |s: &Scene| match scene_units(s, CompositeLimits::default())
        .unwrap()
        .into_iter()
        .find(|u| matches!(u, SceneUnit::Domain(_)))
        .unwrap()
    {
        SceneUnit::Domain(d) => *d,
        _ => unreachable!(),
    };
    let before = render_source(&capture(&s));
    s.update_object(&"g".into(), |o| o.style.grayscale = 1.)
        .unwrap();
    let after = render_source(&capture(&s));
    assert_alpha_eq(&before, &after);
    assert!(
        after
            .premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0 && p[3] < 255)
    );
    assert_eq!(pixel(&after, 25, 25), [54, 54, 54, 255]);
}

#[test]
fn grayscale_image_text_and_canvas_ink_keep_every_alpha_byte_without_full_text_sampling() {
    for kind in ["image", "text", "canvas"] {
        let pixels: Vec<u8> = (0..256u16).flat_map(|a| [a as u8, 0, 0, a as u8]).collect();
        let image = Arc::new(
            StaticImage::from_premultiplied_rgba(16, 16, pixels, CompositeLimits::default())
                .unwrap(),
        );
        let shape: Shape = match kind {
            "image" => display::Image::new("local.png", Size::new(16., 16.)).into(),
            "text" => display::Text::new("ink", Size::new(16., 16.)).into(),
            _ => display::Canvas::new(16., 16.).into(),
        };
        let mut s = Scene::new();
        s.insert(
            DisplayObject::new("r", shape).with_transform(Affine::translation(10., 10.)),
            None,
            None,
        )
        .unwrap();
        let mut images = ImageInputs::default();
        let mut texts = TextInputs::default();
        if kind == "text" {
            texts.insert(
                "r".into(),
                Ok(Some(Arc::new(TextImage {
                    image,
                    local_bounds: Bounds::new(Point::ZERO, Size::new(16., 16.)),
                }))),
            );
        } else {
            images.insert("r".into(), Ok(image));
        }
        let render = |s: &Scene| {
            rasterize_with_inputs(
                &source(s),
                &images,
                &texts,
                viewport(),
                &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
                &AtomicBool::new(false),
            )
            .unwrap()
            .unwrap()
        };
        let before = render(&s);
        s.update_object(&"r".into(), |o| o.style.grayscale = 1.)
            .unwrap();
        let source = source(&s);
        assert!(source.text_sources().iter().all(|(_, _, full)| !*full));
        let after = render(&s);
        assert_alpha_eq(&before, &after);
        for (a, b) in before
            .premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(after.premul_rgba.as_chunks::<4>().0.iter())
        {
            assert_eq!(*b, filtered(*a, 1.));
        }
    }
}

#[test]
fn grayscale_owned_backdrop_and_outside_siblings_keep_their_color_and_own_blend_runs_last() {
    let mut s = Scene::new();
    s.insert(rect("prior", Color::rgb(0, 0, 255)), None, None)
        .unwrap();
    let mut r = rect("r", Color::rgb(255, 0, 0));
    r.transform = Affine::translation(20., 10.);
    r.style.grayscale = 1.;
    s.insert(r, None, None).unwrap();
    let mut last = rect("last", Color::rgb(0, 255, 0));
    last.transform = Affine::translation(35., 10.);
    s.insert(last, None, None).unwrap();
    let capture = |s: &Scene| {
        owned_canvas(
            s,
            CanvasBackdrop {
                color: Color::rgb(255, 0, 255),
                grid: None,
            },
            CompositeLimits::default(),
        )
        .unwrap()
    };
    let f = render_source(&capture(&s));
    assert_eq!(pixel(&f, 15, 15), [0, 0, 255, 255]);
    assert_eq!(pixel(&f, 25, 15), [54, 54, 54, 255]);
    assert_eq!(pixel(&f, 40, 15), [0, 255, 0, 255]);
    assert_eq!(pixel(&f, 90, 90), [255, 0, 255, 255]);
    s.update_object(&"r".into(), |o| o.style.blend_mode = BlendMode::Multiply)
        .unwrap();
    assert_eq!(pixel(&render_source(&capture(&s)), 25, 15), [0, 0, 54, 255]);
    assert!(
        scene_units(&s, CompositeLimits::default())
            .unwrap()
            .iter()
            .any(|u| matches!(
                u,
                SceneUnit::Rejected(CompositeIssue {
                    error: CompositeError::UnsupportedExternalBackdrop,
                    ..
                })
            ))
    );
}

#[test]
fn grayscale_unsafe_pass_through_capture_rejects_even_inside_owned_canvas() {
    for shape in [
        Shape::from(display::Group::new()),
        display::Box::new(40., 40.).into(),
        display::Frame::new(40., 40.).into(),
    ] {
        let mut s = Scene::new();
        s.insert(
            DisplayObject::new("g", shape).with_style(Style {
                grayscale: 1.,
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
        let mut r = rect("r", Color::WHITE);
        r.style.blend_mode = BlendMode::Multiply;
        insert(&mut s, r, "g");
        assert!(matches!(
            scene_units(&s, CompositeLimits::default()).unwrap()[0],
            SceneUnit::Rejected(CompositeIssue {
                error: CompositeError::UnsupportedGrayscaleBackdrop(_),
                ..
            })
        ));
        assert!(matches!(
            owned_canvas(
                &s,
                CanvasBackdrop {
                    color: Color::WHITE,
                    grid: None
                },
                CompositeLimits::default()
            ),
            Err(CompositeError::UnsupportedGrayscaleBackdrop(_))
        ));
        for (opacity, blend) in [(0.5, BlendMode::PassThrough), (1., BlendMode::Normal)] {
            s.update_object(&"g".into(), |o| {
                o.style.opacity = opacity;
                o.style.blend_mode = blend;
            })
            .unwrap();
            assert!(matches!(
                scene_units(&s, CompositeLimits::default()).unwrap()[0],
                SceneUnit::Domain(_)
            ));
        }
    }
}

#[test]
fn grayscale_pixel_eraser_preserves_erasure_path_skips_filter_and_parent_filters_survivors() {
    for mode in [EraserMode::Pixel, EraserMode::Path] {
        let mut s = group_scene(0.);
        insert(&mut s, rect("r", Color::rgb(255, 0, 0)), "g");
        let mut erase = DisplayObject::new("erase", display::Rect::new(10., 10.))
            .with_transform(Affine::translation(20., 10.))
            .with_style(Style::fill(Color::rgba(0, 255, 0, 128)))
            .with_eraser(mode);
        erase.style.opacity = 0.5;
        insert(&mut s, erase, "g");
        let measure = |s: &Scene| {
            let mut b = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
            let f = rasterize(&source(s), viewport(), &mut b, &AtomicBool::new(false))
                .unwrap()
                .unwrap();
            (f, b)
        };
        let (before, base) = measure(&s);
        s.update_object(&"erase".into(), |o| o.style.grayscale = 1.)
            .unwrap();
        let (after, filtered) = measure(&s);
        assert_eq!(before.premul_rgba, after.premul_rgba);
        if mode == EraserMode::Path {
            assert_eq!(base.surface_writes(), filtered.surface_writes());
            assert_eq!(base.peak_bytes(), filtered.peak_bytes());
        } else {
            assert_eq!(
                filtered.surface_writes() - base.surface_writes(),
                after.width as u64 * after.height as u64
            );
        }
        s.update_object(&"g".into(), |o| o.style.grayscale = 1.)
            .unwrap();
        let final_frame = render(&s);
        assert_alpha_eq(&after, &final_frame);
        assert_eq!(pixel(&final_frame, 15, 15), [54, 54, 54, 255]);
    }
}

#[test]
fn grayscale_alpha_masks_ignore_rgb_and_pass_through_filter_does_not_flush_run() {
    for opacity in [0.5, 1.] {
        let mut s = group_scene(0.);
        let mut mask = rect("mask", Color::rgba(255, 0, 0, 128)).with_mask(MaskMode::Alpha);
        mask.style.opacity = opacity;
        insert(&mut s, mask, "g");
        insert(&mut s, rect("a", Color::rgb(0, 255, 0)), "g");
        let mut b = rect("b", Color::rgba(0, 0, 255, 128));
        b.style.grayscale = 1.;
        insert(&mut s, b, "g");
        let before = render(&s);
        s.update_object(&"mask".into(), |o| o.style.grayscale = 1.)
            .unwrap();
        let after = render(&s);
        assert_eq!(before.premul_rgba, after.premul_rgba);
        let expected = if opacity == 1. {
            [5, 68, 5, 128]
        } else {
            [2, 34, 2, 64]
        };
        for (a, b) in pixel(&after, 20, 20).iter().zip(expected) {
            assert!(
                (*a as i16 - b as i16).abs() <= 1,
                "{:?}",
                pixel(&after, 20, 20)
            );
        }
        s.update_object(&"mask".into(), |o| o.visible = false)
            .unwrap();
        assert_eq!(pixel(&render(&s), 20, 20)[3], 255);
        let mut active = rect("active", Color::WHITE).with_mask(MaskMode::Alpha);
        active.z_index = -1;
        insert(&mut s, active, "g");
        assert!(render(&s).premul_rgba.iter().all(|b| *b == 0));
    }
}

fn assert_admission_matches(s: &Scene) {
    let source = source(s);
    let mut reference = PixelBudget::new(CompositeLimits::default(), 7).unwrap();
    let f = rasterize(&source, viewport(), &mut reference, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    let exact = CompositeLimits {
        max_surface_bytes: reference.peak_bytes(),
        max_surface_writes: reference.surface_writes(),
        ..Default::default()
    };
    let mut budget = PixelBudget::new(exact, 7).unwrap();
    let again = rasterize(&source, viewport(), &mut budget, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(again.premul_rgba, f.premul_rgba);
    assert_eq!(budget.peak_bytes(), reference.peak_bytes());
    assert_eq!(budget.surface_writes(), reference.surface_writes());
    budget.discard_frame(again);
    assert_eq!(budget.retained_bytes(), 7);
    for (limits, error) in [
        (
            CompositeLimits {
                max_surface_bytes: exact.max_surface_bytes - 1,
                ..exact
            },
            CompositeError::WorkingSetLimit,
        ),
        (
            CompositeLimits {
                max_surface_writes: exact.max_surface_writes - 1,
                ..exact
            },
            CompositeError::WorkLimit,
        ),
    ] {
        let mut b = PixelBudget::new(limits, 7).unwrap();
        assert_eq!(
            rasterize(&source, viewport(), &mut b, &AtomicBool::new(false)).unwrap_err(),
            error
        );
        assert_eq!(
            (b.retained_bytes(), b.peak_bytes(), b.surface_writes()),
            (7, 7, 0)
        );
    }
}

#[test]
fn grayscale_new_isolation_has_exact_no_scratch_bytes_and_pass_work() {
    let mut s = scene(Color::rgb(255, 0, 0), 0.);
    let mut base = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    let before = rasterize(&source(&s), viewport(), &mut base, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    s.update_object(&"r".into(), |o| o.style.grayscale = 1.)
        .unwrap();
    let mut filtered = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    let after = rasterize(
        &source(&s),
        viewport(),
        &mut filtered,
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_alpha_eq(&before, &after);
    let n = after.width as u64 * after.height as u64;
    assert_eq!((base.peak_bytes(), base.surface_writes()), (4 * n, n));
    assert_eq!(
        (filtered.peak_bytes(), filtered.surface_writes()),
        (8 * n, 3 * n)
    );
    assert_admission_matches(&s);
}

#[test]
fn grayscale_repeated_shadow_source_admission_matches_runtime_masks_clips_erasers_and_orders() {
    for order in [
        ContainerPaintOrder::BeforeChildren,
        ContainerPaintOrder::StrokeAfterChildren,
    ] {
        for mask_opacity in [0.5, 1.] {
            let mut s = group_scene(0.5);
            s.update_object(&"g".into(), |o| {
                o.style.shadow_layers = vec![
                    Shadow::new(12., 0., 2., Color::rgb(255, 0, 0)),
                    Shadow::new(0., 12., 0., Color::rgb(0, 255, 0)),
                ];
                o.style.inner_shadow_layers = vec![Shadow::new(4., 0., 1., Color::rgb(0, 0, 255))];
            })
            .unwrap();
            insert(
                &mut s,
                DisplayObject::new(
                    "frame",
                    display::Frame::new(45., 45.)
                        .rounded(6.)
                        .with_paint_order(order),
                )
                .with_style(Style {
                    grayscale: 0.25,
                    stroke: Some(Stroke::new(Color::WHITE, 3.)),
                    ..Default::default()
                }),
                "g",
            );
            insert(
                &mut s,
                DisplayObject::new("masked", display::Group::new()),
                "frame",
            );
            let mut m = rect("mask", Color::rgba(255, 0, 0, 180)).with_mask(MaskMode::Alpha);
            m.style.opacity = mask_opacity;
            m.style.grayscale = 1.;
            insert(&mut s, m, "masked");
            let mut r = rect("r", Color::rgba(0, 255, 0, 160));
            r.style.grayscale = 0.5;
            r.style.shadow = Some(Shadow::new(4., 0., 1., Color::rgb(0, 0, 255)));
            insert(&mut s, r, "masked");
            let mut erase =
                rect("erase", Color::rgba(0, 255, 0, 128)).with_eraser(EraserMode::Pixel);
            erase.transform = Affine::translation(23., 23.);
            erase.style.grayscale = 1.;
            insert(&mut s, erase, "masked");
            assert_admission_matches(&s);
        }
    }
}

#[test]
fn grayscale_depth_precancel_and_missing_producers_return_no_frame_or_live_byte_leak() {
    let s = scene(Color::WHITE, 1.);
    let src = source(&s);
    for (limits, cancel, error) in [
        (
            CompositeLimits {
                max_depth: 0,
                ..Default::default()
            },
            false,
            CompositeError::NestingLimit,
        ),
        (CompositeLimits::default(), true, CompositeError::Superseded),
    ] {
        let mut b = PixelBudget::new(limits, 7).unwrap();
        assert_eq!(
            rasterize(&src, viewport(), &mut b, &AtomicBool::new(cancel)).unwrap_err(),
            error
        );
        assert_eq!(
            (b.retained_bytes(), b.peak_bytes(), b.surface_writes()),
            (7, 7, 0)
        );
    }
    for shape in [
        Shape::from(display::Text::new("ink", Size::new(20., 20.))),
        display::Image::new("missing.png", Size::new(20., 20.)).into(),
    ] {
        let mut s = group_scene(1.);
        insert(&mut s, DisplayObject::new("missing", shape), "g");
        let mut b = PixelBudget::new(CompositeLimits::default(), 7).unwrap();
        assert!(rasterize(&source(&s), viewport(), &mut b, &AtomicBool::new(false)).is_err());
        assert_eq!(b.retained_bytes(), 7);
    }
}

#[test]
fn grayscale_domain_presence_and_placement_follow_zero_and_positive_amount_changes() {
    let mut s = scene(Color::WHITE, 0.);
    assert!(matches!(
        scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Native(_)
    ));
    let zero = source(&s);
    s.update_object(&"r".into(), |o| o.style.grayscale = 0.25)
        .unwrap();
    let first = source(&s);
    assert!(matches!(
        scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Domain(_)
    ));
    assert!(!zero.placement_eq(&first));
    s.update_object(&"r".into(), |o| o.style.grayscale = 0.75)
        .unwrap();
    let second = source(&s);
    assert!(!first.placement_eq(&second));
    assert_ne!(first, second);
    s.update_object(&"r".into(), |o| o.style.grayscale = 0.)
        .unwrap();
    assert!(zero.placement_eq(&source(&s)));
    assert_eq!(pixel(&render_source(&first), 20, 20), [255; 4]); // immutable old snapshot stays valid independently
}

#[test]
fn grayscale_canvas_draw_is_transactional_and_source_snapshot_is_immutable() {
    let mut s = scene(Color::rgb(255, 0, 0), 1.);
    let snapshot = source(&s);
    let mut canvas =
        CanvasSurface::new(display::Canvas::new(40., 40.), CompositeLimits::default()).unwrap();
    let old = canvas.snapshot();
    s.update_object(&"r".into(), |o| o.style.grayscale = 0.)
        .unwrap();
    canvas
        .draw(
            &snapshot,
            Affine::translation(5., 5.),
            &ImageInputs::default(),
            &TextInputs::default(),
        )
        .unwrap();
    let after = canvas.snapshot();
    let index = (10 * 40 + 10) * 4;
    assert_eq!(
        &after.premultiplied_rgba()[index..index + 4],
        &[54, 54, 54, 255]
    );
    assert!(old.premultiplied_rgba().iter().all(|p| *p == 0));
    let generation = canvas.generation();
    let mut bad = group_scene(1.);
    insert(
        &mut bad,
        DisplayObject::new("missing", display::Text::new("ink", Size::new(20., 20.))),
        "g",
    );
    assert!(matches!(
        canvas.draw(
            &source(&bad),
            Affine::IDENTITY,
            &ImageInputs::default(),
            &TextInputs::default()
        ),
        Err(CompositeError::TextBackendRequired(_))
    ));
    assert_eq!(canvas.generation(), generation);
    assert!(Arc::ptr_eq(&after, &canvas.snapshot()));
    let mut limited = CanvasSurface::new(
        display::Canvas::new(40., 40.),
        CompositeLimits {
            max_surface_writes: 1600,
            ..Default::default()
        },
    )
    .unwrap();
    let before = limited.snapshot();
    assert_eq!(
        limited
            .draw(
                &snapshot,
                Affine::IDENTITY,
                &ImageInputs::default(),
                &TextInputs::default()
            )
            .unwrap_err(),
        CompositeError::WorkLimit
    );
    assert!(Arc::ptr_eq(&before, &limited.snapshot()));
}

#[test]
fn grayscale_png_webp_jpeg_exports_keep_background_and_snapshot_semantics() {
    let mut s = scene(Color::rgb(255, 0, 0), 1.);
    for format in [
        SceneExportFormat::Png,
        SceneExportFormat::WebPLossless,
        SceneExportFormat::Jpeg { quality: 100 },
    ] {
        s.update_object(&"r".into(), |o| o.style.grayscale = 1.)
            .unwrap();
        let plan = SceneExportPlan::new(
            &s,
            SceneExportRequest {
                world_bounds: Bounds::new(Point::ZERO, Size::new(64., 64.)),
                scale: 1.,
                pixel_ratio: 1.,
                background: Color::rgb(0, 255, 0),
                format,
            },
            CompositeLimits::default(),
        )
        .unwrap();
        s.update_object(&"r".into(), |o| o.style.grayscale = 0.)
            .unwrap();
        let result = plan
            .encode_with_inputs(
                &ImageInputs::default(),
                &TextInputs::default(),
                SceneExportPreparation::default(),
                &AtomicBool::new(false),
            )
            .unwrap();
        let data = match format {
            SceneExportFormat::Png => {
                let mut decoder = png::Decoder::new(std::io::Cursor::new(&result.bytes))
                    .read_info()
                    .unwrap();
                let mut rgba = vec![0; decoder.output_buffer_size()];
                decoder.next_frame(&mut rgba).unwrap();
                rgba
            }
            other => ::image::load_from_memory_with_format(
                &result.bytes,
                if other == SceneExportFormat::WebPLossless {
                    ::image::ImageFormat::WebP
                } else {
                    ::image::ImageFormat::Jpeg
                },
            )
            .unwrap()
            .into_rgba8()
            .into_raw(),
        };
        for (x, y, expected) in [(20, 20, [54, 54, 54, 255]), (55, 55, [0, 255, 0, 255])] {
            let actual = &data[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
            for (a, b) in actual.iter().zip(expected) {
                assert!(
                    (*a as i16 - b as i16).abs()
                        <= if matches!(format, SceneExportFormat::Jpeg { .. }) {
                            2
                        } else {
                            0
                        },
                    "{format:?}: {actual:?}"
                );
            }
        }
        assert_eq!(
            plan.encode_with_inputs(
                &ImageInputs::default(),
                &TextInputs::default(),
                SceneExportPreparation::default(),
                &AtomicBool::new(true)
            )
            .unwrap_err(),
            CompositeError::Superseded
        );
    }
}

#[test]
fn grayscale_svg_rejects_at_plan_before_resources_but_hidden_zero_and_noop_are_allowed() {
    let request = SceneExportRequest {
        world_bounds: bounds(),
        scale: 1.,
        pixel_ratio: 1.,
        background: Color::TRANSPARENT,
        format: SceneExportFormat::Svg,
    };
    for shape in [
        Shape::from(display::Image::new("unloaded.png", Size::new(20., 20.))),
        display::Text::new("missing bridge", Size::new(20., 20.)).into(),
    ] {
        let mut s = Scene::new();
        s.insert(
            DisplayObject::new("r", shape).with_style(Style {
                grayscale: 1.,
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
        assert!(
            matches!(SceneExportPlan::new(&s,request,CompositeLimits::default()),Err(CompositeError::UnsupportedShape(ref why)) if why=="SVG grayscale")
        );
        for (visible, opacity, amount) in [(false, 1., 1.), (true, 0., 1.), (true, 1., 0.)] {
            s.update_object(&"r".into(), |o| {
                o.visible = visible;
                o.style.opacity = opacity;
                o.style.grayscale = amount;
            })
            .unwrap();
            assert!(SceneExportPlan::new(&s, request, CompositeLimits::default()).is_ok());
        }
    }
    let mut s = group_scene(1.);
    insert(
        &mut s,
        DisplayObject::new("custom", display::Custom::new("test", bounds(), vec![])),
        "g",
    );
    assert!(matches!(
        scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Domain(_)
    ));
    let mut budget = PixelBudget::new(CompositeLimits::default(), 17).unwrap();
    assert_eq!(
        rasterize(
            &source(&s),
            viewport(),
            &mut budget,
            &AtomicBool::new(false)
        )
        .unwrap_err(),
        CompositeError::CustomBackendRequired("custom".into())
    );
    assert_eq!(budget.retained_bytes(), 17);
    s.update_object(&"custom".into(), |o| o.visible = false)
        .unwrap();
    assert!(matches!(
        scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Domain(_)
    ));
}

#[test]
fn grayscale_offscreen_shadow_source_still_samples_child_effect_and_zero_opacity_needs_no_producer()
{
    let mut s = group_scene(1.);
    s.update_object(&"g".into(), |o| {
        o.style.shadow = Some(Shadow::new(40., 0., 0., Color::rgb(0, 255, 0)))
    })
    .unwrap();
    let mut r = rect("r", Color::rgb(255, 0, 0));
    r.transform = Affine::translation(-30., 10.);
    r.style.grayscale = 0.5;
    insert(&mut s, r, "g");
    let f = render(&s);
    assert_eq!(pixel(&f, 15, 15), [182, 182, 182, 255]);
    let mut invisible = group_scene(1.);
    insert(
        &mut invisible,
        DisplayObject::new("text", display::Text::new("ink", Size::new(20., 20.))),
        "g",
    );
    invisible
        .update_object(&"g".into(), |o| o.style.opacity = 0.)
        .unwrap();
    assert!(
        scene_units(&invisible, CompositeLimits::default())
            .unwrap()
            .is_empty()
    );
    let mut b = PixelBudget::new(CompositeLimits::default(), 7).unwrap();
    assert!(
        rasterize(
            &source(&invisible),
            viewport(),
            &mut b,
            &AtomicBool::new(false)
        )
        .unwrap()
        .is_none()
    );
    assert_eq!((b.retained_bytes(), b.surface_writes()), (7, 0));
}

#[test]
fn grayscale_backdrop_guard_keeps_translucent_mask_and_direct_eraser_isolation_boundaries() {
    for pixel_container in [false, true] {
        let mut s = group_scene(1.);
        if pixel_container {
            s.update_object(&"g".into(), |o| o.eraser = EraserMode::Pixel)
                .unwrap();
        } else {
            // Even hidden direct erasers already force source isolation.
            let mut erase = rect("erase", Color::WHITE).with_eraser(EraserMode::Pixel);
            erase.visible = false;
            insert(&mut s, erase, "g");
        }
        let mut artistic = rect("r", Color::rgb(255, 0, 0));
        artistic.style.blend_mode = BlendMode::Multiply;
        insert(&mut s, artistic, "g");
        assert!(
            owned_canvas(
                &s,
                CanvasBackdrop {
                    color: Color::WHITE,
                    grid: None
                },
                CompositeLimits::default()
            )
            .is_ok()
        );
    }
    let mut s = group_scene(1.);
    let mut mask = rect("mask", Color::WHITE).with_mask(MaskMode::Alpha);
    mask.style.opacity = 0.5;
    insert(&mut s, mask, "g");
    let mut artistic = rect("r", Color::rgb(255, 0, 0));
    artistic.style.blend_mode = BlendMode::Multiply;
    insert(&mut s, artistic, "g");
    assert!(matches!(
        scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Domain(_)
    ));
    s.update_object(&"mask".into(), |o| o.style.opacity = 1.)
        .unwrap();
    assert!(matches!(
        scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::UnsupportedGrayscaleBackdrop(_),
            ..
        })
    ));
}

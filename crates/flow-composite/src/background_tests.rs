use super::*;
use gpui_flow_scene::{display, *};
use std::sync::{Arc, atomic::AtomicBool};

fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds {
    Bounds::new(Point::new(x, y), Size::new(w, h))
}
fn view(b: Bounds) -> RasterViewport {
    RasterViewport {
        world_to_screen: Affine::IDENTITY,
        canvas_bounds: b,
        content_mask: b,
        device_scale: 1.,
    }
}
fn rect(id: &str, b: Bounds, color: Color) -> DisplayObject {
    DisplayObject::new(id, display::Rect::new(b.size.width, b.size.height))
        .with_transform(Affine::translation(b.origin.x, b.origin.y))
        .with_style(Style::fill(color))
}
fn glass(id: &str, b: Bounds, radius: f32) -> DisplayObject {
    let mut object = rect(id, b, Color::TRANSPARENT);
    object.style.fill = None;
    object.style.background_blur = radius;
    object
}
fn insert(s: &mut Scene, o: DisplayObject, parent: Option<&str>) {
    s.insert(o, parent.map(Into::into), None).unwrap();
}
fn source(s: &Scene) -> DomainSource {
    owned_canvas(
        s,
        CanvasBackdrop {
            color: Color::WHITE,
            grid: None,
        },
        CompositeLimits::default(),
    )
    .unwrap()
}
fn draw(
    source: &DomainSource,
    v: RasterViewport,
    limits: CompositeLimits,
) -> (CpuFrame, PixelBudget) {
    let mut budget = PixelBudget::new(limits, 0).unwrap();
    let frame = rasterize(source, v, &mut budget, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    (frame, budget)
}
fn px(f: &CpuFrame, x: u32, y: u32) -> [u8; 4] {
    f.premul_rgba[((y * f.width + x) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}
fn stripes() -> Scene {
    let mut s = Scene::new();
    insert(
        &mut s,
        rect("red", bounds(-24., -24., 40., 80.), Color::rgb(255, 0, 0)),
        None,
    );
    insert(
        &mut s,
        rect("blue", bounds(16., -24., 40., 80.), Color::rgb(0, 0, 255)),
        None,
    );
    s
}
#[test]
fn background_fillless_contour_blurs_complete_prior_prefix_with_analytical_step() {
    let mut s = stripes();
    insert(&mut s, glass("glass", bounds(4., 4., 24., 24.), 1.), None);
    let (f, _) = draw(
        &source(&s),
        view(bounds(0., 0., 32., 32.)),
        CompositeLimits::default(),
    );
    // Independent normalized five-tap Gaussian at sigma=1 on an infinite step.
    let k = [
        (-2f64).exp(),
        (-0.5f64).exp(),
        1.,
        (-0.5f64).exp(),
        (-2f64).exp(),
    ];
    let sum: f64 = k.iter().sum();
    for x in 14..18 {
        let red: f64 = k
            .iter()
            .enumerate()
            .filter(|(i, _)| x as i32 + *i as i32 - 2 < 16)
            .map(|(_, w)| *w)
            .sum();
        let expected = [
            (255. * red / sum).round() as u8,
            0,
            (255. * (1. - red / sum)).round() as u8,
            255,
        ];
        assert_eq!(px(&f, x, 16), expected, "x={x}");
    }
    assert_eq!(px(&f, 15, 2), [255, 0, 0, 255]);
    // A later producer never leaks backwards into an earlier background sample.
    s.update_object(&"glass".into(), |o| o.z_index = -1)
        .unwrap();
    let (later, _) = draw(
        &source(&s),
        view(bounds(0., 0., 32., 32.)),
        CompositeLimits::default(),
    );
    assert_eq!(px(&later, 15, 16), [255, 0, 0, 255]);
}
#[test]
fn background_alpha_constant_one_tap_zero_and_opaque_foreground_are_exact() {
    let mut s = Scene::new();
    insert(&mut s, glass("glass", bounds(0., 0., 12., 12.), 1.), None);
    let mut owned = source(&s);
    owned.canvas.as_mut().unwrap().color = Color::rgba(64, 128, 32, 128);
    let (f, _) = draw(
        &owned,
        view(bounds(0., 0., 12., 12.)),
        CompositeLimits::default(),
    );
    assert!(
        f.premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [32, 64, 16, 128])
    );
    let mut s = stripes();
    insert(
        &mut s,
        glass("glass", bounds(0., 0., 32., 32.), 0.001),
        None,
    );
    let (one, _) = draw(
        &source(&s),
        view(bounds(0., 0., 32., 32.)),
        CompositeLimits::default(),
    );
    s.update_object(&"glass".into(), |o| o.style.background_blur = 0.)
        .unwrap();
    let (zero, b0) = draw(
        &source(&s),
        view(bounds(0., 0., 32., 32.)),
        CompositeLimits::default(),
    );
    assert_eq!(one.premul_rgba, zero.premul_rgba);
    s.update_object(&"glass".into(), |o| o.style.background_blur = -0.)
        .unwrap();
    let (negative, bn) = draw(
        &source(&s),
        view(bounds(0., 0., 32., 32.)),
        CompositeLimits::default(),
    );
    assert_eq!(negative.premul_rgba, zero.premul_rgba);
    assert_eq!(
        (bn.peak_bytes(), bn.surface_writes()),
        (b0.peak_bytes(), b0.surface_writes())
    );
    s.update_object(&"glass".into(), |o| {
        o.style.background_blur = 1.;
        o.style.fill = Some(Color::rgb(8, 16, 32).into());
    })
    .unwrap();
    let (opaque, _) = draw(
        &source(&s),
        view(bounds(0., 0., 32., 32.)),
        CompositeLimits::default(),
    );
    assert!(
        opaque
            .premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [8, 16, 32, 255])
    );
}
#[test]
fn background_grid_and_prior_offscreen_ink_match_generous_crop_at_every_edge_corner() {
    for scale in [1., 1.5] {
        let mut s = Scene::new();
        insert(
            &mut s,
            rect("offleft", bounds(-4., -12., 3., 40.), Color::BLACK),
            None,
        );
        insert(
            &mut s,
            rect("offright", bounds(17., -12., 3., 40.), Color::BLACK),
            None,
        );
        insert(
            &mut s,
            rect("offtop", bounds(-12., -4., 40., 3.), Color::BLACK),
            None,
        );
        insert(
            &mut s,
            rect("offbottom", bounds(-12., 17., 40., 3.), Color::BLACK),
            None,
        );
        insert(&mut s, glass("glass", bounds(-8., -8., 32., 32.), 2.), None);
        let mut owned = source(&s);
        owned.canvas.as_mut().unwrap().grid = Some(CanvasGrid {
            color: Color::rgb(240, 0, 0),
            spacing: 8.,
            offset: Point::new(1.5, 2.5),
        });
        let mut small = view(bounds(0.25, 0.25, 16., 16.));
        small.device_scale = scale;
        let mut large = view(bounds(-15.75, -15.75, 48., 48.));
        large.device_scale = scale;
        let (tight, _) = draw(&owned, small, CompositeLimits::default());
        let (wide, _) = draw(&owned, large, CompositeLimits::default());
        let offset = (16. * scale) as u32;
        for y in 0..tight.height {
            for x in 0..tight.width {
                assert_eq!(
                    px(&tight, x, y),
                    px(&wide, x + offset, y + offset),
                    "scale={scale},xy={x},{y}"
                );
            }
        }
        // Sampling outside all four visible edges changes their in-crop pixels.
        for (x, y) in [
            (0, tight.height / 2),
            (tight.width - 1, tight.height / 2),
            (tight.width / 2, 0),
            (tight.width / 2, tight.height - 1),
        ] {
            assert!(px(&tight, x, y)[1] < 255);
        }
    }
}
#[test]
fn background_sequential_offscreen_operation_is_an_actual_prior_contributor() {
    let mut s = Scene::new();
    insert(
        &mut s,
        rect("red", bounds(-20., -20., 17., 50.), Color::rgb(255, 0, 0)),
        None,
    );
    insert(
        &mut s,
        glass("earlier", bounds(-8., -8., 8., 24.), 2.),
        None,
    );
    insert(&mut s, glass("later", bounds(0., -8., 8., 24.), 2.), None);
    let (both, _) = draw(
        &source(&s),
        view(bounds(0., 0., 8., 8.)),
        CompositeLimits::default(),
    );
    let (wide, _) = draw(
        &source(&s),
        view(bounds(-12., -12., 32., 32.)),
        CompositeLimits::default(),
    );
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(px(&both, x, y), px(&wide, x + 12, y + 12));
        }
    }
    s.update_object(&"earlier".into(), |o| o.style.background_blur = 0.)
        .unwrap();
    let (single, _) = draw(
        &source(&s),
        view(bounds(0., 0., 8., 8.)),
        CompositeLimits::default(),
    );
    assert_ne!(both.premul_rgba, single.premul_rgba);
}
#[test]
fn background_internal_transparent_owner_never_borrows_canvas_or_native_siblings() {
    let mut s = Scene::new();
    insert(&mut s, glass("glass", bounds(0., 0., 12., 12.), 1.), None);
    assert_eq!(
        capture_subtree(&s, &"glass".into(), CompositeLimits::default()).unwrap_err(),
        CompositeError::UnsupportedExternalBackdrop
    );
    assert!(matches!(
        &scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::UnsupportedExternalBackdrop,
            ..
        })
    ));
    let mut internal = Scene::new();
    insert(
        &mut internal,
        DisplayObject::new("owner", display::Group::new()).with_style(Style {
            blend_mode: BlendMode::Normal,
            ..Default::default()
        }),
        None,
    );
    insert(
        &mut internal,
        glass("glass", bounds(0., 0., 12., 12.), 1.),
        Some("owner"),
    );
    let captured = capture_subtree(&internal, &"owner".into(), CompositeLimits::default()).unwrap();
    let (f, _) = draw(
        &captured,
        view(bounds(0., 0., 12., 12.)),
        CompositeLimits::default(),
    );
    assert!(f.premul_rgba.iter().all(|c| *c == 0));
    let (owned, _) = draw(
        &source(&internal),
        view(bounds(0., 0., 12., 12.)),
        CompositeLimits::default(),
    );
    assert!(
        owned
            .premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [255; 4])
    );
}
#[test]
fn background_typed_contexts_precede_missing_producers_and_path_precedence() {
    fn error(s: &Scene) -> CompositeError {
        owned_canvas(
            s,
            CanvasBackdrop {
                color: Color::WHITE,
                grid: None,
            },
            CompositeLimits::default(),
        )
        .unwrap_err()
    }
    for (kind, expected) in [
        (0, BackgroundBlurContext::TargetShape),
        (1, BackgroundBlurContext::ArtisticBlend),
        (2, BackgroundBlurContext::MaskRun),
        (3, BackgroundBlurContext::RootEraser),
        (4, BackgroundBlurContext::AncestorEffect),
        (5, BackgroundBlurContext::NestedBackground),
        (6, BackgroundBlurContext::EraserOwner),
    ] {
        let mut s = Scene::new();
        let mut parent =
            DisplayObject::new("parent", display::Frame::new(16., 16.)).with_style(Style {
                fill: None,
                ..Default::default()
            });
        if kind == 2 {
            parent.shape = display::Group::new().into();
        }
        match kind {
            4 => parent.style.blur = 1.,
            5 => parent.style.background_blur = 1.,
            6 => parent.eraser = EraserMode::Pixel,
            _ => {}
        }
        // EraserOwner is tested inside a non-root owner to avoid RootEraser precedence.
        if kind == 6 {
            insert(
                &mut s,
                DisplayObject::new("root", display::Group::new()).with_style(Style {
                    blend_mode: BlendMode::Normal,
                    ..Default::default()
                }),
                None,
            );
        }
        insert(&mut s, parent, if kind == 6 { Some("root") } else { None });
        if kind == 2 {
            let mut marker = rect("mask", bounds(0., 0., 16., 16.), Color::WHITE);
            marker.mask = MaskMode::Alpha;
            insert(&mut s, marker, Some("parent"));
        }
        if kind == 3 {
            let mut marker = rect("eraser", bounds(0., 0., 2., 2.), Color::WHITE);
            marker.eraser = EraserMode::Pixel;
            marker.visible = false;
            insert(&mut s, marker, None);
        }
        let mut target = glass("glass", bounds(2., 2., 8., 8.), 1.);
        if kind == 0 {
            target.shape = display::Text::new("missing", Size::new(16., 16.)).into();
        }
        if kind == 1 {
            target.style.blend_mode = BlendMode::Multiply;
        }
        insert(&mut s, target, Some("parent"));
        assert!(
            matches!(error(&s),CompositeError::UnsupportedBackgroundBlur{reason,..} if reason==expected),
            "kind={kind}"
        );
        if kind == 2 {
            s.update_object(&"mask".into(), |o| o.visible = false)
                .unwrap();
            // No visible marker means no mask run; the owned backdrop is valid.
            assert!(source(&s).has_background_blur());
        }
    }
    let mut s = Scene::new();
    let mut path = glass("path", bounds(0., 0., 4., 4.), 256.);
    path.eraser = EraserMode::Path;
    insert(&mut s, path, None);
    let owned = source(&s);
    assert!(!owned.has_background_blur());
    assert!(
        draw(
            &owned,
            view(bounds(0., 0., 8., 8.)),
            CompositeLimits::default()
        )
        .0
        .premul_rgba
        .iter()
        .any(|c| *c != 0)
    );
}
#[test]
fn background_exact_admission_caps_cancellation_and_retained_outputs_are_atomic() {
    let mut s = stripes();
    insert(&mut s, glass("glass", bounds(10., 10., 12., 12.), 1.), None);
    let owned = source(&s);
    let v = view(bounds(8., 8., 16., 16.));
    let (frame, measured) = draw(&owned, v, CompositeLimits::default());
    let exact = CompositeLimits {
        max_surface_bytes: measured.peak_bytes(),
        max_surface_writes: measured.surface_writes(),
        ..Default::default()
    };
    assert_eq!(draw(&owned, v, exact).0.premul_rgba, frame.premul_rgba);
    for limits in [
        CompositeLimits {
            max_surface_bytes: exact.max_surface_bytes - 1,
            ..exact
        },
        CompositeLimits {
            max_surface_writes: exact.max_surface_writes - 1,
            ..exact
        },
    ] {
        let mut budget = PixelBudget::new(limits, 0).unwrap();
        assert!(matches!(
            rasterize(&owned, v, &mut budget, &AtomicBool::new(false)),
            Err(CompositeError::WorkingSetLimit | CompositeError::WorkLimit)
        ));
        assert_eq!(budget.retained_bytes(), 0);
        assert_eq!(
            budget.surface_writes(),
            0,
            "admission must fail before pixels"
        );
    }
    let mut budget = PixelBudget::new(CompositeLimits::default(), 64).unwrap();
    assert!(matches!(
        rasterize(&owned, v, &mut budget, &AtomicBool::new(true)),
        Err(CompositeError::Superseded)
    ));
    assert_eq!(budget.retained_bytes(), 64);
}
#[test]
fn background_full_source_text_and_complete_source_compatibility() {
    let mut s = stripes();
    insert(
        &mut s,
        DisplayObject::new("text", display::Text::new("prior", Size::new(16., 16.))),
        None,
    );
    insert(&mut s, glass("glass", bounds(4., 4., 12., 12.), 1.), None);
    let old = source(&s);
    assert!(old.text_sources().iter().all(|(_, _, full)| *full));
    for mutation in [0, 1, 2] {
        let mut changed = s.clone();
        changed
            .update_object(&"red".into(), |o| match mutation {
                0 => o.style.fill = Some(Color::BLACK.into()),
                1 => o.style.opacity = 0.5,
                _ => o.z_index = 4,
            })
            .unwrap();
        assert!(!old.placement_eq(&source(&changed)));
    }
    let mut budget = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    assert!(
        matches!(rasterize(&old,view(bounds(8.,8.,4.,4.)),&mut budget,&AtomicBool::new(false)),Err(CompositeError::TextBackendRequired(id)) if id.as_str()=="text")
    );
    assert_eq!(budget.retained_bytes(), 0);
    // Host-provided glyph alpha is an actual earlier contributor, including
    // pixels outside the output. No font or placeholder producer is fabricated.
    let mut inputs = TextInputs::default();
    inputs.insert(
        "text".into(),
        Ok(Some(Arc::new(TextImage {
            image: Arc::new(
                StaticImage::from_premultiplied_rgba(
                    2,
                    12,
                    [0, 0, 0, 255].repeat(24),
                    CompositeLimits::default(),
                )
                .unwrap(),
            ),
            local_bounds: bounds(6., 2., 2., 12.),
        }))),
    );
    let mut budget = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    let f = rasterize_with_inputs(
        &old,
        &ImageInputs::default(),
        &inputs,
        view(bounds(8., 8., 4., 4.)),
        &mut budget,
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert!(px(&f, 0, 0)[0] < 255);
}

#[test]
fn background_contour_coverage_preserves_evenodd_holes_rounded_corners_and_empty_lines() {
    use display::PathCommand::*;
    let commands = vec![
        MoveTo {
            to: Point::new(4., 4.),
        },
        LineTo {
            to: Point::new(28., 4.),
        },
        LineTo {
            to: Point::new(28., 28.),
        },
        LineTo {
            to: Point::new(4., 28.),
        },
        Close,
        MoveTo {
            to: Point::new(12., 12.),
        },
        LineTo {
            to: Point::new(20., 12.),
        },
        LineTo {
            to: Point::new(20., 20.),
        },
        LineTo {
            to: Point::new(12., 20.),
        },
        Close,
    ];
    let mut s = stripes();
    let mut hole = DisplayObject::new("glass", display::Path::new(commands)).with_style(Style {
        fill: None,
        background_blur: 1.,
        fill_rule: FillRule::EvenOdd,
        ..Default::default()
    });
    insert(&mut s, hole.clone(), None);
    let (f, _) = draw(
        &source(&s),
        view(bounds(0., 0., 32., 32.)),
        CompositeLimits::default(),
    );
    assert_eq!(px(&f, 15, 16), [255, 0, 0, 255]);
    assert_ne!(px(&f, 15, 8), [255, 0, 0, 255]);
    // Path's implicit fill closure is the same contour when Close is omitted.
    if let Shape::Path(path) = &mut hole.shape {
        path.commands.retain(|c| !matches!(c, Close));
    }
    s.update_object(&"glass".into(), |o| *o = hole).unwrap();
    let (open, _) = draw(
        &source(&s),
        view(bounds(0., 0., 32., 32.)),
        CompositeLimits::default(),
    );
    assert_eq!(f.premul_rgba, open.premul_rgba);
    s.update_object(&"glass".into(), |o| {
        o.shape = display::Rect::new(24., 24.).rounded(10.).into();
        o.transform = Affine::translation(4., 4.);
    })
    .unwrap();
    let (rounded, _) = draw(
        &source(&s),
        view(bounds(0., 0., 32., 32.)),
        CompositeLimits::default(),
    );
    assert_eq!(px(&rounded, 4, 4), [255, 0, 0, 255]);
    assert_ne!(px(&rounded, 15, 16), [255, 0, 0, 255]);
    s.update_object(&"glass".into(), |o| {
        o.shape = display::Line::new(Point::new(0., 0.), Point::new(24., 24.)).into();
    })
    .unwrap();
    let (line, _) = draw(
        &source(&s),
        view(bounds(0., 0., 32., 32.)),
        CompositeLimits::default(),
    );
    s.update_object(&"glass".into(), |o| o.style.background_blur = 0.)
        .unwrap();
    assert_eq!(
        line.premul_rgba,
        draw(
            &source(&s),
            view(bounds(0., 0., 32., 32.)),
            CompositeLimits::default()
        )
        .0
        .premul_rgba
    );
}

#[test]
fn background_own_effects_only_filter_foreground_and_preserve_stroke_outside_contour() {
    let mut s = Scene::new();
    let mut target = glass("glass", bounds(4., 4., 12., 12.), 1.);
    target.style.shadow = Some(Shadow::new(2., 2., 1., Color::BLACK));
    target.style.inner_shadow_layers = vec![Shadow::new(1., 1., 1., Color::BLACK)];
    target.style.blur = 1.;
    target.style.grayscale = 1.;
    insert(&mut s, target, None);
    let mut src = source(&s);
    src.canvas.as_mut().unwrap().color = Color::rgba(64, 128, 32, 128);
    let (empty, _) = draw(
        &src,
        view(bounds(0., 0., 24., 24.)),
        CompositeLimits::default(),
    );
    assert!(
        empty
            .premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [32, 64, 16, 128]),
        "empty foreground must not cast/filter the captured backdrop"
    );
    s.update_object(&"glass".into(), |o| {
        o.style.shadow = None;
        o.style.inner_shadow_layers.clear();
        o.style.blur = 0.;
        o.style.grayscale = 0.;
        o.style.stroke = Some(Stroke::new(Color::rgb(255, 0, 0), 4.));
    })
    .unwrap();
    let (stroke, _) = draw(
        &source(&s),
        view(bounds(0., 0., 24., 24.)),
        CompositeLimits::default(),
    );
    assert_eq!(
        px(&stroke, 3, 10),
        [255, 0, 0, 255],
        "foreground outside m must survive"
    );
    let mut s = stripes();
    let mut target = glass("glass", bounds(4., 4., 24., 24.), 1.);
    target.style.fill = Some(Color::rgba(255, 0, 0, 64).into());
    target.style.grayscale = 1.;
    insert(&mut s, target, None);
    let (f, _) = draw(
        &source(&s),
        view(bounds(0., 0., 32., 32.)),
        CompositeLimits::default(),
    );
    assert_eq!(px(&f, 15, 16), [148, 14, 71, 255]);
}

#[test]
fn background_image_and_canvas_prior_snapshots_are_actual_immutable_inputs() {
    for canvas in [false, true] {
        let mut s = Scene::new();
        let shape: Shape = if canvas {
            display::Canvas::new(2., 12.).into()
        } else {
            display::Image::new("known.png", Size::new(2., 12.)).into()
        };
        insert(
            &mut s,
            DisplayObject::new("producer", shape)
                .with_transform(Affine::translation(6., 2.))
                .with_style(Style {
                    fill: None,
                    ..Default::default()
                }),
            None,
        );
        insert(&mut s, glass("glass", bounds(4., 4., 12., 12.), 1.), None);
        let src = source(&s);
        let mut images = ImageInputs::default();
        let pixels = Arc::new(
            StaticImage::from_premultiplied_rgba(
                2,
                12,
                [0, 0, 0, 255].repeat(24),
                CompositeLimits::default(),
            )
            .unwrap(),
        );
        images.insert("producer".into(), Ok(pixels.clone()));
        let before = pixels.pixmap.data().to_vec();
        let mut budget = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
        let f = rasterize_with_images(
            &src,
            &images,
            view(bounds(8., 8., 4., 4.)),
            &mut budget,
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        assert!(px(&f, 0, 0)[0] < 255, "producer canvas={canvas}");
        assert_eq!(pixels.pixmap.data(), before);
        let mut missing = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
        assert!(matches!(
            rasterize(
                &src,
                view(bounds(8., 8., 4., 4.)),
                &mut missing,
                &AtomicBool::new(false)
            ),
            Err(CompositeError::ImagePending(_))
        ));
        assert_eq!(missing.retained_bytes(), 0);
    }
}

#[test]
fn background_sparse_grid_and_nested_owners_have_exact_admission_and_bounded_work() {
    let mut s = Scene::new();
    insert(
        &mut s,
        DisplayObject::new("owner", display::Group::new()).with_style(Style {
            opacity: 0.5,
            ..Default::default()
        }),
        None,
    );
    insert(
        &mut s,
        rect("red", bounds(0., 0., 4., 8.), Color::rgb(255, 0, 0)),
        Some("owner"),
    );
    insert(
        &mut s,
        glass("glass", bounds(2., 2., 4., 4.), 1.),
        Some("owner"),
    );
    let mut src = source(&s);
    src.canvas.as_mut().unwrap().grid = Some(CanvasGrid {
        color: Color::BLACK,
        spacing: 1000.,
        offset: Point::new(500., 500.),
    });
    let v = view(bounds(0., 0., 8., 8.));
    let (_, m) = draw(&src, v, CompositeLimits::default());
    let exact = CompositeLimits {
        max_surface_bytes: m.peak_bytes(),
        max_surface_writes: m.surface_writes(),
        ..Default::default()
    };
    draw(&src, v, exact);
    for limits in [
        CompositeLimits {
            max_surface_writes: exact.max_surface_writes - 1,
            ..exact
        },
        CompositeLimits {
            max_surface_bytes: exact.max_surface_bytes - 1,
            ..exact
        },
    ] {
        let mut b = PixelBudget::new(limits, 0).unwrap();
        assert!(rasterize(&src, v, &mut b, &AtomicBool::new(false)).is_err());
        assert_eq!(b.surface_writes(), 0);
    }
    // Distant active samples are conservatively included even with a tiny output.
    insert(
        &mut s,
        glass("distant", bounds(5000., 0., 4., 4.), 1.),
        None,
    );
    let mut b = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    assert!(matches!(
        rasterize(&source(&s), v, &mut b, &AtomicBool::new(false)),
        Err(CompositeError::TooLarge)
    ));
    assert_eq!(b.retained_bytes(), 0);
}

#[test]
fn background_alpha_png_webp_exports_and_canvas_failure_are_transactional() {
    let mut s = Scene::new();
    insert(&mut s, glass("glass", bounds(-4., -4., 16., 16.), 1.), None);
    for format in [SceneExportFormat::Png, SceneExportFormat::WebPLossless] {
        let plan = SceneExportPlan::new(
            &s,
            SceneExportRequest {
                world_bounds: bounds(0., 0., 8., 8.),
                scale: 1.,
                pixel_ratio: 1.,
                background: Color::rgba(64, 128, 32, 128),
                format,
            },
            CompositeLimits::default(),
        )
        .unwrap();
        let encoded = plan
            .encode_with_inputs(
                &ImageInputs::default(),
                &TextInputs::default(),
                SceneExportPreparation::default(),
                &AtomicBool::new(false),
            )
            .unwrap();
        let decoded = if format == SceneExportFormat::Png {
            let mut decoder = png::Decoder::new(std::io::Cursor::new(&encoded.bytes))
                .read_info()
                .unwrap();
            let mut rgba = vec![0; decoder.output_buffer_size()];
            let info = decoder.next_frame(&mut rgba).unwrap();
            rgba.truncate(info.buffer_size());
            rgba
        } else {
            ::image::load_from_memory_with_format(&encoded.bytes, ::image::ImageFormat::WebP)
                .unwrap()
                .into_rgba8()
                .into_raw()
        };
        assert!(
            decoded
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [64, 128, 32, 128])
        );
    }
    assert!(
        matches!(SceneExportPlan::new(&s,SceneExportRequest{world_bounds:bounds(0.,0.,8.,8.),scale:1.,pixel_ratio:1.,background:Color::TRANSPARENT,format:SceneExportFormat::Svg},CompositeLimits::default()),Err(CompositeError::UnsupportedShape(name)) if name=="SVG background_blur")
    );
    let mut canvas =
        CanvasSurface::new(display::Canvas::new(8., 8.), CompositeLimits::default()).unwrap();
    let before = canvas.snapshot();
    let generation = canvas.generation();
    // A raw external root cannot acquire Canvas pixels through draw's existing
    // source-over contract, even if the caller already has a valid owned source.
    let mut external = source(&s);
    external.canvas = None;
    external.root = external.root.children.remove(0);
    assert_eq!(
        canvas
            .draw(
                &external,
                Affine::IDENTITY,
                &ImageInputs::default(),
                &TextInputs::default()
            )
            .unwrap_err(),
        CompositeError::UnsupportedExternalBackdrop
    );
    assert_eq!(canvas.generation(), generation);
    assert!(Arc::ptr_eq(&before, &canvas.snapshot()));
    let mut internal = Scene::new();
    insert(
        &mut internal,
        DisplayObject::new("owner", display::Group::new()).with_style(Style {
            blend_mode: BlendMode::Normal,
            ..Default::default()
        }),
        None,
    );
    insert(
        &mut internal,
        rect("red", bounds(0., 0., 4., 8.), Color::rgb(255, 0, 0)),
        Some("owner"),
    );
    insert(
        &mut internal,
        glass("glass", bounds(0., 0., 8., 8.), 1.),
        Some("owner"),
    );
    canvas
        .draw(
            &capture_subtree(&internal, &"owner".into(), CompositeLimits::default()).unwrap(),
            Affine::IDENTITY,
            &ImageInputs::default(),
            &TextInputs::default(),
        )
        .unwrap();
    assert_eq!(canvas.generation(), generation + 1);
    assert!(
        canvas
            .snapshot()
            .pixmap
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0 && p[3] < 255)
    );
}

#[test]
fn background_fractional_opacity_and_inherited_clip_mix_original_prefix_once() {
    let mut s = stripes();
    insert(
        &mut s,
        DisplayObject::new("clip", display::Frame::new(20., 20.))
            .with_transform(Affine::translation(8.25, 4.))
            .with_style(Style {
                fill: None,
                ..Default::default()
            }),
        None,
    );
    let mut target = glass("glass", bounds(-8.25, 0., 32., 24.), 1.);
    target.style.opacity = 0.5;
    insert(&mut s, target, Some("clip"));
    let (f, _) = draw(
        &source(&s),
        view(bounds(0., 0., 32., 32.)),
        CompositeLimits::default(),
    );
    // At x15 the clip is full and B/G are the independently known step bytes.
    assert_eq!(px(&f, 15, 12), [217, 0, 38, 255]);
    // Outside the inherited clip the prefix is exactly untouched.
    assert_eq!(px(&f, 7, 12), [255, 0, 0, 255]);
    // Move the AA clip boundary onto the color transition. Its 3/4 A8 coverage
    // is 192, so q=0.5*192/255; G must still sample across the write clip.
    s.update_object(&"clip".into(), |o| {
        o.transform = Affine::translation(15.25, 4.)
    })
    .unwrap();
    s.update_object(&"glass".into(), |o| {
        o.transform = Affine::translation(-15.25, 0.)
    })
    .unwrap();
    let (aa, _) = draw(
        &source(&s),
        view(bounds(0., 0., 32., 32.)),
        CompositeLimits::default(),
    );
    let q = 0.5 * 192. / 255.;
    assert_eq!(
        px(&aa, 15, 12),
        [
            ((1. - q) * 255. + q * 179f64).round() as u8,
            0,
            (q * 76f64).round() as u8,
            255
        ]
    );
}

#[test]
fn background_normal_owner_inside_eraser_owner_is_distinct_but_pixel_ancestry_refuses() {
    let mut s = Scene::new();
    insert(
        &mut s,
        DisplayObject::new("root", display::Group::new()),
        None,
    );
    insert(
        &mut s,
        DisplayObject::new("eraser_owner", display::Group::new()),
        Some("root"),
    );
    let mut marker = rect("marker", bounds(0., 0., 1., 1.), Color::WHITE);
    marker.eraser = EraserMode::Pixel;
    marker.visible = false;
    insert(&mut s, marker, Some("eraser_owner"));
    insert(
        &mut s,
        DisplayObject::new("inner", display::Group::new()).with_style(Style {
            blend_mode: BlendMode::Normal,
            ..Default::default()
        }),
        Some("eraser_owner"),
    );
    insert(
        &mut s,
        glass("glass", bounds(0., 0., 8., 8.), 1.),
        Some("inner"),
    );
    assert!(
        draw(
            &source(&s),
            view(bounds(0., 0., 8., 8.)),
            CompositeLimits::default()
        )
        .0
        .premul_rgba
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [255; 4])
    );
    s.update_object(&"eraser_owner".into(), |o| o.eraser = EraserMode::Pixel)
        .unwrap();
    assert!(matches!(
        owned_canvas(
            &s,
            CanvasBackdrop {
                color: Color::WHITE,
                grid: None
            },
            CompositeLimits::default()
        ),
        Err(CompositeError::UnsupportedBackgroundBlur {
            reason: BackgroundBlurContext::EraserOwner,
            ..
        })
    ));
}

#[test]
fn background_completed_foreground_blur_and_shadows_match_independent_final_composition() {
    fn absolute(f: &CpuFrame, x: u32, y: u32) -> [u8; 4] {
        let x = x as i32 - f.screen_bounds.origin.x as i32;
        let y = y as i32 - f.screen_bounds.origin.y as i32;
        if x < 0 || y < 0 || x >= f.width as i32 || y >= f.height as i32 {
            [0; 4]
        } else {
            px(f, x as u32, y as u32)
        }
    }
    let viewport = view(bounds(0., 0., 32., 32.));
    let base = stripes();
    let (b, _) = draw(&source(&base), viewport, CompositeLimits::default());
    let contour = bounds(10., 10., 12., 12.);
    let mut background_only = base.clone();
    insert(&mut background_only, glass("glass", contour, 1.), None);
    let (b1, _) = draw(
        &source(&background_only),
        viewport,
        CompositeLimits::default(),
    );
    let mut foreground = glass("glass", contour, 0.);
    foreground.style.fill = Some(Color::rgba(255, 0, 0, 64).into());
    foreground.style.stroke = Some(Stroke::new(Color::BLACK, 2.));
    foreground.style.shadow = Some(Shadow::new(3., 2., 0.5, Color::rgba(0, 0, 255, 128)));
    foreground.style.inner_shadow_layers =
        vec![Shadow::new(1., 1., 0.5, Color::rgba(0, 255, 0, 128))];
    foreground.style.blur = 0.75;
    foreground.style.grayscale = 0.5;
    let mut fs = Scene::new();
    insert(&mut fs, foreground.clone(), None);
    let (f, _) = draw(
        &capture_subtree(&fs, &"glass".into(), CompositeLimits::default()).unwrap(),
        viewport,
        CompositeLimits::default(),
    );
    foreground.style.background_blur = 1.;
    foreground.style.opacity = 0.6;
    let mut scene = base;
    insert(&mut scene, foreground, None);
    let (actual, measured) = draw(&source(&scene), viewport, CompositeLimits::default());
    let q = f64::from(0.6f32);
    for y in 0..32 {
        for x in 0..32 {
            let backdrop = px(&b, x, y);
            let filtered = px(&b1, x, y);
            let foreground = absolute(&f, x, y);
            let mut expected = [0; 4];
            for c in 0..4 {
                let r = f64::from(foreground[c])
                    + (1. - f64::from(foreground[3]) / 255.) * f64::from(filtered[c]);
                expected[c] = ((1. - q) * f64::from(backdrop[c]) + q * r)
                    .round()
                    .clamp(0., 255.) as u8;
            }
            for c in 0..3 {
                expected[c] = expected[c].min(expected[3]);
            }
            assert_eq!(px(&actual, x, y), expected, "pixel={x},{y}");
        }
    }
    // The same exact lifetime accounting also covers F's recursive effects.
    let limits = CompositeLimits {
        max_surface_bytes: measured.peak_bytes(),
        max_surface_writes: measured.surface_writes(),
        ..Default::default()
    };
    assert_eq!(
        draw(&source(&scene), viewport, limits).0.premul_rgba,
        actual.premul_rgba
    );
}

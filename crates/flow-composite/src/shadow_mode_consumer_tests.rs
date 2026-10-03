use super::*;
use gpui_flow_scene::{display, *};
use std::sync::{Arc, atomic::AtomicBool};

fn effect(spread: f32, knockout: bool, fixed: bool) -> Shadow {
    Shadow {
        spread,
        r#box: knockout,
        scale_fixed: fixed,
        ..Shadow::new(14., 3., 0., Color::rgb(255, 0, 0))
    }
}

fn source(scene: &Scene, id: &str) -> DomainSource {
    capture_subtree(scene, &id.into(), CompositeLimits::default()).unwrap()
}

fn frame_pixels(frame: &CpuFrame, size: u32, dpr: f32) -> Vec<u8> {
    let mut pixels = vec![0; (size * size * 4) as usize];
    let x0 = (frame.screen_bounds.origin.x * dpr).round() as i32;
    let y0 = (frame.screen_bounds.origin.y * dpr).round() as i32;
    for y in 0..frame.height {
        for x in 0..frame.width {
            let (dx, dy) = (x0 + x as i32, y0 + y as i32);
            if dx >= 0 && dy >= 0 && dx < size as i32 && dy < size as i32 {
                let from = ((y * frame.width + x) * 4) as usize;
                let to = ((dy as u32 * size + dx as u32) * 4) as usize;
                pixels[to..to + 4].copy_from_slice(&frame.premul_rgba[from..from + 4]);
            }
        }
    }
    pixels
}

fn viewport(dpr: f32) -> RasterViewport {
    let bounds = Bounds::new(Point::ZERO, Size::new(96., 96.));
    RasterViewport {
        world_to_screen: Affine::IDENTITY,
        canvas_bounds: bounds,
        content_mask: bounds,
        device_scale: dpr,
    }
}

fn canvas_scene(inner: bool) -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("ancestor", display::Group::new())
                .with_transform(Affine::translation(160., -80.) * Affine::scale(2., 2.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("owner", display::Group::new())
                .with_transform(
                    Affine::translation(30., 20.) * Affine::new(0., 1., -1., 0., 0., 0.),
                )
                .with_style(Style {
                    blend_mode: BlendMode::Normal,
                    shadow: (!inner).then(|| effect(3., true, true)),
                    inner_shadow_layers: if inner {
                        vec![effect(3., true, true)]
                    } else {
                        vec![]
                    },
                    ..Default::default()
                }),
            Some("ancestor".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("ink", display::Rect::new(12., 6.))
                .with_transform(Affine::translation(4., 2.))
                .with_style(Style::fill(Color::rgba(0, 0, 255, 128))),
            Some("owner".into()),
            None,
        )
        .unwrap();
    scene
}

#[test]
fn canvas_shadow_modes_keep_logical_affine_separate_from_dpr_and_remove_root_pose() {
    for inner in [false, true] {
        for zoom in [0.5, 1., 2.] {
            for dpr in [1., 2.] {
                for rotation in [Affine::IDENTITY, Affine::new(0., 1., -1., 0., 0., 0.)] {
                    let original = canvas_scene(inner);
                    let frozen = source(&original, "owner");
                    let local =
                        Affine::translation(48., 32.) * rotation * Affine::scale(zoom, zoom);
                    let mut config = display::Canvas::new(96., 96.);
                    config.pixel_ratio = dpr;
                    let mut canvas =
                        CanvasSurface::new(config, CompositeLimits::default()).unwrap();
                    let before = canvas.snapshot();
                    let generation = canvas.generation();
                    canvas
                        .draw(
                            &frozen,
                            local,
                            &ImageInputs::default(),
                            &TextInputs::default(),
                        )
                        .unwrap();
                    assert_eq!(canvas.generation(), generation + 1);
                    assert!(before.premultiplied_rgba().iter().all(|v| *v == 0));
                    let mut expected_scene = original.clone();
                    expected_scene
                        .update_object(&"ancestor".into(), |o| o.transform = Affine::IDENTITY)
                        .unwrap();
                    expected_scene
                        .update_object(&"owner".into(), |o| o.transform = local)
                        .unwrap();
                    let expected = rasterize(
                        &source(&expected_scene, "owner"),
                        viewport(dpr),
                        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
                        &AtomicBool::new(false),
                    )
                    .unwrap()
                    .unwrap();
                    let expected = frame_pixels(&expected, (96. * dpr) as u32, dpr);
                    assert!(expected.iter().any(|v| *v > 0));
                    assert_eq!(
                        canvas.snapshot().premultiplied_rgba(),
                        expected,
                        "inner={inner} zoom={zoom} dpr={dpr} rotation={rotation:?}"
                    );
                    assert_eq!(frozen, source(&original, "owner"));
                }
            }
        }
    }
}

#[test]
fn canvas_mode_admission_failure_keeps_snapshot_generation_and_inputs() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("owner", display::Image::new("ready", Size::new(8., 8.)))
                .with_style(Style {
                    shadow: Some(effect(3., true, true)),
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
    let image = Arc::new(
        StaticImage::from_premultiplied_rgba(
            8,
            8,
            [0, 0, 128, 128].repeat(64),
            CompositeLimits::default(),
        )
        .unwrap(),
    );
    let input_bytes = image.premultiplied_rgba().to_vec();
    let mut inputs = ImageInputs::default();
    inputs.insert("owner".into(), Ok(image.clone()));
    let mut canvas =
        CanvasSurface::new(display::Canvas::new(96., 96.), CompositeLimits::default()).unwrap();
    canvas
        .draw(
            &source(&scene, "owner"),
            Affine::translation(20., 20.),
            &inputs,
            &TextInputs::default(),
        )
        .unwrap();
    let before = canvas.snapshot();
    let generation = canvas.generation();
    scene
        .update_object(&"owner".into(), |o| {
            o.style.shadow_layers = vec![effect(4096., true, true)]
        })
        .unwrap();
    assert!(matches!(
        canvas.draw(
            &source(&scene, "owner"),
            Affine::IDENTITY,
            &inputs,
            &TextInputs::default()
        ),
        Err(CompositeError::TooLarge)
    ));
    assert!(Arc::ptr_eq(&before, &canvas.snapshot()));
    assert_eq!(canvas.generation(), generation);
    assert_eq!(image.premultiplied_rgba(), input_bytes);
    scene
        .update_object(&"owner".into(), |o| o.style.shadow_layers.clear())
        .unwrap();
    assert!(
        canvas
            .draw(
                &source(&scene, "owner"),
                Affine::IDENTITY,
                &ImageInputs::default(),
                &TextInputs::default()
            )
            .is_err()
    );
    assert!(Arc::ptr_eq(&before, &canvas.snapshot()));
    assert_eq!(canvas.generation(), generation);

    // Canvas copy admission succeeds; the complete mode source/filter passes
    // exceed the remaining work budget before publishing a candidate snapshot.
    let mut limited = CanvasSurface::new(
        display::Canvas::new(96., 96.),
        CompositeLimits {
            max_surface_writes: 96 * 96 * 3 + 1,
            ..Default::default()
        },
    )
    .unwrap();
    let before = limited.snapshot();
    let generation = limited.generation();
    assert_eq!(
        limited
            .draw(
                &source(&scene, "owner"),
                Affine::translation(20., 20.),
                &inputs,
                &TextInputs::default()
            )
            .unwrap_err(),
        CompositeError::WorkLimit
    );
    assert!(Arc::ptr_eq(&before, &limited.snapshot()));
    assert_eq!(limited.generation(), generation);
    assert_eq!(image.premultiplied_rgba(), input_bytes);
}

#[test]
fn shadow_modes_keep_strict_svg_and_interchange_refusal_before_producers() {
    for mode in [
        effect(3., false, false),
        effect(0., true, false),
        effect(0., false, true),
    ] {
        for inner in [false, true] {
            let mut scene = Scene::new();
            scene
                .insert(
                    DisplayObject::new(
                        "missing",
                        display::Image::new("not-loaded", Size::new(8., 8.)),
                    )
                    .with_style(Style {
                        shadow: (!inner).then_some(mode),
                        inner_shadow_layers: if inner { vec![mode] } else { vec![] },
                        ..Default::default()
                    }),
                    None,
                    None,
                )
                .unwrap();
            let request = SceneExportRequest {
                world_bounds: Bounds::new(Point::ZERO, Size::new(96., 96.)),
                scale: 1.,
                pixel_ratio: 2.,
                background: Color::TRANSPARENT,
                format: SceneExportFormat::Svg,
            };
            let error =
                SceneExportPlan::new(&scene, request, CompositeLimits::default()).unwrap_err();
            assert_eq!(
                error,
                if inner {
                    CompositeError::UnsupportedInnerShadow("missing".into())
                } else {
                    CompositeError::UnsupportedShadow("missing".into())
                }
            );
            assert!(scene.to_leafer_json().is_err());
            scene
                .update_object(&"missing".into(), |o| o.visible = false)
                .unwrap();
            assert!(SceneExportPlan::new(&scene, request, CompositeLimits::default()).is_ok());
            assert!(
                scene.to_leafer_json().is_err(),
                "interchange may not discard dormant modes"
            );
        }
    }
    for name in ["shadow", "innerShadow"] {
        let input = serde_json::json!({"tag":"Rect", "id":"r", "width":8, "height":8,
            name: {"x":1,"y":2,"blur":0,"color":"#000000","spread":3,"box":true,"scaleFixed":true}});
        assert!(Scene::from_leafer_json(&input.to_string()).is_err());
    }
}

#[test]
fn every_nondefault_mode_stays_in_cpu_domain_without_native_fallback() {
    for mode in [
        effect(3., false, false),
        effect(0., true, false),
        effect(0., false, true),
    ] {
        for inner in [false, true] {
            let mut scene = Scene::new();
            scene
                .insert(
                    DisplayObject::new("r", display::Rect::new(8., 8.)).with_style(Style {
                        fill: Some(Color::WHITE.into()),
                        shadow: (!inner).then_some(mode),
                        inner_shadow_layers: if inner { vec![mode] } else { vec![] },
                        ..Default::default()
                    }),
                    None,
                    None,
                )
                .unwrap();
            let units = scene_units(&scene, CompositeLimits::default()).unwrap();
            assert!(
                matches!(units.as_slice(), [SceneUnit::Domain(domain)] if domain.root_id() == &DisplayId::from("r"))
            );
            let mut cfg = display::Canvas::new(96., 96.);
            cfg.pixel_ratio = 2.;
            let mut canvas = CanvasSurface::new(cfg, CompositeLimits::default()).unwrap();
            canvas
                .draw(
                    &source(&scene, "r"),
                    Affine::translation(30., 30.),
                    &ImageInputs::default(),
                    &TextInputs::default(),
                )
                .unwrap();
            assert!(
                canvas
                    .snapshot()
                    .premultiplied_rgba()
                    .iter()
                    .any(|v| *v > 0)
            );
        }
    }
}

#[test]
fn complete_text_alpha_can_project_into_view_and_missing_host_rolls_back_canvas() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("owner", display::Group::new()).with_style(Style {
                shadow: Some(Shadow {
                    x: 30.,
                    y: 0.,
                    ..effect(3., true, true)
                }),
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new(
                "text",
                display::Text::new("full source", Size::new(20., 10.)),
            )
            .with_transform(Affine::translation(-20., 20.)),
            Some("owner".into()),
            None,
        )
        .unwrap();
    let frozen = source(&scene, "owner");
    let text_sources = frozen.text_sources();
    assert_eq!(text_sources.len(), 1);
    assert_eq!(text_sources[0].0.id, DisplayId::from("text"));
    assert!(
        text_sources[0].2,
        "ancestor mode requires complete alignment-box alpha"
    );
    let mut canvas =
        CanvasSurface::new(display::Canvas::new(96., 96.), CompositeLimits::default()).unwrap();
    let before = canvas.snapshot();
    let generation = canvas.generation();
    assert_eq!(
        canvas
            .draw(
                &frozen,
                Affine::IDENTITY,
                &ImageInputs::default(),
                &TextInputs::default()
            )
            .unwrap_err(),
        CompositeError::TextBackendRequired("text".into())
    );
    assert!(Arc::ptr_eq(&before, &canvas.snapshot()));
    assert_eq!(canvas.generation(), generation);
    let mut rgba = vec![0; 20 * 10 * 4];
    for y in 2..8 {
        for x in 2..6 {
            rgba[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4].copy_from_slice(&[0, 0, 128, 128]);
        }
    }
    let image = Arc::new(
        StaticImage::from_premultiplied_rgba(20, 10, rgba.clone(), CompositeLimits::default())
            .unwrap(),
    );
    let mut texts = TextInputs::default();
    texts.insert(
        "text".into(),
        Ok(Some(Arc::new(TextImage {
            image: image.clone(),
            local_bounds: Bounds::new(Point::ZERO, Size::new(20., 10.)),
        }))),
    );
    canvas
        .draw(&frozen, Affine::IDENTITY, &ImageInputs::default(), &texts)
        .unwrap();
    let output = canvas.snapshot();
    assert!(
        output
            .premultiplied_rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0] > 0)
    );
    assert!(
        output
            .premultiplied_rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[2] == 0),
        "original glyphs remain wholly outside the viewport"
    );
    assert_eq!(image.premultiplied_rgba(), rgba);
    assert_eq!(canvas.generation(), generation + 1);
}

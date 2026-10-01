use super::*;
use gpui_flow_scene::{display, *};
use std::sync::{Arc, atomic::AtomicBool};

fn red(x: f32, blur: f32) -> Shadow {
    Shadow::new(x, 0., blur, Color::rgb(255, 0, 0))
}
fn insert(s: &mut Scene, id: &str, parent: Option<&str>, shape: impl Into<Shape>, style: Style) {
    s.insert(
        DisplayObject::new(id, shape).with_style(style),
        parent.map(Into::into),
        None,
    )
    .unwrap();
}
fn group(s: &mut Scene, id: &str, parent: Option<&str>, layers: Vec<Shadow>) {
    insert(
        s,
        id,
        parent,
        display::Group::new(),
        Style {
            inner_shadow_layers: layers,
            ..Default::default()
        },
    );
}
fn rect(s: &mut Scene, id: &str, parent: &str, x: f32, y: f32, color: Color) {
    insert(
        s,
        id,
        Some(parent),
        display::Rect::new(10., 10.),
        Style::fill(color),
    );
    s.update_object(&id.into(), |o| o.transform = Affine::translation(x, y))
        .unwrap();
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
    let bounds = Bounds::new(Point::ZERO, Size::new(150., 150.));
    RasterViewport {
        world_to_screen: Affine::IDENTITY,
        canvas_bounds: bounds,
        content_mask: bounds,
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
fn container(frame: bool, late: bool) -> Shape {
    let order = if late {
        ContainerPaintOrder::StrokeAfterChildren
    } else {
        ContainerPaintOrder::BeforeChildren
    };
    if frame {
        display::Frame::new(30., 30.).with_paint_order(order).into()
    } else {
        display::Box::new(30., 30.).with_paint_order(order).into()
    }
}

#[test]
fn group_inner_samples_sparse_child_results_and_own_outer_stays_independent() {
    let mut s = Scene::new();
    group(&mut s, "g", None, vec![red(0., 0.)]);
    rect(&mut s, "r", "g", 20., 20., Color::rgba(0, 0, 255, 128));
    rect(&mut s, "other", "g", 20., 45., Color::rgb(0, 0, 255));
    s.update_object(&"r".into(), |o| {
        o.style.inner_shadow_layers = vec![Shadow::new(0., 0., 0., Color::rgb(0, 255, 0))];
        o.style.shadow = Some(Shadow::new(15., 0., 0., Color::BLACK));
    })
    .unwrap();
    s.update_object(&"g".into(), |o| {
        o.style.shadow = Some(Shadow::new(70., 0., 0., Color::BLACK))
    })
    .unwrap();
    let f = render(&s);
    // Child final alpha is 160. Parent inner independently derives 60 from it.
    assert_eq!(pixel(&f, 25., 25.), [60, 49, 73, 182]);
    assert_eq!(pixel(&f, 40., 25.), [64, 0, 0, 160]); // child's outer alpha is a real producer
    assert_eq!(pixel(&f, 95., 25.), [0, 0, 0, 160]); // includes child inner, excludes own inner
    assert_eq!(pixel(&f, 110., 25.), [0, 0, 0, 128]);
    assert_eq!(pixel(&f, 25., 38.), [0; 4]);
    assert_eq!(pixel(&f, 95., 38.), [0; 4]);
}

#[test]
fn before_children_and_late_stroke_have_distinct_inner_insertion_points() {
    for frame in [false, true] {
        for late in [false, true] {
            let mut s = Scene::new();
            insert(
                &mut s,
                "c",
                None,
                container(frame, late),
                Style {
                    fill: None,
                    inner_shadow_layers: vec![red(0., 0.)],
                    ..Default::default()
                },
            );
            rect(&mut s, "r", "c", 10., 10., Color::rgba(0, 0, 255, 128));
            let f = render(&s);
            // Both sample child A=128, even before the child has been painted.
            assert_eq!(
                pixel(&f, 15., 15.),
                if late {
                    [64, 0, 96, 160]
                } else {
                    [32, 0, 128, 160]
                }
            );
            s.update_object(&"r".into(), |o| {
                o.style.fill = Some(Color::rgb(0, 0, 255).into())
            })
            .unwrap();
            s.update_object(&"c".into(), |o| {
                o.style.inner_shadow_layers = vec![red(50., 0.)]
            })
            .unwrap();
            assert_eq!(
                pixel(&render(&s), 15., 15.),
                if late {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                }
            );
        }
    }
}

#[test]
fn inner_never_moves_strokes_across_children_or_cuts_outside_strokes() {
    for frame in [false, true] {
        for late in [false, true] {
            let mut s = Scene::new();
            insert(
                &mut s,
                "c",
                None,
                container(frame, late),
                Style {
                    fill: Some(Color::rgb(0, 0, 255).into()),
                    stroke: Some(
                        Stroke::new(Color::rgb(0, 255, 0), 4.)
                            .with_alignment(StrokeAlignment::Outside),
                    ),
                    inner_shadow_layers: vec![red(60., 0.)],
                    ..Default::default()
                },
            );
            s.update_object(&"c".into(), |o| o.transform = Affine::translation(20., 20.))
                .unwrap();
            rect(&mut s, "r", "c", 27., 10., Color::WHITE);
            let f = render(&s);
            assert_eq!(pixel(&f, 18., 35.), [0, 255, 0, 255]);
            assert_eq!(pixel(&f, 25., 35.), [255, 0, 0, 255]);
            assert_eq!(
                pixel(&f, 51., 35.),
                if frame || late {
                    [0, 255, 0, 255]
                } else {
                    [255; 4]
                }
            );
            assert_eq!(
                pixel(&f, 55., 35.),
                if frame {
                    [0; 4]
                } else if late {
                    [255, 0, 0, 255]
                } else {
                    [255; 4]
                }
            );
        }
    }
}

#[test]
fn whole_group_opacity_is_after_inner_layers_and_child_composition() {
    let mut s = Scene::new();
    group(
        &mut s,
        "g",
        None,
        vec![red(0., 0.), Shadow::new(0., 0., 0., Color::rgb(0, 255, 0))],
    );
    rect(&mut s, "r", "g", 20., 20., Color::rgba(0, 0, 255, 128));
    let f = render(&s);
    assert_eq!(pixel(&f, 25., 25.), [48, 64, 72, 184]);
    s.update_object(&"g".into(), |o| o.style.opacity = 0.5)
        .unwrap();
    assert_eq!(pixel(&render(&s), 25., 25.), [24, 32, 36, 92]);
}

#[test]
fn own_child_clip_changes_source_but_ancestor_clip_does_not_create_inner_edge() {
    let mut s = Scene::new();
    insert(
        &mut s,
        "c",
        None,
        container(true, true),
        Style {
            fill: None,
            inner_shadow_layers: vec![red(4., 0.)],
            ..Default::default()
        },
    );
    s.update_object(&"c".into(), |o| o.transform = Affine::translation(20., 20.))
        .unwrap();
    insert(
        &mut s,
        "r",
        Some("c"),
        display::Rect::new(60., 50.),
        Style::fill(Color::rgb(0, 0, 255)),
    );
    s.update_object(&"r".into(), |o| {
        o.transform = Affine::translation(-10., -10.)
    })
    .unwrap();
    let f = render(&s);
    assert_eq!(pixel(&f, 21., 35.), [255, 0, 0, 255]);
    assert_eq!(pixel(&f, 26., 35.), [0, 0, 255, 255]);
    assert_eq!(pixel(&f, 51., 35.), [0; 4]);
    s.update_object(&"c".into(), |o| {
        o.style.inner_shadow_layers.clear();
        o.style.blend_mode = BlendMode::Normal;
    })
    .unwrap();
    group(&mut s, "g", Some("c"), vec![red(4., 0.)]);
    s.reparent(&"r".into(), Some("g".into()), None, true)
        .unwrap();
    assert_eq!(pixel(&render(&s), 21., 35.), [0, 0, 255, 255]);
}

#[test]
fn inherited_rounded_clip_is_applied_once_after_group_inner_at_fractional_dpr() {
    let mut s = Scene::new();
    insert(
        &mut s,
        "f",
        None,
        display::Frame::new(20., 20.).rounded(8.),
        Style {
            fill: None,
            blend_mode: BlendMode::Normal,
            ..Default::default()
        },
    );
    s.update_object(&"f".into(), |o| {
        o.transform = Affine::translation(20.25, 20.25)
    })
    .unwrap();
    group(&mut s, "g", Some("f"), vec![red(100., 0.)]);
    insert(
        &mut s,
        "r",
        Some("g"),
        display::Rect::new(50., 50.),
        Style::fill(Color::WHITE),
    );
    s.update_object(&"r".into(), |o| {
        o.transform = Affine::translation(-10., -10.)
    })
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
    s.update_object(&"g".into(), |o| o.style.inner_shadow_layers.clear())
        .unwrap();
    s.update_object(&"r".into(), |o| {
        o.style.fill = Some(Color::rgb(255, 0, 0).into())
    })
    .unwrap();
    let expected = render(&s);
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
fn container_inner_uses_masked_result_and_as_mask_exposes_final_alpha() {
    let mut s = Scene::new();
    group(&mut s, "g", None, vec![red(100., 0.)]);
    rect(&mut s, "mask", "g", 20., 20., Color::WHITE);
    s.update_object(&"mask".into(), |o| o.mask = MaskMode::Alpha)
        .unwrap();
    rect(&mut s, "r", "g", 25., 20., Color::rgb(0, 0, 255));
    let f = render(&s);
    assert_eq!(pixel(&f, 22., 25.), [0; 4]);
    assert_eq!(pixel(&f, 27., 25.), [255, 0, 0, 255]);
    assert_eq!(pixel(&f, 32., 25.), [0; 4]);
    let mut s = Scene::new();
    group(&mut s, "root", None, vec![]);
    group(&mut s, "mask", Some("root"), vec![red(0., 0.)]);
    s.update_object(&"mask".into(), |o| o.mask = MaskMode::Alpha)
        .unwrap();
    rect(&mut s, "m", "mask", 20., 20., Color::rgba(0, 0, 255, 128));
    rect(&mut s, "r", "root", 20., 20., Color::WHITE);
    assert_eq!(pixel(&render(&s), 25., 25.), [160; 4]);
}

fn blend_scene() -> Scene {
    let mut s = Scene::new();
    group(&mut s, "root", None, vec![]);
    s.update_object(&"root".into(), |o| o.style.blend_mode = BlendMode::Normal)
        .unwrap();
    rect(&mut s, "back", "root", 20., 20., Color::WHITE);
    group(&mut s, "g", Some("root"), vec![red(50., 0.)]);
    group(&mut s, "nested", Some("g"), vec![]);
    rect(&mut s, "r", "nested", 20., 20., Color::rgb(0, 0, 255));
    s.update_object(&"r".into(), |o| o.style.blend_mode = BlendMode::Multiply)
        .unwrap();
    s
}
#[test]
fn visible_inner_guard_rejects_unsafe_pass_through_even_inside_owned_canvas() {
    let s = blend_scene();
    assert!(
        matches!(&scene_units(&s, CompositeLimits::default()).unwrap()[0], SceneUnit::Rejected(CompositeIssue {error: CompositeError::UnsupportedShadowBackdrop(id), ..}) if id.as_str() == "g")
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
        CompositeError::UnsupportedShadowBackdrop("g".into())
    );
    for target in ["g", "nested"] {
        let mut s = s.clone();
        s.update_object(&target.into(), |o| o.style.blend_mode = BlendMode::Normal)
            .unwrap();
        assert_eq!(pixel(&render(&s), 25., 25.), [255, 0, 0, 255]);
    }
    let mut s = s;
    s.update_object(&"g".into(), |o| o.style.opacity = 0.5)
        .unwrap();
    assert_eq!(pixel(&render(&s), 25., 25.), [255, 128, 128, 255]);
}

#[test]
fn transparent_inner_does_not_add_isolation_and_mask_guard_remains_explicit() {
    let mut s = blend_scene();
    s.update_object(&"g".into(), |o| {
        o.style.inner_shadow_layers[0] = Shadow::new(4096., 0., 256., Color::TRANSPARENT)
    })
    .unwrap();
    let transparent = render(&s);
    assert_eq!(pixel(&transparent, 25., 25.), [0, 0, 255, 255]);
    s.update_object(&"g".into(), |o| {
        o.style.inner_shadow_layers = vec![red(50., 0.)]
    })
    .unwrap();
    s.insert(
        DisplayObject::new("mask", display::Rect::new(60., 60.)).with_mask(MaskMode::Alpha),
        Some("g".into()),
        Some(0),
    )
    .unwrap();
    assert!(matches!(
        &scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::UnsupportedShadowBackdrop(_),
            ..
        })
    ));
    s.update_object(&"mask".into(), |o| o.style.opacity = 0.5)
        .unwrap();
    assert!(matches!(
        &scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Domain(_)
    ));
    assert!(pixel(&render(&s), 25., 25.)[0] > 0);
}

#[test]
fn artistic_container_needs_owned_backdrop_and_blends_final_inner_result_once() {
    let mut s = Scene::new();
    group(&mut s, "g", None, vec![red(50., 0.)]);
    rect(&mut s, "r", "g", 20., 20., Color::WHITE);
    s.update_object(&"g".into(), |o| o.style.blend_mode = BlendMode::Multiply)
        .unwrap();
    assert!(matches!(
        &scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::UnsupportedExternalBackdrop,
            ..
        })
    ));
    let src = owned_canvas(
        &s,
        CanvasBackdrop {
            color: Color::rgb(128, 128, 128),
            grid: None,
        },
        CompositeLimits::default(),
    )
    .unwrap();
    let f = rasterize(
        &src,
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(pixel(&f, 25., 25.), [128, 0, 0, 255]);
}

#[test]
fn descendant_image_and_text_holes_and_full_offscreen_sources_are_preserved() {
    for text in [false, true] {
        let mut s = Scene::new();
        group(&mut s, "g", None, vec![red(4., 0.)]);
        let shape: Shape = if text {
            display::Text::new("ink", Size::new(20., 10.)).into()
        } else {
            display::Image::new("ink.png", Size::new(20., 10.)).into()
        };
        insert(&mut s, "ink", Some("g"), shape, Style::default());
        s.update_object(&"ink".into(), |o| {
            o.transform = Affine::translation(-10., 20.)
        })
        .unwrap();
        let src = source(&s);
        if text {
            assert!(src.text_sources()[0].2);
        }
        let mut rgba = [0, 0, 255, 255].repeat(200);
        for y in 3..7 {
            for x in 15..18 {
                rgba[(y * 20 + x) * 4..][..4].fill(0);
            }
        }
        let image = Arc::new(
            StaticImage::from_premultiplied_rgba(20, 10, rgba, CompositeLimits::default()).unwrap(),
        );
        let mut images = ImageInputs::default();
        let mut texts = TextInputs::default();
        if text {
            texts.insert(
                "ink".into(),
                Ok(Some(Arc::new(TextImage {
                    image,
                    local_bounds: Bounds::new(Point::ZERO, Size::new(20., 10.)),
                }))),
            );
        } else {
            images.insert("ink".into(), Ok(image));
        }
        let f = rasterize_with_inputs(
            &src,
            &images,
            &texts,
            viewport(),
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        assert_eq!(pixel(&f, 1., 25.), [0, 0, 255, 255]); // no false viewport edge
        assert_eq!(pixel(&f, 6., 25.), [0; 4]);
        assert!(
            rasterize(
                &src,
                viewport(),
                &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
}

#[test]
fn empty_container_never_synthesizes_rectangular_alpha() {
    for shape in [
        Shape::from(display::Group::new()),
        container(false, false),
        container(true, true),
    ] {
        let mut s = Scene::new();
        insert(
            &mut s,
            "empty",
            None,
            shape,
            Style {
                fill: None,
                inner_shadow_layers: vec![red(4., 2.)],
                ..Default::default()
            },
        );
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
}

#[test]
fn nested_inner_replays_and_late_invalid_layer_reject_before_domain_allocation() {
    let mut s = Scene::new();
    for n in 0..7 {
        group(
            &mut s,
            &format!("g{n}"),
            (n > 0).then(|| format!("g{}", n - 1)).as_deref(),
            vec![red(0., 0.); 8],
        );
    }
    rect(&mut s, "r", "g6", 20., 20., Color::WHITE);
    let mut b = PixelBudget::new(CompositeLimits::default(), 7).unwrap();
    assert!(matches!(
        rasterize(&source(&s), viewport(), &mut b, &AtomicBool::new(false)),
        Err(CompositeError::WorkLimit | CompositeError::WorkingSetLimit)
    ));
    assert_eq!(
        (b.peak_bytes(), b.retained_bytes(), b.surface_writes()),
        (7, 7, 0)
    );
    let mut s = Scene::new();
    group(&mut s, "g", None, vec![red(0., 0.), red(0., 256.)]);
    rect(&mut s, "r", "g", 20., 20., Color::WHITE);
    assert_eq!(
        rasterize(&source(&s), viewport(), &mut b, &AtomicBool::new(false)).unwrap_err(),
        CompositeError::TooLarge
    );
    assert_eq!(
        (b.peak_bytes(), b.retained_bytes(), b.surface_writes()),
        (7, 7, 0)
    );
    s.update_object(&"g".into(), |o| {
        o.style.inner_shadow_layers = vec![red(1., 0.)];
        o.transform = Affine::scale(2., 1.);
    })
    .unwrap();
    assert_eq!(
        rasterize(&source(&s), viewport(), &mut b, &AtomicBool::new(false)).unwrap_err(),
        CompositeError::UnsupportedTransform
    );
    assert_eq!(b.peak_bytes(), 7);
}

#[test]
fn inner_outer_mask_clip_phase_matrix_matches_exact_recursive_admission() {
    for frame in [false, true] {
        for late in [false, true] {
            for translucent in [false, true] {
                let mut s = Scene::new();
                group(&mut s, "root", None, vec![red(2., 1.)]);
                s.update_object(&"root".into(), |o| {
                    o.style.blend_mode = BlendMode::Normal;
                    o.style.shadow = Some(Shadow::new(35., 0., 0., Color::BLACK));
                })
                .unwrap();
                insert(
                    &mut s,
                    "c",
                    Some("root"),
                    container(frame, late),
                    Style {
                        fill: Some(Color::rgba(30, 50, 100, 100).into()),
                        stroke: Some(Stroke::new(Color::WHITE, 3.)),
                        blend_mode: BlendMode::Normal,
                        inner_shadow_layers: vec![red(2., 0.), red(-1., 1.)],
                        shadow: Some(Shadow::new(3., 0., 0., Color::BLACK)),
                        ..Default::default()
                    },
                );
                s.update_object(&"c".into(), |o| {
                    o.transform = Affine::translation(10.25, 10.5)
                })
                .unwrap();
                group(&mut s, "masked", Some("c"), vec![]);
                insert(
                    &mut s,
                    "mask",
                    Some("masked"),
                    display::Frame::new(25., 25.).rounded(5.),
                    Style {
                        opacity: if translucent { 0.5 } else { 1. },
                        inner_shadow_layers: vec![red(1., 0.)],
                        ..Default::default()
                    },
                );
                s.update_object(&"mask".into(), |o| o.mask = MaskMode::Alpha)
                    .unwrap();
                rect(&mut s, "mc", "mask", 0., 0., Color::WHITE);
                rect(&mut s, "r", "masked", 8., 8., Color::rgb(0, 0, 255));
                s.update_object(&"r".into(), |o| {
                    o.style.inner_shadow_layers = vec![red(2., 0.)];
                    o.style.blend_mode = BlendMode::Screen;
                })
                .unwrap();
                let src = source(&s);
                let mut reference = PixelBudget::new(CompositeLimits::default(), 5).unwrap();
                let f = rasterize(&src, viewport(), &mut reference, &AtomicBool::new(false))
                    .unwrap()
                    .unwrap();
                let exact = CompositeLimits {
                    max_surface_bytes: reference.peak_bytes(),
                    max_surface_writes: reference.surface_writes(),
                    ..Default::default()
                };
                let mut b = PixelBudget::new(exact, 5).unwrap();
                let again = rasterize(&src, viewport(), &mut b, &AtomicBool::new(false))
                    .unwrap()
                    .unwrap();
                assert_eq!(again.premul_rgba, f.premul_rgba);
                assert_eq!(
                    (b.peak_bytes(), b.surface_writes()),
                    (reference.peak_bytes(), reference.surface_writes())
                );
                b.discard_frame(again);
                assert_eq!(b.retained_bytes(), 5);
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
                    let mut b = PixelBudget::new(limits, 5).unwrap();
                    assert!(matches!(
                        rasterize(&src, viewport(), &mut b, &AtomicBool::new(false)),
                        Err(CompositeError::WorkLimit | CompositeError::WorkingSetLimit)
                    ));
                    assert_eq!(
                        (b.peak_bytes(), b.retained_bytes(), b.surface_writes()),
                        (5, 5, 0)
                    );
                }
            }
        }
    }
}

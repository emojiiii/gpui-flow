use super::*;
use gpui_flow_scene::{display, *};
use std::sync::atomic::AtomicBool;

fn viewport() -> RasterViewport {
    let bounds = Bounds::new(Point::ZERO, Size::new(200., 200.));
    RasterViewport {
        world_to_screen: Affine::IDENTITY,
        canvas_bounds: bounds,
        content_mask: bounds,
        device_scale: 1.,
    }
}
fn source(s: &Scene) -> DomainSource {
    scene_units(s, CompositeLimits::default())
        .unwrap()
        .into_iter()
        .find_map(|unit| match unit {
            SceneUnit::Domain(d) => Some(*d),
            SceneUnit::Rejected(issue) => panic!("{issue:?}"),
            _ => None,
        })
        .expect("composite domain")
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
fn scene(layers: Vec<Shadow>) -> Scene {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("r", display::Rect::new(10., 10.))
            .with_transform(Affine::translation(20., 20.))
            .with_style(Style {
                fill: Some(Color::hex(0x0000ff).into()),
                shadow_layers: layers,
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    s
}
fn effect(x: f32, y: f32, color: Color) -> Shadow {
    Shadow::new(x, y, 0., color)
}

#[test]
fn base_then_layers_are_source_over_back_to_front_behind_source() {
    let red = effect(20., 0., Color::rgba(255, 0, 0, 128));
    let green = effect(20., 0., Color::rgba(0, 255, 0, 128));
    let mut s = scene(vec![green]);
    s.update_object(&"r".into(), |o| o.style.shadow = Some(red))
        .unwrap();
    let f = render(&s);
    assert_eq!(pixel(&f, 45., 25.), [64, 128, 0, 192]);
    assert_eq!(pixel(&f, 25., 25.), [0, 0, 255, 255]);
    s.update_object(&"r".into(), |o| {
        o.style.shadow = Some(green);
        o.style.shadow_layers = vec![red];
    })
    .unwrap();
    assert_eq!(pixel(&render(&s), 45., 25.), [128, 64, 0, 192]);
    s.update_object(&"r".into(), |o| {
        o.style.shadow.as_mut().unwrap().x = 0.;
        o.style.shadow_layers[0].x = 0.;
    })
    .unwrap();
    assert_eq!(pixel(&render(&s), 25., 25.), [0, 0, 255, 255]);
}

#[test]
fn same_node_layers_never_cast_each_other_or_chain_the_bounds() {
    let s = scene(vec![
        effect(20., 0., Color::hex(0xff0000)),
        effect(40., 0., Color::hex(0x00ff00)),
    ]);
    let f = render(&s);
    assert_eq!(pixel(&f, 45., 25.), [255, 0, 0, 255]);
    assert_eq!(pixel(&f, 65., 25.), [0, 255, 0, 255]);
    assert_eq!(pixel(&f, 85., 25.), [0; 4]);
    assert_eq!(f.screen_bounds.max_x(), 73.); // source 30 + 40 + AA guard
}

#[test]
fn distinct_blur_offset_color_and_fractional_alpha_keep_independent_sources() {
    let s = scene(vec![
        effect(20.5, 0., Color::rgba(255, 0, 0, 128)),
        Shadow::new(0., 30., 4., Color::rgba(0, 255, 0, 160)),
    ]);
    let f = render(&s);
    assert_eq!(pixel(&f, 40., 25.), [64, 0, 0, 64]);
    assert_eq!(pixel(&f, 45., 25.), [128, 0, 0, 128]);
    let soft = pixel(&f, 18., 55.);
    assert!(soft[3] > 0 && soft[3] < 160);
    assert_eq!(soft[0], 0);
    assert_eq!(soft[1], soft[3]);
    assert_eq!(pixel(&f, 45., 55.), [0; 4]); // no blue/green projection of the red layer
}

#[test]
fn opacity_applies_once_to_source_plus_all_layers() {
    let mut s = scene(vec![
        effect(0., 0., Color::rgba(255, 0, 0, 128)),
        effect(0., 0., Color::rgba(0, 255, 0, 128)),
    ]);
    s.update_object(&"r".into(), |o| {
        o.style.fill = Some(Color::rgba(0, 0, 255, 128).into());
        o.style.opacity = 0.5;
    })
    .unwrap();
    let p = pixel(&render(&s), 25., 25.);
    // Each shadow gets source A=.5 and tint A=.5 => .25, composed .4375,
    // then source .5 => .71875; whole object opacity .5 => about 92/255.
    assert!((p[3] as i32 - 92).abs() <= 1);
}

#[test]
fn all_transparent_layers_keep_cpu_declaration_but_no_filter_or_isolation() {
    let s = scene(vec![
        effect(100., 0., Color::TRANSPARENT);
        MAX_SHADOW_LAYERS
    ]);
    let mut budget = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    let f = rasterize(
        &source(&s),
        viewport(),
        &mut budget,
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(pixel(&f, 25., 25.), [0, 0, 255, 255]);
    assert_eq!(budget.peak_bytes(), f.premul_rgba.len() as u64);
    assert_eq!(budget.surface_writes(), f.width as u64 * f.height as u64);
}

#[test]
fn parent_projects_all_child_layers_but_not_its_own_layers() {
    let mut s = scene(vec![
        effect(15., 0., Color::WHITE),
        effect(30., 0., Color::WHITE),
    ]);
    s.insert(
        DisplayObject::new("g", display::Group::new()).with_style(Style {
            shadow_layers: vec![
                effect(60., 0., Color::hex(0xff0000)),
                effect(0., 60., Color::hex(0x00ff00)),
            ],
            ..Default::default()
        }),
        None,
        Some(0),
    )
    .unwrap();
    s.reparent(&"r".into(), Some("g".into()), None, false)
        .unwrap();
    let f = render(&s);
    for x in [25., 40., 55.] {
        assert_eq!(pixel(&f, x + 60., 25.), [255, 0, 0, 255]);
        assert_eq!(pixel(&f, x, 85.), [0, 255, 0, 255]);
        assert_eq!(pixel(&f, x + 60., 85.), [0; 4]);
    }
}

#[test]
fn layers_only_container_keeps_unsafe_pass_through_rejection() {
    let mut s = scene(vec![]);
    s.insert(
        DisplayObject::new("g", display::Group::new()).with_style(Style {
            shadow_layers: vec![effect(20., 0., Color::BLACK)],
            ..Default::default()
        }),
        None,
        Some(0),
    )
    .unwrap();
    s.reparent(&"r".into(), Some("g".into()), None, false)
        .unwrap();
    s.update_object(&"r".into(), |o| o.style.blend_mode = BlendMode::Multiply)
        .unwrap();
    assert!(
        matches!(&scene_units(&s,CompositeLimits::default()).unwrap()[0],SceneUnit::Rejected(CompositeIssue{error:CompositeError::UnsupportedShadowBackdrop(id),..}) if id==&DisplayId::from("g"))
    );
    s.update_object(&"g".into(), |o| {
        o.style.shadow_layers[0].color = Color::TRANSPARENT
    })
    .unwrap();
    assert!(matches!(
        &scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::UnsupportedExternalBackdrop,
            ..
        })
    ));
    s.update_object(&"g".into(), |o| {
        o.style.shadow_layers[0].color = Color::BLACK;
        o.style.blend_mode = BlendMode::Normal;
    })
    .unwrap();
    assert!(pixel(&render(&s), 45., 25.)[3] > 0);
}

#[test]
fn multiple_shadow_cache_placement_covers_order_color_blur_and_layer_removal() {
    let s = scene(vec![
        effect(20., 0., Color::BLACK),
        effect(40., 0., Color::WHITE),
    ]);
    let before = source(&s);
    for variant in 0..4 {
        let mut next = s.clone();
        next.update_object(&"r".into(), |o| match variant {
            0 => o.style.shadow_layers.reverse(),
            1 => o.style.shadow_layers[0].color = Color::TRANSPARENT,
            2 => o.style.shadow_layers[0].blur = 1.,
            _ => {
                o.style.shadow_layers.pop();
            }
        })
        .unwrap();
        assert!(!before.placement_eq(&source(&next)));
    }
}

#[test]
fn multiple_layers_admit_full_independent_work_and_release_each_scratch() {
    let s = scene(vec![
        Shadow::new(10., 0., 1., Color::BLACK),
        Shadow::new(30., 0., 2., Color::BLACK),
        effect(0., 30., Color::WHITE),
    ]);
    let src = source(&s);
    let mut reference = PixelBudget::new(CompositeLimits::default(), 7).unwrap();
    let f = rasterize(&src, viewport(), &mut reference, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    let limits = CompositeLimits {
        max_surface_bytes: reference.peak_bytes(),
        max_surface_writes: reference.surface_writes(),
        ..Default::default()
    };
    let mut exact = PixelBudget::new(limits, 7).unwrap();
    let second = rasterize(&src, viewport(), &mut exact, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(f.premul_rgba, second.premul_rgba);
    assert_eq!(exact.retained_bytes(), 7 + second.premul_rgba.len() as u64);
    exact.discard_frame(second);
    assert_eq!(exact.retained_bytes(), 7);
    for limited in [
        CompositeLimits {
            max_surface_bytes: limits.max_surface_bytes - 1,
            ..limits
        },
        CompositeLimits {
            max_surface_writes: limits.max_surface_writes - 1,
            ..limits
        },
    ] {
        let mut denied = PixelBudget::new(limited, 7).unwrap();
        assert!(matches!(
            rasterize(&src, viewport(), &mut denied, &AtomicBool::new(false)),
            Err(CompositeError::WorkingSetLimit | CompositeError::WorkLimit)
        ));
        assert_eq!(denied.peak_bytes(), 7);
        assert_eq!(denied.retained_bytes(), 7);
        assert_eq!(denied.surface_writes(), 0);
    }
}

#[test]
fn multilayer_deep_chain_rejects_before_any_pixel_or_filter_work() {
    let mut s = Scene::new();
    for n in 0..8 {
        s.insert(
            DisplayObject::new(format!("g{n}"), display::Group::new()).with_style(Style {
                shadow_layers: vec![effect(0., 0., Color::BLACK); 3],
                ..Default::default()
            }),
            (n > 0).then(|| format!("g{}", n - 1).into()),
            None,
        )
        .unwrap();
    }
    s.insert(
        DisplayObject::new("r", display::Rect::new(10., 10.)),
        Some("g7".into()),
        None,
    )
    .unwrap();
    let mut budget = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    assert!(matches!(
        rasterize(
            &source(&s),
            viewport(),
            &mut budget,
            &AtomicBool::new(false)
        ),
        Err(CompositeError::WorkLimit)
    ));
    assert_eq!(budget.peak_bytes(), 0);
    assert_eq!(budget.surface_writes(), 0);
}

#[test]
fn later_device_invalid_layer_rejects_before_valid_first_layer_does_work() {
    let s = scene(vec![
        effect(10., 0., Color::BLACK),
        Shadow::new(0., 0., 256., Color::BLACK),
    ]);
    let mut budget = PixelBudget::new(CompositeLimits::default(), 9).unwrap();
    assert_eq!(
        rasterize(
            &source(&s),
            viewport(),
            &mut budget,
            &AtomicBool::new(false)
        )
        .unwrap_err(),
        CompositeError::TooLarge
    );
    assert_eq!(budget.peak_bytes(), 9);
    assert_eq!(budget.retained_bytes(), 9);
    assert_eq!(budget.surface_writes(), 0);
}

#[test]
fn multiple_shadows_mask_coverage_and_masked_source_remain_distinct() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("g", display::Group::new()).with_style(Style {
            shadow_layers: vec![
                effect(0., 50., Color::hex(0xff0000)),
                effect(60., 0., Color::hex(0x00ff00)),
            ],
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("mask", display::Rect::new(10., 10.))
            .with_transform(Affine::translation(20., 20.))
            .with_style(Style {
                shadow_layers: vec![effect(15., 0., Color::WHITE), effect(30., 0., Color::WHITE)],
                ..Default::default()
            })
            .with_mask(MaskMode::Alpha),
        Some("g".into()),
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("ink", display::Rect::new(45., 10.))
            .with_transform(Affine::translation(25., 20.))
            .with_style(Style::fill(Color::hex(0x0000ff))),
        Some("g".into()),
        None,
    )
    .unwrap();
    let f = render(&s);
    for x in [27., 37., 52.] {
        assert_eq!(pixel(&f, x, 25.), [0, 0, 255, 255]);
        assert_eq!(pixel(&f, x, 75.), [255, 0, 0, 255]);
        assert_eq!(pixel(&f, x + 60., 25.), [0, 255, 0, 255]);
    }
    for x in [22., 32., 47., 65.] {
        assert_eq!(pixel(&f, x, 25.), [0; 4]);
        assert_eq!(pixel(&f, x, 75.), [0; 4]);
        assert_eq!(pixel(&f, x + 60., 25.), [0; 4]);
    }
}

#[test]
fn both_container_phases_and_rounded_child_clips_feed_each_layer() {
    for frame in [false, true] {
        for order in [
            ContainerPaintOrder::BeforeChildren,
            ContainerPaintOrder::StrokeAfterChildren,
        ] {
            let mut s = Scene::new();
            let shape = if frame {
                Shape::Frame(
                    display::Frame::new(30., 30.)
                        .rounded(8.)
                        .with_paint_order(order),
                )
            } else {
                Shape::Box(
                    display::Box::new(30., 30.)
                        .rounded(8.)
                        .with_paint_order(order),
                )
            };
            s.insert(
                DisplayObject::new("g", shape)
                    .with_transform(Affine::translation(20., 20.))
                    .with_style(Style {
                        fill: Some(Color::rgba(0, 255, 0, 100).into()),
                        stroke: Some(
                            Stroke::new(Color::hex(0x0000ff), 4.)
                                .with_alignment(StrokeAlignment::Outside),
                        ),
                        shadow_layers: vec![
                            effect(0., 80., Color::hex(0xff0000)),
                            effect(80., 0., Color::hex(0x00ff00)),
                        ],
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
            s.insert(
                DisplayObject::new("ink", display::Rect::new(10., 10.))
                    .with_transform(Affine::translation(25., 10.))
                    .with_style(Style::fill(Color::WHITE)),
                Some("g".into()),
                None,
            )
            .unwrap();
            let f = render(&s);
            for y in 14..58 {
                for x in 14..58 {
                    let a = pixel(&f, x as f32, y as f32)[3];
                    assert_eq!(
                        a,
                        pixel(&f, x as f32, y as f32 + 80.)[3],
                        "vertical {frame} {order:?} {x},{y}"
                    );
                    assert_eq!(
                        a,
                        pixel(&f, x as f32 + 80., y as f32)[3],
                        "horizontal {frame} {order:?} {x},{y}"
                    );
                }
            }
            assert_eq!(
                pixel(&f, 51., 35.),
                if frame || order == ContainerPaintOrder::StrokeAfterChildren {
                    [0, 0, 255, 255]
                } else {
                    [255; 4]
                }
            );
            assert_eq!(pixel(&f, 54., 115.)[3], if frame { 0 } else { 255 });
            assert_eq!(pixel(&f, 134., 35.)[3], if frame { 0 } else { 255 });
        }
    }
}

#[test]
fn inherited_antialiased_clip_is_applied_once_after_all_layers_and_source() {
    let mut s = scene(vec![
        effect(0., 0., Color::hex(0xff0000)),
        effect(0., 0., Color::hex(0x00ff00)),
    ]);
    s.insert(
        DisplayObject::new("frame", display::Frame::new(10., 10.).rounded(4.))
            .with_transform(Affine::translation(20.25, 20.5))
            .with_style(Style {
                fill: None,
                ..Default::default()
            }),
        None,
        Some(0),
    )
    .unwrap();
    s.reparent(&"r".into(), Some("frame".into()), None, true)
        .unwrap();
    let f = render(&s);
    assert!(
        f.premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0 && p[3] < 255)
    );
    for p in f.premul_rgba.as_chunks::<4>().0 {
        assert_eq!(p[0], 0, "red shadow must be occluded even at clip AA edges");
        assert_eq!(
            p[1], 0,
            "green shadow must be occluded even at clip AA edges"
        );
    }
}

#[test]
fn offscreen_source_can_cast_each_layer_into_viewport() {
    let mut s = scene(vec![
        effect(50., 0., Color::hex(0xff0000)),
        effect(70., 0., Color::hex(0x00ff00)),
    ]);
    s.update_object(&"r".into(), |o| {
        o.transform = Affine::translation(-30., 20.)
    })
    .unwrap();
    let f = render(&s);
    assert_eq!(pixel(&f, 25., 25.), [255, 0, 0, 255]);
    assert_eq!(pixel(&f, 45., 25.), [0, 255, 0, 255]);
    assert_eq!(pixel(&f, 95., 25.), [0; 4]);
}

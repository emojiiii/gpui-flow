use super::*;
use gpui_flow_scene::{display, *};
use std::sync::atomic::AtomicBool;
fn style(shadow: Option<Shadow>) -> Style {
    Style {
        shadow,
        ..Default::default()
    }
}
fn group(s: &mut Scene, id: &str, parent: Option<&str>, shadow: Option<Shadow>) {
    s.insert(
        DisplayObject::new(id, display::Group::new()).with_style(style(shadow)),
        parent.map(Into::into),
        None,
    )
    .unwrap();
}
fn rect(s: &mut Scene, id: &str, parent: &str, x: f32, y: f32, color: Color) {
    s.insert(
        DisplayObject::new(id, display::Rect::new(10., 10.))
            .with_transform(Affine::translation(x, y))
            .with_style(Style::fill(color)),
        Some(parent.into()),
        None,
    )
    .unwrap();
}
fn shadow(x: f32, y: f32) -> Option<Shadow> {
    Some(Shadow::new(x, y, 0., Color::hex(0xff0000)))
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
    let bounds = Bounds::new(Point::ZERO, Size::new(200., 200.));
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
#[test]
fn sparse_group_casts_composed_alpha_including_child_shadows() {
    let mut s = Scene::new();
    group(&mut s, "group", None, shadow(60., 0.));
    group(&mut s, "nested", Some("group"), shadow(15., 0.));
    rect(&mut s, "ink", "nested", 20., 20., Color::WHITE);
    rect(&mut s, "separate", "group", 20., 45., Color::WHITE);
    let f = render(&s);
    assert_eq!(pixel(&f, 25., 25.), [255; 4]);
    assert_eq!(pixel(&f, 40., 25.), [255, 0, 0, 255]);
    assert_eq!(pixel(&f, 85., 25.), [255, 0, 0, 255]);
    assert_eq!(pixel(&f, 100., 25.), [255, 0, 0, 255]); // parent's shadow of child's shadow
    assert_eq!(pixel(&f, 85., 38.), [0; 4]); // not the rectangular union
    assert_eq!(pixel(&f, 85., 50.), [255, 0, 0, 255]);
    assert!(f.screen_bounds.max_x() >= 105.);
}
#[test]
fn parent_opacity_is_once_after_overlapping_source_and_shadow() {
    let mut s = Scene::new();
    group(&mut s, "g", None, shadow(0., 0.));
    rect(&mut s, "a", "g", 20., 20., Color::rgba(0, 0, 255, 128));
    rect(&mut s, "b", "g", 25., 20., Color::rgba(0, 0, 255, 128));
    s.update_object(&"g".into(), |o| o.style.opacity = 0.5)
        .unwrap();
    let f = render(&s);
    // Composed ink alpha 192, shadow alpha 192, overlap 240, then parent .5.
    assert!((pixel(&f, 27., 25.)[3] as i32 - 120).abs() <= 1);
    assert!((pixel(&f, 22., 25.)[3] as i32 - 96).abs() <= 1);
}
#[test]
fn containers_shadow_clipped_complete_phase_stream_and_preserve_own_strokes() {
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
                DisplayObject::new("box", shape)
                    .with_transform(Affine::translation(20., 20.))
                    .with_style(Style {
                        fill: Some(Color::rgba(0, 255, 0, 100).into()),
                        stroke: Some(
                            Stroke::new(Color::hex(0x0000ff), 4.)
                                .with_alignment(StrokeAlignment::Outside),
                        ),
                        shadow: shadow(0., 80.),
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
            rect(&mut s, "overflow", "box", 25., 10., Color::WHITE);
            let f = render(&s);
            assert_eq!(pixel(&f, 18., 35.), [0, 0, 255, 255]);
            assert_eq!(pixel(&f, 18., 115.), [255, 0, 0, 255]); // own outside stroke survives child clip
            assert_eq!(pixel(&f, 53., 35.)[3], 255); // own stroke or child
            assert_eq!(pixel(&f, 54., 35.)[3], if frame { 0 } else { 255 });
            assert_eq!(pixel(&f, 54., 115.)[3], if frame { 0 } else { 255 });
            for y in 14..58 {
                for x in 14..58 {
                    assert_eq!(
                        pixel(&f, x as f32, y as f32)[3],
                        pixel(&f, x as f32, y as f32 + 80.)[3],
                        "{frame} {order:?} at {x},{y}"
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
        }
    }
}
#[test]
fn group_shadow_uses_masked_content_not_mask_or_unmasked_child_bounds() {
    let mut s = Scene::new();
    group(&mut s, "g", None, shadow(60., 0.));
    rect(&mut s, "mask", "g", 20., 20., Color::WHITE);
    s.update_object(&"mask".into(), |o| o.mask = MaskMode::Alpha)
        .unwrap();
    rect(&mut s, "ink", "g", 25., 20., Color::hex(0x0000ff));
    let f = render(&s);
    assert_eq!(pixel(&f, 82., 25.), [0; 4]);
    assert_eq!(pixel(&f, 87., 25.), [255, 0, 0, 255]);
    assert_eq!(pixel(&f, 92., 25.), [0; 4]);
}
fn blend_scene() -> Scene {
    let mut s = Scene::new();
    group(&mut s, "root", None, None);
    s.update_object(&"root".into(), |o| o.style.blend_mode = BlendMode::Normal)
        .unwrap();
    rect(&mut s, "backdrop", "root", 20., 20., Color::WHITE);
    group(&mut s, "shadow", Some("root"), shadow(30., 0.));
    group(&mut s, "nested", Some("shadow"), None);
    rect(&mut s, "ink", "nested", 20., 20., Color::hex(0x0000ff));
    s.update_object(&"ink".into(), |o| o.style.blend_mode = BlendMode::Multiply)
        .unwrap();
    s
}
#[test]
fn unsafe_pass_through_rejects_inside_owned_and_normal_domains() {
    let s = blend_scene();
    assert!(
        matches!(&scene_units(&s,CompositeLimits::default()).unwrap()[0],SceneUnit::Rejected(CompositeIssue{error:CompositeError::UnsupportedShadowBackdrop(id),..}) if id==&DisplayId::from("shadow"))
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
        CompositeError::UnsupportedShadowBackdrop("shadow".into())
    );
}
#[test]
fn nested_normal_or_existing_opacity_isolation_makes_shadow_safe() {
    for target in ["nested", "shadow"] {
        let mut s = blend_scene();
        s.update_object(&target.into(), |o| o.style.blend_mode = BlendMode::Normal)
            .unwrap();
        assert_eq!(pixel(&render(&s), 55., 25.), [255, 0, 0, 255]);
    }
    let mut s = blend_scene();
    s.update_object(&"shadow".into(), |o| o.style.opacity = 0.5)
        .unwrap();
    assert_eq!(pixel(&render(&s), 55., 25.)[3], 128);
}
#[test]
fn transparent_shadow_does_not_introduce_isolation_or_rejection() {
    let mut s = blend_scene();
    s.update_object(&"shadow".into(), |o| {
        o.style.shadow.as_mut().unwrap().color = Color::TRANSPARENT
    })
    .unwrap();
    assert_eq!(pixel(&render(&s), 25., 25.), [0, 0, 255, 255]);
}
#[test]
fn opaque_mask_does_not_hide_unsafe_blends_but_translucent_run_does() {
    let mut s = blend_scene();
    s.insert(
        DisplayObject::new("mask", display::Rect::new(80., 80.))
            .with_style(Style::fill(Color::WHITE))
            .with_mask(MaskMode::Alpha),
        Some("shadow".into()),
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
    assert_eq!(pixel(&render(&s), 55., 25.)[3], 128);
}
#[test]
fn artistic_parent_shadow_uses_owned_backdrop_but_external_root_rejects() {
    let mut s = Scene::new();
    group(&mut s, "g", None, shadow(30., 0.));
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
    let source = owned_canvas(
        &s,
        CanvasBackdrop {
            color: Color::hex(0x00ffff),
            grid: None,
        },
        CompositeLimits::default(),
    )
    .unwrap();
    let f = rasterize(
        &source,
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(pixel(&f, 55., 25.), [0, 0, 0, 255]);
}
#[test]
fn recursive_filters_and_masks_are_admitted_before_output_allocation() {
    let mut s = Scene::new();
    group(
        &mut s,
        "g",
        None,
        Some(Shadow::new(10., 0., 2., Color::BLACK)),
    );
    group(
        &mut s,
        "inner",
        Some("g"),
        Some(Shadow::new(10., 0., 2., Color::BLACK)),
    );
    rect(&mut s, "mask", "inner", 20., 20., Color::WHITE);
    s.update_object(&"mask".into(), |o| o.mask = MaskMode::Alpha)
        .unwrap();
    rect(&mut s, "ink", "inner", 20., 20., Color::WHITE);
    for limits in [
        CompositeLimits {
            max_surface_bytes: 20_000,
            ..Default::default()
        },
        CompositeLimits {
            max_surface_writes: 20_000,
            ..Default::default()
        },
    ] {
        let mut budget = PixelBudget::new(limits, 7).unwrap();
        assert!(matches!(
            rasterize(
                &source(&s),
                viewport(),
                &mut budget,
                &AtomicBool::new(false)
            ),
            Err(CompositeError::WorkingSetLimit | CompositeError::WorkLimit)
        ));
        assert_eq!(budget.peak_bytes(), 7);
        assert_eq!(budget.retained_bytes(), 7);
        assert_eq!(budget.surface_writes(), 0);
    }
}
#[test]
fn exponential_shadow_chain_is_rejected_without_filter_or_pixel_work() {
    let mut s = Scene::new();
    for n in 0..14 {
        group(
            &mut s,
            &format!("g{n}"),
            (n > 0).then(|| format!("g{}", n - 1)).as_deref(),
            shadow(0., 0.),
        );
    }
    rect(&mut s, "ink", "g13", 20., 20., Color::WHITE);
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
    assert_eq!(budget.retained_bytes(), 0);
}
#[test]
fn admission_matches_recursive_runtime_charges_and_releases_all_scratch() {
    let mut s = Scene::new();
    group(&mut s, "g", None, shadow(40., 0.));
    group(&mut s, "inner", Some("g"), shadow(15., 0.));
    rect(&mut s, "r", "inner", 20., 20., Color::WHITE);
    let source = source(&s);
    let mut reference = PixelBudget::new(CompositeLimits::default(), 9).unwrap();
    let f = rasterize(&source, viewport(), &mut reference, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    let limits = CompositeLimits {
        max_surface_bytes: reference.peak_bytes(),
        max_surface_writes: reference.surface_writes(),
        ..Default::default()
    };
    let mut exact = PixelBudget::new(limits, 9).unwrap();
    let second = rasterize(&source, viewport(), &mut exact, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(f.premul_rgba, second.premul_rgba);
    assert_eq!(exact.retained_bytes(), 9 + second.premul_rgba.len() as u64);
    exact.discard_frame(second);
    assert_eq!(exact.retained_bytes(), 9);
}
#[test]
fn offscreen_descendant_still_casts_visible_group_shadow() {
    let mut s = Scene::new();
    group(&mut s, "g", None, shadow(50., 0.));
    rect(&mut s, "r", "g", -30., 20., Color::WHITE);
    let f = render(&s);
    assert_eq!(pixel(&f, 25., 25.), [255, 0, 0, 255]);
}

#[test]
fn recursive_admission_runtime_matrix_covers_mask_shadows_clips_and_phase_order() {
    for enabled in [false, true] {
        for translucent in [false, true] {
            for late_stroke in [false, true] {
                let mut s = Scene::new();
                group(
                    &mut s,
                    "root",
                    None,
                    if enabled { shadow(30., 40.) } else { None },
                );
                s.update_object(&"root".into(), |o| o.style.blend_mode = BlendMode::Normal)
                    .unwrap();
                let order = if late_stroke {
                    ContainerPaintOrder::StrokeAfterChildren
                } else {
                    ContainerPaintOrder::BeforeChildren
                };
                s.insert(
                    DisplayObject::new(
                        "frame",
                        display::Frame::new(50., 45.)
                            .rounded(7.)
                            .with_paint_order(order),
                    )
                    .with_transform(Affine::translation(10.25, 10.5))
                    .with_style(Style {
                        fill: Some(Color::rgba(30, 50, 100, 100).into()),
                        blend_mode: BlendMode::Normal,
                        stroke: Some(Stroke::new(Color::WHITE, 3.)),
                        shadow: if enabled { shadow(5., 2.) } else { None },
                        ..Default::default()
                    }),
                    Some("root".into()),
                    None,
                )
                .unwrap();
                group(&mut s, "masked", Some("frame"), None);
                s.insert(
                    DisplayObject::new(
                        "mask",
                        display::Frame::new(30., 25.)
                            .rounded(6.)
                            .with_paint_order(order),
                    )
                    .with_style(Style {
                        fill: Some(Color::rgba(255, 255, 255, 180).into()),
                        opacity: if translucent { 0.5 } else { 1. },
                        shadow: if enabled { shadow(2., 1.) } else { None },
                        ..Default::default()
                    })
                    .with_mask(MaskMode::Alpha),
                    Some("masked".into()),
                    None,
                )
                .unwrap();
                rect(&mut s, "mask-child", "mask", 5., 5., Color::WHITE);
                rect(&mut s, "ink", "masked", 8., 8., Color::hex(0x0000ff));
                s.update_object(&"ink".into(), |o| {
                    o.style.shadow = if enabled { shadow(3., 0.) } else { None };
                    o.style.blend_mode = BlendMode::Screen;
                })
                .unwrap();
                rect(
                    &mut s,
                    "tail",
                    "masked",
                    20.,
                    20.,
                    Color::rgba(0, 255, 0, 160),
                );
                let source = source(&s);
                let mut reference = PixelBudget::new(CompositeLimits::default(), 5).unwrap();
                let f = rasterize(&source, viewport(), &mut reference, &AtomicBool::new(false))
                    .unwrap()
                    .unwrap();
                let limits = CompositeLimits {
                    max_surface_bytes: reference.peak_bytes(),
                    max_surface_writes: reference.surface_writes(),
                    ..Default::default()
                };
                let mut budget = PixelBudget::new(limits, 5).unwrap();
                let again = rasterize(&source, viewport(), &mut budget, &AtomicBool::new(false))
                    .unwrap()
                    .unwrap();
                assert_eq!(again.premul_rgba, f.premul_rgba);
                assert_eq!(budget.surface_writes(), reference.surface_writes());
                assert_eq!(budget.peak_bytes(), reference.peak_bytes());
                if enabled {
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
                        let mut denied = PixelBudget::new(limited, 5).unwrap();
                        assert!(matches!(
                            rasterize(&source, viewport(), &mut denied, &AtomicBool::new(false)),
                            Err(CompositeError::WorkingSetLimit | CompositeError::WorkLimit)
                        ));
                        assert_eq!(denied.peak_bytes(), 5);
                        assert_eq!(denied.surface_writes(), 0);
                        assert_eq!(denied.retained_bytes(), 5);
                    }
                }
            }
        }
    }
}

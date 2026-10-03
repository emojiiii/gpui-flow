use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, DisplayId, DisplayObject, MaskMode, Point, Scene, Size,
    Style, display,
};
use std::sync::atomic::AtomicBool;
fn backdrop(color: Color) -> CanvasBackdrop {
    CanvasBackdrop { color, grid: None }
}
fn viewport() -> RasterViewport {
    let bounds = Bounds::new(Point::ZERO, Size::new(100., 80.));
    RasterViewport {
        world_to_screen: Affine::IDENTITY,
        canvas_bounds: bounds,
        content_mask: bounds,
        device_scale: 1.,
    }
}
fn rect(id: &str, color: Color, blend: BlendMode) -> DisplayObject {
    DisplayObject::new(id, display::Rect::new(30., 30.)).with_style(Style {
        fill: Some(color.into()),
        blend_mode: blend,
        ..Default::default()
    })
}
fn render(scene: &Scene, backdrop: CanvasBackdrop, view: RasterViewport) -> CpuFrame {
    rasterize(
        &owned_canvas(scene, backdrop, CompositeLimits::default()).unwrap(),
        view,
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}
fn pixel(frame: &CpuFrame, x: u32, y: u32) -> [u8; 4] {
    frame.premul_rgba[((y * frame.width + x) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}
#[test]
fn empty_canvas_has_real_opaque_pixels_and_no_canonical_identity() {
    let scene = Scene::new();
    let source = owned_canvas(
        &scene,
        backdrop(Color::hex(0x123456)),
        CompositeLimits::default(),
    )
    .unwrap();
    assert!(source.is_owned_canvas());
    assert!(source.root_id().as_str().is_empty());
    assert!(source.ids().is_empty());
    assert_eq!(scene, Scene::new());
    let frame = render(&scene, backdrop(Color::hex(0x123456)), viewport());
    assert_eq!((frame.width, frame.height), (100, 80));
    assert_eq!(pixel(&frame, 99, 79), [0x12, 0x34, 0x56, 255]);
}
#[test]
fn artistic_root_modes_use_owned_background_and_respect_rounded_clipping() {
    for (blend, expected) in [
        (BlendMode::Difference, [255, 0, 255, 255]),
        (BlendMode::Darken, [0, 0, 0, 255]),
        (BlendMode::Lighten, [255, 0, 255, 255]),
        (BlendMode::Hue, [94, 0, 0, 255]),
        (BlendMode::Color, [94, 0, 0, 255]),
        (BlendMode::Saturation, [0, 0, 255, 255]),
        (BlendMode::Luminosity, [54, 54, 255, 255]),
    ] {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("clip", display::Frame::new(30., 30.).rounded(10.)).with_style(
                    Style {
                        fill: None,
                        ..Default::default()
                    },
                ),
                None,
                None,
            )
            .unwrap();
        scene
            .insert(
                rect("front", Color::hex(0xff0000), blend),
                Some("clip".into()),
                None,
            )
            .unwrap();
        let frame = render(&scene, backdrop(Color::hex(0x0000ff)), viewport());
        let got = pixel(&frame, 15, 15);
        for i in 0..4 {
            assert!(
                (got[i] as i16 - expected[i]).abs() <= 2,
                "{blend:?} {got:?}"
            );
        }
        assert_eq!(pixel(&frame, 0, 0), [0, 0, 255, 255]);
        assert_eq!(pixel(&frame, 40, 40), [0, 0, 255, 255]);
    }
}
#[test]
fn root_blend_reads_canvas_and_prior_roots_in_stable_tree_order() {
    for (blend, expected) in [
        (BlendMode::Multiply, [0, 0, 0, 255]),
        (BlendMode::Screen, [255, 0, 255, 255]),
    ] {
        let mut scene = Scene::new();
        scene
            .insert(rect("blue", Color::hex(0x0000ff), blend), None, None)
            .unwrap();
        assert_eq!(
            pixel(
                &render(&scene, backdrop(Color::hex(0xff0000)), viewport()),
                5,
                5
            ),
            expected
        );
        // Document order, not ID order. Same-z roots preserve insertion order.
        scene
            .insert(
                rect("aaa-green", Color::hex(0x00ff00), BlendMode::PassThrough),
                None,
                Some(0),
            )
            .unwrap();
        let actual = pixel(&render(&scene, backdrop(Color::WHITE), viewport()), 5, 5);
        assert_eq!(
            actual,
            if blend == BlendMode::Multiply {
                [0, 0, 0, 255]
            } else {
                [0, 255, 255, 255]
            }
        );
        scene
            .update_object(&"aaa-green".into(), |o| o.z_index = 10)
            .unwrap();
        assert_eq!(
            pixel(&render(&scene, backdrop(Color::WHITE), viewport()), 5, 5),
            [0, 255, 0, 255]
        );
    }
}
#[test]
fn nested_normal_isolation_blocks_outer_backdrop_but_pass_through_does_not() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("group", display::Group::new()),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            rect("blue", Color::hex(0x0000ff), BlendMode::Multiply),
            Some("group".into()),
            None,
        )
        .unwrap();
    assert_eq!(
        pixel(
            &render(&scene, backdrop(Color::hex(0xff0000)), viewport()),
            5,
            5
        ),
        [0, 0, 0, 255]
    );
    scene
        .update_object(&"group".into(), |o| o.style.blend_mode = BlendMode::Normal)
        .unwrap();
    assert_eq!(
        pixel(
            &render(&scene, backdrop(Color::hex(0xff0000)), viewport()),
            5,
            5
        ),
        [0, 0, 255, 255]
    );
}
#[test]
fn transparent_root_and_group_opacity_are_applied_once_over_opaque_canvas() {
    let mut scene = Scene::new();
    let mut group = DisplayObject::new("group", display::Group::new());
    group.style.opacity = 0.5;
    scene.insert(group, None, None).unwrap();
    for id in ["a", "b"] {
        scene
            .insert(
                rect(id, Color::hex(0x0000ff), BlendMode::PassThrough),
                Some("group".into()),
                None,
            )
            .unwrap();
    }
    assert_eq!(
        pixel(&render(&scene, backdrop(Color::WHITE), viewport()), 5, 5),
        [128, 128, 255, 255]
    );
    scene
        .update_object(&"group".into(), |o| o.style.opacity = 1.)
        .unwrap();
    scene
        .update_object(&"a".into(), |o| {
            o.style.fill = Some(Color::rgba(255, 0, 0, 128).into())
        })
        .unwrap();
    scene
        .update_object(&"b".into(), |o| o.visible = false)
        .unwrap();
    assert_eq!(
        pixel(&render(&scene, backdrop(Color::WHITE), viewport()), 5, 5),
        [255, 127, 127, 255]
    );
}
#[test]
fn grid_is_a_real_earlier_backdrop_and_stays_screen_anchored() {
    let mut scene = Scene::new();
    scene
        .insert(
            rect("blue", Color::hex(0x0000ff), BlendMode::Multiply),
            None,
            None,
        )
        .unwrap();
    let grid = CanvasBackdrop {
        color: Color::WHITE,
        grid: Some(CanvasGrid {
            color: Color::hex(0xff0000),
            spacing: 12.,
            offset: Point::new(-1., 2.),
        }),
    };
    let frame = render(&scene, grid, viewport());
    assert_eq!(pixel(&frame, 11, 2), [0, 0, 0, 255]);
    assert_eq!(pixel(&frame, 12, 2), [0, 0, 255, 255]);
    let mut view = viewport();
    view.world_to_screen = Affine::translation(40., 30.) * Affine::scale(2., 2.);
    let frame = render(&scene, grid, view);
    assert_eq!(pixel(&frame, 11, 2), [255, 0, 0, 255]);
    assert_eq!(pixel(&frame, 47, 38), [0, 0, 0, 255]);
    assert_eq!(pixel(&frame, 48, 38), [0, 0, 255, 255]);
}
#[test]
fn grid_crop_nonzero_canvas_origin_resize_and_fractional_dpr_keep_correct_phase() {
    let mut view = viewport();
    view.canvas_bounds = Bounds::new(Point::new(10., 20.), Size::new(100., 80.));
    view.content_mask = Bounds::new(Point::new(34., 32.), Size::new(26., 20.));
    view.device_scale = 1.5;
    let grid = CanvasBackdrop {
        color: Color::WHITE,
        grid: Some(CanvasGrid {
            color: Color::BLACK,
            spacing: 12.,
            offset: Point::ZERO,
        }),
    };
    let frame = render(&Scene::new(), grid, view);
    assert_eq!(frame.screen_bounds, view.content_mask);
    assert_eq!((frame.width, frame.height), (39, 30));
    assert_eq!(pixel(&frame, 0, 0), [0, 0, 0, 255]);
    assert_eq!(pixel(&frame, 3, 3), [255, 255, 255, 255]);
    view.canvas_bounds.size = Size::new(15., 15.);
    assert!(
        rasterize(
            &owned_canvas(&Scene::new(), grid, CompositeLimits::default()).unwrap(),
            view,
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false)
        )
        .unwrap()
        .is_none()
    );
}
#[test]
fn alpha_masks_and_rounded_frames_preserve_canvas_outside_their_ink() {
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("g", display::Group::new()), None, None)
        .unwrap();
    scene
        .insert(
            rect("mask", Color::WHITE, BlendMode::PassThrough).with_mask(MaskMode::Alpha),
            Some("g".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            rect("blue", Color::hex(0x0000ff), BlendMode::Multiply)
                .with_transform(Affine::translation(15., 0.)),
            Some("g".into()),
            None,
        )
        .unwrap();
    let frame = render(&scene, backdrop(Color::hex(0xff0000)), viewport());
    assert_eq!(pixel(&frame, 20, 5), [0, 0, 0, 255]);
    assert_eq!(pixel(&frame, 35, 5), [255, 0, 0, 255]);
    scene
        .update_object(&"mask".into(), |o| o.visible = false)
        .unwrap();
    assert_eq!(
        pixel(
            &render(&scene, backdrop(Color::hex(0xff0000)), viewport()),
            20,
            5
        ),
        [0, 0, 0, 255] // no active marker: the blue blend sees the canvas
    );
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("f", display::Frame::new(30., 30.).rounded(10.)).with_style(Style {
                fill: None,
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            rect("ink", Color::hex(0x0000ff), BlendMode::Screen),
            Some("f".into()),
            None,
        )
        .unwrap();
    let frame = render(&scene, backdrop(Color::hex(0xff0000)), viewport());
    assert_eq!(pixel(&frame, 0, 0), [255, 0, 0, 255]);
    assert_eq!(pixel(&frame, 15, 15), [255, 0, 255, 255]);
}
#[test]
fn visible_custom_rejects_whole_canvas_hidden_subtrees_need_no_producer() {
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("g", display::Group::new()), None, None)
        .unwrap();
    scene
        .insert(
            DisplayObject::new(
                "custom",
                display::Custom::new(
                    "native",
                    Bounds::new(Point::ZERO, Size::new(20., 20.)),
                    vec![],
                ),
            ),
            Some("g".into()),
            None,
        )
        .unwrap();
    assert!(matches!(
        owned_canvas(&scene, backdrop(Color::WHITE), CompositeLimits::default()),
        Err(CompositeError::UnsupportedShape(_))
    ));
    scene
        .update_object(&"g".into(), |o| o.visible = false)
        .unwrap();
    let source = owned_canvas(&scene, backdrop(Color::WHITE), CompositeLimits::default()).unwrap();
    assert_eq!(source.ids(), vec![&DisplayId::from("g")]);
    assert_eq!(
        pixel(&render(&scene, backdrop(Color::WHITE), viewport()), 5, 5),
        [255; 4]
    );
}
#[test]
fn missing_image_or_text_is_not_removed_from_the_backdrop() {
    for shape in [
        gpui_flow_scene::Shape::Image(display::Image::new("missing.png", Size::new(20., 20.))),
        gpui_flow_scene::Shape::Text(display::Text::new("ink", Size::new(20., 20.))),
    ] {
        let mut scene = Scene::new();
        scene
            .insert(DisplayObject::new("producer", shape), None, None)
            .unwrap();
        scene
            .insert(rect("blend", Color::BLACK, BlendMode::Multiply), None, None)
            .unwrap();
        let mut budget = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
        assert!(
            rasterize(
                &owned_canvas(&scene, backdrop(Color::WHITE), CompositeLimits::default()).unwrap(),
                viewport(),
                &mut budget,
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert_eq!(budget.retained_bytes(), 0);
    }
}
#[test]
fn owned_canvas_budgets_and_cancellation_release_all_surfaces() {
    let source = owned_canvas(
        &Scene::new(),
        backdrop(Color::WHITE),
        CompositeLimits::default(),
    )
    .unwrap();
    let mut view = viewport();
    view.canvas_bounds.size = Size::new(1920., 1080.);
    view.content_mask = view.canvas_bounds;
    view.device_scale = 2.;
    let mut budget = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    assert!(matches!(
        rasterize(&source, view, &mut budget, &AtomicBool::new(false)),
        Err(CompositeError::TooLarge)
    ));
    assert_eq!(budget.retained_bytes(), 0);
    for limits in [
        CompositeLimits {
            max_surface_bytes: 2,
            ..Default::default()
        },
        CompositeLimits {
            max_surface_writes: 2,
            ..Default::default()
        },
    ] {
        let mut budget = PixelBudget::new(limits, 0).unwrap();
        assert!(rasterize(&source, viewport(), &mut budget, &AtomicBool::new(false)).is_err());
        assert_eq!(budget.retained_bytes(), 0);
    }
    assert!(matches!(
        rasterize(&source, viewport(), &mut budget, &AtomicBool::new(true)),
        Err(CompositeError::Superseded)
    ));
}
#[test]
fn invalid_transparent_backdrops_and_unbounded_grid_geometry_reject() {
    assert_eq!(
        owned_canvas(
            &Scene::new(),
            backdrop(Color::TRANSPARENT),
            CompositeLimits::default()
        ),
        Err(CompositeError::UnsupportedExternalBackdrop)
    );
    for spacing in [0., f32::NAN, f32::INFINITY] {
        assert!(
            owned_canvas(
                &Scene::new(),
                CanvasBackdrop {
                    color: Color::WHITE,
                    grid: Some(CanvasGrid {
                        color: Color::BLACK,
                        spacing,
                        offset: Point::ZERO
                    })
                },
                CompositeLimits::default()
            )
            .is_err()
        );
    }
    let source = owned_canvas(
        &Scene::new(),
        CanvasBackdrop {
            color: Color::WHITE,
            grid: Some(CanvasGrid {
                color: Color::BLACK,
                spacing: 1.,
                offset: Point::ZERO,
            }),
        },
        CompositeLimits::default(),
    )
    .unwrap();
    let mut budget = PixelBudget::new(
        CompositeLimits {
            max_total_points: 10,
            ..Default::default()
        },
        0,
    )
    .unwrap();
    assert!(matches!(
        rasterize(&source, viewport(), &mut budget, &AtomicBool::new(false)),
        Err(CompositeError::InputLimit)
    ));
    assert_eq!(budget.retained_bytes(), 0);
}
#[test]
fn offscreen_prepared_ink_is_culled_without_intermediate_surface_work() {
    let mut scene = Scene::new();
    scene
        .insert(
            rect("far", Color::BLACK, BlendMode::Multiply)
                .with_transform(Affine::translation(2_000_000., 0.)),
            None,
            None,
        )
        .unwrap();
    let source = owned_canvas(&scene, backdrop(Color::WHITE), CompositeLimits::default()).unwrap();
    let mut budget = PixelBudget::new(
        CompositeLimits {
            max_surface_writes: 8_000,
            ..Default::default()
        },
        0,
    )
    .unwrap();
    let frame = rasterize(&source, viewport(), &mut budget, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(pixel(&frame, 0, 0), [255; 4]);
    assert_eq!(budget.surface_writes(), 8_000);
    assert_eq!(budget.peak_bytes(), 32_000);
}
#[test]
fn default_domains_still_reject_external_host_backdrops() {
    let mut scene = Scene::new();
    scene
        .insert(rect("blend", Color::BLACK, BlendMode::Multiply), None, None)
        .unwrap();
    assert!(matches!(
        &scene_units(&scene, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::UnsupportedExternalBackdrop,
            ..
        })
    ));
    assert!(matches!(
        &scene_units(&Scene::new(), CompositeLimits::default()).unwrap()[..],
        []
    ));
}

#[test]
fn offscreen_alpha_mask_keeps_zero_run_and_next_mask_resumes() {
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("g", display::Group::new()), None, None)
        .unwrap();
    for object in [
        rect("far-mask", Color::WHITE, BlendMode::PassThrough)
            .with_transform(Affine::translation(2_000_000., 0.))
            .with_mask(MaskMode::Alpha),
        rect("blocked", Color::BLACK, BlendMode::PassThrough),
        rect("next-mask", Color::WHITE, BlendMode::PassThrough)
            .with_transform(Affine::translation(50., 0.))
            .with_mask(MaskMode::Alpha),
        rect("green", Color::hex(0x00ff00), BlendMode::PassThrough)
            .with_transform(Affine::translation(50., 0.)),
    ] {
        scene.insert(object, Some("g".into()), None).unwrap();
    }
    let frame = render(&scene, backdrop(Color::WHITE), viewport());
    assert_eq!(pixel(&frame, 5, 5), [255; 4]);
    assert_eq!(pixel(&frame, 55, 5), [0, 255, 0, 255]);
}
#[test]
fn offscreen_stroke_centerline_does_not_cull_visible_transformed_ink() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new(
                "stroke",
                display::Line::new(Point::new(-2., 5.), Point::new(-2., 25.)),
            )
            .with_transform(Affine::scale(2., 2.))
            .with_style(Style {
                fill: None,
                stroke: Some(gpui_flow_scene::Stroke::new(Color::hex(0xff0000), 6.)),
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    let mut view = viewport();
    view.device_scale = 1.5;
    let frame = render(&scene, backdrop(Color::WHITE), view);
    assert_eq!(pixel(&frame, 0, 30), [255, 0, 0, 255]);
    assert_eq!(pixel(&frame, 10, 30), [255; 4]);
}
#[test]
fn fractional_grid_crop_matches_corresponding_full_canvas_pixels() {
    let mut full_view = viewport();
    full_view.canvas_bounds = Bounds::new(Point::new(10.25, 20.5), Size::new(100.5, 80.25));
    full_view.content_mask = full_view.canvas_bounds;
    full_view.device_scale = 1.5;
    let backdrop = CanvasBackdrop {
        color: Color::hex(0xe2dcbd),
        grid: Some(CanvasGrid {
            color: Color::rgba(21, 35, 66, 150),
            spacing: 12.25,
            offset: Point::new(-3.5, 2.75),
        }),
    };
    let full = render(&Scene::new(), backdrop, full_view);
    let mut cropped_view = full_view;
    cropped_view.content_mask = Bounds::new(Point::new(34.1, 32.2), Size::new(27.1, 22.3));
    let crop = render(&Scene::new(), backdrop, cropped_view);
    let x = ((crop.screen_bounds.origin.x - full.screen_bounds.origin.x) * 1.5).round() as u32;
    let y = ((crop.screen_bounds.origin.y - full.screen_bounds.origin.y) * 1.5).round() as u32;
    for yy in 0..crop.height {
        for xx in 0..crop.width {
            for (a, b) in pixel(&crop, xx, yy)
                .into_iter()
                .zip(pixel(&full, x + xx, y + yy))
            {
                assert!(a.abs_diff(b) <= 1, "crop phase shifted at {xx},{yy}");
            }
        }
    }
}

#[test]
fn enormous_dpr_grid_dots_are_clipped_before_path_generation() {
    let mut view = viewport();
    view.canvas_bounds.size = Size::new(1e-10, 1e-10);
    view.content_mask = view.canvas_bounds;
    view.device_scale = 1e10;
    let frame = render(
        &Scene::new(),
        CanvasBackdrop {
            color: Color::WHITE,
            grid: Some(CanvasGrid {
                color: Color::BLACK,
                spacing: 12.,
                offset: Point::ZERO,
            }),
        },
        view,
    );
    assert_eq!(pixel(&frame, 0, 0), [0, 0, 0, 255]);
    assert!(frame.width <= 2 && frame.height <= 2);
}

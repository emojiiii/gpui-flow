use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, DisplayObject, EraserMode, FillRule, MaskMode, Point, Scene,
    Shape, Size, Stroke, Style, display,
};
use std::sync::{Arc, atomic::AtomicBool};

fn rect(id: &str, x: f32, y: f32, w: f32, h: f32, color: Color) -> DisplayObject {
    DisplayObject::new(id, display::Rect::new(w, h))
        .with_transform(Affine::translation(x, y))
        .with_style(Style::fill(color))
}
fn insert(s: &mut Scene, object: DisplayObject, parent: &str) {
    s.insert(object, Some(parent.into()), None).unwrap();
}
fn scene() -> Scene {
    let mut s = Scene::new();
    s.insert(DisplayObject::new("g", display::Group::new()), None, None)
        .unwrap();
    insert(
        &mut s,
        rect("red", 0., 0., 40., 40., Color::rgb(255, 0, 0)),
        "g",
    );
    s
}
fn domain(s: &Scene) -> DomainSource {
    scene_units(s, CompositeLimits::default())
        .unwrap()
        .into_iter()
        .find_map(|u| match u {
            SceneUnit::Domain(d) => Some(*d),
            _ => None,
        })
        .expect("eraser forces a domain")
}
fn viewport() -> RasterViewport {
    let b = Bounds::new(Point::ZERO, Size::new(80., 80.));
    RasterViewport {
        world_to_screen: Affine::IDENTITY,
        canvas_bounds: b,
        content_mask: b,
        device_scale: 1.,
    }
}
fn render(source: &DomainSource) -> CpuFrame {
    rasterize(
        source,
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}
fn at(f: &CpuFrame, x: u32, y: u32) -> [u8; 4] {
    let x = x as f32 - f.screen_bounds.origin.x;
    let y = y as f32 - f.screen_bounds.origin.y;
    if x < 0. || y < 0. || x >= f.width as f32 || y >= f.height as f32 {
        return [0; 4];
    }
    f.premul_rgba[((y as u32 * f.width + x as u32) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}
fn eraser(mode: EraserMode) -> DisplayObject {
    rect("erase", 10., 10., 20., 20., Color::WHITE).with_eraser(mode)
}
#[test]
fn pixel_eraser_alpha_opacity_and_override_use_complete_source_once() {
    for (paint_alpha, opacity, expected) in [
        (255, 1., 0),
        (128, 1., 127),
        (255, 0.5, 128),
        (128, 0.5, 191),
    ] {
        let mut s = scene();
        let mut e = eraser(EraserMode::Pixel);
        e.style.fill = Some(Color::rgba(0, 255, 0, paint_alpha).into());
        e.style.opacity = opacity;
        e.style.blend_mode = BlendMode::Multiply;
        insert(&mut s, e, "g");
        let frame = render(&domain(&s));
        assert_eq!(at(&frame, 20, 20), [expected, 0, 0, expected]);
        assert_eq!(at(&frame, 5, 5), [255, 0, 0, 255]);
    }
    let mut s = scene();
    let mut e = eraser(EraserMode::Pixel);
    e.style.opacity = 0.5;
    e.style.stroke = Some(Stroke::new(Color::WHITE, 10.));
    insert(&mut s, e, "g");
    assert_eq!(at(&render(&domain(&s)), 12, 20), [128, 0, 0, 128]);
    let mut s = scene();
    insert(
        &mut s,
        DisplayObject::new("erase", display::Group::new())
            .with_eraser(EraserMode::Pixel)
            .with_style(Style {
                opacity: 0.5,
                ..Default::default()
            }),
        "g",
    );
    insert(&mut s, rect("a", 5., 5., 25., 25., Color::WHITE), "erase");
    insert(&mut s, rect("b", 10., 10., 25., 25., Color::WHITE), "erase");
    let frame = render(&domain(&s));
    assert_eq!(at(&frame, 7, 7), [128, 0, 0, 128]);
    assert_eq!(at(&frame, 20, 20), [128, 0, 0, 128]);
}
#[test]
fn pixel_erasers_keep_stable_sibling_order_and_never_erase_outside_direct_parent() {
    let mut s = Scene::new();
    s.insert(
        rect("external", 0., 0., 50., 50., Color::rgb(0, 0, 255)),
        None,
        None,
    )
    .unwrap();
    s.insert(DisplayObject::new("g", display::Group::new()), None, None)
        .unwrap();
    insert(
        &mut s,
        rect("red", 0., 0., 30., 30., Color::rgb(255, 0, 0)),
        "g",
    );
    insert(&mut s, eraser(EraserMode::Pixel), "g");
    insert(
        &mut s,
        rect("later", 15., 15., 5., 5., Color::rgb(0, 255, 0)),
        "g",
    );
    let source = owned_canvas(
        &s,
        CanvasBackdrop {
            color: Color::WHITE,
            grid: None,
        },
        CompositeLimits::default(),
    )
    .unwrap();
    let f = render(&source);
    assert_eq!(at(&f, 12, 12), [0, 0, 255, 255]);
    assert_eq!(at(&f, 17, 17), [0, 255, 0, 255]);
    assert_eq!(at(&f, 4, 4), [255, 0, 0, 255]);
}
#[test]
fn hidden_and_zero_opacity_markers_force_direct_parent_isolation() {
    for hidden in [true, false] {
        let mut s = Scene::new();
        s.insert(
            rect("external", 0., 0., 40., 40., Color::rgb(255, 0, 0)),
            None,
            None,
        )
        .unwrap();
        s.insert(DisplayObject::new("g", display::Group::new()), None, None)
            .unwrap();
        let mut blue = rect("blue", 0., 0., 40., 40., Color::rgb(0, 0, 255));
        blue.style.blend_mode = BlendMode::Multiply;
        insert(&mut s, blue, "g");
        let mut e = eraser(EraserMode::Pixel);
        if hidden {
            e.visible = false;
        } else {
            e.style.opacity = 0.;
        }
        insert(&mut s, e, "g");
        let units = scene_units(&s, CompositeLimits::default()).unwrap();
        assert!(
            units
                .iter()
                .any(|u| matches!(u, SceneUnit::Domain(d) if d.root_id() == &"g".into()))
        );
        let source = owned_canvas(
            &s,
            CanvasBackdrop {
                color: Color::WHITE,
                grid: None,
            },
            CompositeLimits::default(),
        )
        .unwrap();
        assert_eq!(at(&render(&source), 20, 20), [0, 0, 255, 255]);
    }
}
#[test]
fn path_erasers_ignore_positive_opacity_fill_alpha_strokes_and_effects() {
    let mut expected = None;
    for opacity in [0.01, 0.5, 1.] {
        for fill in [
            None,
            Some(Color::rgba(255, 255, 255, 1).into()),
            Some(Color::WHITE.into()),
        ] {
            let mut s = scene();
            let mut e = eraser(EraserMode::Path);
            e.style.fill = fill;
            e.style.opacity = opacity;
            e.style.stroke = Some(Stroke::new(Color::WHITE, 12.));
            e.style.shadow = Some(gpui_flow_scene::Shadow::new(8., 0., 4., Color::WHITE));
            e.style.inner_shadow_layers =
                vec![gpui_flow_scene::Shadow::new(4., 0., 2., Color::WHITE)];
            insert(&mut s, e, "g");
            let f = render(&domain(&s));
            assert_eq!(at(&f, 12, 12), [0; 4]);
            assert_eq!(at(&f, 8, 20), [255, 0, 0, 255]);
            if let Some(expected) = &expected {
                assert_eq!(&f.premul_rgba, expected);
            } else {
                expected = Some(f.premul_rgba);
            }
        }
    }
    for hidden in [true, false] {
        let mut s = scene();
        let mut e = eraser(EraserMode::Path);
        if hidden {
            e.visible = false;
        } else {
            e.style.opacity = 0.;
        }
        insert(&mut s, e, "g");
        assert_eq!(at(&render(&domain(&s)), 20, 20), [255, 0, 0, 255]);
    }
}
#[test]
fn path_erasers_preserve_winding_holes_implicit_closure_and_straight_line_zero_area() {
    for rule in [FillRule::EvenOdd, FillRule::NonZero] {
        let mut s = scene();
        let path = display::Path::from_svg("M5 5H35V35H5Z M10 10H30V30H10Z").unwrap();
        insert(
            &mut s,
            DisplayObject::new("erase", path)
                .with_eraser(EraserMode::Path)
                .with_style(Style {
                    fill: None,
                    fill_rule: rule,
                    ..Default::default()
                }),
            "g",
        );
        let f = render(&domain(&s));
        assert_eq!(at(&f, 7, 7), [0; 4]);
        assert_eq!(
            at(&f, 20, 20),
            if rule == FillRule::EvenOdd {
                [255, 0, 0, 255]
            } else {
                [0; 4]
            }
        );
    }
    let mut s = scene();
    insert(
        &mut s,
        DisplayObject::new("erase", display::Path::from_svg("M5 5L35 5L5 35").unwrap())
            .with_eraser(EraserMode::Path),
        "g",
    );
    let f = render(&domain(&s));
    assert_eq!(at(&f, 10, 10), [0; 4]);
    assert_eq!(at(&f, 30, 30), [255, 0, 0, 255]);
    s.update_object(&"erase".into(), |o| {
        o.shape = display::Line::new(Point::new(0., 20.), Point::new(40., 20.)).into()
    })
    .unwrap();
    assert_eq!(at(&render(&domain(&s)), 20, 20), [255, 0, 0, 255]);
}
#[test]
fn affine_erasers_receive_inherited_rounded_clip_once() {
    for mode in [EraserMode::Pixel, EraserMode::Path] {
        let mut s = scene();
        s.update_object(&"g".into(), |o| o.style.blend_mode = BlendMode::Normal)
            .unwrap();
        insert(
            &mut s,
            DisplayObject::new("frame", display::Frame::new(20., 20.).rounded(9.))
                .with_style(Style {
                    fill: None,
                    ..Default::default()
                })
                .with_transform(Affine::translation(10., 10.)),
            "g",
        );
        // The direct parent includes a target so the isolated frame has positive ink.
        insert(
            &mut s,
            rect("inside", 0., 0., 20., 20., Color::rgb(0, 255, 0)),
            "frame",
        );
        let mut e = rect("erase", 0., 0., 60., 60., Color::WHITE).with_eraser(mode);
        e.transform = Affine::new(-1., 0., 0.2, 1., 30., -10.);
        insert(&mut s, e, "frame");
        let f = render(&domain(&s));
        assert_eq!(at(&f, 20, 20), [255, 0, 0, 255]);
        assert_eq!(at(&f, 10, 10), [255, 0, 0, 255]);
        assert_eq!(at(&f, 5, 5), [255, 0, 0, 255]);
    }
}
fn masked_scene(mode: EraserMode, mask_opacity: f32, mask_alpha: u8, blend: BlendMode) -> Scene {
    let mut s = Scene::new();
    s.insert(DisplayObject::new("g", display::Group::new()), None, None)
        .unwrap();
    let mut m = rect(
        "mask",
        0.,
        0.,
        40.,
        40.,
        Color::rgba(255, 255, 255, mask_alpha),
    )
    .with_mask(MaskMode::Alpha);
    m.style.opacity = mask_opacity;
    insert(&mut s, m, "g");
    insert(
        &mut s,
        rect("red", 0., 0., 40., 40., Color::rgb(255, 0, 0)),
        "g",
    );
    let mut e = eraser(mode);
    e.style.blend_mode = blend;
    insert(&mut s, e, "g");
    s
}
#[test]
fn mask_flush_uses_effective_pixel_operation_and_mask_object_opacity() {
    for alpha in [255, 128] {
        let s = masked_scene(EraserMode::Pixel, 1., alpha, BlendMode::PassThrough);
        assert_eq!(at(&render(&domain(&s)), 20, 20), [alpha, 0, 0, alpha]);
        let s = masked_scene(EraserMode::Pixel, 0.5, alpha, BlendMode::PassThrough);
        let f = render(&domain(&s));
        assert_eq!(at(&f, 20, 20), [0; 4]);
        let expected = if alpha == 255 { 128 } else { 64 };
        assert_eq!(at(&f, 5, 5), [expected, 0, 0, expected]);
    }
}
#[test]
fn path_mask_runs_clear_unflushed_content_but_explicit_blend_flushes_at_opacity_one() {
    for (opacity, blend, erased) in [
        (1., BlendMode::PassThrough, true),
        (0.5, BlendMode::PassThrough, true),
        (1., BlendMode::Multiply, false),
        (0.5, BlendMode::Multiply, true),
    ] {
        let s = masked_scene(EraserMode::Path, opacity, 255, blend);
        assert_eq!(
            at(&render(&domain(&s)), 20, 20),
            if erased { [0; 4] } else { [255, 0, 0, 255] }
        );
    }
}
#[test]
fn eraser_mask_has_zero_coverage_and_next_mask_starts_fresh() {
    for mode in [EraserMode::Pixel, EraserMode::Path] {
        let mut s = masked_scene(mode, 1., 255, BlendMode::PassThrough);
        s.update_object(&"mask".into(), |o| o.eraser = mode)
            .unwrap();
        assert_eq!(at(&render(&domain(&s)), 5, 5), [0; 4]);
        insert(
            &mut s,
            rect("mask2", 0., 0., 40., 40., Color::WHITE).with_mask(MaskMode::Alpha),
            "g",
        );
        insert(
            &mut s,
            rect("blue", 0., 0., 40., 40., Color::rgb(0, 0, 255)),
            "g",
        );
        assert_eq!(at(&render(&domain(&s)), 20, 20), [0, 0, 255, 255]);
    }
}
#[test]
fn nested_eraser_group_masks_its_completed_result_normally() {
    let mut s = masked_scene(EraserMode::Pixel, 1., 255, BlendMode::PassThrough);
    s.remove(&"erase".into()).unwrap();
    insert(
        &mut s,
        DisplayObject::new("inner", display::Group::new()),
        "g",
    );
    insert(
        &mut s,
        rect("blue", 0., 0., 40., 40., Color::rgb(0, 0, 255)),
        "inner",
    );
    insert(&mut s, eraser(EraserMode::Pixel), "inner");
    let f = render(&domain(&s));
    assert_eq!(at(&f, 20, 20), [255, 0, 0, 255]);
    assert_eq!(at(&f, 5, 5), [0, 0, 255, 255]);
}
#[test]
fn owned_root_erasers_reveal_background_grid_and_force_document_isolation() {
    let mut s = Scene::new();
    s.insert(
        rect("red", 0., 0., 40., 40., Color::rgb(255, 0, 0)),
        None,
        None,
    )
    .unwrap();
    s.insert(
        rect("erase", 0., 0., 30., 30., Color::WHITE).with_eraser(EraserMode::Pixel),
        None,
        None,
    )
    .unwrap();
    let d = owned_canvas(
        &s,
        CanvasBackdrop {
            color: Color::rgb(0, 0, 255),
            grid: Some(CanvasGrid {
                color: Color::WHITE,
                spacing: 10.,
                offset: Point::ZERO,
            }),
        },
        CompositeLimits::default(),
    )
    .unwrap();
    let f = render(&d);
    assert_eq!(at(&f, 5, 5), [0, 0, 255, 255]);
    assert_eq!(at(&f, 10, 10), [255, 255, 255, 255]);
    assert_eq!(at(&f, 35, 35), [255, 0, 0, 255]);
    s.update_object(&"erase".into(), |o| o.visible = false)
        .unwrap();
    s.update_object(&"red".into(), |o| o.style.blend_mode = BlendMode::Multiply)
        .unwrap();
    let d = owned_canvas(
        &s,
        CanvasBackdrop {
            color: Color::rgb(0, 0, 255),
            grid: None,
        },
        CompositeLimits::default(),
    )
    .unwrap();
    assert_eq!(at(&render(&d), 5, 5), [255, 0, 0, 255]);
}
#[test]
fn erasers_reject_external_backdrops_unsupported_paths_svg_and_bare_canvas_capture() {
    let mut s = Scene::new();
    s.insert(eraser(EraserMode::Pixel), None, None).unwrap();
    assert!(
        matches!(&scene_units(&s, CompositeLimits::default()).unwrap()[0], SceneUnit::Rejected(i) if i.error == CompositeError::UnsupportedExternalBackdrop)
    );
    assert_eq!(
        capture_subtree(&s, &"erase".into(), CompositeLimits::default()).unwrap_err(),
        CompositeError::UnsupportedExternalBackdrop
    );
    assert!(
        owned_canvas(
            &s,
            CanvasBackdrop {
                color: Color::TRANSPARENT,
                grid: None
            },
            CompositeLimits::default()
        )
        .is_err()
    );
    let request = SceneExportRequest {
        world_bounds: Bounds::new(Point::ZERO, Size::new(40., 40.)),
        scale: 1.,
        pixel_ratio: 1.,
        background: Color::TRANSPARENT,
        format: SceneExportFormat::Svg,
    };
    assert!(
        matches!(SceneExportPlan::new(&s, request, CompositeLimits::default()), Err(CompositeError::UnsupportedShape(message)) if message == "SVG erasers")
    );
    let shapes: Vec<Shape> = vec![
        display::Group::new().into(),
        display::Box::new(20., 20.).into(),
        display::Frame::new(20., 20.).into(),
        display::Text::new("x", Size::new(20., 20.)).into(),
        display::Image::new("missing", Size::new(20., 20.)).into(),
        display::Canvas::new(20., 20.).into(),
        display::Custom::new(
            "custom",
            Bounds::new(Point::ZERO, Size::new(20., 20.)),
            vec![],
        )
        .into(),
    ];
    for shape in shapes {
        let mut s = scene();
        insert(
            &mut s,
            DisplayObject::new("erase", shape).with_eraser(EraserMode::Path),
            "g",
        );
        assert!(
            matches!(&scene_units(&s, CompositeLimits::default()).unwrap()[0], SceneUnit::Rejected(i) if i.error == CompositeError::UnsupportedPathEraser("erase".into()))
        );
    }
}
#[test]
fn pixel_erasers_use_actual_image_canvas_and_text_alpha() {
    for producer in 0..3 {
        let mut s = scene();
        let shape: Shape = match producer {
            0 => display::Image::new("memory", Size::new(20., 20.)).into(),
            1 => display::Canvas::new(20., 20.).into(),
            _ => display::Text::new("glyph fixture", Size::new(20., 20.)).into(),
        };
        let mut e = DisplayObject::new("erase", shape).with_eraser(EraserMode::Pixel);
        e.style.fill = None;
        insert(&mut s, e, "g");
        let mut rgba = [0, 0, 0, 0].repeat(400);
        for y in 0..20 {
            for x in 0..10 {
                rgba[(y * 20 + x) * 4..][..4].copy_from_slice(&[0, 128, 0, 128]);
            }
        }
        let image = Arc::new(
            StaticImage::from_premultiplied_rgba(20, 20, rgba, CompositeLimits::default()).unwrap(),
        );
        let mut images = ImageInputs::default();
        let mut texts = TextInputs::default();
        if producer == 2 {
            texts.insert(
                "erase".into(),
                Ok(Some(Arc::new(TextImage {
                    image,
                    local_bounds: Bounds::new(Point::ZERO, Size::new(20., 20.)),
                }))),
            );
        } else {
            images.insert("erase".into(), Ok(image));
        }
        let f = rasterize_with_inputs(
            &domain(&s),
            &images,
            &texts,
            viewport(),
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        assert_eq!(at(&f, 5, 5), [127, 0, 0, 127]);
        assert_eq!(at(&f, 15, 5), [255, 0, 0, 255]);
    }
}
#[test]
fn eraser_operation_bounds_do_not_inflate_allocations_and_mode_changes_invalidate_placement() {
    let mut s = scene();
    let mut e = eraser(EraserMode::Pixel);
    e.shape = display::Rect::new(10_000., 10_000.).into();
    e.transform = Affine::translation(200_000., 200_000.);
    insert(&mut s, e, "g");
    let first = domain(&s);
    let f = render(&first);
    assert_eq!((f.width, f.height), (41, 41));
    s.update_object(&"erase".into(), |o| o.eraser = EraserMode::Path)
        .unwrap();
    let second = domain(&s);
    assert!(!first.placement_eq(&second));
    assert_eq!(render(&second).premul_rgba, f.premul_rgba);
}
#[test]
fn eraser_admission_rejects_before_writes_and_restores_retained_ledger() {
    for mode in [EraserMode::Pixel, EraserMode::Path] {
        let mut s = scene();
        insert(&mut s, eraser(mode), "g");
        let d = domain(&s);
        let mut baseline = PixelBudget::new(CompositeLimits::default(), 17).unwrap();
        let f = rasterize(&d, viewport(), &mut baseline, &AtomicBool::new(false))
            .unwrap()
            .unwrap();
        let peak = baseline.peak_bytes();
        let work = baseline.surface_writes();
        baseline.discard_frame(f);
        assert_eq!(baseline.retained_bytes(), 17);
        for limits in [
            CompositeLimits {
                max_surface_bytes: peak - 1,
                ..Default::default()
            },
            CompositeLimits {
                max_surface_writes: work - 1,
                ..Default::default()
            },
            CompositeLimits {
                max_depth: 0,
                ..Default::default()
            },
        ] {
            let mut budget = PixelBudget::new(limits, 17).unwrap();
            assert!(rasterize(&d, viewport(), &mut budget, &AtomicBool::new(false)).is_err());
            assert_eq!(budget.retained_bytes(), 17);
            assert_eq!(
                budget.surface_writes(),
                0,
                "admission must run before pixel work"
            );
        }
        let mut budget = PixelBudget::new(CompositeLimits::default(), 17).unwrap();
        assert!(matches!(
            rasterize(&d, viewport(), &mut budget, &AtomicBool::new(true)),
            Err(CompositeError::Superseded)
        ));
        assert_eq!(budget.retained_bytes(), 17);
    }
}
#[test]
fn raster_export_reveals_explicit_alpha_background_after_root_erasure() {
    let mut s = Scene::new();
    s.insert(
        rect("red", 0., 0., 40., 40., Color::rgb(255, 0, 0)),
        None,
        None,
    )
    .unwrap();
    s.insert(eraser(EraserMode::Path), None, None).unwrap();
    let request = SceneExportRequest {
        world_bounds: Bounds::new(Point::ZERO, Size::new(40., 40.)),
        scale: 1.,
        pixel_ratio: 1.,
        background: Color::rgba(0, 0, 128, 128),
        format: SceneExportFormat::Png,
    };
    let plan = SceneExportPlan::new(&s, request, CompositeLimits::default()).unwrap();
    let f = rasterize(
        plan.source(),
        plan.viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(at(&f, 20, 20), [0, 0, 64, 128]);
    assert_eq!(at(&f, 5, 5), [255, 0, 0, 255]);
}

#[test]
fn pixel_containers_erase_complete_outer_and_inner_shadow_alpha_at_full_and_half_opacity() {
    use gpui_flow_scene::Shadow;
    for shape in [
        Shape::from(display::Group::new()),
        display::Box::new(30., 30.).into(),
        display::Frame::new(30., 30.).into(),
    ] {
        for inner in [false, true] {
            for opacity in [1., 0.5] {
                let mut s = scene();
                let mut e =
                    DisplayObject::new("erase", shape.clone()).with_eraser(EraserMode::Pixel);
                e.style.fill = None;
                e.style.opacity = opacity;
                let effect = Shadow::new(10., 0., 0., Color::WHITE);
                if inner {
                    e.style.inner_shadow_layers.push(effect);
                } else {
                    e.style.shadow = Some(effect);
                }
                insert(&mut s, e, "g");
                insert(
                    &mut s,
                    rect("ink", 5., 5., 10., 10., Color::rgba(255, 255, 255, 128)),
                    "erase",
                );
                let frame = render(&domain(&s));
                // Independent ordinary rendering supplies the full effect alpha;
                // the assertion checks that erase applies object opacity once.
                let mut reference = s.clone();
                reference.remove(&"red".into()).unwrap();
                reference
                    .update_object(&"erase".into(), |o| {
                        o.eraser = EraserMode::None;
                        o.style.opacity = 1.;
                    })
                    .unwrap();
                let alpha = render(&domain(&reference));
                for (x, y) in [(8, 8), (18, 8), (28, 8)] {
                    let a = at(&alpha, x, y)[3];
                    let expected = (255. - f64::from(a) * f64::from(opacity)).round() as u8;
                    assert_eq!(
                        at(&frame, x, y),
                        [expected, 0, 0, expected],
                        "{} inner={inner} opacity={opacity} at {x},{y}",
                        shape.tag()
                    );
                }
                assert!(
                    at(&alpha, 8, 8)[3] > 128 || !inner,
                    "inner effect must alter alpha"
                );
                assert!(
                    at(&alpha, 18, 8)[3] > 0 || inner,
                    "outer effect must extend alpha"
                );
            }
        }
    }
}

use super::*;
use gpui_flow_scene::{display, *};
use std::{io::Cursor, sync::atomic::AtomicBool};
fn scene(policy: StrokeScaleFixed) -> Scene {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("g", display::Group::new()).with_style(Style {
            blend_mode: BlendMode::Normal,
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new(
            "line",
            display::Line::new(Point::new(20., 10.), Point::new(20., 70.)),
        )
        .with_style(Style {
            fill: None,
            stroke: Some(
                Stroke::new(Color::BLACK, 8.)
                    .with_scale_fixed(policy)
                    .with_cap(StrokeCap::Butt),
            ),
            ..Default::default()
        }),
        Some("g".into()),
        None,
    )
    .unwrap();
    s
}
fn source(s: &Scene) -> DomainSource {
    match scene_units(s, CompositeLimits::default())
        .unwrap()
        .remove(0)
    {
        SceneUnit::Domain(d) => *d,
        _ => panic!("domain"),
    }
}
fn render(source: &DomainSource, zoom: f32, dpr: f32) -> CpuFrame {
    let bounds = Bounds::new(Point::ZERO, Size::new(180., 180.));
    rasterize(
        source,
        RasterViewport {
            world_to_screen: Affine::scale(zoom, zoom),
            canvas_bounds: bounds,
            content_mask: bounds,
            device_scale: dpr,
        },
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}
fn width(frame: &CpuFrame, logical_y: f32, dpr: f32) -> f64 {
    let y = ((logical_y - frame.screen_bounds.origin.y) * dpr).round() as u32;
    (0..frame.width)
        .map(|x| frame.premul_rgba[((y * frame.width + x) * 4 + 3) as usize] as f64 / 255.)
        .sum()
}
#[test]
fn scale_fixed_cpu_uses_logical_zoom_and_dpr_only_changes_physical_width() {
    for policy in [StrokeScaleFixed::Fixed, StrokeScaleFixed::Threshold(1.)] {
        let s = scene(policy);
        let raw = source(&s);
        for zoom in [0.5, 1., 2.] {
            let resolved = source(&s.resolved_for_view(zoom).unwrap());
            for dpr in [1., 1.5, 2.] {
                let a = render(&raw, zoom, dpr);
                let b = render(&resolved, zoom, dpr);
                assert_eq!(a.premul_rgba, b.premul_rgba);
                assert_eq!(a.screen_bounds, b.screen_bounds);
                let expected = 8.
                    * dpr
                    * if policy == StrokeScaleFixed::Fixed {
                        1.
                    } else {
                        zoom.min(1.)
                    };
                assert!(
                    (width(&a, 40. * zoom, dpr) - expected as f64).abs() < 0.05,
                    "{policy:?} zoom {zoom} DPR {dpr}"
                );
            }
        }
    }
}
#[test]
fn scale_fixed_export_separates_logical_scale_and_pixel_ratio_and_rejects_svg_canvas_capture() {
    let s = scene(StrokeScaleFixed::Fixed);
    let authored = s.to_json().unwrap();
    let request = SceneExportRequest {
        world_bounds: Bounds::new(Point::ZERO, Size::new(80., 80.)),
        scale: 2.,
        pixel_ratio: 2.,
        background: Color::TRANSPARENT,
        format: SceneExportFormat::Png,
    };
    let plan = SceneExportPlan::new(&s, request, CompositeLimits::default()).unwrap();
    let encoded = plan
        .encode_with_inputs(
            &ImageInputs::default(),
            &TextInputs::default(),
            SceneExportPreparation::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
    let mut reader = png::Decoder::new(Cursor::new(encoded.bytes))
        .read_info()
        .unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut bytes).unwrap();
    let mass: f64 = (0..info.width)
        .map(|x| bytes[((160 * info.width + x) * 4 + 3) as usize] as f64 / 255.)
        .sum();
    assert!((mass - 16.).abs() < 0.05, "physical width {mass}");
    assert_eq!(s.to_json().unwrap(), authored);
    assert!(matches!(
        SceneExportPlan::new(
            &s,
            SceneExportRequest {
                format: SceneExportFormat::Svg,
                ..request
            },
            CompositeLimits::default()
        ),
        Err(CompositeError::UnsupportedScaleFixed(_))
    ));
    assert!(matches!(
        capture_subtree(&s, &"g".into(), CompositeLimits::default()),
        Err(CompositeError::UnsupportedScaleFixed(_))
    ));
}
#[test]
fn scale_fixed_cpu_preserves_ancestor_provenance_and_rejects_unadmitted_camera() {
    let mut s = scene(StrokeScaleFixed::Fixed);
    s.update_object(&"g".into(), |o| o.transform = Affine::scale(2., 1.))
        .unwrap();
    s.update_object(&"line".into(), |o| {
        o.transform = Affine::translation(80., 0.) * Affine::rotation(90.)
    })
    .unwrap();
    let raw = source(&s);
    let resolved = source(&s.resolved_for_view(2.).unwrap());
    assert_eq!(
        render(&raw, 2., 1.).premul_rgba,
        render(&resolved, 2., 1.).premul_rgba
    );
    let bounds = Bounds::new(Point::ZERO, Size::new(180., 180.));
    let result = rasterize(
        &raw,
        RasterViewport {
            world_to_screen: Affine::scale(2., 1.),
            canvas_bounds: bounds,
            content_mask: bounds,
            device_scale: 1.,
        },
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    );
    assert!(matches!(
        result,
        Err(CompositeError::UnsupportedScaleFixed(_))
    ));
}

#[test]
fn scale_fixed_derived_export_and_cross_camera_cpu_reuse_are_explicitly_rejected() {
    let s = scene(StrokeScaleFixed::Fixed)
        .resolved_for_view(2.)
        .unwrap();
    let bounds = Bounds::new(Point::ZERO, Size::new(100., 100.));
    assert!(matches!(
        SceneExportPlan::new(
            &s,
            SceneExportRequest {
                world_bounds: bounds,
                scale: 1.,
                pixel_ratio: 1.,
                background: Color::TRANSPARENT,
                format: SceneExportFormat::Png
            },
            CompositeLimits::default()
        ),
        Err(CompositeError::UnsupportedScaleFixed(_))
    ));
    let source = source(&s);
    assert!(matches!(
        rasterize(
            &source,
            RasterViewport {
                world_to_screen: Affine::IDENTITY,
                canvas_bounds: bounds,
                content_mask: bounds,
                device_scale: 1.
            },
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false)
        ),
        Err(CompositeError::UnsupportedScaleFixed(_))
    ));
}

#[test]
fn scale_fixed_transient_subtree_context_requires_insertion_before_rendering() {
    let mut s = scene(StrokeScaleFixed::Fixed);
    s.update_object(&"g".into(), |o| o.transform = Affine::scale(2., 2.))
        .unwrap();
    let transfer = s.transfer_subtree(&"line".into()).unwrap();
    assert!(scene_units(&transfer, CompositeLimits::default()).is_err());
    assert!(matches!(
        owned_canvas(
            &transfer,
            CanvasBackdrop {
                color: Color::WHITE,
                grid: None
            },
            CompositeLimits::default()
        ),
        Err(CompositeError::UnsupportedScaleFixed(_))
    ));
}

#[test]
fn scale_fixed_cpu_retains_rotated_threshold_uncertainty_before_admission() {
    let mut s = scene(StrokeScaleFixed::Threshold(1.));
    s.update_object(&"g".into(), |o| {
        o.transform = Affine::rotation(60.) * Affine::scale(0.5, 2.)
    })
    .unwrap();
    let source = source(&s);
    let bounds = Bounds::new(Point::ZERO, Size::new(100., 100.));
    let result = rasterize(
        &source,
        RasterViewport {
            world_to_screen: Affine::scale(0.5, 0.5),
            canvas_bounds: bounds,
            content_mask: bounds,
            device_scale: 2.,
        },
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    );
    assert!(
        matches!(result,Err(CompositeError::InvalidGeometry(message)) if message.contains("ambiguous"))
    );
}

#[test]
fn scale_fixed_raw_rotated_camera_cannot_guess_a_nonunit_logical_threshold() {
    let mut s = scene(StrokeScaleFixed::Threshold(1.25));
    s.update_object(&"g".into(), |o| o.transform = Affine::scale(0.5, 1.))
        .unwrap();
    let source = source(&s);
    let bounds = Bounds::new(Point::ZERO, Size::new(100., 100.));
    let camera = Affine::rotation(4.) * Affine::scale(1.25, 1.25);
    let result = rasterize(
        &source,
        RasterViewport {
            world_to_screen: camera,
            canvas_bounds: bounds,
            content_mask: bounds,
            device_scale: 1.,
        },
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    );
    assert!(
        matches!(result,Err(CompositeError::UnsupportedScaleFixed(message)) if message.contains("camera"))
    );
    assert!(
        rasterize(
            &source,
            RasterViewport {
                world_to_screen: Affine::scale(1.25, 1.25),
                canvas_bounds: bounds,
                content_mask: bounds,
                device_scale: 1.
            },
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false)
        )
        .is_ok()
    );
}

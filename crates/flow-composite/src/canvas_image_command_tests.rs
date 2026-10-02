use super::*;
use gpui_flow_scene::{
    Affine, Bounds, Color, FillRule, Point, Size,
    display::{Canvas, Path},
};
use std::sync::Arc;
fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds {
    Bounds::new(Point::new(x, y), Size::new(w, h))
}
fn surface(smooth: bool, dpr: f32) -> CanvasSurface {
    let mut c = Canvas::new(16., 16.);
    c.smooth = smooth;
    c.pixel_ratio = dpr;
    CanvasSurface::new(c, CompositeLimits::default()).unwrap()
}
fn inputs() -> ImageInputs {
    // RGBA rows: red, green / blue, transparent.
    let image = StaticImage::from_premultiplied_rgba(
        2,
        2,
        vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 0, 0, 0],
        CompositeLimits::default(),
    )
    .unwrap();
    let mut inputs = ImageInputs::default();
    inputs.insert("pixels".into(), Ok(Arc::new(image)));
    inputs
}
fn draw(source: Option<Bounds>, destination: Bounds) -> CanvasCommand {
    CanvasCommand::DrawImage {
        image: "pixels".into(),
        source,
        destination,
    }
}
fn pixel(s: &CanvasSurface, x: usize, y: usize) -> [u8; 4] {
    let p = s.snapshot();
    p.premultiplied_rgba()[(y * p.width() as usize + x) * 4..][..4]
        .try_into()
        .unwrap()
}
fn fill() -> CanvasCommand {
    CanvasCommand::FillPath {
        path: Path::from_svg("M 0 0 L 16 0 L 16 16 L 0 16 Z").unwrap(),
        paint: Color::WHITE.into(),
        rule: FillRule::NonZero,
    }
}
#[test]
fn canvas_image_batch_mixes_vector_and_image_draws_in_order_with_per_draw_alpha() {
    let mut s = surface(false, 1.);
    let old = s.snapshot();
    s.draw_commands_with_images(
        &[
            fill(),
            CanvasCommand::SetOpacity(0.5),
            draw(None, bounds(2., 2., 8., 8.)),
            draw(None, bounds(2., 2., 8., 8.)),
        ],
        &inputs(),
    )
    .unwrap();
    assert_eq!(s.generation(), 2);
    assert_eq!(pixel(&s, 3, 3), [255, 64, 64, 255]);
    assert_eq!(pixel(&s, 8, 8), [255; 4]);
    assert!(old.premultiplied_rgba().iter().all(|v| *v == 0));
}
#[test]
fn canvas_image_source_crop_uses_physical_pixels_and_destination_logical_units() {
    let mut s = surface(false, 2.);
    s.draw_commands_with_images(
        &[draw(Some(bounds(1., 0., 1., 1.)), bounds(2., 3., 4., 5.))],
        &inputs(),
    )
    .unwrap();
    assert_eq!(pixel(&s, 5, 7), [0, 255, 0, 255]);
    assert_eq!(pixel(&s, 10, 14), [0, 255, 0, 255]);
    assert_eq!(pixel(&s, 3, 7), [0; 4]);
    assert_eq!(pixel(&s, 12, 7), [0; 4]);
    assert_eq!(pixel(&s, 5, 16), [0; 4]);
}
#[test]
fn canvas_image_outside_source_crop_preserves_mapping_with_transparent_margins() {
    let mut s = surface(false, 1.);
    s.draw_commands_with_images(
        &[draw(Some(bounds(-1., -1., 2., 2.)), bounds(0., 0., 8., 8.))],
        &inputs(),
    )
    .unwrap();
    assert_eq!(pixel(&s, 2, 2), [0; 4]);
    assert_eq!(pixel(&s, 6, 6), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 9, 6), [0; 4]);
}
#[test]
fn canvas_image_affine_and_frozen_clip_restore_share_the_existing_pipeline() {
    let mut s = surface(false, 1.);
    s.draw_commands_with_images(
        &[
            CanvasCommand::Save,
            CanvasCommand::ClipPath {
                path: Path::from_svg("M 2 2 L 6 2 L 6 10 L 2 10 Z").unwrap(),
                rule: FillRule::NonZero,
            },
            CanvasCommand::SetTransform(Affine::translation(10., 2.) * Affine::scale(-1., 1.)),
            draw(None, bounds(0., 0., 8., 8.)),
            CanvasCommand::Restore,
            draw(Some(bounds(0., 1., 1., 1.)), bounds(10., 10., 4., 4.)),
        ],
        &inputs(),
    )
    .unwrap();
    assert_eq!(pixel(&s, 3, 3), [0, 255, 0, 255]);
    assert_eq!(pixel(&s, 8, 3), [0; 4]);
    assert_eq!(pixel(&s, 11, 11), [0, 0, 255, 255]);
    s.draw_commands_with_images(&[draw(None, bounds(10., 0., 4., 4.))], &inputs())
        .unwrap();
    assert_eq!(pixel(&s, 10, 0), [255, 0, 0, 255]);
}
#[test]
fn canvas_image_late_missing_or_failed_input_preserves_pixels_and_generation() {
    let mut s = surface(false, 1.);
    s.draw_commands(&[fill()]).unwrap();
    let old = s.snapshot();
    let generation = s.generation();
    let mut commands = vec![
        draw(None, bounds(0., 0., 8., 8.)),
        CanvasCommand::DrawImage {
            image: "missing".into(),
            source: None,
            destination: bounds(0., 0., 8., 8.),
        },
    ];
    assert_eq!(
        s.draw_commands_with_images(&commands, &inputs()),
        Err(CompositeError::ImagePending("missing".into()))
    );
    let mut failed = inputs();
    failed.insert("missing".into(), Err(CompositeError::InvalidImage));
    assert_eq!(
        s.draw_commands_with_images(&commands, &failed),
        Err(CompositeError::InvalidImage)
    );
    commands.pop();
    commands.push(CanvasCommand::Restore);
    assert!(s.draw_commands_with_images(&commands, &inputs()).is_err());
    assert_eq!(s.generation(), generation);
    assert!(Arc::ptr_eq(&old, &s.snapshot()));
    assert!(
        s.draw_commands(&[draw(None, bounds(0., 0., 8., 8.))])
            .is_err()
    );
}
#[test]
fn canvas_image_invalid_rectangles_and_transforms_are_atomic() {
    let mut s = surface(false, 1.);
    let old = s.snapshot();
    for bad in [
        bounds(0., 0., 0., 1.),
        bounds(0., 0., -1., 1.),
        bounds(f32::NAN, 0., 1., 1.),
        bounds(0., 0., f32::INFINITY, 1.),
        bounds(0., 0., 1_000_001., 1.),
    ] {
        for command in [draw(Some(bad), bounds(0., 0., 8., 8.)), draw(None, bad)] {
            assert!(
                s.draw_commands_with_images(&[fill(), command], &inputs())
                    .is_err()
            );
            assert!(Arc::ptr_eq(&old, &s.snapshot()));
        }
    }
    assert!(
        s.draw_commands_with_images(
            &[
                CanvasCommand::Transform(Affine::scale(0., 1.)),
                draw(None, bounds(0., 0., 8., 8.))
            ],
            &inputs()
        )
        .is_err()
    );
}
#[test]
fn canvas_image_smoothing_and_direct_image_draw_share_physical_mapping() {
    let inputs = inputs();
    let image = inputs.get(&"pixels".into()).unwrap();
    for smooth in [false, true] {
        let mut batch = surface(smooth, 2.);
        let mut direct = surface(smooth, 2.);
        batch
            .draw_commands_with_images(&[draw(None, bounds(1., 1., 10., 10.))], &inputs)
            .unwrap();
        direct
            .draw_image(&image, bounds(1., 1., 10., 10.), 1.)
            .unwrap();
        assert_eq!(
            batch.snapshot().premultiplied_rgba(),
            direct.snapshot().premultiplied_rgba()
        );
    }
    let mut nearest = surface(false, 1.);
    let mut smooth = surface(true, 1.);
    nearest
        .draw_commands_with_images(&[draw(None, bounds(0., 0., 10., 10.))], &inputs)
        .unwrap();
    smooth
        .draw_commands_with_images(&[draw(None, bounds(0., 0., 10., 10.))], &inputs)
        .unwrap();
    assert_ne!(
        nearest.snapshot().premultiplied_rgba(),
        smooth.snapshot().premultiplied_rgba()
    );
}
#[test]
fn canvas_image_crop_consumes_depth_nodes_and_mask_budget_before_publish() {
    for limits in [
        CompositeLimits {
            max_depth: 1,
            ..Default::default()
        },
        CompositeLimits {
            max_nodes: 3,
            ..Default::default()
        },
        CompositeLimits {
            max_mask_runs: 0,
            ..Default::default()
        },
        CompositeLimits {
            max_total_points: 9,
            ..Default::default()
        },
    ] {
        let mut s = CanvasSurface::new(Canvas::new(16., 16.), limits).unwrap();
        let old = s.snapshot();
        assert!(
            s.draw_commands_with_images(
                &[draw(Some(bounds(0., 0., 1., 1.)), bounds(0., 0., 8., 8.))],
                &inputs()
            )
            .is_err()
        );
        assert!(Arc::ptr_eq(&old, &s.snapshot()));
    }
}
#[test]
fn canvas_image_shared_input_aliases_are_deduplicated_and_unused_errors_ignored() {
    let mut inputs = inputs();
    let shared = inputs.get(&"pixels".into()).unwrap();
    let weak = Arc::downgrade(&shared);
    inputs.insert("alias".into(), Ok(shared));
    inputs.insert("unused".into(), Err(CompositeError::InvalidImage));
    assert_eq!(inputs.byte_len(), 16);
    let mut s = surface(false, 1.);
    s.draw_commands_with_images(
        &[
            draw(None, bounds(0., 0., 8., 8.)),
            CanvasCommand::DrawImage {
                image: "alias".into(),
                source: None,
                destination: bounds(8., 8., 8., 8.),
            },
        ],
        &inputs,
    )
    .unwrap();
    drop(inputs);
    assert!(weak.upgrade().is_none());
    assert_eq!(pixel(&s, 9, 9), [255, 0, 0, 255]);
}

#[test]
fn canvas_image_input_limits_and_work_budget_reject_before_publication() {
    let mut data = inputs();
    data.insert(
        "large".into(),
        Ok(Arc::new(
            StaticImage::from_premultiplied_rgba(
                32,
                32,
                vec![255; 32 * 32 * 4],
                Default::default(),
            )
            .unwrap(),
        )),
    );
    for (limits, command) in [
        (
            CompositeLimits {
                max_dimension: 16,
                ..Default::default()
            },
            CanvasCommand::DrawImage {
                image: "large".into(),
                source: None,
                destination: bounds(0., 0., 8., 8.),
            },
        ),
        (
            CompositeLimits {
                max_surface_writes: 800,
                ..Default::default()
            },
            draw(None, bounds(0., 0., 16., 16.)),
        ),
        (
            CompositeLimits {
                max_surface_bytes: 2300,
                ..Default::default()
            },
            draw(None, bounds(0., 0., 16., 16.)),
        ),
        (
            CompositeLimits::default(),
            CanvasCommand::DrawImage {
                image: "a".repeat(513).into(),
                source: None,
                destination: bounds(0., 0., 8., 8.),
            },
        ),
    ] {
        let mut s = CanvasSurface::new(Canvas::new(16., 16.), limits).unwrap();
        let old = s.snapshot();
        assert!(
            s.draw_commands_with_images(&[command], &data).is_err(),
            "limits={limits:?}"
        );
        assert!(Arc::ptr_eq(&old, &s.snapshot()));
    }
}

#[test]
fn canvas_image_can_draw_its_own_immutable_snapshot_without_feedback_or_aliasing() {
    let mut s = surface(false, 1.);
    s.fill_rect(bounds(0., 0., 4., 4.), Color::hex(0xff0000))
        .unwrap();
    let old = s.snapshot();
    let mut data = ImageInputs::default();
    data.insert("pixels".into(), Ok(old.clone()));
    s.draw_commands_with_images(&[draw(None, bounds(4., 4., 16., 16.))], &data)
        .unwrap();
    assert_eq!(pixel(&s, 5, 5), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 10, 10), [0; 4]);
    assert_eq!(&old.premultiplied_rgba()[(5 * 16 + 5) * 4..][..4], &[0; 4]);
    s.clear().unwrap();
    assert_eq!(&old.premultiplied_rgba()[..4], &[255, 0, 0, 255]);
}

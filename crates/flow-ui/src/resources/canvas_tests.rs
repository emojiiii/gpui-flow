use super::*;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{Bounds, Color, DisplayObject, Point, Scene, Size, display};
fn setup() -> (SceneEngine, SceneResourceAdapter, ElementHandle) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("c", display::Canvas::new(10., 8.)),
            None,
            None,
        )
        .unwrap();
    let mut engine = SceneEngine::new(scene).unwrap();
    let handle = engine.handle(&"c".into()).unwrap();
    let adapter = SceneResourceAdapter::new(&mut engine).unwrap();
    (engine, adapter, handle)
}
fn fill(
    s: &mut gpui_flow_composite::CanvasSurface,
) -> Result<(), gpui_flow_composite::CompositeError> {
    s.fill_rect(
        Bounds::new(Point::ZERO, Size::new(10., 8.)),
        Color::hex(0xff0000),
    )
}
fn at(r: &SceneResourceAdapter, e: &SceneEngine, h: ElementHandle, x: u32, y: u32) -> [u8; 4] {
    let p = r.canvas_snapshot(e, h).unwrap();
    p.premultiplied_rgba()[((y * p.width() + x) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}
#[test]
fn canvas_resource_initialization_and_pixel_edits_leave_scene_history_untouched() {
    let (mut e, mut r, h) = setup();
    let before = e.scene().clone();
    let revision = e.revision();
    let events = r.edit_canvas(&mut e, h, fill).unwrap();
    assert!(
        events
            .iter()
            .any(|v| v.state == ResourceState::Loaded && v.ticket.kind == ResourceKind::Canvas)
    );
    assert_eq!(at(&r, &e, h, 2, 2), [255, 0, 0, 255]);
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), revision);
    assert!(r.images.is_empty());
    assert!(r.playbacks.is_empty());
    let saved = serde_json::to_string(e.scene()).unwrap();
    assert!(!saved.contains("pixels"));
    let mut second = SceneEngine::new(serde_json::from_str(&saved).unwrap()).unwrap();
    let mut other = SceneResourceAdapter::new(&mut second).unwrap();
    other.prepare_canvases(&mut second).unwrap();
    let h2 = second.handle(&"c".into()).unwrap();
    assert_eq!(at(&other, &second, h2, 2, 2), [0; 4]);
    assert!(r.edit_canvas(&mut second, h2, fill).is_err());
}
#[test]
fn intermediate_safe_shrinks_and_unsafe_a_b_a_cannot_resurrect_pixels() {
    let (mut e, mut r, h) = setup();
    r.edit_canvas(&mut e, h, fill).unwrap();
    let mut c = display::Canvas::new(4., 4.);
    c.safe_resize = true;
    e.set_property(
        h,
        PropertyKey::Shape,
        serde_json::to_value(Shape::from(c.clone())).unwrap(),
    )
    .unwrap();
    c.size = Size::new(10., 8.);
    e.set_property(
        h,
        PropertyKey::Shape,
        serde_json::to_value(Shape::from(c.clone())).unwrap(),
    )
    .unwrap();
    r.prepare_canvases(&mut e).unwrap();
    assert_eq!(at(&r, &e, h, 3, 3), [255, 0, 0, 255]);
    assert_eq!(at(&r, &e, h, 5, 3), [0; 4]);
    r.edit_canvas(&mut e, h, fill).unwrap();
    c.safe_resize = false;
    c.size = Size::new(4., 4.);
    e.set_property(
        h,
        PropertyKey::Shape,
        serde_json::to_value(Shape::from(c.clone())).unwrap(),
    )
    .unwrap();
    c.safe_resize = true;
    c.size = Size::new(10., 8.);
    e.set_property(
        h,
        PropertyKey::Shape,
        serde_json::to_value(Shape::from(c)).unwrap(),
    )
    .unwrap();
    r.prepare_canvases(&mut e).unwrap();
    assert_eq!(at(&r, &e, h, 3, 3), [0; 4]);
}
#[test]
fn corner_and_smooth_edits_preserve_surface_but_change_composite_observation() {
    let (mut e, mut r, h) = setup();
    r.edit_canvas(&mut e, h, fill).unwrap();
    let cfg = match &e.object(h).unwrap().shape {
        Shape::Canvas(c) => c.clone(),
        _ => unreachable!(),
    };
    let before = r.composite_canvas(&e, &"c".into(), &cfg);
    let mut next = cfg.clone().rounded(3.);
    next.smooth = false;
    e.set_property(
        h,
        PropertyKey::Shape,
        serde_json::to_value(Shape::from(next.clone())).unwrap(),
    )
    .unwrap();
    assert!(r.composite_canvas(&e, &"c".into(), &cfg).image.is_err());
    r.prepare_canvases(&mut e).unwrap();
    assert_eq!(at(&r, &e, h, 2, 2), [255, 0, 0, 255]);
    let after = r.composite_canvas(&e, &"c".into(), &next);
    assert!(!before.resource_eq(&after));
}
#[test]
fn detached_destroyed_retyped_and_disposed_canvas_release_owned_pixels_immediately() {
    for mode in 0..4 {
        let (mut e, mut r, h) = setup();
        r.edit_canvas(&mut e, h, fill).unwrap();
        let pixels = r.canvas_snapshot(&e, h).unwrap();
        let weak = Arc::downgrade(&pixels);
        drop(pixels);
        match mode {
            0 => {
                let (sub, _) = e.detach(h).unwrap();
                assert!(weak.upgrade().is_none());
                e.attach(&sub, None, None).unwrap();
                r.prepare_canvases(&mut e).unwrap();
                assert_eq!(at(&r, &e, h, 2, 2), [0; 4]);
            }
            1 => {
                e.destroy(h).unwrap();
            }
            2 => {
                e.set_property(
                    h,
                    PropertyKey::Shape,
                    serde_json::to_value(Shape::from(display::Rect::new(10., 8.))).unwrap(),
                )
                .unwrap();
            }
            _ => {
                r.dispose(&mut e).unwrap();
            }
        }
        assert!(weak.upgrade().is_none());
    }
}
#[test]
fn failed_pixel_edit_is_atomic_and_cannot_resize_outside_document_transaction() {
    let (mut e, mut r, h) = setup();
    r.edit_canvas(&mut e, h, fill).unwrap();
    let before = r.canvas_snapshot(&e, h).unwrap();
    assert!(
        r.edit_canvas(&mut e, h, |s| {
            s.clear()?;
            Err(gpui_flow_composite::CompositeError::TooLarge)
        })
        .is_err()
    );
    assert!(Arc::ptr_eq(&before, &r.canvas_snapshot(&e, h).unwrap()));
    assert!(
        r.edit_canvas(&mut e, h, |s| s
            .reconfigure(display::Canvas::new(2., 2.))
            .map(|_| ()))
            .is_err()
    );
    assert!(Arc::ptr_eq(&before, &r.canvas_snapshot(&e, h).unwrap()));
    e.destroy(h).unwrap();
    e.create("canvas", "c", serde_json::json!({}), None, None)
        .unwrap();
    assert!(r.edit_canvas(&mut e, h, fill).is_err());
}
#[test]
fn surface_capacity_fails_explicitly_and_disposal_frees_admission() {
    let (mut e, mut r, _) = setup();
    e.transaction(|scene| {
        scene.remove(&"c".into())?;
        for i in 0..5 {
            scene.insert(
                DisplayObject::new(format!("c{i}"), display::Canvas::new(2048., 2048.)),
                None,
                None,
            )?;
        }
        Ok(())
    })
    .unwrap();
    let events = r.prepare_canvases(&mut e).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|v| v.state == ResourceState::Loaded)
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|v| matches!(v.state, ResourceState::Failed(_)))
            .count(),
        3
    );
    let h = e.handle(&"c0".into()).unwrap();
    e.destroy(h).unwrap();
    r.retry_canvases(&mut e).unwrap();
    // Existing immutable sources plus candidates remain budgeted conservatively.
    assert!(
        r.canvases
            .image(
                r.tracker
                    .ticket(&e, e.handle(&"c2".into()).unwrap())
                    .unwrap()
            )
            .is_some()
    );
}

#[test]
fn physical_pixel_transactions_publish_new_snapshot_and_keep_document_unchanged() {
    use gpui_flow_composite::{CanvasPixels, CompositeLimits};
    let (mut e, mut r, h) = setup();
    r.prepare_canvases(&mut e).unwrap();
    let scene = e.scene().clone();
    let revision = e.revision();
    let old = r.canvas_snapshot(&e, h).unwrap();
    let pixels =
        CanvasPixels::from_rgba(1, 1, vec![255, 128, 0, 128], CompositeLimits::default()).unwrap();
    r.edit_canvas(&mut e, h, |s| s.write_pixels(&pixels, 2, 3).map(|_| ()))
        .unwrap();
    let new = r.canvas_snapshot(&e, h).unwrap();
    assert!(!Arc::ptr_eq(&old, &new));
    assert_eq!(
        new.read_pixels(2, 3, 1, 1, CompositeLimits::default())
            .unwrap()
            .rgba(),
        &[255, 128, 0, 128]
    );
    assert_eq!(old.premultiplied_rgba(), &[0; 320]);
    assert_eq!(e.scene(), &scene);
    assert_eq!(e.revision(), revision);
    assert!(
        new.encode_png(CompositeLimits::default())
            .unwrap()
            .starts_with(b"\x89PNG")
    );
}
#[test]
fn failed_pixel_transaction_rolls_back_all_writes_and_retains_exportable_snapshot() {
    use gpui_flow_composite::{CanvasPixels, CompositeError, CompositeLimits};
    let (mut e, mut r, h) = setup();
    r.prepare_canvases(&mut e).unwrap();
    let old = r.canvas_snapshot(&e, h).unwrap();
    let pixels = CanvasPixels::from_rgba(1, 1, vec![255; 4], CompositeLimits::default()).unwrap();
    assert!(
        r.edit_canvas(&mut e, h, |s| {
            s.write_pixels(&pixels, 0, 0)?;
            Err(CompositeError::InvalidImage)
        })
        .is_err()
    );
    assert!(Arc::ptr_eq(&old, &r.canvas_snapshot(&e, h).unwrap()));
    e.destroy(h).unwrap();
    assert!(r.canvas_snapshot(&e, h).is_none());
    assert!(old.encode_png(CompositeLimits::default()).is_ok());
}

#[test]
fn canvas_vector_commands_publish_one_generation_without_document_edits() {
    use gpui_flow_composite::CanvasCommand;
    use gpui_flow_scene::FillRule;
    let (mut e, mut r, h) = setup();
    r.prepare_canvases(&mut e).unwrap();
    let cfg = match &e.object(h).unwrap().shape {
        Shape::Canvas(c) => c.clone(),
        _ => unreachable!(),
    };
    let before = r.composite_canvas(&e, &"c".into(), &cfg);
    let document = e.scene().clone();
    let revision = e.revision();
    let commands = [
        CanvasCommand::SetOpacity(0.5),
        CanvasCommand::FillPath {
            path: display::Path::from_svg("M 1 1 L 7 1 L 7 6 L 1 6 Z").unwrap(),
            paint: Color::WHITE.into(),
            rule: FillRule::NonZero,
        },
    ];
    r.edit_canvas(&mut e, h, |s| s.draw_commands(&commands).map(|_| ()))
        .unwrap();
    let after = r.composite_canvas(&e, &"c".into(), &cfg);
    assert_eq!(after.playback_generation, before.playback_generation + 1);
    assert!(!before.resource_eq(&after));
    assert_eq!(at(&r, &e, h, 2, 2), [128; 4]);
    assert_eq!(e.scene(), &document);
    assert_eq!(e.revision(), revision);
    let old = r.canvas_snapshot(&e, h).unwrap();
    r.edit_canvas(&mut e, h, |s| {
        s.draw_commands(&[CanvasCommand::Save, CanvasCommand::Restore])
            .map(|_| ())
    })
    .unwrap();
    assert!(Arc::ptr_eq(&old, &r.canvas_snapshot(&e, h).unwrap()));
    assert!(after.resource_eq(&r.composite_canvas(&e, &"c".into(), &cfg)));
    let mut invalid = commands.to_vec();
    invalid.push(CanvasCommand::Restore);
    assert!(
        r.edit_canvas(&mut e, h, |s| s.draw_commands(&invalid).map(|_| ()))
            .is_err()
    );
    assert!(after.resource_eq(&r.composite_canvas(&e, &"c".into(), &cfg)));
    assert!(Arc::ptr_eq(&old, &r.canvas_snapshot(&e, h).unwrap()));
}
#[test]
fn canvas_vector_commands_reject_stale_handle_and_release_owned_snapshot() {
    use gpui_flow_composite::CanvasCommand;
    let (mut e, mut r, h) = setup();
    let commands = [CanvasCommand::StrokePath {
        path: display::Path::from_svg("M 1 3 L 9 3").unwrap(),
        stroke: gpui_flow_scene::Stroke::new(Color::WHITE, 2.),
    }];
    r.edit_canvas(&mut e, h, |s| s.draw_commands(&commands).map(|_| ()))
        .unwrap();
    let old = r.canvas_snapshot(&e, h).unwrap();
    let weak = Arc::downgrade(&old);
    drop(old);
    e.destroy(h).unwrap();
    assert!(weak.upgrade().is_none());
    assert!(
        r.edit_canvas(&mut e, h, |s| s.draw_commands(&commands).map(|_| ()))
            .is_err()
    );
}
#[test]
fn canvas_clip_batches_preserve_configuration_and_snapshot_on_late_failure() {
    use gpui_flow_composite::CanvasCommand;
    use gpui_flow_scene::FillRule;
    let (mut e, mut r, h) = setup();
    let scene = e.scene().clone();
    let command = CanvasCommand::FillPath {
        path: display::Path::from_svg("M 0 0 L 10 0 L 10 8 L 0 8 Z").unwrap(),
        paint: Color::WHITE.into(),
        rule: FillRule::NonZero,
    };
    let clip = CanvasCommand::ClipPath {
        path: display::Path::from_svg("M 2 2 L 7 2 L 7 6 L 2 6 Z").unwrap(),
        rule: FillRule::NonZero,
    };
    r.edit_canvas(&mut e, h, |s| {
        s.draw_commands(&[clip.clone(), command.clone()])
            .map(|_| ())
    })
    .unwrap();
    assert_eq!(at(&r, &e, h, 3, 3), [255; 4]);
    assert_eq!(at(&r, &e, h, 1, 1), [0; 4]);
    assert_eq!(e.scene(), &scene);
    let old = r.canvas_snapshot(&e, h).unwrap();
    assert!(
        r.edit_canvas(&mut e, h, |s| s
            .draw_commands(&[clip, command, CanvasCommand::Save])
            .map(|_| ()))
            .is_err()
    );
    assert!(Arc::ptr_eq(&old, &r.canvas_snapshot(&e, h).unwrap()));
    assert_eq!(e.scene(), &scene);
}

#[test]
fn canvas_image_commands_publish_one_resource_without_document_changes_and_reject_stale_targets() {
    use gpui_flow_composite::{CanvasCommand, ImageInputs, StaticImage};
    let (mut e, mut r, h) = setup();
    let scene = e.scene().clone();
    let revision = e.revision();
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "decoded".into(),
        Ok(Arc::new(
            StaticImage::from_premultiplied_rgba(1, 1, vec![255, 0, 0, 255], Default::default())
                .unwrap(),
        )),
    );
    let command = CanvasCommand::DrawImage {
        image: "decoded".into(),
        source: Some(Bounds::new(Point::ZERO, Size::new(1., 1.))),
        destination: Bounds::new(Point::new(2., 2.), Size::new(5., 4.)),
    };
    r.edit_canvas(&mut e, h, |s| {
        s.draw_commands_with_images(std::slice::from_ref(&command), &inputs)
            .map(|_| ())
    })
    .unwrap();
    assert_eq!(at(&r, &e, h, 3, 3), [255, 0, 0, 255]);
    assert_eq!(at(&r, &e, h, 0, 0), [0; 4]);
    assert_eq!(e.scene(), &scene);
    assert_eq!(e.revision(), revision);
    let before = r.canvas_snapshot(&e, h).unwrap();
    assert!(
        r.edit_canvas(&mut e, h, |s| s
            .draw_commands_with_images(&[command.clone(), CanvasCommand::Restore], &inputs)
            .map(|_| ()))
            .is_err()
    );
    assert!(Arc::ptr_eq(&before, &r.canvas_snapshot(&e, h).unwrap()));
    let weak = Arc::downgrade(&before);
    drop(before);
    e.destroy(h).unwrap();
    assert!(weak.upgrade().is_none());
    assert!(
        r.edit_canvas(&mut e, h, |s| s
            .draw_commands_with_images(&[command], &inputs)
            .map(|_| ()))
            .is_err()
    );
}

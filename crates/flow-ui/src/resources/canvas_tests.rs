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

fn current_path_rectangle() -> [gpui_flow_composite::CanvasCommand; 6] {
    use gpui_flow_composite::CanvasCommand;
    [
        CanvasCommand::BeginPath,
        CanvasCommand::MoveTo {
            to: Point::new(1., 1.),
        },
        CanvasCommand::LineTo {
            to: Point::new(7., 1.),
        },
        CanvasCommand::LineTo {
            to: Point::new(7., 6.),
        },
        CanvasCommand::LineTo {
            to: Point::new(1., 6.),
        },
        CanvasCommand::ClosePath,
    ]
}
fn fill_current_path() -> gpui_flow_composite::CanvasCommand {
    gpui_flow_composite::CanvasCommand::FillCurrentPath {
        paint: Color::WHITE.into(),
        rule: gpui_flow_scene::FillRule::NonZero,
    }
}
fn assert_empty_current_path(r: &mut SceneResourceAdapter, e: &mut SceneEngine, h: ElementHandle) {
    r.edit_canvas(e, h, |surface| {
        assert_eq!(surface.current_path_byte_len(), 0);
        assert!(!surface.draw_commands(&[fill_current_path()])?);
        Ok(())
    })
    .unwrap();
}

#[test]
fn canvas_current_path_cross_call_construction_keeps_pixel_generation_and_upload_until_draw() {
    let (mut e, mut r, h) = setup();
    r.prepare_canvases(&mut e).unwrap();
    let ticket = r.tracker.ticket(&e, h).unwrap();
    let (upload, generation) = r.canvases.image(ticket).unwrap();
    let pixels = r.canvas_snapshot(&e, h).unwrap();
    let scene = e.scene().clone();
    let revision = e.revision();
    let commands = current_path_rectangle();
    for batch in [&commands[..3], &commands[3..]] {
        r.edit_canvas(&mut e, h, |s| {
            assert!(!s.draw_commands(batch)?);
            assert!(s.current_path_byte_len() > 0);
            Ok(())
        })
        .unwrap();
        let (next_upload, next_generation) = r.canvases.image(ticket).unwrap();
        assert!(Arc::ptr_eq(&upload, &next_upload));
        assert_eq!(generation, next_generation);
        assert!(Arc::ptr_eq(&pixels, &r.canvas_snapshot(&e, h).unwrap()));
    }
    r.edit_canvas(&mut e, h, |s| {
        assert!(s.draw_commands(&[fill_current_path()])?);
        Ok(())
    })
    .unwrap();
    let (next_upload, next_generation) = r.canvases.image(ticket).unwrap();
    assert!(!Arc::ptr_eq(&upload, &next_upload));
    assert_eq!(next_generation, generation + 1);
    assert_eq!(at(&r, &e, h, 2, 2), [255; 4]);
    assert_eq!(at(&r, &e, h, 8, 2), [0; 4]);
    r.edit_canvas(&mut e, h, |s| {
        assert!(!s.draw_commands(&[gpui_flow_composite::CanvasCommand::BeginPath])?);
        assert_eq!(s.current_path_byte_len(), 0);
        Ok(())
    })
    .unwrap();
    let (reset_upload, reset_generation) = r.canvases.image(ticket).unwrap();
    assert!(Arc::ptr_eq(&next_upload, &reset_upload));
    assert_eq!(next_generation, reset_generation);
    assert_empty_current_path(&mut r, &mut e, h);
    assert_eq!(at(&r, &e, h, 2, 2), [255; 4]);
    assert_eq!(e.scene(), &scene);
    assert_eq!(e.revision(), revision);
    let mut editor = gpui_flow_editor::SceneEditor::from_engine(e);
    assert!(!editor.undo());
    assert!(!editor.redo());
}

#[test]
fn canvas_current_path_late_failure_rolls_back_path_and_pixels_together() {
    use gpui_flow_composite::CanvasCommand;
    let (mut e, mut r, h) = setup();
    r.edit_canvas(&mut e, h, |s| {
        s.draw_commands(&current_path_rectangle()).map(|_| ())
    })
    .unwrap();
    let ticket = r.tracker.ticket(&e, h).unwrap();
    let (upload, generation) = r.canvases.image(ticket).unwrap();
    let pixels = r.canvas_snapshot(&e, h).unwrap();
    assert!(
        r.edit_canvas(&mut e, h, |s| {
            s.draw_commands(&[
                fill_current_path(),
                CanvasCommand::BeginPath,
                CanvasCommand::MoveTo {
                    to: Point::new(8., 2.),
                },
                CanvasCommand::Restore,
            ])
            .map(|_| ())
        })
        .is_err()
    );
    let (after_upload, after_generation) = r.canvases.image(ticket).unwrap();
    assert!(Arc::ptr_eq(&upload, &after_upload));
    assert_eq!(generation, after_generation);
    assert!(Arc::ptr_eq(&pixels, &r.canvas_snapshot(&e, h).unwrap()));
    r.edit_canvas(&mut e, h, |s| {
        s.draw_commands(&[fill_current_path()]).map(|_| ())
    })
    .unwrap();
    assert_eq!(at(&r, &e, h, 2, 2), [255; 4]);
    assert_eq!(at(&r, &e, h, 8, 2), [0; 4]);
}

#[test]
fn canvas_current_path_resets_on_a_b_a_size_and_dpr_changes_before_prepare() {
    for change in 0..4 {
        let (mut e, mut r, h) = setup();
        let mut original = display::Canvas::new(10., 8.);
        original.safe_resize = true;
        e.set_property(
            h,
            PropertyKey::Shape,
            serde_json::json!(Shape::from(original.clone())),
        )
        .unwrap();
        r.edit_canvas(&mut e, h, |s| {
            fill(s)?;
            s.draw_commands(&current_path_rectangle()).map(|_| ())
        })
        .unwrap();
        let mut changed = original.clone();
        match change {
            0 => changed.size = Size::new(4., 4.),
            1 => changed.pixel_ratio = 2.,
            // Same physical dimensions still represent a logical-size change.
            2 => changed.size.width = 9.75,
            _ => {
                changed.size = Size::new(4., 4.);
                changed.safe_resize = false;
            }
        }
        e.set_property(
            h,
            PropertyKey::Shape,
            serde_json::json!(Shape::from(changed)),
        )
        .unwrap();
        e.set_property(
            h,
            PropertyKey::Shape,
            serde_json::json!(Shape::from(original)),
        )
        .unwrap();
        r.prepare_canvases(&mut e).unwrap();
        assert_empty_current_path(&mut r, &mut e, h);
        assert_eq!(
            at(&r, &e, h, 2, 2),
            if change == 3 {
                [0; 4]
            } else {
                [255, 0, 0, 255]
            }
        );
        if change == 0 {
            assert_eq!(at(&r, &e, h, 5, 2), [0; 4]);
        }
    }
}

#[test]
fn canvas_current_path_preserves_cosmetics_clear_and_pixel_exchange() {
    use gpui_flow_composite::{CanvasPixels, CompositeLimits};
    let (mut e, mut r, h) = setup();
    r.edit_canvas(&mut e, h, |s| {
        s.draw_commands(&current_path_rectangle()).map(|_| ())
    })
    .unwrap();
    let mut config = display::Canvas::new(10., 8.).rounded(2.);
    config.smooth = false;
    config.safe_resize = true;
    e.set_property(
        h,
        PropertyKey::Shape,
        serde_json::json!(Shape::from(config)),
    )
    .unwrap();
    r.prepare_canvases(&mut e).unwrap();
    let pixel =
        CanvasPixels::from_rgba(1, 1, vec![255, 0, 0, 255], CompositeLimits::default()).unwrap();
    r.edit_canvas(&mut e, h, |s| {
        let path_bytes = s.current_path_byte_len();
        assert!(path_bytes > 0);
        s.clear()?;
        s.write_pixels(&pixel, 0, 0)?;
        assert_eq!(s.current_path_byte_len(), path_bytes);
        assert!(s.draw_commands(&[fill_current_path()])?);
        assert_eq!(s.current_path_byte_len(), path_bytes);
        Ok(())
    })
    .unwrap();
    assert_eq!(at(&r, &e, h, 0, 0), [255, 0, 0, 255]);
    assert_eq!(at(&r, &e, h, 2, 2), [255; 4]);
}

#[test]
fn canvas_current_path_clone_detach_retype_and_recreated_identity_start_empty() {
    for mode in 0..5 {
        let (mut e, mut r, h) = setup();
        r.edit_canvas(&mut e, h, |s| {
            s.draw_commands(&current_path_rectangle()).map(|_| ())
        })
        .unwrap();
        let target = match mode {
            0 => {
                let cloned = e.clone_subtree(h, None, None).unwrap().root;
                // Cloning cannot consume or reset the source's runtime path.
                r.edit_canvas(&mut e, h, |s| {
                    assert!(s.draw_commands(&[fill_current_path()])?);
                    Ok(())
                })
                .unwrap();
                assert_eq!(at(&r, &e, h, 2, 2), [255; 4]);
                cloned
            }
            1 => {
                let (detached, _) = e.detach(h).unwrap();
                assert!(r.canvas_snapshot(&e, h).is_none());
                e.attach(&detached, None, None).unwrap();
                h
            }
            2 => {
                e.set_property(
                    h,
                    PropertyKey::Shape,
                    serde_json::json!(Shape::from(display::Rect::new(10., 8.))),
                )
                .unwrap();
                // Retyping creates a fresh runtime identity at the same
                // document ID; resolve it again for each shape transition.
                let retyped = e.handle(&"c".into()).unwrap();
                assert_ne!(retyped, h);
                e.set_property(
                    retyped,
                    PropertyKey::Shape,
                    serde_json::json!(Shape::from(display::Canvas::new(10., 8.))),
                )
                .unwrap();
                e.handle(&"c".into()).unwrap()
            }
            3 => {
                e.destroy(h).unwrap();
                e.create("canvas", "c", serde_json::json!({}), None, None)
                    .unwrap();
                assert!(r.edit_canvas(&mut e, h, |_| Ok(())).is_err());
                e.handle(&"c".into()).unwrap()
            }
            _ => {
                let saved = serde_json::to_string(e.scene()).unwrap();
                assert!(!saved.contains("current_path"));
                e.reset_scene(serde_json::from_str(&saved).unwrap())
                    .unwrap();
                e.handle(&"c".into()).unwrap()
            }
        };
        r.prepare_canvases(&mut e).unwrap();
        assert_empty_current_path(&mut r, &mut e, target);
        assert_eq!(at(&r, &e, target, 2, 2), [0; 4]);
    }
}

#[test]
fn canvas_current_path_configuration_undo_does_not_replay_runtime_geometry() {
    let (e, mut r, h) = setup();
    let mut editor = gpui_flow_editor::SceneEditor::from_engine(e);
    r.edit_canvas(&mut editor.engine, h, |s| {
        s.draw_commands(&current_path_rectangle()).map(|_| ())
    })
    .unwrap();
    assert!(!editor.undo());
    let mut config = display::Canvas::new(12., 9.);
    config.safe_resize = true;
    editor
        .set_property(
            h,
            PropertyKey::Shape,
            serde_json::json!(Shape::from(config)),
        )
        .unwrap();
    // Undo before preparation is another A→B→A path-reset boundary.
    assert!(editor.undo());
    r.prepare_canvases(&mut editor.engine).unwrap();
    assert_empty_current_path(&mut r, &mut editor.engine, h);
    r.edit_canvas(&mut editor.engine, h, |s| {
        s.draw_commands(&current_path_rectangle()).map(|_| ())
    })
    .unwrap();
    assert!(editor.redo());
    assert_empty_current_path(&mut r, &mut editor.engine, h);
}

#[test]
fn canvas_current_path_aggregate_budget_checks_final_replacement_before_publication() {
    use gpui_flow_composite::{CanvasCommand, CanvasSurface, CompositeLimits};
    let (mut e, mut r, h) = setup();
    let mut source =
        CanvasSurface::new(display::Canvas::new(10., 8.), CompositeLimits::default()).unwrap();
    let mut commands = vec![
        CanvasCommand::LineTo {
            to: Point::new(2., 2.)
        };
        256
    ];
    commands[0] = CanvasCommand::MoveTo {
        to: Point::new(1., 1.),
    };
    for _ in 0..64 {
        assert!(!source.draw_commands(&commands).unwrap());
    }
    let path_bytes = source.current_path_byte_len();
    assert!(path_bytes > 16 * 1024 * 1024 / 64);
    r.prepare_canvases(&mut e).unwrap();
    let mut rejected = None;
    for index in 0..64 {
        let target = if index == 0 {
            h
        } else {
            e.clone_subtree(h, None, None).unwrap().root
        };
        r.prepare_canvases(&mut e).unwrap();
        let ticket = r.tracker.ticket(&e, target).unwrap();
        let (upload, generation) = r.canvases.image(ticket).unwrap();
        let pixels = r.canvas_snapshot(&e, target).unwrap();
        // Host closures can replace a candidate wholesale. The final byte check
        // must still catch a surface that was built outside the lowered budget.
        let result = r.edit_canvas(&mut e, target, |s| {
            *s = source.clone();
            Ok(())
        });
        if result.is_err() {
            assert!(matches!(
                result,
                Err(CanvasEditError::Pixels(
                    gpui_flow_composite::CompositeError::WorkingSetLimit
                ))
            ));
            let (after_upload, after_generation) = r.canvases.image(ticket).unwrap();
            assert!(Arc::ptr_eq(&upload, &after_upload));
            assert_eq!(generation, after_generation);
            assert!(Arc::ptr_eq(
                &pixels,
                &r.canvas_snapshot(&e, target).unwrap()
            ));
            rejected = Some(target);
            break;
        }
        assert!(path_bytes * (index + 1) <= 16 * 1024 * 1024);
    }
    let rejected = rejected.expect("aggregate current paths must have a separate bound");
    // A nearly full pool must still allow shared-path configuration updates,
    // no-allocation commands, ordinary pixel clearing, and releasing geometry.
    e.set_property(
        h,
        PropertyKey::Shape,
        serde_json::json!(Shape::from(display::Canvas::new(10., 8.).rounded(2.))),
    )
    .unwrap();
    r.prepare_canvases(&mut e).unwrap();
    let before = r.canvas_snapshot(&e, h).unwrap();
    assert!(matches!(
        r.edit_canvas(&mut e, h, |s| s
            .draw_commands(&[CanvasCommand::LineTo {
                to: Point::new(3., 3.)
            },])
            .map(|_| ())),
        Err(CanvasEditError::Pixels(
            gpui_flow_composite::CompositeError::WorkingSetLimit
        ))
    ));
    assert!(Arc::ptr_eq(&before, &r.canvas_snapshot(&e, h).unwrap()));
    r.edit_canvas(&mut e, h, |s| {
        assert_eq!(s.current_path_byte_len(), path_bytes);
        assert!(!s.draw_commands(&[CanvasCommand::Save, CanvasCommand::Restore])?);
        s.clear()?;
        assert_eq!(s.current_path_byte_len(), path_bytes);
        assert!(!s.draw_commands(&[CanvasCommand::BeginPath])?);
        assert_eq!(s.current_path_byte_len(), 0);
        Ok(())
    })
    .unwrap();
    assert_empty_current_path(&mut r, &mut e, h);
    r.edit_canvas(&mut e, rejected, |s| {
        *s = source.clone();
        Ok(())
    })
    .unwrap();
}

#[test]
fn canvas_guarded_resource_rejection_after_upload_keeps_pixels_generation_and_current_path() {
    use gpui_flow_composite::{CanvasCommand, CompositeError};
    let (mut e, mut r, h) = setup();
    r.edit_canvas(&mut e, h, |s| {
        s.draw_commands(&current_path_rectangle()).map(|_| ())
    })
    .unwrap();
    let ticket = r.tracker.ticket(&e, h).unwrap();
    let before = r.canvas_snapshot(&e, h).unwrap();
    let (upload, generation) = r.canvases.image(ticket).unwrap();
    let checked = std::cell::Cell::new(false);
    let result = r.edit_canvas_guarded(
        &mut e,
        h,
        |s| {
            s.draw_commands(&[fill_current_path(), CanvasCommand::BeginPath])
                .map(|_| ())
        },
        || {
            checked.set(true);
            Err(CompositeError::Superseded)
        },
    );
    assert!(matches!(
        result,
        Err(CanvasEditError::Pixels(CompositeError::Superseded))
    ));
    assert!(checked.get());
    let (after_upload, after_generation) = r.canvases.image(ticket).unwrap();
    assert!(Arc::ptr_eq(&upload, &after_upload));
    assert_eq!(generation, after_generation);
    assert!(Arc::ptr_eq(&before, &r.canvas_snapshot(&e, h).unwrap()));
    assert_eq!(
        before.premultiplied_rgba(),
        r.canvas_snapshot(&e, h).unwrap().premultiplied_rgba()
    );
    r.edit_canvas(&mut e, h, |s| {
        s.draw_commands(&[fill_current_path()]).map(|_| ())
    })
    .unwrap();
    assert!(
        r.canvas_snapshot(&e, h)
            .unwrap()
            .premultiplied_rgba()
            .iter()
            .any(|b| *b != 0)
    );
}
#[test]
fn canvas_guarded_resource_geometry_only_rejection_preserves_upload_and_path() {
    use gpui_flow_composite::{CanvasCommand, CompositeError};
    let (mut e, mut r, h) = setup();
    r.edit_canvas(&mut e, h, |s| {
        s.draw_commands(&current_path_rectangle()).map(|_| ())
    })
    .unwrap();
    let ticket = r.tracker.ticket(&e, h).unwrap();
    let before = r.canvas_snapshot(&e, h).unwrap();
    let (upload, generation) = r.canvases.image(ticket).unwrap();
    assert!(
        r.edit_canvas_guarded(
            &mut e,
            h,
            |s| s.draw_commands(&[CanvasCommand::BeginPath]).map(|_| ()),
            || Err(CompositeError::Superseded)
        )
        .is_err()
    );
    let (after_upload, after_generation) = r.canvases.image(ticket).unwrap();
    assert!(Arc::ptr_eq(&upload, &after_upload));
    assert_eq!(generation, after_generation);
    assert!(Arc::ptr_eq(&before, &r.canvas_snapshot(&e, h).unwrap()));
    r.edit_canvas(&mut e, h, |s| {
        s.draw_commands(&[fill_current_path()]).map(|_| ())
    })
    .unwrap();
    assert!(
        r.canvas_snapshot(&e, h)
            .unwrap()
            .premultiplied_rgba()
            .iter()
            .any(|b| *b != 0)
    );
}

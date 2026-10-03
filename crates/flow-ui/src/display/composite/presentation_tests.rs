use super::tests::{engine, key};
use super::*;
use gpui_flow_scene::{Affine, Color, DisplayObject, Style, display};

fn sample(engine: &SceneEngine, x: f32, width: f32) -> Arc<RequestKey> {
    let mut scene = engine.scene().clone();
    scene
        .update_object(&"g".into(), |object| {
            object.transform = Affine::translation(x, 0.)
        })
        .unwrap();
    scene
        .update_object(&"r".into(), |object| {
            object.shape = display::Rect::new(width, 20.).into()
        })
        .unwrap();
    let source = gpui_flow_composite::scene_units(&scene, CompositeLimits::default())
        .unwrap()
        .into_iter()
        .filter_map(|unit| match unit {
            gpui_flow_composite::SceneUnit::Domain(domain) => Some(*domain),
            _ => None,
        })
        .collect();
    let base = key(engine);
    Arc::new(RequestKey::new(source, engine, base.viewport, base.window))
}
fn finish(controller: &mut CompositeController) {
    let work = controller.take_work().expect("one admitted worker");
    controller.finish(work.key.clone(), work.run());
}
fn upload(controller: &mut CompositeController, key: &Arc<RequestKey>) {
    for (id, frame) in controller.presentation_uploads(key) {
        controller.presentation_uploaded(key, &id, &frame.image);
    }
}
#[test]
fn coherent_geometry_retains_old_placement_until_complete_real_upload_receipt() {
    let engine = engine();
    let first = sample(&engine, 0., 20.);
    let next = sample(&engine, 35., 40.);
    let mut controller = CompositeController::default();
    controller.request_coherent(first.clone(), false);
    finish(&mut controller);
    assert!(controller.publish_presentation(&first).is_none());
    upload(&mut controller, &first);
    assert!(controller.publish_presentation(&first).unwrap().is_empty());
    let old = controller.presentation_frame(&first, &"g".into()).unwrap();
    assert!(controller.request_coherent(next.clone(), true).is_empty());
    assert!(controller.drain_obsolete().is_empty());
    finish(&mut controller);
    assert!(controller.publish_presentation(&next).is_none());
    assert!(Arc::ptr_eq(
        &old.image,
        &controller
            .presentation_frame(&first, &"g".into())
            .unwrap()
            .image
    ));
    assert!(controller.presentation_frame(&next, &"g".into()).is_none());
    upload(&mut controller, &next);
    let retired = controller.publish_presentation(&next).unwrap();
    assert_eq!(retired.len(), 1);
    assert!(Arc::ptr_eq(&retired[0].image, &old.image));
    assert!(controller.presentation_frame(&first, &"g".into()).is_none());
    let new = controller.presentation_frame(&next, &"g".into()).unwrap();
    assert_ne!(old.bounds, new.bounds);
}
#[test]
fn coherent_batch_never_publishes_partial_domains_or_wrong_upload_owner() {
    let mut engine = engine();
    engine
        .transaction(|scene| {
            scene.insert(
                DisplayObject::new("g2", display::Group::new()).with_style(Style {
                    opacity: 0.5,
                    ..Default::default()
                }),
                None,
                None,
            )?;
            scene.insert(
                DisplayObject::new("r2", display::Rect::new(10., 10.))
                    .with_style(Style::fill(Color::WHITE)),
                Some("g2".into()),
                None,
            )
        })
        .unwrap();
    let key = key(&engine);
    assert_eq!(key.domains.len(), 2);
    let mut controller = CompositeController::default();
    controller.request_coherent(key.clone(), false);
    finish(&mut controller);
    let uploads = controller.presentation_uploads(&key);
    assert_eq!(uploads.len(), 2);
    controller.presentation_uploaded(&key, &uploads[0].0, &uploads[1].1.image);
    assert!(controller.publish_presentation(&key).is_none());
    controller.presentation_uploaded(&key, &uploads[0].0, &uploads[0].1.image);
    assert!(controller.publish_presentation(&key).is_none());
    assert!(controller.accepted.is_empty());
    controller.presentation_uploaded(&key, &uploads[1].0, &uploads[1].1.image);
    controller.publish_presentation(&key).unwrap();
    assert_eq!(controller.accepted.len(), 2);
}
#[test]
fn coherent_hard_invalidation_cancels_work_and_ignores_late_aba_completion() {
    let engine = engine();
    let first = sample(&engine, 0., 20.);
    let mut controller = CompositeController::default();
    controller.request_coherent(first.clone(), false);
    let worker = controller.take_work().unwrap();
    let mut other = (*first).clone();
    other.viewport.device_scale = 2.;
    controller.request_coherent(Arc::new(other), false);
    controller.request_coherent(first.clone(), false);
    assert!(worker.cancel.load(Ordering::Relaxed));
    assert!(controller.take_work().is_none());
    controller.finish(worker.key.clone(), worker.run());
    assert!(controller.candidates.is_empty());
    assert!(controller.publish_presentation(&first).is_none());
    finish(&mut controller);
    upload(&mut controller, &first);
    controller.publish_presentation(&first).unwrap();
}
#[test]
fn coherent_uploaded_cancellation_keeps_retirement_and_working_set_bounded() {
    let engine = engine();
    let first = sample(&engine, 0., 20.);
    let next = sample(&engine, 10., 30.);
    let mut controller = CompositeController::default();
    controller.request_coherent(first.clone(), false);
    finish(&mut controller);
    upload(&mut controller, &first);
    let bytes = controller.held_bytes();
    controller.retired_bytes = MAX_RETIRED;
    let retired = controller.request_coherent(next.clone(), false);
    assert!(retired.is_empty());
    assert_eq!(controller.obsolete_uploads.len(), 1);
    assert_eq!(controller.held_bytes(), MAX_RETIRED + bytes);
    assert!(
        controller.take_work().is_none(),
        "do not accumulate another warmed candidate batch"
    );
    controller.release_retired(MAX_RETIRED);
    let retired = controller.drain_obsolete();
    assert_eq!(retired.len(), 1);
    controller.retired_bytes += retired[0].bytes;
    finish(&mut controller);
    upload(&mut controller, &next);
    controller.publish_presentation(&next).unwrap();
    assert!(controller.held_bytes() <= MAX_RETAINED + MAX_RETIRED);
}
#[test]
fn coherent_atomic_swap_waits_for_retirement_budget_without_hiding_old() {
    let engine = engine();
    let first = sample(&engine, 0., 20.);
    let next = sample(&engine, 25., 35.);
    let mut controller = CompositeController::default();
    controller.request_coherent(first.clone(), false);
    finish(&mut controller);
    upload(&mut controller, &first);
    controller.publish_presentation(&first).unwrap();
    controller.request_coherent(next.clone(), true);
    finish(&mut controller);
    upload(&mut controller, &next);
    controller.retired_bytes = MAX_RETIRED;
    assert!(controller.publish_presentation(&next).is_none());
    assert!(controller.presentation_frame(&first, &"g".into()).is_some());
    controller.release_retired(MAX_RETIRED);
    assert_eq!(controller.publish_presentation(&next).unwrap().len(), 1);
}
#[test]
fn coherent_key_rejects_canonical_owner_viewport_resource_and_destroy_changes() {
    struct Layout;
    impl gpui_flow_engine::LayoutProvider for Layout {
        fn derive(
            &self,
            _: &SceneEngine,
            scene: &gpui_flow_scene::Scene,
        ) -> Result<gpui_flow_scene::Scene, gpui_flow_engine::EngineError> {
            Ok(scene.clone())
        }
    }
    let mut engine = engine();
    let first = key(&engine);
    let current = |engine: &SceneEngine, key: &RequestKey| {
        key.presentation_current(engine, None, None, key.viewport, key.window)
    };
    assert!(current(&engine, &first));
    let other_engine = SceneEngine::new(engine.scene().clone()).unwrap();
    assert!(!current(&other_engine, &first));
    let provider = engine.install_layout_provider(Layout).unwrap();
    assert!(!current(&engine, &first));
    assert!(engine.remove_layout_provider(provider));
    assert!(current(&engine, &first));
    for change in 0..4 {
        let mut viewport = first.viewport;
        match change {
            0 => viewport.world_to_screen = Affine::scale(2., 2.),
            1 => viewport.canvas_bounds.size.width += 1.,
            2 => viewport.content_mask.size.height -= 1.,
            _ => viewport.device_scale = 2.,
        }
        assert!(!first.presentation_current(&engine, None, None, viewport, first.window));
    }
    assert!(!first.presentation_current(&engine, None, None, first.viewport, first.window + 1));
    engine
        .transaction(|scene| scene.update_object(&"r".into(), |object| object.style.opacity = 0.7))
        .unwrap();
    assert!(!current(&engine, &first));
    let image_key = super::tests::animated_key();
    let mut revoked = (*image_key).clone();
    let image = revoked.domains[0].images.values_mut().next().unwrap();
    image.playback_generation += 1;
    assert!(!image_key.animation_only(&revoked));
    let mut removed = (*image_key).clone();
    removed.domains[0].images.clear();
    assert!(!image_key.animation_only(&removed));
    let fresh = key(&engine);
    engine
        .transaction(|scene| {
            scene.remove(&"g".into())?;
            Ok(())
        })
        .unwrap();
    assert!(!current(&engine, &fresh));
}

#[test]
fn coherent_upload_failure_keeps_old_batch_and_new_target_recovers() {
    let engine = engine();
    let first = sample(&engine, 0., 20.);
    let failed = sample(&engine, 20., 30.);
    let latest = sample(&engine, 40., 45.);
    let mut controller = CompositeController::default();
    controller.request_coherent(first.clone(), false);
    finish(&mut controller);
    upload(&mut controller, &first);
    controller.publish_presentation(&first).unwrap();
    controller.request_coherent(failed.clone(), true);
    finish(&mut controller);
    controller.upload_failed(&failed, &"g".into(), "injected atlas failure".into());
    assert!(controller.publish_presentation(&failed).is_none());
    assert!(controller.presentation_failed());
    assert!(controller.presentation_frame(&first, &"g".into()).is_some());
    assert!(controller.presentation_uploads(&failed).is_empty());
    controller.request_coherent(latest.clone(), true);
    finish(&mut controller);
    upload(&mut controller, &latest);
    assert_eq!(controller.publish_presentation(&latest).unwrap().len(), 1);
    assert!(controller.errors.is_empty());
}

#[test]
fn coherent_empty_cpu_result_can_be_republished_for_native_only_changes() {
    let engine = engine();
    let offscreen = sample(&engine, 250., 20.);
    let mut controller = CompositeController::default();
    controller.request_coherent(offscreen.clone(), false);
    finish(&mut controller);
    assert!(controller.presentation_uploads(&offscreen).is_empty());
    assert!(
        controller
            .publish_presentation(&offscreen)
            .unwrap()
            .is_empty()
    );
    for _ in 0..25 {
        controller.request_coherent(offscreen.clone(), true);
        assert!(controller.take_work().is_none());
        assert!(
            controller.publish_presentation(&offscreen).is_some(),
            "unchanged empty domain must not block native-only successors"
        );
        assert!(
            controller
                .presentation_frame(&offscreen, &"g".into())
                .is_none()
        );
        assert_eq!(controller.accepted_empty.len(), 1);
    }
    let visible = sample(&engine, 0., 20.);
    controller.request_coherent(visible.clone(), true);
    finish(&mut controller);
    upload(&mut controller, &visible);
    controller.publish_presentation(&visible).unwrap();
    assert!(controller.accepted_empty.is_empty());
    assert!(
        controller
            .presentation_frame(&visible, &"g".into())
            .is_some()
    );
}

#[test]
fn eraser_mode_changes_hide_incompatible_frames_cancel_late_work_and_wait_for_upload() {
    use gpui_flow_engine::PropertyKey;
    use gpui_flow_scene::EraserMode;

    let mut engine = engine();
    engine
        .transaction(|scene| {
            scene.insert(
                DisplayObject::new("back", display::Rect::new(40., 40.))
                    .with_style(Style::fill(Color::WHITE)),
                Some("g".into()),
                Some(0),
            )
        })
        .unwrap();
    let domain: DisplayId = "g".into();
    let mut controller = CompositeController::default();
    let first = key(&engine);
    controller.request(first.clone());
    finish(&mut controller);
    let frame = controller.frame(&domain).unwrap().0;
    assert!(controller.accepted.is_empty());
    controller.accepted(&first, &domain, &frame.image);

    for mode in [EraserMode::Pixel, EraserMode::Path, EraserMode::None] {
        // Finish a paint-only refresh but delay its delivery until the mode edit.
        engine
            .set_property(
                engine.handle(&"r".into()).unwrap(),
                PropertyKey::Fill,
                serde_json::to_value(gpui_flow_scene::Paint::solid(Color::rgb(40, 90, 180)))
                    .unwrap(),
            )
            .unwrap();
        let previous = key(&engine);
        controller.request(previous.clone());
        // Later loop iterations may already have this exact fill.
        let late = controller.take_work().map(|work| {
            let result = work.run();
            (work, result)
        });
        engine
            .set_property(
                engine.handle(&"r".into()).unwrap(),
                PropertyKey::Eraser,
                serde_json::to_value(mode).unwrap(),
            )
            .unwrap();
        let current = key(&engine);
        assert!(!current.compatible(&previous, &domain));
        controller.request(current.clone());
        assert!(controller.frame(&domain).is_none());
        if let Some((work, result)) = late {
            assert!(work.cancel.load(Ordering::Relaxed));
            controller.finish(work.key.clone(), result);
            assert!(controller.candidates.is_empty());
        }
        finish(&mut controller);
        assert!(controller.accepted.is_empty());
        let (frame, candidate) = controller.frame(&domain).unwrap();
        assert!(candidate);
        controller.accepted(&previous, &domain, &frame.image);
        assert!(controller.accepted.is_empty());
        controller.accepted(&current, &domain, &frame.image);
        assert_eq!(controller.accepted.len(), 1);
    }
}

#[test]
fn grayscale_amount_changes_withdraw_frames_cancel_late_success_and_require_current_upload() {
    use gpui_flow_engine::PropertyKey;
    for owned in [false, true] {
        let mut engine = engine();
        engine
            .set_property(
                engine.handle(&"r".into()).unwrap(),
                PropertyKey::Fill,
                serde_json::to_value(gpui_flow_scene::Paint::solid(Color::rgb(255, 0, 0))).unwrap(),
            )
            .unwrap();
        let make_key = |engine: &SceneEngine| {
            if !owned {
                return key(engine);
            }
            let base = key(engine);
            let domain = gpui_flow_composite::owned_canvas(
                engine.scene(),
                gpui_flow_composite::CanvasBackdrop {
                    color: Color::WHITE,
                    grid: None,
                },
                CompositeLimits::default(),
            )
            .unwrap();
            Arc::new(RequestKey::new(
                vec![domain],
                engine,
                base.viewport,
                base.window,
            ))
        };
        let domain: DisplayId = if owned { "" } else { "g" }.into();
        let mut controller = CompositeController::default();
        let first = make_key(&engine);
        controller.request(first.clone());
        finish(&mut controller);
        let frame = controller.frame(&domain).unwrap().0;
        controller.accepted(&first, &domain, &frame.image);
        for (index, amount) in [0.25, 0.75, 0.].into_iter().enumerate() {
            engine
                .set_property(
                    engine.handle(&"r".into()).unwrap(),
                    PropertyKey::Fill,
                    serde_json::to_value(gpui_flow_scene::Paint::solid(Color::rgb(
                        200 + index as u8,
                        20,
                        30,
                    )))
                    .unwrap(),
                )
                .unwrap();
            let previous = make_key(&engine);
            controller.request(previous.clone());
            let late = controller.take_work().unwrap();
            let result = late.run();
            engine
                .set_property(
                    engine.handle(&"g".into()).unwrap(),
                    PropertyKey::Grayscale,
                    serde_json::json!(amount),
                )
                .unwrap();
            let current = make_key(&engine);
            assert!(!current.compatible(&previous, &domain));
            controller.request(current.clone());
            assert!(controller.frame(&domain).is_none());
            assert!(late.cancel.load(Ordering::Relaxed));
            controller.finish(late.key.clone(), result);
            assert!(controller.candidates.is_empty());
            finish(&mut controller);
            assert!(controller.accepted.is_empty());
            let (frame, candidate) = controller.frame(&domain).unwrap();
            assert!(candidate);
            controller.accepted(&previous, &domain, &frame.image);
            assert!(controller.accepted.is_empty());
            controller.accepted(&current, &domain, &frame.image);
            assert_eq!(controller.accepted.len(), 1);
        }
    }
}

#[test]
fn grayscale_domain_appearance_disappearance_rejects_late_results() {
    use gpui_flow_engine::PropertyKey;
    let mut engine = engine();
    engine
        .set_property(
            engine.handle(&"g".into()).unwrap(),
            PropertyKey::BlendMode,
            serde_json::json!("pass_through"),
        )
        .unwrap();
    assert!(key(&engine).domains.is_empty());
    engine
        .set_property(
            engine.handle(&"g".into()).unwrap(),
            PropertyKey::Grayscale,
            serde_json::json!(1.),
        )
        .unwrap();
    let first = key(&engine);
    assert_eq!(first.domains.len(), 1);
    let mut controller = CompositeController::default();
    controller.request(first.clone());
    let late = controller.take_work().unwrap();
    let result = late.run();
    engine
        .set_property(
            engine.handle(&"g".into()).unwrap(),
            PropertyKey::Grayscale,
            serde_json::json!(0.),
        )
        .unwrap();
    let zero = key(&engine);
    assert!(zero.domains.is_empty());
    controller.request(zero);
    assert!(late.cancel.load(Ordering::Relaxed));
    controller.finish(late.key.clone(), result);
    assert!(controller.frame(&"g".into()).is_none());
    assert!(controller.candidates.is_empty());
    engine
        .set_property(
            engine.handle(&"g".into()).unwrap(),
            PropertyKey::Grayscale,
            serde_json::json!(1.),
        )
        .unwrap();
    let current = key(&engine);
    controller.request(current.clone());
    finish(&mut controller);
    let frame = controller.frame(&"g".into()).unwrap().0;
    controller.accepted(&current, &"g".into(), &frame.image);
    assert_eq!(controller.accepted.len(), 1);
}

#[test]
fn grayscale_undo_restores_original_pixels_and_stale_placement_identity() {
    use gpui_flow_engine::PropertyKey;
    let mut editor = gpui_flow_editor::SceneEditor::new(engine().scene().clone()).unwrap();
    editor
        .set_property(
            editor.engine.handle(&"r".into()).unwrap(),
            PropertyKey::Fill,
            serde_json::to_value(gpui_flow_scene::Paint::solid(Color::rgb(255, 0, 0))).unwrap(),
        )
        .unwrap();
    let render = |engine: &SceneEngine| {
        let request = key(engine);
        let source = &request.domains[0].source;
        gpui_flow_composite::rasterize(
            source,
            request.viewport,
            &mut gpui_flow_composite::PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap()
    };
    let original = render(&editor.engine);
    let first = key(&editor.engine);
    editor
        .set_property(
            editor.engine.handle(&"g".into()).unwrap(),
            PropertyKey::Grayscale,
            serde_json::json!(1.),
        )
        .unwrap();
    assert_ne!(original.premul_rgba, render(&editor.engine).premul_rgba);
    assert!(!key(&editor.engine).compatible(&first, &"g".into()));
    assert!(editor.undo());
    let restored = render(&editor.engine);
    assert_eq!(original.premul_rgba, restored.premul_rgba);
    assert_eq!(original.screen_bounds, restored.screen_bounds);
    assert!(key(&editor.engine).compatible(&first, &"g".into()));
}

#[test]
fn blur_amount_changes_withdraw_frames_cancel_late_success_and_require_current_upload() {
    use gpui_flow_engine::PropertyKey;
    for owned in [false, true] {
        let mut engine = engine();
        engine
            .set_property(
                engine.handle(&"r".into()).unwrap(),
                PropertyKey::Fill,
                serde_json::to_value(gpui_flow_scene::Paint::solid(Color::rgb(255, 0, 0))).unwrap(),
            )
            .unwrap();
        let make_key = |engine: &SceneEngine| {
            if !owned {
                return key(engine);
            }
            let base = key(engine);
            let domain = gpui_flow_composite::owned_canvas(
                engine.scene(),
                gpui_flow_composite::CanvasBackdrop {
                    color: Color::WHITE,
                    grid: None,
                },
                CompositeLimits::default(),
            )
            .unwrap();
            Arc::new(RequestKey::new(
                vec![domain],
                engine,
                base.viewport,
                base.window,
            ))
        };
        let domain: DisplayId = if owned { "" } else { "g" }.into();
        let mut controller = CompositeController::default();
        let first = make_key(&engine);
        controller.request(first.clone());
        finish(&mut controller);
        let frame = controller.frame(&domain).unwrap().0;
        controller.accepted(&first, &domain, &frame.image);
        for (index, amount) in [0.3, 0.31, 0.].into_iter().enumerate() {
            engine
                .set_property(
                    engine.handle(&"r".into()).unwrap(),
                    PropertyKey::Fill,
                    serde_json::to_value(gpui_flow_scene::Paint::solid(Color::rgb(
                        200 + index as u8,
                        20,
                        30,
                    )))
                    .unwrap(),
                )
                .unwrap();
            let previous = make_key(&engine);
            controller.request(previous.clone());
            let late = controller.take_work().unwrap();
            let result = late.run();
            engine
                .set_property(
                    engine.handle(&"g".into()).unwrap(),
                    PropertyKey::Blur,
                    serde_json::json!(amount),
                )
                .unwrap();
            let current = make_key(&engine);
            assert!(!current.compatible(&previous, &domain));
            controller.request(current.clone());
            assert!(controller.frame(&domain).is_none());
            assert!(late.cancel.load(Ordering::Relaxed));
            controller.finish(late.key.clone(), result);
            assert!(controller.candidates.is_empty());
            finish(&mut controller);
            assert!(controller.accepted.is_empty());
            let (frame, candidate) = controller.frame(&domain).unwrap();
            assert!(candidate);
            controller.accepted(&previous, &domain, &frame.image);
            assert!(controller.accepted.is_empty());
            controller.accepted(&current, &domain, &frame.image);
            assert_eq!(controller.accepted.len(), 1);
        }
    }
}

#[test]
fn blur_domain_appearance_disappearance_rejects_late_results() {
    use gpui_flow_engine::PropertyKey;
    let mut engine = engine();
    engine
        .set_property(
            engine.handle(&"g".into()).unwrap(),
            PropertyKey::BlendMode,
            serde_json::json!("pass_through"),
        )
        .unwrap();
    assert!(key(&engine).domains.is_empty());
    engine
        .set_property(
            engine.handle(&"g".into()).unwrap(),
            PropertyKey::Blur,
            serde_json::json!(1.),
        )
        .unwrap();
    let first = key(&engine);
    assert_eq!(first.domains.len(), 1);
    let mut controller = CompositeController::default();
    controller.request(first.clone());
    let late = controller.take_work().unwrap();
    let result = late.run();
    engine
        .set_property(
            engine.handle(&"g".into()).unwrap(),
            PropertyKey::Blur,
            serde_json::json!(0.),
        )
        .unwrap();
    let zero = key(&engine);
    assert!(zero.domains.is_empty());
    controller.request(zero);
    assert!(late.cancel.load(Ordering::Relaxed));
    controller.finish(late.key.clone(), result);
    assert!(controller.frame(&"g".into()).is_none());
    assert!(controller.candidates.is_empty());
    engine
        .set_property(
            engine.handle(&"g".into()).unwrap(),
            PropertyKey::Blur,
            serde_json::json!(1.),
        )
        .unwrap();
    let current = key(&engine);
    controller.request(current.clone());
    finish(&mut controller);
    let frame = controller.frame(&"g".into()).unwrap().0;
    controller.accepted(&current, &"g".into(), &frame.image);
    assert_eq!(controller.accepted.len(), 1);
}

#[test]
fn blur_undo_restores_original_pixels_and_stale_placement_identity() {
    use gpui_flow_engine::PropertyKey;
    let mut editor = gpui_flow_editor::SceneEditor::new(engine().scene().clone()).unwrap();
    editor
        .set_property(
            editor.engine.handle(&"r".into()).unwrap(),
            PropertyKey::Fill,
            serde_json::to_value(gpui_flow_scene::Paint::solid(Color::rgb(255, 0, 0))).unwrap(),
        )
        .unwrap();
    let render = |engine: &SceneEngine| {
        let request = key(engine);
        let source = &request.domains[0].source;
        gpui_flow_composite::rasterize(
            source,
            request.viewport,
            &mut gpui_flow_composite::PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap()
    };
    let original = render(&editor.engine);
    let first = key(&editor.engine);
    editor
        .set_property(
            editor.engine.handle(&"g".into()).unwrap(),
            PropertyKey::Blur,
            serde_json::json!(1.),
        )
        .unwrap();
    assert_ne!(original.premul_rgba, render(&editor.engine).premul_rgba);
    assert!(!key(&editor.engine).compatible(&first, &"g".into()));
    assert!(editor.undo());
    let restored = render(&editor.engine);
    assert_eq!(original.premul_rgba, restored.premul_rgba);
    assert_eq!(original.screen_bounds, restored.screen_bounds);
    assert!(key(&editor.engine).compatible(&first, &"g".into()));
}

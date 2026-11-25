// Included inside composite.rs's tests module so existing controller helpers
// remain private to tests.
mod background_blur_tests {
    use super::*;
    use gpui_flow_scene::{Affine, Color, DisplayObject, Point, Size, Style, display};

    fn glass() -> DisplayObject {
        DisplayObject::new("glass", display::Rect::new(24., 12.))
            .with_transform(Affine::translation(4., 4.))
            .with_style(Style {
                fill: None,
                background_blur: 1.,
                ..Default::default()
            })
    }

    fn glass_engine() -> SceneEngine {
        let mut engine = engine();
        engine
            .transaction(|scene| {
                scene.update_object(&"r".into(), |object| {
                    object.style = Style::fill(Color::rgb(255, 0, 0));
                })?;
                scene.insert(glass(), Some("g".into()), None)
            })
            .unwrap();
        engine
    }

    fn animated_glass_key() -> Arc<RequestKey> {
        let mut engine = glass_engine();
        let mut image = display::Image::new("animated.gif", Size::new(20., 20.));
        image.fit = display::ImageFit::Fill;
        engine
            .transaction(|scene| {
                scene.update_object(&"r".into(), |object| {
                    object.shape = image.clone().into();
                    object.style.fill = None;
                })?;
                scene.insert(
                    DisplayObject::new("foreground", image)
                        .with_transform(Affine::translation(30., 0.)),
                    None,
                    None,
                )
            })
            .unwrap();
        let mut key = (*canvas_key(&engine, Color::WHITE, false)).clone();
        let observation = animated_key().domains[0].images[&"r".into()].clone();
        key.domains[0]
            .images
            .insert("r".into(), observation.clone());
        key.domains[0]
            .images
            .insert("foreground".into(), observation);
        assert_eq!(key.domains.len(), 1);
        assert!(key.domains[0].source.is_owned_canvas());
        assert!(key.domains[0].source.has_background_blur());
        Arc::new(key)
    }

    fn sample(key: &Arc<RequestKey>, frame_index: usize) -> Arc<RequestKey> {
        let mut key = (**key).clone();
        for observation in key.domains[0].images.values_mut() {
            observation.frame_index = frame_index;
        }
        Arc::new(key)
    }

    fn complete(controller: &mut CompositeController, key: &Arc<RequestKey>) -> ImageFrame {
        let work = controller.take_work().expect("one admitted complete frame");
        assert_eq!(work.key, *key);
        let result = work.run();
        assert!(matches!(result.as_slice(), [(_, Ok(Some(_)))]));
        controller.finish(key.clone(), result);
        controller.frame(key.domains[0].source.root_id()).unwrap().0
    }

    fn pixel(frame: &ImageFrame, x: usize, y: usize) -> [u8; 4] {
        // These requests use the 100x100 physical viewport from canvas_key.
        let offset = (y * 100 + x) * 4;
        frame.image.as_bytes(0).unwrap()[offset..offset + 4]
            .try_into()
            .unwrap()
    }

    #[test]
    fn background_blur_animation_coalesces_only_complete_prefix_and_foreground_frames() {
        let first = animated_glass_key();
        let next = sample(&first, 1);
        let id = DisplayId::default();
        let mut controller = CompositeController::default();
        controller.request(first.clone());
        let work = controller.take_work().unwrap();
        let late = work.run();
        assert!(matches!(late.as_slice(), [(_, Ok(Some(_)))]));
        controller.request(next.clone());
        assert!(!work.cancel.load(Ordering::Relaxed));
        assert_eq!(controller.requested.as_ref(), Some(&first));
        controller.finish(first.clone(), late);
        let old = controller.frame(&id).unwrap().0;
        assert_eq!(pixel(&old, 8, 8), [0, 0, 255, 255]);
        assert_eq!(pixel(&old, 38, 8), [0, 0, 255, 255]);
        let edge = pixel(&old, 19, 10);
        assert!(edge[0] > 0 && edge[0] < 255 && edge[2] == 255);
        assert_eq!(old.bytes, 100 * 100 * 4);
        controller.accepted(&next, &id, &old.image);
        assert!(
            controller.accepted.is_empty(),
            "a later sample cannot accept old work"
        );
        controller.request(next.clone());
        assert_eq!(controller.requested.as_ref(), Some(&first));
        controller.accepted(&first, &id, &old.image);
        controller.request(next.clone());
        assert!(Arc::ptr_eq(
            &old.image,
            &controller.frame(&id).unwrap().0.image
        ));
        let fresh = complete(&mut controller, &next);
        assert_eq!(pixel(&fresh, 8, 8), [0, 255, 0, 255]);
        assert_eq!(pixel(&fresh, 38, 8), [0, 255, 0, 255]);
        assert_ne!(fresh.image.as_bytes(0), old.image.as_bytes(0));
        controller.accepted(&next, &id, &old.image);
        assert!(Arc::ptr_eq(
            &controller.accepted[&id].frame.image,
            &old.image
        ));
        let retired = controller.accepted(&next, &id, &fresh.image).unwrap();
        assert!(Arc::ptr_eq(&retired.image, &old.image));
        assert_eq!(controller.accepted[&id].key, next);
        assert!(Arc::ptr_eq(
            &controller.frame(&id).unwrap().0.image,
            &fresh.image
        ));
    }

    #[test]
    fn background_blur_coherent_presentation_waits_for_current_whole_frame_upload() {
        let first = animated_glass_key();
        let next = sample(&first, 1);
        let id = DisplayId::default();
        let mut controller = CompositeController::default();
        controller.request_coherent(first.clone(), false);
        let old = complete(&mut controller, &first);
        assert!(controller.publish_presentation(&first).is_none());
        assert_eq!(controller.presentation_uploads(&first).len(), 1);
        controller.presentation_uploaded(&first, &id, &old.image);
        assert!(controller.publish_presentation(&first).unwrap().is_empty());
        assert!(controller.request_coherent(next.clone(), true).is_empty());
        let fresh = complete(&mut controller, &next);
        assert_eq!(pixel(&old, 8, 8), [0, 0, 255, 255]);
        assert_eq!(pixel(&old, 38, 8), [0, 0, 255, 255]);
        assert_eq!(pixel(&fresh, 8, 8), [0, 255, 0, 255]);
        assert_eq!(pixel(&fresh, 38, 8), [0, 255, 0, 255]);
        assert!(controller.presentation_frame(&next, &id).is_none());
        assert!(Arc::ptr_eq(
            &controller.presentation_frame(&first, &id).unwrap().image,
            &old.image
        ));
        controller.presentation_uploaded(&first, &id, &fresh.image);
        controller.presentation_uploaded(&next, &id, &old.image);
        assert!(controller.publish_presentation(&next).is_none());
        assert!(controller.uploaded.is_empty());
        controller.presentation_uploaded(&next, &id, &fresh.image);
        assert!(controller.publish_presentation(&first).is_none());
        let retired = controller.publish_presentation(&next).unwrap();
        assert_eq!(retired.len(), 1);
        assert!(Arc::ptr_eq(&retired[0].image, &old.image));
        assert!(controller.presentation_frame(&first, &id).is_none());
        assert!(Arc::ptr_eq(
            &controller.presentation_frame(&next, &id).unwrap().image,
            &fresh.image
        ));
        assert_eq!(controller.accepted[&id].key, next);
    }

    #[test]
    fn background_blur_earlier_color_or_opacity_edit_withdraws_unchanged_geometry() {
        for opacity in [false, true] {
            let mut engine = glass_engine();
            let first = key(&engine);
            let id: DisplayId = "g".into();
            assert!(first.domains[0].source.has_background_blur());
            let mut controller = CompositeController::default();
            let old = ready(&mut controller, first.clone());
            controller.accepted(&first, &id, &old.image);
            let glass_before = engine.scene().get(&"glass".into()).unwrap().object.clone();
            engine
                .transaction(|scene| {
                    scene.update_object(&"r".into(), |object| {
                        if opacity {
                            object.style.opacity = 0.5;
                        } else {
                            object.style.fill = Some(Color::rgb(0, 0, 255).into());
                        }
                    })
                })
                .unwrap();
            assert_eq!(
                engine.scene().get(&"glass".into()).unwrap().object,
                glass_before
            );
            let changed = key(&engine);
            assert!(!first.compatible(&changed, &id));
            assert!(
                !first.domains[0]
                    .source
                    .placement_eq(&changed.domains[0].source)
            );
            let retired = controller.request(changed.clone());
            assert_eq!(retired.len(), 1);
            assert!(Arc::ptr_eq(&retired[0].image, &old.image));
            assert!(controller.frame(&id).is_none());
            let work = controller.take_work().unwrap();
            let late = work.run();
            assert!(matches!(late.as_slice(), [(_, Ok(Some(_)))]));
            engine
                .transaction(|scene| {
                    scene.update_object(&"r".into(), |object| {
                        object.style.fill = Some(Color::rgb(0, 255, 0).into());
                    })
                })
                .unwrap();
            let newest = key(&engine);
            controller.request(newest.clone());
            assert!(work.cancel.load(Ordering::Relaxed));
            controller.finish(changed, late);
            assert!(controller.frame(&id).is_none());
            let fresh = complete(&mut controller, &newest);
            controller.upload_failed(&newest, &id, "simulated upload failure".into());
            assert!(
                controller.frame(&id).is_none(),
                "incompatible backdrop cannot be fallback"
            );
            controller.accepted(&newest, &id, &fresh.image);
            assert!(controller.accepted.is_empty());
        }
    }

    #[test]
    fn background_blur_resource_aba_rejects_already_successful_obsolete_work() {
        let first = animated_glass_key();
        let id = DisplayId::default();
        let mut controller = CompositeController::default();
        controller.request(first.clone());
        let worker = controller.take_work().unwrap();
        let late = worker.run();
        assert!(matches!(late.as_slice(), [(_, Ok(Some(_)))]));
        let mut replacement = (*first).clone();
        replacement.domains[0]
            .images
            .get_mut(&"r".into())
            .unwrap()
            .image = Ok(input_image(2));
        let replacement = Arc::new(replacement);
        assert!(!replacement.animation_only(&first));
        controller.request(replacement);
        controller.request(first.clone());
        assert!(worker.cancel.load(Ordering::Relaxed));
        controller.finish(first.clone(), late);
        assert!(controller.candidates.is_empty());
        assert!(controller.completed.is_none());
        assert!(controller.frame(&id).is_none());
        let current = complete(&mut controller, &first);
        assert!(controller.accepted.is_empty());
        controller.accepted(&first, &id, &current.image);
        assert_eq!(controller.accepted[&id].key, first);
    }

    #[test]
    fn background_blur_seek_or_producer_withdrawal_cancels_and_recovers_whole_canvas() {
        for change in 0..3 {
            let first = animated_glass_key();
            let next = sample(&first, 1);
            let id = DisplayId::default();
            let mut controller = CompositeController::default();
            controller.request(first.clone());
            let old = complete(&mut controller, &first);
            controller.accepted(&first, &id, &old.image);
            controller.request(next.clone());
            let worker = controller.take_work().unwrap();
            let late = worker.run();
            assert!(matches!(late.as_slice(), [(_, Ok(Some(_)))]));
            let mut hard = (*next).clone();
            let observation = hard.domains[0].images.get_mut(&"r".into()).unwrap();
            match change {
                0 => observation.playback_generation += 1,
                1 => observation.image = Err(CompositeError::ImagePending("r".into())),
                _ => {
                    observation.image = Err(CompositeError::ImageUnavailable {
                        image: "r".into(),
                        reason: "producer withdrawn".into(),
                    });
                }
            }
            let hard = Arc::new(hard);
            assert!(!hard.animation_only(&next));
            assert!(!hard.compatible(&first, &id));
            assert_eq!(controller.request(hard.clone()).len(), 1);
            assert!(worker.cancel.load(Ordering::Relaxed));
            controller.finish(next, late);
            assert!(controller.frame(&id).is_none());
            let worker = controller.take_work().unwrap();
            let result = worker.run();
            if change == 0 {
                assert!(matches!(result.as_slice(), [(_, Ok(Some(_)))]));
            } else {
                assert!(matches!(result.as_slice(), [(_, Err(_))]));
            }
            controller.finish(hard.clone(), result);
            if change == 0 {
                let fresh = controller.frame(&id).unwrap().0;
                controller.accepted(&first, &id, &fresh.image);
                assert!(controller.accepted.is_empty());
                controller.accepted(&hard, &id, &fresh.image);
                assert_eq!(controller.accepted[&id].key, hard);
            } else {
                assert!(controller.frame(&id).is_none());
                controller.request(first.clone());
                let recovered = complete(&mut controller, &first);
                controller.accepted(&first, &id, &recovered.image);
                assert!(controller.errors.is_empty());
                assert_eq!(controller.accepted[&id].key, first);
            }
        }
    }

    #[test]
    fn background_blur_canvas_version_rejects_late_pixels_without_scene_revision_change() {
        let mut engine = glass_engine();
        engine
            .transaction(|scene| {
                scene.update_object(&"r".into(), |object| {
                    object.shape = display::Canvas::new(20., 20.).into();
                    object.style.fill = None;
                })
            })
            .unwrap();
        let handle = engine.handle(&"r".into()).unwrap();
        let mut resources = crate::resources::SceneResourceAdapter::new(&mut engine).unwrap();
        let bounds = gpui_flow_scene::Bounds::new(Point::ZERO, Size::new(20., 20.));
        resources
            .edit_canvas(&mut engine, handle, |surface| {
                surface.fill_rect(bounds, Color::rgb(255, 0, 0))
            })
            .unwrap();
        let revision = engine.revision();
        let make_key = |engine: &SceneEngine,
                        resources: &crate::resources::SceneResourceAdapter| {
            Arc::new(
                (*canvas_key(engine, Color::WHITE, false))
                    .clone()
                    .with_resources(engine, Some(resources)),
            )
        };
        let first = make_key(&engine, &resources);
        let id = DisplayId::default();
        let mut controller = CompositeController::default();
        controller.request(first.clone());
        let old = complete(&mut controller, &first);
        assert_eq!(pixel(&old, 8, 8), [0, 0, 255, 255]);
        controller.accepted(&first, &id, &old.image);
        resources
            .edit_canvas(&mut engine, handle, |surface| {
                surface.fill_rect(bounds, Color::rgb(0, 0, 255))
            })
            .unwrap();
        let second = make_key(&engine, &resources);
        assert!(!second.compatible(&first, &id));
        assert_eq!(controller.request(second.clone()).len(), 1);
        let worker = controller.take_work().unwrap();
        let late = worker.run();
        assert!(matches!(late.as_slice(), [(_, Ok(Some(_)))]));
        resources
            .edit_canvas(&mut engine, handle, |surface| {
                surface.fill_rect(bounds, Color::rgb(0, 255, 0))
            })
            .unwrap();
        assert_eq!(engine.revision(), revision);
        let current = make_key(&engine, &resources);
        assert!(!current.animation_only(&second));
        controller.request(current.clone());
        assert!(worker.cancel.load(Ordering::Relaxed));
        controller.finish(second.clone(), late);
        assert!(controller.frame(&id).is_none());
        let fresh = complete(&mut controller, &current);
        assert_eq!(pixel(&fresh, 8, 8), [0, 255, 0, 255]);
        controller.accepted(&second, &id, &fresh.image);
        assert!(controller.accepted.is_empty());
        controller.accepted(&current, &id, &fresh.image);
        assert_eq!(controller.accepted[&id].key, current);
    }

    fn text_engine() -> SceneEngine {
        let mut engine = glass_engine();
        engine
            .transaction(|scene| {
                scene.update_object(&"r".into(), |object| {
                    let mut text = display::Text::new("████████", Size::new(24., 24.));
                    text.font_family = "DejaVu Sans".into();
                    text.font_size = 18.;
                    object.shape = text.into();
                    object.transform = Affine::translation(-24., 0.);
                    object.style = Style::fill(Color::BLACK);
                })?;
                scene.insert(
                    DisplayObject::new("later_text", display::Text::new("B", Size::new(20., 24.)))
                        .with_transform(Affine::translation(120., 0.))
                        .with_style(Style::fill(Color::BLACK)),
                    None,
                    None,
                )
            })
            .unwrap();
        engine
    }

    #[test]
    fn background_blur_prepares_real_full_source_offscreen_prior_and_later_text() {
        let (_app, bridge) = gpui_flow_text::GpuiTextRasterizer::application(
            crate::display::native_text_test_platform(),
        );
        let backend = TextBackendSnapshot::new(&bridge);
        let mut engine = text_engine();
        let active = canvas_key(&engine, Color::WHITE, false);
        let source = &active.domains[0].source;
        let sources = source.text_sources();
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].0.id, DisplayId::from("r"));
        assert_eq!(sources[1].0.id, DisplayId::from("later_text"));
        assert!(sources.iter().all(|(_, _, full_source)| *full_source));
        let prepare = |source: &DomainSource| {
            let mut inputs = TextInputs::default();
            let mut source_bytes = 0;
            let mut remaining_work = CompositeLimits::default().max_surface_writes;
            TextPreparation {
                backend: Some(&backend),
                viewport: active.viewport,
                cancelled: &AtomicBool::new(false),
                retained: 0,
                source_bytes: &mut source_bytes,
                remaining_work: &mut remaining_work,
                limits: CompositeLimits::default(),
            }
            .domain(source, &mut inputs);
            assert_eq!(source_bytes, inputs.byte_len());
            source_bytes
        };
        assert!(
            prepare(source) > 0,
            "real glyph pixels survive outside both viewport edges"
        );
        engine
            .transaction(|scene| {
                scene.update_object(&"glass".into(), |object| object.style.background_blur = 0.)
            })
            .unwrap();
        let zero = canvas_key(&engine, Color::WHITE, false);
        assert!(
            zero.domains[0]
                .source
                .text_sources()
                .iter()
                .all(|(_, _, full_source)| !*full_source)
        );
        assert_eq!(prepare(&zero.domains[0].source), 0);
    }

    #[test]
    fn background_blur_font_epoch_and_backend_withdrawal_reject_whole_frame_success() {
        let (_app, bridge) = gpui_flow_text::GpuiTextRasterizer::application(
            crate::display::native_text_test_platform(),
        );
        let engine = text_engine();
        let make_key = || {
            Arc::new(
                (*canvas_key(&engine, Color::WHITE, false))
                    .clone()
                    .with_text_backend(Some(&bridge)),
            )
        };
        let first = make_key();
        let id = DisplayId::default();
        let mut controller = CompositeController::default();
        controller.request(first.clone());
        let worker = controller.take_work().unwrap();
        let late = worker.run();
        assert!(matches!(late.as_slice(), [(_, Ok(Some(_)))]));
        bridge.invalidate();
        assert!(!first.text_current());
        let current = make_key();
        assert!(!current.compatible(&first, &id));
        assert!(!current.animation_only(&first));
        controller.request(current.clone());
        assert!(worker.cancel.load(Ordering::Relaxed));
        controller.finish(first, late);
        assert!(controller.frame(&id).is_none());
        let fresh = complete(&mut controller, &current);
        controller.accepted(&current, &id, &fresh.image);
        let withdrawn = canvas_key(&engine, Color::WHITE, false);
        assert_eq!(controller.request(withdrawn.clone()).len(), 1);
        assert!(controller.frame(&id).is_none());
        let worker = controller.take_work().unwrap();
        let failed = worker.run();
        assert!(matches!(
            failed.as_slice(),
            [(_, Err(CompositeError::TextBackendRequired(_)))]
        ));
        controller.finish(withdrawn, failed);
        assert!(controller.candidates.is_empty());
        assert!(controller.frame(&id).is_none());
        let recovered = make_key();
        controller.request(recovered.clone());
        let fresh = complete(&mut controller, &recovered);
        controller.accepted(&recovered, &id, &fresh.image);
        assert_eq!(controller.accepted[&id].key, recovered);
    }

    #[test]
    fn background_blur_direct_native_renderer_rejects_effect_and_preserves_zero_path() {
        for radius in [0., -0., 1.] {
            let mut object = glass();
            object.style.background_blur = radius;
            let render = crate::display::RenderObject {
                object,
                screen_transform: Affine::IDENTITY,
                viewport_transform: Affine::IDENTITY,
                opacity: 1.,
                clips: vec![],
            };
            assert_eq!(render.native_renderable(), radius == 0.);
            assert_eq!(
                render.diagnostics().iter().any(|issue| {
                    issue.feature == crate::display::UnsupportedFeature::CompositingDomain
                }),
                radius > 0.
            );
        }
    }
}

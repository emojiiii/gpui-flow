use super::*;
use crate::display::{CustomRasterOutput, CustomRasterRequest, CustomRasterizer};
use gpui_flow_scene::{
    Affine, BlendMode, Bounds as SceneBounds, DisplayObject, Point, Scene, Shape, Size, Style,
    display,
};
use std::sync::atomic::AtomicUsize;
fn b() -> SceneBounds {
    SceneBounds::new(Point::ZERO, Size::new(8., 8.))
}
fn engine() -> SceneEngine {
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
        DisplayObject::new("custom", display::Custom::new("host", b(), vec![])),
        Some("g".into()),
        None,
    )
    .unwrap();
    SceneEngine::new(s).unwrap()
}
fn key(e: &SceneEngine, r: &CustomRasterizers) -> Arc<RequestKey> {
    let source =
        gpui_flow_composite::capture_subtree(e.scene(), &"g".into(), CompositeLimits::default())
            .unwrap();
    Arc::new(
        RequestKey::new(
            vec![source],
            e,
            RasterViewport {
                world_to_screen: Affine::IDENTITY,
                canvas_bounds: b(),
                content_mask: b(),
                device_scale: 1.,
            },
            1,
        )
        .with_resources(e, None)
        .with_custom_rasterizers(r),
    )
}
struct Producer {
    calls: Arc<AtomicUsize>,
    mode: u8,
}
impl CustomRasterizer for Producer {
    fn rasterize(
        &self,
        request: CustomRasterRequest<'_>,
    ) -> Result<Option<CustomRasterOutput>, String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        assert!(!request.cancellation.is_cancelled());
        assert_eq!(request.object.id, "custom".into());
        if self.mode == 1 {
            return Ok(None);
        }
        if self.mode == 2 {
            panic!("intentional Custom producer panic");
        }
        if self.mode == 3 {
            return Err("failed".into());
        }
        let mut rgba = vec![255; 8 * 8 * 4];
        if self.mode == 4 {
            rgba[3] = 1;
        }
        if self.mode == 5 {
            rgba.reserve(1024);
        }
        Ok(Some(CustomRasterOutput {
            width: 8,
            height: 8,
            premul_rgba: rgba,
            local_bounds: if self.mode == 6 {
                SceneBounds::new(Point::new(1., 0.), Size::new(8., 8.))
            } else {
                b()
            },
            pixel_work: 64,
        }))
    }
}
fn registered(mode: u8) -> (CustomRasterizers, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut r = CustomRasterizers::default();
    r.set(
        "host".into(),
        Some(Arc::new(Producer {
            calls: calls.clone(),
            mode,
        })),
    );
    (r, calls)
}
fn finish(c: &mut CompositeController) {
    let w = c.take_work().unwrap();
    c.finish(w.key.clone(), w.run());
}
#[test]
fn custom_worker_requires_explicit_bridge_and_accepts_only_complete_uploaded_domain() {
    let e = engine();
    let (r, calls) = registered(0);
    let first = key(&e, &r);
    assert!(first.domains[0].customs[&"custom".into()].ticket.is_none());
    let mut c = CompositeController::default();
    c.request_coherent(first.clone(), false);
    finish(&mut c);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(c.publish_presentation(&first).is_none());
    let uploads = c.presentation_uploads(&first);
    assert_eq!(uploads.len(), 1);
    assert!(c.presentation_frame(&first, &"g".into()).is_none());
    c.presentation_uploaded(&first, &uploads[0].0, &uploads[0].1.image);
    assert!(c.publish_presentation(&first).is_some());
    assert!(c.presentation_frame(&first, &"g".into()).is_some());
    let missing = key(&e, &CustomRasterizers::default());
    assert_eq!(c.request(missing.clone()).len(), 1);
    finish(&mut c);
    assert!(c.frame(&"g".into()).is_none());
    assert_eq!(
        c.diagnostics()[0].error,
        CompositeError::CustomBackendRequired("custom".into())
    );
}
#[test]
fn registration_replacement_invalidation_and_drop_revoke_worker_candidates_and_coherent_frames() {
    for stage in 0..4 {
        let e = engine();
        let (mut r, _) = registered(0);
        let first = key(&e, &r);
        let mut c = CompositeController::default();
        c.request_coherent(first.clone(), false);
        let w = c.take_work().unwrap();
        let result = w.run();
        if stage > 0 {
            c.finish(first.clone(), result);
        }
        if stage > 1 {
            for (id, frame) in c.presentation_uploads(&first) {
                c.presentation_uploaded(&first, &id, &frame.image);
            }
        }
        if stage > 2 {
            assert!(c.publish_presentation(&first).is_some());
        }
        assert!(r.invalidate("host"));
        assert!(!first.inputs_current());
        if stage == 0 {
            c.finish(first.clone(), w.run());
        }
        assert!(c.frame(&"g".into()).is_none());
        assert!(c.old_frame(&"g".into()).is_none());
        assert!(c.presentation_uploads(&first).is_empty());
        assert!(c.publish_presentation(&first).is_none());
        assert!(c.presentation_frame(&first, &"g".into()).is_none());
        let fresh = key(&e, &r);
        assert_ne!(first, fresh);
        c.request_coherent(fresh.clone(), false);
        finish(&mut c);
        assert!(c.frame(&"g".into()).is_some());
        drop(r);
        assert!(!fresh.inputs_current());
        assert!(c.frame(&"g".into()).is_none());
    }
    let same: Arc<dyn CustomRasterizer> = Arc::new(Producer {
        calls: Arc::default(),
        mode: 0,
    });
    let e_same = engine();
    let mut same_registry = CustomRasterizers::default();
    same_registry.set("host".into(), Some(same.clone()));
    let old = key(&e_same, &same_registry);
    let mut same_controller = CompositeController::default();
    same_controller.request(old.clone());
    finish(&mut same_controller);
    let (frame, _) = same_controller.frame(&"g".into()).unwrap();
    same_controller.accepted(&old, &"g".into(), &frame.image);
    same_registry.set("host".into(), Some(same));
    assert!(!old.inputs_current());
    assert!(same_controller.old_frame(&"g".into()).is_none());
    assert!(same_controller.frame(&"g".into()).is_none());
    assert!(
        same_controller
            .presentation_frame(&old, &"g".into())
            .is_none()
    );
    let e = engine();
    let (mut r, _) = registered(0);
    let a = key(&e, &r);
    let producer = r.get("host").unwrap();
    r.set("host".into(), None);
    r.set(
        "host".into(),
        Some(Arc::new(Producer {
            calls: Arc::default(),
            mode: 0,
        })),
    );
    assert!(!producer.current());
    assert!(!a.inputs_current());
    assert_ne!(a, key(&e, &r));
}
#[test]
fn canonical_data_geometry_viewport_and_dpr_changes_reject_late_custom_work() {
    for change in 0..5 {
        let mut e = engine();
        let (r, _) = registered(0);
        let first = key(&e, &r);
        let mut c = CompositeController::default();
        c.request(first.clone());
        let old = c.take_work().unwrap();
        let result = old.run();
        if change < 3 {
            e.transaction(|s| {
                s.update_object(&"custom".into(), |o| match change {
                    0 => {
                        if let Shape::Custom(shape) = &mut o.shape {
                            shape.data = serde_json::json!({"n":2});
                        }
                    }
                    1 => o.transform = Affine::translation(1., 0.),
                    _ => {
                        if let Shape::Custom(shape) = &mut o.shape {
                            shape.bounds.size.width = 7.;
                        }
                    }
                })
            })
            .unwrap();
        }
        let mut next = (*key(&e, &r)).clone();
        if change == 3 {
            next.viewport.world_to_screen = Affine::scale(2., 2.);
        }
        if change == 4 {
            next.viewport.device_scale = 2.;
        }
        let next = Arc::new(next);
        assert!(!first.presentation_current(&e, None, None, &r, next.viewport, next.window));
        assert!(!next.compatible(&first, &"g".into()));
        c.request(next);
        assert!(old.cancel.load(Ordering::Relaxed));
        c.finish(first, result);
        assert!(c.frame(&"g".into()).is_none());
    }
}
#[test]
fn custom_errors_empty_output_and_shared_capacity_are_terminal_and_atomic() {
    for mode in 1..7 {
        let e = engine();
        let (r, calls) = registered(mode);
        let first = key(&e, &r);
        let mut c = CompositeController::default();
        c.request(first.clone());
        if mode == 5 {
            c.retired_bytes = CompositeLimits::default().max_surface_bytes - 256;
        }
        finish(&mut c);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(c.frame(&"g".into()).is_none());
        assert!(c.take_work().is_none());
        if mode == 1 {
            assert!(c.diagnostics().is_empty());
        } else {
            let issue = &c.diagnostics()[0].error;
            assert!(match mode {
                2 | 3 => matches!(issue, CompositeError::CustomRasterization { .. }),
                4 => *issue == CompositeError::InvalidImage,
                5 => *issue == CompositeError::WorkingSetLimit,
                6 => matches!(issue, CompositeError::InvalidGeometry(_)),
                _ => false,
            });
        }
    }
}
#[test]
fn custom_input_capacity_combines_with_images_text_and_retained_frames() {
    let e = engine();
    let (r, _) = registered(0);
    let first = key(&e, &r);
    let domain = &first.domains[0];
    for (source_bytes, retained, work, error) in [
        (
            32 * 1024 * 1024 - 255,
            0,
            1000,
            CompositeError::WorkingSetLimit,
        ),
        (
            0,
            CompositeLimits::default().max_surface_bytes - 255,
            1000,
            CompositeError::WorkingSetLimit,
        ),
        (0, 0, 63, CompositeError::WorkLimit),
    ] {
        let mut bytes = source_bytes;
        let mut writes = work;
        let mut inputs = CustomInputs::default();
        CustomPreparation {
            viewport: first.viewport,
            cancelled: &AtomicBool::new(false),
            retained,
            source_bytes: &mut bytes,
            remaining_work: &mut writes,
            limits: CompositeLimits::default(),
        }
        .domain(&domain.source, &domain.customs, &mut inputs);
        let got = rasterize_with_custom_inputs(
            &domain.source,
            &ImageInputs::default(),
            &TextInputs::default(),
            &inputs,
            first.viewport,
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false),
        );
        assert_eq!(got.unwrap_err(), error);
        assert_eq!(bytes, source_bytes);
    }
}

#[test]
fn uploaded_custom_candidates_do_not_survive_document_window_or_canonical_a_b_a() {
    for change in 0..3 {
        let mut e = engine();
        let (r, _) = registered(0);
        let first = key(&e, &r);
        let mut c = CompositeController::default();
        c.request_coherent(first.clone(), false);
        finish(&mut c);
        let uploads = c.presentation_uploads(&first);
        for (id, frame) in &uploads {
            c.presentation_uploaded(&first, id, &frame.image);
        }
        let original = e.scene().clone();
        if change == 0 {
            e.reset_scene(original.clone()).unwrap();
        }
        if change == 2 {
            e.transaction(|s| {
                s.update_object(&"custom".into(), |o| {
                    if let Shape::Custom(shape) = &mut o.shape {
                        shape.data = serde_json::json!({"new":1});
                    }
                })
            })
            .unwrap();
            e.replace_scene(original).unwrap();
        }
        let mut next = (*key(&e, &r)).clone();
        if change == 1 {
            next.window += 1;
        }
        let next = Arc::new(next);
        assert!(!first.presentation_current(&e, None, None, &r, next.viewport, next.window));
        c.request_coherent(next.clone(), false);
        assert!(c.publish_presentation(&first).is_none());
        assert!(c.presentation_uploads(&first).is_empty());
        assert!(c.presentation_frame(&first, &"g".into()).is_none());
        assert!(c.frame(&"g".into()).is_none());
        for (id, frame) in &uploads {
            c.presentation_uploaded(&first, id, &frame.image);
            assert!(c.accepted(&first, id, &frame.image).is_none());
        }
        assert!(c.publish_presentation(&next).is_none());
    }
}

#[test]
fn invalidation_during_host_callback_discards_output_and_exposes_cooperative_cancel() {
    struct Blocked {
        entered: Arc<std::sync::Barrier>,
        release: Arc<std::sync::Barrier>,
    }
    impl CustomRasterizer for Blocked {
        fn rasterize(
            &self,
            request: CustomRasterRequest<'_>,
        ) -> Result<Option<CustomRasterOutput>, String> {
            self.entered.wait();
            self.release.wait();
            assert!(request.cancellation.is_cancelled());
            Ok(Some(CustomRasterOutput {
                width: 1,
                height: 1,
                premul_rgba: vec![255; 4],
                local_bounds: SceneBounds::new(Point::ZERO, Size::new(1., 1.)),
                pixel_work: 1,
            }))
        }
    }
    let entered = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    let mut registry = CustomRasterizers::default();
    registry.set(
        "host".into(),
        Some(Arc::new(Blocked {
            entered: entered.clone(),
            release: release.clone(),
        })),
    );
    let e = engine();
    let first = key(&e, &registry);
    let mut controller = CompositeController::default();
    controller.request(first.clone());
    let work = controller.take_work().unwrap();
    let result = std::thread::scope(|scope| {
        let worker = scope.spawn(|| work.run());
        entered.wait();
        registry.invalidate("host");
        release.wait();
        worker.join().unwrap()
    });
    assert_eq!(
        result[0].1.as_ref().unwrap_err(),
        &CompositeError::Superseded
    );
    controller.finish(first, result);
    assert!(controller.candidates.is_empty());
    assert!(controller.frame(&"g".into()).is_none());
}

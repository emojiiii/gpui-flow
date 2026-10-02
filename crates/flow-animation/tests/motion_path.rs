use gpui_flow_animation::*;
use gpui_flow_engine::{
    ChangeOrigin, ElementHandle, EngineError, LayoutProvider, PropertyKey, SceneEngine,
};
use gpui_flow_scene::{
    Affine, DisplayObject, Point, Scene, Shape,
    display::{Group, Path, Pen, Rect},
};
use serde_json::json;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}
fn engine() -> SceneEngine {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("source-parent", Group::new()),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("path", Path::from_svg("M0 0L100 0").unwrap()),
            Some("source-parent".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("target-parent", Group::new()),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("target", Rect::new(10., 20.))
                .with_transform(Affine::translation(-10., 5.)),
            Some("target-parent".into()),
            None,
        )
        .unwrap();
    SceneEngine::new(scene).unwrap()
}
fn h(e: &SceneEngine, id: &str) -> ElementHandle {
    e.handle(&id.into()).unwrap()
}
fn binding(e: &SceneEngine) -> MotionPathAnimation {
    MotionPathAnimation::new(
        e,
        h(e, "target"),
        h(e, "path"),
        MotionPathSpec::new(1000, 0., 100.),
    )
    .unwrap()
}
fn position(e: &SceneEngine) -> Point {
    e.local_to_world(&"target".into(), Point::ZERO).unwrap()
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.005, "{a} != {b}");
}
fn point(a: Point, b: Point) {
    near(a.x, b.x);
    near(a.y, b.y);
}
fn status(a: &mut MotionPathAnimation, e: &SceneEngine) -> MotionPathStatus {
    a.info(e).unwrap().status
}

#[test]
fn fake_clock_pause_seek_endpoints_and_terminal_cancel() {
    let mut e = engine();
    let mut a = binding(&e);
    let before = e.scene().clone();
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), 0);
    let report = a.tick(&mut e, ms(250)).unwrap();
    assert_eq!(report.info.elapsed, ms(250));
    assert_eq!(report.changes.origin, ChangeOrigin::Animation);
    point(position(&e), Point::new(25., 0.));
    a.pause(&e).unwrap();
    let revision = e.revision();
    assert!(!a.tick(&mut e, Duration::MAX).unwrap().changes.changed());
    assert_eq!(e.revision(), revision);
    a.seek(&mut e, ms(500)).unwrap();
    assert_eq!(status(&mut a, &e), MotionPathStatus::Paused);
    point(position(&e), Point::new(50., 0.));
    a.resume(&e).unwrap();
    a.tick(&mut e, Duration::MAX).unwrap();
    point(position(&e), Point::new(100., 0.));
    assert_eq!(status(&mut a, &e), MotionPathStatus::Completed);
    assert_eq!(a.info(&e).unwrap().elapsed, ms(1000));
    a.seek(&mut e, ms(100)).unwrap();
    assert_eq!(status(&mut a, &e), MotionPathStatus::Paused);
    a.cancel(&e).unwrap();
    let before = e.scene().clone();
    assert!(a.seek(&mut e, ms(1000)).is_err());
    assert!(a.resume(&e).is_err());
    a.tick(&mut e, ms(1000)).unwrap();
    assert_eq!(e.scene(), &before);
}

#[test]
fn affine_curve_frames_preserve_anchor_scale_shear_and_reflection_for_path_and_pen() {
    for pen in [false, true] {
        for rotation in [None, Some(37.)] {
            let mut e = engine();
            e.transaction(|s| {
                s.update_object(&"source-parent".into(), |o| {
                    o.transform = Affine::translation(1070., 1020.)
                        * Affine::skew(23., -8.)
                        * Affine::scale(-2., 3.)
                })?;
                s.update_object(&"target-parent".into(), |o| {
                    o.transform = Affine::translation(-40., 9.)
                        * Affine::rotation(27.)
                        * Affine::scale(0.7, 1.4)
                })?;
                s.update_object(&"target".into(), |o| {
                    o.transform =
                        Affine::rotation(-15.) * Affine::skew(12., 4.) * Affine::scale(2., -3.)
                })?;
                let path = Path::from_svg("M0 0C0 60 80 -10 100 40").unwrap();
                s.update_object(&"path".into(), |o| {
                    o.shape = if pen {
                        Shape::Pen(Pen { path })
                    } else {
                        Shape::Path(path)
                    }
                })
            })
            .unwrap();
            let p = h(&e, "path");
            let t = h(&e, "target");
            let before = e.world_transform(&"target".into()).unwrap();
            let mut spec = MotionPathSpec::new(1000, 0., e.motion_total(p).unwrap());
            spec.anchor = Point::new(5., 10.);
            spec.rotation = rotation;
            let mut a = MotionPathAnimation::new(&e, t, p, spec.clone()).unwrap();
            for _ in 0..20 {
                let report = a.tick(&mut e, ms(50)).unwrap();
                let sample = e
                    .motion_point(p, report.info.distance.unwrap())
                    .unwrap()
                    .unwrap();
                let after = e.world_transform(&"target".into()).unwrap();
                point(after.transform_point(spec.anchor), sample.point);
                near(
                    after.a * after.a + after.b * after.b,
                    before.a * before.a + before.b * before.b,
                );
                near(
                    after.c * after.c + after.d * after.d,
                    before.c * before.c + before.d * before.d,
                );
                near(
                    after.a * after.c + after.b * after.d,
                    before.a * before.c + before.b * before.d,
                );
                assert_eq!(before.determinant().signum(), after.determinant().signum());
                if let Some(offset) = rotation {
                    let angle = (sample.rotation_degrees + offset).to_radians();
                    point(
                        Point::new(after.a, after.b) / after.a.hypot(after.b),
                        Point::new(angle.cos(), angle.sin()),
                    );
                } else {
                    near(before.a, after.a);
                    near(before.b, after.b);
                    near(before.c, after.c);
                    near(before.d, after.d);
                }
            }
        }
    }
}

#[test]
fn local_distance_clamps_but_never_turns_into_world_distance() {
    let mut e = engine();
    let p = h(&e, "path");
    let t = h(&e, "target");
    e.set_property(p, PropertyKey::Transform, json!(Affine::scale(3., 1.)))
        .unwrap();
    let mut a = MotionPathAnimation::new(&e, t, p, MotionPathSpec::new(1000, -100., 200.)).unwrap();
    a.seek(&mut e, Duration::ZERO).unwrap();
    point(position(&e), Point::ZERO);
    a.seek(&mut e, ms(500)).unwrap();
    point(position(&e), Point::new(150., 0.));
    a.seek(&mut e, Duration::MAX).unwrap();
    point(position(&e), Point::new(300., 0.));
}

#[test]
fn source_edits_and_parent_edits_cancel_before_a_write() {
    for id in ["path", "source-parent", "target-parent"] {
        let mut e = engine();
        let mut a = binding(&e);
        a.tick(&mut e, ms(100)).unwrap();
        let handle = h(&e, id);
        e.set_property(
            handle,
            PropertyKey::Transform,
            json!(Affine::translation(40., 30.)),
        )
        .unwrap();
        let before = e.scene().clone();
        let revision = e.revision();
        let expected = if id == "target-parent" {
            MotionPathCancelReason::TargetParentChanged
        } else {
            MotionPathCancelReason::SourceChanged
        };
        assert_eq!(
            a.tick(&mut e, ms(200)).unwrap().info.status,
            MotionPathStatus::Cancelled(expected)
        );
        assert_eq!(e.scene(), &before);
        assert_eq!(e.revision(), revision);
        assert!(a.seek(&mut e, ms(800)).is_err());
    }
    let mut e = engine();
    let mut a = binding(&e);
    let p = h(&e, "path");
    e.set_path_data(p, Path::from_svg("M0 0L0 100").unwrap())
        .unwrap();
    assert_eq!(
        status(&mut a, &e),
        MotionPathStatus::Cancelled(MotionPathCancelReason::SourceChanged)
    );
}

#[test]
fn overrides_after_pause_or_completion_are_never_overwritten() {
    for complete in [false, true] {
        let mut e = engine();
        let mut a = binding(&e);
        a.tick(&mut e, if complete { ms(1000) } else { ms(100) })
            .unwrap();
        if !complete {
            a.pause(&e).unwrap();
        }
        let t = h(&e, "target");
        e.set_property(
            t,
            PropertyKey::Transform,
            json!(Affine::translation(999., 7.)),
        )
        .unwrap();
        let before = e.scene().clone();
        assert!(a.seek(&mut e, ms(500)).is_err());
        assert_eq!(
            status(&mut a, &e),
            MotionPathStatus::Cancelled(MotionPathCancelReason::TargetOverridden)
        );
        assert_eq!(e.scene(), &before);
    }
}

#[test]
fn unrelated_low_level_style_edits_do_not_take_over_transform() {
    let mut e = engine();
    let mut a = binding(&e);
    for id in ["target", "path"] {
        let handle = h(&e, id);
        e.set_property(handle, PropertyKey::Opacity, json!(0.4))
            .unwrap();
    }
    a.tick(&mut e, ms(500)).unwrap();
    point(position(&e), Point::new(50., 0.));
    assert_eq!(status(&mut a, &e), MotionPathStatus::Playing);
    assert_eq!(e.object(h(&e, "target")).unwrap().style.opacity, 0.4);
}

#[test]
fn destroy_recreate_detach_remount_and_reset_cannot_retarget() {
    for id in ["target", "path"] {
        for destroy in [false, true] {
            let mut e = engine();
            let mut a = binding(&e);
            let handle = h(&e, id);
            if destroy {
                let node = e.scene().get(&id.into()).unwrap().clone();
                e.destroy(handle).unwrap();
                e.transaction(|s| s.insert(node.object, node.parent, None))
                    .unwrap();
            } else {
                let (tree, _) = e.detach(handle).unwrap();
                e.attach(&tree, None, None).unwrap();
            }
            let before = e.scene().clone();
            let expected = if id == "target" {
                MotionPathCancelReason::TargetUnavailable
            } else {
                MotionPathCancelReason::SourceUnavailable
            };
            assert_eq!(
                a.tick(&mut e, ms(100)).unwrap().info.status,
                MotionPathStatus::Cancelled(expected)
            );
            assert_eq!(e.scene(), &before);
        }
    }
    let mut e = engine();
    let mut a = binding(&e);
    e.reset_scene(e.scene().clone()).unwrap();
    assert_eq!(
        status(&mut a, &e),
        MotionPathStatus::Cancelled(MotionPathCancelReason::TargetUnavailable)
    );
}

#[test]
fn reparenting_with_equivalent_world_pose_is_still_a_dependency_change() {
    let mut e = engine();
    let mut a = binding(&e);
    let p = h(&e, "path");
    e.reparent(p, None, None, true).unwrap();
    assert_eq!(
        status(&mut a, &e),
        MotionPathStatus::Cancelled(MotionPathCancelReason::SourceChanged)
    );
    let mut e = engine();
    let mut a = binding(&e);
    let t = h(&e, "target");
    e.reparent(t, None, None, true).unwrap();
    assert_eq!(
        status(&mut a, &e),
        MotionPathStatus::Cancelled(MotionPathCancelReason::TargetParentChanged)
    );
}

#[test]
fn invalid_inputs_dependencies_and_singular_parents_reject_without_mutation() {
    let mut e = engine();
    let t = h(&e, "target");
    let p = h(&e, "path");
    let other = engine();
    let before = e.scene().clone();
    let mut specs = vec![
        MotionPathSpec::new(0, 0., 100.),
        MotionPathSpec::new(1000, f64::NAN, 100.),
        MotionPathSpec::new(1000, 0., f64::INFINITY),
    ];
    let mut spec = MotionPathSpec::new(1000, 0., 100.);
    spec.anchor.x = f32::INFINITY;
    specs.push(spec);
    let mut spec = MotionPathSpec::new(1000, 0., 100.);
    spec.rotation = Some(f32::NAN);
    specs.push(spec);
    let mut spec = MotionPathSpec::new(1000, 0., 100.);
    spec.timing.iterations = IterationCount::Finite(0);
    specs.push(spec);
    for spec in specs {
        assert!(MotionPathAnimation::new(&e, t, p, spec).is_err());
    }
    for (t, p) in [
        (p, p),
        (h(&e, "source-parent"), p),
        (t, t),
        (h(&other, "target"), p),
        (t, h(&other, "path")),
    ] {
        assert!(MotionPathAnimation::new(&e, t, p, MotionPathSpec::new(1000, 0., 100.)).is_err());
    }
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), 0);
    let parent = h(&e, "target-parent");
    e.set_property(parent, PropertyKey::Transform, json!(Affine::scale(0., 1.)))
        .unwrap();
    assert!(MotionPathAnimation::new(&e, t, p, MotionPathSpec::new(1000, 0., 100.)).is_err());
}

#[test]
fn empty_paths_collapsed_source_tangent_and_collapsed_target_axis_reject() {
    for path in ["", "M0 0", "M0 0L0 0", "M0 0Q0 0 0 0"] {
        let mut e = engine();
        let p = h(&e, "path");
        e.set_path_data(p, Path::from_svg(path).unwrap()).unwrap();
        assert!(
            MotionPathAnimation::new(&e, h(&e, "target"), p, MotionPathSpec::new(1000, 0., 100.))
                .is_err()
        );
    }
    let mut e = engine();
    let p = h(&e, "path");
    e.set_property(p, PropertyKey::Transform, json!(Affine::scale(0., 1.)))
        .unwrap();
    assert!(
        MotionPathAnimation::new(&e, h(&e, "target"), p, MotionPathSpec::new(1000, 0., 100.))
            .is_err()
    );
    let mut e = engine();
    let t = h(&e, "target");
    let p = h(&e, "path");
    e.set_property(t, PropertyKey::Transform, json!(Affine::scale(0., 1.)))
        .unwrap();
    let mut spec = MotionPathSpec::new(1000, 0., 100.);
    spec.rotation = Some(0.);
    assert!(MotionPathAnimation::new(&e, t, p, spec).is_err());
    let mut a = MotionPathAnimation::new(&e, t, p, MotionPathSpec::new(1000, 0., 100.)).unwrap();
    a.tick(&mut e, ms(500)).unwrap();
    point(position(&e), Point::new(50., 0.));
}

#[test]
fn degenerate_curve_endpoints_and_disjoint_subpaths_reuse_engine_sampling() {
    for path in [
        "M0 0C0 0 100 0 100 0",
        "M0 0L30 0M100 100L100 170",
        "M0 0Q100 0 0 0",
    ] {
        let mut e = engine();
        let p = h(&e, "path");
        e.set_path_data(p, Path::from_svg(path).unwrap()).unwrap();
        let total = e.motion_total(p).unwrap();
        let mut spec = MotionPathSpec::new(1000, 0., total);
        spec.rotation = Some(0.);
        let mut a = MotionPathAnimation::new(&e, h(&e, "target"), p, spec).unwrap();
        for n in [0, 250, 500, 501, 750, 1000] {
            a.seek(&mut e, ms(n)).unwrap();
            let sample = e
                .motion_point(p, a.info(&e).unwrap().distance.unwrap())
                .unwrap()
                .unwrap();
            point(position(&e), sample.point);
        }
    }
}

#[test]
fn delay_reverse_easing_repeat_gap_and_terminal_endpoint_share_timing() {
    let mut e = engine();
    let original = e.object(h(&e, "target")).unwrap().transform;
    let mut spec = MotionPathSpec::new(1000, 0., 100.);
    spec.easing = Easing::EaseIn;
    spec.timing = AnimationTiming {
        delay_ms: 100,
        repeat_delay_ms: 200,
        iterations: IterationCount::Finite(2),
        direction: PlaybackDirection::AlternateReverse,
        ..Default::default()
    };
    let mut a = MotionPathAnimation::new(&e, h(&e, "target"), h(&e, "path"), spec).unwrap();
    assert!(!a.tick(&mut e, ms(50)).unwrap().changes.changed());
    assert_eq!(a.info(&e).unwrap().distance, None);
    a.seek(&mut e, ms(100)).unwrap();
    point(position(&e), Point::new(100., 0.));
    a.seek(&mut e, ms(600)).unwrap();
    point(position(&e), Point::new(87.5, 0.));
    a.seek(&mut e, ms(1150)).unwrap();
    point(position(&e), Point::ZERO);
    assert_eq!(a.timing_sample(&e).unwrap().phase, TimingPhase::RepeatDelay);
    a.seek(&mut e, Duration::MAX).unwrap();
    point(position(&e), Point::new(100., 0.));
    assert_eq!(a.info(&e).unwrap().elapsed, ms(2300));
    a.seek(&mut e, ms(50)).unwrap();
    assert_eq!(e.object(h(&e, "target")).unwrap().transform, original);
    assert_eq!(status(&mut a, &e), MotionPathStatus::Paused);
}

#[test]
fn fractional_clock_carry_pause_rate_reset_and_overflow_are_atomic() {
    let mut e = engine();
    let mut spec = MotionPathSpec::new(1000, 0., 100.);
    spec.timing.speed = PlaybackRate::new(1, 3).unwrap();
    spec.timing.iterations = IterationCount::Infinite;
    let mut a = MotionPathAnimation::new(&e, h(&e, "target"), h(&e, "path"), spec).unwrap();
    a.tick(&mut e, Duration::from_nanos(1)).unwrap();
    a.pause(&e).unwrap();
    a.tick(&mut e, Duration::from_nanos(100)).unwrap();
    a.resume(&e).unwrap();
    a.tick(&mut e, Duration::from_nanos(2)).unwrap();
    assert_eq!(a.info(&e).unwrap().elapsed, Duration::from_nanos(1));
    a.tick(&mut e, Duration::from_nanos(2)).unwrap();
    a.set_playback_rate(&e, PlaybackRate::default()).unwrap();
    a.tick(&mut e, Duration::from_nanos(1)).unwrap();
    assert_eq!(a.info(&e).unwrap().elapsed, Duration::from_nanos(2));
    a.seek(&mut e, Duration::MAX).unwrap();
    let before = e.scene().clone();
    let revision = e.revision();
    assert!(a.tick(&mut e, Duration::from_nanos(1)).is_err());
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), revision);
    assert_eq!(a.info(&e).unwrap().elapsed, Duration::MAX);
}

struct Override {
    enabled: Arc<AtomicBool>,
    id: &'static str,
}
impl LayoutProvider for Override {
    fn derive(&self, _: &SceneEngine, candidate: &Scene) -> Result<Scene, EngineError> {
        let mut next = candidate.clone();
        if self.enabled.load(Ordering::SeqCst) {
            next.update_object(&self.id.into(), |o| o.transform.tx = 999.)?;
        }
        Ok(next)
    }
}
#[test]
fn layout_failure_is_atomic_for_document_clock_notifications_and_fractional_carry() {
    for id in ["target", "path", "source-parent", "target-parent"] {
        let mut e = engine();
        let enabled = Arc::new(AtomicBool::new(false));
        e.install_layout_provider(Override {
            enabled: enabled.clone(),
            id,
        })
        .unwrap();
        let mut spec = MotionPathSpec::new(1000, 0., 100.);
        spec.timing.speed = PlaybackRate::new(1, 3).unwrap();
        let mut a = MotionPathAnimation::new(&e, h(&e, "target"), h(&e, "path"), spec).unwrap();
        a.tick(&mut e, Duration::from_nanos(1)).unwrap();
        let calls = Rc::new(RefCell::new(0));
        let log = calls.clone();
        e.on_change(None, move |_| *log.borrow_mut() += 1).unwrap();
        let before = e.scene().clone();
        let revision = e.revision();
        let info = a.info(&e).unwrap();
        enabled.store(true, Ordering::SeqCst);
        assert!(a.tick(&mut e, Duration::from_millis(1)).is_err());
        assert_eq!(e.scene(), &before);
        assert_eq!(e.revision(), revision);
        assert_eq!(*calls.borrow(), 0);
        assert_eq!(a.info(&e).unwrap(), info);
        enabled.store(false, Ordering::SeqCst);
        a.tick(&mut e, Duration::from_nanos(1)).unwrap();
        assert_eq!(a.info(&e).unwrap().elapsed, Duration::ZERO);
        a.tick(&mut e, Duration::from_nanos(1)).unwrap();
        assert_eq!(a.info(&e).unwrap().elapsed, Duration::from_nanos(1));
    }
}

#[test]
fn independent_binding_conflict_cancels_the_previous_writer_on_observation() {
    let mut e = engine();
    let mut first = binding(&e);
    let mut second = binding(&e);
    first.tick(&mut e, ms(100)).unwrap();
    assert_eq!(
        second.tick(&mut e, ms(200)).unwrap().info.status,
        MotionPathStatus::Cancelled(MotionPathCancelReason::TargetOverridden)
    );
    point(position(&e), Point::new(10., 0.));
}

#[test]
fn huge_finite_distance_endpoints_interpolate_without_subtraction_overflow() {
    let mut e = engine();
    let mut a = MotionPathAnimation::new(
        &e,
        h(&e, "target"),
        h(&e, "path"),
        MotionPathSpec::new(1000, -f64::MAX, f64::MAX),
    )
    .unwrap();
    a.seek(&mut e, ms(500)).unwrap();
    point(position(&e), Point::ZERO);
    a.seek(&mut e, ms(1000)).unwrap();
    point(position(&e), Point::new(100., 0.));
}

#[test]
fn kind_replacement_from_derived_layout_rejects_before_identity_or_clock_changes() {
    struct ReplaceTarget;
    impl LayoutProvider for ReplaceTarget {
        fn derive(&self, _: &SceneEngine, candidate: &Scene) -> Result<Scene, EngineError> {
            let mut next = candidate.clone();
            if next.get(&"target".into()).unwrap().object.transform.tx > 0. {
                next.update_object(&"target".into(), |o| {
                    o.shape = Shape::Ellipse(gpui_flow_scene::display::Ellipse::new(10., 20.));
                })?;
            }
            Ok(next)
        }
    }
    let mut e = engine();
    e.install_layout_provider(ReplaceTarget).unwrap();
    let target = h(&e, "target");
    let mut a = binding(&e);
    let before = e.scene().clone();
    let revision = e.revision();
    assert!(a.tick(&mut e, ms(500)).is_err());
    assert_eq!(h(&e, "target"), target);
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), revision);
    assert_eq!(a.info(&e).unwrap().elapsed, Duration::ZERO);
}

#[test]
fn constructor_rejects_detached_and_destroyed_handles_without_reserving_any_state() {
    for id in ["target", "path"] {
        let mut e = engine();
        let target = h(&e, "target");
        let path = h(&e, "path");
        let handle = h(&e, id);
        let (detached, _) = e.detach(handle).unwrap();
        assert!(
            MotionPathAnimation::new(&e, target, path, MotionPathSpec::new(1000, 0., 100.))
                .is_err()
        );
        e.destroy_detached(&detached).unwrap();
        assert!(
            MotionPathAnimation::new(&e, target, path, MotionPathSpec::new(1000, 0., 100.))
                .is_err()
        );
    }
}

#[test]
fn collapsed_interior_tangent_rejects_frame_without_clock_or_document_commit() {
    let mut e = engine();
    let path = h(&e, "path");
    e.set_path_data(path, Path::from_svg("M0 0L10 0L10 10L20 10").unwrap())
        .unwrap();
    e.set_property(path, PropertyKey::Transform, json!(Affine::scale(1., 0.)))
        .unwrap();
    let mut a = MotionPathAnimation::new(
        &e,
        h(&e, "target"),
        path,
        MotionPathSpec::new(1000, 0., 30.),
    )
    .unwrap();
    let before = e.scene().clone();
    let revision = e.revision();
    assert!(a.tick(&mut e, ms(500)).is_err());
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), revision);
    assert_eq!(a.info(&e).unwrap().elapsed, Duration::ZERO);
    a.seek(&mut e, ms(900)).unwrap();
    point(position(&e), Point::new(17., 0.));
}

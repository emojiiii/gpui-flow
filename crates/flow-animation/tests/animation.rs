use gpui_flow_animation::*;
use gpui_flow_engine::{
    ChangeOrigin, CustomDefinition, DirtyFlags, ElementRegistry, InstanceState, PropertyDescriptor,
    PropertyKey, PropertySchema, SceneEngine,
};
use gpui_flow_scene::{Affine, Color, DisplayObject, Paint, Point, Scene, Shape, Size, display};
use serde_json::json;
use std::{cell::RefCell, rc::Rc, time::Duration};
fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}
fn engine() -> SceneEngine {
    let mut scene = Scene::new();
    for id in ["a", "b"] {
        scene
            .insert(
                DisplayObject::new(id, display::Rect::new(100., 80.)),
                None,
                None,
            )
            .unwrap();
    }
    SceneEngine::new(scene).unwrap()
}
fn position(e: &SceneEngine, id: &str) -> Point {
    let o = &e.scene().get(&id.into()).unwrap().object;
    Point::new(o.transform.tx, o.transform.ty)
}
fn status(c: &mut AnimationController, e: &SceneEngine, id: AnimationId) -> AnimationStatus {
    c.info(e, id).unwrap().status
}
fn moving(c: &mut AnimationController, e: &SceneEngine, id: &str) -> AnimationId {
    c.animate(
        e,
        e.handle(&id.into()).unwrap(),
        AnimationSpec::new(1000, vec![Tween::translation(Point::new(100., 50.))]),
    )
    .unwrap()
}
#[test]
fn atomic_multi_target_tick_has_one_revision_origin_and_notification() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    moving(&mut c, &e, "a");
    moving(&mut c, &e, "b");
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    e.on_change(None, move |change| {
        log.borrow_mut().push((change.revision, change.origin))
    })
    .unwrap();
    let before = e.revision();
    let report = c.tick(&mut e, ms(500)).unwrap();
    assert_eq!(e.revision(), before + 1);
    assert_eq!(position(&e, "a"), Point::new(50., 25.));
    assert_eq!(position(&e, "b"), Point::new(50., 25.));
    assert_eq!(
        *seen.borrow(),
        vec![(e.revision(), ChangeOrigin::Animation)]
    );
    assert_eq!(report.advanced.len(), 2);
    assert!(report.changes.changed());
}
#[test]
fn pause_resume_and_zero_delta_never_advance_or_emit_changes() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let id = moving(&mut c, &e, "a");
    assert!(!c.tick(&mut e, Duration::ZERO).unwrap().changes.changed());
    c.tick(&mut e, ms(250)).unwrap();
    c.pause(&e, id).unwrap();
    let before = e.scene().clone();
    assert!(!c.tick(&mut e, ms(500)).unwrap().changes.changed());
    assert_eq!(e.scene(), &before);
    assert_eq!(c.info(&e, id).unwrap().elapsed, ms(250));
    c.resume(&e, id).unwrap();
    c.tick(&mut e, ms(250)).unwrap();
    assert_eq!(position(&e, "a"), Point::new(50., 25.));
}
#[test]
fn seek_is_real_and_completed_rewind_pauses_until_resumed() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let id = moving(&mut c, &e, "a");
    c.seek(&mut e, id, ms(600)).unwrap();
    assert_eq!(position(&e, "a"), Point::new(60., 30.));
    assert_eq!(status(&mut c, &e, id), AnimationStatus::Playing);
    c.tick(&mut e, Duration::MAX).unwrap();
    assert_eq!(status(&mut c, &e, id), AnimationStatus::Completed);
    assert_eq!(position(&e, "a"), Point::new(100., 50.));
    c.seek(&mut e, id, ms(250)).unwrap();
    assert_eq!(status(&mut c, &e, id), AnimationStatus::Paused);
    assert_eq!(position(&e, "a"), Point::new(25., 12.5));
    c.resume(&e, id).unwrap();
    c.tick(&mut e, ms(750)).unwrap();
    assert_eq!(status(&mut c, &e, id), AnimationStatus::Completed);
}
#[test]
fn cancel_keeps_current_values_and_remove_releases_bounded_record() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let id = moving(&mut c, &e, "a");
    c.tick(&mut e, ms(200)).unwrap();
    c.cancel(&e, id).unwrap();
    assert_eq!(
        status(&mut c, &e, id),
        AnimationStatus::Cancelled(CancelReason::User)
    );
    let before = e.scene().clone();
    c.tick(&mut e, ms(9999)).unwrap();
    assert_eq!(e.scene(), &before);
    assert!(c.resume(&e, id).is_err());
    assert!(c.seek(&mut e, id, Duration::ZERO).is_err());
    assert!(c.remove(id));
    assert!(!c.remove(id));
    assert!(c.is_empty());
    assert!(c.info(&e, id).is_err());
}
#[test]
fn external_write_cancels_only_conflicting_animation_and_never_overwrites_user() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let a = moving(&mut c, &e, "a");
    let b = moving(&mut c, &e, "b");
    c.tick(&mut e, ms(200)).unwrap();
    e.transaction(|s| s.update_object(&"a".into(), |o| o.transform.tx = 999.))
        .unwrap();
    let report = c.tick(&mut e, ms(200)).unwrap();
    assert_eq!(report.cancelled, vec![a]);
    assert_eq!(position(&e, "a").x, 999.);
    assert_eq!(position(&e, "b").x, 40.);
    assert_eq!(status(&mut c, &e, b), AnimationStatus::Playing);
}
#[test]
fn wrong_engine_or_foreign_animation_id_never_changes_either_engine_or_clock() {
    let mut e = engine();
    let mut other = engine();
    let mut c = AnimationController::new(&e);
    let id = moving(&mut c, &e, "a");
    assert_eq!(
        c.tick(&mut other, ms(500)).unwrap_err(),
        AnimationError::WrongEngine
    );
    assert_eq!(c.info(&e, id).unwrap().elapsed, Duration::ZERO);
    assert_eq!(position(&other, "a"), Point::ZERO);
    let mut second = AnimationController::new(&e);
    let foreign = moving(&mut second, &e, "a");
    assert_ne!(id, foreign);
    assert!(c.seek(&mut e, foreign, ms(500)).is_err());
}
#[test]
fn reset_and_same_id_recreation_cannot_resurrect_an_old_target() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let id = moving(&mut c, &e, "a");
    let old = e.handle(&"a".into()).unwrap();
    e.reset_scene(e.scene().clone()).unwrap();
    assert_ne!(e.handle(&"a".into()).unwrap(), old);
    let report = c.tick(&mut e, ms(500)).unwrap();
    assert_eq!(report.cancelled, vec![id]);
    assert_eq!(position(&e, "a"), Point::ZERO);
    assert!(!report.changes.changed());
}
#[test]
fn detach_cancels_on_sync_and_reattach_does_not_implicitly_resume() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let id = moving(&mut c, &e, "a");
    let handle = e.handle(&"a".into()).unwrap();
    let (tree, _) = e.detach(handle).unwrap();
    assert_eq!(
        status(&mut c, &e, id),
        AnimationStatus::Cancelled(CancelReason::TargetUnavailable)
    );
    e.attach(&tree, None, None).unwrap();
    c.tick(&mut e, ms(1000)).unwrap();
    assert_eq!(position(&e, "a"), Point::ZERO);
    assert!(c.resume(&e, id).is_err());
}
fn custom_engine(reject_middle: bool) -> SceneEngine {
    let mut registry = ElementRegistry::default();
    registry
        .register(CustomDefinition::new(
            "disc",
            PropertySchema {
                properties: vec![PropertyDescriptor::number(
                    "radius",
                    10.,
                    1.,
                    40.,
                    DirtyFlags::GEOMETRY | DirtyFlags::HIT | DirtyFlags::PAINT,
                )],
                allow_unknown: false,
            },
            move |data: &serde_json::Value| {
                let radius = data["radius"].as_f64().unwrap() as f32;
                if reject_middle && (14.9..15.1).contains(&radius) {
                    return Err("test rejects intermediate geometry".into());
                }
                let shape = display::Ellipse::circle(radius);
                Ok((shape.bounds(), vec![shape.flatten(0.1)]))
            },
        ))
        .unwrap();
    let mut e = SceneEngine::with_registry(Scene::new(), registry).unwrap();
    e.create("disc", "disc", json!({}), None, None).unwrap();
    e.create("rect", "b", json!({}), None, None).unwrap();
    e
}
#[test]
fn custom_numeric_defaults_and_geometry_hit_bounds_follow_the_same_tick() {
    let mut e = custom_engine(false);
    let mut c = AnimationController::new(&e);
    let handle = e.handle(&"disc".into()).unwrap();
    c.animate(
        &e,
        handle,
        AnimationSpec::new(1000, vec![Tween::custom_number("radius", 20.)]),
    )
    .unwrap();
    let before = e.scene().clone();
    assert!(!c.tick(&mut e, Duration::ZERO).unwrap().changes.changed());
    assert_eq!(e.scene(), &before);
    c.tick(&mut e, ms(500)).unwrap();
    let object = e.object(handle).unwrap();
    let Shape::Custom(custom) = &object.shape else {
        panic!("custom")
    };
    assert_eq!(custom.data["radius"], 15.);
    assert_eq!(custom.bounds.size, Size::new(30., 30.));
    assert!(e.last_change().dirty[&"disc".into()].contains(DirtyFlags::GEOMETRY));
}
#[test]
fn invalid_intermediate_geometry_rolls_back_every_target_and_all_clocks() {
    let mut e = custom_engine(true);
    let mut c = AnimationController::new(&e);
    let a = c
        .animate(
            &e,
            e.handle(&"disc".into()).unwrap(),
            AnimationSpec::new(1000, vec![Tween::custom_number("radius", 20.)]),
        )
        .unwrap();
    let b = moving(&mut c, &e, "b");
    let scene = e.scene().clone();
    let revision = e.revision();
    assert!(c.tick(&mut e, ms(500)).is_err());
    assert_eq!(e.scene(), &scene);
    assert_eq!(e.revision(), revision);
    assert_eq!(c.info(&e, a).unwrap().elapsed, Duration::ZERO);
    assert_eq!(c.info(&e, b).unwrap().elapsed, Duration::ZERO);
    c.tick(&mut e, ms(1000)).unwrap();
    assert_eq!(status(&mut c, &e, a), AnimationStatus::Completed);
    assert_eq!(status(&mut c, &e, b), AnimationStatus::Completed);
}
#[test]
fn invalid_schedules_are_rejected_before_any_input_or_runtime_record_is_changed() {
    let e = engine();
    let mut c = AnimationController::new(&e);
    let h = e.handle(&"a".into()).unwrap();
    for spec in [
        AnimationSpec::new(0, vec![Tween::opacity(0.)]),
        AnimationSpec::new(100, vec![]),
        AnimationSpec::new(100, vec![Tween::opacity(2.)]),
        AnimationSpec::new(100, vec![Tween::translation(Point::new(f32::NAN, 1.))]),
        AnimationSpec::new(100, vec![Tween::custom_number("missing", 10.)]),
        AnimationSpec::new(
            100,
            vec![Tween {
                property: TweenProperty::Opacity,
                to: TweenValue::Color(Color::WHITE),
            }],
        ),
    ] {
        assert!(c.animate(&e, h, spec).is_err());
        assert!(c.is_empty());
        assert_eq!(e.revision(), 0);
    }
}
#[test]
fn conflicting_or_duplicate_properties_reject_but_disjoint_properties_can_run() {
    let e = engine();
    let mut c = AnimationController::new(&e);
    let h = e.handle(&"a".into()).unwrap();
    let first = moving(&mut c, &e, "a");
    c.pause(&e, first).unwrap();
    assert_eq!(
        c.animate(
            &e,
            h,
            AnimationSpec::new(100, vec![Tween::translation(Point::ZERO)])
        )
        .unwrap_err(),
        AnimationError::Conflict
    );
    c.animate(&e, h, AnimationSpec::new(100, vec![Tween::opacity(0.5)]))
        .unwrap();
    assert!(
        c.animate(
            &e,
            h,
            AnimationSpec::new(
                100,
                vec![
                    Tween::fill_color(Color::WHITE),
                    Tween::fill_color(Color::BLACK)
                ]
            )
        )
        .is_err()
    );
}
#[test]
fn completed_seek_must_reacquire_property_ownership_and_reapply_endpoint_after_manual_change() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let id = moving(&mut c, &e, "a");
    c.tick(&mut e, ms(1000)).unwrap();
    e.transaction(|s| s.update_object(&"a".into(), |o| o.transform.tx = 7.))
        .unwrap();
    c.seek(&mut e, id, ms(1000)).unwrap();
    assert_eq!(position(&e, "a").x, 100.);
    let other = c
        .animate(
            &e,
            e.handle(&"a".into()).unwrap(),
            AnimationSpec::new(500, vec![Tween::translation(Point::ZERO)]),
        )
        .unwrap();
    assert_eq!(
        c.seek(&mut e, id, ms(0)).unwrap_err(),
        AnimationError::Conflict
    );
    c.cancel(&e, other).unwrap();
    c.seek(&mut e, id, Duration::ZERO).unwrap();
    assert_eq!(position(&e, "a"), Point::ZERO);
}
#[test]
fn subframe_opacity_rounding_does_not_look_like_an_external_override() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let id = c
        .animate(
            &e,
            e.handle(&"a".into()).unwrap(),
            AnimationSpec::new(
                1000,
                vec![Tween {
                    property: TweenProperty::Opacity,
                    to: TweenValue::Number(0.3),
                }],
            ),
        )
        .unwrap();
    for _ in 0..100 {
        c.tick(&mut e, ms(10)).unwrap();
        assert!(!matches!(
            status(&mut c, &e, id),
            AnimationStatus::Cancelled(_)
        ));
    }
    assert_eq!(
        e.object(e.handle(&"a".into()).unwrap())
            .unwrap()
            .style
            .opacity,
        0.3f32
    );
    assert_eq!(status(&mut c, &e, id), AnimationStatus::Completed);
}
#[test]
fn translation_preserves_other_affine_components_and_unrelated_host_edits() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let id = moving(&mut c, &e, "a");
    e.transaction(|s| {
        s.update_object(&"a".into(), |o| {
            o.transform = Affine::rotation(30.) * Affine::scale(2., 3.);
            o.data = json!({"host":true});
        })
    })
    .unwrap();
    let linear = e.scene().get(&"a".into()).unwrap().object.transform;
    c.tick(&mut e, ms(500)).unwrap();
    let object = e.object(e.handle(&"a".into()).unwrap()).unwrap();
    assert_eq!(
        (
            object.transform.a,
            object.transform.b,
            object.transform.c,
            object.transform.d
        ),
        (linear.a, linear.b, linear.c, linear.d)
    );
    assert_eq!(object.data["host"], true);
    assert_eq!(status(&mut c, &e, id), AnimationStatus::Playing);
}
#[test]
fn solid_color_and_opacity_reach_exact_endpoints_with_defined_channel_rounding() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let h = e.handle(&"a".into()).unwrap();
    let id = c
        .animate(
            &e,
            h,
            AnimationSpec::new(
                1000,
                vec![
                    Tween::fill_color(Color::rgba(255, 100, 10, 0)),
                    Tween::opacity(0.5),
                ],
            ),
        )
        .unwrap();
    c.tick(&mut e, ms(500)).unwrap();
    assert_eq!(
        e.object(h).unwrap().style.fill,
        Some(Paint::solid(Color::rgba(128, 50, 5, 128)))
    );
    assert_eq!(e.object(h).unwrap().style.opacity, 0.75);
    c.tick(&mut e, ms(500)).unwrap();
    assert_eq!(
        e.object(h).unwrap().style.fill,
        Some(Paint::solid(Color::rgba(255, 100, 10, 0)))
    );
    assert_eq!(status(&mut c, &e, id), AnimationStatus::Completed);
}
#[test]
fn all_easings_have_known_midpoints_and_deterministic_chunking() {
    for (easing, quarter) in [
        (Easing::Linear, 25.),
        (Easing::EaseIn, 1.5625),
        (Easing::EaseOut, 57.8125),
        (Easing::EaseInOut, 15.625),
    ] {
        let mut e = engine();
        let mut c = AnimationController::new(&e);
        let mut spec = AnimationSpec::new(1000, vec![Tween::translation(Point::new(100., 0.))]);
        spec.easing = easing;
        c.animate(&e, e.handle(&"a".into()).unwrap(), spec).unwrap();
        for _ in 0..5 {
            c.tick(&mut e, ms(50)).unwrap();
        }
        assert!((position(&e, "a").x - quarter).abs() < 0.0001);
    }
}
#[test]
fn spec_serde_roundtrip_does_not_serialize_runtime_handles_or_caches() {
    let spec = AnimationSpec::new(
        1200,
        vec![Tween::translation(Point::new(5., 8.)), Tween::opacity(0.5)],
    );
    let value = serde_json::to_string(&spec).unwrap();
    assert_eq!(serde_json::from_str::<AnimationSpec>(&value).unwrap(), spec);
    assert!(!value.contains("generation"));
    assert!(!value.contains("controller"));
}
#[test]
fn engine_batch_invalid_late_property_or_foreign_handle_is_atomic_and_preview_is_read_only() {
    let mut e = engine();
    let other = engine();
    let h = e.handle(&"a".into()).unwrap();
    let original = e.scene().clone();
    e.validate_properties_batch([(h, PropertyKey::Opacity, json!(0.5))])
        .unwrap();
    assert_eq!(e.scene(), &original);
    assert_eq!(e.revision(), 0);
    assert!(
        e.set_properties_batch(
            ChangeOrigin::Animation,
            [
                (h, PropertyKey::Opacity, json!(0.5)),
                (
                    e.handle(&"b".into()).unwrap(),
                    PropertyKey::Opacity,
                    json!(2.)
                )
            ]
        )
        .is_err()
    );
    assert_eq!(e.scene(), &original);
    assert!(
        e.set_properties_batch(
            ChangeOrigin::Animation,
            [
                (h, PropertyKey::Opacity, json!(0.5)),
                (
                    other.handle(&"a".into()).unwrap(),
                    PropertyKey::Opacity,
                    json!(0.5)
                )
            ]
        )
        .is_err()
    );
    assert_eq!(e.scene(), &original);
}
#[test]
fn destroy_after_completion_and_controller_drop_do_not_restore_or_spawn_work() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let h = e.handle(&"a".into()).unwrap();
    let id = moving(&mut c, &e, "a");
    c.tick(&mut e, ms(1000)).unwrap();
    e.destroy(h).unwrap();
    assert_eq!(
        status(&mut c, &e, id),
        AnimationStatus::Cancelled(CancelReason::TargetUnavailable)
    );
    assert!(c.seek(&mut e, id, ms(0)).is_err());
    drop(c);
    assert!(e.scene().get(&"a".into()).is_none());
    assert_eq!(e.instance(h).unwrap().state, InstanceState::Destroyed);
}

#[test]
fn bounded_records_require_explicit_remove_and_do_not_reuse_old_animation_ids() {
    let mut scene = Scene::new();
    for index in 0..=MAX_ANIMATIONS {
        scene
            .insert(
                DisplayObject::new(format!("n{index}"), display::Rect::new(10., 10.)),
                None,
                None,
            )
            .unwrap();
    }
    let e = SceneEngine::new(scene).unwrap();
    let mut c = AnimationController::new(&e);
    let mut first = None;
    for index in 0..MAX_ANIMATIONS {
        let id = moving(&mut c, &e, &format!("n{index}"));
        first.get_or_insert(id);
    }
    assert_eq!(c.len(), MAX_ANIMATIONS);
    let target = e.handle(&format!("n{MAX_ANIMATIONS}").into()).unwrap();
    let spec = AnimationSpec::new(1000, vec![Tween::opacity(0.5)]);
    assert_eq!(
        c.animate(&e, target, spec.clone()).unwrap_err(),
        AnimationError::Capacity
    );
    let old = first.unwrap();
    assert!(c.remove(old));
    let next = c.animate(&e, target, spec).unwrap();
    assert_ne!(old, next);
    assert!(c.info(&e, old).is_err());
}
#[test]
fn separate_controllers_treat_each_others_writes_as_external_overrides() {
    let mut e = engine();
    let mut first = AnimationController::new(&e);
    let mut second = AnimationController::new(&e);
    moving(&mut first, &e, "a");
    let id = moving(&mut second, &e, "a");
    first.tick(&mut e, ms(250)).unwrap();
    assert_eq!(second.tick(&mut e, ms(500)).unwrap().cancelled, vec![id]);
    assert_eq!(position(&e, "a"), Point::new(25., 12.5));
}
#[test]
fn dropping_a_playing_controller_keeps_values_and_has_no_deferred_work() {
    let mut e = engine();
    let mut controller = AnimationController::new(&e);
    moving(&mut controller, &e, "a");
    controller.tick(&mut e, ms(300)).unwrap();
    let scene = e.scene().clone();
    let revision = e.revision();
    drop(controller);
    assert_eq!(e.scene(), &scene);
    assert_eq!(e.revision(), revision);
    let mut fresh = AnimationController::new(&e);
    moving(&mut fresh, &e, "a");
    fresh.tick(&mut e, ms(1000)).unwrap();
    assert_eq!(position(&e, "a"), Point::new(100., 50.));
}

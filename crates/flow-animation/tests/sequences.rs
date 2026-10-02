use gpui_flow_animation::*;
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{
    ChangeOrigin, CustomDefinition, DirtyFlags, ElementRegistry, PropertyDescriptor, PropertyKey,
    PropertySchema, SceneEngine,
};
use gpui_flow_scene::{Color, DisplayObject, Paint, Point, Scene, Style, display};
use serde_json::json;
use std::{cell::RefCell, rc::Rc, time::Duration};
fn ms(t: u64) -> Duration {
    Duration::from_millis(t)
}
fn engine() -> SceneEngine {
    let mut scene = Scene::new();
    for id in ["a", "b"] {
        scene
            .insert(
                DisplayObject::new(id, display::Rect::new(40., 30.))
                    .with_style(Style::fill(Color::BLACK)),
                None,
                None,
            )
            .unwrap();
    }
    SceneEngine::new(scene).unwrap()
}
fn sequence() -> AnimationSequence {
    AnimationSequence::new(vec![
        AnimationSpec::new(100, vec![Tween::translation(Point::new(100., 0.))]),
        AnimationSpec::new(
            200,
            vec![Tween::opacity(0.5), Tween::fill_color(Color::WHITE)],
        ),
        AnimationSpec::new(
            100,
            vec![Tween::translation(Point::new(200., 0.)), Tween::opacity(1.)],
        ),
    ])
}
fn pose(e: &SceneEngine) -> (f32, f32, Paint) {
    let o = &e.scene().get(&"a".into()).unwrap().object;
    (
        o.transform.tx,
        o.style.opacity,
        o.style.fill.clone().unwrap(),
    )
}
fn start(e: &SceneEngine, c: &mut AnimationController) -> AnimationId {
    c.animate_sequence(e, e.handle(&"a".into()).unwrap(), sequence())
        .unwrap()
}
#[test]
fn sparse_frames_hold_values_and_boundaries_seek_exactly_in_both_directions() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let id = start(&e, &mut c);
    assert_eq!(c.info(&e, id).unwrap().duration, ms(400));
    for (t, x, opacity, color) in [
        (0, 0., 1., Color::BLACK),
        (50, 50., 1., Color::BLACK),
        (100, 100., 1., Color::BLACK),
        (200, 100., 0.75, Color::rgb(128, 128, 128)),
        (300, 100., 0.5, Color::WHITE),
        (350, 150., 0.75, Color::WHITE),
        (400, 200., 1., Color::WHITE),
        (100, 100., 1., Color::BLACK),
        (0, 0., 1., Color::BLACK),
    ] {
        c.seek(&mut e, id, ms(t)).unwrap();
        assert_eq!(pose(&e), (x, opacity, color.into()));
    }
    assert_eq!(c.info(&e, id).unwrap().status, AnimationStatus::Paused);
}
#[test]
fn segment_easing_resets_at_each_boundary_instead_of_easing_total_time() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let mut first = AnimationSpec::new(100, vec![Tween::translation(Point::new(100., 0.))]);
    first.easing = Easing::EaseIn;
    let mut second = AnimationSpec::new(200, vec![Tween::translation(Point::new(200., 0.))]);
    second.easing = Easing::EaseOut;
    let id = c
        .animate_sequence(
            &e,
            e.handle(&"a".into()).unwrap(),
            AnimationSequence::new(vec![first, second]),
        )
        .unwrap();
    c.seek(&mut e, id, ms(50)).unwrap();
    assert_eq!(pose(&e).0, 12.5);
    c.seek(&mut e, id, ms(100)).unwrap();
    assert_eq!(pose(&e).0, 100.);
    c.seek(&mut e, id, ms(200)).unwrap();
    assert_eq!(pose(&e).0, 187.5);
}
#[test]
fn oversized_tick_matches_partitioned_ticks_and_one_atomic_notification() {
    let mut a = engine();
    let mut ca = AnimationController::new(&a);
    let id = start(&a, &mut ca);
    ca.animate_sequence(&a, a.handle(&"b".into()).unwrap(), sequence())
        .unwrap();
    let seen = Rc::new(RefCell::new(vec![]));
    let observed = seen.clone();
    a.on_change(None, move |change| {
        observed.borrow_mut().push(change.origin)
    })
    .unwrap();
    let revision = a.revision();
    let result = ca.tick(&mut a, Duration::MAX).unwrap();
    assert_eq!(result.completed.len(), 2);
    assert_eq!(a.revision(), revision + 1);
    assert_eq!(*seen.borrow(), vec![ChangeOrigin::Animation]);
    assert_eq!(ca.info(&a, id).unwrap().elapsed, ms(400));
    let mut b = engine();
    let mut cb = AnimationController::new(&b);
    start(&b, &mut cb);
    cb.animate_sequence(&b, b.handle(&"b".into()).unwrap(), sequence())
        .unwrap();
    for _ in 0..40 {
        cb.tick(&mut b, ms(10)).unwrap();
    }
    assert_eq!(a.scene(), b.scene());
    assert!(ca.tick(&mut a, ms(1)).unwrap().completed.is_empty());
}
#[test]
fn pause_seek_resume_and_completed_rewind_use_existing_lifecycle() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let id = start(&e, &mut c);
    c.tick(&mut e, ms(150)).unwrap();
    c.pause(&e, id).unwrap();
    let before = e.scene().clone();
    assert!(!c.tick(&mut e, ms(1000)).unwrap().changes.changed());
    assert_eq!(e.scene(), &before);
    c.seek(&mut e, id, ms(350)).unwrap();
    assert_eq!(c.info(&e, id).unwrap().status, AnimationStatus::Paused);
    c.resume(&e, id).unwrap();
    c.tick(&mut e, ms(50)).unwrap();
    assert_eq!(c.info(&e, id).unwrap().status, AnimationStatus::Completed);
    c.seek(&mut e, id, ms(200)).unwrap();
    assert_eq!(c.info(&e, id).unwrap().status, AnimationStatus::Paused);
    c.resume(&e, id).unwrap();
    c.tick(&mut e, ms(200)).unwrap();
    assert_eq!(pose(&e).0, 200.);
}
#[test]
fn later_invalid_endpoint_rejects_whole_registration_without_reserving_properties() {
    let e = engine();
    let scene = e.scene().clone();
    let revision = e.revision();
    let mut c = AnimationController::new(&e);
    let h = e.handle(&"a".into()).unwrap();
    let mut invalid = sequence();
    invalid.keyframes[2].tweens.push(Tween::opacity(2.));
    assert!(c.animate_sequence(&e, h, invalid).is_err());
    assert!(c.is_empty());
    let mut invalid = sequence();
    invalid.keyframes[2].tweens = vec![Tween::opacity(2.)];
    assert!(c.animate_sequence(&e, h, invalid).is_err());
    assert!(c.is_empty());
    assert_eq!(e.scene(), &scene);
    assert_eq!(e.revision(), revision);
    start(&e, &mut c);
    assert_eq!(c.len(), 1);
}
#[test]
fn bounded_keyframes_durations_and_union_properties_reject_invalid_schedules() {
    let e = engine();
    let mut c = AnimationController::new(&e);
    let h = e.handle(&"a".into()).unwrap();
    let frame = AnimationSpec::new(100, vec![Tween::opacity(0.5)]);
    for invalid in [
        AnimationSequence::new(vec![]),
        AnimationSequence::new(vec![frame.clone(); 65]),
        AnimationSequence::new(vec![AnimationSpec::new(0, vec![Tween::opacity(0.5)])]),
        AnimationSequence::new(vec![
            AnimationSpec::new(u64::MAX, vec![Tween::opacity(0.5)]),
            frame.clone(),
        ]),
        AnimationSequence::new(vec![AnimationSpec::new(1, vec![])]),
        AnimationSequence::new(vec![AnimationSpec::new(
            1,
            (0..33)
                .map(|i| Tween::custom_number(format!("p{i}"), 1.))
                .collect(),
        )]),
    ] {
        assert!(c.animate_sequence(&e, h, invalid).is_err());
        assert!(c.is_empty());
    }
    // Each frame is valid-sized, but the distinct property union is bounded too.
    let union = AnimationSequence::new(
        (0..33)
            .map(|i| AnimationSpec::new(1, vec![Tween::custom_number(format!("p{i}"), 1.)]))
            .collect(),
    );
    assert!(
        matches!(c.animate_sequence(&e,h,union),Err(AnimationError::InvalidInput(s)) if s.contains("distinct"))
    );
    c.animate_sequence(&e, h, AnimationSequence::new(vec![frame; 64]))
        .unwrap();
}
#[test]
fn maximum_duration_samples_without_overflow_or_nan() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let id = c
        .animate_sequence(
            &e,
            e.handle(&"a".into()).unwrap(),
            AnimationSequence::new(vec![
                AnimationSpec::new(u64::MAX - 1, vec![Tween::opacity(0.)]),
                AnimationSpec::new(1, vec![Tween::opacity(1.)]),
            ]),
        )
        .unwrap();
    c.seek(&mut e, id, ms(u64::MAX / 2)).unwrap();
    assert!((pose(&e).1 - 0.5).abs() < 1e-6);
    c.tick(&mut e, Duration::MAX).unwrap();
    assert_eq!(pose(&e).1, 1.);
}
#[test]
fn property_union_is_reserved_even_before_its_first_active_segment() {
    let e = engine();
    let mut c = AnimationController::new(&e);
    let id = start(&e, &mut c);
    assert_eq!(
        c.animate(
            &e,
            e.handle(&"a".into()).unwrap(),
            AnimationSpec::new(100, vec![Tween::opacity(0.)])
        )
        .unwrap_err(),
        AnimationError::Conflict
    );
    c.cancel(&e, id).unwrap();
    c.animate(
        &e,
        e.handle(&"a".into()).unwrap(),
        AnimationSpec::new(100, vec![Tween::opacity(0.)]),
    )
    .unwrap();
}
#[test]
fn external_write_to_future_property_cancels_without_overwriting_other_targets() {
    let mut e = engine();
    let mut c = AnimationController::new(&e);
    let a = start(&e, &mut c);
    let b = c
        .animate_sequence(&e, e.handle(&"b".into()).unwrap(), sequence())
        .unwrap();
    c.tick(&mut e, ms(20)).unwrap();
    e.set_property(
        e.handle(&"a".into()).unwrap(),
        PropertyKey::Opacity,
        json!(0.2),
    )
    .unwrap();
    let result = c.tick(&mut e, ms(200)).unwrap();
    assert_eq!(result.cancelled, vec![a]);
    assert_eq!(pose(&e).0, 20.);
    assert_eq!(pose(&e).1, 0.2);
    assert_eq!(c.info(&e, b).unwrap().elapsed, ms(220));
}
#[test]
fn detach_reattach_between_ticks_cannot_revive_single_or_sequence_motion() {
    for single in [false, true] {
        let mut e = engine();
        let mut c = AnimationController::new(&e);
        let h = e.handle(&"a".into()).unwrap();
        let id = if single {
            c.animate(&e, h, AnimationSpec::new(100, vec![Tween::opacity(0.)]))
                .unwrap()
        } else {
            start(&e, &mut c)
        };
        c.tick(&mut e, ms(20)).unwrap();
        let before = e.object(h).unwrap().clone();
        let (tree, _) = e.detach(h).unwrap();
        e.attach(&tree, None, None).unwrap();
        let result = c.tick(&mut e, ms(200)).unwrap();
        assert_eq!(result.cancelled, vec![id]);
        assert_eq!(e.object(h).unwrap(), &before);
        assert_eq!(
            c.info(&e, id).unwrap().status,
            AnimationStatus::Cancelled(CancelReason::TargetUnavailable)
        );
    }
}
#[test]
fn editor_finish_and_cancel_preserve_old_undo_redo_across_many_segments() {
    for cancel in [false, true] {
        let mut e = SceneEditor::from_engine(engine());
        e.transaction(|s| s.update_object(&"a".into(), |o| o.transform.tx = 10.))
            .unwrap();
        let token = e.begin_playback().unwrap();
        let mut c = AnimationController::new(&e.engine);
        start(&e.engine, &mut c);
        c.tick_editor(&mut e, token, ms(350)).unwrap();
        if cancel {
            c.cancel_editor(&mut e, token).unwrap();
        } else {
            c.finish_editor(&mut e, token).unwrap();
            assert!(e.undo());
        }
        assert_eq!(pose(&e.engine).0, 10.);
        assert!(e.undo());
        assert_eq!(pose(&e.engine).0, 0.);
        assert!(!e.undo());
        assert!(e.redo());
        assert_eq!(pose(&e.engine).0, 10.);
        if !cancel {
            assert!(e.redo());
            assert_eq!(pose(&e.engine).0, 150.);
        }
    }
}
#[test]
fn later_schema_failure_rolls_back_all_targets_and_clocks_and_preserves_playback() {
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
                    DirtyFlags::GEOMETRY | DirtyFlags::PAINT,
                )],
                allow_unknown: false,
            },
            |data: &serde_json::Value| {
                let radius = data["radius"].as_f64().unwrap() as f32;
                if (14.9..15.1).contains(&radius) {
                    return Err("intermediate radius rejected".into());
                }
                let shape = display::Ellipse::circle(radius);
                Ok((shape.bounds(), shape.flatten(0.1)))
            },
        ))
        .unwrap();
    let mut engine = SceneEngine::with_registry(engine().scene().clone(), registry).unwrap();
    engine
        .create("disc", "disc", json!({}), None, None)
        .unwrap();
    let mut e = SceneEditor::from_engine(engine);
    let token = e.begin_playback().unwrap();
    let mut c = AnimationController::new(&e.engine);
    let a = start(&e.engine, &mut c);
    let disc = c
        .animate_sequence(
            &e.engine,
            e.engine.handle(&"disc".into()).unwrap(),
            AnimationSequence::new(vec![
                AnimationSpec::new(100, vec![Tween::custom_number("radius", 20.)]),
                AnimationSpec::new(100, vec![Tween::custom_number("radius", 10.)]),
            ]),
        )
        .unwrap();
    c.tick_editor(&mut e, token, ms(100)).unwrap();
    let scene = e.scene().clone();
    let revision = e.engine.revision();
    assert!(c.tick_editor(&mut e, token, ms(50)).is_err());
    assert_eq!(e.scene(), &scene);
    assert_eq!(e.engine.revision(), revision);
    assert!(e.is_playback_active(token));
    assert_eq!(c.info(&e.engine, a).unwrap().elapsed, ms(100));
    assert_eq!(c.info(&e.engine, disc).unwrap().elapsed, ms(100));
    c.tick_editor(&mut e, token, ms(100)).unwrap();
    c.cancel_editor(&mut e, token).unwrap();
    assert!(!e.undo());
}
#[test]
fn sequence_json_roundtrip_preserves_segments_and_rejects_unknown_fields() {
    let value = serde_json::to_value(sequence()).unwrap();
    assert_eq!(
        serde_json::from_value::<AnimationSequence>(value).unwrap(),
        sequence()
    );
    assert!(
        serde_json::from_value::<AnimationSequence>(json!({"keyframes":[],"repeat":3})).is_err()
    );
}

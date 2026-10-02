use gpui_flow_animation::*;
use gpui_flow_engine::{
    ChangeOrigin, CustomDefinition, DirtyFlags, ElementRegistry, PropertyDescriptor, PropertyKey,
    PropertySchema, SceneEngine,
};
use gpui_flow_scene::{Color, DisplayObject, Point, Scene, Style, display};
use serde_json::json;
use std::{cell::RefCell, rc::Rc, time::Duration};
fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}
fn engine() -> SceneEngine {
    let mut scene = Scene::new();
    for id in ["a", "b", "c"] {
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
fn x(e: &SceneEngine, name: &str) -> f32 {
    e.object(e.handle(&name.into()).unwrap())
        .unwrap()
        .transform
        .tx
}
fn move_to(e: &SceneEngine, name: &str, duration: u64, x: f32) -> TimelineChild {
    TimelineChild::animate(
        e.handle(&name.into()).unwrap(),
        AnimationSpec::new(duration, vec![Tween::translation(Point::new(x, 0.))]),
    )
}
fn opacity(e: &SceneEngine, name: &str, duration: u64, value: f32) -> TimelineChild {
    TimelineChild::animate(
        e.handle(&name.into()).unwrap(),
        AnimationSpec::new(duration, vec![Tween::opacity(value)]),
    )
}
fn start(e: &SceneEngine, c: &mut TimelineController) -> TimelineId {
    c.animate(
        e,
        TimelineSpec::serial(vec![
            move_to(e, "a", 100, 100.).with_offset(10),
            TimelineChild::group(TimelineSpec::parallel(vec![
                move_to(e, "a", 200, 200.),
                move_to(e, "b", 100, 50.).with_offset(50),
            ]))
            .with_offset(20),
            move_to(e, "a", 100, 300.).with_offset(30),
        ]),
    )
    .unwrap()
}
#[test]
fn serial_parallel_nested_boundaries_gaps_and_backwards_seeks_are_exact() {
    let mut e = engine();
    let mut c = TimelineController::new(&e);
    let id = start(&e, &mut c);
    assert_eq!(c.info(&e, id).unwrap().duration, ms(460));
    assert_eq!(e.revision(), 0);
    for (time, a, b) in [
        (0, 0., 0.),
        (10, 0., 0.),
        (60, 50., 0.),
        (110, 100., 0.),
        (120, 100., 0.),
        (130, 100., 0.),
        (180, 125., 0.),
        (230, 150., 25.),
        (280, 175., 50.),
        (330, 200., 50.),
        (350, 200., 50.),
        (360, 200., 50.),
        (410, 250., 50.),
        (460, 300., 50.),
        (230, 150., 25.),
        (120, 100., 0.),
        (0, 0., 0.),
    ] {
        c.seek(&mut e, id, ms(time)).unwrap();
        assert_eq!((x(&e, "a"), x(&e, "b")), (a, b), "at {time}");
    }
    assert_eq!(c.info(&e, id).unwrap().status, AnimationStatus::Paused);
}
#[test]
fn a_parent_frame_emits_one_revision_and_one_change_notification() {
    let mut e = engine();
    let mut c = TimelineController::new(&e);
    start(&e, &mut c);
    let changes = Rc::new(RefCell::new(Vec::new()));
    let seen = changes.clone();
    e.on_change(None, move |change| seen.borrow_mut().push(change.origin))
        .unwrap();
    let revision = e.revision();
    let report = c.tick(&mut e, ms(230)).unwrap();
    assert_eq!(e.revision(), revision + 1);
    assert_eq!(*changes.borrow(), vec![ChangeOrigin::Animation]);
    assert_eq!(report.advanced.len(), 1);
    assert_eq!((x(&e, "a"), x(&e, "b")), (150., 25.));
}
#[test]
fn oversized_tick_matches_partitioned_ticks_and_completes_once() {
    let mut a = engine();
    let mut ca = TimelineController::new(&a);
    let id = start(&a, &mut ca);
    assert_eq!(ca.tick(&mut a, Duration::MAX).unwrap().completed, vec![id]);
    let mut b = engine();
    let mut cb = TimelineController::new(&b);
    start(&b, &mut cb);
    for _ in 0..46 {
        cb.tick(&mut b, ms(10)).unwrap();
    }
    assert_eq!(a.scene(), b.scene());
    assert!(ca.tick(&mut a, ms(1)).unwrap().completed.is_empty());
}
#[test]
fn reverse_parent_cursor_preserves_pose_and_retraces_nested_children() {
    let mut e = engine();
    let mut c = TimelineController::new(&e);
    let id = start(&e, &mut c);
    c.tick(&mut e, ms(280)).unwrap();
    let before = e.scene().clone();
    c.reverse(&e, id).unwrap();
    assert_eq!(e.scene(), &before);
    c.tick(&mut e, ms(50)).unwrap();
    assert_eq!((x(&e, "a"), x(&e, "b")), (150., 25.));
    c.pause(&e, id).unwrap();
    c.tick(&mut e, ms(1000)).unwrap();
    assert_eq!(c.info(&e, id).unwrap().elapsed, ms(230));
    c.resume(&e, id).unwrap();
    c.tick(&mut e, Duration::MAX).unwrap();
    assert_eq!((x(&e, "a"), x(&e, "b")), (0., 0.));
    assert_eq!(c.info(&e, id).unwrap().status, AnimationStatus::Completed);
    c.reverse(&e, id).unwrap();
    assert_eq!(c.info(&e, id).unwrap().status, AnimationStatus::Paused);
    c.resume(&e, id).unwrap();
    c.tick(&mut e, ms(60)).unwrap();
    assert_eq!(x(&e, "a"), 50.);
}
#[test]
fn reverse_direction_uses_existing_easing_in_parent_travel_direction() {
    let mut e = engine();
    let mut c = TimelineController::new(&e);
    let mut spec = AnimationSpec::new(100, vec![Tween::translation(Point::new(100., 0.))]);
    spec.easing = Easing::EaseIn;
    let child = TimelineChild::animate(e.handle(&"a".into()).unwrap(), spec);
    let root = TimelineSpec::parallel(vec![TimelineChild::group(TimelineSpec::serial(vec![
        child,
    ]))])
    .with_timing(AnimationTiming {
        direction: PlaybackDirection::Reverse,
        ..Default::default()
    });
    let id = c.animate(&e, root).unwrap();
    c.seek(&mut e, id, ms(0)).unwrap();
    assert_eq!(x(&e, "a"), 100.);
    c.seek(&mut e, id, ms(50)).unwrap();
    assert_eq!(x(&e, "a"), 87.5);
    c.seek(&mut e, id, ms(100)).unwrap();
    assert_eq!(x(&e, "a"), 0.);
}
#[test]
fn parent_delay_repeat_gap_alternate_and_speed_reuse_schedule() {
    let mut e = engine();
    let mut c = TimelineController::new(&e);
    let root =
        TimelineSpec::serial(vec![move_to(&e, "a", 100, 100.)]).with_timing(AnimationTiming {
            delay_ms: 20,
            repeat_delay_ms: 30,
            iterations: IterationCount::Finite(2),
            direction: PlaybackDirection::Alternate,
            speed: PlaybackRate::new(2, 1).unwrap(),
        });
    let id = c.animate(&e, root).unwrap();
    assert_eq!(c.info(&e, id).unwrap().duration, ms(250));
    for (t, value) in [
        (0, 0.),
        (10, 0.),
        (20, 0.),
        (70, 50.),
        (120, 100.),
        (149, 100.),
        (150, 100.),
        (200, 50.),
        (250, 0.),
    ] {
        c.seek(&mut e, id, ms(t)).unwrap();
        assert_eq!(x(&e, "a"), value);
    }
    c.seek(&mut e, id, ms(0)).unwrap();
    c.resume(&e, id).unwrap();
    c.tick(&mut e, ms(35)).unwrap();
    assert_eq!(x(&e, "a"), 50.);
    assert_eq!(c.timing_sample(&e, id).unwrap().elapsed, ms(70));
}
#[test]
fn child_delay_repeat_and_speed_determine_serial_start_and_captured_pose() {
    let mut e = engine();
    let mut c = TimelineController::new(&e);
    let child = AnimationClip::from(AnimationSpec::new(
        100,
        vec![Tween::translation(Point::new(100., 0.))],
    ))
    .with_timing(AnimationTiming {
        delay_ms: 20,
        repeat_delay_ms: 30,
        iterations: IterationCount::Finite(2),
        direction: PlaybackDirection::Alternate,
        speed: PlaybackRate::new(2, 1).unwrap(),
    });
    let id = c
        .animate(
            &e,
            TimelineSpec::serial(vec![
                TimelineChild::animate(e.handle(&"a".into()).unwrap(), child),
                move_to(&e, "a", 100, 200.),
            ]),
        )
        .unwrap();
    assert_eq!(c.info(&e, id).unwrap().duration, ms(225));
    for (time, value) in [
        (5, 0.),
        (35, 50.),
        (65, 100.),
        (100, 50.),
        (125, 0.),
        (175, 100.),
        (225, 200.),
    ] {
        c.seek(&mut e, id, ms(time)).unwrap();
        assert_eq!(x(&e, "a"), value, "at {time}");
    }
}
#[test]
fn ending_restore_is_cumulative_and_seekable_at_child_and_parent_levels() {
    let mut e = engine();
    let mut c = TimelineController::new(&e);
    let id = c
        .animate(
            &e,
            TimelineSpec::serial(vec![
                move_to(&e, "a", 100, 100.).with_ending_style(EndingStyle::Restore),
                move_to(&e, "a", 100, 200.),
            ])
            .with_ending_style(EndingStyle::Restore),
        )
        .unwrap();
    for (time, value) in [
        (50, 50.),
        (100, 0.),
        (150, 100.),
        (200, 0.),
        (150, 100.),
        (50, 50.),
    ] {
        c.seek(&mut e, id, ms(time)).unwrap();
        assert_eq!(x(&e, "a"), value, "at {time}");
    }
}
#[test]
fn sparse_keyframes_and_disjoint_parallel_properties_share_captured_baseline() {
    let mut e = engine();
    let mut c = TimelineController::new(&e);
    let sequence = AnimationSequence::new(vec![
        AnimationSpec::new(100, vec![Tween::translation(Point::new(100., 0.))]),
        AnimationSpec::new(100, vec![Tween::fill_color(Color::WHITE)]),
    ]);
    let id = c
        .animate(
            &e,
            TimelineSpec::parallel(vec![
                TimelineChild::animate(e.handle(&"a".into()).unwrap(), sequence),
                opacity(&e, "a", 200, 0.),
            ]),
        )
        .unwrap();
    c.seek(&mut e, id, ms(150)).unwrap();
    let object = e.object(e.handle(&"a".into()).unwrap()).unwrap();
    assert_eq!(object.transform.tx, 100.);
    assert_eq!(object.style.opacity, 0.25);
    assert_eq!(object.style.fill, Some(Color::rgb(128, 128, 128).into()));
}
#[test]
fn conflicting_parallel_siblings_reject_even_if_offsets_do_not_overlap() {
    let e = engine();
    let mut c = TimelineController::new(&e);
    let spec = TimelineSpec::parallel(vec![
        move_to(&e, "a", 100, 100.),
        move_to(&e, "a", 100, 200.).with_offset(200),
    ]);
    assert_eq!(c.animate(&e, spec).unwrap_err(), AnimationError::Conflict);
    assert!(c.is_empty());
    assert_eq!(e.revision(), 0);
    assert!(
        c.animate(
            &e,
            TimelineSpec::serial(vec![
                move_to(&e, "a", 100, 100.),
                move_to(&e, "a", 100, 200.)
            ])
        )
        .is_ok()
    );
}
#[test]
fn future_properties_are_reserved_across_parent_pause_gaps_and_delays() {
    let mut e = engine();
    let mut c = TimelineController::new(&e);
    let id = start(&e, &mut c);
    c.pause(&e, id).unwrap();
    assert_eq!(
        c.animate(&e, TimelineSpec::serial(vec![move_to(&e, "b", 50, 10.)]))
            .unwrap_err(),
        AnimationError::Conflict
    );
    c.cancel(&e, id).unwrap();
    let other = c
        .animate(&e, TimelineSpec::serial(vec![move_to(&e, "b", 50, 10.)]))
        .unwrap();
    c.tick(&mut e, ms(50)).unwrap();
    c.seek(&mut e, other, ms(0)).unwrap();
    assert_eq!(c.info(&e, other).unwrap().status, AnimationStatus::Paused);
}
#[test]
fn completed_rewind_or_reverse_cannot_steal_another_parent_properties() {
    let mut e = engine();
    let mut c = TimelineController::new(&e);
    let id = start(&e, &mut c);
    c.tick(&mut e, Duration::MAX).unwrap();
    c.animate(&e, TimelineSpec::serial(vec![move_to(&e, "b", 100, 10.)]))
        .unwrap();
    let before = e.scene().clone();
    assert_eq!(
        c.seek(&mut e, id, ms(0)).unwrap_err(),
        AnimationError::Conflict
    );
    assert_eq!(c.reverse(&e, id).unwrap_err(), AnimationError::Conflict);
    assert_eq!(e.scene(), &before);
}
#[test]
fn external_edit_to_one_future_child_cancels_whole_parent_without_sibling_writes() {
    let mut e = engine();
    let mut c = TimelineController::new(&e);
    let id = start(&e, &mut c);
    c.tick(&mut e, ms(60)).unwrap();
    let target = e.handle(&"b".into()).unwrap();
    e.set_property(
        target,
        PropertyKey::Transform,
        serde_json::to_value(gpui_flow_scene::Affine::translation(12., 0.)).unwrap(),
    )
    .unwrap();
    let before = e.scene().clone();
    let report = c.tick(&mut e, ms(200)).unwrap();
    assert_eq!(report.cancelled, vec![id]);
    assert_eq!(e.scene(), &before);
    assert_eq!(
        c.info(&e, id).unwrap().status,
        AnimationStatus::Cancelled(CancelReason::Overridden)
    );
    assert!(c.seek(&mut e, id, ms(0)).is_err());
}
#[test]
fn destroyed_or_detached_child_cancels_whole_parent_and_never_retargets_reused_id() {
    for destroy in [false, true] {
        let mut e = engine();
        let mut c = TimelineController::new(&e);
        let id = start(&e, &mut c);
        c.tick(&mut e, ms(60)).unwrap();
        let h = e.handle(&"b".into()).unwrap();
        if destroy {
            e.destroy(h).unwrap();
            e.create("rect", "b", json!({}), None, None).unwrap();
        } else {
            let (tree, _) = e.detach(h).unwrap();
            e.attach(&tree, None, None).unwrap();
        }
        let before = e.scene().clone();
        let report = c.tick(&mut e, Duration::MAX).unwrap();
        assert_eq!(report.cancelled, vec![id]);
        assert_eq!(e.scene(), &before);
        assert_eq!(
            c.info(&e, id).unwrap().status,
            AnimationStatus::Cancelled(CancelReason::TargetUnavailable)
        );
    }
}
#[test]
fn duration_offsets_infinite_loops_and_child_parent_rate_limits_are_rejected() {
    let e = engine();
    let mut c = TimelineController::new(&e);
    let infinite = AnimationTiming {
        iterations: IterationCount::Infinite,
        ..Default::default()
    };
    let slow = AnimationTiming {
        speed: PlaybackRate::new(1, u32::MAX).unwrap(),
        ..Default::default()
    };
    for spec in [
        TimelineSpec::serial(vec![]),
        TimelineSpec::serial(vec![move_to(&e, "a", 0, 1.)]),
        TimelineSpec::serial(vec![move_to(&e, "a", 1, 1.).with_offset(u64::MAX)]),
        TimelineSpec::serial(vec![move_to(&e, "a", 86_400_001, 1.)]),
        TimelineSpec::serial(vec![move_to(&e, "a", 1, 1.)]).with_timing(infinite),
        TimelineSpec::serial(vec![TimelineChild::group(
            TimelineSpec::serial(vec![move_to(&e, "a", 1, 1.)]).with_timing(infinite),
        )]),
        TimelineSpec::serial(vec![move_to(&e, "a", 1000, 1.)]).with_timing(slow),
        TimelineSpec::serial(vec![TimelineChild::group(
            TimelineSpec::serial(vec![move_to(&e, "a", 1000, 1.)]).with_timing(slow),
        )]),
    ] {
        assert!(c.animate(&e, spec).is_err());
        assert!(c.is_empty());
    }
    let id = c
        .animate(
            &e,
            TimelineSpec::serial(vec![move_to(&e, "a", 86_400_000, 1.)]),
        )
        .unwrap();
    assert_eq!(c.info(&e, id).unwrap().duration, MAX_TIMELINE_DURATION);
}
#[test]
fn nesting_and_total_nodes_have_exact_bounds() {
    let e = engine();
    let mut c = TimelineController::new(&e);
    let mut node = move_to(&e, "a", 1, 1.);
    for _ in 0..MAX_TIMELINE_DEPTH - 2 {
        node = TimelineChild::group(TimelineSpec::serial(vec![node]));
    }
    let root = TimelineSpec::serial(vec![node]);
    let id = c.animate(&e, root.clone()).unwrap();
    c.remove(id);
    assert!(
        c.animate(&e, TimelineSpec::serial(vec![TimelineChild::group(root)]))
            .is_err()
    );
    let children = vec![move_to(&e, "a", 1, 1.); MAX_TIMELINE_NODES - 1];
    let id = c.animate(&e, TimelineSpec::serial(children)).unwrap();
    c.remove(id);
    assert_eq!(
        c.animate(
            &e,
            TimelineSpec::serial(vec![move_to(&e, "a", 1, 1.); MAX_TIMELINE_NODES])
        )
        .unwrap_err(),
        AnimationError::Capacity
    );
}
#[test]
fn union_property_and_controller_record_budgets_are_bounded() {
    let mut scene = Scene::new();
    for n in 0..129 {
        scene
            .insert(
                DisplayObject::new(format!("n{n}"), display::Rect::new(1., 1.)),
                None,
                None,
            )
            .unwrap();
    }
    let e = SceneEngine::new(scene).unwrap();
    let mut c = TimelineController::new(&e);
    let children: Vec<_> = (0..129)
        .map(|n| {
            TimelineChild::animate(
                e.handle(&format!("n{n}").into()).unwrap(),
                AnimationSpec::new(
                    1,
                    vec![Tween::translation(Point::new(1., 1.)), Tween::opacity(0.)],
                ),
            )
        })
        .collect();
    let id = c
        .animate(&e, TimelineSpec::parallel(children[..128].to_vec()))
        .unwrap();
    assert!(c.remove(id));
    assert_eq!(
        c.animate(&e, TimelineSpec::parallel(children)).unwrap_err(),
        AnimationError::Capacity
    );
    let e = engine();
    let mut c = TimelineController::new(&e);
    for _ in 0..MAX_TIMELINES {
        let id = c
            .animate(&e, TimelineSpec::serial(vec![move_to(&e, "a", 1, 1.)]))
            .unwrap();
        c.cancel(&e, id).unwrap();
    }
    assert_eq!(
        c.animate(&e, TimelineSpec::serial(vec![move_to(&e, "a", 1, 1.)]))
            .unwrap_err(),
        AnimationError::Capacity
    );
}
#[test]
fn wrong_engine_and_foreign_controller_ids_are_rejected_without_writes() {
    let mut e = engine();
    let other = engine();
    let mut c = TimelineController::new(&e);
    let id = start(&e, &mut c);
    assert_eq!(c.info(&other, id).unwrap_err(), AnimationError::WrongEngine);
    let mut second = TimelineController::new(&e);
    assert_eq!(
        second.seek(&mut e, id, ms(100)).unwrap_err(),
        AnimationError::UnknownAnimation
    );
}
#[test]
fn fractional_parent_ticks_match_combined_clock_and_reverse_carries() {
    let mut a = engine();
    let mut ca = TimelineController::new(&a);
    let id = ca
        .animate(
            &a,
            TimelineSpec::serial(vec![move_to(&a, "a", 1, 1.)]).with_timing(AnimationTiming {
                speed: PlaybackRate::new(1, 3).unwrap(),
                ..Default::default()
            }),
        )
        .unwrap();
    for _ in 0..10 {
        ca.tick(&mut a, Duration::from_nanos(1)).unwrap();
    }
    assert_eq!(ca.info(&a, id).unwrap().elapsed, Duration::from_nanos(3));
    ca.pause(&a, id).unwrap();
    ca.resume(&a, id).unwrap();
    for _ in 0..2 {
        ca.tick(&mut a, Duration::from_nanos(1)).unwrap();
    }
    assert_eq!(ca.info(&a, id).unwrap().elapsed, Duration::from_nanos(4));
    ca.reverse(&a, id).unwrap();
    for _ in 0..3 {
        ca.tick(&mut a, Duration::from_nanos(1)).unwrap();
    }
    assert_eq!(ca.info(&a, id).unwrap().elapsed, Duration::from_nanos(3));
    ca.seek(&mut a, id, Duration::from_nanos(10)).unwrap();
    ca.tick(&mut a, Duration::from_nanos(1)).unwrap();
    assert_eq!(ca.info(&a, id).unwrap().elapsed, Duration::from_nanos(10));
}
#[test]
fn invalid_late_child_endpoint_leaves_no_registration_ownership_or_revision() {
    let e = engine();
    let mut c = TimelineController::new(&e);
    let before = e.scene().clone();
    assert!(
        c.animate(
            &e,
            TimelineSpec::serial(vec![move_to(&e, "a", 100, 100.), opacity(&e, "b", 100, 2.)])
        )
        .is_err()
    );
    assert!(c.is_empty());
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), 0);
    start(&e, &mut c);
}
#[test]
fn late_child_geometry_failure_rolls_back_all_parent_clocks_and_scene() {
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
    let mut e = SceneEngine::with_registry(engine().scene().clone(), registry).unwrap();
    e.create("disc", "disc", json!({}), None, None).unwrap();
    let mut c = TimelineController::new(&e);
    let id = c
        .animate(
            &e,
            TimelineSpec::parallel(vec![
                move_to(&e, "a", 100, 100.),
                TimelineChild::animate(
                    e.handle(&"disc".into()).unwrap(),
                    AnimationSpec::new(100, vec![Tween::custom_number("radius", 20.)]),
                ),
            ]),
        )
        .unwrap();
    let other = c
        .animate(&e, TimelineSpec::serial(vec![move_to(&e, "b", 100, 100.)]))
        .unwrap();
    let before = e.scene().clone();
    let revision = e.revision();
    assert!(c.seek(&mut e, id, ms(50)).is_err());
    assert_eq!(e.scene(), &before);
    assert_eq!(c.info(&e, id).unwrap().elapsed, Duration::ZERO);
    assert!(c.tick(&mut e, ms(50)).is_err());
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), revision);
    assert_eq!(c.info(&e, id).unwrap().elapsed, Duration::ZERO);
    assert_eq!(c.info(&e, other).unwrap().elapsed, Duration::ZERO);
    c.tick(&mut e, ms(100)).unwrap();
    assert_eq!(x(&e, "a"), 100.);
    assert_eq!(x(&e, "b"), 100.);
}

#[test]
fn child_rate_ceiling_places_successor_at_exact_parent_nanosecond_boundary() {
    let mut e = engine();
    let mut c = TimelineController::new(&e);
    let clip = AnimationClip::from(AnimationSpec::new(
        1,
        vec![Tween::translation(Point::new(1., 0.))],
    ))
    .with_timing(AnimationTiming {
        speed: PlaybackRate::new(3, 1).unwrap(),
        ..Default::default()
    });
    let id = c
        .animate(
            &e,
            TimelineSpec::serial(vec![
                TimelineChild::animate(e.handle(&"a".into()).unwrap(), clip),
                move_to(&e, "a", 1, 2.),
            ]),
        )
        .unwrap();
    assert_eq!(
        c.info(&e, id).unwrap().duration,
        Duration::from_nanos(1_333_334)
    );
    c.seek(&mut e, id, Duration::from_nanos(333_333)).unwrap();
    assert!((x(&e, "a") - 0.999_999).abs() < 1e-7);
    c.seek(&mut e, id, Duration::from_nanos(333_334)).unwrap();
    assert_eq!(x(&e, "a"), 1.);
    c.seek(&mut e, id, Duration::from_nanos(833_334)).unwrap();
    assert_eq!(x(&e, "a"), 1.5);
}

#[test]
fn cancelled_parent_does_not_prevent_an_unrelated_parent_from_advancing() {
    let mut e = engine();
    let mut c = TimelineController::new(&e);
    let id = start(&e, &mut c);
    let other = c
        .animate(&e, TimelineSpec::serial(vec![move_to(&e, "c", 100, 100.)]))
        .unwrap();
    c.tick(&mut e, ms(20)).unwrap();
    e.destroy(e.handle(&"b".into()).unwrap()).unwrap();
    let report = c.tick(&mut e, ms(30)).unwrap();
    assert_eq!(report.cancelled, vec![id]);
    assert_eq!(report.advanced, vec![other]);
    assert_eq!(x(&e, "a"), 10.);
    assert_eq!(x(&e, "c"), 50.);
}

#[test]
fn playback_rate_changes_reset_fraction_and_invalid_slow_rate_is_atomic() {
    let mut e = engine();
    let mut c = TimelineController::new(&e);
    let id = c
        .animate(
            &e,
            TimelineSpec::serial(vec![move_to(&e, "a", 100, 100.)]).with_timing(AnimationTiming {
                speed: PlaybackRate::new(1, 3).unwrap(),
                ..Default::default()
            }),
        )
        .unwrap();
    c.tick(&mut e, Duration::from_nanos(2)).unwrap();
    c.set_playback_rate(&e, id, PlaybackRate::new(1, 2).unwrap())
        .unwrap();
    c.tick(&mut e, Duration::from_nanos(1)).unwrap();
    assert_eq!(c.info(&e, id).unwrap().elapsed, Duration::ZERO);
    assert!(
        c.set_playback_rate(&e, id, PlaybackRate::new(1, u32::MAX).unwrap())
            .is_err()
    );
    c.tick(&mut e, Duration::from_nanos(1)).unwrap();
    assert_eq!(c.info(&e, id).unwrap().elapsed, Duration::from_nanos(1));
}

#[test]
fn completed_reverse_reacquires_current_values_independently_of_idle_ticks() {
    for idle_tick in [false, true] {
        let mut e = engine();
        let mut c = TimelineController::new(&e);
        let id = c
            .animate(&e, TimelineSpec::serial(vec![move_to(&e, "a", 100, 100.)]))
            .unwrap();
        c.tick(&mut e, ms(100)).unwrap();
        e.set_property(
            e.handle(&"a".into()).unwrap(),
            PropertyKey::Transform,
            serde_json::to_value(gpui_flow_scene::Affine::translation(125., 0.)).unwrap(),
        )
        .unwrap();
        if idle_tick {
            c.tick(&mut e, Duration::ZERO).unwrap();
        }
        c.reverse(&e, id).unwrap();
        assert_eq!(x(&e, "a"), 125.);
        c.resume(&e, id).unwrap();
        c.tick(&mut e, ms(50)).unwrap();
        assert_eq!(x(&e, "a"), 50.);
        assert_eq!(c.info(&e, id).unwrap().elapsed, ms(50));
    }
}

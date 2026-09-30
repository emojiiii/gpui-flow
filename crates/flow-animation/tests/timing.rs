//! Timing regression sources. These tests have not been executed in this change.
use gpui_flow_animation::*;
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{
    CustomDefinition, DirtyFlags, ElementRegistry, PropertyDescriptor, PropertyKey, PropertySchema,
    SceneEngine,
};
use gpui_flow_scene::{DisplayObject, Point, Scene, display};
use serde_json::json;
use std::time::Duration;

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}
fn ns(value: u128) -> Duration {
    Duration::new(
        (value / 1_000_000_000) as u64,
        (value % 1_000_000_000) as u32,
    )
}
fn engine() -> SceneEngine {
    let mut scene = Scene::new();
    for id in ["a", "b"] {
        scene
            .insert(
                DisplayObject::new(id, display::Rect::new(40., 30.)),
                None,
                None,
            )
            .unwrap();
    }
    SceneEngine::new(scene).unwrap()
}
fn x(engine: &SceneEngine, id: &str) -> f32 {
    engine.scene().get(&id.into()).unwrap().object.transform.tx
}
fn start(
    controller: &mut AnimationController,
    engine: &SceneEngine,
    target: &str,
    timing: AnimationTiming,
) -> AnimationId {
    controller
        .animate_with_timing(
            engine,
            engine.handle(&target.into()).unwrap(),
            AnimationSpec::new(100, vec![Tween::translation(Point::new(100., 0.))]),
            timing,
        )
        .unwrap()
}

#[test]
fn pure_mapping_defines_delay_repeat_gaps_alternate_and_terminal_endpoints() {
    let timing = AnimationTiming {
        delay_ms: 10,
        repeat_delay_ms: 20,
        iterations: IterationCount::Finite(3),
        direction: PlaybackDirection::Alternate,
        ..Default::default()
    };
    assert_eq!(timing.total_duration(ms(100)).unwrap(), Some(ms(350)));
    for (position, phase, iteration, local, reversed) in [
        (0, TimingPhase::Delay, 0, 0, false),
        (9, TimingPhase::Delay, 0, 0, false),
        (10, TimingPhase::Active, 0, 0, false),
        (60, TimingPhase::Active, 0, 50, false),
        (110, TimingPhase::RepeatDelay, 0, 100, false),
        (129, TimingPhase::RepeatDelay, 0, 100, false),
        (130, TimingPhase::Active, 1, 100, true),
        (180, TimingPhase::Active, 1, 50, true),
        (230, TimingPhase::RepeatDelay, 1, 0, true),
        (249, TimingPhase::RepeatDelay, 1, 0, true),
        (250, TimingPhase::Active, 2, 0, false),
        (350, TimingPhase::Completed, 2, 100, false),
        (900, TimingPhase::Completed, 2, 100, false),
    ] {
        let sample = timing.sample(ms(100), ms(position)).unwrap();
        assert_eq!(sample.phase, phase);
        assert_eq!(sample.iteration, iteration);
        assert_eq!(sample.cycle_position, ms(local));
        assert_eq!(sample.reversed, reversed);
        assert_eq!(sample.elapsed, ms(position.min(350)));
    }
}

#[test]
fn repeat_intervals_are_half_open_but_finite_end_keeps_last_endpoint() {
    let timing = AnimationTiming {
        iterations: IterationCount::Finite(2),
        ..Default::default()
    };
    let boundary = timing.sample(ms(100), ms(100)).unwrap();
    assert_eq!(boundary.phase, TimingPhase::Active);
    assert_eq!(boundary.iteration, 1);
    assert_eq!(boundary.cycle_position, Duration::ZERO);
    let endpoint = timing.sample(ms(100), ms(200)).unwrap();
    assert_eq!(endpoint.phase, TimingPhase::Completed);
    assert_eq!(endpoint.iteration, 1);
    assert_eq!(endpoint.cycle_position, ms(100));
}

#[test]
fn all_directions_have_defined_start_and_final_iteration_parity() {
    for (direction, first, second, end) in [
        (PlaybackDirection::Forward, 25, 25, 100),
        (PlaybackDirection::Reverse, 75, 75, 0),
        (PlaybackDirection::Alternate, 25, 75, 0),
        (PlaybackDirection::AlternateReverse, 75, 25, 100),
    ] {
        let timing = AnimationTiming {
            direction,
            iterations: IterationCount::Finite(2),
            ..Default::default()
        };
        assert_eq!(
            timing.sample(ms(100), ms(25)).unwrap().cycle_position,
            ms(first)
        );
        assert_eq!(
            timing.sample(ms(100), ms(125)).unwrap().cycle_position,
            ms(second)
        );
        assert_eq!(
            timing.sample(ms(100), ms(200)).unwrap().cycle_position,
            ms(end)
        );
    }
}

#[test]
fn huge_infinite_iteration_mapping_preserves_integer_parity_and_remainder() {
    let timing = AnimationTiming {
        iterations: IterationCount::Infinite,
        direction: PlaybackDirection::Alternate,
        ..Default::default()
    };
    // Far beyond exact integer parity in f32/f64. There is no iteration walk.
    for iteration in [(1u128 << 60), (1u128 << 60) + 1] {
        let sample = timing.sample(ns(1000), ns(iteration * 1000 + 250)).unwrap();
        assert_eq!(sample.iteration, iteration);
        assert_eq!(
            sample.cycle_position,
            ns(if iteration % 2 == 0 { 250 } else { 750 })
        );
        assert_eq!(sample.phase, TimingPhase::Active);
        assert_eq!(sample.total_duration, None);
    }
    let maximum = timing.sample(ns(3), Duration::MAX).unwrap();
    assert_eq!(maximum.iteration, Duration::MAX.as_nanos() / 3);
    assert_ne!(maximum.phase, TimingPhase::Completed);
}

#[test]
fn timing_rejects_zero_cycles_zero_iterations_and_unrepresentable_totals() {
    assert!(
        AnimationTiming::default()
            .sample(Duration::ZERO, Duration::ZERO)
            .is_err()
    );
    assert!(
        AnimationTiming {
            iterations: IterationCount::Finite(0),
            ..Default::default()
        }
        .total_duration(ms(100))
        .is_err()
    );
    assert!(
        AnimationTiming {
            iterations: IterationCount::Finite(u64::MAX),
            ..Default::default()
        }
        .total_duration(Duration::MAX)
        .is_err()
    );
    assert!(
        AnimationTiming {
            delay_ms: 1,
            ..Default::default()
        }
        .total_duration(Duration::MAX)
        .is_err()
    );
    assert_eq!(
        AnimationTiming {
            repeat_delay_ms: u64::MAX,
            ..Default::default()
        }
        .total_duration(Duration::MAX)
        .unwrap(),
        Some(Duration::MAX)
    );
    assert!(PlaybackRate::new(0, 1).is_err());
    assert!(PlaybackRate::new(1, 0).is_err());
    assert!(
        serde_json::from_value::<PlaybackRate>(json!({"numerator": 1, "denominator": 0})).is_err()
    );
}

#[test]
fn default_timing_preserves_existing_sequence_api_and_json() {
    let mut first = engine();
    let mut second = engine();
    let mut a = AnimationController::new(&first);
    let mut b = AnimationController::new(&second);
    let spec = AnimationSpec::new(100, vec![Tween::translation(Point::new(100., 0.))]);
    let old_json = json!({"duration_ms":100,"easing":"linear","tweens":[
        {"property":{"kind":"translation"},"to":{"type":"point","value":{"x":100.0,"y":0.0}}}
    ]});
    assert_eq!(
        serde_json::from_value::<AnimationSpec>(old_json).unwrap(),
        spec
    );
    let id = a
        .animate(&first, first.handle(&"a".into()).unwrap(), spec.clone())
        .unwrap();
    let clip = AnimationClip::from(spec);
    let serialized = serde_json::to_value(&clip).unwrap();
    assert_eq!(
        serde_json::from_value::<AnimationClip>(serialized).unwrap(),
        clip
    );
    let other = b
        .animate_clip(&second, second.handle(&"a".into()).unwrap(), clip)
        .unwrap();
    for position in [0, 25, 100, 25, 0] {
        a.seek(&mut first, id, ms(position)).unwrap();
        b.seek(&mut second, other, ms(position)).unwrap();
        assert_eq!(first.scene(), second.scene());
        assert_eq!(
            a.info(&first, id).unwrap().status,
            b.info(&second, other).unwrap().status
        );
    }
    assert_eq!(a.info(&first, id).unwrap().duration, ms(100));
}

#[test]
fn delayed_reverse_holds_original_pose_then_uses_easing_in_travel_direction() {
    let mut engine = engine();
    let mut controller = AnimationController::new(&engine);
    let mut spec = AnimationSpec::new(100, vec![Tween::translation(Point::new(100., 0.))]);
    spec.easing = Easing::EaseIn;
    let id = controller
        .animate_with_timing(
            &engine,
            engine.handle(&"a".into()).unwrap(),
            spec,
            AnimationTiming {
                delay_ms: 20,
                direction: PlaybackDirection::Reverse,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(x(&engine, "a"), 0.); // registration never writes
    controller.tick(&mut engine, ms(19)).unwrap();
    assert_eq!(x(&engine, "a"), 0.);
    controller.tick(&mut engine, ms(1)).unwrap();
    assert_eq!(x(&engine, "a"), 100.);
    controller.seek(&mut engine, id, ms(45)).unwrap();
    assert_eq!(x(&engine, "a"), 98.4375); // 100 -> 0, ease_in(0.25)
    controller.seek(&mut engine, id, ms(120)).unwrap();
    assert_eq!(x(&engine, "a"), 0.);
    controller.seek(&mut engine, id, ms(19)).unwrap();
    assert_eq!(x(&engine, "a"), 0.);
    assert_eq!(
        controller.info(&engine, id).unwrap().status,
        AnimationStatus::Paused
    );
}

#[test]
fn speed_scales_delay_and_ticks_but_seek_always_uses_absolute_logical_time() {
    let mut engine = engine();
    let mut controller = AnimationController::new(&engine);
    let id = start(
        &mut controller,
        &engine,
        "a",
        AnimationTiming {
            delay_ms: 100,
            speed: PlaybackRate::new(2, 1).unwrap(),
            ..Default::default()
        },
    );
    controller.tick(&mut engine, ms(25)).unwrap();
    assert_eq!(controller.info(&engine, id).unwrap().elapsed, ms(50));
    assert_eq!(x(&engine, "a"), 0.);
    controller.tick(&mut engine, ms(50)).unwrap();
    assert_eq!(x(&engine, "a"), 50.);
    for _ in 0..3 {
        assert!(!controller.seek(&mut engine, id, ms(150)).unwrap().changed());
        assert_eq!(x(&engine, "a"), 50.);
    }
    controller.pause(&engine, id).unwrap();
    controller.tick(&mut engine, Duration::MAX).unwrap();
    assert_eq!(controller.info(&engine, id).unwrap().elapsed, ms(150));
    controller
        .set_playback_rate(&engine, id, PlaybackRate::new(1, 2).unwrap())
        .unwrap();
    controller.resume(&engine, id).unwrap();
    controller.tick(&mut engine, ms(100)).unwrap();
    assert_eq!(
        controller.info(&engine, id).unwrap().status,
        AnimationStatus::Completed
    );
    assert_eq!(x(&engine, "a"), 100.);
}

#[test]
fn rational_rate_retains_subnanoseconds_across_tick_partitioning_and_pause() {
    let timing = AnimationTiming {
        speed: PlaybackRate::new(1, 3).unwrap(),
        ..Default::default()
    };
    let mut engine = engine();
    let mut controller = AnimationController::new(&engine);
    let id = start(&mut controller, &engine, "a", timing);
    controller.tick(&mut engine, ns(1)).unwrap();
    assert_eq!(
        controller.info(&engine, id).unwrap().elapsed,
        Duration::ZERO
    );
    controller.pause(&engine, id).unwrap();
    controller.tick(&mut engine, Duration::MAX).unwrap();
    controller.resume(&engine, id).unwrap();
    controller.tick(&mut engine, ns(2)).unwrap();
    assert_eq!(controller.info(&engine, id).unwrap().elapsed, ns(1));
    controller.seek(&mut engine, id, Duration::ZERO).unwrap();
    for _ in 0..30 {
        controller.tick(&mut engine, ns(1)).unwrap();
    }
    let expected = engine.scene().clone();
    controller.seek(&mut engine, id, Duration::ZERO).unwrap();
    controller.tick(&mut engine, ns(30)).unwrap();
    assert_eq!(controller.info(&engine, id).unwrap().elapsed, ns(10));
    assert_eq!(engine.scene(), &expected);
}

#[test]
fn seek_and_rate_changes_explicitly_clear_old_fractional_clock_carry() {
    let mut engine = engine();
    let mut controller = AnimationController::new(&engine);
    let id = start(
        &mut controller,
        &engine,
        "a",
        AnimationTiming {
            speed: PlaybackRate::new(1, 3).unwrap(),
            ..Default::default()
        },
    );
    controller.tick(&mut engine, ns(2)).unwrap();
    controller.seek(&mut engine, id, ms(10)).unwrap();
    controller.tick(&mut engine, ns(1)).unwrap();
    assert_eq!(controller.info(&engine, id).unwrap().elapsed, ms(10));
    controller
        .set_playback_rate(&engine, id, PlaybackRate::new(1, 2).unwrap())
        .unwrap();
    controller.tick(&mut engine, ns(1)).unwrap();
    assert_eq!(controller.info(&engine, id).unwrap().elapsed, ms(10));
    controller.tick(&mut engine, ns(1)).unwrap();
    assert_eq!(
        controller.info(&engine, id).unwrap().elapsed,
        ms(10) + ns(1)
    );
}

#[test]
fn large_delta_skips_cycles_without_missing_finite_completion_or_final_parity() {
    for (iterations, endpoint) in [(2, 0.), (3, 100.), (u64::MAX, 100.)] {
        let mut engine = engine();
        let mut controller = AnimationController::new(&engine);
        let id = start(
            &mut controller,
            &engine,
            "a",
            AnimationTiming {
                iterations: IterationCount::Finite(iterations),
                direction: PlaybackDirection::Alternate,
                speed: PlaybackRate::new(u32::MAX, 1).unwrap(),
                ..Default::default()
            },
        );
        let report = controller.tick(&mut engine, Duration::MAX).unwrap();
        assert_eq!(report.completed, vec![id]);
        assert_eq!(x(&engine, "a"), endpoint);
        assert!(
            controller
                .tick(&mut engine, Duration::MAX)
                .unwrap()
                .completed
                .is_empty()
        );
    }
}

#[test]
fn invalid_timing_and_zero_frames_reserve_nothing_and_do_not_change_the_document() {
    let engine = engine();
    let before = engine.scene().clone();
    let mut controller = AnimationController::new(&engine);
    for (duration, iterations) in [(0, 1), (100, 0), (u64::MAX, u64::MAX)] {
        let result = controller.animate_with_timing(
            &engine,
            engine.handle(&"a".into()).unwrap(),
            AnimationSpec::new(duration, vec![Tween::translation(Point::new(100., 0.))]),
            AnimationTiming {
                iterations: IterationCount::Finite(iterations),
                ..Default::default()
            },
        );
        assert!(result.is_err());
        assert!(controller.is_empty());
        assert_eq!(engine.scene(), &before);
    }
    start(&mut controller, &engine, "a", AnimationTiming::default());
}

#[test]
fn infinite_overflow_rolls_back_every_target_clock_and_property_atomically() {
    let mut engine = engine();
    let mut controller = AnimationController::new(&engine);
    let a = start(&mut controller, &engine, "a", AnimationTiming::default());
    let b = start(
        &mut controller,
        &engine,
        "b",
        AnimationTiming {
            iterations: IterationCount::Infinite,
            ..Default::default()
        },
    );
    controller.seek(&mut engine, a, ms(50)).unwrap();
    controller
        .seek(&mut engine, b, Duration::MAX - ns(1))
        .unwrap();
    let scene = engine.scene().clone();
    let revision = engine.revision();
    let before_a = controller.info(&engine, a).unwrap();
    let before_b = controller.info(&engine, b).unwrap();
    assert!(controller.tick(&mut engine, ns(2)).is_err());
    assert_eq!(engine.scene(), &scene);
    assert_eq!(engine.revision(), revision);
    assert_eq!(controller.info(&engine, a).unwrap(), before_a);
    assert_eq!(controller.info(&engine, b).unwrap(), before_b);
    controller.seek(&mut engine, b, Duration::ZERO).unwrap();
    controller.tick(&mut engine, ns(2)).unwrap();
    assert_eq!(controller.info(&engine, a).unwrap().elapsed, ms(50) + ns(2));
    assert_eq!(controller.info(&engine, b).unwrap().elapsed, ns(2));
}

#[test]
fn delayed_paused_and_repeating_animations_keep_ownership_until_cancel_or_completion() {
    let mut engine = engine();
    let mut controller = AnimationController::new(&engine);
    let id = start(
        &mut controller,
        &engine,
        "a",
        AnimationTiming {
            delay_ms: 50,
            repeat_delay_ms: 50,
            iterations: IterationCount::Finite(2),
            ..Default::default()
        },
    );
    for position in [0, 25, 175, 200] {
        controller.seek(&mut engine, id, ms(position)).unwrap();
        controller.pause(&engine, id).unwrap();
        let result = controller.animate(
            &engine,
            engine.handle(&"a".into()).unwrap(),
            AnimationSpec::new(100, vec![Tween::translation(Point::ZERO)]),
        );
        assert_eq!(result.unwrap_err(), AnimationError::Conflict);
    }
    controller.cancel(&engine, id).unwrap();
    let replacement = start(&mut controller, &engine, "a", AnimationTiming::default());
    controller.tick(&mut engine, ms(100)).unwrap();
    assert_eq!(
        controller.info(&engine, replacement).unwrap().status,
        AnimationStatus::Completed
    );
    start(&mut controller, &engine, "a", AnimationTiming::default());
    assert!(controller.seek(&mut engine, id, Duration::ZERO).is_err());
}

#[test]
fn looping_destroy_same_id_recreation_and_detach_reattach_never_retarget_old_record() {
    for destroy in [false, true] {
        let mut engine = engine();
        let mut controller = AnimationController::new(&engine);
        let target = engine.handle(&"a".into()).unwrap();
        let id = start(
            &mut controller,
            &engine,
            "a",
            AnimationTiming {
                iterations: IterationCount::Infinite,
                direction: PlaybackDirection::Alternate,
                ..Default::default()
            },
        );
        controller.tick(&mut engine, ms(25)).unwrap();
        if destroy {
            engine.destroy(target).unwrap();
            engine.create("rect", "a", json!({}), None, None).unwrap();
            assert_ne!(engine.handle(&"a".into()).unwrap(), target);
        } else {
            let (tree, _) = engine.detach(target).unwrap();
            engine.attach(&tree, None, None).unwrap();
        }
        let scene = engine.scene().clone();
        assert_eq!(
            controller.tick(&mut engine, ms(1000)).unwrap().cancelled,
            vec![id]
        );
        assert_eq!(engine.scene(), &scene);
        assert_eq!(
            controller.info(&engine, id).unwrap().status,
            AnimationStatus::Cancelled(CancelReason::TargetUnavailable)
        );
        assert!(controller.resume(&engine, id).is_err());
        assert!(controller.seek(&mut engine, id, ms(25)).is_err());
    }
}

#[test]
fn delayed_external_override_is_not_overwritten_and_does_not_cancel_other_target() {
    let mut engine = engine();
    let mut controller = AnimationController::new(&engine);
    let timing = AnimationTiming {
        delay_ms: 50,
        iterations: IterationCount::Infinite,
        ..Default::default()
    };
    let a = start(&mut controller, &engine, "a", timing);
    let b = start(&mut controller, &engine, "b", timing);
    engine
        .transaction(|scene| scene.update_object(&"a".into(), |object| object.transform.tx = 400.))
        .unwrap();
    let report = controller.tick(&mut engine, ms(75)).unwrap();
    assert_eq!(report.cancelled, vec![a]);
    assert_eq!(x(&engine, "a"), 400.);
    assert_eq!(x(&engine, "b"), 25.);
    assert_eq!(controller.info(&engine, b).unwrap().elapsed, ms(75));
}

#[test]
fn timed_cancel_restores_document_and_existing_undo_redo_after_repeated_seek() {
    let mut editor = SceneEditor::from_engine(engine());
    for value in [10., 20.] {
        editor
            .transaction(|scene| {
                scene.update_object(&"a".into(), |object| object.transform.tx = value)
            })
            .unwrap();
    }
    assert!(editor.undo());
    let token = editor.begin_playback().unwrap();
    let mut controller = AnimationController::new(&editor.engine);
    let id = start(
        &mut controller,
        &editor.engine,
        "a",
        AnimationTiming {
            delay_ms: 20,
            repeat_delay_ms: 10,
            iterations: IterationCount::Infinite,
            direction: PlaybackDirection::Alternate,
            speed: PlaybackRate::new(3, 2).unwrap(),
        },
    );
    for position in [70, 245, 140, 70] {
        controller
            .seek_editor(&mut editor, token, id, ms(position))
            .unwrap();
    }
    controller.tick_editor(&mut editor, token, ms(10)).unwrap();
    assert_eq!(x(&editor.engine, "a"), 68.5);
    controller.cancel_editor(&mut editor, token).unwrap();
    assert_eq!(x(&editor.engine, "a"), 10.);
    assert!(editor.redo());
    assert_eq!(x(&editor.engine, "a"), 20.);
    assert!(editor.undo());
    assert_eq!(x(&editor.engine, "a"), 10.);
    assert!(editor.undo());
    assert_eq!(x(&editor.engine, "a"), 0.);
    assert!(!editor.undo());
    assert!(
        controller
            .seek_editor(&mut editor, token, id, ms(70))
            .is_err()
    );
}

#[test]
fn finishing_infinite_playback_creates_one_undo_and_releases_property_ownership() {
    let mut editor = SceneEditor::from_engine(engine());
    editor
        .transaction(|scene| scene.update_object(&"a".into(), |object| object.transform.tx = 10.))
        .unwrap();
    let token = editor.begin_playback().unwrap();
    let mut controller = AnimationController::new(&editor.engine);
    let id = start(
        &mut controller,
        &editor.engine,
        "a",
        AnimationTiming {
            iterations: IterationCount::Infinite,
            direction: PlaybackDirection::Alternate,
            ..Default::default()
        },
    );
    for position in [250, 75, 1000, 50] {
        controller
            .seek_editor(&mut editor, token, id, ms(position))
            .unwrap();
    }
    controller.finish_editor(&mut editor, token).unwrap();
    assert_eq!(x(&editor.engine, "a"), 55.);
    assert_eq!(
        controller.info(&editor.engine, id).unwrap().status,
        AnimationStatus::Cancelled(CancelReason::User)
    );
    start(
        &mut controller,
        &editor.engine,
        "a",
        AnimationTiming::default(),
    );
    assert!(editor.undo());
    assert_eq!(x(&editor.engine, "a"), 10.);
    assert!(editor.undo());
    assert_eq!(x(&editor.engine, "a"), 0.);
    assert!(!editor.undo());
    assert!(editor.redo());
    assert!(editor.redo());
    assert_eq!(x(&editor.engine, "a"), 55.);
}

#[test]
fn rejected_frame_preserves_fractional_clock_carry_playback_and_prior_history() {
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
                    return Err("reject midpoint".into());
                }
                let shape = display::Ellipse::circle(radius);
                Ok((shape.bounds(), vec![shape.flatten(0.1)]))
            },
        ))
        .unwrap();
    let mut engine = SceneEngine::with_registry(engine().scene().clone(), registry).unwrap();
    engine
        .create("disc", "disc", json!({}), None, None)
        .unwrap();
    let mut editor = SceneEditor::from_engine(engine);
    let token = editor.begin_playback().unwrap();
    let mut controller = AnimationController::new(&editor.engine);
    let a = start(
        &mut controller,
        &editor.engine,
        "a",
        AnimationTiming::default(),
    );
    let disc = controller
        .animate_with_timing(
            &editor.engine,
            editor.engine.handle(&"disc".into()).unwrap(),
            AnimationSpec::new(100, vec![Tween::custom_number("radius", 20.)]),
            AnimationTiming {
                speed: PlaybackRate::new(1, 3).unwrap(),
                iterations: IterationCount::Finite(2),
                ..Default::default()
            },
        )
        .unwrap();
    controller.tick_editor(&mut editor, token, ns(1)).unwrap();
    let before = editor.scene().clone();
    let revision = editor.engine.revision();
    assert!(
        controller
            .tick_editor(&mut editor, token, ns(149_999_999))
            .is_err()
    );
    assert_eq!(editor.scene(), &before);
    assert_eq!(editor.engine.revision(), revision);
    assert_eq!(
        controller.info(&editor.engine, disc).unwrap().elapsed,
        Duration::ZERO
    );
    assert_eq!(controller.info(&editor.engine, a).unwrap().elapsed, ns(1));
    assert!(editor.is_playback_active(token));
    controller.tick_editor(&mut editor, token, ns(2)).unwrap();
    assert_eq!(
        controller.info(&editor.engine, disc).unwrap().elapsed,
        ns(1)
    );
    controller.cancel_editor(&mut editor, token).unwrap();
    assert!(!editor.undo());
}

#[test]
fn interrupted_timed_editor_playback_cannot_cancel_or_overwrite_the_external_edit() {
    let mut editor = SceneEditor::from_engine(engine());
    let token = editor.begin_playback().unwrap();
    let mut controller = AnimationController::new(&editor.engine);
    let id = start(
        &mut controller,
        &editor.engine,
        "a",
        AnimationTiming {
            iterations: IterationCount::Infinite,
            ..Default::default()
        },
    );
    controller.tick_editor(&mut editor, token, ms(150)).unwrap();
    editor
        .set_property(
            editor.engine.handle(&"b".into()).unwrap(),
            PropertyKey::Opacity,
            json!(0.2),
        )
        .unwrap();
    assert_eq!(
        controller
            .tick_editor(&mut editor, token, ms(10))
            .unwrap_err(),
        AnimationError::EditorSessionEnded
    );
    assert!(controller.cancel_editor(&mut editor, token).is_err());
    assert_eq!(x(&editor.engine, "a"), 50.);
    assert_eq!(
        controller.info(&editor.engine, id).unwrap().status,
        AnimationStatus::Cancelled(CancelReason::EditorInterrupted)
    );
    assert!(editor.undo());
    assert_eq!(x(&editor.engine, "a"), 50.);
    assert!(editor.undo());
    assert_eq!(x(&editor.engine, "a"), 0.);
}

#[test]
fn reverse_sequence_traverses_sparse_keyframes_with_each_segments_own_easing() {
    let mut engine = engine();
    let mut controller = AnimationController::new(&engine);
    let mut first = AnimationSpec::new(100, vec![Tween::translation(Point::new(100., 0.))]);
    first.easing = Easing::EaseIn;
    let mut second = AnimationSpec::new(200, vec![Tween::opacity(0.)]);
    second.easing = Easing::EaseOut;
    let sequence = AnimationSequence::new(vec![
        first,
        second,
        AnimationSpec::new(100, vec![Tween::translation(Point::new(200., 0.))]),
    ]);
    let id = controller
        .animate_sequence_with_timing(
            &engine,
            engine.handle(&"a".into()).unwrap(),
            sequence,
            AnimationTiming {
                direction: PlaybackDirection::Reverse,
                ..Default::default()
            },
        )
        .unwrap();
    for (position, expected_x, opacity) in [
        (0, 200., 0.),
        (50, 150., 0.),
        (100, 100., 0.),
        (200, 100., 0.875),
        (300, 100., 1.),
        (350, 87.5, 1.),
        (400, 0., 1.),
    ] {
        controller.seek(&mut engine, id, ms(position)).unwrap();
        assert_eq!(x(&engine, "a"), expected_x);
        assert_eq!(
            engine
                .object(engine.handle(&"a".into()).unwrap())
                .unwrap()
                .style
                .opacity,
            opacity
        );
    }
    assert_eq!(
        controller.info(&engine, id).unwrap().status,
        AnimationStatus::Completed
    );
}

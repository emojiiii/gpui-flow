use gpui_flow_animation::*;
use gpui_flow_editor::{PlaybackToken, SceneEditor};
use gpui_flow_engine::{
    CustomDefinition, DirtyFlags, ElementRegistry, PropertyDescriptor, PropertyKey, PropertySchema,
    SceneEngine,
};
use gpui_flow_scene::{DisplayObject, Point, Scene, display};
use serde_json::json;
use std::time::Duration;
fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}
fn editor() -> SceneEditor {
    let mut scene = Scene::new();
    for name in ["a", "b"] {
        scene
            .insert(
                DisplayObject::new(name, display::Rect::new(10., 10.)),
                None,
                None,
            )
            .unwrap();
    }
    SceneEditor::new(scene).unwrap()
}
fn edit(e: &mut SceneEditor, x: f32) {
    e.transaction(|s| s.update_object(&"a".into(), |o| o.transform.tx = x))
        .unwrap();
}
fn pose(e: &SceneEditor) -> (f32, f32) {
    (
        e.scene().get(&"a".into()).unwrap().object.transform.tx,
        e.scene().get(&"b".into()).unwrap().object.style.opacity,
    )
}
fn start(e: &mut SceneEditor) -> (TimelineController, PlaybackToken, TimelineId) {
    let token = e.begin_playback().unwrap();
    let mut c = TimelineController::new(&e.engine);
    let a = e.engine.handle(&"a".into()).unwrap();
    let b = e.engine.handle(&"b".into()).unwrap();
    let id = c
        .animate(
            &e.engine,
            TimelineSpec::serial(vec![
                TimelineChild::animate(
                    a,
                    AnimationSpec::new(100, vec![Tween::translation(Point::new(100., 0.))]),
                ),
                TimelineChild::group(TimelineSpec::parallel(vec![
                    TimelineChild::animate(
                        a,
                        AnimationSpec::new(100, vec![Tween::translation(Point::new(200., 0.))]),
                    ),
                    TimelineChild::animate(b, AnimationSpec::new(100, vec![Tween::opacity(0.)])),
                ])),
            ]),
        )
        .unwrap();
    (c, token, id)
}
#[test]
fn nested_parent_many_ticks_seeks_pause_reverse_and_finish_are_one_undo_step() {
    let mut e = editor();
    edit(&mut e, 10.);
    let (mut c, token, id) = start(&mut e);
    for _ in 0..15 {
        c.tick_editor(&mut e, token, ms(10)).unwrap();
    }
    assert_eq!(pose(&e), (150., 0.5));
    c.pause(&e.engine, id).unwrap();
    c.seek_editor(&mut e, token, id, ms(175)).unwrap();
    c.reverse(&e.engine, id).unwrap();
    c.resume(&e.engine, id).unwrap();
    c.tick_editor(&mut e, token, ms(25)).unwrap();
    assert_eq!(pose(&e), (150., 0.5));
    c.finish_editor(&mut e, token).unwrap();
    assert!(e.undo());
    assert_eq!(pose(&e), (10., 1.));
    assert!(e.undo());
    assert_eq!(pose(&e), (0., 1.));
    assert!(!e.undo());
    assert!(e.redo());
    assert_eq!(pose(&e), (10., 1.));
    assert!(e.redo());
    assert_eq!(pose(&e), (150., 0.5));
    assert!(!e.redo());
}
#[test]
fn cancel_restores_all_children_and_retains_old_undo_redo_selection_and_viewport() {
    let mut e = editor();
    edit(&mut e, 10.);
    edit(&mut e, 20.);
    assert!(e.undo());
    let (mut c, token, id) = start(&mut e);
    c.seek_editor(&mut e, token, id, ms(175)).unwrap();
    e.selection.select("b".into(), false);
    e.viewport.pan = Point::new(10., 20.);
    c.cancel_editor(&mut e, token).unwrap();
    assert_eq!(pose(&e), (10., 1.));
    assert!(e.selection.ids.contains(&"b".into()));
    assert_eq!(e.viewport.pan, Point::new(10., 20.));
    assert!(e.redo());
    assert_eq!(pose(&e), (20., 1.));
    assert!(!e.redo());
    assert!(e.undo());
    assert!(e.undo());
    assert_eq!(pose(&e), (0., 1.));
}
#[test]
fn target_and_unrelated_external_edits_end_token_before_any_child_can_write() {
    for (raw, owned) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut e = editor();
        edit(&mut e, 10.);
        let (mut c, token, id) = start(&mut e);
        c.tick_editor(&mut e, token, ms(150)).unwrap();
        let target = e.engine.handle(&"b".into()).unwrap();
        let (key, value) = if owned {
            (PropertyKey::Opacity, json!(0.2))
        } else {
            (
                PropertyKey::Transform,
                serde_json::to_value(gpui_flow_scene::Affine::translation(50., 0.)).unwrap(),
            )
        };
        if raw {
            e.engine.set_property(target, key, value).unwrap();
        } else {
            e.set_property(target, key, value).unwrap();
        }
        let before = e.scene().clone();
        assert_eq!(
            c.seek_editor(&mut e, token, id, ms(0)).unwrap_err(),
            AnimationError::EditorSessionEnded
        );
        assert_eq!(
            c.cancel_editor(&mut e, token).unwrap_err(),
            AnimationError::EditorSessionEnded
        );
        assert_eq!(e.scene(), &before);
        assert!(e.undo());
        assert_eq!(pose(&e), (150., 0.5));
        assert!(e.undo());
        assert_eq!(pose(&e), (10., 1.));
        assert!(e.undo());
        assert_eq!(pose(&e), (0., 1.));
    }
}
#[test]
fn destruction_reset_and_stale_tokens_never_restore_old_children() {
    for reset in [false, true] {
        let mut e = editor();
        edit(&mut e, 10.);
        let (mut c, token, _) = start(&mut e);
        c.tick_editor(&mut e, token, ms(150)).unwrap();
        if reset {
            e.engine.reset_scene(e.scene().clone()).unwrap();
        } else {
            e.engine
                .destroy(e.engine.handle(&"b".into()).unwrap())
                .unwrap();
        }
        let before = e.scene().clone();
        assert!(c.cancel_editor(&mut e, token).is_err());
        assert_eq!(e.scene(), &before);
        assert!(c.tick_editor(&mut e, token, ms(10)).is_err());
        assert_eq!(e.scene(), &before);
        if reset {
            assert!(!e.undo());
        } else {
            assert!(e.undo());
            assert_eq!(pose(&e), (150., 0.5));
        }
    }
}
#[test]
fn undo_while_parent_is_active_ends_all_child_ownership() {
    let mut e = editor();
    edit(&mut e, 10.);
    let (mut c, token, _) = start(&mut e);
    c.tick_editor(&mut e, token, ms(150)).unwrap();
    assert!(e.undo());
    assert_eq!(pose(&e), (10., 1.));
    assert!(c.tick_editor(&mut e, token, ms(50)).is_err());
    assert_eq!(pose(&e), (10., 1.));
    assert!(e.undo());
    assert_eq!(pose(&e), (0., 1.));
    assert!(e.redo());
    assert!(e.redo());
    assert_eq!(pose(&e), (150., 0.5));
}
#[test]
fn wrong_engine_and_finished_token_leave_valid_session_untouched() {
    let mut e = editor();
    let (mut c, token, id) = start(&mut e);
    let mut other = editor();
    let foreign = other.begin_playback().unwrap();
    assert_eq!(
        c.tick_editor(&mut other, foreign, ms(1)).unwrap_err(),
        AnimationError::WrongEngine
    );
    assert!(e.is_playback_active(token));
    c.tick_editor(&mut e, token, ms(150)).unwrap();
    c.finish_editor(&mut e, token).unwrap();
    let before = e.scene().clone();
    assert_eq!(
        c.seek_editor(&mut e, token, id, ms(0)).unwrap_err(),
        AnimationError::EditorSessionEnded
    );
    assert_eq!(e.scene(), &before);
}
#[test]
fn parent_ending_restore_and_zero_frame_do_not_create_empty_undo_entries() {
    for finish_at_end in [false, true] {
        let mut e = editor();
        edit(&mut e, 10.);
        let token = e.begin_playback().unwrap();
        let mut c = TimelineController::new(&e.engine);
        c.animate(
            &e.engine,
            TimelineSpec::serial(vec![TimelineChild::animate(
                e.engine.handle(&"a".into()).unwrap(),
                AnimationSpec::new(100, vec![Tween::translation(Point::new(100., 0.))]),
            )])
            .with_ending_style(EndingStyle::Restore),
        )
        .unwrap();
        c.tick_editor(
            &mut e,
            token,
            if finish_at_end {
                ms(50)
            } else {
                Duration::ZERO
            },
        )
        .unwrap();
        if finish_at_end {
            c.tick_editor(&mut e, token, ms(50)).unwrap();
        }
        c.finish_editor(&mut e, token).unwrap();
        assert_eq!(pose(&e), (10., 1.));
        assert!(e.undo());
        assert_eq!(pose(&e), (0., 1.));
        assert!(!e.undo());
    }
}
#[test]
fn late_invalid_child_frame_rolls_back_parent_fraction_and_preserves_token_and_redo() {
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
    let base = editor();
    let mut engine = SceneEngine::with_registry(base.scene().clone(), registry).unwrap();
    engine
        .create("disc", "disc", json!({}), None, None)
        .unwrap();
    let mut e = SceneEditor::from_engine(engine);
    edit(&mut e, 10.);
    edit(&mut e, 20.);
    assert!(e.undo());
    let token = e.begin_playback().unwrap();
    let mut c = TimelineController::new(&e.engine);
    let id = c
        .animate(
            &e.engine,
            TimelineSpec::parallel(vec![
                TimelineChild::animate(
                    e.engine.handle(&"a".into()).unwrap(),
                    AnimationSpec::new(100, vec![Tween::translation(Point::new(100., 0.))]),
                ),
                TimelineChild::animate(
                    e.engine.handle(&"disc".into()).unwrap(),
                    AnimationSpec::new(100, vec![Tween::custom_number("radius", 20.)]),
                ),
            ])
            .with_timing(AnimationTiming {
                speed: PlaybackRate::new(1, 3).unwrap(),
                ..Default::default()
            }),
        )
        .unwrap();
    c.tick_editor(&mut e, token, Duration::from_nanos(1))
        .unwrap();
    let before = e.scene().clone();
    let revision = e.engine.revision();
    assert!(c.tick_editor(&mut e, token, ms(150)).is_err());
    assert_eq!(e.scene(), &before);
    assert_eq!(e.engine.revision(), revision);
    assert!(e.is_playback_active(token));
    assert_eq!(c.info(&e.engine, id).unwrap().elapsed, Duration::ZERO);
    c.tick_editor(&mut e, token, Duration::from_nanos(2))
        .unwrap();
    assert_eq!(
        c.info(&e.engine, id).unwrap().elapsed,
        Duration::from_nanos(1)
    );
    c.cancel_editor(&mut e, token).unwrap();
    assert_eq!(pose(&e), (10., 1.));
    assert!(e.redo());
    assert_eq!(pose(&e), (20., 1.));
}

#[test]
fn foreign_token_rejects_without_changing_either_editor_baseline() {
    let mut e = editor();
    edit(&mut e, 10.);
    let (mut c, token, _) = start(&mut e);
    c.tick_editor(&mut e, token, ms(150)).unwrap();
    let mut other = editor();
    let foreign = other.begin_playback().unwrap();
    let before = e.scene().clone();
    assert_eq!(
        c.cancel_editor(&mut e, foreign).unwrap_err(),
        AnimationError::EditorSessionEnded
    );
    assert_eq!(e.scene(), &before);
    assert!(e.is_playback_active(token));
    assert!(other.is_playback_active(foreign));
    assert!(e.cancel_playback(token));
    assert_eq!(pose(&e), (10., 1.));
    assert!(e.undo());
    assert_eq!(pose(&e), (0., 1.));
}

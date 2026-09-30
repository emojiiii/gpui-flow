use gpui_flow_animation::*;
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{DisplayObject, Point, Scene, display};
use serde_json::json;
use std::time::Duration;
fn editor() -> SceneEditor {
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
    SceneEditor::new(scene).unwrap()
}
fn edit(e: &mut SceneEditor, x: f32) {
    e.transaction(|scene| scene.update_object(&"a".into(), |o| o.transform.tx = x))
        .unwrap();
}
fn x(e: &SceneEditor) -> f32 {
    e.scene().get(&"a".into()).unwrap().object.transform.tx
}
fn start(
    e: &mut SceneEditor,
) -> (
    AnimationController,
    gpui_flow_editor::PlaybackToken,
    AnimationId,
) {
    let token = e.begin_playback().unwrap();
    let mut c = AnimationController::new(&e.engine);
    let id = c
        .animate(
            &e.engine,
            e.engine.handle(&"a".into()).unwrap(),
            AnimationSpec::new(1000, vec![Tween::translation(Point::new(100., 0.))]),
        )
        .unwrap();
    (c, token, id)
}
#[test]
fn edit_animate_finish_preserves_previous_undo_and_adds_only_one_playback_step() {
    let mut e = editor();
    edit(&mut e, 10.);
    let (mut c, token, _) = start(&mut e);
    for _ in 0..10 {
        c.tick_editor(&mut e, token, Duration::from_millis(100))
            .unwrap();
    }
    c.finish_editor(&mut e, token).unwrap();
    assert_eq!(x(&e), 100.);
    assert!(e.undo());
    assert_eq!(x(&e), 10.);
    assert!(e.undo());
    assert_eq!(x(&e), 0.);
    assert!(!e.undo());
    assert!(e.redo());
    assert_eq!(x(&e), 10.);
    assert!(e.redo());
    assert_eq!(x(&e), 100.);
    assert!(!e.redo());
}
#[test]
fn edit_animate_cancel_restores_baseline_without_erasing_previous_undo() {
    let mut e = editor();
    edit(&mut e, 10.);
    let (mut c, token, _) = start(&mut e);
    c.tick_editor(&mut e, token, Duration::from_millis(500))
        .unwrap();
    assert_eq!(x(&e), 55.);
    c.cancel_editor(&mut e, token).unwrap();
    assert_eq!(x(&e), 10.);
    assert!(e.undo());
    assert_eq!(x(&e), 0.);
    assert!(!e.undo());
    assert!(e.redo());
    assert_eq!(x(&e), 10.);
}
#[test]
fn cancelled_playback_preserves_an_existing_redo_branch() {
    let mut e = editor();
    edit(&mut e, 10.);
    edit(&mut e, 20.);
    assert!(e.undo());
    let (mut c, token, _) = start(&mut e);
    c.tick_editor(&mut e, token, Duration::from_millis(500))
        .unwrap();
    c.cancel_editor(&mut e, token).unwrap();
    assert_eq!(x(&e), 10.);
    assert!(e.redo());
    assert_eq!(x(&e), 20.);
    assert!(!e.redo());
}
#[test]
fn user_edit_interrupts_playback_without_merging_or_overwriting_the_new_edit() {
    let mut e = editor();
    edit(&mut e, 10.);
    let (mut c, token, _) = start(&mut e);
    c.tick_editor(&mut e, token, Duration::from_millis(500))
        .unwrap();
    e.set_property(
        e.engine.handle(&"b".into()).unwrap(),
        PropertyKey::Opacity,
        json!(0.2),
    )
    .unwrap();
    assert_eq!(
        c.tick_editor(&mut e, token, Duration::from_millis(100))
            .unwrap_err(),
        AnimationError::EditorSessionEnded
    );
    assert!(c.cancel_editor(&mut e, token).is_err());
    assert_eq!(
        e.scene().get(&"b".into()).unwrap().object.style.opacity,
        0.2
    );
    assert!(e.undo());
    assert_eq!(e.scene().get(&"b".into()).unwrap().object.style.opacity, 1.);
    assert_eq!(x(&e), 55.);
    assert!(e.undo());
    assert_eq!(x(&e), 10.);
    assert!(e.undo());
    assert_eq!(x(&e), 0.);
}
#[test]
fn direct_engine_edit_during_playback_records_frame_and_external_edit_separately() {
    let mut e = editor();
    edit(&mut e, 10.);
    let (mut c, token, _) = start(&mut e);
    c.tick_editor(&mut e, token, Duration::from_millis(500))
        .unwrap();
    e.engine
        .set_property(
            e.engine.handle(&"b".into()).unwrap(),
            PropertyKey::Opacity,
            json!(0.2),
        )
        .unwrap();
    assert!(c.cancel_editor(&mut e, token).is_err());
    assert_eq!(
        e.scene().get(&"b".into()).unwrap().object.style.opacity,
        0.2
    );
    assert!(e.undo());
    assert_eq!(e.scene().get(&"b".into()).unwrap().object.style.opacity, 1.);
    assert_eq!(x(&e), 55.);
    assert!(e.undo());
    assert_eq!(x(&e), 10.);
    assert!(e.undo());
    assert_eq!(x(&e), 0.);
}
#[test]
fn direct_target_destroy_is_never_undone_by_stale_cancel_but_explicit_undo_can_restore_document() {
    let mut e = editor();
    edit(&mut e, 10.);
    let (mut c, token, _) = start(&mut e);
    c.tick_editor(&mut e, token, Duration::from_millis(500))
        .unwrap();
    let old = e.engine.handle(&"a".into()).unwrap();
    e.engine.destroy(old).unwrap();
    assert!(c.cancel_editor(&mut e, token).is_err());
    assert!(e.scene().get(&"a".into()).is_none());
    assert!(e.undo());
    assert_eq!(x(&e), 55.);
    assert_ne!(e.engine.handle(&"a".into()).unwrap(), old);
    assert!(e.undo());
    assert_eq!(x(&e), 10.);
    assert!(e.undo());
    assert_eq!(x(&e), 0.);
}
#[test]
fn new_document_reset_remains_history_boundary_even_after_another_raw_write() {
    let mut e = editor();
    edit(&mut e, 10.);
    let (mut c, token, _) = start(&mut e);
    c.tick_editor(&mut e, token, Duration::from_millis(500))
        .unwrap();
    let epoch = e.engine.document_epoch();
    e.engine.reset_scene(e.scene().clone()).unwrap();
    assert_eq!(e.engine.document_epoch(), epoch + 1);
    e.engine
        .set_property(
            e.engine.handle(&"b".into()).unwrap(),
            PropertyKey::Opacity,
            json!(0.2),
        )
        .unwrap();
    assert!(c.tick_editor(&mut e, token, Duration::ZERO).is_err());
    assert!(!e.undo());
    assert_eq!(
        e.scene().get(&"b".into()).unwrap().object.style.opacity,
        0.2
    );
}
#[test]
fn undo_during_playback_ends_ownership_and_restores_one_baseline_step() {
    let mut e = editor();
    edit(&mut e, 10.);
    let (mut c, token, _) = start(&mut e);
    c.tick_editor(&mut e, token, Duration::from_millis(500))
        .unwrap();
    assert!(e.undo());
    assert_eq!(x(&e), 10.);
    assert!(
        c.tick_editor(&mut e, token, Duration::from_millis(100))
            .is_err()
    );
    assert_eq!(x(&e), 10.);
    assert!(e.undo());
    assert_eq!(x(&e), 0.);
    assert!(e.redo());
    assert_eq!(x(&e), 10.);
    assert!(e.redo());
    assert_eq!(x(&e), 55.);
}
#[test]
fn session_rejects_nested_gestures_foreign_tokens_and_old_tokens_without_touching_baseline() {
    let mut e = editor();
    assert!(e.begin_edit());
    assert!(e.begin_playback().is_err());
    e.cancel_edit();
    let first = e.begin_playback().unwrap();
    assert!(e.begin_playback().is_err());
    assert!(e.finish_playback(first));
    let second = e.begin_playback().unwrap();
    assert!(!e.cancel_playback(first));
    assert!(e.is_playback_active(second));
    let mut other = editor();
    let foreign = other.begin_playback().unwrap();
    assert!(!e.finish_playback(foreign));
    assert!(e.cancel_playback(second));
}
#[test]
fn invalid_frame_keeps_session_history_and_redo_intact() {
    let mut e = editor();
    edit(&mut e, 10.);
    let token = e.begin_playback().unwrap();
    let h = e.engine.handle(&"a".into()).unwrap();
    assert!(
        e.apply_playback_properties(token, [(h, PropertyKey::Opacity, json!(2.))])
            .is_err()
    );
    assert!(e.is_playback_active(token));
    assert_eq!(x(&e), 10.);
    assert!(e.cancel_playback(token));
    assert!(e.undo());
    assert_eq!(x(&e), 0.);
}
#[test]
fn zero_delta_finish_does_not_create_an_extra_undo_step() {
    let mut e = editor();
    edit(&mut e, 10.);
    let (mut c, token, _) = start(&mut e);
    c.tick_editor(&mut e, token, Duration::ZERO).unwrap();
    c.finish_editor(&mut e, token).unwrap();
    assert!(e.undo());
    assert_eq!(x(&e), 0.);
    assert!(!e.undo());
}
#[test]
fn seek_and_pause_share_the_same_single_playback_history_transaction() {
    let mut e = editor();
    edit(&mut e, 10.);
    let (mut c, token, id) = start(&mut e);
    c.pause(&e.engine, id).unwrap();
    c.seek_editor(&mut e, token, id, Duration::from_millis(500))
        .unwrap();
    c.resume(&e.engine, id).unwrap();
    c.tick_editor(&mut e, token, Duration::from_millis(500))
        .unwrap();
    c.finish_editor(&mut e, token).unwrap();
    assert_eq!(x(&e), 100.);
    assert!(e.undo());
    assert_eq!(x(&e), 10.);
    assert!(e.undo());
    assert_eq!(x(&e), 0.);
}
#[test]
fn cancel_preserves_host_selection_and_viewport_while_restoring_only_document() {
    let mut e = editor();
    edit(&mut e, 10.);
    let (mut c, token, _) = start(&mut e);
    c.tick_editor(&mut e, token, Duration::from_millis(500))
        .unwrap();
    e.selection.select("b".into(), false);
    e.viewport.pan = Point::new(20., 30.);
    c.cancel_editor(&mut e, token).unwrap();
    assert_eq!(x(&e), 10.);
    assert!(e.selection.ids.contains(&"b".into()));
    assert_eq!(e.viewport.pan, Point::new(20., 30.));
}
#[test]
fn dropped_controller_does_not_discard_editor_baseline_or_prior_history() {
    let mut e = editor();
    edit(&mut e, 10.);
    let (mut c, token, _) = start(&mut e);
    c.tick_editor(&mut e, token, Duration::from_millis(500))
        .unwrap();
    drop(c);
    assert!(e.cancel_playback(token));
    assert_eq!(x(&e), 10.);
    assert!(e.undo());
    assert_eq!(x(&e), 0.);
}

#[test]
fn starting_a_user_drag_commits_playback_then_keeps_drag_as_its_own_undo_step() {
    let mut e = editor();
    edit(&mut e, 10.);
    e.selection.select("a".into(), false);
    let (mut c, token, _) = start(&mut e);
    c.tick_editor(&mut e, token, Duration::from_millis(500))
        .unwrap();
    assert!(e.begin_edit());
    e.translate_selection(Point::new(5., 0.)).unwrap();
    e.commit_edit();
    assert!(
        c.tick_editor(&mut e, token, Duration::from_millis(100))
            .is_err()
    );
    assert_eq!(x(&e), 60.);
    assert!(e.undo());
    assert_eq!(x(&e), 55.);
    assert!(e.undo());
    assert_eq!(x(&e), 10.);
    assert!(e.undo());
    assert_eq!(x(&e), 0.);
}

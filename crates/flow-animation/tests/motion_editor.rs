use gpui_flow_animation::*;
use gpui_flow_editor::{PlaybackToken, SceneEditor};
use gpui_flow_engine::{
    ChangeOrigin, EngineError, LayoutProvider, PathPlacement, PropertyKey, SceneEngine,
};
use gpui_flow_scene::{
    Affine, DisplayObject, Point, Scene,
    display::{Path, Rect},
};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}
fn editor() -> SceneEditor {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("path", Path::from_svg("M0 0L100 0").unwrap()),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("target", Rect::new(10., 20.)),
        None,
        None,
    )
    .unwrap();
    s.insert(DisplayObject::new("other", Rect::new(10., 20.)), None, None)
        .unwrap();
    SceneEditor::new(s).unwrap()
}
fn x(e: &SceneEditor) -> f32 {
    e.scene().get(&"target".into()).unwrap().object.transform.tx
}
fn edit(e: &mut SceneEditor, x: f32) {
    e.transaction(|s| s.update_object(&"target".into(), |o| o.transform.tx = x))
        .unwrap();
}
fn start(e: &mut SceneEditor) -> (MotionPathAnimation, PlaybackToken) {
    let token = e.begin_playback().unwrap();
    let t = e.engine.handle(&"target".into()).unwrap();
    let p = e.engine.handle(&"path".into()).unwrap();
    let a = MotionPathAnimation::new_editor(e, token, t, p, MotionPathSpec::new(1000, 0., 100.))
        .unwrap();
    (a, token)
}
#[test]
fn completion_is_one_undo_after_many_ticks_and_seeks_with_animation_origin() {
    let mut e = editor();
    edit(&mut e, 10.);
    let (mut a, token) = start(&mut e);
    for _ in 0..4 {
        assert_eq!(
            a.tick_editor(&mut e, token, ms(100))
                .unwrap()
                .changes
                .origin,
            ChangeOrigin::Animation
        );
    }
    a.pause(&e.engine).unwrap();
    a.seek_editor(&mut e, token, ms(900)).unwrap();
    a.seek_editor(&mut e, token, ms(500)).unwrap();
    a.resume(&e.engine).unwrap();
    a.tick_editor(&mut e, token, ms(500)).unwrap();
    assert_eq!(x(&e), 100.);
    a.finish_editor(&mut e, token).unwrap();
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
fn cancel_restores_full_affine_and_preserves_the_existing_redo_branch() {
    let mut e = editor();
    e.transaction(|s| {
        s.update_object(&"target".into(), |o| {
            o.transform =
                Affine::translation(10., 20.) * Affine::rotation(45.) * Affine::scale(-2., 3.)
        })
    })
    .unwrap();
    let original = e.scene().clone();
    edit(&mut e, 50.);
    assert!(e.undo());
    let (mut a, token) = start(&mut e);
    a.tick_editor(&mut e, token, ms(600)).unwrap();
    a.seek_editor(&mut e, token, ms(900)).unwrap();
    a.cancel_editor(&mut e, token).unwrap();
    assert_eq!(e.scene(), &original);
    assert!(e.redo());
    assert_eq!(x(&e), 50.);
    assert!(!e.redo());
    assert!(a.seek_editor(&mut e, token, ms(100)).is_err());
}
#[test]
fn external_editor_and_direct_engine_edits_interrupt_and_keep_separate_history() {
    for direct in [false, true] {
        let mut e = editor();
        edit(&mut e, 10.);
        let (mut a, token) = start(&mut e);
        a.tick_editor(&mut e, token, ms(500)).unwrap();
        let other = e.engine.handle(&"other".into()).unwrap();
        if direct {
            e.engine
                .set_property(other, PropertyKey::Opacity, json!(0.2))
                .unwrap();
        } else {
            e.set_property(other, PropertyKey::Opacity, json!(0.2))
                .unwrap();
        }
        let before = e.scene().clone();
        assert_eq!(
            a.tick_editor(&mut e, token, ms(100)).unwrap_err(),
            AnimationError::EditorSessionEnded
        );
        assert!(a.cancel_editor(&mut e, token).is_err());
        assert!(a.finish_editor(&mut e, token).is_err());
        assert_eq!(e.scene(), &before);
        assert!(e.undo());
        assert_eq!(
            e.scene().get(&"other".into()).unwrap().object.style.opacity,
            1.
        );
        assert_eq!(x(&e), 50.);
        assert!(e.undo());
        assert_eq!(x(&e), 10.);
        assert!(e.undo());
        assert_eq!(x(&e), 0.);
    }
}
#[test]
fn original_token_and_engine_route_are_bound_even_if_old_token_was_not_polled() {
    let mut e = editor();
    let (mut a, old) = start(&mut e);
    a.tick_editor(&mut e, old, ms(200)).unwrap();
    assert!(a.tick(&mut e.engine, ms(100)).is_err());
    assert!(a.seek(&mut e.engine, ms(400)).is_err());
    assert!(a.cancel(&e.engine).is_err());
    assert!(e.finish_playback(old));
    let new = e.begin_playback().unwrap();
    let before = e.scene().clone();
    assert_eq!(
        a.tick_editor(&mut e, new, ms(100)).unwrap_err(),
        AnimationError::EditorSessionEnded
    );
    assert_eq!(
        a.seek_editor(&mut e, new, ms(100)).unwrap_err(),
        AnimationError::EditorSessionEnded
    );
    assert!(a.cancel_editor(&mut e, new).is_err());
    assert!(e.is_playback_active(new));
    assert_eq!(e.scene(), &before);
    e.cancel_playback(new);
}
#[test]
fn engine_binding_cannot_borrow_an_editor_token_and_wrong_engine_cannot_write() {
    let mut e = editor();
    let t = e.engine.handle(&"target".into()).unwrap();
    let p = e.engine.handle(&"path".into()).unwrap();
    let mut a =
        MotionPathAnimation::new(&e.engine, t, p, MotionPathSpec::new(1000, 0., 100.)).unwrap();
    let token = e.begin_playback().unwrap();
    assert!(a.tick_editor(&mut e, token, ms(100)).is_err());
    assert!(e.is_playback_active(token));
    assert_eq!(x(&e), 0.);
    let mut other = editor();
    assert_eq!(
        a.tick(&mut other.engine, ms(100)).unwrap_err(),
        AnimationError::WrongEngine
    );
}
#[test]
fn source_edit_and_document_reset_revoke_editor_binding() {
    for reset in [false, true] {
        let mut e = editor();
        let (mut a, token) = start(&mut e);
        a.tick_editor(&mut e, token, ms(500)).unwrap();
        if reset {
            e.load_scene(e.scene().clone()).unwrap();
        } else {
            let p = e.engine.handle(&"path".into()).unwrap();
            e.engine
                .set_path_data(p, Path::from_svg("M0 0L0 100").unwrap())
                .unwrap();
        }
        let before = e.scene().clone();
        assert!(a.seek_editor(&mut e, token, ms(1000)).is_err());
        assert!(a.cancel_editor(&mut e, token).is_err());
        assert_eq!(e.scene(), &before);
    }
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
fn layout_rejection_leaves_clock_token_and_history_available_for_retry() {
    for id in ["target", "path"] {
        let mut e = editor();
        edit(&mut e, 10.);
        let enabled = Arc::new(AtomicBool::new(false));
        e.engine
            .install_layout_provider(Override {
                enabled: enabled.clone(),
                id,
            })
            .unwrap();
        let (mut a, token) = start(&mut e);
        let revision = e.engine.revision();
        let before = e.scene().clone();
        enabled.store(true, Ordering::SeqCst);
        assert!(a.tick_editor(&mut e, token, ms(500)).is_err());
        assert_eq!(e.scene(), &before);
        assert_eq!(e.engine.revision(), revision);
        assert_eq!(a.info(&e.engine).unwrap().elapsed, Duration::ZERO);
        assert!(e.is_playback_active(token));
        enabled.store(false, Ordering::SeqCst);
        a.tick_editor(&mut e, token, ms(500)).unwrap();
        assert_eq!(x(&e), 50.);
        a.cancel_editor(&mut e, token).unwrap();
        assert_eq!(e.scene(), &before);
        assert!(e.undo());
        assert_eq!(x(&e), 0.);
        assert!(!e.undo());
    }
}
#[test]
fn delayed_seek_restores_original_pose_with_post_layout_validation_and_origin() {
    let mut e = editor();
    edit(&mut e, 10.);
    let enabled = Arc::new(AtomicBool::new(false));
    e.engine
        .install_layout_provider(Override {
            enabled: enabled.clone(),
            id: "target",
        })
        .unwrap();
    let token = e.begin_playback().unwrap();
    let mut spec = MotionPathSpec::new(1000, 0., 100.);
    spec.timing.delay_ms = 100;
    let t = e.engine.handle(&"target".into()).unwrap();
    let p = e.engine.handle(&"path".into()).unwrap();
    let mut a = MotionPathAnimation::new_editor(&mut e, token, t, p, spec).unwrap();
    a.seek_editor(&mut e, token, ms(600)).unwrap();
    assert_eq!(x(&e), 50.);
    let before = e.scene().clone();
    let revision = e.engine.revision();
    enabled.store(true, Ordering::SeqCst);
    assert!(a.seek_editor(&mut e, token, ms(50)).is_err());
    assert_eq!(e.scene(), &before);
    assert_eq!(e.engine.revision(), revision);
    assert_eq!(a.info(&e.engine).unwrap().elapsed, ms(600));
    enabled.store(false, Ordering::SeqCst);
    let changes = a.seek_editor(&mut e, token, ms(50)).unwrap();
    assert_eq!(changes.origin, ChangeOrigin::Animation);
    assert_eq!(x(&e), 10.);
    assert_eq!(a.info(&e.engine).unwrap().distance, None);
    a.cancel_editor(&mut e, token).unwrap();
    assert!(e.undo());
    assert_eq!(x(&e), 0.);
}
#[test]
fn bridges_reject_foreign_or_stale_tokens_without_affecting_active_playback() {
    let mut e = editor();
    let mut other = editor();
    let token = e.begin_playback().unwrap();
    let foreign = other.begin_playback().unwrap();
    let t = e.engine.handle(&"target".into()).unwrap();
    let p = e.engine.handle(&"path".into()).unwrap();
    let before = e.scene().clone();
    assert!(
        e.apply_playback_path_placement(foreign, t, p, PathPlacement::new(10.))
            .is_err()
    );
    assert!(
        e.apply_playback_transaction_validated(
            foreign,
            |s| s.update_object(&"target".into(), |o| o.transform.tx = 10.),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert!(e.is_playback_active(token));
    assert_eq!(
        e.apply_playback_path_placement(token, t, p, PathPlacement::new(10.))
            .unwrap()
            .origin,
        ChangeOrigin::Animation
    );
    e.cancel_playback(token);
    assert!(
        e.apply_playback_path_placement(token, t, p, PathPlacement::new(20.))
            .is_err()
    );
    assert_eq!(x(&e), 0.);
}
#[test]
fn ordinary_one_shot_placement_retains_edit_origin_and_history() {
    let mut e = editor();
    let t = e.engine.handle(&"target".into()).unwrap();
    let p = e.engine.handle(&"path".into()).unwrap();
    assert_eq!(
        e.engine
            .place_on_path(t, p, PathPlacement::new(20.))
            .unwrap()
            .origin,
        ChangeOrigin::Edit
    );
    e.place_on_path(
        t,
        p,
        PathPlacement {
            anchor: Point::new(5., 0.),
            ..PathPlacement::new(50.)
        },
    )
    .unwrap();
    assert_eq!(x(&e), 45.);
    assert!(e.undo());
    assert_eq!(x(&e), 20.);
}

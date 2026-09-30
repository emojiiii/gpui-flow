//! Ratio allocation shares the existing atomic engine/editor derivation path.
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ChangeOrigin, DirtyFlags, PropertyKey, SceneEngine};
use gpui_flow_layout::*;
use gpui_flow_scene::{
    Affine, DisplayObject, EventKind, HitTestOptions, ListenerOptions, Point, Scene, SceneEvent,
    Size, display,
};
use serde_json::json;
use std::{cell::Cell, rc::Rc};

fn fixture() -> (Scene, FlowLayout) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Frame::new(100., 100.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("a", display::Rect::new(20., 10.)),
            Some("root".into()),
            None,
        )
        .unwrap();
    let mut layout = FlowLayout::default();
    layout
        .containers
        .insert("root".into(), FlowStyle::default());
    layout.items.insert(
        "a".into(),
        FlowItem {
            lock_ratio: true,
            grow: 1.,
            ..Default::default()
        },
    );
    (scene, layout)
}
fn setup() -> (SceneEditor, FlowLayout) {
    let (mut scene, layout) = fixture();
    layout.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    layout.install_reactive(&mut editor.engine).unwrap();
    (editor, layout)
}
fn size(scene: &Scene, id: &str) -> Size {
    scene
        .get(&id.into())
        .unwrap()
        .object
        .shape
        .local_bounds()
        .unwrap()
        .size
}
fn resize(editor: &mut SceneEditor, width: f32) -> Result<(), gpui_flow_scene::SceneError> {
    editor.transaction(|scene| {
        scene.update_object(&"root".into(), |o| {
            o.shape = display::Frame::new(width, 200.).into()
        })
    })
}

#[test]
fn explicit_layout_updates_engine_bounds_hit_damage_and_keeps_listener_identity() {
    let (scene, layout) = fixture();
    let mut engine = SceneEngine::new(scene).unwrap();
    let handle = engine.handle(&"a".into()).unwrap();
    let clicks = Rc::new(Cell::new(0));
    let observed = clicks.clone();
    engine
        .on(
            handle,
            EventKind::Click,
            ListenerOptions::default(),
            move |_| observed.set(observed.get() + 1),
        )
        .unwrap();
    let (_, changes) = layout.apply_to_engine(&mut engine).unwrap();
    assert_eq!(changes.origin, ChangeOrigin::Layout);
    assert!(changes.dirty[&"a".into()].contains(DirtyFlags::GEOMETRY));
    assert!(!changes.damage.is_empty());
    assert_eq!(
        engine.world_bounds(&"a".into()).unwrap().unwrap().size,
        Size::new(100., 50.)
    );
    assert_eq!(
        engine
            .hit_test(Point::new(90., 40.), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id,
        "a".into()
    );
    assert_eq!(engine.handle(&"a".into()), Some(handle));
    engine
        .dispatch(SceneEvent::new(EventKind::Click, "a"))
        .unwrap();
    assert_eq!(clicks.get(), 1);
    assert!(!layout.apply_to_engine(&mut engine).unwrap().1.changed());
}

#[test]
fn reactive_parent_resize_is_one_notification_and_one_exact_undo_redo() {
    let (mut editor, _) = setup();
    let before = editor.scene().clone();
    let handle = editor.engine.handle(&"a".into());
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    editor
        .engine
        .on_change(None, move |_| observed.set(observed.get() + 1))
        .unwrap();
    resize(&mut editor, 160.).unwrap();
    let after = editor.scene().clone();
    assert_eq!(size(editor.scene(), "a"), Size::new(160., 80.));
    assert_eq!(calls.get(), 1);
    assert_eq!(editor.engine.handle(&"a".into()), handle);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(!editor.undo());
    assert!(editor.redo());
    assert_eq!(editor.scene(), &after);
}

#[test]
fn reactive_child_shape_edit_uses_new_input_ratio_without_persisting_a_hidden_ratio() {
    let (mut editor, layout) = setup();
    let before = editor.scene().clone();
    editor
        .transaction(|scene| {
            scene.update_object(&"a".into(), |o| {
                o.shape = display::Rect::new(40., 40.).into()
            })
        })
        .unwrap();
    assert_eq!(size(editor.scene(), "a"), Size::new(100., 100.));
    assert!(
        layout
            .arranged(editor.scene())
            .unwrap()
            .1
            .changed
            .is_empty()
    );
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(editor.redo());
    assert_eq!(size(editor.scene(), "a"), Size::new(100., 100.));
}

#[test]
fn reactive_transform_edit_reflows_both_sizes_without_overwriting_the_linear_part() {
    let (mut editor, _) = setup();
    editor
        .transaction(|scene| {
            scene.update_object(&"a".into(), |o| o.transform = Affine::scale(2., 3.))
        })
        .unwrap();
    assert_eq!(size(editor.scene(), "a"), Size::new(50., 25.));
    assert_eq!(
        editor.scene().get(&"a".into()).unwrap().object.transform,
        Affine::scale(2., 3.)
    );
}

#[test]
fn failed_derived_ratio_preserves_scene_revision_notifications_and_redo() {
    let (mut editor, _) = setup();
    resize(&mut editor, 160.).unwrap();
    let after = editor.scene().clone();
    assert!(editor.undo());
    let before = editor.scene().clone();
    let revision = editor.engine.revision();
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    editor
        .engine
        .on_change(None, move |_| observed.set(observed.get() + 1))
        .unwrap();
    assert!(
        editor
            .transaction(
                |scene| scene.update_object(&"a".into(), |o| o.transform = Affine::rotation(30.))
            )
            .is_err()
    );
    assert_eq!(editor.scene(), &before);
    assert_eq!(editor.engine.revision(), revision);
    assert_eq!(calls.get(), 0);
    assert!(editor.redo());
    assert_eq!(editor.scene(), &after);
}

#[test]
fn explicit_wrapped_ratio_error_preserves_active_playback_and_prior_redo() {
    let (mut editor, mut layout) = setup();
    let before = editor.scene().clone();
    resize(&mut editor, 140.).unwrap();
    let after = editor.scene().clone();
    assert!(editor.undo());
    let token = editor.begin_playback().unwrap();
    let handle = editor.engine.handle(&"a".into()).unwrap();
    editor
        .apply_playback_properties(token, [(handle, PropertyKey::Opacity, json!(0.5))])
        .unwrap();
    let frame = editor.scene().clone();
    let revision = editor.engine.revision();
    layout.containers.get_mut(&"root".into()).unwrap().wrap = true;
    assert!(
        layout
            .transaction_editor(&mut editor, |scene| scene
                .update_object(&"root".into(), |o| o.name = "should not commit".into()))
            .is_err()
    );
    assert_eq!(editor.scene(), &frame);
    assert_eq!(editor.engine.revision(), revision);
    assert!(editor.is_playback_active(token));
    assert!(editor.cancel_playback(token));
    assert_eq!(editor.scene(), &before);
    assert!(editor.redo());
    assert_eq!(editor.scene(), &after);
}

#[test]
fn successful_ratio_reflow_finishes_playback_then_records_edit_once() {
    let (mut editor, _) = setup();
    let before = editor.scene().clone();
    let token = editor.begin_playback().unwrap();
    let handle = editor.engine.handle(&"a".into()).unwrap();
    editor
        .apply_playback_properties(token, [(handle, PropertyKey::Opacity, json!(0.5))])
        .unwrap();
    let frame = editor.scene().clone();
    resize(&mut editor, 180.).unwrap();
    assert_eq!(size(editor.scene(), "a"), Size::new(180., 90.));
    assert!(!editor.is_playback_active(token));
    assert!(!editor.cancel_playback(token));
    assert!(editor.undo());
    assert_eq!(editor.scene(), &frame);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(!editor.undo());
}

#[test]
fn destroyed_and_recreated_id_does_not_inherit_old_ratio_binding() {
    let (mut editor, _) = setup();
    let old = editor.engine.handle(&"a".into()).unwrap();
    editor.engine.destroy(old).unwrap();
    editor
        .transaction(|scene| {
            scene.insert(
                DisplayObject::new("a", display::Rect::new(20., 10.)),
                Some("root".into()),
                None,
            )
        })
        .unwrap();
    resize(&mut editor, 160.).unwrap();
    assert_eq!(size(editor.scene(), "a"), Size::new(20., 10.));
    assert_ne!(editor.engine.handle(&"a".into()), Some(old));
}

#[test]
fn late_ratio_overflow_rolls_back_earlier_sibling_layout_and_input_edit() {
    let (mut scene, mut layout) = fixture();
    scene
        .insert(
            DisplayObject::new("bad", display::Rect::new(1., f32::MAX)),
            Some("root".into()),
            None,
        )
        .unwrap();
    layout.items.insert(
        "bad".into(),
        FlowItem {
            lock_ratio: true,
            basis: Some(2.),
            ..Default::default()
        },
    );
    let mut editor = SceneEditor::new(scene).unwrap();
    let before = editor.scene().clone();
    assert!(
        layout
            .transaction_editor(&mut editor, |scene| scene
                .update_object(&"root".into(), |o| o.name = "input".into()))
            .is_err()
    );
    assert_eq!(editor.scene(), &before);
    assert_eq!(editor.engine.revision(), 0);
    assert!(!editor.undo());
}

#[test]
fn many_fractional_ratios_converge_under_engine_reflow_and_repeat_without_history_noise() {
    for i in 1..=64 {
        let (mut scene, layout) = fixture();
        scene
            .update_object(&"a".into(), |o| {
                o.shape = display::Rect::new(i as f32 * 0.731 + 0.1, i as f32 * 0.379 + 0.2).into();
                o.transform = Affine::scale(i as f32 * 0.073 + 0.1, i as f32 * 0.037 + 0.2);
            })
            .unwrap();
        layout.apply_to_scene(&mut scene).unwrap();
        let mut editor = SceneEditor::new(scene).unwrap();
        layout.install_reactive(&mut editor.engine).unwrap();
        resize(&mut editor, 89.123 + i as f32 * 1.733).unwrap();
        for _ in 0..4 {
            assert!(
                layout
                    .apply_to_editor(&mut editor)
                    .unwrap()
                    .changed
                    .is_empty()
            );
        }
        assert!(editor.undo());
        assert!(!editor.undo());
    }
}

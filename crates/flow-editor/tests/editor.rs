use gpui_flow_editor::*;
use gpui_flow_scene::{display::*, *};
fn scene() -> Scene {
    let mut scene = Scene::default();
    scene
        .insert(
            DisplayObject::new("group", Group::new()).with_transform(Affine::scale(2., 2.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("rect", Rect::new(100., 80.)),
            Some("group".into()),
            None,
        )
        .unwrap();
    scene
}
#[test]
fn world_translation_respects_parent() {
    let mut e = SceneEditor::new(scene()).unwrap();
    e.selection.select("rect".into(), false);
    e.translate_selection(Point::new(20., 10.)).unwrap();
    assert_eq!(
        e.scene.get(&"rect".into()).unwrap().object.transform.tx,
        10.
    );
    assert_eq!(
        e.scene.local_to_world(&"rect".into(), Point::ZERO).unwrap(),
        Point::new(20., 10.)
    );
}
#[test]
fn nested_selection_moves_once() {
    let mut e = SceneEditor::new(scene()).unwrap();
    e.selection.select("group".into(), false);
    e.selection.select("rect".into(), true);
    e.translate_selection(Point::new(20., 10.)).unwrap();
    assert_eq!(
        e.scene.get(&"rect".into()).unwrap().object.transform,
        Affine::IDENTITY
    );
    assert_eq!(
        e.scene.local_to_world(&"rect".into(), Point::ZERO).unwrap(),
        Point::new(20., 10.)
    );
}
#[test]
fn gesture_history_and_cancel() {
    let mut e = SceneEditor::new(scene()).unwrap();
    e.selection.select("rect".into(), false);
    let original = e.scene.clone();
    assert!(e.begin_edit());
    assert!(!e.begin_edit());
    e.translate_selection(Point::new(4., 0.)).unwrap();
    e.translate_selection(Point::new(8., 0.)).unwrap();
    assert!(!e.undo());
    e.commit_edit();
    assert!(e.undo());
    assert_eq!(e.scene, original);
    assert!(e.redo());
    e.begin_edit();
    e.translate_selection(Point::new(1., 0.)).unwrap();
    e.cancel_edit();
    assert_eq!(
        e.scene.local_to_world(&"rect".into(), Point::ZERO).unwrap(),
        Point::new(12., 0.)
    );
}
#[test]
fn failed_edit_is_atomic() {
    let mut e = SceneEditor::new(scene()).unwrap();
    let before = e.scene.clone();
    assert!(
        e.transaction(|scene| scene.update_object(&"rect".into(), |o| o.transform.tx = f32::NAN))
            .is_err()
    );
    assert_eq!(e.scene, before);
    assert!(!e.undo());
}
#[test]
fn locked_ancestor_protects_children() {
    let mut s = scene();
    s.update_object(&"group".into(), |o| o.locked = true)
        .unwrap();
    let mut e = SceneEditor::new(s.clone()).unwrap();
    e.selection.select("rect".into(), false);
    e.translate_selection(Point::new(20., 0.)).unwrap();
    e.delete_selection().unwrap();
    assert_eq!(s, e.scene);
}
#[test]
fn deleting_group_is_undoable() {
    let mut e = SceneEditor::new(scene()).unwrap();
    e.selection.select("group".into(), false);
    e.delete_selection().unwrap();
    assert!(e.scene.get(&"rect".into()).is_none());
    assert!(e.undo());
    assert!(e.scene.get(&"rect".into()).is_some());
}

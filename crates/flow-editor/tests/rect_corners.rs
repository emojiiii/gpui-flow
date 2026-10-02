use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{DisplayObject, Scene, Shape, display::Rect};
use serde_json::json;
fn editor() -> SceneEditor {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("r", Rect::new(60., 40.).rounded(4.)),
            None,
            None,
        )
        .unwrap();
    let mut e = SceneEditor::new(scene).unwrap();
    e.selection.select("r".into(), false);
    e
}
#[test]
fn independent_corners_undo_redo_once_without_identity_or_selection_change() {
    let mut e = editor();
    let h = e.engine.handle(&"r".into()).unwrap();
    let before = e.scene().clone();
    e.set_property(
        h,
        PropertyKey::Shape,
        json!(Shape::from(
            Rect::new(60., 40.).with_corners([15., 0., 10., 0.])
        )),
    )
    .unwrap();
    let after = e.scene().clone();
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
    assert_eq!(e.engine.handle(&"r".into()), Some(h));
    assert!(e.selection.ids.contains(&"r".into()));
}
#[test]
fn cancel_and_invalid_corner_edit_keep_prior_redo() {
    let mut e = editor();
    let h = e.engine.handle(&"r".into()).unwrap();
    let before = e.scene().clone();
    e.set_property(
        h,
        PropertyKey::Shape,
        json!(Shape::from(
            Rect::new(60., 40.).with_corners([15., 0., 10., 0.])
        )),
    )
    .unwrap();
    let after = e.scene().clone();
    assert!(e.undo());
    assert!(e.begin_edit());
    e.set_property(
        h,
        PropertyKey::Shape,
        json!(Shape::from(
            Rect::new(60., 40.).with_corners([0., 15., 0., 10.])
        )),
    )
    .unwrap();
    assert!(e.cancel_edit());
    assert_eq!(e.scene(), &before);
    assert!(
        e.set_property(
            h,
            PropertyKey::Shape,
            json!(Shape::from(Rect::new(60., 40.).with_corners([-1.; 4])))
        )
        .is_err()
    );
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
}

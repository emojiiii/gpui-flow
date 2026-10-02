use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{DisplayObject, Scene, Shape, display::Ellipse};
use serde_json::json;
fn editor() -> SceneEditor {
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("e", Ellipse::circle(20.)), None, None)
        .unwrap();
    let mut e = SceneEditor::new(scene).unwrap();
    e.selection.select("e".into(), false);
    e
}
#[test]
fn sector_and_hole_changes_undo_redo_once_with_identity_and_selection() {
    let mut e = editor();
    let h = e.engine.handle(&"e".into()).unwrap();
    let before = e.scene().clone();
    e.set_property(
        h,
        PropertyKey::Shape,
        json!(Shape::from(Ellipse::circle(20.).sector(0., 270.).ring(0.5))),
    )
    .unwrap();
    let after = e.scene().clone();
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
    assert_eq!(e.engine.handle(&"e".into()), Some(h));
    assert!(e.selection.ids.contains(&"e".into()));
}
#[test]
fn cancelled_arc_edit_and_invalid_ratio_preserve_existing_redo() {
    let mut e = editor();
    let h = e.engine.handle(&"e".into()).unwrap();
    let before = e.scene().clone();
    e.set_property(
        h,
        PropertyKey::Shape,
        json!(Shape::from(Ellipse::circle(20.).ring(0.5))),
    )
    .unwrap();
    let after = e.scene().clone();
    assert!(e.undo());
    assert!(e.begin_edit());
    e.set_property(
        h,
        PropertyKey::Shape,
        json!(Shape::from(Ellipse::circle(20.).arc(-90., 90.))),
    )
    .unwrap();
    assert!(e.cancel_edit());
    assert_eq!(e.scene(), &before);
    assert!(
        e.set_property(
            h,
            PropertyKey::Shape,
            json!(Shape::from(Ellipse::circle(20.).ring(-1.)))
        )
        .is_err()
    );
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
}

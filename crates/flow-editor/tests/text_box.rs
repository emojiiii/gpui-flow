use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{
    DisplayObject, Scene, Shape, Size,
    display::{Text, TextVerticalAlign},
};
use serde_json::json;
#[test]
fn text_box_undo_redo_and_cancel_preserve_source_and_history() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("t", Text::new("content", Size::new(100., 60.))),
        None,
        None,
    )
    .unwrap();
    let mut e = SceneEditor::new(s).unwrap();
    let h = e.engine.handle(&"t".into()).unwrap();
    let before = e.scene().clone();
    let Shape::Text(mut t) = e.engine.object(h).unwrap().shape.clone() else {
        unreachable!()
    };
    t.padding = [4.; 4];
    t.vertical_align = TextVerticalAlign::Bottom;
    e.set_property(h, PropertyKey::Shape, json!(Shape::from(t.clone())))
        .unwrap();
    let after = e.scene().clone();
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(e.begin_edit());
    t.padding = [6.; 4];
    e.set_property(h, PropertyKey::Shape, json!(Shape::from(t.clone())))
        .unwrap();
    assert!(e.cancel_edit());
    t.padding = [100.; 4];
    assert!(
        e.set_property(h, PropertyKey::Shape, json!(Shape::from(t)))
            .is_err()
    );
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
    assert_eq!(e.engine.handle(&"t".into()), Some(h));
}

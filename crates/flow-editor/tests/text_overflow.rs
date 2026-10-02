use gpui_flow_editor::SceneEditor;
use gpui_flow_scene::{
    DisplayObject, Scene, Shape, Size,
    display::{Text, TextOverflow},
};
#[test]
fn ellipsis_configuration_and_content_edits_keep_one_history_step_and_restore() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("t", Text::new("long source text", Size::new(80., 30.))),
        None,
        None,
    )
    .unwrap();
    let mut e = SceneEditor::new(s).unwrap();
    let before = e.scene().clone();
    let handle = e.engine.handle(&"t".into()).unwrap();
    e.transaction(|s| {
        s.update_object(&"t".into(), |o| {
            if let Shape::Text(t) = &mut o.shape {
                t.overflow = TextOverflow::Ellipsis;
                t.content.push_str(" with more");
            }
        })
    })
    .unwrap();
    let after = e.scene().clone();
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
    assert_eq!(e.engine.handle(&"t".into()), Some(handle));
}

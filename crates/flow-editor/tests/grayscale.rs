use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{display, *};
use serde_json::json;

#[test]
fn grayscale_undo_redo_and_failed_edit_keep_selection_geometry_identity_and_redo() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("r", display::Rect::new(20., 20.)),
        None,
        None,
    )
    .unwrap();
    let mut e = SceneEditor::new(s).unwrap();
    e.selection.select("r".into(), false);
    let before = e.scene().clone();
    let selection = e.selection.clone();
    let handle = e.engine.handle(&"r".into()).unwrap();
    let identity = e.engine.instance(handle).unwrap();
    let bounds = e.engine.effect_bounds(&"r".into()).unwrap();
    e.set_property(handle, PropertyKey::Grayscale, json!(0.5))
        .unwrap();
    let after = e.scene().clone();
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(
        e.set_property(handle, PropertyKey::Grayscale, json!(2))
            .is_err()
    );
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
    e.set_property(handle, PropertyKey::Grayscale, json!(0.))
        .unwrap();
    assert_eq!(e.scene(), &before);
    assert!(e.undo());
    assert_eq!(e.scene(), &after);
    assert_eq!(e.selection, selection);
    assert_eq!(e.engine.instance(handle).unwrap(), identity);
    assert_eq!(e.engine.effect_bounds(&"r".into()).unwrap(), bounds);
}

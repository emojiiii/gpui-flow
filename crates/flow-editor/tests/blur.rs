use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{display, *};
use serde_json::json;

#[test]
fn blur_undo_redo_and_failed_edit_preserve_selection_geometry_identity_and_redo() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(20., 20.)),
            None,
            None,
        )
        .unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    editor.selection.select("r".into(), false);
    let before = editor.scene().clone();
    let selection = editor.selection.clone();
    let handle = editor.engine.handle(&"r".into()).unwrap();
    let identity = editor.engine.instance(handle).unwrap();
    let geometry = editor.engine.world_bounds(&"r".into()).unwrap();
    editor
        .set_property(handle, PropertyKey::Blur, json!(2.))
        .unwrap();
    let after = editor.scene().clone();
    let effect = editor.engine.effect_bounds(&"r".into()).unwrap();
    assert_ne!(effect, geometry);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(
        editor
            .set_property(handle, PropertyKey::Blur, json!(257))
            .is_err()
    );
    assert!(editor.redo());
    assert_eq!(editor.scene(), &after);
    assert_eq!(editor.engine.effect_bounds(&"r".into()).unwrap(), effect);
    editor
        .set_property(handle, PropertyKey::Blur, json!(0.))
        .unwrap();
    assert_eq!(editor.scene(), &before);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &after);
    assert_eq!(editor.selection, selection);
    assert_eq!(editor.engine.instance(handle).unwrap(), identity);
    assert_eq!(editor.engine.world_bounds(&"r".into()).unwrap(), geometry);
}

use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{display, *};
use serde_json::json;

fn editor() -> SceneEditor {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("glass", display::Rect::new(20., 20.)),
            None,
            None,
        )
        .unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    editor.selection.select("glass".into(), false);
    editor
}

#[test]
fn background_blur_undo_redo_retains_identity_selection_and_write_bounds() {
    let mut editor = editor();
    let handle = editor.engine.handle(&"glass".into()).unwrap();
    let identity = editor.engine.instance(handle).unwrap();
    let selection = editor.selection.clone();
    let geometry = editor.engine.world_bounds(&"glass".into()).unwrap();
    let effects = editor.engine.effect_bounds(&"glass".into()).unwrap();
    let before = editor.scene().clone();
    editor
        .set_property(handle, PropertyKey::BackgroundBlur, json!(8.))
        .unwrap();
    let after = editor.scene().clone();
    assert_ne!(after, before);
    assert_eq!(
        editor.engine.effect_bounds(&"glass".into()).unwrap(),
        effects
    );
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(editor.redo());
    assert_eq!(editor.scene(), &after);
    editor
        .set_property(handle, PropertyKey::BackgroundBlur, json!(0.))
        .unwrap();
    assert_eq!(editor.scene(), &before);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &after);
    assert_eq!(editor.selection, selection);
    assert_eq!(editor.engine.instance(handle).unwrap(), identity);
    assert_eq!(
        editor.engine.world_bounds(&"glass".into()).unwrap(),
        geometry
    );
    assert_eq!(
        editor.engine.effect_bounds(&"glass".into()).unwrap(),
        effects
    );
}

#[test]
fn background_blur_noops_do_not_create_history_or_discard_redo() {
    let mut editor = editor();
    let handle = editor.engine.handle(&"glass".into()).unwrap();
    let revision = editor.engine.revision();
    editor
        .set_property(handle, PropertyKey::BackgroundBlur, json!(-0.))
        .unwrap();
    assert_eq!(editor.engine.revision(), revision);
    assert!(!editor.undo());
    editor
        .set_property(handle, PropertyKey::BackgroundBlur, json!(4.))
        .unwrap();
    let after = editor.scene().clone();
    editor
        .set_property(handle, PropertyKey::BackgroundBlur, json!(4.))
        .unwrap();
    assert!(editor.undo());
    assert_eq!(
        editor
            .scene()
            .get(&"glass".into())
            .unwrap()
            .object
            .style
            .background_blur,
        0.
    );
    assert!(!editor.undo());
    editor
        .set_property(handle, PropertyKey::BackgroundBlur, json!(0.))
        .unwrap();
    assert!(editor.redo());
    assert_eq!(editor.scene(), &after);
    assert!(!editor.redo());
}

#[test]
fn invalid_retained_background_blur_edits_preserve_redo_and_selection() {
    let mut editor = editor();
    let handle = editor.engine.handle(&"glass".into()).unwrap();
    editor
        .transaction(|scene| {
            scene.update_object(&"glass".into(), |object| {
                object.visible = false;
                object.style.opacity = 0.;
            })
        })
        .unwrap();
    editor
        .set_property(handle, PropertyKey::BackgroundBlur, json!(6.))
        .unwrap();
    let after = editor.scene().clone();
    assert!(editor.undo());
    let before = editor.scene().clone();
    let revision = editor.engine.revision();
    let selection = editor.selection.clone();
    let identity = editor.engine.instance(handle).unwrap();
    for value in [json!(-1), json!(257), json!(null), json!({"blur": 2})] {
        assert!(
            editor
                .set_property(handle, PropertyKey::BackgroundBlur, value)
                .is_err()
        );
        assert_eq!(editor.scene(), &before);
        assert_eq!(editor.engine.revision(), revision);
        assert_eq!(editor.selection, selection);
        assert_eq!(editor.engine.instance(handle).unwrap(), identity);
    }
    assert!(
        editor
            .transaction(|scene| {
                scene.update_object(&"glass".into(), |object| {
                    object.name = "must roll back".into();
                    object.style.background_blur = f32::NAN;
                })
            })
            .is_err()
    );
    assert_eq!(editor.scene(), &before);
    assert_eq!(editor.engine.revision(), revision);
    assert!(editor.redo());
    assert_eq!(editor.scene(), &after);
    assert_eq!(editor.selection, selection);
    assert_eq!(editor.engine.instance(handle).unwrap(), identity);
}

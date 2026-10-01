use gpui_flow_editor::*;
use gpui_flow_engine::{PropertyKey, SceneEngine};
use gpui_flow_scene::{DisplayObject, Scene, Shape, Size, display};
use serde_json::json;
fn editor() -> SceneEditor {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("text", display::Text::new("before", Size::new(100., 40.))),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("rect", display::Rect::new(10., 10.)),
            None,
            None,
        )
        .unwrap();
    SceneEditor::new(scene).unwrap()
}
fn text(editor: &SceneEditor) -> &str {
    let Shape::Text(text) = &editor.scene().get(&"text".into()).unwrap().object.shape else {
        panic!()
    };
    &text.content
}
#[test]
fn many_live_updates_form_one_undo_and_noop_preserves_redo() {
    let mut editor = editor();
    let handle = editor.engine.handle(&"text".into()).unwrap();
    let token = editor.begin_edit_session().unwrap();
    for value in ["one", "two", "three"] {
        editor
            .set_edit_session_property(token, handle, PropertyKey::TextContent, json!(value))
            .unwrap();
    }
    assert!(editor.finish_edit_session(token));
    assert!(!editor.finish_edit_session(token));
    assert!(editor.undo());
    assert_eq!(text(&editor), "before");
    assert!(!editor.undo());
    let noop = editor.begin_edit_session().unwrap();
    editor
        .set_edit_session_property(noop, handle, PropertyKey::TextContent, json!("before"))
        .unwrap();
    assert!(editor.finish_edit_session(noop));
    assert!(editor.redo());
    assert_eq!(text(&editor), "three");
}
#[test]
fn cancel_restores_baseline_preserves_redo_and_selection() {
    let mut editor = editor();
    let handle = editor.engine.handle(&"text".into()).unwrap();
    editor
        .set_property(handle, PropertyKey::TextContent, json!("redo"))
        .unwrap();
    assert!(editor.undo());
    let token = editor.begin_edit_session().unwrap();
    editor
        .set_edit_session_property(token, handle, PropertyKey::TextContent, json!("cancel"))
        .unwrap();
    editor.selection.select("rect".into(), false);
    assert!(editor.cancel_edit_session(token));
    assert_eq!(text(&editor), "before");
    assert!(editor.selection.ids.contains(&"rect".into()));
    assert!(editor.redo());
    assert_eq!(text(&editor), "redo");
}
#[test]
fn ordinary_editor_write_closes_session_without_contaminating_undo() {
    let mut editor = editor();
    let handle = editor.engine.handle(&"text".into()).unwrap();
    let token = editor.begin_edit_session().unwrap();
    editor
        .set_edit_session_property(token, handle, PropertyKey::TextContent, json!("typed"))
        .unwrap();
    editor
        .transaction(|scene| {
            scene.update_object(&"rect".into(), |object| object.name = "external".into())
        })
        .unwrap();
    assert!(!editor.cancel_edit_session(token));
    assert_eq!(text(&editor), "typed");
    assert!(editor.undo());
    assert_eq!(editor.scene().get(&"rect".into()).unwrap().object.name, "");
    assert_eq!(text(&editor), "typed");
    assert!(editor.undo());
    assert_eq!(text(&editor), "before");
    assert!(!editor.undo());
}
#[test]
fn direct_engine_write_closes_session_and_preserves_both_history_steps() {
    let mut editor = editor();
    let handle = editor.engine.handle(&"text".into()).unwrap();
    let token = editor.begin_edit_session().unwrap();
    editor
        .set_edit_session_property(token, handle, PropertyKey::TextContent, json!("typed"))
        .unwrap();
    editor
        .engine
        .set_property(handle, PropertyKey::TextContent, json!("external"))
        .unwrap();
    assert!(!editor.cancel_edit_session(token));
    assert_eq!(text(&editor), "external");
    assert!(editor.undo());
    assert_eq!(text(&editor), "typed");
    assert!(editor.undo());
    assert_eq!(text(&editor), "before");
}
#[test]
fn failed_external_transaction_preserves_session_and_redo() {
    let mut editor = editor();
    let handle = editor.engine.handle(&"text".into()).unwrap();
    let token = editor.begin_edit_session().unwrap();
    assert!(
        editor
            .transaction(|scene| scene
                .update_object(&"text".into(), |object| object.transform.tx = f32::NAN))
            .is_err()
    );
    assert!(editor.is_edit_session_active(token));
    assert!(
        editor
            .set_edit_session_property(token, handle, PropertyKey::TextContent, json!(false))
            .is_err()
    );
    assert!(editor.is_edit_session_active(token));
    assert!(editor.cancel_edit_session(token));
    assert!(!editor.undo());
}
#[test]
fn document_load_and_engine_replacement_never_restore_old_session() {
    let mut editor = editor();
    let handle = editor.engine.handle(&"text".into()).unwrap();
    let token = editor.begin_edit_session().unwrap();
    editor
        .set_edit_session_property(token, handle, PropertyKey::TextContent, json!("typed"))
        .unwrap();
    let document = editor.scene().clone();
    editor.engine.reset_scene(document.clone()).unwrap();
    assert!(!editor.cancel_edit_session(token));
    assert!(!editor.undo());
    let token = editor.begin_edit_session().unwrap();
    editor.engine = SceneEngine::new(document).unwrap();
    assert!(!editor.cancel_edit_session(token));
    assert!(!editor.undo());
}
#[test]
fn gestures_playback_and_stale_tokens_cannot_share_baselines() {
    let mut editor = editor();
    let handle = editor.engine.handle(&"text".into()).unwrap();
    let token = editor.begin_edit_session().unwrap();
    editor
        .set_edit_session_property(token, handle, PropertyKey::TextContent, json!("typed"))
        .unwrap();
    assert!(editor.begin_edit_session().is_err());
    assert!(editor.begin_playback().is_err());
    assert!(editor.begin_edit());
    assert!(!editor.is_edit_session_active(token));
    assert!(editor.begin_edit_session().is_err());
    assert!(!editor.cancel_edit_session(token));
    editor.cancel_edit();
    assert_eq!(text(&editor), "typed");
    assert!(editor.undo());
    assert_eq!(text(&editor), "before");
    let newer = editor.begin_edit_session().unwrap();
    assert!(
        editor
            .set_edit_session_property(token, handle, PropertyKey::TextContent, json!("stale"))
            .is_err()
    );
    assert!(editor.is_edit_session_active(newer));
}

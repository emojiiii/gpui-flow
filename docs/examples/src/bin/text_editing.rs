use gpui_flow_editor::SceneEditor;
use gpui_flow_interaction::scene::{ScenePluginHost, SceneToolController};
use gpui_flow_scene::{DisplayObject, Scene, Shape, Size, display};
fn content(editor: &SceneEditor) -> &str {
    let Shape::Text(text) = &editor.scene().get(&"text".into()).unwrap().object.shape else {
        panic!("text")
    };
    &text.content
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new();
    scene.insert(
        DisplayObject::new("text", display::Text::new("before", Size::new(200., 80.))),
        None,
        None,
    )?;
    let mut editor = SceneEditor::new(scene)?;
    let mut host = ScenePluginHost::new();
    let mut tools = SceneToolController::with_text();
    let target = editor.engine.handle(&"text".into()).unwrap();
    let token = tools.open_text(&mut editor, &mut host, target)?;
    for text in ["one", "two", "multiline\n中文 👩‍💻"] {
        tools.set_text(&mut editor, &mut host, token, text)?;
    }
    assert!(tools.finish(&mut editor, &mut host, token)?);
    assert!(!tools.finish(&mut editor, &mut host, token)?);
    assert!(editor.undo());
    assert_eq!(content(&editor), "before");
    assert!(!editor.undo());
    let cancel = tools.open_text(&mut editor, &mut host, target)?;
    tools.set_text(&mut editor, &mut host, cancel, "cancel me")?;
    assert!(tools.cancel(&mut editor, &mut host, cancel)?);
    assert!(editor.redo());
    assert_eq!(content(&editor), "multiline\n中文 👩‍💻");
    tools.dispose(&mut editor, &mut host)?;
    Ok(())
}

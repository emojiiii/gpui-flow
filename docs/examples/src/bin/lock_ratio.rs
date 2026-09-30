//! Executable bounded ratio-lock allocation, reflow, history and JSON contract.
use gpui_flow_editor::SceneEditor;
use gpui_flow_layout::{FlowDocument, FlowItem, FlowLayout, FlowStyle};
use gpui_flow_scene::{DisplayObject, Scene, Size, display};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new();
    scene.insert(
        DisplayObject::new("row", display::Frame::new(100., 100.)),
        None,
        None,
    )?;
    scene.insert(
        DisplayObject::new("card", display::Rect::new(20., 10.)),
        Some("row".into()),
        None,
    )?;
    let mut layout = FlowLayout::default();
    layout.containers.insert("row".into(), FlowStyle::default());
    layout.items.insert(
        "card".into(),
        FlowItem {
            lock_ratio: true,
            grow: 1.,
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene)?;
    let size = |scene: &Scene| {
        scene
            .get(&"card".into())
            .unwrap()
            .object
            .shape
            .local_bounds()
            .unwrap()
            .size
    };
    assert_eq!(size(&scene), Size::new(100., 50.));
    let mut editor = SceneEditor::new(scene.clone())?;
    layout.install_reactive(&mut editor.engine)?;
    editor.transaction(|scene| {
        scene.update_object(&"row".into(), |o| {
            o.shape = display::Frame::new(160., 100.).into()
        })
    })?;
    assert_eq!(size(editor.scene()), Size::new(160., 80.));
    let resized = editor.scene().clone();
    assert!(editor.undo());
    assert_eq!(editor.scene(), &scene);
    assert!(!editor.undo());
    assert!(editor.redo());
    assert_eq!(editor.scene(), &resized);
    assert!(layout.apply_to_editor(&mut editor)?.changed.is_empty());
    let document = FlowDocument {
        scene: resized,
        layout,
    };
    assert_eq!(FlowDocument::from_json(&document.to_json()?)?, document);
    println!("lock_ratio: 20x10 -> 100x50 -> 160x80, reactive one-Undo and JSON verified");
    Ok(())
}

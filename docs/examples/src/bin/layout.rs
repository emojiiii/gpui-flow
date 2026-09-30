//! Headless, executable layout contract and persistence example.
use gpui_flow_editor::SceneEditor;
use gpui_flow_layout::{FlowDocument, FlowItem, FlowLayout, FlowStyle, Insets};
use gpui_flow_scene::{Affine, DisplayObject, Scene, Shape, display};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new();
    scene.insert(
        DisplayObject::new("row", display::Frame::new(400., 100.)),
        None,
        None,
    )?;
    for id in ["a", "b", "c"] {
        scene.insert(
            DisplayObject::new(id, display::Rect::new(80., 40.))
                .with_transform(Affine::translation(2., 3.)),
            Some("row".into()),
            None,
        )?;
    }
    let mut layout = FlowLayout::default();
    layout.containers.insert(
        "row".into(),
        FlowStyle {
            padding: Insets::all(10.),
            gap: 10.,
            ..Default::default()
        },
    );
    layout.items.insert(
        "c".into(),
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
    );
    let before = scene.clone();
    let mut editor = SceneEditor::new(scene)?;
    let original_handle = editor.engine.handle(&"a".into()).unwrap();
    let report = layout.apply_to_editor(&mut editor)?;
    assert_eq!(report.changed.len(), 3);
    let a = &editor.scene().get(&"a".into()).unwrap().object;
    assert_eq!((a.transform.tx, a.transform.ty), (10., 10.));
    assert_eq!(
        editor.scene().get(&"b".into()).unwrap().object.transform.tx,
        100.
    );
    let Shape::Rect(c) = &editor.scene().get(&"c".into()).unwrap().object.shape else {
        panic!("rect")
    };
    assert_eq!(c.size.width, 200.);
    assert_eq!(editor.engine.handle(&"a".into()), Some(original_handle));
    assert!(layout.apply_to_editor(&mut editor)?.changed.is_empty());
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(!editor.undo());
    assert!(editor.redo());
    let document = FlowDocument {
        scene: editor.scene().clone(),
        layout,
    };
    assert_eq!(FlowDocument::from_json(&document.to_json()?)?, document);
    println!("layout: row/grow geometry, stable instances, single undo and persistence verified");
    Ok(())
}

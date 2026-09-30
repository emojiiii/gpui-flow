use gpui_flow_editor::SceneEditor;
use gpui_flow_layout::{AxisSizing, FlowLayout, FlowStyle, Insets};
use gpui_flow_scene::{DisplayObject, Scene, Shape, display};
fn height(scene: &Scene) -> f32 {
    scene
        .get(&"frame".into())
        .unwrap()
        .object
        .shape
        .local_bounds()
        .unwrap()
        .size
        .height
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new();
    scene.insert(
        DisplayObject::new("frame", display::Frame::new(620., 900.)),
        None,
        None,
    )?;
    for index in 0..7 {
        scene.insert(
            DisplayObject::new(format!("card-{index}"), display::Rect::new(140., 80.)),
            Some("frame".into()),
            None,
        )?;
    }
    let mut layout = FlowLayout::default();
    layout.containers.insert(
        "frame".into(),
        FlowStyle {
            height_sizing: AxisSizing::content(),
            padding: Insets::all(20.),
            gap: 16.,
            line_gap: 18.,
            wrap: true,
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene)?;
    assert_eq!(height(&scene), 316.);
    let mut editor = SceneEditor::new(scene)?;
    layout.install_reactive(&mut editor.engine)?;
    editor.transaction(|scene| {
        scene.update_object(&"frame".into(), |object| object.name = "prior edit".into())
    })?;
    editor.transaction(|scene| {
        scene.update_object(&"frame".into(), |object| {
            let Shape::Frame(frame) = &mut object.shape else {
                unreachable!()
            };
            frame.size.width = 360.;
        })
    })?;
    assert_eq!(height(editor.scene()), 414.);
    editor.transaction(|scene| {
        for index in 7..9 {
            scene.insert(
                DisplayObject::new(format!("card-{index}"), display::Rect::new(140., 80.)),
                Some("frame".into()),
                None,
            )?;
        }
        Ok(())
    })?;
    assert_eq!(height(editor.scene()), 512.);
    assert!(editor.undo());
    assert_eq!(height(editor.scene()), 414.);
    assert!(editor.undo());
    assert_eq!(height(editor.scene()), 316.);
    assert_eq!(
        editor.scene().get(&"frame".into()).unwrap().object.name,
        "prior edit"
    );
    assert!(editor.undo());
    assert_eq!(editor.scene().get(&"frame".into()).unwrap().object.name, "");
    println!(
        "Reactive engine layout: ordinary edits update dependent height and preserve single-step history"
    );
    Ok(())
}

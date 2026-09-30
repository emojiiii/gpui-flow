use gpui_flow_animation::{AnimationController, AnimationSpec, Tween};
use gpui_flow_editor::SceneEditor;
use gpui_flow_scene::{DisplayObject, Point, Scene, display};
use std::time::Duration;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new();
    scene.insert(
        DisplayObject::new("card", display::Rect::new(100., 70.)),
        None,
        None,
    )?;
    let mut editor = SceneEditor::new(scene)?;
    editor.transaction(|scene| {
        scene.update_object(&"card".into(), |object| object.transform.tx = 10.)
    })?;
    let token = editor.begin_playback()?;
    let mut controller = AnimationController::new(&editor.engine);
    let target = editor.engine.handle(&"card".into()).unwrap();
    let id = controller.animate(
        &editor.engine,
        target,
        AnimationSpec::new(
            1000,
            vec![
                Tween::translation(Point::new(100., 40.)),
                Tween::opacity(0.5),
            ],
        ),
    )?;
    controller.tick_editor(&mut editor, token, Duration::from_millis(250))?;
    assert_eq!(editor.engine.object(target)?.transform.tx, 32.5);
    controller.pause(&editor.engine, id)?;
    assert!(
        !controller
            .tick_editor(&mut editor, token, Duration::from_secs(1))?
            .changes
            .changed()
    );
    controller.seek_editor(&mut editor, token, id, Duration::from_millis(500))?;
    controller.resume(&editor.engine, id)?;
    controller.tick_editor(&mut editor, token, Duration::from_millis(500))?;
    controller.finish_editor(&mut editor, token)?;
    assert!(editor.undo());
    assert_eq!(editor.engine.object(target)?.transform.tx, 10.);
    assert!(editor.undo());
    assert_eq!(editor.engine.object(target)?.transform.tx, 0.);
    assert!(!editor.undo());
    println!(
        "Animation interpolation, playback control and one-step commit preserve prior edit history"
    );
    Ok(())
}

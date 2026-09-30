use gpui_flow_editor::SceneEditor;
use gpui_flow_interaction::{CanvasEvent, KeyCode, Modifiers, scene::*};
use gpui_flow_scene::{Color, DisplayObject, EventKind, ListenerOptions, Scene, Style, display};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new();
    for id in ["a", "b"] {
        scene.insert(
            DisplayObject::new(id, display::Rect::new(80., 50.))
                .with_style(Style::fill(Color::BLACK)),
            None,
            None,
        )?;
    }
    let mut editor = SceneEditor::new(scene)?;
    editor.transaction(|scene| {
        scene.update_object(&"a".into(), |object| object.name = "edited".into())
    })?;
    let a = editor.engine.handle(&"a".into()).unwrap();
    let b = editor.engine.handle(&"b".into()).unwrap();
    let mut router = SceneEventRouter::new();
    let mut host = ScenePluginHost::new();
    let mut styles = SceneStateStyles::new(&editor.engine);
    styles.bind(
        &editor.engine,
        a,
        StateStyles {
            focus: StatePaint::fill(Color::WHITE),
            ..Default::default()
        },
    )?;
    router.set_focus(&mut editor, &mut host, Some(a))?;
    assert!(editor.selection.ids.is_empty());
    assert_eq!(
        styles
            .presentation(&editor, &router)?
            .get(&"a".into())
            .unwrap()
            .object
            .style
            .fill,
        Some(Color::WHITE.into())
    );
    let sender = router.action_sender();
    let request = SceneFocusRequest::new(&editor.engine, a, Some(b))?;
    editor.engine.on(
        a,
        EventKind::KeyDown,
        ListenerOptions::default(),
        move |_| {
            sender.enqueue_focus(request).unwrap();
        },
    )?;
    let report = router.dispatch(
        &mut editor,
        &mut host,
        CanvasEvent::KeyDown {
            key: KeyCode::Character('x'),
            modifiers: Modifiers::default(),
        },
    );
    assert!(report.focus_requested);
    assert_eq!(router.focused(&editor.engine), Some(b));
    let focus = report
        .events
        .iter()
        .find(|event| event.kind == EventKind::Focus)
        .unwrap();
    assert!(!focus.cancelable);
    assert_eq!(focus.related_target.as_ref().unwrap().as_str(), "a");
    assert_eq!(
        styles
            .presentation(&editor, &router)?
            .get(&"a".into())
            .unwrap()
            .object
            .style
            .fill,
        Some(Color::BLACK.into())
    );
    router.clear_focus(&mut editor, &mut host);
    assert_eq!(router.focused(&editor.engine), None);
    assert!(editor.undo());
    assert!(!editor.undo());
    println!(
        "Programmatic/deferred focus, state presentation and independent selection/history verified"
    );
    Ok(())
}

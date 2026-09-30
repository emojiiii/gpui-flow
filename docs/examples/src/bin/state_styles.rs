use gpui_flow_editor::SceneEditor;
use gpui_flow_interaction::{CanvasEvent, HitTarget, PointerInput, scene::*};
use gpui_flow_scene::{Color, DisplayObject, Paint, Point, Scene, Style, display};
fn fill(scene: &Scene) -> Paint {
    scene
        .get(&"card".into())
        .unwrap()
        .object
        .style
        .fill
        .clone()
        .unwrap()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new();
    scene.insert(
        DisplayObject::new("card", display::Rect::new(160., 90.))
            .with_style(Style::fill(Color::BLACK)),
        None,
        None,
    )?;
    let mut editor = SceneEditor::new(scene)?;
    let handle = editor.engine.handle(&"card".into()).unwrap();
    let mut sheet = SceneStateStyles::new(&editor.engine);
    sheet.bind(
        &editor.engine,
        handle,
        StateStyles {
            hover: StatePaint::fill(Color::hex(0x239e91)),
            press: StatePaint::fill(Color::hex(0xd38b36)),
            disabled: StatePaint::fill(Color::hex(0x586476)),
            ..Default::default()
        },
    )?;
    let mut host = ScenePluginHost::new();
    let mut router = SceneEventRouter::new();
    let pointer = PointerInput::left(Point::new(20., 20.), HitTarget::Object("card".into()));
    router.dispatch(
        &mut editor,
        &mut host,
        CanvasEvent::PointerMove(pointer.clone()),
    );
    assert_eq!(
        fill(sheet.presentation(&editor, &router)?.as_ref()),
        Color::hex(0x239e91).into()
    );
    assert_eq!(fill(editor.scene()), Color::BLACK.into());
    editor.transaction(|scene| {
        scene.update_object(&"card".into(), |o| o.style.fill = Some(Color::WHITE.into()))
    })?;
    router.dispatch(
        &mut editor,
        &mut host,
        CanvasEvent::PointerDown(pointer.clone()),
    );
    assert_eq!(
        fill(sheet.presentation(&editor, &router)?.as_ref()),
        Color::hex(0xd38b36).into()
    );
    sheet.set_disabled(&editor.engine, handle, true)?;
    assert_eq!(
        fill(sheet.presentation(&editor, &router)?.as_ref()),
        Color::hex(0x586476).into()
    );
    sheet.set_disabled(&editor.engine, handle, false)?;
    router.dispatch(&mut editor, &mut host, CanvasEvent::PointerUp(pointer));
    router.dispatch(&mut editor, &mut host, CanvasEvent::FocusLost);
    assert_eq!(
        fill(sheet.presentation(&editor, &router)?.as_ref()),
        Color::WHITE.into()
    );
    assert!(editor.undo());
    assert_eq!(fill(editor.scene()), Color::BLACK.into());
    assert!(!editor.undo());
    sheet.presentation(&editor, &router)?; // accept the new canonical base after Undo
    sheet.set_transitions(
        &editor.engine,
        handle,
        StateTransitions {
            transition: StateTransition::new(100),
            transition_out: Some(StateTransition::new(200)),
        },
    )?;
    router.dispatch(
        &mut editor,
        &mut host,
        CanvasEvent::PointerMove(PointerInput::left(
            Point::new(20., 20.),
            HitTarget::Object("card".into()),
        )),
    );
    use std::time::Duration;
    sheet.presentation_at(&editor, &router, Duration::ZERO)?;
    let midway = fill(
        sheet
            .presentation_at(&editor, &router, Duration::from_millis(50))?
            .as_ref(),
    );
    assert_ne!(midway, Color::BLACK.into());
    assert_ne!(midway, Color::hex(0x239e91).into());
    router.dispatch(&mut editor, &mut host, CanvasEvent::PointerLeave);
    assert_eq!(
        fill(
            sheet
                .presentation_at(&editor, &router, Duration::from_millis(50))?
                .as_ref()
        ),
        midway
    );
    assert_eq!(
        fill(
            sheet
                .presentation_at(&editor, &router, Duration::from_millis(250))?
                .as_ref()
        ),
        Color::BLACK.into()
    );
    assert!(!sheet.has_active_transitions());
    assert!(!editor.undo());
    println!("State priority, transition entry/exit/reversal and untouched history verified");
    Ok(())
}

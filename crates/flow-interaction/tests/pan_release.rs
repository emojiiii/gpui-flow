use gpui_flow_core::{Editor, Graph, Point};
use gpui_flow_interaction::*;
fn pan_start() -> (PluginHost, Editor) {
    let mut host = PluginHost::with_defaults();
    let mut editor = Editor::new(Graph::new()).unwrap();
    editor.viewport.pan = Point::new(35., -12.);
    let mut input = PointerInput::left(Point::new(100., 100.), HitTarget::Background);
    input.modifiers.space = true;
    host.dispatch(&mut editor, CanvasEvent::PointerDown(input.clone()));
    input.position = Point::new(270., 190.);
    host.dispatch(&mut editor, CanvasEvent::PointerMove(input));
    assert_eq!(editor.viewport.pan, Point::new(205., 78.));
    (host, editor)
}
#[test]
fn release_space_before_mouse_commits_pan_and_later_blur_cannot_recenter() {
    let (mut host, mut editor) = pan_start();
    host.dispatch(
        &mut editor,
        CanvasEvent::KeyUp {
            key: KeyCode::Space,
            modifiers: Modifiers::default(),
        },
    );
    assert!(!host.has_capture());
    host.dispatch(&mut editor, CanvasEvent::FocusLost);
    host.dispatch(
        &mut editor,
        CanvasEvent::PointerUp(PointerInput::left(
            Point::new(270., 190.),
            HitTarget::Background,
        )),
    );
    assert_eq!(editor.viewport.pan, Point::new(205., 78.));
    assert_eq!(editor.history_len(), 0);
}
#[test]
fn release_mouse_outside_canvas_commits_before_space_up() {
    let (mut host, mut editor) = pan_start();
    host.dispatch(
        &mut editor,
        CanvasEvent::PointerUp(PointerInput::left(
            Point::new(-40., -20.),
            HitTarget::Background,
        )),
    );
    host.dispatch(
        &mut editor,
        CanvasEvent::KeyUp {
            key: KeyCode::Space,
            modifiers: Modifiers::default(),
        },
    );
    assert!(!host.has_capture());
    assert_eq!(editor.viewport.pan, Point::new(205., 78.));
}
#[test]
fn escape_remains_an_explicit_rollback() {
    let (mut host, mut editor) = pan_start();
    host.dispatch(
        &mut editor,
        CanvasEvent::KeyDown {
            key: KeyCode::Escape,
            modifiers: Modifiers::default(),
        },
    );
    assert_eq!(editor.viewport.pan, Point::new(35., -12.));
    assert!(!host.has_capture());
}

//! Cross-operation contracts exercised by the two-SceneView native trace probe.
use gpui_flow_editor::SceneEditor;
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{
    DisplayObject, EventInput, EventKind, ListenerOptions, Point, Scene, display,
};
use std::{cell::RefCell, rc::Rc};

struct Lab {
    editor: SceneEditor,
    host: ScenePluginHost,
    router: SceneEventRouter,
}
impl Lab {
    fn new() -> Self {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("root", display::Group::new()),
                None,
                None,
            )
            .unwrap();
        for id in ["a", "b"] {
            scene
                .insert(
                    DisplayObject::new(id, display::Rect::new(80., 60.)),
                    Some("root".into()),
                    None,
                )
                .unwrap();
        }
        Self {
            editor: SceneEditor::new(scene).unwrap(),
            host: ScenePluginHost::with_defaults(),
            router: SceneEventRouter::new(),
        }
    }
    fn focus(&mut self, id: &str) -> RoutedReport {
        let handle = self.editor.engine.handle(&id.into()).unwrap();
        self.router
            .set_focus(&mut self.editor, &mut self.host, Some(handle))
            .unwrap()
    }
    fn send(&mut self, event: CanvasEvent) -> RoutedReport {
        self.router
            .dispatch(&mut self.editor, &mut self.host, event)
    }
}
fn key(down: bool, key: KeyCode) -> CanvasEvent {
    if down {
        CanvasEvent::KeyDown {
            key,
            modifiers: Modifiers::default(),
        }
    } else {
        CanvasEvent::KeyUp {
            key,
            modifiers: Modifiers::default(),
        }
    }
}

#[test]
fn same_focus_is_no_op_even_while_space_pan_capture_is_active() {
    let mut lab = Lab::new();
    lab.focus("a");
    lab.send(key(true, KeyCode::Space));
    lab.send(CanvasEvent::PointerDown(PointerInput::left(
        Point::new(10., 10.),
        HitTarget::Object("a".into()),
    )));
    assert_eq!(lab.host.capture_owner(), Some("pan"));
    let result = lab.focus("a");
    assert!(result.events.is_empty());
    assert!(result.errors.is_empty());
    assert!(lab.router.space_held());
    assert_eq!(lab.host.capture_owner(), Some("pan"));
    let repeat = lab.send(key(true, KeyCode::Space));
    assert!(matches!(
        repeat.events[0].input,
        Some(EventInput::Key { repeat: true, .. })
    ));
    lab.router.clear_focus(&mut lab.editor, &mut lab.host);
    assert!(!lab.router.space_held());
    assert!(!lab.host.has_capture());
    assert!(!lab.editor.editing());
}

#[test]
fn independent_routers_do_not_share_clear_focus_repeat_or_space_state() {
    let mut first = Lab::new();
    let mut second = Lab::new();
    first.focus("a");
    second.focus("b");
    first.send(key(true, KeyCode::Space));
    second.send(key(true, KeyCode::Space));
    first.router.clear_focus(&mut first.editor, &mut first.host);
    assert!(!first.router.space_held());
    assert!(second.router.space_held());
    let repeat = second.send(key(true, KeyCode::Space));
    assert!(matches!(
        repeat.events[0].input,
        Some(EventInput::Key { repeat: true, .. })
    ));
    let handle = second.editor.engine.handle(&"b".into()).unwrap();
    second
        .editor
        .engine
        .on(
            handle,
            EventKind::KeyUp,
            ListenerOptions::default(),
            |event| event.prevent_default(),
        )
        .unwrap();
    let up = second.send(key(false, KeyCode::Space));
    assert!(up.default.default_prevented);
    assert!(!second.router.space_held());
    assert!(first.send(key(false, KeyCode::Space)).events.is_empty());
}

#[test]
fn programmatic_transfer_restarts_repeat_and_cleans_space_before_new_target_key_up() {
    let mut lab = Lab::new();
    lab.focus("a");
    lab.send(key(true, KeyCode::Character('x')));
    lab.send(key(true, KeyCode::Space));
    let transfer = lab.focus("b");
    assert_eq!(
        transfer
            .events
            .iter()
            .map(|event| event.kind.clone())
            .collect::<Vec<_>>(),
        vec![EventKind::Blur, EventKind::Focus]
    );
    assert!(!lab.router.space_held());
    let up = lab.send(key(false, KeyCode::Space));
    assert_eq!(up.events[0].target.as_str(), "b");
    let down = lab.send(key(true, KeyCode::Character('x')));
    assert!(matches!(
        down.events[0].input,
        Some(EventInput::Key { repeat: false, .. })
    ));
    assert_eq!(down.events[0].target.as_str(), "b");
}

#[test]
fn focus_and_blur_each_route_capture_target_bubble_with_correct_related_target() {
    let mut lab = Lab::new();
    let observed = Rc::new(RefCell::new(Vec::new()));
    for kind in [EventKind::Focus, EventKind::Blur] {
        for (id, capture, label) in [
            ("root", true, "capture"),
            ("a", false, "target-a"),
            ("b", false, "target-b"),
            ("root", false, "bubble"),
        ] {
            let observed = observed.clone();
            lab.editor
                .engine
                .on(
                    lab.editor.engine.handle(&id.into()).unwrap(),
                    kind.clone(),
                    ListenerOptions {
                        capture,
                        once: false,
                    },
                    move |event| {
                        event.prevent_default();
                        observed.borrow_mut().push((
                            event.kind.clone(),
                            label,
                            event.target.to_string(),
                            event.related_target.as_ref().map(ToString::to_string),
                            event.cancelable,
                            event.default_prevented(),
                        ));
                    },
                )
                .unwrap();
        }
    }
    lab.focus("a");
    observed.borrow_mut().clear();
    lab.focus("b");
    assert_eq!(
        *observed.borrow(),
        vec![
            (
                EventKind::Blur,
                "capture",
                "a".into(),
                Some("b".into()),
                false,
                false
            ),
            (
                EventKind::Blur,
                "target-a",
                "a".into(),
                Some("b".into()),
                false,
                false
            ),
            (
                EventKind::Blur,
                "bubble",
                "a".into(),
                Some("b".into()),
                false,
                false
            ),
            (
                EventKind::Focus,
                "capture",
                "b".into(),
                Some("a".into()),
                false,
                false
            ),
            (
                EventKind::Focus,
                "target-b",
                "b".into(),
                Some("a".into()),
                false,
                false
            ),
            (
                EventKind::Focus,
                "bubble",
                "b".into(),
                Some("a".into()),
                false,
                false
            ),
        ]
    );
}

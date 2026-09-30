use gpui_flow_core::Point;
use gpui_flow_editor::SceneEditor;
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{
    Affine, DisplayObject, EventInput, EventKind, ListenerOptions, Scene, display,
};
use std::{cell::RefCell, rc::Rc};

fn setup() -> (SceneEditor, ScenePluginHost, SceneEventRouter) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("a", display::Rect::new(100., 100.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("b", display::Rect::new(100., 100.))
                .with_transform(Affine::translation(200., 0.)),
            None,
            None,
        )
        .unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    let mut host = ScenePluginHost::with_defaults();
    let mut router = SceneEventRouter::new();
    focus(&mut editor, &mut host, &mut router, "a", 20.);
    (editor, host, router)
}
fn pointer(id: &str, x: f32) -> PointerInput {
    PointerInput::left(Point::new(x, 20.), HitTarget::Object(id.into()))
}
fn focus(e: &mut SceneEditor, h: &mut ScenePluginHost, r: &mut SceneEventRouter, id: &str, x: f32) {
    r.dispatch(e, h, CanvasEvent::PointerDown(pointer(id, x)));
    r.dispatch(e, h, CanvasEvent::PointerUp(pointer(id, x)));
}
fn down(key: KeyCode) -> CanvasEvent {
    CanvasEvent::KeyDown {
        key,
        modifiers: Modifiers::default(),
    }
}
fn up(key: KeyCode) -> CanvasEvent {
    CanvasEvent::KeyUp {
        key,
        modifiers: Modifiers::default(),
    }
}
fn prevent(e: &mut SceneEditor, kind: EventKind, once: bool) {
    e.engine
        .on(
            e.engine.handle(&"a".into()).unwrap(),
            kind,
            ListenerOptions {
                once,
                ..Default::default()
            },
            |event| event.prevent_default(),
        )
        .unwrap();
}
#[test]
fn prevented_first_space_down_cannot_arm_pan_even_after_once_listener_expires() {
    let (mut e, mut h, mut r) = setup();
    prevent(&mut e, EventKind::KeyDown, true);
    let first = r.dispatch(&mut e, &mut h, down(KeyCode::Space));
    assert!(first.default.default_prevented);
    assert!(!r.space_held());
    r.dispatch(&mut e, &mut h, down(KeyCode::Space));
    assert!(
        !r.space_held(),
        "auto-repeat must not bypass the prevented initial press"
    );
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(pointer("b", 220.)));
    assert_ne!(h.capture_owner(), Some("pan"));
    assert!(e.selection.ids.contains(&"b".into()));
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerCancel);
    r.dispatch(&mut e, &mut h, up(KeyCode::Space));
    r.dispatch(&mut e, &mut h, down(KeyCode::Space));
    assert!(r.space_held());
}
#[test]
fn accepted_space_pan_preserves_selection_and_object_geometry() {
    let (mut e, mut h, mut r) = setup();
    let scene = e.scene().clone();
    let selected = e.selection.clone();
    r.dispatch(&mut e, &mut h, down(KeyCode::Space));
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(pointer("b", 220.)));
    assert_eq!(h.capture_owner(), Some("pan"));
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(pointer("b", 270.)));
    assert_eq!(e.viewport.pan.x, 50.);
    assert_eq!(e.scene(), &scene);
    assert_eq!(e.selection, selected);
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerUp(pointer("b", 270.)));
    assert!(!h.has_capture());
}
#[test]
fn key_up_reaches_tree_and_cannot_prevent_space_release() {
    let (mut e, mut h, mut r) = setup();
    prevent(&mut e, EventKind::KeyUp, false);
    r.dispatch(&mut e, &mut h, down(KeyCode::Space));
    assert!(r.space_held());
    let out = r.dispatch(&mut e, &mut h, up(KeyCode::Space));
    assert!(out.default.default_prevented);
    assert!(!r.space_held());
    let key = out
        .events
        .iter()
        .find(|event| event.kind == EventKind::KeyUp)
        .unwrap();
    assert_eq!(key.target.as_str(), "a");
    assert!(matches!(&key.input,Some(EventInput::Key {key,repeat:false,..}) if key=="Space"));
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(pointer("b", 220.)));
    assert_ne!(h.capture_owner(), Some("pan"));
}
#[test]
fn repeat_metadata_is_scoped_and_resets_after_key_up() {
    let (mut e, mut h, mut r) = setup();
    for (event, expected) in [
        (down(KeyCode::Character('a')), false),
        (down(KeyCode::Character('a')), true),
        (up(KeyCode::Character('a')), false),
        (down(KeyCode::Character('a')), false),
    ] {
        let out = r.dispatch(&mut e, &mut h, event);
        assert!(
            matches!(&out.events[0].input,Some(EventInput::Key {repeat,..}) if *repeat==expected)
        );
    }
    let (mut other, mut other_host, mut other_router) = setup();
    let out = other_router.dispatch(&mut other, &mut other_host, down(KeyCode::Character('a')));
    assert!(matches!(
        &out.events[0].input,
        Some(EventInput::Key { repeat: false, .. })
    ));
}
#[test]
fn key_up_capture_target_bubble_order_and_modifiers_are_preserved() {
    let (mut e, mut h, mut r) = setup();
    e.transaction(|scene| {
        scene.insert(
            DisplayObject::new("parent", display::Group::new()),
            None,
            None,
        )?;
        scene.reparent(&"a".into(), Some("parent".into()), None, false)
    })
    .unwrap();
    focus(&mut e, &mut h, &mut r, "a", 20.);
    let order = Rc::new(RefCell::new(Vec::new()));
    for (id, capture, label) in [
        ("parent", true, "parent capture"),
        ("a", true, "target capture"),
        ("a", false, "target bubble"),
        ("parent", false, "parent bubble"),
    ] {
        let order = order.clone();
        e.engine
            .on(
                e.engine.handle(&id.into()).unwrap(),
                EventKind::KeyUp,
                ListenerOptions {
                    capture,
                    once: false,
                },
                move |_| order.borrow_mut().push(label),
            )
            .unwrap();
    }
    let out = r.dispatch(
        &mut e,
        &mut h,
        CanvasEvent::KeyUp {
            key: KeyCode::Character('z'),
            modifiers: Modifiers {
                control: true,
                shift: true,
                ..Default::default()
            },
        },
    );
    assert_eq!(
        *order.borrow(),
        vec![
            "parent capture",
            "target capture",
            "target bubble",
            "parent bubble"
        ]
    );
    assert!(
        matches!(&out.events[0].input,Some(EventInput::Key {modifiers,..}) if modifiers.control&&modifiers.shift)
    );
}
#[test]
fn key_up_listener_once_off_and_queued_mutation_keep_normal_contracts() {
    let (mut e, mut h, mut r) = setup();
    let handle = e.engine.handle(&"a".into()).unwrap();
    let sender = r.action_sender();
    let token = e
        .engine
        .on(
            handle,
            EventKind::KeyUp,
            ListenerOptions {
                once: true,
                ..Default::default()
            },
            move |_| {
                sender
                    .enqueue(
                        handle,
                        SceneCommand::SetObjectData {
                            id: "a".into(),
                            data: serde_json::json!({"released":true}),
                        },
                    )
                    .unwrap();
            },
        )
        .unwrap();
    assert_eq!(e.engine.listener_count(), 1);
    r.dispatch(&mut e, &mut h, up(KeyCode::Character('x')));
    assert_eq!(
        e.scene().get(&"a".into()).unwrap().object.data["released"],
        true
    );
    assert_eq!(e.engine.listener_count(), 0);
    assert!(!e.engine.off(token));
    assert!(e.undo());
    r.dispatch(&mut e, &mut h, up(KeyCode::Character('x')));
    assert!(e.scene().get(&"a".into()).unwrap().object.data.is_null());
}
#[test]
fn queued_key_up_edits_are_still_denied_in_read_only_mode() {
    let (mut e, mut h, mut r) = setup();
    let handle = e.engine.handle(&"a".into()).unwrap();
    let sender = r.action_sender();
    e.engine
        .on(
            handle,
            EventKind::KeyUp,
            ListenerOptions::default(),
            move |_| {
                sender
                    .enqueue(handle, SceneCommand::DeleteSelection)
                    .unwrap();
            },
        )
        .unwrap();
    h.set_editable(false, &mut e);
    let out = r.dispatch(&mut e, &mut h, up(KeyCode::Delete));
    assert!(!out.errors.is_empty());
    assert!(e.scene().get(&"a".into()).is_some());
}
#[test]
fn all_terminal_paths_clear_keyboard_state_and_pointer_capture_despite_prevention() {
    for terminal in [
        CanvasEvent::PointerCancel,
        CanvasEvent::FocusLost,
        down(KeyCode::Escape),
    ] {
        let (mut e, mut h, mut r) = setup();
        r.dispatch(&mut e, &mut h, down(KeyCode::Space));
        r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(pointer("a", 20.)));
        assert!(h.has_capture());
        prevent(&mut e, EventKind::PointerCancel, false);
        prevent(&mut e, EventKind::KeyDown, false);
        r.dispatch(&mut e, &mut h, terminal);
        assert!(!h.has_capture());
        assert!(!e.editing());
        assert!(!r.space_held());
    }
}
#[test]
fn focused_instance_reset_never_retargets_old_keyboard_listeners_or_space() {
    let (mut e, mut h, mut r) = setup();
    let calls = Rc::new(RefCell::new(0));
    let called = calls.clone();
    e.engine
        .on(
            e.engine.handle(&"a".into()).unwrap(),
            EventKind::KeyUp,
            ListenerOptions::default(),
            move |_| *called.borrow_mut() += 1,
        )
        .unwrap();
    r.dispatch(&mut e, &mut h, down(KeyCode::Space));
    e.load_scene(e.scene().clone()).unwrap();
    let out = r.dispatch(&mut e, &mut h, up(KeyCode::Space));
    assert!(out.events.is_empty());
    assert!(!r.space_held());
    assert_eq!(*calls.borrow(), 0);
    assert_eq!(e.engine.listener_count(), 0);
}
#[test]
fn detach_suspends_focus_but_retains_listener_for_explicit_refocus_after_attach() {
    let (mut e, mut h, mut r) = setup();
    let calls = Rc::new(RefCell::new(0));
    let called = calls.clone();
    let handle = e.engine.handle(&"a".into()).unwrap();
    e.engine
        .on(
            handle,
            EventKind::KeyUp,
            ListenerOptions::default(),
            move |_| *called.borrow_mut() += 1,
        )
        .unwrap();
    r.dispatch(&mut e, &mut h, down(KeyCode::Space));
    let (tree, _) = e.engine.detach(handle).unwrap();
    r.dispatch(&mut e, &mut h, up(KeyCode::Space));
    assert!(!r.space_held());
    assert_eq!(*calls.borrow(), 0);
    assert_eq!(e.engine.listener_count(), 1);
    e.engine.attach(&tree, None, None).unwrap();
    r.dispatch(&mut e, &mut h, up(KeyCode::Character('a')));
    assert_eq!(*calls.borrow(), 0);
    focus(&mut e, &mut h, &mut r, "a", 20.);
    r.dispatch(&mut e, &mut h, up(KeyCode::Character('a')));
    assert_eq!(*calls.borrow(), 1);
}
#[test]
fn prevented_key_up_during_drag_does_not_cancel_valid_pointer_gesture() {
    let (mut e, mut h, mut r) = setup();
    prevent(&mut e, EventKind::KeyUp, false);
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(pointer("a", 20.)));
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(pointer("a", 45.)));
    assert!(e.editing());
    r.dispatch(&mut e, &mut h, up(KeyCode::Character('x')));
    assert!(e.editing());
    assert!(h.has_capture());
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerCancel);
    assert!(!e.editing());
    assert_eq!(e.scene().get(&"a".into()).unwrap().object.transform.tx, 0.);
}

#[test]
fn keyboard_listener_destroying_its_own_focus_clears_space_before_dispatch_returns() {
    let (mut e, mut h, mut r) = setup();
    let handle = e.engine.handle(&"a".into()).unwrap();
    let sender = r.action_sender();
    e.engine
        .on(
            handle,
            EventKind::KeyDown,
            ListenerOptions::default(),
            move |_| {
                sender
                    .enqueue(handle, SceneCommand::DeleteSelection)
                    .unwrap();
            },
        )
        .unwrap();
    let out = r.dispatch(&mut e, &mut h, down(KeyCode::Space));
    assert!(out.errors.is_empty());
    assert!(e.scene().get(&"a".into()).is_none());
    assert!(!r.space_held());
    assert_eq!(e.engine.listener_count(), 0);
    assert!(
        r.dispatch(&mut e, &mut h, up(KeyCode::Space))
            .events
            .is_empty()
    );
}

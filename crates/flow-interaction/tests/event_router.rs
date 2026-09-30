use gpui_flow_core::Point;
use gpui_flow_editor::SceneEditor;
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{DisplayObject, EventKind, ListenerOptions, Scene, display};
fn setup() -> (SceneEditor, ScenePluginHost, SceneEventRouter) {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("a", display::Rect::new(100., 100.)),
        None,
        None,
    )
    .unwrap();
    (
        SceneEditor::new(s).unwrap(),
        ScenePluginHost::with_defaults(),
        SceneEventRouter::new(),
    )
}
fn pointer(x: f32) -> PointerInput {
    PointerInput::left(Point::new(x, 20.), HitTarget::Object("a".into()))
}
fn prevent(e: &mut SceneEditor, kind: EventKind) {
    let h = e.engine.handle(&"a".into()).unwrap();
    e.engine
        .on(h, kind, ListenerOptions::default(), |event| {
            event.prevent_default()
        })
        .unwrap();
}
#[test]
fn prevented_down_blocks_selection_and_capture() {
    let (mut e, mut h, mut r) = setup();
    prevent(&mut e, EventKind::PointerDown);
    let out = r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(pointer(20.)));
    assert!(out.default.default_prevented);
    assert!(e.selection.ids.is_empty());
    assert!(!h.has_capture());
}
#[test]
fn prevented_move_runs_before_mutation() {
    let (mut e, mut h, mut r) = setup();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(pointer(20.)));
    prevent(&mut e, EventKind::PointerMove);
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(pointer(70.)));
    assert_eq!(e.scene().get(&"a".into()).unwrap().object.transform.tx, 0.);
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerCancel);
    assert!(!e.editing());
}
#[test]
fn prevented_release_rolls_back_and_cannot_strand_capture() {
    let (mut e, mut h, mut r) = setup();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(pointer(20.)));
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(pointer(70.)));
    assert_ne!(e.scene().get(&"a".into()).unwrap().object.transform.tx, 0.);
    prevent(&mut e, EventKind::PointerUp);
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerUp(pointer(70.)));
    assert_eq!(e.scene().get(&"a".into()).unwrap().object.transform.tx, 0.);
    assert!(!e.editing());
    assert!(!h.has_capture());
}
#[test]
fn key_and_wheel_defaults_are_cancelable() {
    let (mut e, mut h, mut r) = setup();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(pointer(20.)));
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerUp(pointer(20.)));
    prevent(&mut e, EventKind::KeyDown);
    prevent(&mut e, EventKind::Wheel);
    r.dispatch(
        &mut e,
        &mut h,
        CanvasEvent::KeyDown {
            key: KeyCode::Delete,
            modifiers: Modifiers::default(),
        },
    );
    assert!(e.scene().get(&"a".into()).is_some());
    let old = e.viewport;
    r.dispatch(
        &mut e,
        &mut h,
        CanvasEvent::Wheel {
            position: Point::new(20., 20.),
            delta: Point::new(0., 40.),
            modifiers: Modifiers {
                control: true,
                ..Default::default()
            },
        },
    );
    assert_eq!(old, e.viewport);
}
#[test]
fn reset_document_does_not_retarget_old_capture_or_focus() {
    let (mut e, mut h, mut r) = setup();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(pointer(20.)));
    let scene = e.scene().clone();
    e.load_scene(scene).unwrap();
    let out = r.dispatch(&mut e, &mut h, CanvasEvent::PointerUp(pointer(20.)));
    assert!(
        out.events
            .iter()
            .all(|e| e.kind != EventKind::Click && e.kind != EventKind::PointerUp)
    );
}
#[test]
fn listener_actions_run_after_dispatch_and_obey_read_only() {
    let (mut e, mut h, mut r) = setup();
    let handle = e.engine.handle(&"a".into()).unwrap();
    let sender = r.action_sender();
    e.engine
        .on(
            handle,
            EventKind::Click,
            ListenerOptions::default(),
            move |_| {
                sender
                    .enqueue(
                        handle,
                        SceneCommand::SetObjectData {
                            id: "a".into(),
                            data: serde_json::json!({"clicked":true}),
                        },
                    )
                    .unwrap();
            },
        )
        .unwrap();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(pointer(20.)));
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerUp(pointer(20.)));
    assert_eq!(
        e.scene().get(&"a".into()).unwrap().object.data["clicked"],
        true
    );
    assert!(e.undo());
    h.set_editable(false, &mut e);
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(pointer(20.)));
    let out = r.dispatch(&mut e, &mut h, CanvasEvent::PointerUp(pointer(20.)));
    assert!(!out.errors.is_empty());
    assert!(e.scene().get(&"a".into()).unwrap().object.data.is_null());
}
#[test]
fn action_queue_is_bounded_and_stale_identity_rejected() {
    let (mut e, mut h, mut r) = setup();
    let handle = e.engine.handle(&"a".into()).unwrap();
    let sender = r.action_sender();
    for _ in 0..256 {
        sender
            .enqueue(handle, SceneCommand::ClearSelection)
            .unwrap();
    }
    assert_eq!(
        sender.enqueue(handle, SceneCommand::ClearSelection),
        Err(ActionQueueError::Full)
    );
    let scene = e.scene().clone();
    e.load_scene(scene).unwrap();
    let out = r.dispatch(&mut e, &mut h, CanvasEvent::PointerCancel);
    assert_eq!(out.errors.len(), 256);
}
#[test]
fn secondary_pointer_cannot_move_or_release_primary_gesture() {
    let (mut e, mut h, mut r) = setup();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(pointer(20.)));
    let mut p = pointer(80.);
    p.pointer_id = 2;
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(p.clone()));
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerUp(p));
    assert!(h.has_capture());
    assert_eq!(e.scene().get(&"a".into()).unwrap().object.transform.tx, 0.);
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerCancel);
    assert!(!h.has_capture());
}
#[test]
fn external_destroy_during_drag_never_resurrects_or_strands_pending() {
    let (mut e, mut h, mut r) = setup();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(pointer(20.)));
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(pointer(70.)));
    let handle = e.engine.handle(&"a".into()).unwrap();
    e.engine.destroy(handle).unwrap();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerCancel);
    assert!(e.scene().get(&"a".into()).is_none());
    assert!(!e.editing());
    assert!(!h.has_capture());
    assert!(!e.undo());
}
#[test]
fn external_detach_during_drag_never_strands_pending() {
    let (mut e, mut h, mut r) = setup();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(pointer(20.)));
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(pointer(70.)));
    let handle = e.engine.handle(&"a".into()).unwrap();
    let _token = e.engine.detach(handle).unwrap();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerCancel);
    assert!(!e.editing());
    assert!(!h.has_capture());
    assert!(e.begin_edit());
}

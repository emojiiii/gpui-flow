use gpui_flow_editor::SceneEditor;
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{
    DisplayObject, EventInput, EventKind, ListenerOptions, Point, Scene, SemanticAction,
    SemanticSource, Size, display,
};
use std::{cell::RefCell, rc::Rc};

fn fixture() -> (SceneEditor, ScenePluginHost, SceneEventRouter) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Group::new()),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("text", display::Text::new("Hello", Size::new(80., 30.))),
            Some("root".into()),
            None,
        )
        .unwrap();
    (
        SceneEditor::new(scene).unwrap(),
        ScenePluginHost::with_defaults(),
        SceneEventRouter::new(),
    )
}
#[test]
fn semantic_metadata_is_non_pointer_after_a_real_timed_pointer_and_focus_default_never_selects() {
    let (mut e, mut h, mut r) = fixture();
    let target = e.engine.handle(&"text".into()).unwrap();
    let mut pointer = PointerInput::left(Point::new(12., 19.), HitTarget::Object("text".into()));
    pointer.timestamp_millis = Some(4321);
    pointer.pressure = Some(0.7);
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(pointer));
    let selection = e.selection.clone();
    let dispatch = r
        .dispatch_semantic(
            &mut e,
            &mut h,
            target,
            SemanticAction::Focus,
            SemanticSource::AssistiveTechnology,
        )
        .unwrap();
    assert_eq!(dispatch.report.events.len(), 1);
    let event = &dispatch.report.events[0];
    assert_eq!(event.kind, EventKind::SemanticAction);
    assert_eq!(
        event.input,
        Some(EventInput::Semantic {
            action: SemanticAction::Focus,
            source: SemanticSource::AssistiveTechnology
        })
    );
    assert_eq!(event.world_position, None);
    assert_eq!(event.local_position, None);
    assert_eq!(event.timestamp_millis, None);
    assert!(event.cancelable);
    let focus = r
        .set_focus_after_semantic(&mut e, &mut h, dispatch.default.unwrap())
        .unwrap();
    assert_eq!(r.focused(&e.engine), Some(target));
    assert!(
        focus
            .events
            .iter()
            .all(|event| event.timestamp_millis.is_none())
    );
    assert_eq!(e.selection, selection);
    assert!(!e.undo());
}
#[test]
fn capture_target_bubble_can_cancel_but_queued_cleanup_still_runs() {
    let (mut e, mut h, mut r) = fixture();
    let target = e.engine.handle(&"text".into()).unwrap();
    let root = e.engine.handle(&"root".into()).unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    for (handle, capture, label) in [
        (root, true, "capture"),
        (target, false, "target"),
        (root, false, "bubble"),
    ] {
        let log = log.clone();
        let sender = r.action_sender();
        e.engine
            .on(
                handle,
                EventKind::SemanticAction,
                ListenerOptions {
                    capture,
                    once: false,
                },
                move |event| {
                    log.borrow_mut().push(label);
                    if label == "target" {
                        event.prevent_default();
                        sender
                            .enqueue(
                                target,
                                SceneCommand::SetObjectData {
                                    id: "text".into(),
                                    data: serde_json::json!({"seen": true}),
                                },
                            )
                            .unwrap();
                    }
                },
            )
            .unwrap();
    }
    let dispatch = r
        .dispatch_semantic(
            &mut e,
            &mut h,
            target,
            SemanticAction::OpenText,
            SemanticSource::Keyboard,
        )
        .unwrap();
    assert_eq!(*log.borrow(), ["capture", "target", "bubble"]);
    assert!(dispatch.default.is_none());
    assert!(dispatch.report.default.default_prevented);
    assert_eq!(
        e.engine.object(target).unwrap().data,
        serde_json::json!({"seen": true})
    );
    assert_eq!(r.focused(&e.engine), None);
}
#[test]
fn queued_document_replacement_revokes_the_semantic_default() {
    let (mut e, mut h, mut r) = fixture();
    let target = e.engine.handle(&"text".into()).unwrap();
    let replacement = e.scene().clone();
    let sender = r.action_sender();
    e.engine
        .on(
            target,
            EventKind::SemanticAction,
            ListenerOptions::default(),
            move |_| {
                sender
                    .enqueue(target, SceneCommand::LoadScene(replacement.clone()))
                    .unwrap();
            },
        )
        .unwrap();
    let result = r
        .dispatch_semantic(
            &mut e,
            &mut h,
            target,
            SemanticAction::OpenText,
            SemanticSource::AssistiveTechnology,
        )
        .unwrap();
    assert!(result.default.is_none());
    assert!(result.report.default.default_prevented);
    assert!(
        result
            .report
            .errors
            .iter()
            .any(|error| error.contains("invalidated"))
    );
}
#[test]
fn single_use_default_rejects_detach_reattach_and_document_reset_between_stages() {
    for reset in [false, true] {
        let (mut e, mut h, mut r) = fixture();
        let target = e.engine.handle(&"text".into()).unwrap();
        let permit = r
            .dispatch_semantic(
                &mut e,
                &mut h,
                target,
                SemanticAction::Focus,
                SemanticSource::Keyboard,
            )
            .unwrap()
            .default
            .unwrap();
        if reset {
            e.engine.reset_scene(e.scene().clone()).unwrap();
        } else {
            let (tree, _) = e.engine.detach(target).unwrap();
            let root = e.engine.handle(&"root".into()).unwrap();
            e.engine.attach(&tree, Some(root), None).unwrap();
        }
        assert!(r.set_focus_after_semantic(&mut e, &mut h, permit).is_err());
        assert_eq!(r.focused(&e.engine), None);
    }
}
#[test]
fn semantic_listener_and_default_focus_share_one_256_action_budget() {
    let (mut e, mut h, mut r) = fixture();
    let target = e.engine.handle(&"text".into()).unwrap();
    let root = e.engine.handle(&"root".into()).unwrap();
    let sender = r.action_sender();
    e.engine
        .on(
            target,
            EventKind::SemanticAction,
            ListenerOptions::default(),
            move |_| {
                for _ in 0..255 {
                    sender
                        .enqueue(target, SceneCommand::ClearSelection)
                        .unwrap();
                }
            },
        )
        .unwrap();
    let sender = r.action_sender();
    let redirect = SceneFocusRequest::new(&e.engine, target, Some(root)).unwrap();
    e.engine
        .on(
            target,
            EventKind::Focus,
            ListenerOptions::default(),
            move |_| {
                sender.enqueue_focus(redirect).unwrap();
            },
        )
        .unwrap();
    let sender = r.action_sender();
    let redirect = SceneFocusRequest::new(&e.engine, root, Some(target)).unwrap();
    e.engine
        .on(
            root,
            EventKind::Focus,
            ListenerOptions::default(),
            move |_| {
                // At the exhausted boundary the queue refuses more focus redirects.
                let _ = sender.enqueue_focus(redirect);
            },
        )
        .unwrap();
    let dispatch = r
        .dispatch_semantic(
            &mut e,
            &mut h,
            target,
            SemanticAction::Focus,
            SemanticSource::Keyboard,
        )
        .unwrap();
    let report = r
        .set_focus_after_semantic(&mut e, &mut h, dispatch.default.unwrap())
        .unwrap();
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("budget exhausted"))
    );
    assert_eq!(r.focused(&e.engine), Some(root));
}

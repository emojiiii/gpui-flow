use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ElementHandle, EngineError};
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{
    Affine, Color, DisplayObject, EventInput, EventKind, ListenerOptions, Point, Scene, Style,
    display,
};
use std::{cell::RefCell, rc::Rc};
struct Lab {
    e: SceneEditor,
    h: ScenePluginHost,
    r: SceneEventRouter,
    a: ElementHandle,
    b: ElementHandle,
}
impl Lab {
    fn new() -> Self {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("root", display::Frame::new(500., 300.)),
                None,
                None,
            )
            .unwrap();
        for (id, x) in [("a", 0.), ("b", 200.)] {
            scene
                .insert(
                    DisplayObject::new(id, display::Rect::new(80., 60.))
                        .with_transform(Affine::translation(x, 0.))
                        .with_style(Style::fill(Color::BLACK)),
                    Some("root".into()),
                    None,
                )
                .unwrap();
        }
        let e = SceneEditor::new(scene).unwrap();
        let a = e.engine.handle(&"a".into()).unwrap();
        let b = e.engine.handle(&"b".into()).unwrap();
        Self {
            e,
            h: ScenePluginHost::with_defaults(),
            r: SceneEventRouter::new(),
            a,
            b,
        }
    }
    fn focus(&mut self, h: ElementHandle) -> RoutedReport {
        self.r.set_focus(&mut self.e, &mut self.h, Some(h)).unwrap()
    }
    fn event(&mut self, event: CanvasEvent) -> RoutedReport {
        self.r.dispatch(&mut self.e, &mut self.h, event)
    }
}
fn pointer(id: &str, x: f32) -> PointerInput {
    PointerInput::left(Point::new(x, 20.), HitTarget::Object(id.into()))
}
fn key(key: KeyCode) -> CanvasEvent {
    CanvasEvent::KeyDown {
        key,
        modifiers: Modifiers::default(),
    }
}
#[test]
fn focus_transfer_is_blur_then_focus_with_related_targets_and_no_selection_or_history() {
    let mut l = Lab::new();
    let scene = l.e.scene().clone();
    let revision = l.e.engine.revision();
    l.e.selection.ids.insert("a".into());
    let selection = l.e.selection.clone();
    let first = l.focus(l.a);
    assert_eq!(first.events.len(), 1);
    assert_eq!(first.events[0].kind, EventKind::Focus);
    assert_eq!(first.events[0].related_target, None);
    let next = l.focus(l.b);
    assert_eq!(next.events.len(), 2);
    for (event, kind, target, related) in [
        (&next.events[0], EventKind::Blur, "a", "b"),
        (&next.events[1], EventKind::Focus, "b", "a"),
    ] {
        assert_eq!(event.kind, kind);
        assert_eq!(event.target.as_str(), target);
        assert_eq!(event.related_target.as_ref().unwrap().as_str(), related);
        assert!(!event.cancelable);
        assert!(event.world_position.is_none());
        assert!(event.input.is_none());
    }
    assert!(next.events[0].sequence < next.events[1].sequence);
    assert_eq!(l.r.focused(&l.e.engine), Some(l.b));
    assert_eq!(l.e.selection, selection);
    assert_eq!(l.e.scene(), &scene);
    assert_eq!(l.e.engine.revision(), revision);
    assert!(!l.e.undo());
}
#[test]
fn focus_notifications_cannot_veto_but_propagation_still_works() {
    let mut l = Lab::new();
    l.focus(l.a);
    let trace = Rc::new(RefCell::new(vec![]));
    let root = l.e.engine.handle(&"root".into()).unwrap();
    for (handle, kind, capture, label) in [
        (root, EventKind::Focus, true, "capture"),
        (l.b, EventKind::Focus, false, "target"),
        (root, EventKind::Focus, false, "bubble"),
    ] {
        let trace = trace.clone();
        l.e.engine
            .on(
                handle,
                kind,
                ListenerOptions {
                    capture,
                    once: false,
                },
                move |event| {
                    trace.borrow_mut().push(label);
                    event.prevent_default();
                },
            )
            .unwrap();
    }
    l.e.engine
        .on(l.a, EventKind::Blur, ListenerOptions::default(), |event| {
            event.prevent_default()
        })
        .unwrap();
    let report = l.focus(l.b);
    assert_eq!(*trace.borrow(), vec!["capture", "target", "bubble"]);
    assert!(report.events.iter().all(|e| !e.default_prevented()));
    assert_eq!(l.r.focused(&l.e.engine), Some(l.b));
}
#[test]
fn same_live_focus_is_idempotent_and_preserves_current_key_latch() {
    let mut l = Lab::new();
    l.focus(l.a);
    l.event(key(KeyCode::Space));
    assert!(l.r.space_held());
    assert!(l.focus(l.a).events.is_empty());
    assert!(l.r.space_held());
    l.r.clear_focus(&mut l.e, &mut l.h);
    assert!(!l.r.space_held());
    assert_eq!(l.r.focused(&l.e.engine), None);
    assert!(l.r.clear_focus(&mut l.e, &mut l.h).events.is_empty());
}
#[test]
fn invalid_detached_foreign_or_destroyed_request_preserves_previous_owner_and_keys() {
    let mut l = Lab::new();
    l.focus(l.a);
    l.event(key(KeyCode::Space));
    let other = Lab::new();
    assert!(matches!(
        l.r.set_focus(&mut l.e, &mut l.h, Some(other.a)),
        Err(EngineError::InvalidHandle)
    ));
    let (tree, _) = l.e.engine.detach(l.b).unwrap();
    assert!(matches!(
        l.r.set_focus(&mut l.e, &mut l.h, Some(l.b)),
        Err(EngineError::DetachedHandle)
    ));
    l.e.engine.attach(&tree, None, None).unwrap();
    l.e.engine.destroy(l.b).unwrap();
    assert!(matches!(
        l.r.set_focus(&mut l.e, &mut l.h, Some(l.b)),
        Err(EngineError::InvalidHandle)
    ));
    assert_eq!(l.r.focused(&l.e.engine), Some(l.a));
    assert!(l.r.space_held());
}
#[test]
fn keyboard_delivery_and_repeat_restart_at_the_new_owner() {
    let mut l = Lab::new();
    l.focus(l.a);
    l.event(key(KeyCode::Character('x')));
    l.event(key(KeyCode::Space));
    l.focus(l.b);
    assert!(!l.r.space_held());
    let report = l.event(key(KeyCode::Character('x')));
    let event = report
        .events
        .iter()
        .find(|e| e.kind == EventKind::KeyDown)
        .unwrap();
    assert_eq!(event.target.as_str(), "b");
    assert!(matches!(
        event.input,
        Some(EventInput::Key { repeat: false, .. })
    ));
    let report = l.event(CanvasEvent::KeyUp {
        key: KeyCode::Space,
        modifiers: Modifiers::default(),
    });
    assert_eq!(report.events[0].target.as_str(), "b");
    assert!(!l.r.space_held());
}
#[test]
fn programmatic_transfer_cancels_old_drag_retains_hover_and_never_selects_new_target() {
    let mut l = Lab::new();
    l.event(CanvasEvent::PointerMove(pointer("a", 20.)));
    l.event(CanvasEvent::PointerDown(pointer("a", 20.)));
    l.event(CanvasEvent::PointerMove(pointer("a", 70.)));
    assert!(l.h.has_capture());
    assert_ne!(l.e.engine.object(l.a).unwrap().transform.tx, 0.);
    let report = l.focus(l.b);
    assert!(
        report
            .events
            .iter()
            .any(|e| e.kind == EventKind::PointerCancel)
    );
    assert!(!l.h.has_capture());
    assert!(!l.r.has_pressed(&l.e.engine));
    assert_eq!(l.e.engine.object(l.a).unwrap().transform.tx, 0.);
    assert!(l.e.selection.ids.contains(&"a".into()));
    assert!(!l.e.selection.ids.contains(&"b".into()));
    assert!(l.r.state(&l.e.engine, l.a).hover);
    assert!(l.r.state(&l.e.engine, l.b).focus);
    assert!(!l.e.undo());
}
#[test]
fn prevented_pointer_down_still_blocks_click_focus_transfer() {
    let mut l = Lab::new();
    l.focus(l.a);
    l.e.engine
        .on(
            l.b,
            EventKind::PointerDown,
            ListenerOptions::default(),
            |e| e.prevent_default(),
        )
        .unwrap();
    let report = l.event(CanvasEvent::PointerDown(pointer("b", 220.)));
    assert!(report.default.default_prevented);
    assert_eq!(l.r.focused(&l.e.engine), Some(l.a));
    assert!(
        report
            .events
            .iter()
            .all(|e| e.kind != EventKind::Blur && e.kind != EventKind::Focus)
    );
}
#[test]
fn focus_style_transitions_follow_programmatic_focus_without_touching_undo() {
    let mut l = Lab::new();
    l.e.transaction(|s| s.update_object(&"a".into(), |o| o.name = "edited".into()))
        .unwrap();
    let mut styles = SceneStateStyles::new(&l.e.engine);
    styles
        .bind(
            &l.e.engine,
            l.a,
            StateStyles {
                focus: StatePaint::fill(Color::WHITE),
                ..Default::default()
            },
        )
        .unwrap();
    styles
        .set_transitions(
            &l.e.engine,
            l.a,
            StateTransitions {
                transition: StateTransition::new(100),
                transition_out: None,
            },
        )
        .unwrap();
    l.focus(l.a);
    styles
        .presentation_at(&l.e, &l.r, std::time::Duration::ZERO)
        .unwrap();
    let shown = styles
        .presentation_at(&l.e, &l.r, std::time::Duration::from_millis(50))
        .unwrap();
    assert_eq!(
        shown.get(&"a".into()).unwrap().object.style.fill,
        Some(Color::rgb(128, 128, 128).into())
    );
    assert_eq!(
        l.e.engine.object(l.a).unwrap().style.fill,
        Some(Color::BLACK.into())
    );
    l.focus(l.b);
    assert!(l.e.undo());
    assert!(!l.e.undo());
}
#[test]
fn detach_and_reattach_between_events_does_not_revive_focus_but_explicit_new_request_can() {
    let mut l = Lab::new();
    l.focus(l.a);
    l.event(key(KeyCode::Space));
    let (tree, _) = l.e.engine.detach(l.a).unwrap();
    l.e.engine
        .attach(&tree, l.e.engine.handle(&"root".into()), None)
        .unwrap();
    assert_eq!(l.r.focused(&l.e.engine), None);
    assert!(!l.r.state(&l.e.engine, l.a).focus);
    let report = l.event(key(KeyCode::Character('x')));
    assert!(report.events.is_empty());
    assert!(!l.r.space_held());
    let report = l.focus(l.a);
    assert_eq!(report.events.len(), 1);
    assert_eq!(report.events[0].kind, EventKind::Focus);
}
#[test]
fn destroyed_and_reset_ids_never_inherit_focus_or_callbacks() {
    for reset in [false, true] {
        let mut l = Lab::new();
        l.focus(l.a);
        l.event(key(KeyCode::Space));
        if reset {
            l.e.engine.reset_scene(l.e.scene().clone()).unwrap();
        } else {
            l.e.engine.destroy(l.a).unwrap();
            l.e.engine
                .create("rect", "a", serde_json::json!({}), None, None)
                .unwrap();
        }
        assert_eq!(l.r.focused(&l.e.engine), None);
        assert!(l.r.set_focus(&mut l.e, &mut l.h, Some(l.a)).is_err());
        let fresh = l.e.engine.handle(&"a".into()).unwrap();
        assert_ne!(fresh, l.a);
        let report = l.focus(fresh);
        assert_eq!(report.events.len(), 1);
        assert_eq!(report.events[0].kind, EventKind::Focus);
        assert!(!l.r.space_held());
    }
}
#[test]
fn focus_listener_delete_is_drained_and_new_target_cannot_receive_later_keys() {
    let mut l = Lab::new();
    l.e.selection.ids.insert("b".into());
    let sender = l.r.action_sender();
    let origin = l.b;
    l.e.engine
        .on(
            l.b,
            EventKind::Focus,
            ListenerOptions {
                once: true,
                ..Default::default()
            },
            move |_| {
                sender
                    .enqueue(origin, SceneCommand::DeleteSelection)
                    .unwrap();
            },
        )
        .unwrap();
    let report = l.focus(l.b);
    assert!(report.default.document_changed);
    assert_eq!(l.r.focused(&l.e.engine), None);
    assert!(l.event(key(KeyCode::Character('x'))).events.is_empty());
    assert!(l.e.undo());
    assert_ne!(l.e.engine.handle(&"b".into()).unwrap(), l.b);
    assert_eq!(l.r.focused(&l.e.engine), None);
}
#[test]
fn blur_listener_reset_retires_both_handles_before_obsolete_focus_can_emit() {
    let mut l = Lab::new();
    l.focus(l.a);
    let sender = l.r.action_sender();
    let origin = l.a;
    let scene = l.e.scene().clone();
    l.e.engine
        .on(
            l.a,
            EventKind::Blur,
            ListenerOptions::default(),
            move |_| {
                sender
                    .enqueue(origin, SceneCommand::LoadScene(scene.clone()))
                    .unwrap();
            },
        )
        .unwrap();
    let report = l.focus(l.b);
    assert_eq!(
        report
            .events
            .iter()
            .map(|e| e.kind.clone())
            .collect::<Vec<_>>(),
        vec![EventKind::Blur]
    );
    assert_eq!(l.r.focused(&l.e.engine), None);
    assert_ne!(l.e.engine.handle(&"b".into()).unwrap(), l.b);
}
#[test]
fn read_only_focus_is_allowed_but_focus_listener_edit_remains_policy_gated() {
    let mut l = Lab::new();
    l.e.selection.ids.insert("b".into());
    l.h.set_editable(false, &mut l.e);
    let sender = l.r.action_sender();
    let origin = l.b;
    l.e.engine
        .on(
            l.b,
            EventKind::Focus,
            ListenerOptions::default(),
            move |_| {
                sender
                    .enqueue(origin, SceneCommand::DeleteSelection)
                    .unwrap();
            },
        )
        .unwrap();
    let report = l.focus(l.b);
    assert!(!report.errors.is_empty());
    assert!(l.e.scene().get(&"b".into()).is_some());
    assert_eq!(l.r.focused(&l.e.engine), Some(l.b));
    assert!(!l.e.undo());
}
#[test]
fn native_focus_lost_uses_non_cancelable_blur_and_repeated_loss_is_idempotent() {
    let mut l = Lab::new();
    l.focus(l.a);
    l.event(key(KeyCode::Space));
    l.e.engine
        .on(l.a, EventKind::Blur, ListenerOptions::default(), |e| {
            e.prevent_default()
        })
        .unwrap();
    let report = l.event(CanvasEvent::FocusLost);
    let blur = report
        .events
        .iter()
        .find(|e| e.kind == EventKind::Blur)
        .unwrap();
    assert!(!blur.cancelable);
    assert!(!blur.default_prevented());
    assert_eq!(blur.related_target, None);
    assert_eq!(l.r.focused(&l.e.engine), None);
    assert!(!l.r.space_held());
    assert!(l.event(CanvasEvent::FocusLost).events.is_empty());
}

#[test]
fn blur_listener_deleting_requested_target_prevents_obsolete_focus() {
    let mut l = Lab::new();
    l.focus(l.a);
    l.e.selection.ids.insert("b".into());
    let sender = l.r.action_sender();
    let origin = l.a;
    l.e.engine
        .on(
            l.a,
            EventKind::Blur,
            ListenerOptions::default(),
            move |_| {
                sender
                    .enqueue(origin, SceneCommand::DeleteSelection)
                    .unwrap();
            },
        )
        .unwrap();
    let report = l.focus(l.b);
    assert_eq!(
        report
            .events
            .iter()
            .map(|e| e.kind.clone())
            .collect::<Vec<_>>(),
        vec![EventKind::Blur]
    );
    assert!(report.errors.iter().any(|e| e.contains("invalidated")));
    assert_eq!(l.r.focused(&l.e.engine), None);
    assert!(!l.h.has_capture());
    assert!(!l.r.space_held());
}
#[test]
fn blur_focus_redirect_is_deferred_and_supersedes_original_target_before_focus() {
    let mut l = Lab::new();
    let c =
        l.e.engine
            .create("rect", "c", serde_json::json!({}), None, None)
            .unwrap()
            .0;
    l.focus(l.a);
    let sender = l.r.action_sender();
    let request = SceneFocusRequest::new(&l.e.engine, l.a, Some(c)).unwrap();
    l.e.engine
        .on(
            l.a,
            EventKind::Blur,
            ListenerOptions::default(),
            move |_| {
                sender.enqueue_focus(request).unwrap();
            },
        )
        .unwrap();
    let report = l.focus(l.b);
    assert!(report.focus_requested);
    assert_eq!(
        report
            .events
            .iter()
            .map(|e| (e.kind.clone(), e.target.as_str()))
            .collect::<Vec<_>>(),
        vec![(EventKind::Blur, "a"), (EventKind::Focus, "c")]
    );
    assert_eq!(
        report.events[1].related_target.as_ref().unwrap().as_str(),
        "a"
    );
    assert_eq!(l.r.focused(&l.e.engine), Some(c));
}
#[test]
fn explicit_clear_and_native_focus_loss_cannot_be_undone_by_blur_listener_redirects() {
    for native in [false, true] {
        let mut l = Lab::new();
        l.focus(l.a);
        let sender = l.r.action_sender();
        let request = SceneFocusRequest::new(&l.e.engine, l.a, Some(l.b)).unwrap();
        l.e.engine
            .on(
                l.a,
                EventKind::Blur,
                ListenerOptions::default(),
                move |_| {
                    sender.enqueue_focus(request).unwrap();
                },
            )
            .unwrap();
        let report = if native {
            l.event(CanvasEvent::FocusLost)
        } else {
            l.r.clear_focus(&mut l.e, &mut l.h)
        };
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("explicit focus clearing"))
        );
        assert_eq!(l.r.focused(&l.e.engine), None);
        assert!(report.events.iter().all(|e| e.kind != EventKind::Focus));
        assert!(l.event(key(KeyCode::Character('x'))).events.is_empty());
    }
}
#[test]
fn mutual_focus_listeners_stop_at_budget_and_subsequent_keyboard_input_recovers() {
    let mut l = Lab::new();
    for (origin, target) in [(l.a, l.b), (l.b, l.a)] {
        let request = SceneFocusRequest::new(&l.e.engine, origin, Some(target)).unwrap();
        let sender = l.r.action_sender();
        l.e.engine
            .on(
                origin,
                EventKind::Focus,
                ListenerOptions::default(),
                move |_| {
                    let _ = sender.enqueue_focus(request);
                },
            )
            .unwrap();
    }
    let report = l.focus(l.a);
    assert!(report.focus_requested);
    assert!(report.events.len() <= 514);
    assert!(report.errors.iter().any(|e| e.contains("budget exhausted")));
    let final_target = l.r.focused(&l.e.engine).unwrap();
    assert!(!l.h.has_capture());
    assert!(!l.r.space_held());
    let next = l.event(key(KeyCode::Character('x')));
    assert_eq!(next.events.len(), 1);
    assert_eq!(next.events[0].kind, EventKind::KeyDown);
    assert_eq!(
        next.events[0].target,
        l.e.engine.instance(final_target).unwrap().id
    );
    assert!(next.errors.is_empty());
    assert!(!next.focus_requested);
}
#[test]
fn budget_exhaustion_keeps_queued_editor_commands_for_next_input_in_fifo_order() {
    use std::cell::Cell;
    let mut l = Lab::new();
    let count = Rc::new(Cell::new(0));
    for (origin, target) in [(l.a, l.b), (l.b, l.a)] {
        let request = SceneFocusRequest::new(&l.e.engine, origin, Some(target)).unwrap();
        let sender = l.r.action_sender();
        let count = count.clone();
        l.e.engine
            .on(
                origin,
                EventKind::Focus,
                ListenerOptions::default(),
                move |_| {
                    count.set(count.get() + 1);
                    let _ = sender.enqueue_focus(request);
                    sender
                        .enqueue(
                            origin,
                            SceneCommand::SetObjectData {
                                id: "root".into(),
                                data: serde_json::json!({"count":count.get()}),
                            },
                        )
                        .unwrap();
                },
            )
            .unwrap();
    }
    let report = l.focus(l.a);
    assert!(report.errors.iter().any(|e| e.contains("budget exhausted")));
    let emitted = count.get();
    assert!(
        l.e.scene().get(&"root".into()).unwrap().object.data["count"]
            .as_u64()
            .unwrap()
            < (emitted as u64)
    );
    let report = l.event(key(KeyCode::Character('x')));
    assert!(report.default.document_changed);
    assert_eq!(count.get(), emitted);
    assert_eq!(
        l.e.scene().get(&"root".into()).unwrap().object.data["count"],
        serde_json::json!(emitted)
    );
}
#[test]
fn invalid_deferred_focus_request_does_not_replace_the_valid_requested_target() {
    let mut l = Lab::new();
    l.focus(l.a);
    let other = Lab::new();
    let sender = l.r.action_sender();
    let request = SceneFocusRequest::new(&other.e.engine, other.a, Some(other.b)).unwrap();
    l.e.engine
        .on(
            l.a,
            EventKind::Blur,
            ListenerOptions::default(),
            move |_| {
                sender.enqueue_focus(request).unwrap();
            },
        )
        .unwrap();
    let report = l.focus(l.b);
    assert!(!report.errors.is_empty());
    assert_eq!(l.r.focused(&l.e.engine), Some(l.b));
    assert_eq!(report.events.last().unwrap().kind, EventKind::Focus);
}

#[test]
fn pointer_down_listener_focus_redirect_does_not_rearm_cancelled_press_or_click() {
    let mut l = Lab::new();
    l.focus(l.a);
    let sender = l.r.action_sender();
    let request = SceneFocusRequest::new(&l.e.engine, l.b, Some(l.a)).unwrap();
    l.e.engine
        .on(
            l.b,
            EventKind::PointerDown,
            ListenerOptions::default(),
            move |_| {
                sender.enqueue_focus(request).unwrap();
            },
        )
        .unwrap();
    let report = l.event(CanvasEvent::PointerDown(pointer("b", 220.)));
    assert!(report.focus_requested);
    assert_eq!(l.r.focused(&l.e.engine), Some(l.a));
    assert!(!l.r.has_pressed(&l.e.engine));
    assert!(!l.h.has_capture());
    let up = l.event(CanvasEvent::PointerUp(pointer("b", 220.)));
    assert!(up.events.iter().all(|e| e.kind != EventKind::Click));
}

#[test]
fn deferred_focus_mount_tokens_reject_origin_or_target_reattach_until_prepared_again() {
    for remount_origin in [false, true] {
        let mut l = Lab::new();
        l.focus(l.a);
        let request = SceneFocusRequest::new(&l.e.engine, l.a, Some(l.b)).unwrap();
        let sender = l.r.action_sender();
        sender.enqueue_focus(request).unwrap();
        let moved = if remount_origin { l.a } else { l.b };
        let (tree, _) = l.e.engine.detach(moved).unwrap();
        l.e.engine.attach(&tree, None, None).unwrap();
        let report = l.event(key(KeyCode::Character('x')));
        assert!(!report.errors.is_empty());
        assert_ne!(l.r.focused(&l.e.engine), Some(l.b));
        sender.enqueue_focus(request).unwrap(); // even enqueueing the old prepared token later is stale
        assert!(!l.event(key(KeyCode::Character('x'))).errors.is_empty());
        let fresh = SceneFocusRequest::new(&l.e.engine, l.a, Some(l.b)).unwrap();
        sender.enqueue_focus(fresh).unwrap();
        l.event(key(KeyCode::Character('x')));
        assert_eq!(l.r.focused(&l.e.engine), Some(l.b));
    }
}

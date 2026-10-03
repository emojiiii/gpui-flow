use gpui_flow_core::Point;
use gpui_flow_editor::SceneEditor;
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{Affine, DisplayObject, EventInput, EventKind, Scene, display};

struct Lab {
    editor: SceneEditor,
    host: ScenePluginHost,
    router: SceneEventRouter,
}
impl Lab {
    fn new() -> Self {
        let mut scene = Scene::new();
        for (id, x) in [("a", 0.), ("b", 200.)] {
            scene
                .insert(
                    DisplayObject::new(id, display::Rect::new(100., 100.))
                        .with_transform(Affine::translation(x, 0.)),
                    None,
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
    fn send(&mut self, event: CanvasEvent) -> RoutedReport {
        let out = self
            .router
            .dispatch(&mut self.editor, &mut self.host, event);
        assert!(out.errors.is_empty(), "{:?}", out.errors);
        assert!(out.default.errors.is_empty(), "{:?}", out.default.errors);
        out
    }
    fn tick(&mut self, now: u64) -> RoutedReport {
        self.router
            .advance_time(&mut self.editor, &mut self.host, now)
    }
    fn tap(&mut self, pointer: PointerInput, up: u64) -> RoutedReport {
        self.send(CanvasEvent::PointerDown(pointer.clone()));
        self.send(CanvasEvent::PointerUp(release(pointer, up)))
    }
}
fn pointer(device: PointerDevice, id: u64, target: &str, time: u64) -> PointerInput {
    PointerInput {
        device,
        pointer_id: id,
        timestamp_millis: Some(time),
        buttons: Some(1),
        ..PointerInput::left(
            Point::new(if target == "a" { 20. } else { 220. }, 20.),
            HitTarget::Object(target.into()),
        )
    }
}
fn release(mut p: PointerInput, time: u64) -> PointerInput {
    p.timestamp_millis = Some(time);
    p.buttons = Some(0);
    p
}
fn kinds(report: &RoutedReport) -> Vec<EventKind> {
    report.events.iter().map(|e| e.kind.clone()).collect()
}
fn count(report: &RoutedReport, kind: EventKind) -> usize {
    report.events.iter().filter(|e| e.kind == kind).count()
}
#[test]
fn metadata_preserves_device_mask_time_and_actual_pressure() {
    let mut lab = Lab::new();
    let mut pen = pointer(PointerDevice::Pen, 7, "a", 42);
    pen.buttons = Some(5);
    pen.pressure = Some(0.75);
    let report = lab.send(CanvasEvent::PointerDown(pen.clone()));
    let event = report
        .events
        .iter()
        .find(|e| e.kind == EventKind::PointerDown)
        .unwrap();
    assert_eq!(event.timestamp_millis, Some(42));
    assert!(matches!(
        event.input,
        Some(EventInput::Pointer {
            pointer_id: 7,
            device: PointerDevice::Pen,
            buttons: 5,
            pressure: Some(0.75),
            ..
        })
    ));
    let up = lab.send(CanvasEvent::PointerUp(release(pen, 52)));
    assert_eq!(up.events[0].timestamp_millis, Some(52));
    assert!(matches!(
        up.events[0].input,
        Some(EventInput::Pointer { buttons: 0, .. })
    ));
}
#[test]
fn immediate_taps_double_pairs_and_no_triple_overlap() {
    let mut lab = Lab::new();
    let first = lab.tap(pointer(PointerDevice::Mouse, 0, "a", 0), 30);
    assert_eq!(
        kinds(&first),
        vec![EventKind::PointerUp, EventKind::Tap, EventKind::Click]
    );
    let second = lab.tap(pointer(PointerDevice::Mouse, 0, "a", 100), 130);
    assert_eq!(
        kinds(&second),
        vec![
            EventKind::PointerUp,
            EventKind::Tap,
            EventKind::Click,
            EventKind::DoubleTap,
            EventKind::DoubleClick
        ]
    );
    let third = lab.tap(pointer(PointerDevice::Mouse, 0, "a", 200), 230);
    assert_eq!(count(&third, EventKind::DoubleTap), 0);
    assert!(
        second
            .events
            .windows(2)
            .all(|p| p[0].sequence < p[1].sequence)
    );
}
#[test]
fn touch_double_tap_can_use_new_contact_id_but_never_another_device_or_target() {
    let mut lab = Lab::new();
    lab.tap(pointer(PointerDevice::Touch, 1, "a", 0), 10);
    assert_eq!(
        count(
            &lab.tap(pointer(PointerDevice::Touch, 2, "a", 40), 50),
            EventKind::DoubleTap
        ),
        1
    );
    lab.tap(pointer(PointerDevice::Touch, 3, "a", 80), 90);
    assert_eq!(
        count(
            &lab.tap(pointer(PointerDevice::Mouse, 0, "a", 100), 110),
            EventKind::DoubleTap
        ),
        0
    );
    assert_eq!(
        count(
            &lab.tap(pointer(PointerDevice::Touch, 4, "b", 120), 130),
            EventKind::DoubleTap
        ),
        0
    );
}
#[test]
fn long_press_fires_once_at_boundary_and_suppresses_release_tap() {
    let mut lab = Lab::new();
    let p = pointer(PointerDevice::Touch, 9, "a", 100);
    lab.send(CanvasEvent::PointerDown(p.clone()));
    assert!(lab.router.needs_gesture_frame());
    assert_eq!(count(&lab.tick(599), EventKind::LongPress), 0);
    let held = lab.tick(600);
    assert_eq!(count(&held, EventKind::LongPress), 1);
    assert_eq!(held.events[0].timestamp_millis, Some(600));
    assert!(!lab.router.needs_gesture_frame());
    assert_eq!(count(&lab.tick(800), EventKind::LongPress), 0);
    let up = lab.send(CanvasEvent::PointerUp(release(p, 900)));
    assert_eq!(kinds(&up), vec![EventKind::PointerUp]);
    assert!(!lab.host.has_capture());
}
#[test]
fn moving_away_and_back_cannot_resurrect_tap_or_long_press() {
    let mut lab = Lab::new();
    let p = pointer(PointerDevice::Touch, 1, "a", 0);
    lab.send(CanvasEvent::PointerDown(p.clone()));
    let mut moved = p.clone();
    moved.position.x += 4.;
    moved.timestamp_millis = Some(20);
    lab.send(CanvasEvent::PointerMove(moved));
    assert!(!lab.router.needs_gesture_frame());
    assert_eq!(count(&lab.tick(500), EventKind::LongPress), 0);
    let up = lab.send(CanvasEvent::PointerUp(release(p, 501)));
    assert_eq!(count(&up, EventKind::Tap), 0);
}
#[test]
fn concurrent_touch_targets_and_cancellation_are_isolated() {
    let mut lab = Lab::new();
    let first = pointer(PointerDevice::Touch, 1, "a", 0);
    let second = pointer(PointerDevice::Touch, 2, "b", 1);
    lab.send(CanvasEvent::PointerDown(first.clone()));
    let report = lab.send(CanvasEvent::PointerDown(second.clone()));
    assert_eq!(report.events[0].target.as_str(), "b");
    let b = lab.editor.engine.handle(&"b".into()).unwrap();
    assert!(lab.router.state(&lab.editor.engine, b).press);
    let cancelled = lab.send(CanvasEvent::PointerCancelFor(second.identity()));
    assert!(matches!(
        cancelled.events[0].input,
        Some(EventInput::Pointer {
            pointer_id: 2,
            cancelled: true,
            buttons: 0,
            ..
        })
    ));
    assert!(lab.host.has_capture());
    assert!(!lab.router.state(&lab.editor.engine, b).press);
    let up = lab.send(CanvasEvent::PointerUp(release(first, 10)));
    assert_eq!(count(&up, EventKind::Tap), 1);
    assert_eq!(up.events[0].target.as_str(), "a");
}
#[test]
fn same_numeric_id_on_touch_and_mouse_cannot_release_capture() {
    let mut lab = Lab::new();
    let mouse = pointer(PointerDevice::Mouse, 0, "a", 0);
    let touch = pointer(PointerDevice::Touch, 0, "b", 1);
    lab.send(CanvasEvent::PointerDown(mouse.clone()));
    lab.send(CanvasEvent::PointerDown(touch.clone()));
    let up = lab.send(CanvasEvent::PointerUp(release(touch, 10)));
    assert_eq!(up.events[0].target.as_str(), "b");
    assert!(lab.host.has_capture());
    lab.send(CanvasEvent::PointerUp(release(mouse, 20)));
    assert!(!lab.host.has_capture());
}
#[test]
fn releasing_first_touch_never_promotes_held_second_into_an_edit() {
    let mut lab = Lab::new();
    let first = pointer(PointerDevice::Touch, 1, "a", 0);
    let mut second = pointer(PointerDevice::Touch, 2, "b", 1);
    lab.send(CanvasEvent::PointerDown(first.clone()));
    lab.send(CanvasEvent::PointerDown(second.clone()));
    lab.send(CanvasEvent::PointerUp(release(first, 10)));
    second.position.x += 30.;
    second.timestamp_millis = Some(20);
    lab.send(CanvasEvent::PointerMove(second.clone()));
    assert!(!lab.host.has_capture());
    assert_eq!(
        lab.editor
            .scene()
            .get(&"b".into())
            .unwrap()
            .object
            .transform
            .tx,
        200.
    );
    lab.send(CanvasEvent::PointerUp(release(second, 30)));
}
#[test]
fn focus_loss_cancels_all_contacts_and_no_timer_or_release_survives() {
    let mut lab = Lab::new();
    let a = pointer(PointerDevice::Touch, 1, "a", 0);
    let b = pointer(PointerDevice::Touch, 2, "b", 1);
    lab.send(CanvasEvent::PointerDown(a.clone()));
    lab.send(CanvasEvent::PointerDown(b.clone()));
    let report = lab.send(CanvasEvent::FocusLost);
    assert_eq!(count(&report, EventKind::PointerCancel), 2);
    assert!(!lab.router.needs_gesture_frame());
    assert!(!lab.host.has_capture());
    assert!(lab.tick(600).events.is_empty());
    assert!(
        lab.send(CanvasEvent::PointerUp(release(a, 601)))
            .events
            .is_empty()
    );
    assert!(
        lab.send(CanvasEvent::PointerUp(release(b, 602)))
            .events
            .is_empty()
    );
}
#[test]
fn replacing_same_public_id_never_receives_stale_gestures() {
    let mut lab = Lab::new();
    let p = pointer(PointerDevice::Touch, 1, "a", 0);
    lab.send(CanvasEvent::PointerDown(p.clone()));
    lab.editor.load_scene(lab.editor.scene().clone()).unwrap();
    assert!(lab.tick(500).events.is_empty());
    assert!(!lab.host.has_capture());
    assert!(
        lab.send(CanvasEvent::PointerUp(release(p, 501)))
            .events
            .is_empty()
    );
}
#[test]
fn invalid_metadata_and_backwards_time_leave_accepted_contact_intact() {
    let mut lab = Lab::new();
    let p = pointer(PointerDevice::Pen, 3, "a", 100);
    lab.send(CanvasEvent::PointerDown(p.clone()));
    for pressure in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        let mut invalid = p.clone();
        invalid.pressure = Some(pressure);
        invalid.timestamp_millis = Some(101);
        let report = lab.router.dispatch(
            &mut lab.editor,
            &mut lab.host,
            CanvasEvent::PointerMove(invalid),
        );
        assert_eq!(report.errors.len(), 1);
        assert!(report.events.is_empty());
        assert!(lab.host.has_capture());
    }
    let report = lab.router.dispatch(
        &mut lab.editor,
        &mut lab.host,
        CanvasEvent::PointerUp(release(p.clone(), 99)),
    );
    assert_eq!(report.errors.len(), 1);
    assert!(lab.host.has_capture());
    assert_eq!(lab.tick(99).errors.len(), 1);
    assert_eq!(
        count(
            &lab.send(CanvasEvent::PointerUp(release(p, 110))),
            EventKind::Tap
        ),
        1
    );
}
#[test]
fn prevented_release_and_explicit_focus_transfer_cancel_pending_gestures() {
    let mut lab = Lab::new();
    let a = lab.editor.engine.handle(&"a".into()).unwrap();
    lab.editor
        .engine
        .on(a, EventKind::PointerUp, Default::default(), |event| {
            event.prevent_default()
        })
        .unwrap();
    let up = lab.tap(pointer(PointerDevice::Mouse, 0, "a", 0), 20);
    assert_eq!(count(&up, EventKind::Tap), 0);
    assert!(!lab.host.has_capture());
    lab.send(CanvasEvent::PointerDown(pointer(
        PointerDevice::Touch,
        1,
        "a",
        30,
    )));
    let b = lab.editor.engine.handle(&"b".into()).unwrap();
    let focus = lab
        .router
        .set_focus(&mut lab.editor, &mut lab.host, Some(b))
        .unwrap();
    assert_eq!(count(&focus, EventKind::PointerCancel), 1);
    assert!(!lab.router.needs_gesture_frame());
}
#[test]
fn legacy_untimed_constructor_keeps_clicks_without_fabricated_timing() {
    let mut lab = Lab::new();
    let p = PointerInput::left(Point::new(20., 20.), HitTarget::Object("a".into()));
    for _ in 0..2 {
        lab.send(CanvasEvent::PointerDown(p.clone()));
        assert!(!lab.router.needs_gesture_frame());
        let up = lab.send(CanvasEvent::PointerUp(p.clone()));
        assert_eq!(count(&up, EventKind::Click), 1);
        assert_eq!(count(&up, EventKind::DoubleClick), 0);
        assert!(up.events.iter().all(|e| e.timestamp_millis.is_none()));
    }
}
#[test]
fn button_chord_does_not_cancel_capture_or_create_a_tap() {
    let mut lab = Lab::new();
    let p = pointer(PointerDevice::Mouse, 0, "a", 0);
    lab.send(CanvasEvent::PointerDown(p.clone()));
    let mut chord = p.clone();
    chord.button = Some(PointerButton::Right);
    chord.buttons = Some(3);
    chord.timestamp_millis = Some(10);
    lab.send(CanvasEvent::PointerDown(chord.clone()));
    assert!(lab.host.has_capture());
    chord.buttons = Some(1);
    chord.timestamp_millis = Some(20);
    lab.send(CanvasEvent::PointerUp(chord));
    assert!(lab.host.has_capture());
    assert_eq!(
        count(
            &lab.send(CanvasEvent::PointerUp(release(p, 30))),
            EventKind::Tap
        ),
        0
    );
}
#[test]
fn pointer_capacity_is_bounded_without_evicting_existing_contacts() {
    let mut lab = Lab::new();
    for id in 0..16 {
        lab.send(CanvasEvent::PointerDown(pointer(
            PointerDevice::Touch,
            id,
            "a",
            id,
        )));
    }
    let out = lab.router.dispatch(
        &mut lab.editor,
        &mut lab.host,
        CanvasEvent::PointerDown(pointer(PointerDevice::Touch, 16, "a", 16)),
    );
    assert_eq!(out.errors.len(), 1);
    assert_eq!(
        count(
            &lab.send(CanvasEvent::PointerCancel),
            EventKind::PointerCancel
        ),
        16
    );
    assert!(!lab.host.has_capture());
}

#[test]
fn equal_timestamp_overlapping_contacts_never_form_a_double() {
    let mut lab = Lab::new();
    let a = pointer(PointerDevice::Touch, 1, "a", 0);
    let b = pointer(PointerDevice::Touch, 2, "a", 0);
    lab.send(CanvasEvent::PointerDown(a.clone()));
    lab.send(CanvasEvent::PointerDown(b.clone()));
    lab.send(CanvasEvent::PointerUp(release(a, 0)));
    assert_eq!(
        count(
            &lab.send(CanvasEvent::PointerUp(release(b, 0))),
            EventKind::DoubleTap
        ),
        0
    );
}
#[test]
fn intervening_other_target_breaks_the_double_tap_sequence() {
    let mut lab = Lab::new();
    lab.tap(pointer(PointerDevice::Touch, 1, "a", 0), 10);
    lab.tap(pointer(PointerDevice::Touch, 2, "b", 20), 30);
    assert_eq!(
        count(
            &lab.tap(pointer(PointerDevice::Touch, 3, "a", 40), 50),
            EventKind::DoubleTap
        ),
        0
    );
}
#[test]
fn mouse_hover_and_unknown_scoped_cancel_leave_touch_contacts_armed() {
    let mut lab = Lab::new();
    let touch = pointer(PointerDevice::Touch, 1, "a", 0);
    lab.send(CanvasEvent::PointerDown(touch.clone()));
    let mut mouse = pointer(PointerDevice::Mouse, 0, "b", 1);
    mouse.buttons = Some(0);
    mouse.button = None;
    lab.send(CanvasEvent::PointerMove(mouse));
    lab.send(CanvasEvent::PointerCancelFor(PointerIdentity {
        device: PointerDevice::Mouse,
        pointer_id: 0,
    }));
    assert!(lab.host.has_capture());
    assert!(lab.router.has_pointer(touch.identity()));
    assert_eq!(count(&lab.tick(500), EventKind::LongPress), 1);
}
#[test]
fn direct_plugin_host_capture_is_scoped_by_device_as_well_as_id() {
    let mut lab = Lab::new();
    let mouse = pointer(PointerDevice::Mouse, 0, "a", 0);
    lab.host
        .dispatch(&mut lab.editor, CanvasEvent::PointerDown(mouse.clone()));
    assert!(lab.host.has_capture());
    lab.host.dispatch(
        &mut lab.editor,
        CanvasEvent::PointerCancelFor(PointerIdentity {
            device: PointerDevice::Touch,
            pointer_id: 0,
        }),
    );
    assert!(lab.host.has_capture());
    lab.host.dispatch(
        &mut lab.editor,
        CanvasEvent::PointerCancelFor(mouse.identity()),
    );
    assert!(!lab.host.has_capture());
}

#[test]
fn timer_retires_capture_cancelled_by_plugin_lifecycle() {
    let mut lab = Lab::new();
    let p = pointer(PointerDevice::Touch, 1, "a", 0);
    lab.send(CanvasEvent::PointerDown(p.clone()));
    assert!(lab.host.has_capture());
    lab.host
        .set_enabled("drag", false, &mut lab.editor)
        .unwrap();
    let timer = lab.tick(500);
    assert_eq!(count(&timer, EventKind::LongPress), 0);
    assert_eq!(count(&timer, EventKind::PointerCancel), 1);
    assert!(!lab.router.has_pointer(p.identity()));
    assert!(
        lab.send(CanvasEvent::PointerUp(release(p, 501)))
            .events
            .is_empty()
    );
}
#[test]
fn invalid_coordinates_cancel_only_the_matching_contact() {
    let mut lab = Lab::new();
    let first = pointer(PointerDevice::Touch, 1, "a", 0);
    let mut second = pointer(PointerDevice::Touch, 2, "b", 1);
    lab.send(CanvasEvent::PointerDown(first.clone()));
    lab.send(CanvasEvent::PointerDown(second.clone()));
    second.position.x = f32::NAN;
    let report = lab.router.dispatch(
        &mut lab.editor,
        &mut lab.host,
        CanvasEvent::PointerMove(second.clone()),
    );
    assert_eq!(report.errors.len(), 1);
    assert!(lab.host.has_capture());
    assert!(lab.router.has_pointer(first.identity()));
    assert!(!lab.router.has_pointer(second.identity()));
    assert_eq!(count(&lab.tick(500), EventKind::LongPress), 1);
}
#[test]
fn timer_reconciles_external_document_writes_before_recognition() {
    let mut lab = Lab::new();
    let p = pointer(PointerDevice::Touch, 1, "a", 0);
    lab.send(CanvasEvent::PointerDown(p.clone()));
    lab.editor
        .engine
        .transaction(|scene| scene.update_object(&"a".into(), |object| object.transform.tx = 15.))
        .unwrap();
    let timer = lab.tick(500);
    assert_eq!(count(&timer, EventKind::LongPress), 0);
    assert!(!lab.host.has_capture());
    assert!(!lab.router.has_pointer(p.identity()));
    assert_eq!(
        lab.editor
            .scene()
            .get(&"a".into())
            .unwrap()
            .object
            .transform
            .tx,
        15.
    );
}

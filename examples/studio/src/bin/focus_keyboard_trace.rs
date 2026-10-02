//! Two real SceneViews replay every observed normalized input into independent
//! pure routers. JSON lines contain both results, never a duplicate native trace.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneInputOperation, SceneInputTrace, SceneView, SceneViewEvent,
    editor::SceneEditor,
    engine::SceneEngine,
    interaction::scene::{
        RoutedReport, SceneActionSender, SceneEventRouter, SceneFocusRequest, ScenePluginHost,
        StatePaint, StateStyles,
    },
    scene::{
        Affine, Color, DisplayObject, EventInput, EventKind, ListenerOptions, Scene, SceneEvent,
        Style, display,
    },
};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use serde_json::{Value, json};
use std::{cell::Cell, collections::VecDeque, rc::Rc};

#[derive(Default)]
struct Policy {
    block_space: Cell<bool>,
    redirect_focus: Cell<bool>,
}

fn scene() -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Group::new()),
            None,
            None,
        )
        .unwrap();
    for (id, x, color) in [("a", 36., 0x287a86), ("b", 286., 0x80589e)] {
        scene
            .insert(
                DisplayObject::new(id, display::Box::new(180., 130.).rounded(14.))
                    .with_transform(Affine::translation(x, 42.))
                    .with_style(Style::fill(Color::hex(color))),
                Some("root".into()),
                None,
            )
            .unwrap();
        let mut label = DisplayObject::new(
            format!("{id}-label"),
            display::Text::new(id.to_uppercase(), gpui_flow::core::Size::new(80., 35.)),
        )
        .with_transform(Affine::translation(72., 47.))
        .with_style(Style::fill(Color::WHITE));
        label.hit_policy = gpui_flow::scene::HitPolicy::None;
        scene.insert(label, Some(id.into()), None).unwrap();
    }
    scene
        .insert(
            DisplayObject::new("dead", display::Rect::new(1., 1.)),
            None,
            None,
        )
        .unwrap();
    scene
}

/// Each call installs distinct listeners, policy cells, queue senders and handles.
fn listeners(engine: &mut SceneEngine, sender: SceneActionSender, policy: Rc<Policy>) {
    let a = engine.handle(&"a".into()).unwrap();
    let b = engine.handle(&"b".into()).unwrap();
    let dead = engine.handle(&"dead".into()).unwrap();
    for (origin, destination) in [(a, b), (b, a)] {
        let redirect = SceneFocusRequest::new(engine, origin, Some(destination)).unwrap();
        let stale = SceneFocusRequest::new(engine, origin, Some(dead)).unwrap();
        let sender = sender.clone();
        let policy = policy.clone();
        engine
            .on(
                origin,
                EventKind::KeyDown,
                ListenerOptions::default(),
                move |event| {
                    let Some(EventInput::Key { key, repeat, .. }) = &event.input else {
                        return;
                    };
                    match key.as_str() {
                        "Space" if policy.block_space.replace(false) => event.prevent_default(),
                        "r" if !repeat => sender.enqueue_focus(redirect).unwrap(),
                        "d" if !repeat => sender.enqueue_focus(stale).unwrap(),
                        _ => {}
                    }
                },
            )
            .unwrap();
        engine
            .on(
                origin,
                EventKind::KeyUp,
                ListenerOptions::default(),
                |event| {
                    if matches!(&event.input, Some(EventInput::Key { key, .. }) if key == "Space") {
                        event.prevent_default();
                    }
                },
            )
            .unwrap();
    }
    let request = SceneFocusRequest::new(engine, a, Some(b)).unwrap();
    engine
        .on(a, EventKind::Focus, ListenerOptions::default(), move |_| {
            if policy.redirect_focus.replace(false) {
                sender.enqueue_focus(request).unwrap();
            }
        })
        .unwrap();
    // Requests above intentionally keep the dead generation. The visual scene
    // and pure mirror each destroy their own instance, never share a handle.
    engine.destroy(dead).unwrap();
}

struct Mirror {
    editor: SceneEditor,
    host: ScenePluginHost,
    router: SceneEventRouter,
    policy: Rc<Policy>,
}
impl Mirror {
    fn new() -> Self {
        let mut editor = SceneEditor::new(scene()).unwrap();
        let router = SceneEventRouter::new();
        let policy = Rc::new(Policy::default());
        listeners(&mut editor.engine, router.action_sender(), policy.clone());
        Self {
            editor,
            host: ScenePluginHost::with_defaults(),
            router,
            policy,
        }
    }
    #[cfg(test)]
    fn replay(&mut self, operation: SceneInputOperation) -> SceneInputTrace {
        self.replay_at(operation, std::time::Duration::ZERO)
    }
    fn replay_at(
        &mut self,
        operation: SceneInputOperation,
        now: std::time::Duration,
    ) -> SceneInputTrace {
        if !matches!(operation, SceneInputOperation::NavigationFrame { .. }) {
            let sync = self.host.synchronize_navigation_time(&mut self.editor, now);
            assert!(sync.errors.is_empty());
        }
        let report = match &operation {
            SceneInputOperation::NavigationFrame { now } => RoutedReport {
                default: self.host.advance_navigation(&mut self.editor, *now),
                ..Default::default()
            },
            SceneInputOperation::Canvas(event) => {
                self.router
                    .dispatch(&mut self.editor, &mut self.host, event.clone())
            }
            SceneInputOperation::PickedWheel {
                position,
                delta,
                modifiers,
                target,
            } => {
                let handle = target.as_ref().map(|id| {
                    self.editor
                        .engine
                        .handle(id)
                        .expect("trace target belongs to mirror document")
                });
                self.router
                    .dispatch_picked_wheel(
                        &mut self.editor,
                        &mut self.host,
                        *position,
                        *delta,
                        *modifiers,
                        handle,
                    )
                    .expect("live mirror wheel target")
            }
            SceneInputOperation::Focus(id) => {
                let handle = self.editor.engine.handle(id).unwrap();
                self.router
                    .set_focus(&mut self.editor, &mut self.host, Some(handle))
                    .unwrap()
            }
            SceneInputOperation::ClearFocus => {
                self.router.clear_focus(&mut self.editor, &mut self.host)
            }
            SceneInputOperation::FocusDeclined(_) => RoutedReport::default(),
        };
        SceneInputTrace::from_report(operation, &report, &self.editor, &self.host, &self.router)
    }
}

fn event_value(event: &SceneEvent) -> Value {
    json!({
        "kind": event.kind, "target": event.target, "current_target": event.current_target,
        "phase": format!("{:?}", event.phase), "related_target": event.related_target,
        "input": event.input, "sequence": event.sequence, "timestamp_millis": event.timestamp_millis,
        "world": event.world_position, "local": event.local_position, "data": event.data,
        "bubbles": event.bubbles, "cancelable": event.cancelable,
        "default_prevented": event.default_prevented(),
        "propagation_stopped": event.propagation_stopped(),
    })
}
fn operation_value(operation: &SceneInputOperation) -> Value {
    use gpui_flow::interaction::CanvasEvent;
    let modifiers = |value: gpui_flow::interaction::Modifiers| {
        json!({
            "shift":value.shift, "control":value.control, "alt":value.alt,
            "meta":value.meta, "space":value.space,
        })
    };
    match operation {
        SceneInputOperation::NavigationFrame { now } => {
            json!({"kind":"NavigationFrame", "secs":now.as_secs(), "nanos":now.subsec_nanos()})
        }
        SceneInputOperation::PickedWheel {
            position,
            delta,
            modifiers: value,
            target,
        } => {
            json!({"kind":"PickedWheel", "position":position, "delta":delta, "modifiers":modifiers(*value), "target":target})
        }
        SceneInputOperation::Focus(target) => json!({"kind":"Focus", "target":target}),
        SceneInputOperation::ClearFocus => json!({"kind":"ClearFocus"}),
        SceneInputOperation::FocusDeclined(target) => {
            json!({"kind":"FocusDeclined", "target":target})
        }
        SceneInputOperation::Canvas(event) => match event {
            CanvasEvent::PointerDown(pointer)
            | CanvasEvent::PointerMove(pointer)
            | CanvasEvent::PointerUp(pointer) => json!({
                "kind":match event {CanvasEvent::PointerDown(_) => "PointerDown", CanvasEvent::PointerMove(_) => "PointerMove", _ => "PointerUp"},
                "position":pointer.position, "button":format!("{:?}",pointer.button),
                "modifiers":modifiers(pointer.modifiers), "hit":format!("{:?}",pointer.hit), "pointer_id":pointer.pointer_id,
            }),
            CanvasEvent::KeyDown {
                key,
                modifiers: value,
            }
            | CanvasEvent::KeyUp {
                key,
                modifiers: value,
            } => json!({
                "kind":if matches!(event, CanvasEvent::KeyDown {..}) { "KeyDown" } else { "KeyUp" },
                "key":format!("{key:?}"), "modifiers":modifiers(*value),
            }),
            CanvasEvent::Wheel {
                position,
                delta,
                modifiers: value,
            } => {
                json!({"kind":"Wheel", "position":position, "delta":delta, "modifiers":modifiers(*value)})
            }
            CanvasEvent::PointerLeave => json!({"kind":"PointerLeave"}),
            CanvasEvent::PointerCancel => json!({"kind":"PointerCancel"}),
            CanvasEvent::FocusLost => json!({"kind":"FocusLost"}),
        },
    }
}
fn trace_value(trace: &SceneInputTrace) -> Value {
    json!({
        "operation": operation_value(&trace.operation),
        "navigation_time": {"secs":trace.navigation_time.as_secs(), "nanos":trace.navigation_time.subsec_nanos()},
        "events": trace.events.iter().map(event_value).collect::<Vec<_>>(),
        "default": {
            "handled": trace.default.handled, "prevented": trace.default.default_prevented,
            "propagation_stopped": trace.default.propagation_stopped,
            "document_changed": trace.default.document_changed,
            "selection_changed": trace.default.selection_changed,
            "viewport_changed": trace.default.viewport_changed,
            "effects": format!("{:?}", trace.default.effects),
            "errors": format!("{:?}", trace.default.errors),
        },
        "errors": trace.errors, "focus_requested": trace.focus_requested,
        "focused": trace.focused, "space": trace.space_held,
        "capture": trace.capture_owner, "pressed": trace.pressed, "editing": trace.editing,
        "selection": trace.selection, "pan": trace.viewport.pan, "zoom": trace.viewport.zoom,
    })
}

struct Lab {
    views: [Entity<SceneView>; 2],
    policies: [Rc<Policy>; 2],
    mirrors: [Mirror; 2],
    input: Entity<InputState>,
    subscriptions: Vec<Subscription>,
    records: VecDeque<String>,
    counts: [usize; 2],
    mismatches: usize,
}
impl Lab {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let policies = [Rc::new(Policy::default()), Rc::new(Policy::default())];
        let views = policies.each_ref().map(|policy| {
            cx.new(|cx| {
                let mut view = SceneView::new(scene(), cx).unwrap();
                let sender = view.action_sender();
                listeners(&mut view.editor.engine, sender, policy.clone());
                view.set_input_tracing(true);
                for id in ["a", "b"] {
                    let handle = view.editor.engine.handle(&id.into()).unwrap();
                    view.state_styles
                        .bind(
                            &view.editor.engine,
                            handle,
                            StateStyles {
                                focus: StatePaint::fill(Color::hex(0xde9d36)),
                                ..Default::default()
                            },
                        )
                        .unwrap();
                }
                view
            })
        });
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Host text input: click here, then type host123")
        });
        let mut lab = Self {
            views,
            policies,
            mirrors: [Mirror::new(), Mirror::new()],
            input,
            subscriptions: vec![],
            records: VecDeque::new(),
            counts: [0, 0],
            mismatches: 0,
        };
        for index in 0..2 {
            lab.subscriptions
                .push(cx.subscribe(&lab.views[index], move |this, _, event, cx| {
                    if let SceneViewEvent::InputTrace(native) = event {
                        let pure = this.mirrors[index]
                            .replay_at(native.operation.clone(), native.navigation_time);
                        let native_value = trace_value(native);
                        let pure_value = trace_value(&pure);
                        let matched = native_value == pure_value;
                        this.counts[index] += 1;
                        this.mismatches += usize::from(!matched);
                        // Bound diagnostic output and UI retention independently. An
                        // exhausted output budget is explicit, never a false PASS.
                        if this.counts.iter().sum::<usize>() <= 8192 {
                            println!(
                                "{}",
                                json!({"view": index + 1, "index": this.counts[index],
                            "match": matched, "native": native_value, "pure": pure_value})
                            );
                        } else if this.counts.iter().sum::<usize>() == 8193 {
                            println!("{}", json!({"trace_limit":8192,"restart_required":true}));
                        }
                        let kinds = native
                            .events
                            .iter()
                            .map(|event| format!("{:?}:{}", event.kind, event.target))
                            .collect::<Vec<_>>()
                            .join(" > ");
                        this.records.push_back(format!(
                            "V{} {} {:?} | {} | focus={:?} space={} capture={:?}",
                            index + 1,
                            if matched { "MATCH" } else { "MISMATCH" },
                            native.operation,
                            kinds,
                            native.focused,
                            native.space_held,
                            native.capture_owner
                        ));
                        while this.records.len() > 9 {
                            this.records.pop_front();
                        }
                        cx.notify();
                    }
                }));
        }
        lab.subscriptions.push(cx.subscribe(&lab.input, |this, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                println!("{}", json!({"host_input_changed":input.read(cx).value().as_ref(),
                    "scene_operations":this.counts,
                    "scene_logical_focus":this.views.each_ref().map(|view| {
                        let view = view.read(cx);
                        view.focused_element().and_then(|handle| view.editor.engine.instance(handle).ok()).map(|instance| instance.id)
                    })}));
            }
        }));
        lab
    }
    fn focus(&mut self, index: usize, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.views[index].update(cx, |view, cx| {
            let handle = view.editor.engine.handle(&id.into()).unwrap();
            let accepted = view.focus_element(handle, window, cx).unwrap();
            println!(
                "{}",
                json!({"host_focus_result": accepted, "view":index + 1, "requested":id})
            );
        });
    }
    fn panel(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .child(format!(
                "SceneView {} · A left / B right · amber = logical focus",
                index + 1
            ))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        ControlButton::new(format!("a-{index}"), "Focus A").on_click(cx.listener(
                            move |this, _, window, cx| this.focus(index, "a", window, cx),
                        )),
                    )
                    .child(
                        ControlButton::new(format!("b-{index}"), "Focus B").on_click(cx.listener(
                            move |this, _, window, cx| this.focus(index, "b", window, cx),
                        )),
                    )
                    .child(
                        ControlButton::new(format!("clear-{index}"), "Clear").on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.views[index]
                                    .update(cx, |view, cx| view.clear_element_focus(window, cx));
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        ControlButton::new(format!("space-{index}"), "Block next Space").on_click(
                            cx.listener(move |this, _, _, _| {
                                this.policies[index].block_space.set(true);
                                this.mirrors[index].policy.block_space.set(true);
                                println!("{}", json!({"arm_block_space":index + 1}));
                            }),
                        ),
                    )
                    .child(
                        ControlButton::new(format!("redirect-{index}"), "Redirect next A → B")
                            .on_click(cx.listener(move |this, _, _, _| {
                                this.policies[index].redirect_focus.set(true);
                                this.mirrors[index].policy.redirect_focus.set(true);
                                println!("{}", json!({"arm_redirect":index + 1}));
                            })),
                    ),
            )
            .child(
                div()
                    .h(px(255.))
                    .border_1()
                    .border_color(rgb(0x435165))
                    .child(self.views[index].clone()),
            )
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = format!(
            "Native ↔ independent pure router: V1 {} / V2 {} operations · {} mismatches",
            self.counts[0], self.counts[1], self.mismatches
        );
        div().size_full().flex().flex_col().p_4().gap_3().bg(rgb(0x141b25)).text_color(rgb(0xe4e9f0)).font_family("sans-serif")
            .child(div().text_lg().child("Focus / keyboard trace acceptance"))
            .child(div().text_sm().child("Focus A twice → B → Clear. r redirects; d tries a dead target. Space Up is always prevented, yet must clean Space. Hold Space and drag to pan; release outside."))
            .child(div().flex().gap_4().child(self.panel(0, cx)).child(self.panel(1, cx)))
            .child(Input::new(&self.input))
            .child(div().text_color(rgb(if self.mismatches == 0 { 0x75d8a5 } else { 0xff7777 })).child(status))
            .child(div().flex_1().min_h_0().overflow_hidden().text_xs().children(self.records.iter().map(|line| div().child(line.clone()))))
            .child(div().text_xs().child("Every row is a real adapter operation, including empty/no-op results. Full bounded JSONL goes to stdout. Host text must stay focused when clearing the other view. No IME/multi-touch/a11y claim."))
    }
}
fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1180.), px(820.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    window.set_window_title("gpui-flow · focus keyboard trace");
                    gpui_kit::component::Theme::change(
                        gpui_kit::component::ThemeMode::Dark,
                        Some(window),
                        cx,
                    );
                    cx.new(|cx| Lab::new(window, cx))
                },
            )
            .unwrap();
            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_flow::interaction::{CanvasEvent, KeyCode, Modifiers};
    #[test]
    fn mirror_uses_its_own_live_handles_and_comparator_detects_wrong_results() {
        let mut first = Mirror::new();
        let mut second = Mirror::new();
        assert_ne!(
            first.editor.engine.handle(&"a".into()),
            second.editor.engine.handle(&"a".into())
        );
        let a = first.replay(SceneInputOperation::Focus("a".into()));
        let b = second.replay(SceneInputOperation::Focus("a".into()));
        assert_eq!(trace_value(&a), trace_value(&b));
        let no_op = first.replay(SceneInputOperation::Focus("a".into()));
        assert!(no_op.events.is_empty());
        let mut wrong = b.clone();
        wrong.focused = Some("b".into());
        assert_ne!(trace_value(&a), trace_value(&wrong));
        wrong = b.clone();
        wrong.events[0].related_target = Some("b".into());
        assert_ne!(trace_value(&a), trace_value(&wrong));
    }
    #[test]
    fn picked_wheel_trace_preserves_explicit_target_and_background() {
        let mut mirror = Mirror::new();
        let operation = |target| SceneInputOperation::PickedWheel {
            position: gpui_flow::scene::Point::new(1000., 1000.),
            delta: gpui_flow::scene::Point::new(0., 12.),
            modifiers: Modifiers::default(),
            target,
        };
        let target = mirror.replay(operation(Some("a".into())));
        assert!(
            target.events.iter().any(|e| e.kind == EventKind::Wheel
                && e.target == gpui_flow::scene::DisplayId::from("a"))
        );
        let background = mirror.replay(operation(None));
        assert!(background.events.is_empty());
        assert_eq!(operation_value(&target.operation)["target"], "a");
        assert!(operation_value(&background.operation)["target"].is_null());
    }
    #[test]
    fn navigation_trace_replays_input_time_without_synthetic_motion() {
        use std::time::Duration;
        let mut mirror = Mirror::new();
        let initial = mirror.editor.viewport;
        let trace = mirror.replay_at(
            SceneInputOperation::Canvas(CanvasEvent::PointerLeave),
            Duration::from_millis(40),
        );
        assert_eq!(trace.navigation_time, Duration::from_millis(40));
        assert_eq!(trace.viewport, initial);
        let trace = mirror.replay_at(
            SceneInputOperation::NavigationFrame {
                now: Duration::from_millis(56),
            },
            Duration::from_millis(56),
        );
        assert_eq!(trace.navigation_time, Duration::from_millis(56));
        assert_eq!(trace.viewport, initial);
    }
    #[test]
    fn fixture_exercises_cancellation_redirect_and_dead_generation() {
        let mut mirror = Mirror::new();
        mirror.replay(SceneInputOperation::Focus("a".into()));
        mirror.policy.block_space.set(true);
        let key = |key| {
            SceneInputOperation::Canvas(CanvasEvent::KeyDown {
                key,
                modifiers: Modifiers::default(),
            })
        };
        let first = mirror.replay(key(KeyCode::Space));
        assert!(first.default.default_prevented);
        assert!(!mirror.replay(key(KeyCode::Space)).space_held);
        let release = mirror.replay(SceneInputOperation::Canvas(CanvasEvent::KeyUp {
            key: KeyCode::Space,
            modifiers: Modifiers::default(),
        }));
        assert!(release.default.default_prevented);
        assert!(!release.space_held);
        assert!(mirror.replay(key(KeyCode::Space)).space_held);
        let redirect = mirror.replay(key(KeyCode::Character('r')));
        assert_eq!(redirect.focused, Some("b".into()));
        assert!(!redirect.space_held);
        let stale = mirror.replay(key(KeyCode::Character('d')));
        assert!(!stale.errors.is_empty());
        assert_eq!(stale.focused, Some("b".into()));
    }
}

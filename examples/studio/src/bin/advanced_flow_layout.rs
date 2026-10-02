//! LAY-03 native fixture: actual next-frame staging, native baselines and receipts.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowFrameEvent, FlowTheme, GpuiTextMeasurer, SceneView, SceneViewEvent,
    engine::{EngineError, LayoutProvider, SceneEngine},
    layout::{AlignItems as FlowAlign, *},
    scene::{self, Affine, Color, DisplayObject, Scene, Shape, Size as SceneSize, Style, display},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

struct NativeMetrics {
    native: GpuiTextMeasurer,
    fail: AtomicBool,
    calls: Mutex<Vec<String>>,
}
impl TextMeasurer for NativeMetrics {
    fn measure(&self, request: &TextMeasureRequest) -> Result<TextMetrics, String> {
        self.calls.lock().unwrap().push(request.content.clone());
        if self.fail.load(Ordering::SeqCst) {
            return Err("deliberate recoverable native backend outage".into());
        }
        self.native.measure(request)
    }
    fn first_baseline(&self, request: &TextMeasureRequest) -> Result<f32, String> {
        self.native.first_baseline(request)
    }
    fn invalidate(&self) {
        self.native.invalidate();
    }
}
struct OtherOwner(Arc<AtomicBool>);
impl LayoutProvider for OtherOwner {
    fn derive(&self, _: &SceneEngine, candidate: &Scene) -> Result<Scene, EngineError> {
        let mut next = candidate.clone();
        if self.0.load(Ordering::SeqCst) {
            next.update_object(&"half".into(), |o| {
                let Shape::Rect(r) = &mut o.shape else {
                    unreachable!()
                };
                r.size.width = 111.;
            })?;
        }
        Ok(next)
    }
}
fn text_request(text: &display::Text) -> TextMeasureRequest {
    TextMeasureRequest {
        content: text.content.clone(),
        font_family: text.font_family.clone(),
        font_size: text.font_size,
        font_weight: text.font_weight,
        italic: text.italic,
        text_case: Default::default(),
        para_spacing: text.para_spacing,
        line_height: text.font_size * text.line_height,
        wrap_width: None,
    }
}
fn insert(scene: &mut Scene, object: DisplayObject, parent: Option<&str>) {
    scene.insert(object, parent.map(Into::into), None).unwrap();
}
fn label(scene: &mut Scene, id: &str, content: &str, x: f32, y: f32) {
    let mut text = display::Text::new(content, SceneSize::new(1100., 28.));
    text.font_size = 17.;
    insert(
        scene,
        DisplayObject::new(id, text)
            .with_transform(Affine::translation(x, y))
            .with_style(Style::fill(Color::hex(0xcbd7e8))),
        None,
    );
}
fn document(native: &NativeMetrics) -> (Scene, FlowLayout) {
    let mut scene = Scene::new();
    let mut layout = FlowLayout::default();
    label(
        &mut scene,
        "percent-label",
        "FIXED CONTENT: 50% + 25%; padding 20; outer width 620",
        30.,
        20.,
    );
    label(
        &mut scene,
        "shrink-label",
        "shrinkContent: over-padding",
        740.,
        20.,
    );
    label(
        &mut scene,
        "baseline-label",
        "NATIVE FIRST BASELINE: 18 / 34 / 52 px; different top padding",
        30.,
        225.,
    );
    label(&mut scene, "stable-label", "INDEPENDENT BRANCH", 930., 225.);
    label(
        &mut scene,
        "instructions",
        "Batch 20: one next-frame commit. Local text: only one of four containers. Undo: exact snapshot.",
        30.,
        470.,
    );
    for (id, x, y, w, h) in [
        ("percent", 30., 65., 620., 130.),
        ("shrink", 740., 65., 220., 120.),
        ("baseline", 30., 275., 840., 150.),
        ("stable", 930., 275., 240., 150.),
    ] {
        insert(
            &mut scene,
            DisplayObject::new(id, display::Frame::new(w, h))
                .with_transform(Affine::translation(x, y))
                .with_style(Style::fill(Color::hex(0x213249))),
            None,
        );
        layout.containers.insert(
            id.into(),
            FlowStyle {
                padding: Insets::all(20.),
                gap: 16.,
                ..Default::default()
            },
        );
    }
    for (id, fraction, color) in [("half", 0.5, 0x47b4b0), ("quarter", 0.25, 0x8b8ddd)] {
        insert(
            &mut scene,
            DisplayObject::new(id, display::Rect::new(20., 20.))
                .with_style(Style::fill(Color::hex(color))),
            Some("percent"),
        );
        layout.items.insert(
            id.into(),
            FlowItem {
                width_percent: Some(fraction),
                height_percent: Some(0.5),
                ..Default::default()
            },
        );
    }
    let shrink = layout.containers.get_mut(&"shrink".into()).unwrap();
    shrink.content_bounds_policy = ContentBoundsPolicy::LeaferShrink;
    shrink.padding = Insets {
        left: 180.,
        right: 100.,
        top: 90.,
        bottom: 60.,
    };
    insert(
        &mut scene,
        DisplayObject::new("shrink-half", display::Rect::new(20., 20.))
            .with_style(Style::fill(Color::hex(0xe4ae68))),
        Some("shrink"),
    );
    layout.items.insert(
        "shrink-half".into(),
        FlowItem {
            width_percent: Some(0.5),
            height_percent: Some(0.5),
            ..Default::default()
        },
    );
    layout
        .containers
        .get_mut(&"baseline".into())
        .unwrap()
        .align_items = FlowAlign::Baseline;
    for (id, content, size, padding, color) in [
        ("small", "Small Ag", 18., 12., 0x7ee5d4),
        ("medium", "Medium Ag", 34., 3., 0xb7b5ff),
        ("large", "Large Ag", 52., 7., 0xf1bb79),
        ("stable-text", "Untouched Ag", 22., 0., 0xd6e2ef),
    ] {
        let mut text = display::Text::new(content, SceneSize::new(200., 100.));
        text.font_family = "DejaVu Sans".into();
        text.font_size = size;
        text.line_height = 1.5;
        text.wrap = false;
        text.padding = [padding, 0., 0., 0.];
        insert(
            &mut scene,
            DisplayObject::new(id, text).with_style(Style::fill(Color::hex(color))),
            Some(if id == "stable-text" {
                "stable"
            } else {
                "baseline"
            }),
        );
        layout.texts.insert(id.into(), TextSizing::Natural);
    }
    // The guide follows the largest participant, whose metrics are unchanged by
    // the local-text button. It is paint-only and excluded from Flow ownership.
    let top = ["small", "medium", "large"]
        .into_iter()
        .map(|id| {
            let Shape::Text(t) = &scene.get(&id.into()).unwrap().object.shape else {
                unreachable!()
            };
            t.padding[0] + native.first_baseline(&text_request(t)).unwrap()
        })
        .fold(0., f32::max)
        + 20.;
    insert(
        &mut scene,
        DisplayObject::new("baseline-guide", display::Rect::new(795., 1.))
            .with_transform(Affine::translation(20., top))
            .with_style(Style::fill(Color::hex(0xf4d381))),
        Some("baseline"),
    );
    layout.items.insert(
        "baseline-guide".into(),
        FlowItem {
            in_flow: false,
            ..Default::default()
        },
    );
    (scene, layout)
}
fn width(scene: &Scene, id: &str) -> f32 {
    scene
        .get(&id.into())
        .unwrap()
        .object
        .shape
        .local_bounds()
        .unwrap()
        .size
        .width
}
fn resize(scene: &mut Scene, value: f32) -> Result<(), scene::SceneError> {
    scene.update_object(&"percent".into(), |o| {
        let Shape::Frame(frame) = &mut o.shape else {
            unreachable!()
        };
        frame.size.width = value;
    })
}
fn verify_geometry(scene: &Scene, layout: &FlowLayout, backend: &NativeMetrics) {
    assert!((width(scene, "half") - (width(scene, "percent") - 40.) * 0.5).abs() < 0.01);
    assert!((width(scene, "quarter") - (width(scene, "percent") - 40.) * 0.25).abs() < 0.01);
    let b = layout.content_bounds(scene, &"shrink".into()).unwrap();
    assert_eq!(
        (b.origin.x, b.origin.y, b.size.width, b.size.height),
        (120., 60., 60., 30.)
    );
    assert_eq!(width(scene, "shrink"), 220.);
    assert_eq!(width(scene, "shrink-half"), 30.);
    let baselines: Vec<_> = ["small", "medium", "large"]
        .into_iter()
        .map(|id| {
            let o = &scene.get(&id.into()).unwrap().object;
            let Shape::Text(t) = &o.shape else {
                unreachable!()
            };
            o.transform.ty + t.padding[0] + backend.native.first_baseline(&text_request(t)).unwrap()
        })
        .collect();
    assert!(baselines.iter().all(|b| (b - baselines[0]).abs() < 0.01));
}
struct Expected {
    action: &'static str,
    scene: Scene,
    revision: u64,
    edits: usize,
    error: Option<&'static str>,
    local: bool,
}
struct Lab {
    view: Entity<SceneView>,
    layout: FlowLayout,
    measured: MeasuredFlowLayout,
    backend: Arc<NativeMetrics>,
    expected: Option<Expected>,
    undo_snapshot: Option<Scene>,
    callbacks: usize,
    settled: bool,
    status: String,
    _subscription: Subscription,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let backend = Arc::new(NativeMetrics {
            native: GpuiTextMeasurer::new(cx.text_system().clone()),
            fail: AtomicBool::new(false),
            calls: Mutex::new(vec![]),
        });
        let (mut scene, layout) = document(&backend);
        let measured = layout.clone().with_text_measurer(backend.clone());
        measured.apply_to_scene(&mut scene).unwrap();
        let view = cx.new(|cx| {
            let mut view = SceneView::new(scene, cx).unwrap();
            view.set_editable(false, cx);
            view.show_grid = false;
            view.set_flow_frame_queue(Some(FlowFrameQueue::measured(measured.clone())), cx)
                .unwrap();
            view
        });
        let subscription = cx.subscribe(&view, |this: &mut Self, _, event, cx| {
            if let SceneViewEvent::FlowFrame(event) = event {
                this.completed(event, cx);
            }
        });
        let this = Self {
            view,
            layout,
            measured,
            backend,
            expected: None,
            undo_snapshot: None,
            callbacks: 0,
            settled: false,
            status: "Ready: first click Batch 20, then Local text, then Undo".into(),
            _subscription: subscription,
        };
        this.verify_geometry(cx);
        this.record("initial", serde_json::json!({}), cx);
        this
    }
    fn verify_geometry(&self, cx: &App) {
        verify_geometry(
            self.view.read(cx).editor.scene(),
            &self.layout,
            &self.backend,
        );
    }

    fn record(&self, action: &str, details: serde_json::Value, cx: &App) {
        let view = self.view.read(cx);
        eprintln!(
            "{}",
            serde_json::json!({"fixture":"advanced-flow-layout", "action":action, "callbacks":self.callbacks, "revision":view.editor.engine.revision(), "pending":view.pending_flow_edits(), "width":width(view.editor.scene(), "percent"), "half":width(view.editor.scene(), "half"), "details":details})
        );
    }
    fn expect(
        &mut self,
        action: &'static str,
        edits: usize,
        error: Option<&'static str>,
        local: bool,
        cx: &App,
    ) {
        let v = self.view.read(cx);
        self.expected = Some(Expected {
            action,
            scene: v.editor.scene().clone(),
            revision: v.editor.engine.revision(),
            edits,
            error,
            local,
        });
        self.backend.calls.lock().unwrap().clear();
    }
    fn completed(&mut self, event: &FlowFrameEvent, cx: &mut Context<Self>) {
        self.callbacks += 1;
        let expected = self
            .expected
            .take()
            .expect("unexpected or duplicate native frame callback");
        let v = self.view.read(cx);
        assert_eq!(event.edits, expected.edits);
        assert_eq!(event.revision_before, expected.revision);
        let details = match &event.result {
            Err(error) => {
                assert!(
                    error.contains(expected.error.expect("unexpected frame failure")),
                    "{error}"
                );
                assert_eq!(v.editor.scene(), &expected.scene);
                assert_eq!(event.revision_after, expected.revision);
                assert_eq!(v.pending_flow_edits(), expected.edits);
                self.status = format!(
                    "PASS {}: atomic rejection, scene/revision unchanged; pending retained",
                    expected.action
                );
                serde_json::json!({"error":error,"atomic":true,"edits":event.edits})
            }
            Ok(Some(report)) => {
                assert!(expected.error.is_none());
                assert_eq!(event.revision_after, expected.revision + 1);
                assert_eq!(v.pending_flow_edits(), 0);
                let calls = self.backend.calls.lock().unwrap().clone();
                if expected.local {
                    assert_eq!(report.containers, 1);
                    assert!(!calls.iter().any(|text| text == "Untouched Ag"));
                    assert_eq!(
                        v.editor.scene().get(&"stable".into()),
                        expected.scene.get(&"stable".into())
                    );
                    assert_eq!(
                        v.editor.scene().get(&"stable-text".into()),
                        expected.scene.get(&"stable-text".into())
                    );
                }
                self.settled = true;
                self.undo_snapshot = Some(expected.scene);
                self.status = format!(
                    "PASS {}: {} edits / one callback / one revision; {} containers",
                    expected.action, event.edits, report.containers
                );
                serde_json::json!({"edits":event.edits,"containers":report.containers,"items":report.items,"passes":report.passes,"changed":report.changed,"measure_calls":calls,"local":expected.local})
            }
            Ok(None) => panic!("scheduled nonempty batch unexpectedly empty"),
        };
        if event.result.is_ok() {
            self.verify_geometry(cx);
        }
        self.record(expected.action, details, cx);
        cx.notify();
    }
    fn clean_queue(&mut self, cx: &mut Context<Self>) {
        self.backend.fail.store(false, Ordering::SeqCst);
        self.view.update(cx, |v, cx| {
            if let Some(id) = v.editor.engine.layout_provider() {
                v.editor.engine.remove_layout_provider(id);
            }
            v.set_flow_frame_queue(Some(FlowFrameQueue::measured(self.measured.clone())), cx)
                .unwrap();
        });
        self.expected = None;
        self.undo_snapshot = None;
        self.settled = false;
    }
    fn batch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.expect("batch20", 20, None, false, cx);
        let initial = width(self.view.read(cx).editor.scene(), "percent");
        let target = if initial > 600. { 500. } else { 620. };
        self.view.update(cx, |v, cx| {
            for i in 1..=20 {
                v.enqueue_flow_edit(
                    |s| resize(s, initial + (target - initial) * i as f32 / 20.),
                    window,
                    cx,
                )
                .unwrap();
            }
        });
        let expected = self.expected.as_ref().unwrap();
        assert_eq!(self.view.read(cx).editor.scene(), &expected.scene);
        assert_eq!(
            self.view.read(cx).editor.engine.revision(),
            expected.revision
        );
        assert_eq!(self.view.read(cx).pending_flow_edits(), 20);
        self.record(
            "batch20-staged-uncommitted",
            serde_json::json!({"edits":20}),
            cx,
        );
    }
    fn local(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.expect("local-text", 1, None, true, cx);
        self.view.update(cx, |v, cx| {
            v.enqueue_flow_edit(
                |s| {
                    s.update_object(&"small".into(), |o| {
                        let Shape::Text(t) = &mut o.shape else {
                            unreachable!()
                        };
                        t.content = if t.content == "Small Ag" {
                            "Small edited Ag"
                        } else {
                            "Small Ag"
                        }
                        .into();
                    })
                },
                window,
                cx,
            )
            .unwrap()
        });
    }
    fn failure(&mut self, mode: u8, window: &mut Window, cx: &mut Context<Self>) {
        self.clean_queue(cx);
        if mode == 0 {
            let active = Arc::new(AtomicBool::new(false));
            self.view.update(cx, |v, _| {
                v.editor
                    .engine
                    .install_layout_provider(OtherOwner(active.clone()))
                    .unwrap();
            });
            active.store(true, Ordering::SeqCst);
        }
        if mode == 2 {
            self.backend.fail.store(true, Ordering::SeqCst);
        }
        self.expect(
            match mode {
                0 => "ownership",
                1 => "stale-reset",
                _ => "backend-outage",
            },
            1,
            Some(match mode {
                0 => "conflicts",
                1 => "stale",
                _ => "outage",
            }),
            false,
            cx,
        );
        let target = width(self.view.read(cx).editor.scene(), "percent") + 20.;
        self.view.update(cx, |v, cx| {
            v.enqueue_flow_edit(|s| resize(s, target), window, cx)
                .unwrap();
            if mode == 1 {
                let old = v.editor.engine.handle(&"half".into()).unwrap();
                v.editor
                    .engine
                    .reset_scene(v.editor.scene().clone())
                    .unwrap();
                assert!(v.editor.engine.object(old).is_err());
                self.expected.as_mut().unwrap().revision = v.editor.engine.revision();
            }
        });
        self.record("failure-staged", serde_json::json!({"mode":mode}), cx);
    }
    fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view.read(cx).pending_flow_edits() == 0 {
            return;
        }
        self.backend.fail.store(false, Ordering::SeqCst);
        self.expect("backend-retry", 1, None, false, cx);
        self.view
            .update(cx, |v, cx| v.retry_flow_frame(window, cx).unwrap());
    }
    fn cancel_or_replace(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.clean_queue(cx);
        let before = self.view.read(cx).editor.scene().clone();
        let revision = self.view.read(cx).editor.engine.revision();
        let callbacks = self.callbacks;
        self.view.update(cx, |v, cx| {
            v.enqueue_flow_edit(|s| resize(s, 333.), window, cx)
                .unwrap();
            if replace {
                v.set_flow_frame_queue(Some(FlowFrameQueue::measured(self.measured.clone())), cx)
                    .unwrap();
            } else {
                assert!(v.cancel_flow_frame(cx).unwrap());
            }
        });
        let entity = cx.entity().downgrade();
        // A verification callback after the invalidated bridge callback proves
        // that the native frame ran without retargeting or emitting a receipt.
        window.on_next_frame(move |_, cx| {
            let _ = entity.update(cx, |this, cx| {
                let view = this.view.read(cx);
                assert_eq!(view.editor.scene(), &before);
                assert_eq!(view.editor.engine.revision(), revision);
                assert_eq!(view.pending_flow_edits(), 0);
                assert_eq!(this.callbacks, callbacks);
                this.status = format!(
                    "PASS {}: old native callback ignored, no commit",
                    if replace { "replace" } else { "cancel" }
                );
                this.record(
                    if replace {
                        "replace-guard"
                    } else {
                        "cancel-guard"
                    },
                    serde_json::json!({"unchanged":true}),
                    cx,
                );
                cx.notify();
            });
        });
    }
    fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(expected) = self.undo_snapshot.take() else {
            return;
        };
        self.view.update(cx, |v, cx| {
            assert!(v.editor.undo());
            assert_eq!(v.editor.scene(), &expected);
            cx.notify();
        });
        self.settled = false;
        self.status = "PASS Undo: exact pre-batch scene restored in one step".into();
        self.record("undo-exact", serde_json::json!({"exact":true}), cx);
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        let pending = self.view.read(cx).pending_flow_edits();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.foreground))
            .font_family("sans-serif")
            .child(
                div()
                    .h(px(60.))
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        ControlButton::new("batch", "Batch 20")
                            .disabled(pending != 0)
                            .on_click(cx.listener(|s, _, w, cx| s.batch(w, cx))),
                    )
                    .child(
                        ControlButton::new("local", "Local text")
                            .disabled(pending != 0 || !self.settled)
                            .on_click(cx.listener(|s, _, w, cx| s.local(w, cx))),
                    )
                    .child(
                        ControlButton::new("undo", "Undo")
                            .disabled(self.undo_snapshot.is_none() || pending != 0)
                            .on_click(cx.listener(|s, _, _, cx| s.undo(cx))),
                    )
                    .child(
                        ControlButton::new("owner", "Owner conflict")
                            .on_click(cx.listener(|s, _, w, cx| s.failure(0, w, cx))),
                    )
                    .child(
                        ControlButton::new("stale", "Stale reset")
                            .on_click(cx.listener(|s, _, w, cx| s.failure(1, w, cx))),
                    )
                    .child(
                        ControlButton::new("outage", "Backend fail")
                            .on_click(cx.listener(|s, _, w, cx| s.failure(2, w, cx))),
                    )
                    .child(
                        ControlButton::new("retry", "Retry")
                            .disabled(!self.backend.fail.load(Ordering::SeqCst))
                            .on_click(cx.listener(|s, _, w, cx| s.retry(w, cx))),
                    ),
            )
            .child(
                div()
                    .h(px(50.))
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        ControlButton::new("cancel", "Cancel staged")
                            .on_click(cx.listener(|s, _, w, cx| s.cancel_or_replace(false, w, cx))),
                    )
                    .child(
                        ControlButton::new("replace", "Replace staged")
                            .on_click(cx.listener(|s, _, w, cx| s.cancel_or_replace(true, w, cx))),
                    )
                    .child(
                        ControlButton::new("recover", "Recover queue").on_click(cx.listener(
                            |s, _, _, cx| {
                                s.clean_queue(cx);
                                s.status = "Queue reset; stage Batch 20 before Local text".into();
                                cx.notify();
                            },
                        )),
                    )
                    .child(format!(
                        "LAY-03 | native callbacks {} | pending {}",
                        self.callbacks, pending
                    )),
            )
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(
                div()
                    .h(px(48.))
                    .px_4()
                    .flex()
                    .items_center()
                    .text_sm()
                    .child(self.status.clone()),
            )
    }
}
fn main() {
    if std::env::args().any(|arg| arg == "--probe") {
        let app = gpui_kit::platform::headless();
        let backend = Arc::new(NativeMetrics {
            native: GpuiTextMeasurer::new(app.text_system()),
            fail: AtomicBool::new(false),
            calls: Mutex::new(vec![]),
        });
        let (mut scene, layout) = document(&backend);
        let measured = layout.clone().with_text_measurer(backend.clone());
        let report = measured.apply_to_scene(&mut scene).unwrap();
        verify_geometry(&scene, &layout, &backend);
        assert_eq!(report.containers, 4);
        assert!(measured.arranged(&scene).unwrap().1.changed.is_empty());
        eprintln!(
            "{}",
            serde_json::json!({"fixture":"advanced-flow-layout", "action":"native-probe", "containers":report.containers, "passes":report.passes, "half":width(&scene,"half"), "quarter":width(&scene,"quarter"), "native_baselines_equal":true, "window_opened":false})
        );
        return;
    }
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1240.), px(820.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · advanced layout LAY-03");
                    gpui_kit::component::Theme::change(
                        gpui_kit::component::ThemeMode::Dark,
                        Some(window),
                        cx,
                    );
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}

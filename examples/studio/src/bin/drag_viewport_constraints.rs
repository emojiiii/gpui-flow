//! INT-06 native acceptance fixture. --trace emits JSON lines from actual input
//! and native frame callbacks. This program does not synthesize physical input.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneInputOperation, SceneInputTrace, SceneView, SceneViewEvent,
    core::{Point as P, Rect as R, Size as S, Viewport},
    editor::SceneEditor,
    interaction::{
        CanvasEvent, SelectionMode,
        scene::{SceneCommand, ScenePluginHost, plugins::*, selection_bounds},
    },
    scene::{Affine, Color, DisplayObject, HitPolicy, Scene, Style, display},
};
use serde_json::{Value, json};
use std::collections::VecDeque;

const CANVAS: S = S::new(1000., 600.);
const WORLD: R = R::new(P::ZERO, S::new(1200., 800.));
const PARENT: R = R::new(P::ZERO, S::new(600., 360.));
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    World,
    Parent,
}
impl Mode {
    fn label(self) -> &'static str {
        match self {
            Self::World => "World bounds",
            Self::Parent => "Rotated parent-local bounds",
        }
    }
    fn bounds(self) -> DragBounds {
        match self {
            Self::World => DragBounds::World(WORLD),
            Self::Parent => DragBounds::ParentLocal(PARENT),
        }
    }
}
fn scene(mode: Mode) -> Scene {
    let mut scene = Scene::new();
    let mut outline = DisplayObject::new(
        "world-outline",
        display::Rect::new(WORLD.size.width, WORLD.size.height),
    )
    .with_style(Style::stroke(Color::hex(0x54667c), 2.));
    outline.hit_policy = HitPolicy::None;
    outline.locked = true;
    scene.insert(outline, None, None).unwrap();
    if mode == Mode::Parent {
        let mut parent = DisplayObject::new("parent", display::Group::new()).with_transform(
            Affine::translation(270., 110.) * Affine::rotation(18.) * Affine::scale(1.05, 0.9),
        );
        parent.hit_policy = HitPolicy::None;
        scene.insert(parent, None, None).unwrap();
        let mut boundary = DisplayObject::new(
            "parent-outline",
            display::Rect::new(PARENT.size.width, PARENT.size.height),
        )
        .with_style(Style::stroke(Color::hex(0xdcb66b), 2.));
        boundary.hit_policy = HitPolicy::None;
        boundary.locked = true;
        scene.insert(boundary, Some("parent".into()), None).unwrap();
    }
    for (id, x, y, width, height, color) in [
        ("a", 80., 80., 120., 80., 0x51a8dc),
        ("b", 320., 130., 160., 100., 0xbc84df),
    ] {
        scene
            .insert(
                DisplayObject::new(id, display::Rect::new(width, height))
                    .with_transform(Affine::translation(x, y))
                    .with_style(Style::fill(Color::hex(color))),
                (mode == Mode::Parent).then(|| "parent".into()),
                None,
            )
            .unwrap();
    }
    scene
}
fn configure(host: &mut ScenePluginHost, editor: &mut SceneEditor, mode: Mode) {
    host.replace(
        ScenePanPlugin::default().with_inertia(PanInertiaOptions::default()),
        editor,
    )
    .unwrap();
    host.install(SceneDragConstraintsPlugin {
        constraints: SceneDragConstraints {
            bounds: Some(mode.bounds()),
            axis: DragAxis::Both,
        },
    })
    .unwrap();
    host.install(SceneSnappingPlugin::default()).unwrap();
    host.install(SceneEdgeSnappingPlugin::default()).unwrap();
    host.install(SceneViewportConstraintsPlugin {
        constraints: SceneViewportConstraints {
            min_zoom: Some(0.5),
            max_zoom: Some(2.),
            bounds: Some(ViewportBounds {
                world: WORLD,
                canvas: CANVAS,
            }),
        },
    })
    .unwrap();
    // This fixture isolates translation; its overlay must not offer handles that
    // resize/rotate the object outside the MoveSelectionTo constraint contract.
    for id in ["resize", "rotate", "line-endpoints"] {
        if host.plugin_ids().contains(&id) {
            host.set_enabled(id, false, editor).unwrap();
        }
    }
    editor.selection.select("a".into(), false);
}
fn operation_value(operation: &SceneInputOperation) -> Value {
    let modifiers = |m: gpui_flow::interaction::Modifiers| json!({"shift":m.shift,"control":m.control,"alt":m.alt,"meta":m.meta,"space":m.space});
    match operation {
        SceneInputOperation::NavigationFrame { now } => {
            json!({"kind":"NavigationFrame","secs":now.as_secs(),"nanos":now.subsec_nanos()})
        }
        SceneInputOperation::PickedWheel {
            position,
            delta,
            modifiers: m,
            target,
        } => {
            json!({"kind":"PickedWheel","position":position,"delta":delta,"modifiers":modifiers(*m),"target":target})
        }
        SceneInputOperation::Focus(id) => json!({"kind":"Focus","target":id}),
        SceneInputOperation::ClearFocus => json!({"kind":"ClearFocus"}),
        SceneInputOperation::FocusDeclined(id) => json!({"kind":"FocusDeclined","target":id}),
        SceneInputOperation::Canvas(event) => match event {
            CanvasEvent::PointerDown(p)
            | CanvasEvent::PointerMove(p)
            | CanvasEvent::PointerUp(p) => {
                json!({"kind":match event {CanvasEvent::PointerDown(_)=>"PointerDown",CanvasEvent::PointerMove(_)=>"PointerMove",_=>"PointerUp"},"position":p.position,"pointer_id":p.pointer_id,"button":format!("{:?}",p.button),"hit":format!("{:?}",p.hit),"modifiers":modifiers(p.modifiers)})
            }
            CanvasEvent::Wheel {
                position,
                delta,
                modifiers: m,
            } => {
                json!({"kind":"Wheel","position":position,"delta":delta,"modifiers":modifiers(*m)})
            }
            CanvasEvent::KeyDown { key, modifiers: m }
            | CanvasEvent::KeyUp { key, modifiers: m } => {
                json!({"kind":if matches!(event,CanvasEvent::KeyDown {..}) {"KeyDown"} else {"KeyUp"},"key":format!("{key:?}"),"modifiers":modifiers(*m)})
            }
            CanvasEvent::PointerLeave => json!({"kind":"PointerLeave"}),
            CanvasEvent::PointerCancel => json!({"kind":"PointerCancel"}),
            CanvasEvent::FocusLost => json!({"kind":"FocusLost"}),
        },
    }
}
fn trace_value(trace: &SceneInputTrace) -> Value {
    json!({"operation":operation_value(&trace.operation),"navigation_time":{"secs":trace.navigation_time.as_secs(),"nanos":trace.navigation_time.subsec_nanos()},"pan":trace.viewport.pan,"zoom":trace.viewport.zoom,"selection":trace.selection,"capture":trace.capture_owner,"editing":trace.editing,"pressed":trace.pressed,"focused":trace.focused,"default_prevented":trace.default.default_prevented,"document_changed":trace.default.document_changed,"viewport_changed":trace.default.viewport_changed,"errors":trace.errors,"default_errors":trace.default.errors.iter().map(ToString::to_string).collect::<Vec<_>>()})
}
fn observed_value(view: &SceneView) -> Value {
    let bounds = selection_bounds(&view.editor);
    let objects: Vec<_> = ["a", "b"].iter().filter_map(|id| view.editor.scene().get(&(*id).into()).map(|node| json!({"id":id,"selected":view.editor.selection.ids.contains(&(*id).into()),"parent":node.parent,"transform":node.object.transform,"world_bounds":view.editor.scene().world_bounds(&(*id).into()).ok().flatten()}))).collect();
    json!({"scene_revision":view.editor.engine.revision(),"document_epoch":view.editor.engine.document_epoch(),"selected_bounds":bounds.as_ref().ok().and_then(|b|*b),"bounds_error":bounds.err().map(|e|e.to_string()),"objects":objects,"pan":view.editor.viewport.pan,"zoom":view.editor.viewport.zoom,"capture":view.interactions.capture_owner(),"editing":view.editor.gesture_editing(),"navigation_pending":view.interactions.needs_navigation_frame(),"editable":view.is_editable(),"grid_snap":view.interactions.is_enabled("snapping"),"edge_snap":view.interactions.is_enabled("edge-snapping"),"error":view.interaction_error,"compositing_pending":view.compositing_pending,"composite_issues":format!("{:?}",view.composite_diagnostics)})
}
#[derive(Clone, Copy)]
enum Action {
    World,
    Parent,
    Reset,
    Single,
    Pair,
    Undo,
    Redo,
    Grid,
    Edges,
    ReadOnly,
    Inertia,
    Center,
    Zoom,
    ProbeEdit,
    Stop,
}
struct Lab {
    view: Entity<SceneView>,
    mode: Mode,
    inertia: bool,
    trace: bool,
    sequence: u64,
    inputs: u64,
    frames: u64,
    moving_frames: u64,
    recent: VecDeque<String>,
    last: String,
    _subscription: Subscription,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut view = SceneView::new(scene(Mode::World), cx).unwrap();
            configure(&mut view.interactions, &mut view.editor, Mode::World);
            view.set_input_tracing(true);
            view
        });
        let subscription = cx.subscribe(&view, |this: &mut Self, _, event, cx| {
            if let SceneViewEvent::InputTrace(trace) = event {
                if matches!(trace.operation, SceneInputOperation::NavigationFrame { .. }) {
                    this.frames += 1;
                    this.moving_frames += u64::from(trace.default.viewport_changed);
                } else {
                    this.inputs += 1;
                }
                let notable = !matches!(
                    trace.operation,
                    SceneInputOperation::Canvas(CanvasEvent::PointerMove(_))
                ) || trace.capture_owner.is_some();
                if notable {
                    this.recent.push_back(format!(
                        "{}  pan ({:.1},{:.1})",
                        operation_value(&trace.operation)["kind"]
                            .as_str()
                            .unwrap_or("input"),
                        trace.viewport.pan.x,
                        trace.viewport.pan.y
                    ));
                    while this.recent.len() > 6 {
                        this.recent.pop_front();
                    }
                }
                this.record("native", Some(trace), cx);
            }
            cx.notify();
        });
        let mut lab = Self {
            view,
            mode: Mode::World,
            inertia: true,
            trace: std::env::args().any(|arg| arg == "--trace"),
            sequence: 0,
            inputs: 0,
            frames: 0,
            moving_frames: 0,
            recent: VecDeque::new(),
            last: "Ready: drag blue rectangle or flick empty canvas".into(),
            _subscription: subscription,
        };
        lab.record("startup", None, cx);
        lab
    }
    fn record(&mut self, action: &str, trace: Option<&SceneInputTrace>, cx: &App) {
        self.sequence += 1;
        if self.trace {
            let view = self.view.read(cx);
            eprintln!(
                "{}",
                json!({"fixture":"drag-viewport","seq":self.sequence,"action":action,"mode":self.mode.label(),"logical_canvas":CANVAS,"world_bounds":WORLD,"drag_bounds":format!("{:?}",self.mode.bounds()),"inertia_enabled":self.inertia,"trace":trace.map(trace_value),"observed_at_subscription":observed_value(view),"observed_viewport_matches_trace":trace.map(|t|t.viewport==view.editor.viewport),"inputs":self.inputs,"frames":self.frames,"moving_frames":self.moving_frames,"last_action":self.last})
            );
        }
    }
    fn reset(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.mode = mode;
        self.view.update(cx, |view, cx| {
            let editable = view.is_editable();
            view.set_editable(true, cx);
            view.set_scene(scene(mode), cx).unwrap();
            view.replace_plugin(
                SceneDragConstraintsPlugin {
                    constraints: SceneDragConstraints {
                        bounds: Some(mode.bounds()),
                        axis: DragAxis::Both,
                    },
                },
                cx,
            )
            .unwrap();
            view.execute_command(
                SceneCommand::SetViewport {
                    viewport: Viewport::default(),
                },
                cx,
            )
            .unwrap();
            view.execute_command(
                SceneCommand::Select {
                    id: "a".into(),
                    mode: SelectionMode::Replace,
                },
                cx,
            )
            .unwrap();
            view.set_editable(editable, cx);
            view.interaction_error = None;
        });
        self.last = format!("Reset {} (history and old capture cleared)", mode.label());
    }
    fn action(&mut self, action: Action, cx: &mut Context<Self>) {
        match action {
            Action::World => self.reset(Mode::World, cx),
            Action::Parent => self.reset(Mode::Parent, cx),
            Action::Reset => self.reset(self.mode, cx),
            _ => {
                let inertia = self.inertia;
                let result = self.view.update(cx, |view, cx| -> Result<String, String> {
                    view.execute_command(SceneCommand::CancelGesture, cx)
                        .map_err(|e| e.to_string())?;
                    view.interaction_error = None;
                    match action {
                        Action::Single | Action::Pair => {
                            view.execute_command(
                                SceneCommand::Select {
                                    id: "a".into(),
                                    mode: SelectionMode::Replace,
                                },
                                cx,
                            )
                            .map_err(|e| e.to_string())?;
                            if matches!(action, Action::Pair) {
                                view.execute_command(
                                    SceneCommand::Select {
                                        id: "b".into(),
                                        mode: SelectionMode::Add,
                                    },
                                    cx,
                                )
                                .map_err(|e| e.to_string())?;
                            }
                        }
                        Action::Undo => view
                            .execute_command(SceneCommand::Undo, cx)
                            .map_err(|e| e.to_string())?,
                        Action::Redo => view
                            .execute_command(SceneCommand::Redo, cx)
                            .map_err(|e| e.to_string())?,
                        Action::Grid | Action::Edges => {
                            let id = if matches!(action, Action::Grid) {
                                "snapping"
                            } else {
                                "edge-snapping"
                            };
                            view.set_plugin_enabled(id, !view.interactions.is_enabled(id), cx)
                                .map_err(|e| e.to_string())?;
                        }
                        Action::ReadOnly => view.set_editable(!view.is_editable(), cx),
                        Action::Inertia => {
                            let plugin = if inertia {
                                ScenePanPlugin::default()
                            } else {
                                ScenePanPlugin::default().with_inertia(PanInertiaOptions::default())
                            };
                            view.replace_plugin(plugin, cx).map_err(|e| e.to_string())?;
                        }
                        Action::Center => view
                            .execute_command(
                                SceneCommand::SetViewport {
                                    viewport: Viewport {
                                        pan: P::new(-100., -100.),
                                        ..Default::default()
                                    },
                                },
                                cx,
                            )
                            .map_err(|e| e.to_string())?,
                        Action::Zoom => view
                            .execute_command(
                                SceneCommand::Zoom {
                                    position: P::new(500., 300.),
                                    zoom: if view.editor.viewport.zoom < 1.25 {
                                        2.
                                    } else {
                                        0.5
                                    },
                                },
                                cx,
                            )
                            .map_err(|e| e.to_string())?,
                        Action::ProbeEdit => {
                            let bounds = selection_bounds(&view.editor)
                                .map_err(|e| e.to_string())?
                                .ok_or("Select a rectangle first")?;
                            view.execute_command(
                                SceneCommand::MoveSelectionTo {
                                    anchor: bounds.origin + P::new(20., 0.),
                                },
                                cx,
                            )
                            .map_err(|e| e.to_string())?;
                        }
                        Action::Stop => {}
                        _ => unreachable!(),
                    }
                    Ok("Toolbar action accepted".into())
                });
                if matches!(action, Action::Inertia) && result.is_ok() {
                    self.inertia = !inertia;
                }
                self.last = result.unwrap_or_else(|e| format!("Rejected: {e}"));
            }
        }
        self.record(
            match action {
                Action::World => "mode-world",
                Action::Parent => "mode-parent",
                Action::Reset => "reset",
                Action::Single => "select-a",
                Action::Pair => "select-pair",
                Action::Undo => "undo",
                Action::Redo => "redo",
                Action::Grid => "toggle-grid",
                Action::Edges => "toggle-edge",
                Action::ReadOnly => "toggle-readonly",
                Action::Inertia => "toggle-inertia",
                Action::Center => "center",
                Action::Zoom => "zoom",
                Action::ProbeEdit => "probe-edit",
                Action::Stop => "stop",
            },
            None,
            cx,
        );
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.view.read(cx);
        let bounds = selection_bounds(&view.editor).ok().flatten();
        let mut lines = vec![
            self.mode.label().to_owned(),
            format!(
                "pan ({:.2}, {:.2})",
                view.editor.viewport.pan.x, view.editor.viewport.pan.y
            ),
            format!("zoom {:.3}  range 0.5..2", view.editor.viewport.zoom),
            format!("revision {}", view.editor.engine.revision()),
            format!("capture {:?}", view.interactions.capture_owner()),
            format!(
                "edit={}  pending={}",
                view.editor.gesture_editing(),
                view.interactions.needs_navigation_frame()
            ),
            format!("selected bounds: {bounds:?}"),
            format!("inputs {} / frames {}", self.inputs, self.frames),
            format!("moving frames {}", self.moving_frames),
        ];
        for id in ["a", "b"] {
            if let Some(node) = view.editor.scene().get(&id.into()) {
                let t = node.object.transform;
                lines.push(format!(
                    "{id} local ({:.1},{:.1}) [{:.2},{:.2},{:.2},{:.2}]",
                    t.tx, t.ty, t.a, t.b, t.c, t.d
                ));
            }
        }
        lines.push(format!("error: {:?}", view.interaction_error));
        let grid = format!(
            "Grid: {}",
            if view.interactions.is_enabled("snapping") {
                "on"
            } else {
                "off"
            }
        );
        let edge = format!(
            "Edges: {}",
            if view.interactions.is_enabled("edge-snapping") {
                "on"
            } else {
                "off"
            }
        );
        let readonly = if view.is_editable() {
            "Read-only: off"
        } else {
            "Read-only: on"
        };
        let inertia = if self.inertia {
            "Inertia: on"
        } else {
            "Inertia: off"
        };
        let toolbar = |items: Vec<(&'static str, String, Action)>| {
            div()
                .h(px(42.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .children(items.into_iter().map(|(id, label, action)| {
                    ControlButton::new(id, label)
                        .on_click(cx.listener(move |this, _, _, cx| this.action(action, cx)))
                }))
        };
        div().size_full().flex().flex_col().overflow_hidden().bg(rgb(0x17212f)).text_color(rgb(0xe4edf7))
            .child(toolbar(vec![("world","World mode".into(),Action::World),("parent","Parent mode".into(),Action::Parent),("reset","Reset".into(),Action::Reset),("single","Select A".into(),Action::Single),("pair","Select A+B".into(),Action::Pair),("undo","Undo".into(),Action::Undo),("redo","Redo".into(),Action::Redo)]))
            .child(toolbar(vec![("grid",grid,Action::Grid),("edges",edge,Action::Edges),("readonly",readonly.into(),Action::ReadOnly),("inertia",inertia.into(),Action::Inertia),("center","Center pan".into(),Action::Center),("zoom","Zoom .5 / 2".into(),Action::Zoom),("probe","Probe edit +20".into(),Action::ProbeEdit),("stop","Stop".into(),Action::Stop)]))
            .child(div().px_3().h(px(32.)).text_sm().child("Drag rectangle · Space/middle/empty: pan · Ctrl/Cmd+wheel: zoom · Escape cancels · Fixed logical canvas 1000×600"))
            .child(div().flex().gap_3().px_3()
                .child(div().w(px(CANVAS.width)).h(px(CANVAS.height)).flex_shrink_0().overflow_hidden().child(self.view.clone()))
                .child(div().w(px(270.)).h(px(CANVAS.height)).flex_shrink_0().flex().flex_col().gap_2().text_sm().overflow_hidden()
                    .children(lines.into_iter().map(|line|div().child(line)))
                    .child(div().text_color(rgb(0xd6c49b)).child("Recent actual input / frames:"))
                    .children(self.recent.iter().cloned().map(|line|div().child(line)))))
            .child(div().px_3().py_2().text_sm().child(self.last.clone()))
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
                        size(px(1340.), px(790.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    window.set_window_title("gpui-flow · Drag / viewport constraints acceptance");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn both_fixture_modes_use_real_bounds_plugins_and_atomic_navigation() {
        for mode in [Mode::World, Mode::Parent] {
            let mut editor = SceneEditor::new(scene(mode)).unwrap();
            let mut host = ScenePluginHost::with_defaults();
            configure(&mut host, &mut editor, mode);
            assert!(host.is_enabled("drag-constraints"));
            assert!(host.is_enabled("viewport-constraints"));
            assert!(!host.is_enabled("resize"));
            assert!(!host.is_enabled("rotate"));
            host.execute(
                &mut editor,
                SceneCommand::MoveSelectionTo {
                    anchor: P::new(9999., 9999.),
                },
            )
            .unwrap();
            let node = &editor.scene().get(&"a".into()).unwrap().object;
            let actual = if mode == Mode::World {
                editor.scene().world_bounds(&"a".into()).unwrap().unwrap()
            } else {
                node.transform
                    .transform_bounds(editor.scene().local_bounds(&"a".into()).unwrap().unwrap())
            };
            let limit = if mode == Mode::World { WORLD } else { PARENT };
            assert!(limit.expanded(0.001).contains_rect(actual));
            host.execute(
                &mut editor,
                SceneCommand::Zoom {
                    position: P::new(500., 300.),
                    zoom: 999.,
                },
            )
            .unwrap();
            assert_eq!(editor.viewport.zoom, 2.);
        }
    }
    #[::core::prelude::v1::test]
    fn frame_json_retains_explicit_clock_and_distinct_operation_kind() {
        let value = operation_value(&SceneInputOperation::NavigationFrame {
            now: std::time::Duration::new(3, 42),
        });
        assert_eq!(value["kind"], "NavigationFrame");
        assert_eq!(value["secs"], 3);
        assert_eq!(value["nanos"], 42);
    }
}

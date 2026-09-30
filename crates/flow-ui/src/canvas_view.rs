use crate::*;
use gpui::{prelude::*, *};
use gpui_flow_core::{
    self as model, Clipboard, Editor, Graph, Node, NodeId, PortDirection, PortRef,
};
use gpui_flow_runtime::{MockRunner, Runtime};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use std::{sync::Arc, time::Duration};

#[derive(Clone, Debug)]
pub enum FlowEvent {
    DocumentChanged,
    SelectionChanged,
    ConnectionRejected(String),
    RunChanged(String),
}
impl EventEmitter<FlowEvent> for FlowCanvas {}
#[derive(Clone)]
enum Gesture {
    Pan {
        last: model::Point,
    },
    Move {
        last: model::Point,
    },
    Resize {
        node: NodeId,
        start: model::Point,
        size: model::Size,
    },
    Marquee {
        start: model::Point,
        current: model::Point,
    },
}
/// Host-owned reusable view. Subscribe to `FlowEvent` for persistence/inspector integration.
/// Canvas coordinates are independent of window origin and account for pan and zoom.
pub struct FlowCanvas {
    pub editor: Editor,
    pub registry: NodeRegistry,
    pub theme: FlowTheme,
    pub edge_renderer: Arc<dyn EdgeRenderer>,
    pub runtime: Runtime,
    pub snap_to_grid: bool,
    pub show_grid: bool,
    focus: FocusHandle,
    origin: model::Point,
    extent: model::Size,
    gesture: Option<Gesture>,
    connecting: Option<PortRef>,
    pointer: model::Point,
    clipboard: Option<Clipboard>,
    message: String,
    snapshot: Option<String>,
    sequence: u64,
    input: Option<Entity<InputState>>,
    input_node: Option<NodeId>,
    input_subscription: Option<Subscription>,
}
impl FlowCanvas {
    pub fn new(graph: Graph, cx: &mut Context<Self>) -> Result<Self, model::GraphError> {
        let editor = Editor::new(graph)?;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        let events = this.runtime.poll();
                        if !events.is_empty() {
                            for event in events {
                                cx.emit(FlowEvent::RunChanged(event.node_id));
                            }
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Ok(Self {
            editor,
            registry: NodeRegistry::default(),
            theme: FlowTheme::default(),
            edge_renderer: Arc::new(BezierEdge),
            runtime: Runtime::default(),
            snap_to_grid: false,
            show_grid: true,
            focus: cx.focus_handle(),
            origin: model::Point::ZERO,
            extent: model::Size::new(1000., 700.),
            gesture: None,
            connecting: None,
            pointer: model::Point::ZERO,
            clipboard: None,
            message: "Ready · offline mock demo".into(),
            snapshot: None,
            sequence: 0,
            input: None,
            input_node: None,
            input_subscription: None,
        })
    }
    pub fn demo(cx: &mut Context<Self>) -> Self {
        Self::new(demo_graph(), cx).expect("valid demo graph")
    }
    pub fn export_json(&self) -> Result<String, model::GraphError> {
        self.editor.graph().to_json()
    }
    pub fn import_json(
        &mut self,
        json: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), model::GraphError> {
        let graph = Graph::from_json(json)?;
        self.editor.transaction("Import", |g| {
            *g = graph;
            Ok(())
        })?;
        self.changed(cx);
        Ok(())
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(FlowEvent::DocumentChanged);
        cx.notify();
    }
    fn report(&mut self, result: Result<(), model::GraphError>, cx: &mut Context<Self>) {
        if let Err(error) = result {
            self.message = error.to_string();
        }
        self.changed(cx);
    }
    fn local(&self, p: gpui::Point<Pixels>) -> model::Point {
        model::Point::new(p.x.into(), p.y.into()) - self.origin
    }
    fn world(&self, p: gpui::Point<Pixels>) -> model::Point {
        self.editor.viewport.screen_to_world(self.local(p))
    }
    fn fit(&mut self) {
        if let Some(bounds) = self.editor.graph().bounds() {
            self.editor.viewport.fit(bounds, self.extent, 55.);
        }
    }
    fn add(&mut self, kind: &str, cx: &mut Context<Self>) {
        self.sequence += 1;
        let id = self
            .editor
            .graph()
            .fresh_id(&format!("{kind}-{}", self.sequence));
        let p = self.editor.viewport.screen_to_world(model::Point::new(
            self.extent.width / 2. - 120.,
            self.extent.height / 2. - 90.,
        ));
        let node = builtin_node(kind, id, p);
        let result = self.editor.transaction("Add node", |g| g.add_node(node));
        self.report(result, cx);
    }
    fn run_selected(&mut self, cx: &mut Context<Self>) {
        let nodes: Vec<_> = self
            .editor
            .graph()
            .nodes
            .values()
            .filter(|n| {
                (n.kind == "generation" || n.kind == "agent")
                    && (self.editor.selection.nodes.is_empty()
                        || self.editor.selection.nodes.contains(&n.id))
            })
            .cloned()
            .collect();
        for node in nodes {
            if let Err(error) =
                self.runtime
                    .start(node.id.0, node.data, Arc::new(MockRunner::default()))
            {
                self.message = error;
            }
        }
        cx.notify();
    }
    fn finish_gesture(&mut self, cx: &mut Context<Self>) {
        match self.gesture.take() {
            Some(Gesture::Move { .. } | Gesture::Resize { .. }) => {
                if let Err(e) = self.editor.commit_transaction() {
                    self.message = e.to_string();
                }
                self.changed(cx);
            }
            Some(Gesture::Marquee { start, current }) => {
                let rect = model::Rect::from_points(start, current);
                self.editor.selection.nodes = self
                    .editor
                    .graph()
                    .nodes
                    .values()
                    .filter(|n| rect.intersects(n.bounds()))
                    .map(|n| n.id.clone())
                    .collect();
                cx.emit(FlowEvent::SelectionChanged);
                cx.notify();
            }
            _ => {}
        }
    }
    fn move_pointer(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let local = self.local(event.position);
        let world = self.world(event.position);
        self.pointer = world;
        if event.pressed_button.is_none() && self.gesture.is_some() {
            self.finish_gesture(cx);
            return;
        }
        match self.gesture.clone() {
            Some(Gesture::Pan { last }) => {
                self.editor.viewport.pan_by(local - last);
                self.gesture = Some(Gesture::Pan { last: local });
            }
            Some(Gesture::Move { last }) => {
                let world = if self.snap_to_grid {
                    world.snapped(20.)
                } else {
                    world
                };
                let result = self.editor.move_selection(world - last);
                if let Err(e) = result {
                    self.message = e.to_string();
                }
                self.gesture = Some(Gesture::Move { last: world });
            }
            Some(Gesture::Resize { node, start, size }) => {
                let delta = world - start;
                let result = self.editor.resize_node(
                    &node,
                    model::Size::new(
                        (size.width + delta.x).max(160.),
                        (size.height + delta.y).max(100.),
                    ),
                );
                if let Err(e) = result {
                    self.message = e.to_string();
                }
            }
            Some(Gesture::Marquee { start, .. }) => {
                self.gesture = Some(Gesture::Marquee {
                    start,
                    current: world,
                })
            }
            None => {}
        }
        cx.notify();
    }
    fn port_click(&mut self, port: PortRef, cx: &mut Context<Self>) {
        if let Some(first) = self.connecting.take() {
            let (source, target) = if self
                .editor
                .graph()
                .port(&first)
                .is_some_and(|p| p.direction == PortDirection::Output)
            {
                (first, port)
            } else {
                (port, first)
            };
            let id = self.editor.graph().fresh_id("edge");
            let result = self
                .editor
                .transaction("Connect", |g| g.connect(id, source, target));
            if let Err(e) = result {
                self.message = e.to_string();
                cx.emit(FlowEvent::ConnectionRejected(self.message.clone()));
            } else {
                self.message = "Connected".into();
                self.changed(cx);
            }
        } else {
            self.connecting = Some(port);
            self.message = "Click a compatible port · Esc cancels".into();
        }
        cx.notify();
    }
    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let command = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
        match (command, key) {
            (true, "z") => {
                if event.keystroke.modifiers.shift {
                    self.editor.redo();
                } else {
                    self.editor.undo();
                }
                self.changed(cx);
            }
            (true, "y") => {
                self.editor.redo();
                self.changed(cx);
            }
            (true, "a") => {
                self.editor.selection.nodes = self.editor.graph().nodes.keys().cloned().collect();
                cx.notify();
            }
            (true, "c") => self.clipboard = Some(self.editor.copy_selection()),
            (true, "v") => {
                if let Some(clip) = self.clipboard.clone() {
                    match self.editor.paste(&clip, model::Point::new(32., 32.)) {
                        Ok(selection) => self.editor.selection = selection,
                        Err(e) => self.message = e.to_string(),
                    }
                    self.changed(cx);
                }
            }
            (true, "s") => {
                match self.export_json() {
                    Ok(json) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(json));
                        self.message = "Document JSON copied to system clipboard".into();
                    }
                    Err(e) => self.message = e.to_string(),
                }
                cx.notify();
            }
            (_, "backspace" | "delete") => {
                let result = self.editor.delete_selection();
                self.report(result, cx);
            }
            (_, "escape") => {
                self.connecting = None;
                if self.gesture.is_some() {
                    let _ = self.editor.cancel_transaction();
                }
                self.gesture = None;
                cx.notify();
            }
            (false, "f") => {
                self.fit();
                cx.notify();
            }
            (false, "g") => {
                self.show_grid = !self.show_grid;
                cx.notify();
            }
            _ => {}
        }
    }
    fn sync_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.editor.selection.nodes.iter().next().cloned();
        if selected == self.input_node {
            return;
        }
        self.input_node = selected.clone();
        self.input = None;
        self.input_subscription = None;
        if let Some(node) = selected
            .and_then(|id| self.editor.graph().node(&id))
            .cloned()
        {
            let key = if matches!(node.kind.as_str(), "image" | "audio" | "video") {
                "source"
            } else if matches!(node.kind.as_str(), "generation" | "agent") {
                "prompt"
            } else {
                "text"
            };
            let value = node
                .data
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value)
                    .placeholder(key)
            });
            self.input_subscription = Some(cx.subscribe_in(
                &input,
                window,
                move |this, state, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value = state.read(cx).value().to_string();
                        let id = node.id.clone();
                        let result = this.editor.transaction("Edit node payload", |g| {
                            let n = g
                                .nodes
                                .get_mut(&id)
                                .ok_or_else(|| model::GraphError::MissingNode(id.clone()))?;
                            n.data[key] = serde_json::Value::String(value);
                            Ok(())
                        });
                        this.report(result, cx);
                    }
                },
            ));
            self.input = Some(input);
        }
    }
    fn button(
        &self,
        id: &'static str,
        label: impl Into<SharedString>,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        let theme = self.theme;
        div()
            .id(id)
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(theme.card))
            .border_1()
            .border_color(rgb(theme.border))
            .text_sm()
            .cursor_pointer()
            .hover(move |s| s.bg(rgb(theme.border)))
            .child(label.into())
            .on_click(cx.listener(move |this, _, _, cx| action(this, cx)))
    }
    fn node_body(&self, node: &Node, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some(renderer) = self.registry.get(&node.kind) {
            return renderer.render(
                node,
                NodeRenderContext {
                    selected: self.editor.selection.nodes.contains(&node.id),
                    zoom: self.editor.viewport.zoom,
                    theme: self.theme,
                },
                window,
                cx,
            );
        }
        let theme = self.theme;
        let color = kind_color(&node.kind);
        let z = self.editor.viewport.zoom;
        let text = node
            .data
            .get("text")
            .or_else(|| node.data.get("caption"))
            .or_else(|| node.data.get("prompt"))
            .and_then(|v| v.as_str())
            .unwrap_or("Custom node")
            .to_owned();
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .p(px(14. * z))
            .text_size(px(12. * z))
            .text_color(rgb(theme.muted));
        match node.kind.as_str() {
            "image" => {
                if let Some(source) = node
                    .data
                    .get("source")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    body = body.child(
                        img(std::path::PathBuf::from(source))
                            .w_full()
                            .h(px(80. * z))
                            .object_fit(ObjectFit::Contain),
                    );
                } else {
                    body = body.child(
                        div()
                            .h(px(66. * z))
                            .rounded_md()
                            .bg(rgb(0x293345))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(color))
                            .child("IMAGE · empty asset"),
                    );
                }
            }
            "video" => {
                body = body.child(
                    div()
                        .h(px(65. * z))
                        .rounded_md()
                        .bg(rgb(0x2b233b))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(color))
                        .child("▶  VIDEO · metadata"),
                )
            }
            "audio" => {
                let mut wave = div().h(px(45. * z)).flex().items_center().gap(px(3. * z));
                for i in 0..28 {
                    wave = wave.child(
                        div()
                            .w(px(4. * z))
                            .h(px((9. + ((i * 17) % 31) as f32) * z))
                            .rounded_sm()
                            .bg(rgb(color).alpha(0.7)),
                    );
                }
                body = body.child(wave);
            }
            "generation" | "agent" => body = body.child(badge("MOCK / OFFLINE", color)),
            _ => {}
        }
        body = body.child(text);
        if let Some(run) = self.runtime.records.get(node.id.as_str()) {
            body = body.child(progress_bar(run.progress, theme)).child(format!(
                "{:?} · {}% · attempt {}",
                run.status,
                (run.progress * 100.) as u32,
                run.attempt
            ));
        }
        body.into_any_element()
    }
    fn render_node(&self, node: &Node, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let z = self.editor.viewport.zoom;
        let pos = self.editor.viewport.world_to_screen(node.position);
        let selected = self.editor.selection.nodes.contains(&node.id);
        let theme = self.theme;
        let id = node.id.clone();
        let resize_id = id.clone();
        let size = node.size;
        let color = kind_color(&node.kind);
        let mut card = div()
            .id(SharedString::from(format!("node-{}", node.id)))
            .absolute()
            .left(px(pos.x))
            .top(px(pos.y))
            .w(px(node.size.width * z))
            .h(px(node.size.height * z))
            .bg(rgb(theme.card))
            .rounded(px(10. * z))
            .border_1()
            .border_color(rgb(if selected { theme.accent } else { theme.border }))
            .shadow_md()
            .child(
                div()
                    .h(px(36. * z))
                    .px(px(13. * z))
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(theme.border))
                    .cursor_grab()
                    .child(div().size(px(7. * z)).rounded_full().bg(rgb(color)))
                    .child(
                        div()
                            .text_size(px(12. * z))
                            .text_color(rgb(theme.foreground))
                            .child(node.title.clone()),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.focus, cx);
                            if event.modifiers.shift {
                                if !this.editor.selection.nodes.insert(id.clone()) {
                                    this.editor.selection.nodes.remove(&id);
                                }
                            } else if !this.editor.selection.nodes.contains(&id) {
                                this.editor.selection.nodes.clear();
                                this.editor.selection.nodes.insert(id.clone());
                            }
                            if this.editor.graph().node(&id).is_some_and(|n| !n.locked) {
                                let _ = this.editor.begin_transaction("Move nodes");
                                let p = this.world(event.position);
                                this.gesture = Some(Gesture::Move {
                                    last: if this.snap_to_grid { p.snapped(20.) } else { p },
                                });
                            }
                            cx.emit(FlowEvent::SelectionChanged);
                            cx.notify();
                        }),
                    ),
            )
            .child(
                div()
                    .overflow_hidden()
                    .h(px((node.size.height - 39.) * z))
                    .child(self.node_body(node, window, cx)),
            )
            .child(
                div()
                    .absolute()
                    .right_0()
                    .bottom_0()
                    .size(px(14.))
                    .cursor_pointer()
                    .text_color(rgb(theme.muted))
                    .child("◢")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let _ = this.editor.begin_transaction("Resize node");
                            this.gesture = Some(Gesture::Resize {
                                node: resize_id.clone(),
                                start: this.world(event.position),
                                size,
                            });
                            cx.notify();
                        }),
                    ),
            );
        for port in &node.ports {
            let anchor = node.port_position(&port.id).unwrap() - node.position;
            let port_ref = PortRef::new(node.id.clone(), port.id.clone());
            let active = self.connecting.as_ref() == Some(&port_ref);
            card = card.child(
                div()
                    .id(SharedString::from(format!("port-{}-{}", node.id, port.id)))
                    .absolute()
                    .left(px(anchor.x * z - 6.))
                    .top(px(anchor.y * z - 6.))
                    .size(px(12.))
                    .rounded_full()
                    .bg(rgb(if active { theme.accent } else { color }))
                    .border_2()
                    .border_color(rgb(theme.background))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.port_click(port_ref.clone(), cx);
                        }),
                    ),
            );
        }
        card.into_any_element()
    }
    fn minimap(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme;
        let viewport = self.editor.viewport;
        let visible = viewport.visible_world_rect(self.extent);
        let bounds = self
            .editor
            .graph()
            .bounds()
            .map(|b| b.union(visible))
            .unwrap_or(visible);
        let map = model::MinimapTransform::new(
            bounds,
            model::Rect::new(model::Point::ZERO, model::Size::new(190., 122.)),
            8.,
        )
        .unwrap();
        let mut view = div()
            .id("minimap")
            .absolute()
            .right(px(18.))
            .bottom(px(50.))
            .w(px(190.))
            .h(px(122.))
            .bg(rgb(theme.panel).alpha(0.95))
            .border_1()
            .border_color(rgb(theme.border))
            .rounded_lg()
            .overflow_hidden();
        for node in self.editor.graph().nodes.values() {
            let r = map.world_rect_to_map(node.bounds());
            view = view.child(
                div()
                    .absolute()
                    .left(px(r.origin.x))
                    .top(px(r.origin.y))
                    .w(px(r.size.width))
                    .h(px(r.size.height))
                    .rounded_sm()
                    .bg(rgb(kind_color(&node.kind)).alpha(0.65)),
            );
        }
        let r = map.world_rect_to_map(visible);
        view = view.child(
            div()
                .absolute()
                .left(px(r.origin.x))
                .top(px(r.origin.y))
                .w(px(r.size.width))
                .h(px(r.size.height))
                .border_1()
                .border_color(rgb(theme.accent)),
        );
        // Mini-map is an overview; Fit control handles navigation deterministically.
        view.child(
            div()
                .absolute()
                .bottom_1()
                .left_2()
                .text_xs()
                .text_color(rgb(theme.muted))
                .child("OVERVIEW"),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_, _, _, cx| cx.stop_propagation()),
        )
        .into_any_element()
    }
}
impl Focusable for FlowCanvas {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Render for FlowCanvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_input(window, cx);
        let theme = self.theme;
        let viewport = self.editor.viewport;
        let grid = self.show_grid;
        let edge_renderer = self.edge_renderer.clone();
        let entity = cx.entity().downgrade();
        let edges: Vec<_> = self
            .editor
            .graph()
            .edges
            .values()
            .filter_map(|e| {
                Some((
                    self.editor.graph().port_position(&e.source)?,
                    self.editor.graph().port_position(&e.target)?,
                ))
            })
            .collect();
        let pending = self
            .connecting
            .as_ref()
            .and_then(|p| self.editor.graph().port_position(p))
            .map(|from| (from, self.pointer));
        let marquee = match self.gesture {
            Some(Gesture::Marquee { start, current }) => {
                Some(model::Rect::from_points(start, current))
            }
            _ => None,
        };
        let paint = canvas(
            move |bounds, _, cx| {
                let _ = entity.update(cx, |this, _| {
                    this.origin = model::Point::new(bounds.origin.x.into(), bounds.origin.y.into());
                    this.extent =
                        model::Size::new(bounds.size.width.into(), bounds.size.height.into());
                });
            },
            move |bounds, _, window, _| {
                let screen = |p: model::Point| {
                    let p = viewport.world_to_screen(p);
                    bounds.origin + point(px(p.x), px(p.y))
                };
                if grid {
                    let gap = (24. * viewport.zoom).max(12.);
                    let width: f32 = bounds.size.width.into();
                    let height: f32 = bounds.size.height.into();
                    let mut x = viewport.pan.x.rem_euclid(gap);
                    while x < width {
                        let mut y = viewport.pan.y.rem_euclid(gap);
                        while y < height {
                            window.paint_quad(fill(
                                Bounds::new(
                                    bounds.origin + point(px(x), px(y)),
                                    size(px(1.5), px(1.5)),
                                ),
                                rgb(theme.grid),
                            ));
                            y += gap;
                        }
                        x += gap;
                    }
                }
                for (from, to) in edges {
                    edge_renderer.paint(
                        screen(from),
                        screen(to),
                        rgb(theme.accent).alpha(0.55),
                        window,
                    );
                }
                if let Some((from, to)) = pending {
                    edge_renderer.paint(screen(from), screen(to), rgb(theme.accent), window);
                }
                if let Some(rect) = marquee {
                    let r = viewport.world_rect_to_screen(rect);
                    window.paint_quad(fill(
                        Bounds::new(
                            bounds.origin + point(px(r.origin.x), px(r.origin.y)),
                            size(px(r.size.width), px(r.size.height)),
                        ),
                        rgb(theme.accent).alpha(0.16),
                    ));
                }
            },
        )
        .absolute()
        .size_full();
        let mut surface = div()
            .id("flow-surface")
            .relative()
            .flex_1()
            .size_full()
            .overflow_hidden()
            .bg(rgb(theme.background))
            .child(paint)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    if event.modifiers.shift {
                        this.gesture = Some(Gesture::Marquee {
                            start: this.world(event.position),
                            current: this.world(event.position),
                        });
                    } else {
                        this.editor.selection.nodes.clear();
                        this.gesture = Some(Gesture::Pan {
                            last: this.local(event.position),
                        });
                    }
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.gesture = Some(Gesture::Pan {
                        last: this.local(event.position),
                    });
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event, _, cx| this.move_pointer(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_gesture(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_gesture(cx)),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|this, _, _, cx| this.finish_gesture(cx)),
            )
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let delta = match event.delta {
                    ScrollDelta::Pixels(p) => model::Point::new(p.x.into(), p.y.into()),
                    ScrollDelta::Lines(p) => model::Point::new(p.x * 24., p.y * 24.),
                };
                if event.modifiers.control || event.modifiers.platform {
                    let zoom = this.editor.viewport.zoom * (-delta.y * 0.004).exp();
                    this.editor
                        .viewport
                        .zoom_at(this.local(event.position), zoom);
                } else {
                    this.editor.viewport.pan_by(delta);
                }
                cx.notify();
            }));
        for group in self.editor.graph().groups.values() {
            let r = viewport.world_rect_to_screen(group.bounds);
            surface = surface.child(
                div()
                    .absolute()
                    .left(px(r.origin.x))
                    .top(px(r.origin.y))
                    .w(px(r.size.width))
                    .h(px(r.size.height))
                    .border_1()
                    .border_color(rgb(theme.accent).alpha(0.35))
                    .rounded_lg()
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .text_xs()
                            .text_color(rgb(theme.accent))
                            .child(group.title.clone()),
                    ),
            );
        }
        let visible = viewport.visible_world_rect(self.extent).expanded(100.);
        for node in self.editor.graph().nodes.values() {
            if visible.intersects(node.bounds()) {
                surface = surface.child(self.render_node(node, window, cx));
            }
        }
        surface = surface.child(self.minimap(cx)).child(
            div()
                .absolute()
                .left(px(18.))
                .bottom(px(18.))
                .flex()
                .gap_2()
                .child(self.button("zoom-out", "−", cx, |this, cx| {
                    this.editor.viewport.zoom_at(
                        model::Point::new(this.extent.width / 2., this.extent.height / 2.),
                        this.editor.viewport.zoom / 1.2,
                    );
                    cx.notify();
                }))
                .child(self.button(
                    "zoom-label",
                    format!("{}%", (viewport.zoom * 100.) as u32),
                    cx,
                    |this, cx| {
                        this.editor.viewport.zoom_at(
                            model::Point::new(this.extent.width / 2., this.extent.height / 2.),
                            1.,
                        );
                        cx.notify();
                    },
                ))
                .child(self.button("zoom-in", "+", cx, |this, cx| {
                    this.editor.viewport.zoom_at(
                        model::Point::new(this.extent.width / 2., this.extent.height / 2.),
                        this.editor.viewport.zoom * 1.2,
                    );
                    cx.notify();
                }))
                .child(self.button("fit", "Fit · F", cx, |this, cx| {
                    this.fit();
                    cx.notify();
                })),
        );
        let mut palette = div()
            .w(px(170.))
            .flex_shrink_0()
            .bg(rgb(theme.panel))
            .border_r_1()
            .border_color(rgb(theme.border))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(panel_title("COMPONENTS / 组件", theme));
        for kind in BUILTIN_KINDS {
            let kind = *kind;
            palette = palette.child(
                div()
                    .id(SharedString::from(format!("add-{kind}")))
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(theme.border))
                    .text_sm()
                    .cursor_pointer()
                    .text_color(rgb(kind_color(kind)))
                    .child(format!("+  {kind}"))
                    .on_click(cx.listener(move |this, _, _, cx| this.add(kind, cx))),
            );
        }
        palette=palette.child(div().mt_4().text_xs().text_color(rgb(theme.muted)).child("Drag background: pan\nCtrl + wheel: zoom\nShift + drag: select\nClick ports: connect\nCtrl + Z: undo\nCtrl + C/V: duplicate\nCtrl + S: copy JSON"));
        let mut inspector = div()
            .w(px(235.))
            .flex_shrink_0()
            .bg(rgb(theme.panel))
            .border_l_1()
            .border_color(rgb(theme.border))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(panel_title("INSPECTOR / 属性", theme));
        if let Some(node) = self
            .editor
            .selection
            .nodes
            .iter()
            .next()
            .and_then(|id| self.editor.graph().node(id))
        {
            inspector = inspector
                .child(node.title.clone())
                .child(badge(node.kind.clone(), kind_color(&node.kind)))
                .child(div().text_xs().text_color(rgb(theme.muted)).child(format!(
                    "ID: {}\nPosition: {:.0}, {:.0}\nSize: {:.0} × {:.0}\nPorts: {}",
                    node.id,
                    node.position.x,
                    node.position.y,
                    node.size.width,
                    node.size.height,
                    node.ports.len()
                )));
            for port in &node.ports {
                inspector = inspector.child(div().text_xs().text_color(rgb(theme.muted)).child(
                    format!("{:?} {} · {:?}", port.direction, port.label, port.data_type),
                ));
            }
        } else {
            inspector = inspector.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme.muted))
                    .child("Select a node to inspect its type, geometry and ports"),
            );
        }
        if let Some(input) = &self.input {
            inspector = inspector
                .child(panel_title("EDIT CONTENT / 内容", theme))
                .child(Input::new(input));
        }
        inspector = inspector
            .child(self.button("group", "Group selection", cx, |this, cx| {
                match this.editor.group_selection("Canvas group", 22.) {
                    Ok(_) => this.changed(cx),
                    Err(e) => {
                        this.message = e.to_string();
                        cx.notify();
                    }
                }
            }))
            .child(self.button("delete", "Delete selection", cx, |this, cx| {
                let result = this.editor.delete_selection();
                this.report(result, cx);
            }))
            .child(self.button("cancel", "Cancel runs", cx, |this, cx| {
                let ids: Vec<_> = this.runtime.records.keys().cloned().collect();
                for id in ids {
                    this.runtime.cancel(&id);
                }
                cx.notify();
            }))
            .child(self.button("retry", "Retry selected", cx, |this, cx| {
                for id in &this.editor.selection.nodes {
                    if let Err(e) = this.runtime.retry(id.as_str()) {
                        this.message = e;
                    }
                }
                cx.notify();
            }));
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.foreground))
            .font_family("sans-serif")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event, window, cx| {
                if this.focus.is_focused(window) {
                    this.key(event, cx);
                }
            }))
            .child(
                div()
                    .h(px(64.))
                    .flex_shrink_0()
                    .px_5()
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(rgb(theme.panel))
                    .border_b_1()
                    .border_color(rgb(theme.border))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().text_lg().child("◈  gpui-flow"))
                            .child(badge("STUDIO  /  0.1", theme.accent)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.button("undo", "Undo", cx, |this, cx| {
                                this.editor.undo();
                                this.changed(cx);
                            }))
                            .child(self.button("redo", "Redo", cx, |this, cx| {
                                this.editor.redo();
                                this.changed(cx);
                            }))
                            .child(self.button("save", "Save snapshot", cx, |this, cx| {
                                match this.export_json() {
                                    Ok(json) => {
                                        this.snapshot = Some(json);
                                        this.message =
                                            "In-memory snapshot saved; Ctrl+S exports JSON".into();
                                    }
                                    Err(e) => this.message = e.to_string(),
                                }
                                cx.notify();
                            }))
                            .child(self.button("load", "Load snapshot", cx, |this, cx| {
                                if let Some(json) = this.snapshot.clone() {
                                    let result = this.import_json(&json, cx);
                                    this.report(result, cx);
                                } else {
                                    this.message = "Save a snapshot first".into();
                                    cx.notify();
                                }
                            }))
                            .child(self.button("mock-run", "Run · MOCK", cx, Self::run_selected)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(palette)
                    .child(surface)
                    .child(inspector),
            )
            .child(
                div()
                    .h(px(28.))
                    .flex_shrink_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(rgb(theme.panel))
                    .text_xs()
                    .text_color(rgb(theme.muted))
                    .child(self.message.clone())
                    .child(format!(
                        "{} nodes  ·  {} edges  ·  {} selected",
                        self.editor.graph().nodes.len(),
                        self.editor.graph().edges.len(),
                        self.editor.selection.nodes.len()
                    )),
            )
    }
}

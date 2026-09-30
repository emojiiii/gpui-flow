use crate::*;
use gpui::{prelude::*, *};
use gpui_flow_core::{
    self as model, Clipboard, Editor, Graph, Node, NodeId, PortDirection, PortRef,
};
use gpui_flow_runtime::{AgentRunner, MockRunner, Runtime};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

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
    pub options: FlowOptions,
    pub runners: BTreeMap<String, Arc<dyn AgentRunner>>,
    pub run_label: SharedString,
    pub snap_to_grid: bool,
    pub show_grid: bool,
    /// Optional host-owned path used by Save/Open instead of native dialogs.
    pub document_path: Option<std::path::PathBuf>,
    file_busy: bool,
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

mod inspector;
mod interaction;
mod minimap;
mod node;
mod persistence;
mod render;

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
            options: FlowOptions::default(),
            runners: BTreeMap::new(),
            run_label: "Run selected".into(),
            snap_to_grid: false,
            show_grid: true,
            document_path: None,
            file_busy: false,
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
        let mut canvas = Self::new(demo_graph(), cx).expect("valid demo graph");
        canvas
            .runners
            .insert("generation".into(), Arc::new(MockRunner::default()));
        canvas
            .runners
            .insert("agent".into(), Arc::new(MockRunner::default()));
        canvas.run_label = "Run · MOCK".into();
        canvas
    }
    pub fn register_runner(&mut self, kind: impl Into<String>, runner: impl AgentRunner) {
        self.runners.insert(kind.into(), Arc::new(runner));
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        let removed: Vec<_> = self
            .runtime
            .records
            .keys()
            .filter(|id| {
                !self
                    .editor
                    .graph()
                    .nodes
                    .contains_key(&NodeId::from((*id).clone()))
            })
            .cloned()
            .collect();
        for id in removed {
            self.runtime.forget(&id);
        }
        if self
            .connecting
            .as_ref()
            .is_some_and(|p| self.editor.graph().port(p).is_none())
        {
            self.connecting = None;
        }

        cx.emit(FlowEvent::DocumentChanged);
        cx.notify();
    }
    fn report(&mut self, result: Result<(), model::GraphError>, cx: &mut Context<Self>) {
        if let Err(error) = result {
            self.message = error.to_string();
        }
        self.changed(cx);
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
                self.runners.contains_key(&n.kind)
                    && (self.editor.selection.nodes.is_empty()
                        || self.editor.selection.nodes.contains(&n.id))
            })
            .cloned()
            .collect();
        for node in nodes {
            if let Err(error) =
                self.runtime
                    .start(node.id.0, node.data, self.runners[&node.kind].clone())
            {
                self.message = error;
            }
        }
        cx.notify();
    }
    fn button(
        &self,
        id: &'static str,
        label: impl Into<SharedString>,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        ControlButton::new(id, label)
            .theme(self.theme)
            .on_click(cx.listener(move |this, _, _, cx| action(this, cx)))
    }
}
impl Focusable for FlowCanvas {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

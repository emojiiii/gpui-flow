use crate::*;
use gpui::{prelude::*, *};
use gpui_flow_core::{self as model, Clipboard, Editor, Graph, Node, NodeId, PortRef};
use gpui_flow_runtime::{AgentRunner, MockRunner, Runtime};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

#[derive(Clone, Debug)]
pub enum FlowEvent {
    DocumentChanged,
    SelectionChanged,
    ViewportChanged,
    SaveRequested,
    ConnectionRejected(String),
    RunChanged(String),
}
impl EventEmitter<FlowEvent> for FlowCanvas {}
/// Optional host document contract, checked before persistence replaces state.
pub type FlowDocumentValidator =
    Arc<dyn Fn(&model::FlowDocument) -> Result<(), String> + Send + Sync>;

/// Host-owned reusable view. Subscribe to `FlowEvent` for persistence/inspector integration.
/// Canvas coordinates are independent of window origin and account for pan and zoom.
pub struct FlowCanvas {
    pub editor: Editor,
    pub interactions: gpui_flow_interaction::PluginHost,
    pub registry: NodeRegistry,
    pub theme: FlowTheme,
    pub edge_renderer: Arc<dyn EdgeRenderer>,
    pub runtime: Runtime,
    pub workflow: Option<gpui_flow_runtime::Workflow>,
    pub workflow_runners: gpui_flow_runtime::RunnerRegistry,
    media_backend: Option<Arc<dyn crate::media_backend::MediaBackend>>,
    media_players: BTreeMap<NodeId, (String, Result<Entity<MediaView>, String>)>,
    pub options: FlowOptions,
    pub runners: BTreeMap<String, Arc<dyn AgentRunner>>,
    pub run_label: SharedString,
    pub show_grid: bool,
    /// Optional host-owned path used by Save/Open instead of native dialogs.
    pub document_path: Option<std::path::PathBuf>,
    file_busy: bool,
    host_persistence: bool,
    selection_on_top: bool,
    port_labels: bool,
    pan_mode: bool,
    document_validator: Option<FlowDocumentValidator>,
    context_menu_handler: Option<CanvasContextMenuHandler>,
    focus: FocusHandle,
    space_held: bool,
    native_pointer: crate::native_pointer::NativePointerState,
    origin: model::Point,
    extent: model::Size,
    interaction_subscriptions: Vec<Subscription>,
    interaction_window: Option<u64>,
    clipboard: Option<Clipboard>,
    message: String,
    snapshot: Option<String>,
    sequence: u64,
    input: Option<Entity<InputState>>,
    input_node: Option<NodeId>,
    input_subscription: Option<Subscription>,
}

mod context_menu;
pub use context_menu::{CanvasContextMenuHandler, CanvasContextMenuRequest};
mod inspector;
mod interaction;
mod media;
mod minimap;
mod node;
mod persistence;
mod plugins;
mod render;
mod workflow;

impl FlowCanvas {
    /// Install a domain contract without changing the existing document. Runtime-only.
    pub fn set_document_validator(
        &mut self,
        validator: Option<FlowDocumentValidator>,
    ) -> Result<(), String> {
        persistence::validate_host_document(&self.editor.document(), validator.as_ref())?;
        self.document_validator = validator;
        Ok(())
    }
    /// Delegate save requests to the embedding project's persistence owner.
    /// Default false preserves the standalone FlowDocument file dialogs.
    pub fn set_host_persistence(&mut self, value: bool) {
        self.host_persistence = value;
    }
    /// Optional object-editor presentation; default graph z-order stays unchanged.
    pub fn set_selection_on_top(&mut self, value: bool) {
        self.selection_on_top = value;
    }
    /// Compact object editors can keep typed handles without inline labels.
    pub fn set_port_labels(&mut self, value: bool) {
        self.port_labels = value;
    }
    pub fn pan_mode(&self) -> bool {
        self.pan_mode
    }
    pub fn set_pan_mode(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.pan_mode != value {
            self.cancel_interaction(cx);
            self.pan_mode = value;
            cx.notify();
        }
    }
    /// Latest interaction/persistence message for host-owned editor shells.
    pub fn status_message(&self) -> &str {
        &self.message
    }

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
                        if let Some(workflow) = this.workflow.as_mut()
                            && !workflow.is_finished()
                        {
                            let events = workflow.poll();
                            for event in events {
                                cx.emit(FlowEvent::RunChanged(event.node_id));
                            }
                            if workflow.is_finished() {
                                this.message = if workflow.succeeded() {
                                    "Workflow completed"
                                } else {
                                    "Workflow ended with failed, skipped or cancelled nodes"
                                }
                                .into();
                            }
                            cx.notify();
                        }
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
            interactions: gpui_flow_interaction::PluginHost::with_defaults(),
            registry: NodeRegistry::default(),
            theme: FlowTheme::default(),
            edge_renderer: Arc::new(BezierEdge),
            runtime: Runtime::default(),
            workflow: None,
            workflow_runners: BTreeMap::new(),
            media_backend: None,
            media_players: BTreeMap::new(),
            options: FlowOptions::default(),
            runners: BTreeMap::new(),
            run_label: "Run selected".into(),
            show_grid: true,
            document_path: None,
            file_busy: false,
            host_persistence: false,
            selection_on_top: false,
            port_labels: true,
            pan_mode: false,
            document_validator: None,
            context_menu_handler: None,
            focus: cx.focus_handle(),
            space_held: false,
            native_pointer: Default::default(),
            origin: model::Point::ZERO,
            extent: model::Size::new(1000., 700.),
            interaction_subscriptions: vec![],
            interaction_window: None,
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
        canvas.set_media_backend(Some(Arc::new(
            crate::media_backend::FfmpegBackend::default(),
        )));
        canvas.workflow_runners = workflow::demo_workflow_runners();
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

        cx.emit(FlowEvent::DocumentChanged);
        cx.notify();
    }
    fn fit(&mut self, cx: &mut Context<Self>) {
        let _ = self.execute_command(
            gpui_flow_interaction::CanvasCommand::Fit {
                canvas: self.extent,
                padding: 55.,
            },
            cx,
        );
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
        let _ = self.execute_command(gpui_flow_interaction::CanvasCommand::AddNode(node), cx);
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
            .disabled(
                !self.is_editable()
                    && matches!(
                        id,
                        "undo" | "redo" | "delete" | "group" | "load" | "file-open"
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| action(this, cx)))
    }
}
impl Focusable for FlowCanvas {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

use super::*;
use gpui_flow_core::{
    AgentEditError, AgentEditReceipt, AgentEditRequest, AgentSnapshot, FlowDocument, Selection,
};
use gpui_flow_interaction::{CanvasCommand, DispatchReport, InteractionError, SelectionMode};
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) fn next_canvas_id() -> u64 {
    static NEXT_CANVAS_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_CANVAS_ID
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .expect("flow canvas identity exhausted")
}

/// A canvas-local node key. Use it with the `FlowCanvas` that issued it.
/// The key resolves its ID only while that ID exists in the canvas.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FlowNodeHandle {
    canvas_id: u64,
    id: NodeId,
}

impl FlowNodeHandle {
    pub fn id(&self) -> &NodeId {
        &self.id
    }
}

impl FlowCanvas {
    /// Mount a workflow document in an entity that can be embedded with `.child(canvas)`.
    pub fn mount(
        document: FlowDocument,
        cx: &mut impl AppContext,
    ) -> Result<Entity<Self>, model::GraphError> {
        let editor = Editor::from_document(document)?;
        Ok(cx.new(|cx| {
            let mut canvas = Self::from_editor(editor, cx);
            canvas.options = FlowOptions::canvas_only();
            canvas.message = "Ready".into();
            canvas
        }))
    }

    pub fn set_options(&mut self, options: FlowOptions, cx: &mut Context<Self>) {
        self.options = options;
        cx.notify();
    }

    /// Read the canvas graph and its current revision for an agent edit request.
    pub fn agent_snapshot(&self) -> AgentSnapshot<'_> {
        self.project_agent_snapshot()
    }

    /// Apply a graph edit batch under the canvas policy, installed middleware,
    /// and final document validator.
    pub fn apply_agent_edit(
        &mut self,
        mut request: AgentEditRequest,
        cx: &mut Context<Self>,
    ) -> Result<AgentEditReceipt, AgentEditError> {
        if !self.is_editable() {
            return Err(AgentEditError::policy_denied("canvas editing is disabled"));
        }
        if self.interactions.has_capture() || self.interactions.connection().active() {
            return Err(AgentEditError::busy());
        }
        self.resolve_agent_assets(&mut request)?;

        let revision = self.editor.document_revision();
        let selection = self.editor.selection.clone();
        let viewport = self.editor.viewport;
        let validator = self.document_validator.as_ref();
        let assets = &self.agent_asset_paths;
        let receipt =
            self.interactions
                .apply_agent_edit(&mut self.editor, request, |before, graph| {
                    Self::validate_agent_assets(before, graph, assets)?;
                    if let Some(validate) = validator {
                        validate(&FlowDocument {
                            graph: graph.clone(),
                            viewport,
                        })?;
                    }
                    Ok(())
                })?;

        let document_changed = self.editor.document_revision() != revision;
        let selection_changed = self.editor.selection != selection;
        if document_changed || selection_changed {
            self.apply_interaction_report(
                DispatchReport {
                    document_changed,
                    selection_changed,
                    ..DispatchReport::default()
                },
                cx,
            );
        }
        Ok(receipt)
    }

    /// Register the GPUI body renderer for a kind of node in this canvas.
    pub fn register_node_renderer(
        &mut self,
        kind: impl Into<String>,
        renderer: impl NodeRenderer + 'static,
        cx: &mut Context<Self>,
    ) {
        self.registry.register(kind, renderer);
        cx.notify();
    }

    /// Add a detached node to this canvas, using its edit policy and history.
    pub fn add(
        &mut self,
        node: Node,
        cx: &mut Context<Self>,
    ) -> Result<FlowNodeHandle, InteractionError> {
        let id = self.execute_checked_command(
            CanvasCommand::AddNode(node),
            |command| match command {
                CanvasCommand::AddNode(node) => Ok(node.id.clone()),
                _ => Err(InteractionError::InvalidInput(
                    "a plugin replaced the canvas add operation".into(),
                )),
            },
            cx,
        )?;
        Ok(self.node_key(&id))
    }

    /// Find the node with this ID in this canvas.
    pub fn find_id(&self, id: &NodeId) -> Option<FlowNodeHandle> {
        self.editor.graph().node(id).map(|_| self.node_key(id))
    }

    /// Iterate this canvas's node children in their graph order.
    pub fn children(&self) -> impl ExactSizeIterator<Item = FlowNodeHandle> + '_ {
        self.editor.graph().nodes.keys().map(|id| self.node_key(id))
    }

    pub fn node(&self, handle: &FlowNodeHandle) -> Result<&Node, InteractionError> {
        if handle.canvas_id != self.canvas_id {
            return Err(InteractionError::InvalidInput(
                "the node belongs to a different flow canvas".into(),
            ));
        }
        self.editor
            .graph()
            .node(&handle.id)
            .ok_or_else(|| model::GraphError::MissingNode(handle.id.clone()).into())
    }

    /// Remove this node and its incident edges as one edit.
    pub fn remove(
        &mut self,
        handle: &FlowNodeHandle,
        cx: &mut Context<Self>,
    ) -> Result<(), InteractionError> {
        let node = self.node(handle)?.id.clone();
        self.execute_node_command(CanvasCommand::RemoveNode { node }, cx)
    }

    /// Select this node, replacing the current selection.
    pub fn select(
        &mut self,
        handle: &FlowNodeHandle,
        cx: &mut Context<Self>,
    ) -> Result<(), InteractionError> {
        let node = self.node(handle)?.id.clone();
        self.execute_node_command(
            CanvasCommand::SelectNode {
                node,
                mode: SelectionMode::Replace,
            },
            cx,
        )
    }

    pub fn selection(&self) -> &Selection {
        &self.editor.selection
    }

    pub fn can_undo(&self) -> bool {
        self.editor.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.editor.can_redo()
    }

    fn node_key(&self, id: &NodeId) -> FlowNodeHandle {
        FlowNodeHandle {
            canvas_id: self.canvas_id,
            id: id.clone(),
        }
    }

    fn execute_node_command(
        &mut self,
        command: CanvasCommand,
        cx: &mut Context<Self>,
    ) -> Result<(), InteractionError> {
        self.execute_checked_command(
            command.clone(),
            |filtered| {
                if filtered == &command {
                    Ok(())
                } else {
                    Err(InteractionError::InvalidInput(
                        "a plugin changed the scoped node operation".into(),
                    ))
                }
            },
            cx,
        )
    }
}

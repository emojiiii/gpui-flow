//! Versioned workflow edits for machine clients, independent of UI and transport.
use crate::{
    EdgeId, Editor, Graph, GraphError, GroupId, Node, NodeId, Point, Port, PortRef, Selection,
    Size, ValidationIssue,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{borrow::Cow, collections::BTreeMap, fmt};

mod wire;

pub const AGENT_EDIT_SCHEMA_VERSION: u32 = 1;
pub const AGENT_EDIT_SCHEMA: &str = include_str!("../schema/agent-edit-v1.schema.json");

/// A view of the workflow and its transient selection.
#[derive(Debug, Serialize)]
pub struct AgentSnapshot<'a> {
    pub schema_version: u32,
    /// Pass this opaque token back as the expected revision of an edit.
    pub revision: String,
    pub graph: Cow<'a, Graph>,
    pub selection: &'a Selection,
}

/// Creation defaults that a host can publish alongside its workflow snapshot.
/// The caller supplies the node ID and position when creating a node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentNodeTemplate {
    pub kind: String,
    pub title: String,
    pub size: Size,
    pub ports: Vec<Port>,
    pub data: Value,
}

impl From<Node> for AgentNodeTemplate {
    fn from(node: Node) -> Self {
        Self {
            kind: node.kind,
            title: node.title,
            size: node.size,
            ports: node.ports,
            data: node.data,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "wire::Object<wire::Request>")]
pub struct AgentEditRequest {
    pub schema_version: u32,
    pub expected_revision: String,
    pub operations: Vec<AgentOperation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentOperation {
    AddNode {
        id: NodeId,
        title: String,
        #[serde(deserialize_with = "wire::object::<_, wire::Position, _>")]
        position: Point,
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(
            default,
            deserialize_with = "wire::optional_object::<_, wire::Dimensions, _>",
            skip_serializing_if = "Option::is_none"
        )]
        size: Option<Size>,
        #[serde(
            default,
            deserialize_with = "wire::ports",
            skip_serializing_if = "Vec::is_empty"
        )]
        ports: Vec<Port>,
        #[serde(default, skip_serializing_if = "Value::is_null")]
        data: Value,
    },
    UpdateNode {
        id: NodeId,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(
            default,
            deserialize_with = "wire::optional_object::<_, wire::Position, _>",
            skip_serializing_if = "Option::is_none"
        )]
        position: Option<Point>,
        #[serde(
            default,
            deserialize_with = "wire::optional_object::<_, wire::Dimensions, _>",
            skip_serializing_if = "Option::is_none"
        )]
        size: Option<Size>,
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(
            default,
            deserialize_with = "wire::optional_ports",
            skip_serializing_if = "Option::is_none"
        )]
        ports: Option<Vec<Port>>,
    },
    SetNodeData {
        id: NodeId,
        data: Value,
    },
    RemoveNode {
        id: NodeId,
    },
    Connect {
        id: EdgeId,
        #[serde(deserialize_with = "wire::object::<_, wire::Endpoint, _>")]
        source: PortRef,
        #[serde(deserialize_with = "wire::object::<_, wire::Endpoint, _>")]
        target: PortRef,
    },
    Disconnect {
        id: EdgeId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEditReceipt {
    pub revision: String,
    pub changes: AgentChanges,
}

/// Net changes including edges and memberships affected by deleting a node.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentChanges {
    pub nodes: ChangedIds<NodeId>,
    pub edges: ChangedIds<EdgeId>,
    pub groups: ChangedIds<GroupId>,
}

/// Each list is sorted by ID.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedIds<Id> {
    pub inserted: Vec<Id>,
    pub updated: Vec<Id>,
    pub removed: Vec<Id>,
}

impl<Id: Ord + Clone> ChangedIds<Id> {
    fn between<Item: PartialEq>(before: &BTreeMap<Id, Item>, after: &BTreeMap<Id, Item>) -> Self {
        let mut updated = Vec::new();
        let mut removed = Vec::new();
        for (id, old) in before {
            match after.get(id) {
                Some(new) if old != new => updated.push(id.clone()),
                None => removed.push(id.clone()),
                Some(_) => {}
            }
        }
        let inserted = after
            .keys()
            .filter(|id| !before.contains_key(*id))
            .cloned()
            .collect();
        Self {
            inserted,
            updated,
            removed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEditErrorCode {
    UnsupportedSchema,
    StaleRevision,
    Busy,
    PolicyDenied,
    Locked,
    InvalidConnection,
    Validation,
    NotFound,
    DuplicateId,
    DocumentTooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEditError {
    pub code: AgentEditErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<ValidationIssue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_revision: Option<String>,
}

impl AgentEditError {
    fn new(code: AgentEditErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            operation_index: None,
            issues: Vec::new(),
            current_revision: None,
        }
    }

    pub fn busy() -> Self {
        Self::new(
            AgentEditErrorCode::Busy,
            "a canvas interaction is in progress",
        )
    }

    pub fn policy_denied(message: impl Into<String>) -> Self {
        Self::new(AgentEditErrorCode::PolicyDenied, message)
    }
}

impl fmt::Display for AgentEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(index) = self.operation_index {
            write!(f, "operations[{index}]: ")?;
        }
        f.write_str(&self.message)
    }
}

impl std::error::Error for AgentEditError {}

impl From<GraphError> for AgentEditError {
    fn from(error: GraphError) -> Self {
        use AgentEditErrorCode as Code;
        let code = match &error {
            GraphError::DuplicateId { .. } => Code::DuplicateId,
            GraphError::MissingNode(_)
            | GraphError::MissingEdge(_)
            | GraphError::MissingGroup(_)
            | GraphError::MissingPort(_) => Code::NotFound,
            GraphError::InvalidConnection(_) => Code::InvalidConnection,
            GraphError::DocumentTooLarge { .. } => Code::DocumentTooLarge,
            GraphError::Locked(_) => Code::Locked,
            GraphError::TransactionInProgress => Code::Busy,
            GraphError::InvalidGeometry(_)
            | GraphError::InvalidClipboard(_)
            | GraphError::Validation(_)
            | GraphError::Serialization(_)
            | GraphError::NoActiveTransaction => Code::Validation,
        };
        let mut response = Self::new(code, error.to_string());
        if let GraphError::Validation(issues) = error {
            response.issues = issues;
        }
        response
    }
}

impl Editor {
    pub fn agent_snapshot(&self) -> AgentSnapshot<'_> {
        AgentSnapshot {
            schema_version: AGENT_EDIT_SCHEMA_VERSION,
            revision: self.document_revision().to_string(),
            graph: Cow::Borrowed(self.graph()),
            selection: &self.selection,
        }
    }

    /// Apply ordered operations atomically and record one undo entry on change.
    pub fn apply_agent_edit(
        &mut self,
        request: AgentEditRequest,
    ) -> Result<AgentEditReceipt, AgentEditError> {
        self.apply_agent_edit_checked(request, |_| Ok(()))
    }

    /// Ask the host to validate the final graph before committing it.
    pub fn apply_agent_edit_checked(
        &mut self,
        request: AgentEditRequest,
        validate: impl FnOnce(&Graph) -> Result<(), String>,
    ) -> Result<AgentEditReceipt, AgentEditError> {
        self.apply_agent_edit_guarded(request, |_, _| Ok(()), |_, candidate| validate(candidate))
    }

    /// Authorize each operation against the staged graph before applying it,
    /// then validate the whole transition before committing once.
    pub fn apply_agent_edit_guarded(
        &mut self,
        request: AgentEditRequest,
        mut authorize: impl FnMut(&Editor, &AgentOperation) -> Result<(), String>,
        validate: impl FnOnce(&Graph, &Graph) -> Result<(), String>,
    ) -> Result<AgentEditReceipt, AgentEditError> {
        if request.schema_version != AGENT_EDIT_SCHEMA_VERSION {
            return Err(AgentEditError::new(
                AgentEditErrorCode::UnsupportedSchema,
                format!("supported agent edit schema version is {AGENT_EDIT_SCHEMA_VERSION}"),
            ));
        }
        let revision = self.document_revision().to_string();
        if request.expected_revision != revision {
            let mut error = AgentEditError::new(
                AgentEditErrorCode::StaleRevision,
                "read the current workflow snapshot before editing",
            );
            error.current_revision = Some(revision);
            return Err(error);
        }
        if self.transaction_active() {
            return Err(AgentEditError::busy());
        }
        if request.operations.is_empty() {
            return Ok(AgentEditReceipt {
                revision,
                changes: AgentChanges::default(),
            });
        }
        let changes = self.checked_staged_transaction(
            "Agent edit",
            |staged| {
                for (index, operation) in request.operations.into_iter().enumerate() {
                    let result = match &operation {
                        AgentOperation::AddNode { id, .. }
                            if staged.before().nodes.contains_key(id) =>
                        {
                            Err(AgentEditError::from(GraphError::DuplicateId {
                                kind: "node",
                                id: id.to_string(),
                            }))
                        }
                        _ => authorize(staged.editor(), &operation)
                            .map_err(AgentEditError::policy_denied)
                            .and_then(|()| operation.apply(staged.graph_mut()).map_err(Into::into)),
                    };
                    result.map_err(|mut error| {
                        error.operation_index = Some(index);
                        error
                    })?;
                }
                let before = staged.before();
                let candidate = staged.editor().graph();
                Ok(AgentChanges {
                    nodes: ChangedIds::between(&before.nodes, &candidate.nodes),
                    edges: ChangedIds::between(&before.edges, &candidate.edges),
                    groups: ChangedIds::between(&before.groups, &candidate.groups),
                })
            },
            |before, candidate| validate(before, candidate).map_err(AgentEditError::policy_denied),
        )?;
        Ok(AgentEditReceipt {
            revision: self.document_revision().to_string(),
            changes,
        })
    }
}

impl AgentOperation {
    /// Materialize the same add-node defaults used when applying the operation.
    pub fn into_added_node(self) -> Option<Node> {
        let Self::AddNode {
            id,
            title,
            position,
            kind,
            size,
            ports,
            data,
        } = self
        else {
            return None;
        };
        let mut node = Node::new(id, title, position);
        if let Some(kind) = kind {
            node.kind = kind;
        }
        if let Some(size) = size {
            node.size = size;
        }
        node.ports = ports;
        node.data = data;
        Some(node)
    }

    fn apply(self, graph: &mut Graph) -> Result<(), GraphError> {
        match self {
            Self::AddNode { .. } => {
                graph.add_node(self.into_added_node().expect("add-node operation"))
            }
            Self::UpdateNode {
                id,
                title,
                position,
                size,
                kind,
                ports,
            } => {
                let node = editable_node(graph, &id)?;
                if let Some(title) = title {
                    node.title = title;
                }
                if let Some(position) = position {
                    node.position = position;
                }
                if let Some(size) = size {
                    node.size = size;
                }
                if let Some(kind) = kind {
                    node.kind = kind;
                }
                if let Some(ports) = ports {
                    node.ports = ports;
                }
                Ok(())
            }
            Self::SetNodeData { id, data } => {
                editable_node(graph, &id)?.data = data;
                Ok(())
            }
            Self::RemoveNode { id } => {
                editable_node(graph, &id)?;
                graph.remove_node(&id)?;
                Ok(())
            }
            Self::Connect { id, source, target } => graph.connect(id, source, target),
            Self::Disconnect { id } => graph.remove_edge(&id).map(|_| ()),
        }
    }
}

fn editable_node<'a>(graph: &'a mut Graph, id: &NodeId) -> Result<&'a mut Node, GraphError> {
    let node = graph
        .nodes
        .get_mut(id)
        .ok_or_else(|| GraphError::MissingNode(id.clone()))?;
    if node.locked {
        return Err(GraphError::Locked(id.to_string()));
    }
    Ok(node)
}

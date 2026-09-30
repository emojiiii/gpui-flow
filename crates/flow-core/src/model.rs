use crate::{Point, Rect, Size};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(
            Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.into())
            }
        }
        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}
string_id!(NodeId);
string_id!(PortId);
string_id!(EdgeId);
string_id!(GroupId);

/// Port type compatibility is exact, except that `Any` accepts any type.
/// Named types let applications define domain-specific sockets without a registry.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum DataType {
    #[default]
    Any,
    Bool,
    Number,
    String,
    Object,
    Named(String),
}
impl DataType {
    pub fn compatible_with(&self, other: &Self) -> bool {
        matches!(self, Self::Any) || matches!(other, Self::Any) || self == other
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortDirection {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Port {
    pub id: PortId,
    pub label: String,
    pub direction: PortDirection,
    pub data_type: DataType,
    /// `None` means unlimited fan-in/fan-out. Zero disables connections.
    pub max_connections: Option<usize>,
    #[serde(default)]
    pub data: Value,
}
impl Port {
    pub fn input(id: impl Into<PortId>, label: impl Into<String>, data_type: DataType) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            direction: PortDirection::Input,
            data_type,
            max_connections: Some(1),
            data: Value::Null,
        }
    }
    pub fn output(id: impl Into<PortId>, label: impl Into<String>, data_type: DataType) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            direction: PortDirection::Output,
            data_type,
            max_connections: None,
            data: Value::Null,
        }
    }
    pub fn with_capacity(mut self, max_connections: Option<usize>) -> Self {
        self.max_connections = max_connections;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PortRef {
    pub node: NodeId,
    pub port: PortId,
}
impl PortRef {
    pub fn new(node: impl Into<NodeId>, port: impl Into<PortId>) -> Self {
        Self {
            node: node.into(),
            port: port.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub title: String,
    /// Application-defined renderer/type identifier.
    pub kind: String,
    pub position: Point,
    pub size: Size,
    #[serde(default)]
    pub ports: Vec<Port>,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub locked: bool,
}
impl Node {
    pub fn new(id: impl Into<NodeId>, title: impl Into<String>, position: Point) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            kind: "default".into(),
            position,
            size: Size::new(224.0, 144.0),
            ports: Vec::new(),
            data: Value::Null,
            locked: false,
        }
    }
    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = kind.into();
        self
    }
    pub fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }
    pub fn with_port(mut self, port: Port) -> Self {
        self.ports.push(port);
        self
    }
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }
    pub fn bounds(&self) -> Rect {
        Rect::new(self.position, self.size)
    }
    pub fn port(&self, id: &PortId) -> Option<&Port> {
        self.ports.iter().find(|p| &p.id == id)
    }
    pub fn inputs(&self) -> impl Iterator<Item = &Port> {
        self.ports
            .iter()
            .filter(|p| p.direction == PortDirection::Input)
    }
    pub fn outputs(&self) -> impl Iterator<Item = &Port> {
        self.ports
            .iter()
            .filter(|p| p.direction == PortDirection::Output)
    }
    /// Default evenly-spaced port anchor, with a 36-world-unit title region.
    /// Custom renderers may use their own anchors without changing graph topology.
    pub fn port_position(&self, id: &PortId) -> Option<Point> {
        let port = self.port(id)?;
        let siblings: Vec<_> = self
            .ports
            .iter()
            .filter(|p| p.direction == port.direction)
            .collect();
        let index = siblings.iter().position(|p| &p.id == id)?;
        let header = 36.0_f32.min(self.size.height * 0.35);
        let y =
            header + (self.size.height - header) * (index + 1) as f32 / (siblings.len() + 1) as f32;
        let x = if port.direction == PortDirection::Input {
            0.0
        } else {
            self.size.width
        };
        Some(self.position + Point::new(x, y))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub id: EdgeId,
    pub source: PortRef,
    pub target: PortRef,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub data: Value,
}
impl Edge {
    pub fn new(id: impl Into<EdgeId>, source: PortRef, target: PortRef) -> Self {
        Self {
            id: id.into(),
            source,
            target,
            label: String::new(),
            data: Value::Null,
        }
    }
}

/// A visual group with explicit bounds. Removing a group does not remove nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: GroupId,
    pub title: String,
    pub bounds: Rect,
    #[serde(default)]
    pub node_ids: BTreeSet<NodeId>,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub locked: bool,
}
impl Group {
    pub fn new(id: impl Into<GroupId>, title: impl Into<String>, bounds: Rect) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            bounds,
            node_ids: BTreeSet::new(),
            data: Value::Null,
            locked: false,
        }
    }
    pub fn with_nodes(mut self, nodes: impl IntoIterator<Item = NodeId>) -> Self {
        self.node_ids.extend(nodes);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphOptions {
    pub allow_cycles: bool,
    pub allow_self_connections: bool,
}
impl Default for GraphOptions {
    fn default() -> Self {
        Self {
            allow_cycles: true,
            allow_self_connections: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub path: String,
    pub message: String,
}
impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    DuplicateId { kind: &'static str, id: String },
    MissingNode(NodeId),
    MissingEdge(EdgeId),
    MissingGroup(GroupId),
    MissingPort(PortRef),
    InvalidConnection(String),
    InvalidGeometry(String),
    InvalidClipboard(String),
    Validation(Vec<ValidationIssue>),
    Serialization(String),
    TransactionInProgress,
    NoActiveTransaction,
    Locked(String),
}
impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId { kind, id } => write!(f, "duplicate {kind} id: {id}"),
            Self::MissingNode(id) => write!(f, "node not found: {id}"),
            Self::MissingEdge(id) => write!(f, "edge not found: {id}"),
            Self::MissingGroup(id) => write!(f, "group not found: {id}"),
            Self::MissingPort(p) => write!(f, "port not found: {}.{}", p.node, p.port),
            Self::InvalidConnection(s) => write!(f, "invalid connection: {s}"),
            Self::InvalidGeometry(s) => write!(f, "invalid geometry: {s}"),
            Self::InvalidClipboard(s) => write!(f, "invalid clipboard: {s}"),
            Self::Validation(issues) => {
                write!(f, "graph validation failed")?;
                for i in issues {
                    write!(f, "; {i}")?;
                }
                Ok(())
            }
            Self::Serialization(s) => write!(f, "serialization error: {s}"),
            Self::TransactionInProgress => write!(f, "a transaction is already in progress"),
            Self::NoActiveTransaction => write!(f, "there is no active transaction"),
            Self::Locked(s) => write!(f, "locked item: {s}"),
        }
    }
}
impl std::error::Error for GraphError {}
impl From<serde_json::Error> for GraphError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialization(e.to_string())
    }
}

pub const GRAPH_SCHEMA_VERSION: u32 = 1;
fn schema_version() -> u32 {
    GRAPH_SCHEMA_VERSION
}

/// Deterministic, versioned graph document. Use `from_json` rather than bare
/// serde deserialization to ensure invalid references and geometry are rejected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub nodes: BTreeMap<NodeId, Node>,
    #[serde(default)]
    pub edges: BTreeMap<EdgeId, Edge>,
    #[serde(default)]
    pub groups: BTreeMap<GroupId, Group>,
    #[serde(default)]
    pub options: GraphOptions,
    #[serde(default)]
    pub data: Value,
}
impl Default for Graph {
    fn default() -> Self {
        Self {
            schema_version: GRAPH_SCHEMA_VERSION,
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            groups: BTreeMap::new(),
            options: GraphOptions::default(),
            data: Value::Null,
        }
    }
}
impl Graph {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn node(&self, id: &NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }
    pub fn port(&self, p: &PortRef) -> Option<&Port> {
        self.nodes.get(&p.node)?.port(&p.port)
    }
    pub fn port_position(&self, p: &PortRef) -> Option<Point> {
        self.nodes.get(&p.node)?.port_position(&p.port)
    }
    pub fn add_node(&mut self, node: Node) -> Result<(), GraphError> {
        if self.nodes.contains_key(&node.id) {
            return Err(GraphError::DuplicateId {
                kind: "node",
                id: node.id.to_string(),
            });
        }
        let issues = validate_node(&node);
        if !issues.is_empty() {
            return Err(GraphError::Validation(issues));
        }
        self.nodes.insert(node.id.clone(), node);
        Ok(())
    }
    /// Removes the node, incident edges, and its membership in all groups.
    pub fn remove_node(&mut self, id: &NodeId) -> Result<Node, GraphError> {
        let node = self
            .nodes
            .remove(id)
            .ok_or_else(|| GraphError::MissingNode(id.clone()))?;
        self.edges
            .retain(|_, e| &e.source.node != id && &e.target.node != id);
        for group in self.groups.values_mut() {
            group.node_ids.remove(id);
        }
        Ok(node)
    }
    pub fn add_edge(&mut self, edge: Edge) -> Result<(), GraphError> {
        if self.edges.contains_key(&edge.id) {
            return Err(GraphError::DuplicateId {
                kind: "edge",
                id: edge.id.to_string(),
            });
        }
        if edge.id.as_str().trim().is_empty() {
            return Err(GraphError::InvalidConnection(
                "edge id cannot be empty".into(),
            ));
        }
        self.validate_connection(&edge.source, &edge.target)?;
        self.edges.insert(edge.id.clone(), edge);
        Ok(())
    }
    pub fn connect(
        &mut self,
        id: impl Into<EdgeId>,
        source: PortRef,
        target: PortRef,
    ) -> Result<(), GraphError> {
        self.add_edge(Edge::new(id, source, target))
    }
    pub fn remove_edge(&mut self, id: &EdgeId) -> Result<Edge, GraphError> {
        self.edges
            .remove(id)
            .ok_or_else(|| GraphError::MissingEdge(id.clone()))
    }
    pub fn add_group(&mut self, group: Group) -> Result<(), GraphError> {
        if self.groups.contains_key(&group.id) {
            return Err(GraphError::DuplicateId {
                kind: "group",
                id: group.id.to_string(),
            });
        }
        let mut candidate = self.clone();
        candidate.groups.insert(group.id.clone(), group);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    /// Ungroup; nodes and edges are retained.
    pub fn remove_group(&mut self, id: &GroupId) -> Result<Group, GraphError> {
        self.groups
            .remove(id)
            .ok_or_else(|| GraphError::MissingGroup(id.clone()))
    }
    pub fn connection_count(&self, port: &PortRef) -> usize {
        self.edges
            .values()
            .filter(|e| &e.source == port || &e.target == port)
            .count()
    }
    pub fn validate_connection(
        &self,
        source: &PortRef,
        target: &PortRef,
    ) -> Result<(), GraphError> {
        self.check_connection(source, target, None)
    }
    fn check_connection(
        &self,
        source: &PortRef,
        target: &PortRef,
        ignore: Option<&EdgeId>,
    ) -> Result<(), GraphError> {
        let out = self
            .port(source)
            .ok_or_else(|| GraphError::MissingPort(source.clone()))?;
        let input = self
            .port(target)
            .ok_or_else(|| GraphError::MissingPort(target.clone()))?;
        let invalid = |s: &str| GraphError::InvalidConnection(s.into());
        if out.direction != PortDirection::Output || input.direction != PortDirection::Input {
            return Err(invalid("connections must run from an output to an input"));
        }
        if !self.options.allow_self_connections && source.node == target.node {
            return Err(invalid("self-connections are disabled"));
        }
        if !out.data_type.compatible_with(&input.data_type) {
            return Err(invalid("port types are incompatible"));
        }
        let edges: Vec<_> = self
            .edges
            .values()
            .filter(|e| Some(&e.id) != ignore)
            .collect();
        if edges
            .iter()
            .any(|e| &e.source == source && &e.target == target)
        {
            return Err(invalid("duplicate connection"));
        }
        for (reference, port) in [(source, out), (target, input)] {
            if let Some(max) = port.max_connections {
                if edges
                    .iter()
                    .filter(|e| &e.source == reference || &e.target == reference)
                    .count()
                    >= max
                {
                    return Err(invalid("port connection capacity reached"));
                }
            }
        }
        if !self.options.allow_cycles && self.path_exists(&target.node, &source.node, ignore) {
            return Err(invalid("connection would create a cycle"));
        }
        Ok(())
    }
    fn path_exists(&self, start: &NodeId, end: &NodeId, ignore: Option<&EdgeId>) -> bool {
        let mut stack = vec![start];
        let mut visited = BTreeSet::new();
        while let Some(node) = stack.pop() {
            if node == end {
                return true;
            }
            if visited.insert(node) {
                for e in self
                    .edges
                    .values()
                    .filter(|e| Some(&e.id) != ignore && &e.source.node == node)
                {
                    stack.push(&e.target.node);
                }
            }
        }
        false
    }
    /// All validation issues, with stable paths suitable for an import UI.
    pub fn validation_issues(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        let mut push =
            |path: String, message: String| issues.push(ValidationIssue { path, message });
        if self.schema_version != GRAPH_SCHEMA_VERSION {
            push(
                "schema_version".into(),
                format!(
                    "unsupported schema version {}; expected {}",
                    self.schema_version, GRAPH_SCHEMA_VERSION
                ),
            );
        }
        for (id, node) in &self.nodes {
            if id != &node.id {
                push(
                    format!("nodes.{id}.id"),
                    "map key and node id differ".into(),
                );
            }
        }
        for (id, edge) in &self.edges {
            if id != &edge.id {
                push(
                    format!("edges.{id}.id"),
                    "map key and edge id differ".into(),
                );
            }
            if edge.id.as_str().trim().is_empty() {
                push(format!("edges.{id}.id"), "id cannot be empty".into());
            }
            if let Err(error) = self.check_connection(&edge.source, &edge.target, Some(id)) {
                push(format!("edges.{id}"), error.to_string());
            }
        }
        let mut memberships = BTreeSet::new();
        for (id, group) in &self.groups {
            if id != &group.id {
                push(
                    format!("groups.{id}.id"),
                    "map key and group id differ".into(),
                );
            }
            if group.id.as_str().trim().is_empty() {
                push(format!("groups.{id}.id"), "id cannot be empty".into());
            }
            if !group.bounds.is_valid() || !group.bounds.size.is_positive() {
                push(
                    format!("groups.{id}.bounds"),
                    "group bounds must be finite and positive".into(),
                );
            }
            for node in &group.node_ids {
                if !self.nodes.contains_key(node) {
                    push(
                        format!("groups.{id}.node_ids"),
                        format!("missing node {node}"),
                    );
                }
                if !memberships.insert(node) {
                    push(
                        format!("groups.{id}.node_ids"),
                        format!("node {node} belongs to more than one group"),
                    );
                }
            }
        }
        for node in self.nodes.values() {
            issues.extend(validate_node(node));
        }
        issues
    }
    pub fn validate(&self) -> Result<(), GraphError> {
        let issues = self.validation_issues();
        if issues.is_empty() {
            Ok(())
        } else {
            Err(GraphError::Validation(issues))
        }
    }
    pub fn bounds(&self) -> Option<Rect> {
        self.nodes
            .values()
            .map(Node::bounds)
            .chain(self.groups.values().map(|g| g.bounds))
            .reduce(Rect::union)
    }
    pub fn node_bounds<'a>(&self, ids: impl IntoIterator<Item = &'a NodeId>) -> Option<Rect> {
        ids.into_iter()
            .filter_map(|id| self.nodes.get(id))
            .map(Node::bounds)
            .reduce(Rect::union)
    }
    pub fn refresh_group_bounds(&mut self, id: &GroupId, padding: f32) -> Result<(), GraphError> {
        if !padding.is_finite() || padding < 0.0 {
            return Err(GraphError::InvalidGeometry(
                "padding must be finite and non-negative".into(),
            ));
        }
        let group = self
            .groups
            .get(id)
            .ok_or_else(|| GraphError::MissingGroup(id.clone()))?;
        if let Some(bounds) = self.node_bounds(&group.node_ids) {
            let bounds = bounds.expanded(padding);
            if !bounds.is_valid() {
                return Err(GraphError::InvalidGeometry("group bounds overflow".into()));
            }
            self.groups.get_mut(id).expect("group exists").bounds = bounds;
        }
        Ok(())
    }
    /// Find a deterministic unused identifier across all graph item types.
    pub fn fresh_id(&self, prefix: &str) -> String {
        for i in 1_u64.. {
            let id = format!("{prefix}-{i}");
            if !self.nodes.contains_key(&NodeId::from(id.clone()))
                && !self.edges.contains_key(&EdgeId::from(id.clone()))
                && !self.groups.contains_key(&GroupId::from(id.clone()))
            {
                return id;
            }
        }
        unreachable!("identifier space exhausted")
    }
    pub fn to_json(&self) -> Result<String, GraphError> {
        self.validate()?;
        Ok(serde_json::to_string_pretty(self)?)
    }
    pub fn from_json(json: &str) -> Result<Self, GraphError> {
        let graph: Self = serde_json::from_str(json)?;
        graph.validate()?;
        Ok(graph)
    }
}

fn validate_node(node: &Node) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let mut push = |field: &str, message: String| {
        issues.push(ValidationIssue {
            path: format!("nodes.{}.{}", node.id, field),
            message,
        })
    };
    if node.id.as_str().trim().is_empty() {
        push("id", "id cannot be empty".into());
    }
    if !node.bounds().is_valid() || !node.size.is_positive() {
        push(
            "bounds",
            "node position must be finite and size must be positive".into(),
        );
    }
    let mut ports = BTreeSet::new();
    for port in &node.ports {
        if port.id.as_str().trim().is_empty() {
            push("ports", "port id cannot be empty".into());
        }
        if !ports.insert(&port.id) {
            push("ports", format!("duplicate port id {}", port.id));
        }
        if matches!(&port.data_type, DataType::Named(name) if name.trim().is_empty()) {
            push("ports", format!("port {} has an empty named type", port.id));
        }
    }
    issues
}

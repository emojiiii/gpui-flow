use crate::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
    sync::atomic::{AtomicU64, Ordering},
};

fn next_document_revision() -> u64 {
    static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);
    NEXT_REVISION
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |revision| {
            revision.checked_add(1)
        })
        .expect("editor document revisions exhausted")
}

/// Selection is transient; it is not stored in exported documents.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Selection {
    pub nodes: BTreeSet<NodeId>,
    pub edges: BTreeSet<EdgeId>,
    pub groups: BTreeSet<GroupId>,
}
impl Selection {
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.edges.clear();
        self.groups.clear();
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty() && self.groups.is_empty()
    }
    pub fn len(&self) -> usize {
        self.nodes.len() + self.edges.len() + self.groups.len()
    }
    pub fn select_node(&mut self, id: impl Into<NodeId>, additive: bool) {
        if !additive {
            self.clear();
        }
        self.nodes.insert(id.into());
    }
    pub fn toggle_node(&mut self, id: impl Into<NodeId>) {
        let id = id.into();
        if !self.nodes.remove(&id) {
            self.nodes.insert(id);
        }
    }
    pub fn select_edge(&mut self, id: impl Into<EdgeId>, additive: bool) {
        if !additive {
            self.clear();
        }
        self.edges.insert(id.into());
    }
    pub fn select_group(&mut self, id: impl Into<GroupId>, additive: bool) {
        if !additive {
            self.clear();
        }
        self.groups.insert(id.into());
    }
    pub fn retain_existing(&mut self, graph: &Graph) {
        let hidden = hidden_node_ids(graph);
        self.nodes
            .retain(|id| graph.nodes.contains_key(id) && !hidden.contains(id));
        self.edges.retain(|id| {
            graph.edges.get(id).is_some_and(|edge| {
                !hidden.contains(&edge.source.node) && !hidden.contains(&edge.target.node)
            })
        });
        self.groups.retain(|id| graph.groups.contains_key(id));
    }
    /// Includes members of selected groups, once each.
    pub fn expanded_nodes(&self, graph: &Graph) -> BTreeSet<NodeId> {
        let mut nodes = self.nodes.clone();
        for id in &self.groups {
            if let Some(group) = graph.groups.get(id) {
                nodes.extend(group.node_ids.iter().cloned());
            }
        }
        nodes.retain(|id| graph.nodes.contains_key(id));
        nodes
    }
}

/// A self-contained clipboard fragment. External edges are intentionally omitted.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clipboard {
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub options: GraphOptions,
}
impl Clipboard {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.groups.is_empty()
    }
    pub fn validate(&self) -> Result<(), GraphError> {
        let mut graph = Graph {
            options: self.options,
            ..Graph::default()
        };
        for node in &self.nodes {
            graph.add_node(node.clone())?;
        }
        for edge in &self.edges {
            graph.add_edge(edge.clone())?;
        }
        for group in &self.groups {
            graph.add_group(group.clone())?;
        }
        graph.validate()
    }
    pub fn to_json(&self) -> Result<String, GraphError> {
        self.validate()?;
        Ok(serde_json::to_string_pretty(self)?)
    }
    pub fn from_json(json: &str) -> Result<Self, GraphError> {
        let clipboard: Self = serde_json::from_str(json)?;
        clipboard.validate()?;
        Ok(clipboard)
    }
}

/// Persist both graph and camera state without serializing transient history.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowDocument {
    pub graph: Graph,
    #[serde(default)]
    pub viewport: Viewport,
}
impl FlowDocument {
    pub fn validate(&self) -> Result<(), GraphError> {
        self.graph.validate()?;
        if !self.viewport.is_valid() {
            return Err(GraphError::InvalidGeometry("invalid viewport".into()));
        }
        Ok(())
    }
    pub fn to_json(&self) -> Result<String, GraphError> {
        self.validate()?;
        Ok(serde_json::to_string_pretty(self)?)
    }
    pub fn from_json(json: &str) -> Result<Self, GraphError> {
        let document: Self = serde_json::from_str(json)?;
        document.validate()?;
        Ok(document)
    }

    /// Check the exact byte length of the saved pretty JSON without allocating it.
    pub fn validate_json_size(&self, max_bytes: usize) -> Result<(), GraphError> {
        pretty_json_len(self, max_bytes).map(|_| ())
    }
}

#[derive(Serialize)]
struct DocumentJson<G> {
    graph: G,
    viewport: Viewport,
}

struct JsonBudget {
    remaining: usize,
    overflow: bool,
}

impl Write for JsonBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let Some(remaining) = self.remaining.checked_sub(bytes.len()) else {
            self.overflow = true;
            return Err(io::Error::other("document byte limit exceeded"));
        };
        self.remaining = remaining;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn pretty_json_len(value: &impl Serialize, max_bytes: usize) -> Result<usize, GraphError> {
    let mut output = JsonBudget {
        remaining: max_bytes,
        overflow: false,
    };
    serde_json::to_writer_pretty(&mut output, value).map_err(|error| {
        if output.overflow {
            GraphError::DocumentTooLarge { max_bytes }
        } else {
            error.into()
        }
    })?;
    Ok(max_bytes - output.remaining)
}

#[derive(Debug, Clone, Copy)]
struct DocumentBudget {
    max_bytes: usize,
    // Size above the same envelope with a null graph, independent of the camera.
    graph_bytes: usize,
}

impl DocumentBudget {
    fn measure(graph: &Graph, viewport: Viewport, max_bytes: usize) -> Result<Self, GraphError> {
        let bytes = pretty_json_len(&DocumentJson { graph, viewport }, max_bytes)?;
        Ok(Self {
            max_bytes,
            graph_bytes: bytes - Self::viewport_bytes(viewport),
        })
    }

    fn viewport_bytes(viewport: Viewport) -> usize {
        pretty_json_len(
            &DocumentJson {
                graph: (),
                viewport,
            },
            usize::MAX,
        )
        .expect("a viewport and null serialize without a byte limit")
    }

    fn check_viewport(self, viewport: Viewport) -> Result<(), GraphError> {
        if Self::viewport_bytes(viewport) > self.max_bytes - self.graph_bytes {
            return Err(GraphError::DocumentTooLarge {
                max_bytes: self.max_bytes,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Snapshot {
    graph: Graph,
    selection: Selection,
}
#[derive(Debug, Clone)]
struct HistoryEntry {
    label: String,
    before: Snapshot,
    after: Snapshot,
}
#[derive(Debug, Clone)]
struct PendingTransaction {
    label: String,
    before: Snapshot,
    budget: Option<DocumentBudget>,
}

/// Graph controller with atomic validated edits and bounded snapshot history.
///
/// Use `begin_transaction`, repeated `transaction`/`move_selection` calls, and
/// `commit_transaction` to coalesce a pointer gesture into a single undo step.
/// Escape can call `cancel_transaction`. Failed updates leave the current
/// transaction intact and do not partially mutate the graph.
#[derive(Debug)]
pub struct Editor {
    graph: Graph,
    document_revision: u64,
    pub selection: Selection,
    pub viewport: Viewport,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    pending: Option<PendingTransaction>,
    history_limit: usize,
    document_budget: Option<DocumentBudget>,
}
impl Clone for Editor {
    fn clone(&self) -> Self {
        Self {
            graph: self.graph.clone(),
            document_revision: next_document_revision(),
            selection: self.selection.clone(),
            viewport: self.viewport,
            undo: self.undo.clone(),
            redo: self.redo.clone(),
            pending: self.pending.clone(),
            history_limit: self.history_limit,
            document_budget: self.document_budget,
        }
    }
}
impl Default for Editor {
    fn default() -> Self {
        Self::new(Graph::default()).expect("empty graph is valid")
    }
}
impl Editor {
    pub fn new(graph: Graph) -> Result<Self, GraphError> {
        graph.validate()?;
        Ok(Self {
            graph,
            document_revision: next_document_revision(),
            selection: Selection::default(),
            viewport: Viewport::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            pending: None,
            history_limit: 100,
            document_budget: None,
        })
    }
    pub fn graph(&self) -> &Graph {
        &self.graph
    }
    /// A process-local stamp for asynchronous work that depends on this graph.
    /// Edits, restores, replacement and cloning receive a new stamp. Compare
    /// the viewport separately when the operation also depends on the camera.
    pub fn document_revision(&self) -> u64 {
        self.document_revision
    }
    pub fn document(&self) -> FlowDocument {
        FlowDocument {
            graph: self.graph.clone(),
            viewport: self.viewport,
        }
    }
    pub fn from_document(document: FlowDocument) -> Result<Self, GraphError> {
        document.validate()?;
        let mut editor = Self::new(document.graph)?;
        editor.viewport = document.viewport;
        Ok(editor)
    }
    pub fn to_json(&self) -> Result<String, GraphError> {
        self.document().to_json()
    }
    pub fn from_json(json: &str) -> Result<Self, GraphError> {
        Self::from_document(FlowDocument::from_json(json)?)
    }
    /// Load a document, clearing selection and undo/redo state.
    pub fn load_document(&mut self, document: FlowDocument) -> Result<(), GraphError> {
        if self.pending.is_some() {
            return Err(GraphError::TransactionInProgress);
        }
        let mut replacement = Self::from_document(document)?;
        replacement.document_budget =
            self.measure_budget(&replacement.graph, replacement.viewport)?;
        replacement.history_limit = self.history_limit;
        *self = replacement;
        Ok(())
    }

    /// Import a document as one graph edit. The imported camera remains navigation state.
    pub fn import_document(
        &mut self,
        document: FlowDocument,
        label: impl Into<String>,
    ) -> Result<(), GraphError> {
        if self.pending.is_some() {
            return Err(GraphError::TransactionInProgress);
        }
        document.validate()?;
        self.document_budget = self.measure_budget(&document.graph, document.viewport)?;
        let before = self.snapshot();
        self.graph = document.graph;
        self.viewport = document.viewport;
        self.selection.retain_existing(&self.graph);
        self.record(label, before);
        Ok(())
    }

    /// Keep committed graph and checked camera changes within a save format limit.
    pub fn set_document_byte_limit(&mut self, max_bytes: usize) -> Result<(), GraphError> {
        if self.pending.is_some() {
            return Err(GraphError::TransactionInProgress);
        }
        self.document_budget = Some(DocumentBudget::measure(
            &self.graph,
            self.viewport,
            max_bytes,
        )?);
        Ok(())
    }

    pub fn set_viewport(&mut self, viewport: Viewport) -> Result<(), GraphError> {
        if !viewport.is_valid() {
            return Err(GraphError::InvalidGeometry("invalid viewport".into()));
        }
        if let Some(budget) = self.document_budget {
            budget.check_viewport(viewport)?;
        }
        if let Some(budget) = self.pending.as_ref().and_then(|pending| pending.budget) {
            budget.check_viewport(viewport)?;
        }
        self.viewport = viewport;
        Ok(())
    }

    fn measure_budget(
        &self,
        graph: &Graph,
        viewport: Viewport,
    ) -> Result<Option<DocumentBudget>, GraphError> {
        self.document_budget
            .map(|budget| DocumentBudget::measure(graph, viewport, budget.max_bytes))
            .transpose()
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            graph: self.graph.clone(),
            selection: self.selection.clone(),
        }
    }
    fn restore(&mut self, snapshot: Snapshot) {
        if self.graph != snapshot.graph {
            self.document_revision = next_document_revision();
        }
        self.graph = snapshot.graph;
        self.selection = snapshot.selection;
    }
    fn record(&mut self, label: impl Into<String>, before: Snapshot) -> bool {
        if before.graph == self.graph {
            return false;
        }
        self.document_revision = next_document_revision();
        if self.pending.is_some() {
            return true;
        }
        self.redo.clear();
        if self.history_limit > 0 {
            self.undo.push(HistoryEntry {
                label: label.into(),
                before,
                after: self.snapshot(),
            });
            let excess = self.undo.len().saturating_sub(self.history_limit);
            if excess > 0 {
                self.undo.drain(..excess);
            }
        }
        true
    }
    /// Mutate a private candidate graph. Only validated results are committed;
    /// errors and panics inside `edit` cannot leave a partially modified graph.
    pub fn transaction<R>(
        &mut self,
        label: impl Into<String>,
        edit: impl FnOnce(&mut Graph) -> Result<R, GraphError>,
    ) -> Result<R, GraphError> {
        let before = self.snapshot();
        let mut candidate = self.graph.clone();
        let result = edit(&mut candidate)?;
        candidate.validate()?;
        self.document_budget = self.measure_budget(&candidate, self.viewport)?;
        self.graph = candidate;
        self.selection.retain_existing(&self.graph);
        self.record(label, before);
        Ok(result)
    }
    pub fn begin_transaction(&mut self, label: impl Into<String>) -> Result<(), GraphError> {
        if self.pending.is_some() {
            return Err(GraphError::TransactionInProgress);
        }
        self.pending = Some(PendingTransaction {
            label: label.into(),
            before: self.snapshot(),
            budget: self.document_budget,
        });
        Ok(())
    }
    /// Returns whether the gesture changed the graph and created an edit.
    pub fn commit_transaction(&mut self) -> Result<bool, GraphError> {
        let pending = self.pending.take().ok_or(GraphError::NoActiveTransaction)?;
        if let Err(error) = self.graph.validate() {
            self.document_budget = pending.budget;
            self.restore(pending.before);
            return Err(error);
        }
        self.selection.retain_existing(&self.graph);
        Ok(self.record(pending.label, pending.before))
    }
    pub fn cancel_transaction(&mut self) -> Result<(), GraphError> {
        let pending = self.pending.take().ok_or(GraphError::NoActiveTransaction)?;
        self.document_budget = pending.budget;
        self.restore(pending.before);
        Ok(())
    }
    pub fn transaction_active(&self) -> bool {
        self.pending.is_some()
    }
    pub fn can_undo(&self) -> bool {
        self.pending.is_none() && !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        self.pending.is_none() && !self.redo.is_empty()
    }
    pub fn history_len(&self) -> usize {
        self.undo.len()
    }
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }
    pub fn history_limit(&self) -> usize {
        self.history_limit
    }
    pub fn set_history_limit(&mut self, limit: usize) {
        self.history_limit = limit;
        let excess = self.undo.len().saturating_sub(limit);
        if excess > 0 {
            self.undo.drain(..excess);
        }
        let excess = self.redo.len().saturating_sub(limit);
        if excess > 0 {
            self.redo.drain(..excess);
        }
    }
    pub fn clear_history(&mut self) -> Result<(), GraphError> {
        if self.pending.is_some() {
            return Err(GraphError::TransactionInProgress);
        }
        self.undo.clear();
        self.redo.clear();
        Ok(())
    }
    /// Undo is disabled during an active gesture; cancel or commit it first.
    pub fn undo(&mut self) -> bool {
        self.try_undo().unwrap_or(false)
    }
    pub fn try_undo(&mut self) -> Result<bool, GraphError> {
        if self.pending.is_some() {
            return Ok(false);
        }
        let Some(entry) = self.undo.last() else {
            return Ok(false);
        };
        self.document_budget = self.measure_budget(&entry.before.graph, self.viewport)?;
        let entry = self.undo.pop().expect("undo entry was checked");
        self.restore(entry.before.clone());
        self.redo.push(entry);
        Ok(true)
    }
    pub fn redo(&mut self) -> bool {
        self.try_redo().unwrap_or(false)
    }
    pub fn try_redo(&mut self) -> Result<bool, GraphError> {
        if self.pending.is_some() {
            return Ok(false);
        }
        let Some(entry) = self.redo.last() else {
            return Ok(false);
        };
        self.document_budget = self.measure_budget(&entry.after.graph, self.viewport)?;
        let entry = self.redo.pop().expect("redo entry was checked");
        self.restore(entry.after.clone());
        self.undo.push(entry);
        let excess = self.undo.len().saturating_sub(self.history_limit);
        if excess > 0 {
            self.undo.drain(..excess);
        }
        Ok(true)
    }
    pub fn select_all(&mut self) {
        let hidden = hidden_node_ids(&self.graph);
        self.selection = Selection {
            nodes: self
                .graph
                .nodes
                .keys()
                .filter(|id| !hidden.contains(*id))
                .cloned()
                .collect(),
            edges: self
                .graph
                .edges
                .values()
                .filter(|edge| {
                    !hidden.contains(&edge.source.node) && !hidden.contains(&edge.target.node)
                })
                .map(|edge| edge.id.clone())
                .collect(),
            groups: self.graph.groups.keys().cloned().collect(),
        };
    }
    /// Marquee selection in world coordinates. Existing selection can be retained.
    pub fn select_rect(&mut self, rect: Rect, additive: bool, fully_contained: bool) {
        if !rect.is_valid() {
            return;
        }
        if !additive {
            self.selection.clear();
        }
        let hidden = hidden_node_ids(&self.graph);
        self.selection.retain_existing(&self.graph);
        for node in self
            .graph
            .nodes
            .values()
            .filter(|node| !hidden.contains(&node.id))
        {
            if if fully_contained {
                rect.contains_rect(node.bounds())
            } else {
                rect.intersects(node.bounds())
            } {
                self.selection.nodes.insert(node.id.clone());
            }
        }
    }
    pub fn selection_bounds(&self) -> Option<Rect> {
        let hidden = hidden_node_ids(&self.graph);
        let nodes = self.selection.expanded_nodes(&self.graph);
        self.graph
            .node_bounds(nodes.iter().filter(|id| !hidden.contains(*id)))
            .into_iter()
            .chain(
                self.selection
                    .groups
                    .iter()
                    .filter_map(|id| self.graph.groups.get(id).map(|g| g.bounds)),
            )
            .reduce(Rect::union)
    }
    pub fn move_selection(&mut self, delta: Point) -> Result<(), GraphError> {
        if !delta.is_finite() {
            return Err(GraphError::InvalidGeometry(
                "movement must be finite".into(),
            ));
        }
        let nodes = self.selection.expanded_nodes(&self.graph);
        let groups = self.selection.groups.clone();
        self.transaction("Move selection", move |graph| {
            for id in &nodes {
                let node = graph
                    .nodes
                    .get_mut(id)
                    .ok_or_else(|| GraphError::MissingNode(id.clone()))?;
                if node.locked {
                    return Err(GraphError::Locked(id.to_string()));
                }
                node.position += delta;
            }
            for id in &groups {
                let group = graph
                    .groups
                    .get_mut(id)
                    .ok_or_else(|| GraphError::MissingGroup(id.clone()))?;
                if group.locked {
                    return Err(GraphError::Locked(id.to_string()));
                }
                group.bounds = group.bounds.translated(delta);
            }
            Ok(())
        })
    }
    pub fn resize_node(&mut self, id: &NodeId, size: Size) -> Result<(), GraphError> {
        if !size.is_positive() {
            return Err(GraphError::InvalidGeometry(
                "node size must be finite and positive".into(),
            ));
        }
        self.transaction("Resize node", |graph| {
            let node = graph
                .nodes
                .get_mut(id)
                .ok_or_else(|| GraphError::MissingNode(id.clone()))?;
            if node.locked {
                return Err(GraphError::Locked(id.to_string()));
            }
            node.size = size;
            Ok(())
        })
    }
    /// Delete selected nodes and edges. Selected groups are ungrouped, keeping
    /// their member nodes unless those nodes were themselves selected.
    pub fn delete_selection(&mut self) -> Result<(), GraphError> {
        let selection = self.selection.clone();
        self.transaction("Delete selection", move |graph| {
            for id in &selection.nodes {
                if graph.nodes.get(id).is_some_and(|n| n.locked) {
                    return Err(GraphError::Locked(id.to_string()));
                }
                if graph.nodes.contains_key(id) {
                    graph.remove_node(id)?;
                }
            }
            for id in &selection.edges {
                graph.edges.remove(id);
            }
            for id in &selection.groups {
                if graph.groups.get(id).is_some_and(|g| g.locked) {
                    return Err(GraphError::Locked(id.to_string()));
                }
                graph.groups.remove(id);
            }
            Ok(())
        })
    }
    pub fn copy_selection(&self) -> Clipboard {
        let hidden = hidden_node_ids(&self.graph);
        let mut nodes = self.selection.expanded_nodes(&self.graph);
        nodes.retain(|id| !hidden.contains(id));
        Clipboard {
            nodes: ordered_node_ids(&self.graph)
                .iter()
                .filter(|id| nodes.contains(*id))
                .filter_map(|id| self.graph.nodes.get(id).cloned())
                .collect(),
            edges: self
                .graph
                .edges
                .values()
                .filter(|e| nodes.contains(&e.source.node) && nodes.contains(&e.target.node))
                .cloned()
                .collect(),
            groups: self
                .graph
                .groups
                .values()
                .filter(|g| {
                    self.selection.groups.contains(&g.id)
                        || (!g.node_ids.is_empty() && g.node_ids.is_subset(&nodes))
                })
                .cloned()
                .map(|mut group| {
                    group.node_ids.retain(|id| nodes.contains(id));
                    group
                })
                .collect(),
            options: self.graph.options,
        }
    }
    /// Paste with fresh IDs, retaining all internal references. `offset` is a
    /// world-space translation relative to the copied coordinates.
    pub fn paste(&mut self, clipboard: &Clipboard, offset: Point) -> Result<Selection, GraphError> {
        clipboard.validate()?;
        if !offset.is_finite() {
            return Err(GraphError::InvalidGeometry(
                "paste offset must be finite".into(),
            ));
        }
        let before = self.snapshot();
        let mut graph = self.graph.clone();
        let mut mapping = BTreeMap::new();
        let mut selection = Selection::default();
        let mut inserted = Vec::with_capacity(clipboard.nodes.len());
        for original in &clipboard.nodes {
            let mut node = original.clone();
            let id = NodeId::from(graph.fresh_id("node"));
            mapping.insert(original.id.clone(), id.clone());
            node.id = id.clone();
            node.position += offset;
            graph.add_node(node)?;
            inserted.push(id.clone());
            selection.nodes.insert(id);
        }
        // Preserve the clipboard's visual order even when fresh IDs cross a
        // lexical boundary (node-9, node-10). Legacy scalar host metadata keeps
        // its previous behavior rather than being replaced implicitly.
        let needs_order = inserted.windows(2).any(|ids| ids[0] > ids[1])
            || clipboard
                .nodes
                .windows(2)
                .any(|nodes| nodes[0].id > nodes[1].id)
            || graph
                .data
                .get("flow_presentation")
                .and_then(|v| v.get("node_order"))
                .is_some();
        if needs_order && (graph.data.is_null() || graph.data.is_object()) {
            append_nodes_to_front(&mut graph, inserted)?;
        }
        for original in &clipboard.edges {
            let mut edge = original.clone();
            edge.id = EdgeId::from(graph.fresh_id("edge"));
            edge.source.node = mapping
                .get(&original.source.node)
                .ok_or_else(|| GraphError::InvalidClipboard("source node missing".into()))?
                .clone();
            edge.target.node = mapping
                .get(&original.target.node)
                .ok_or_else(|| GraphError::InvalidClipboard("target node missing".into()))?
                .clone();
            selection.edges.insert(edge.id.clone());
            graph.add_edge(edge)?;
        }
        for original in &clipboard.groups {
            let mut group = original.clone();
            group.id = GroupId::from(graph.fresh_id("group"));
            group.bounds = group.bounds.translated(offset);
            group.node_ids = original
                .node_ids
                .iter()
                .map(|id| {
                    mapping
                        .get(id)
                        .cloned()
                        .ok_or_else(|| GraphError::InvalidClipboard("group member missing".into()))
                })
                .collect::<Result<_, _>>()?;
            selection.groups.insert(group.id.clone());
            graph.add_group(group)?;
        }
        graph.validate()?;
        self.document_budget = self.measure_budget(&graph, self.viewport)?;
        self.graph = graph;
        self.selection = selection.clone();
        self.record("Paste", before);
        Ok(selection)
    }
    /// Duplicate selected items in a single undo step.
    pub fn duplicate_selection(&mut self, offset: Point) -> Result<Selection, GraphError> {
        let clipboard = self.copy_selection();
        self.paste(&clipboard, offset)
    }
    /// Group selected nodes (including selected groups' members), reparenting
    /// them from prior groups. Empty old groups are kept for explicit cleanup.
    pub fn group_selection(
        &mut self,
        title: impl Into<String>,
        padding: f32,
    ) -> Result<GroupId, GraphError> {
        if !padding.is_finite() || padding < 0.0 {
            return Err(GraphError::InvalidGeometry(
                "group padding must be finite and non-negative".into(),
            ));
        }
        let nodes = self.selection.expanded_nodes(&self.graph);
        let bounds = self
            .graph
            .node_bounds(&nodes)
            .ok_or_else(|| GraphError::InvalidGeometry("select at least one node to group".into()))?
            .expanded(padding);
        let id = GroupId::from(self.graph.fresh_id("group"));
        let group = Group::new(id.clone(), title, bounds).with_nodes(nodes.clone());
        let before = self.snapshot();
        let mut candidate = self.graph.clone();
        for old in candidate.groups.values_mut() {
            if !old.node_ids.is_disjoint(&nodes) && old.locked {
                return Err(GraphError::Locked(old.id.to_string()));
            }
            old.node_ids.retain(|n| !nodes.contains(n));
        }
        candidate.add_group(group)?;
        self.document_budget = self.measure_budget(&candidate, self.viewport)?;
        self.graph = candidate;
        self.selection.clear();
        self.selection.groups.insert(id.clone());
        self.record("Group selection", before);
        Ok(id)
    }
    pub fn ungroup_selection(&mut self) -> Result<(), GraphError> {
        let groups = self.selection.groups.clone();
        self.transaction("Ungroup selection", |graph| {
            for id in &groups {
                if graph.groups.get(id).is_some_and(|g| g.locked) {
                    return Err(GraphError::Locked(id.to_string()));
                }
                graph.groups.remove(id);
            }
            Ok(())
        })
    }
    pub fn fit_to_content(&mut self, canvas: Size, padding: f32) -> bool {
        self.try_fit_to_content(canvas, padding).unwrap_or(false)
    }
    pub fn try_fit_to_content(&mut self, canvas: Size, padding: f32) -> Result<bool, GraphError> {
        let Some(bounds) = visible_graph_bounds(&self.graph) else {
            return Ok(false);
        };
        let mut viewport = self.viewport;
        if !viewport.fit(bounds, canvas, padding) {
            return Ok(false);
        }
        self.set_viewport(viewport)?;
        Ok(true)
    }
    pub fn fit_to_selection(&mut self, canvas: Size, padding: f32) -> bool {
        let mut viewport = self.viewport;
        self.selection_bounds()
            .is_some_and(|bounds| viewport.fit(bounds, canvas, padding))
            && self.set_viewport(viewport).is_ok()
    }
    /// Build renderer-independent minimap geometry, including the visible camera.
    pub fn minimap(&self, canvas: Size, map_bounds: Rect, padding: f32) -> Option<MinimapData> {
        if !canvas.is_positive() || !self.viewport.is_valid() {
            return None;
        }
        let visible = self.viewport.visible_world_rect(canvas);
        let hidden = hidden_node_ids(&self.graph);
        let world =
            visible_graph_bounds(&self.graph).map_or(visible, |bounds| bounds.union(visible));
        let transform = MinimapTransform::new(world, map_bounds, padding)?;
        Some(MinimapData {
            transform,
            nodes: self
                .graph
                .nodes
                .values()
                .filter(|node| !hidden.contains(&node.id))
                .map(|n| (n.id.clone(), transform.world_rect_to_map(n.bounds())))
                .collect(),
            groups: self
                .graph
                .groups
                .values()
                .map(|g| (g.id.clone(), transform.world_rect_to_map(g.bounds)))
                .collect(),
            viewport: transform.world_rect_to_map(visible),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MinimapData {
    pub transform: MinimapTransform,
    pub nodes: Vec<(NodeId, Rect)>,
    pub groups: Vec<(GroupId, Rect)>,
    pub viewport: Rect,
}

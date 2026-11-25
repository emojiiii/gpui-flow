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
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
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
        self.nodes.retain(|id| graph.nodes.contains_key(id));
        self.edges.retain(|id| graph.edges.contains_key(id));
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
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub struct FlowDocument {
    pub graph: Graph,
    #[serde(default)]
    pub viewport: Viewport,
}

#[derive(Serialize)]
#[serde(rename = "FlowDocument")]
struct FlowDocumentRef<'a> {
    graph: &'a Graph,
    viewport: &'a Viewport,
}

impl FlowDocumentRef<'_> {
    fn validate_json_size(&self, max_bytes: usize) -> Result<(), GraphError> {
        let mut writer = BoundedJsonWriter {
            remaining: max_bytes,
            exceeded: false,
        };
        serde_json::to_writer_pretty(&mut writer, self).map_err(|error| {
            if writer.exceeded {
                GraphError::DocumentTooLarge { max_bytes }
            } else {
                error.into()
            }
        })
    }
}

struct BoundedJsonWriter {
    remaining: usize,
    exceeded: bool,
}

impl Write for BoundedJsonWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let Some(remaining) = self.remaining.checked_sub(bytes.len()) else {
            self.exceeded = true;
            return Err(io::Error::other("document byte limit exceeded"));
        };
        self.remaining = remaining;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Serialize for FlowDocument {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_ref().serialize(serializer)
    }
}

impl FlowDocument {
    fn as_ref(&self) -> FlowDocumentRef<'_> {
        FlowDocumentRef {
            graph: &self.graph,
            viewport: &self.viewport,
        }
    }

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

    /// Check the UTF-8 size of the pretty JSON document without allocating it.
    pub fn validate_json_size(&self, max_bytes: usize) -> Result<(), GraphError> {
        self.as_ref().validate_json_size(max_bytes)
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
}

pub(crate) struct StagedGraph<'a> {
    editor: &'a mut Editor,
    before: Option<Graph>,
}

impl StagedGraph<'_> {
    pub(crate) fn before(&self) -> &Graph {
        self.before.as_ref().expect("staged graph is active")
    }

    pub(crate) fn editor(&self) -> &Editor {
        self.editor
    }

    pub(crate) fn graph_mut(&mut self) -> &mut Graph {
        &mut self.editor.graph
    }
}

impl Drop for StagedGraph<'_> {
    fn drop(&mut self) {
        if let Some(before) = self.before.take() {
            self.editor.graph = before;
        }
    }
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
    document_byte_limit: Option<usize>,
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
            document_byte_limit: self.document_byte_limit,
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
            document_byte_limit: None,
        })
    }
    pub fn graph(&self) -> &Graph {
        &self.graph
    }
    /// Compare this stamp across asynchronous work that depends on the current
    /// graph and history. An independent editor starts with a distinct stamp.
    /// Compare the viewport separately when it is part of the operation.
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
        if let Some(max_bytes) = self.document_byte_limit {
            replacement.set_document_byte_limit(max_bytes)?;
        }
        replacement.history_limit = self.history_limit;
        *self = replacement;
        Ok(())
    }

    /// Import graph and camera atomically, recording only the graph in undo history.
    pub fn import_document(
        &mut self,
        document: FlowDocument,
        label: impl Into<String>,
    ) -> Result<(), GraphError> {
        if self.pending.is_some() {
            return Err(GraphError::TransactionInProgress);
        }
        document.validate()?;
        self.check_document_size(&document.graph, &document.viewport)?;
        let before = self.snapshot();
        self.graph = document.graph;
        self.viewport = document.viewport;
        self.selection.retain_existing(&self.graph);
        self.record(label, before);
        Ok(())
    }

    /// Enforce a pretty JSON byte limit on graph and checked viewport changes.
    pub fn set_document_byte_limit(&mut self, max_bytes: usize) -> Result<(), GraphError> {
        if self.pending.is_some() {
            return Err(GraphError::TransactionInProgress);
        }
        FlowDocumentRef {
            graph: &self.graph,
            viewport: &self.viewport,
        }
        .validate_json_size(max_bytes)?;
        self.document_byte_limit = Some(max_bytes);
        Ok(())
    }

    /// Set the camera while honoring the editor's document byte limit.
    pub fn set_viewport(&mut self, viewport: Viewport) -> Result<(), GraphError> {
        if !viewport.is_valid() {
            return Err(GraphError::InvalidGeometry("invalid viewport".into()));
        }
        if self.viewport != viewport {
            self.check_document_size(&self.graph, &viewport)?;
            if let Some(pending) = &self.pending {
                self.check_document_size(&pending.before.graph, &viewport)?;
            }
            self.viewport = viewport;
        }
        Ok(())
    }

    fn check_document_size(&self, graph: &Graph, viewport: &Viewport) -> Result<(), GraphError> {
        if let Some(max_bytes) = self.document_byte_limit {
            FlowDocumentRef { graph, viewport }.validate_json_size(max_bytes)?;
        }
        Ok(())
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            graph: self.graph.clone(),
            selection: self.selection.clone(),
        }
    }
    fn restore(&mut self, snapshot: Snapshot) {
        self.graph = snapshot.graph;
        self.selection = snapshot.selection;
        self.document_revision = next_document_revision();
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
        self.checked_transaction(label, |_, candidate| edit(candidate), |_| Ok(()))
    }

    pub(crate) fn checked_transaction<R, E: From<GraphError>>(
        &mut self,
        label: impl Into<String>,
        edit: impl FnOnce(&Graph, &mut Graph) -> Result<R, E>,
        validate: impl FnOnce(&Graph) -> Result<(), E>,
    ) -> Result<R, E> {
        self.checked_staged_transaction(
            label,
            |staged| {
                let before = staged.before.as_ref().expect("staged graph is active");
                edit(before, &mut staged.editor.graph)
            },
            |_, candidate| validate(candidate),
        )
    }

    /// Stage a candidate graph inside the exclusively borrowed editor. Policy can
    /// inspect earlier steps through the normal read-only Editor interface.
    pub(crate) fn checked_staged_transaction<R, E: From<GraphError>>(
        &mut self,
        label: impl Into<String>,
        edit: impl FnOnce(&mut StagedGraph<'_>) -> Result<R, E>,
        validate: impl FnOnce(&Graph, &Graph) -> Result<(), E>,
    ) -> Result<R, E> {
        let candidate = self.graph.clone();
        let before = std::mem::replace(&mut self.graph, candidate);
        let mut staged = StagedGraph {
            editor: self,
            before: Some(before),
        };
        let result = edit(&mut staged)?;
        staged.editor.graph.validate_change_from(staged.before())?;
        staged
            .editor
            .check_document_size(&staged.editor.graph, &staged.editor.viewport)?;
        validate(staged.before(), &staged.editor.graph)?;
        let before = Snapshot {
            graph: staged.before.take().expect("staged graph is active"),
            selection: staged.editor.selection.clone(),
        };
        staged
            .editor
            .selection
            .retain_existing(&staged.editor.graph);
        staged.editor.record(label, before);
        Ok(result)
    }
    pub fn begin_transaction(&mut self, label: impl Into<String>) -> Result<(), GraphError> {
        if self.pending.is_some() {
            return Err(GraphError::TransactionInProgress);
        }
        self.pending = Some(PendingTransaction {
            label: label.into(),
            before: self.snapshot(),
        });
        Ok(())
    }
    /// Returns whether the gesture changed the graph and created an edit.
    pub fn commit_transaction(&mut self) -> Result<bool, GraphError> {
        let pending = self.pending.take().ok_or(GraphError::NoActiveTransaction)?;
        if let Err(error) = self.graph.validate() {
            self.restore(pending.before);
            return Err(error);
        }
        self.selection.retain_existing(&self.graph);
        Ok(self.record(pending.label, pending.before))
    }
    pub fn cancel_transaction(&mut self) -> Result<(), GraphError> {
        let pending = self.pending.take().ok_or(GraphError::NoActiveTransaction)?;
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
        let undo_excess = self.undo.len().saturating_sub(limit);
        let redo_excess = self.redo.len().saturating_sub(limit);
        if undo_excess > 0 || redo_excess > 0 {
            self.undo.drain(..undo_excess);
            self.redo.drain(..redo_excess);
            self.document_revision = next_document_revision();
        }
    }
    pub fn clear_history(&mut self) -> Result<(), GraphError> {
        if self.pending.is_some() {
            return Err(GraphError::TransactionInProgress);
        }
        if !self.undo.is_empty() || !self.redo.is_empty() {
            self.undo.clear();
            self.redo.clear();
            self.document_revision = next_document_revision();
        }
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
        self.check_document_size(&entry.before.graph, &self.viewport)?;
        let entry = self.undo.pop().expect("checked undo entry");
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
        self.check_document_size(&entry.after.graph, &self.viewport)?;
        let entry = self.redo.pop().expect("checked redo entry");
        self.restore(entry.after.clone());
        self.undo.push(entry);
        let excess = self.undo.len().saturating_sub(self.history_limit);
        if excess > 0 {
            self.undo.drain(..excess);
        }
        Ok(true)
    }
    pub fn select_all(&mut self) {
        self.selection = Selection {
            nodes: self.graph.nodes.keys().cloned().collect(),
            edges: self.graph.edges.keys().cloned().collect(),
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
        for node in self.graph.nodes.values() {
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
        self.graph
            .node_bounds(&self.selection.expanded_nodes(&self.graph))
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
        let nodes = self.selection.expanded_nodes(&self.graph);
        Clipboard {
            nodes: nodes
                .iter()
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
        for original in &clipboard.nodes {
            let mut node = original.clone();
            let id = NodeId::from(graph.fresh_id("node"));
            mapping.insert(original.id.clone(), id.clone());
            node.id = id.clone();
            node.position += offset;
            graph.add_node(node)?;
            selection.nodes.insert(id);
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
        self.check_document_size(&graph, &self.viewport)?;
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
        self.check_document_size(&candidate, &self.viewport)?;
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
        self.fit_viewport(self.graph.bounds(), canvas, padding)
    }
    pub fn fit_to_selection(&mut self, canvas: Size, padding: f32) -> bool {
        self.fit_viewport(self.selection_bounds(), canvas, padding)
            .unwrap_or(false)
    }
    fn fit_viewport(
        &mut self,
        bounds: Option<Rect>,
        canvas: Size,
        padding: f32,
    ) -> Result<bool, GraphError> {
        let mut viewport = self.viewport;
        let fitted = bounds.is_some_and(|bounds| viewport.fit(bounds, canvas, padding));
        if fitted {
            self.set_viewport(viewport)?;
        }
        Ok(fitted)
    }
    /// Build renderer-independent minimap geometry, including the visible camera.
    pub fn minimap(&self, canvas: Size, map_bounds: Rect, padding: f32) -> Option<MinimapData> {
        if !canvas.is_positive() || !self.viewport.is_valid() {
            return None;
        }
        let visible = self.viewport.visible_world_rect(canvas);
        let world = self
            .graph
            .bounds()
            .map_or(visible, |bounds| bounds.union(visible));
        let transform = MinimapTransform::new(world, map_bounds, padding)?;
        Some(MinimapData {
            transform,
            nodes: self
                .graph
                .nodes
                .values()
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

//! Node stacking order stored independently of graph topology and public fields.
//!
//! `data.flow_presentation.node_order` is an array of node IDs, back to front.
//! Queries tolerate older and malformed documents without changing them. Writes
//! reject malformed metadata instead of replacing application-owned data.

use crate::{Graph, GraphError, NodeId, ValidationIssue};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

const PRESENTATION_KEY: &str = "flow_presentation";
const ORDER_KEY: &str = "node_order";

/// A node stack operation. Selected nodes retain their relative order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeOrderAction {
    BringToFront,
    BringForward,
    SendBackward,
    SendToBack,
}

/// Returns every live node once, from back to front, without modifying `graph`.
///
/// Missing or malformed metadata falls back to the graph's `BTreeMap` ID order.
/// Valid stored orders discard duplicates and deleted IDs, then append unlisted
/// nodes at the front in deterministic ID order.
pub fn ordered_node_ids(graph: &Graph) -> Vec<NodeId> {
    normalize_node_order(
        graph,
        stored_node_order(graph).ok().flatten().unwrap_or_default(),
    )
}

/// Normalizes a proposed back-to-front order without modifying the graph.
///
/// Keeps the first occurrence of each live ID and appends any omitted live IDs
/// in deterministic ID order. This does not validate existing metadata.
pub fn normalize_node_order(graph: &Graph, order: impl IntoIterator<Item = NodeId>) -> Vec<NodeId> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::with_capacity(graph.nodes.len());
    for id in order {
        if graph.nodes.contains_key(&id) && seen.insert(id.clone()) {
            normalized.push(id);
        }
    }
    for id in graph.nodes.keys() {
        if seen.insert(id.clone()) {
            normalized.push(id.clone());
        }
    }
    normalized
}

/// Atomically writes a normalized order, preserving unrelated graph metadata.
///
/// `data: null` is the legacy absence of metadata and may become an object.
/// Other non-object data, an existing non-object namespace, or a non-string-array
/// order returns `GraphError::Validation` and leaves the entire graph unchanged.
/// Returns whether metadata changed. This low-level helper does not enforce
/// locks; user reorder commands should use [`reorder_nodes`].
pub fn write_node_order(
    graph: &mut Graph,
    order: impl IntoIterator<Item = NodeId>,
) -> Result<bool, GraphError> {
    stored_node_order(graph)?;
    let order = normalize_node_order(graph, order);
    let mut data = match &graph.data {
        Value::Null => Map::new(),
        Value::Object(data) => data.clone(),
        _ => unreachable!("metadata was validated before constructing the write"),
    };
    let mut presentation = data
        .get(PRESENTATION_KEY)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    presentation.insert(
        ORDER_KEY.into(),
        Value::Array(order.into_iter().map(|id| Value::String(id.0)).collect()),
    );
    data.insert(PRESENTATION_KEY.into(), Value::Object(presentation));
    let data = Value::Object(data);
    if graph.data == data {
        return Ok(false);
    }
    graph.data = data;
    Ok(true)
}

/// Applies a stack operation atomically; returns whether the order changed.
///
/// Invalid targets are ignored. Any live target locked directly or through its
/// containing group rejects the entire operation. Other locked nodes may be
/// crossed, just as unselected nodes may be crossed.
///
/// One-step moves shift each selected run across one adjacent unselected node.
/// Both selected and unselected nodes retain their respective relative order.
/// A boundary or empty-selection no-op does not create or normalize metadata.
/// The caller owns editor transactions and undo history.
pub fn reorder_nodes(
    graph: &mut Graph,
    targets: &BTreeSet<NodeId>,
    action: NodeOrderAction,
) -> Result<bool, GraphError> {
    stored_node_order(graph)?;
    let targets: BTreeSet<_> = targets
        .iter()
        .filter(|id| graph.nodes.contains_key(*id))
        .cloned()
        .collect();
    for id in &targets {
        if graph.nodes[id].locked {
            return Err(GraphError::Locked(id.to_string()));
        }
    }
    for group in graph.groups.values() {
        if group.locked && !group.node_ids.is_disjoint(&targets) {
            return Err(GraphError::Locked(group.id.to_string()));
        }
    }
    if targets.is_empty() {
        return Ok(false);
    }
    let before = ordered_node_ids(graph);
    let mut order = before.clone();
    match action {
        NodeOrderAction::BringToFront => order.sort_by_key(|id| targets.contains(id)),
        NodeOrderAction::SendToBack => order.sort_by_key(|id| !targets.contains(id)),
        NodeOrderAction::BringForward => {
            for index in (0..order.len().saturating_sub(1)).rev() {
                if targets.contains(&order[index]) && !targets.contains(&order[index + 1]) {
                    order.swap(index, index + 1);
                }
            }
        }
        NodeOrderAction::SendBackward => {
            for index in 1..order.len() {
                if targets.contains(&order[index]) && !targets.contains(&order[index - 1]) {
                    order.swap(index, index - 1);
                }
            }
        }
    }
    if order == before {
        return Ok(false);
    }
    write_node_order(graph, order)
}

/// Appends a batch at the front in the supplied order after adding its nodes.
///
/// The existing order is normalized first, then all batch IDs are removed from
/// it and appended once in input order. Missing IDs are ignored. This prevents
/// newly inserted IDs from interleaving with existing nodes through ID sorting.
/// Like [`write_node_order`], this low-level insertion helper does not check
/// locks, so a pasted locked node can retain its lock and still be inserted.
pub fn append_nodes_to_front(
    graph: &mut Graph,
    ids: impl IntoIterator<Item = NodeId>,
) -> Result<bool, GraphError> {
    stored_node_order(graph)?;
    let mut seen = BTreeSet::new();
    let batch: Vec<_> = ids
        .into_iter()
        .filter(|id| graph.nodes.contains_key(id) && seen.insert(id.clone()))
        .collect();
    if batch.is_empty() {
        return Ok(false);
    }
    let mut order = ordered_node_ids(graph);
    order.retain(|id| !seen.contains(id));
    order.extend(batch);
    write_node_order(graph, order)
}

fn metadata_error(path: &str, message: &str) -> GraphError {
    GraphError::Validation(vec![ValidationIssue {
        path: path.into(),
        message: message.into(),
    }])
}

fn stored_node_order(graph: &Graph) -> Result<Option<Vec<NodeId>>, GraphError> {
    let data = match &graph.data {
        Value::Null => return Ok(None),
        Value::Object(data) => data,
        _ => return Err(metadata_error("data", "expected an object or null")),
    };
    let Some(presentation) = data.get(PRESENTATION_KEY) else {
        return Ok(None);
    };
    let presentation = presentation
        .as_object()
        .ok_or_else(|| metadata_error("data.flow_presentation", "expected an object"))?;
    let Some(order) = presentation.get(ORDER_KEY) else {
        return Ok(None);
    };
    let order = order.as_array().ok_or_else(|| {
        metadata_error(
            "data.flow_presentation.node_order",
            "expected an array of node ID strings",
        )
    })?;
    let mut ids = Vec::with_capacity(order.len());
    for (index, id) in order.iter().enumerate() {
        let id = id.as_str().ok_or_else(|| {
            metadata_error(
                &format!("data.flow_presentation.node_order[{index}]"),
                "expected a node ID string",
            )
        })?;
        ids.push(NodeId::from(id));
    }
    Ok(Some(ids))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Group, Node, Point, Rect, Size};
    use serde_json::json;

    fn ids(values: &[&str]) -> Vec<NodeId> {
        values.iter().map(|id| NodeId::from(*id)).collect()
    }

    fn graph(values: &[&str]) -> Graph {
        let mut graph = Graph::new();
        for id in values {
            graph.add_node(Node::new(*id, *id, Point::ZERO)).unwrap();
        }
        graph
    }

    fn with_order(values: &[&str]) -> Graph {
        let mut graph = graph(values);
        write_node_order(&mut graph, ids(values)).unwrap();
        graph
    }

    #[test]
    fn legacy_queries_keep_id_order_without_writing_metadata() {
        let mut graph = graph(&["z", "10", "2", "a"]);
        for data in [
            Value::Null,
            json!({}),
            json!({"flow_presentation": {"zoom": 2}}),
        ] {
            graph.data = data;
            let before = graph.clone();
            assert_eq!(ordered_node_ids(&graph), ids(&["10", "2", "a", "z"]));
            assert_eq!(graph, before);
        }
    }

    #[test]
    fn queries_remove_stale_duplicates_and_append_missing_nodes_at_front() {
        let mut graph = graph(&["a", "b", "c", "d"]);
        graph.data = json!({"flow_presentation": {"node_order": ["c", "deleted", "c", "a"]}});
        let before = graph.clone();
        assert_eq!(ordered_node_ids(&graph), ids(&["c", "a", "b", "d"]));
        assert_eq!(graph, before);
        graph.remove_node(&"c".into()).unwrap();
        assert_eq!(ordered_node_ids(&graph), ids(&["a", "b", "d"]));
        assert_eq!(
            normalize_node_order(&graph, ids(&["d", "d", "missing"])),
            ids(&["d", "a", "b"])
        );
    }

    #[test]
    fn arbitrary_id_multiselection_has_stable_stack_operations() {
        let targets = ids(&["a", "c", "missing"]).into_iter().collect();
        let cases = [
            (NodeOrderAction::BringToFront, ["z", "m", "b", "a", "c"]),
            (NodeOrderAction::BringForward, ["z", "m", "a", "b", "c"]),
            (NodeOrderAction::SendBackward, ["a", "z", "c", "m", "b"]),
            (NodeOrderAction::SendToBack, ["a", "c", "z", "m", "b"]),
        ];
        for (action, expected) in cases {
            let mut graph = with_order(&["z", "a", "m", "c", "b"]);
            assert!(reorder_nodes(&mut graph, &targets, action).unwrap());
            assert_eq!(ordered_node_ids(&graph), ids(&expected), "{action:?}");
        }
    }

    #[test]
    fn adjacent_selected_nodes_move_as_a_block_one_layer() {
        let targets = ids(&["a", "m"]).into_iter().collect();
        let mut forward = with_order(&["z", "a", "m", "c", "b"]);
        reorder_nodes(&mut forward, &targets, NodeOrderAction::BringForward).unwrap();
        assert_eq!(ordered_node_ids(&forward), ids(&["z", "c", "a", "m", "b"]));
        let mut backward = with_order(&["z", "a", "m", "c", "b"]);
        reorder_nodes(&mut backward, &targets, NodeOrderAction::SendBackward).unwrap();
        assert_eq!(ordered_node_ids(&backward), ids(&["a", "m", "z", "c", "b"]));
    }

    #[test]
    fn boundary_and_invalid_selection_no_ops_leave_metadata_absent() {
        let mut graph = graph(&["a", "b", "c"]);
        let before = graph.clone();
        for (action, targets) in [
            (NodeOrderAction::BringToFront, ids(&["c"])),
            (NodeOrderAction::BringForward, ids(&["b", "c"])),
            (NodeOrderAction::SendBackward, ids(&["a", "b"])),
            (NodeOrderAction::SendToBack, ids(&["a"])),
            (NodeOrderAction::BringToFront, ids(&["a", "b", "c"])),
            (NodeOrderAction::BringForward, ids(&["deleted"])),
            (NodeOrderAction::SendBackward, vec![]),
        ] {
            assert!(!reorder_nodes(&mut graph, &targets.into_iter().collect(), action).unwrap());
            assert_eq!(graph, before);
        }
    }

    #[test]
    fn locked_node_or_containing_group_rejects_the_whole_reorder() {
        for lock_group in [false, true] {
            let mut graph = with_order(&["c", "a", "b"]);
            if lock_group {
                let mut group = Group::new(
                    "group",
                    "Group",
                    Rect::new(Point::ZERO, Size::new(400.0, 300.0)),
                )
                .with_nodes(ids(&["a"]));
                group.locked = true;
                graph.add_group(group).unwrap();
            } else {
                graph.nodes.get_mut(&"a".into()).unwrap().locked = true;
            }
            let before = graph.clone();
            let targets = ids(&["a", "c"]).into_iter().collect();
            for action in [
                NodeOrderAction::BringToFront,
                NodeOrderAction::BringForward,
                NodeOrderAction::SendBackward,
                NodeOrderAction::SendToBack,
            ] {
                assert!(matches!(
                    reorder_nodes(&mut graph, &targets, action),
                    Err(GraphError::Locked(_))
                ));
                assert_eq!(graph, before);
            }
        }
    }

    #[test]
    fn malformed_metadata_reads_safely_and_all_writers_reject_atomically() {
        let malformed = [
            json!("application payload"),
            json!(true),
            json!(42),
            json!([]),
            json!({"flow_presentation": null}),
            json!({"flow_presentation": []}),
            json!({"flow_presentation": "owned by application"}),
            json!({"flow_presentation": {"node_order": null}}),
            json!({"flow_presentation": {"node_order": {}}}),
            json!({"flow_presentation": {"node_order": ["c", 1, "a"]}}),
        ];
        let mut graph = graph(&["c", "a", "b"]);
        for data in malformed {
            graph.data = data;
            let before = graph.clone();
            assert_eq!(ordered_node_ids(&graph), ids(&["a", "b", "c"]));
            assert!(matches!(
                write_node_order(&mut graph, ids(&["c", "b", "a"])),
                Err(GraphError::Validation(_))
            ));
            assert_eq!(graph, before);
            assert!(matches!(
                reorder_nodes(
                    &mut graph,
                    &ids(&["a"]).into_iter().collect(),
                    NodeOrderAction::BringToFront
                ),
                Err(GraphError::Validation(_))
            ));
            assert_eq!(graph, before);
            assert!(matches!(
                append_nodes_to_front(&mut graph, ids(&["a"])),
                Err(GraphError::Validation(_))
            ));
            assert_eq!(graph, before);
        }
    }

    #[test]
    fn order_writes_preserve_unrelated_metadata_and_normalize_once() {
        let mut graph = graph(&["a", "b", "c"]);
        graph.data = json!({
            "application": {"nested": [1, true, "keep"]},
            "flow_presentation": {"show_grid": false, "node_order": ["c", "deleted", "c"]}
        });
        assert!(write_node_order(&mut graph, ids(&["c", "deleted", "c"])).unwrap());
        assert_eq!(
            graph.data,
            json!({
                "application": {"nested": [1, true, "keep"]},
                "flow_presentation": {"show_grid": false, "node_order": ["c", "a", "b"]}
            })
        );
        assert!(!write_node_order(&mut graph, ids(&["c", "a", "b"])).unwrap());
    }

    #[test]
    fn appended_batch_keeps_input_order_even_when_new_ids_sort_before_existing() {
        for use_metadata in [false, true] {
            let mut graph = graph(&["m", "z"]);
            if use_metadata {
                write_node_order(&mut graph, ids(&["z", "m"])).unwrap();
            }
            for id in ["a", "b"] {
                let mut node = Node::new(id, id, Point::ZERO);
                node.locked = true;
                graph.add_node(node).unwrap();
            }
            assert!(append_nodes_to_front(&mut graph, ids(&["b", "a", "b", "missing"])).unwrap());
            let expected = if use_metadata {
                ["z", "m", "b", "a"]
            } else {
                ["m", "z", "b", "a"]
            };
            assert_eq!(ordered_node_ids(&graph), ids(&expected));
            assert!(!append_nodes_to_front(&mut graph, ids(&["b", "a"])).unwrap());
        }
    }

    #[test]
    fn graph_serialization_restores_order_without_changing_schema_or_nodes() {
        let mut graph = with_order(&["z", "a", "m"]);
        graph.data["custom"] = json!({"keep": true});
        let json = graph.to_json().unwrap();
        let restored = Graph::from_json(&json).unwrap();
        assert_eq!(restored, graph);
        assert_eq!(ordered_node_ids(&restored), ids(&["z", "a", "m"]));
    }
}

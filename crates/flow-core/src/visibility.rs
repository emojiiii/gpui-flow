//! Optional canvas presentation visibility. Hidden objects remain graph nodes:
//! connections, group membership and runtime inputs are not removed.
use crate::{Edge, Graph, GraphError, NodeId, Rect, ValidationIssue};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

fn invalid(path: &str) -> GraphError {
    GraphError::Validation(vec![ValidationIssue {
        path: path.into(),
        message: "expected object metadata and an array of hidden node ID strings".into(),
    }])
}

fn stored_hidden(graph: &Graph) -> Result<BTreeSet<NodeId>, GraphError> {
    let object = match &graph.data {
        Value::Null => return Ok(BTreeSet::new()),
        Value::Object(v) => v,
        _ => return Err(invalid("data")),
    };
    let Some(presentation) = object.get("flow_presentation") else {
        return Ok(BTreeSet::new());
    };
    let presentation = presentation
        .as_object()
        .ok_or_else(|| invalid("data.flow_presentation"))?;
    let Some(ids) = presentation.get("hidden_nodes") else {
        return Ok(BTreeSet::new());
    };
    let ids = ids
        .as_array()
        .ok_or_else(|| invalid("data.flow_presentation.hidden_nodes"))?;
    let mut hidden = BTreeSet::new();
    for value in ids {
        let id = NodeId::from(
            value
                .as_str()
                .ok_or_else(|| invalid("data.flow_presentation.hidden_nodes"))?,
        );
        if graph.nodes.contains_key(&id) {
            hidden.insert(id);
        }
    }
    Ok(hidden)
}

/// Malformed optional visibility falls back to showing existing nodes. Queries
/// never repair or overwrite document metadata; explicit writes report errors.
pub fn hidden_node_ids(graph: &Graph) -> BTreeSet<NodeId> {
    stored_hidden(graph).unwrap_or_default()
}

pub fn node_is_visible(graph: &Graph, id: &NodeId) -> bool {
    graph.nodes.contains_key(id) && !hidden_node_ids(graph).contains(id)
}

pub fn edge_is_visible(graph: &Graph, edge: &Edge) -> bool {
    let hidden = hidden_node_ids(graph);
    graph.nodes.contains_key(&edge.source.node)
        && graph.nodes.contains_key(&edge.target.node)
        && !hidden.contains(&edge.source.node)
        && !hidden.contains(&edge.target.node)
}

/// Frames retain their own visible rectangle independently of child visibility.
/// Graph::bounds remains the complete document geometry, including hidden data.
pub fn visible_graph_bounds(graph: &Graph) -> Option<Rect> {
    let hidden = hidden_node_ids(graph);
    graph
        .nodes
        .values()
        .filter(|n| !hidden.contains(&n.id))
        .map(|n| n.bounds())
        .chain(graph.groups.values().map(|g| g.bounds))
        .reduce(Rect::union)
}

/// Atomically changes only the optional presentation metadata. A lock protects
/// content and geometry; it does not prevent hiding or showing the layer.
pub fn set_node_visible(graph: &mut Graph, id: &NodeId, visible: bool) -> Result<bool, GraphError> {
    if !graph.nodes.contains_key(id) {
        return Err(GraphError::MissingNode(id.clone()));
    }
    let mut hidden = stored_hidden(graph)?;
    let changed = if visible {
        hidden.remove(id)
    } else {
        hidden.insert(id.clone())
    };
    if !changed {
        return Ok(false);
    }
    let mut data = match &graph.data {
        Value::Object(v) => v.clone(),
        Value::Null => Map::new(),
        _ => unreachable!(),
    };
    let mut presentation = data
        .get("flow_presentation")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    presentation.insert(
        "hidden_nodes".into(),
        Value::Array(hidden.into_iter().map(|id| Value::String(id.0)).collect()),
    );
    data.insert("flow_presentation".into(), Value::Object(presentation));
    graph.data = Value::Object(data);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DataType, Node, Point, Port, PortRef};
    use serde_json::json;

    fn graph() -> Graph {
        let mut g = Graph::new();
        g.add_node(Node::new("a", "A", Point::ZERO).with_port(Port::output(
            "out",
            "Image",
            DataType::Number,
        )))
        .unwrap();
        g.add_node(
            Node::new("b", "B", Point::new(500., 0.)).with_port(Port::input(
                "in",
                "Image",
                DataType::Number,
            )),
        )
        .unwrap();
        g.connect("edge", PortRef::new("a", "out"), PortRef::new("b", "in"))
            .unwrap();
        g
    }

    #[test]
    fn visibility_preserves_topology_and_other_presentation_metadata() {
        let mut graph = graph();
        graph.data = json!({"host": 42, "flow_presentation": {"node_order": ["b", "a"]}});
        let nodes = graph.nodes.clone();
        let edges = graph.edges.clone();
        set_node_visible(&mut graph, &"b".into(), false).unwrap();
        assert!(!node_is_visible(&graph, &"b".into()));
        assert!(!edge_is_visible(&graph, &graph.edges[&"edge".into()]));
        assert_eq!(graph.nodes, nodes);
        assert_eq!(graph.edges, edges);
        assert_eq!(graph.data["host"], 42);
        assert_eq!(
            graph.data["flow_presentation"]["node_order"],
            json!(["b", "a"])
        );
        assert!(
            graph.bounds().unwrap().size.width > visible_graph_bounds(&graph).unwrap().size.width
        );
        let restored = Graph::from_json(&graph.to_json().unwrap()).unwrap();
        assert!(!node_is_visible(&restored, &"b".into()));
        set_node_visible(&mut graph, &"b".into(), true).unwrap();
        assert!(edge_is_visible(&graph, &graph.edges[&"edge".into()]));
    }

    #[test]
    fn malformed_writes_are_atomic_and_queries_fall_back_to_visible() {
        for data in [
            json!(12),
            json!({"flow_presentation": []}),
            json!({"flow_presentation":{"hidden_nodes":["a",false]}}),
        ] {
            let mut graph = graph();
            graph.data = data;
            let before = graph.clone();
            assert!(node_is_visible(&graph, &"a".into()));
            assert!(set_node_visible(&mut graph, &"a".into(), false).is_err());
            assert_eq!(graph, before);
        }
    }
}

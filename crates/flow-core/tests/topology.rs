//! Public graph contracts for the private petgraph reachability adapter.
use gpui_flow_core::{DataType, Edge, Graph, GraphError, Node, Point, Port, PortRef};
use serde_json::json;

fn node(id: &str) -> Node {
    Node::new(id, id, Point::ZERO)
        .with_port(Port::input("in", "Input", DataType::Number).with_capacity(None))
        .with_port(Port::output("out", "Output", DataType::Number))
}

fn graph(ids: &[&str]) -> Graph {
    let mut graph = Graph::new();
    graph.options.allow_cycles = false;
    for id in ids {
        graph.add_node(node(id)).unwrap();
    }
    graph
}

fn edge(id: &str, source: &str, target: &str) -> Edge {
    Edge::new(id, PortRef::new(source, "out"), PortRef::new(target, "in"))
}

#[test]
fn reachability_matches_transitive_closure_for_every_three_node_topology() {
    let ids = ["a", "b", "c"];
    let pairs = [(0, 1), (0, 2), (1, 0), (1, 2), (2, 0), (2, 1)];
    for mask in 0..1_usize << pairs.len() {
        let mut graph = graph(&ids);
        graph.options.allow_self_connections = true;
        let mut reachable = [[false; 3]; 3];
        for (index, &(source, target)) in pairs.iter().enumerate() {
            if mask & (1 << index) != 0 {
                let edge = edge(&format!("edge-{index}"), ids[source], ids[target]);
                // Public fields permit imports with cycles; query behavior still applies.
                graph.edges.insert(edge.id.clone(), edge);
                reachable[source][target] = true;
            }
        }
        for (index, row) in reachable.iter_mut().enumerate() {
            row[index] = true;
        }
        for intermediate in [0, 1, 2] {
            let through = reachable[intermediate];
            for row in &mut reachable {
                if row[intermediate] {
                    for (cell, connected) in row.iter_mut().zip(through) {
                        *cell |= connected;
                    }
                }
            }
        }
        for (source, source_id) in ids.iter().enumerate() {
            for (target, target_id) in ids.iter().enumerate() {
                let duplicate = pairs
                    .iter()
                    .enumerate()
                    .any(|(index, pair)| *pair == (source, target) && mask & (1 << index) != 0);
                let expected = if duplicate {
                    Err(GraphError::InvalidConnection("duplicate connection".into()))
                } else if reachable[target][source] {
                    Err(GraphError::InvalidConnection(
                        "connection would create a cycle".into(),
                    ))
                } else {
                    Ok(())
                };
                assert_eq!(
                    graph.validate_connection(
                        &PortRef::new(*source_id, "out"),
                        &PortRef::new(*target_id, "in"),
                    ),
                    expected,
                    "mask={mask}, {source_id}->{target_id}"
                );
            }
        }
    }
}

#[test]
fn parallel_port_edges_and_disconnected_nodes_keep_document_identity() {
    let mut graph = graph(&["source", "sink", "isolated"]);
    graph
        .nodes
        .get_mut(&"source".into())
        .unwrap()
        .ports
        .push(Port::output("other", "Other", DataType::Number));
    graph.add_edge(edge("z-first", "source", "sink")).unwrap();
    graph
        .add_edge(Edge::new(
            "a-second",
            PortRef::new("source", "other"),
            PortRef::new("sink", "in"),
        ))
        .unwrap();
    graph.validate().unwrap();
    let serialized = graph.to_json().unwrap();
    let value: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            "data",
            "edges",
            "groups",
            "nodes",
            "options",
            "schema_version"
        ])
    );
    assert_eq!(
        value["edges"]["a-second"]["source"]["node"],
        json!("source")
    );
    assert_eq!(value["nodes"]["isolated"]["id"], json!("isolated"));
    assert_eq!(Graph::from_json(&serialized).unwrap(), graph);
    assert_eq!(graph.connection_count(&PortRef::new("sink", "in")), 2);
    assert_eq!(
        graph.add_edge(edge("back", "sink", "source")),
        Err(GraphError::InvalidConnection(
            "connection would create a cycle".into()
        ))
    );
    graph.remove_node(&"source".into()).unwrap();
    graph.add_node(node("source")).unwrap();
    graph.add_edge(edge("reused", "sink", "source")).unwrap();
    graph.validate().unwrap();
}

#[test]
fn cycles_retain_per_edge_diagnostic_paths_and_connection_error_precedence() {
    let mut graph = graph(&["a", "b", "c"]);
    graph.options.allow_cycles = true;
    for edge in [
        edge("ab", "a", "b"),
        edge("bc", "b", "c"),
        edge("ca", "c", "a"),
    ] {
        graph.add_edge(edge).unwrap();
    }
    graph.options.allow_cycles = false;
    let issues = graph.validation_issues();
    assert_eq!(
        issues
            .iter()
            .map(|issue| issue.path.as_str())
            .collect::<Vec<_>>(),
        vec!["edges.ab", "edges.bc", "edges.ca"]
    );
    assert!(issues
        .iter()
        .all(|issue| { issue.message == "invalid connection: connection would create a cycle" }));
    assert_eq!(
        graph.add_edge(edge("duplicate", "a", "b")),
        Err(GraphError::InvalidConnection("duplicate connection".into()))
    );
    graph.edges.clear();
    graph.add_edge(edge("ab", "a", "b")).unwrap();
    graph.nodes.get_mut(&"a".into()).unwrap().ports[0].max_connections = Some(0);
    assert_eq!(
        graph.add_edge(edge("ba", "b", "a")),
        Err(GraphError::InvalidConnection(
            "port connection capacity reached".into()
        ))
    );
    graph.nodes.get_mut(&"a".into()).unwrap().ports[0].data_type = DataType::String;
    assert_eq!(
        graph.add_edge(edge("ba", "b", "a")),
        Err(GraphError::InvalidConnection(
            "port types are incompatible".into()
        ))
    );
}

#[test]
fn dangling_endpoints_in_public_imports_do_not_panic_or_hide_reachable_paths() {
    let mut graph = graph(&["a", "b"]);
    for edge in [
        edge("a-missing", "a", "missing"),
        edge("missing-b", "missing", "b"),
    ] {
        graph.edges.insert(edge.id.clone(), edge);
    }
    assert_eq!(
        graph.validate_connection(&PortRef::new("b", "out"), &PortRef::new("a", "in")),
        Err(GraphError::InvalidConnection(
            "connection would create a cycle".into()
        ))
    );
    let issues = graph.validation_issues();
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].path, "edges.a-missing");
    assert_eq!(issues[0].message, "port not found: missing.in");
    assert_eq!(issues[1].path, "edges.missing-b");
    assert_eq!(issues[1].message, "port not found: missing.out");
}

#[test]
fn self_connection_policy_precedes_reflexive_cycle_rejection() {
    let mut graph = graph(&["a"]);
    let before = graph.clone();
    assert_eq!(
        graph.add_edge(edge("self", "a", "a")),
        Err(GraphError::InvalidConnection(
            "self-connections are disabled".into()
        ))
    );
    assert_eq!(graph, before);
    graph.options.allow_self_connections = true;
    assert_eq!(
        graph.add_edge(edge("self", "a", "a")),
        Err(GraphError::InvalidConnection(
            "connection would create a cycle".into()
        ))
    );
    graph.options.allow_cycles = true;
    graph.add_edge(edge("self", "a", "a")).unwrap();
    graph.validate().unwrap();
}

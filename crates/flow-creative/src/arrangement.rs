//! Pure object arrangement over the existing graph geometry.
//!
//! Frames are core groups and are not accepted as arrangement targets. A selected
//! member may move inside or outside an unlocked frame; its frame bounds and
//! membership stay unchanged. Apply the returned positions in one editor
//! transaction (for example, `CanvasCommand::SetPositions`) to retain undo/redo.

use gpui_flow_core::{
    Graph, GraphError, Node, NodeId, Point, Rect, ValidationIssue, hidden_node_ids,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrangement {
    AlignLeft,
    AlignCenter,
    AlignRight,
    AlignTop,
    AlignMiddle,
    AlignBottom,
    DistributeHorizontal,
    DistributeVertical,
}

/// Return only positions that change; an empty map is a successful no-op.
///
/// Alignment needs at least two distinct nodes and uses the selected objects'
/// bounding rectangle. Distribution needs at least three nodes, orders them by
/// axis origin then stable node ID, and keeps the first and last nodes fixed.
/// Gaps are measured between rectangle edges, accounting for different sizes.
/// Negative gaps are rejected: move the outer objects farther apart first.
///
/// Missing, hidden, individually locked, or locked-frame members reject the
/// entire operation, including anchors that would otherwise stay in place.
/// Geometry is computed in f64, checked before conversion to f32, and the
/// complete candidate graph is validated. The input graph is never modified.
pub fn arrange_nodes(
    graph: &Graph,
    selected: &BTreeSet<NodeId>,
    arrangement: Arrangement,
) -> Result<BTreeMap<NodeId, Point>, GraphError> {
    let distribution = matches!(
        arrangement,
        Arrangement::DistributeHorizontal | Arrangement::DistributeVertical
    );
    let minimum = if distribution { 3 } else { 2 };
    if selected.len() < minimum {
        return Err(GraphError::Validation(vec![ValidationIssue {
            path: "selection.nodes".into(),
            message: format!("select at least {minimum} objects to arrange"),
        }]));
    }

    let hidden = hidden_node_ids(graph);
    let mut nodes = Vec::with_capacity(selected.len());
    for id in selected {
        let node = graph
            .node(id)
            .ok_or_else(|| GraphError::MissingNode(id.clone()))?;
        if crate::layer_guard::node_is_locked(graph, id) {
            return Err(GraphError::Locked(format!(
                "{id}; unlock the object and its containing frame before arranging"
            )));
        }
        if hidden.contains(id) {
            return Err(GraphError::Validation(vec![ValidationIssue {
                path: format!("nodes.{id}"),
                message: "show the hidden object before arranging".into(),
            }]));
        }
        if !node.size.is_positive() || !node.bounds().is_valid() {
            return Err(GraphError::InvalidGeometry(format!(
                "object {id} must have finite bounds and a positive size"
            )));
        }
        nodes.push(node);
    }

    let positions = if distribution {
        distribute(&mut nodes, arrangement == Arrangement::DistributeHorizontal)?
    } else {
        align(&nodes, arrangement)?
    };

    // Reuse core validation, including aggregate bounds, instead of introducing
    // a second coordinate limit or accepting positions the editor would reject.
    if positions.is_empty() {
        graph.validate()?;
    } else {
        let mut candidate = graph.clone();
        for (id, position) in &positions {
            candidate
                .nodes
                .get_mut(id)
                .ok_or_else(|| GraphError::MissingNode(id.clone()))?
                .position = *position;
        }
        candidate.validate()?;
    }
    Ok(positions)
}

fn align(nodes: &[&Node], arrangement: Arrangement) -> Result<BTreeMap<NodeId, Point>, GraphError> {
    let mut left = f64::INFINITY;
    let mut top = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut bottom = f64::NEG_INFINITY;
    for node in nodes {
        left = left.min(f64::from(node.position.x));
        top = top.min(f64::from(node.position.y));
        right = right.max(f64::from(node.position.x) + f64::from(node.size.width));
        bottom = bottom.max(f64::from(node.position.y) + f64::from(node.size.height));
    }

    let mut positions = BTreeMap::new();
    for node in nodes {
        let mut x = f64::from(node.position.x);
        let mut y = f64::from(node.position.y);
        match arrangement {
            Arrangement::AlignLeft => x = left,
            Arrangement::AlignCenter => {
                x = left + (right - left - f64::from(node.size.width)) / 2.0
            }
            Arrangement::AlignRight => x = right - f64::from(node.size.width),
            Arrangement::AlignTop => y = top,
            Arrangement::AlignMiddle => {
                y = top + (bottom - top - f64::from(node.size.height)) / 2.0
            }
            Arrangement::AlignBottom => y = bottom - f64::from(node.size.height),
            Arrangement::DistributeHorizontal | Arrangement::DistributeVertical => unreachable!(),
        }
        insert_changed(&mut positions, node, x, y)?;
    }
    Ok(positions)
}

fn distribute(
    nodes: &mut [&Node],
    horizontal: bool,
) -> Result<BTreeMap<NodeId, Point>, GraphError> {
    let origin = |node: &Node| {
        f64::from(if horizontal {
            node.position.x
        } else {
            node.position.y
        })
    };
    let extent = |node: &Node| {
        f64::from(if horizontal {
            node.size.width
        } else {
            node.size.height
        })
    };
    nodes.sort_by(|a, b| {
        // Geometry is already finite. Treat -0 and +0 as the same origin.
        origin(a)
            .partial_cmp(&origin(b))
            .unwrap()
            .then_with(|| a.id.cmp(&b.id))
    });
    let first = nodes[0];
    let last = nodes[nodes.len() - 1];
    // The last object's extent cancels out. Avoid adding/subtracting it so
    // widely differing object sizes don't introduce unnecessary cancellation.
    let occupied: f64 = nodes[..nodes.len() - 1]
        .iter()
        .map(|node| extent(node))
        .sum();
    let free = origin(last) - origin(first) - occupied;
    if !free.is_finite() || free < 0.0 {
        return Err(GraphError::InvalidGeometry(
            "not enough space for equal gaps; move the outer objects farther apart".into(),
        ));
    }
    let gap = free / (nodes.len() - 1) as f64;
    let mut positions = BTreeMap::new();
    let mut cursor = origin(first) + extent(first) + gap;
    for node in &nodes[1..nodes.len() - 1] {
        let (x, y) = if horizontal {
            (cursor, f64::from(node.position.y))
        } else {
            (f64::from(node.position.x), cursor)
        };
        insert_changed(&mut positions, node, x, y)?;
        cursor += extent(node) + gap;
    }
    Ok(positions)
}

fn insert_changed(
    positions: &mut BTreeMap<NodeId, Point>,
    node: &Node,
    x: f64,
    y: f64,
) -> Result<(), GraphError> {
    let limit = f64::from(f32::MAX);
    if !x.is_finite() || !y.is_finite() || x.abs() > limit || y.abs() > limit {
        return Err(GraphError::InvalidGeometry(format!(
            "arranged position for object {} exceeds the supported coordinate range",
            node.id
        )));
    }
    let position = Point::new(x as f32, y as f32);
    if !Rect::new(position, node.size).is_valid() {
        return Err(GraphError::InvalidGeometry(format!(
            "arranged bounds for object {} exceed the supported coordinate range",
            node.id
        )));
    }
    if position != node.position {
        positions.insert(node.id.clone(), position);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_core::{DataType, Editor, Group, Port, PortRef, Size, set_node_visible};
    use gpui_flow_interaction::{CanvasCommand, PluginHost};
    use serde_json::json;

    fn ids(values: &[&str]) -> BTreeSet<NodeId> {
        values.iter().map(|id| NodeId::from(*id)).collect()
    }

    fn add(graph: &mut Graph, id: &str, x: f32, y: f32, width: f32, height: f32) {
        graph
            .add_node(Node::new(id, id, Point::new(x, y)).with_size(Size::new(width, height)))
            .unwrap();
    }

    fn sample() -> Graph {
        let mut graph = Graph::new();
        add(&mut graph, "a", -40.0, -20.0, 20.0, 40.0);
        add(&mut graph, "b", 30.0, 50.0, 60.0, 20.0);
        add(&mut graph, "c", 120.0, 110.0, 40.0, 80.0);
        graph
    }

    fn after(graph: &Graph, positions: &BTreeMap<NodeId, Point>, id: &str) -> Point {
        positions
            .get(&id.into())
            .copied()
            .unwrap_or_else(|| graph.nodes[&id.into()].position)
    }

    #[test]
    fn six_alignments_use_outer_bounds_and_keep_the_other_axis() {
        let mut graph = sample();
        add(&mut graph, "unselected", 1000.0, 1000.0, 500.0, 500.0);
        let selection = ids(&["c", "a", "b"]);
        let cases = [
            (Arrangement::AlignLeft, [-40.0, -40.0, -40.0], true),
            (Arrangement::AlignCenter, [50.0, 30.0, 40.0], true),
            (Arrangement::AlignRight, [140.0, 100.0, 120.0], true),
            (Arrangement::AlignTop, [-20.0, -20.0, -20.0], false),
            (Arrangement::AlignMiddle, [65.0, 75.0, 45.0], false),
            (Arrangement::AlignBottom, [150.0, 170.0, 110.0], false),
        ];
        for (operation, expected, horizontal) in cases {
            let positions = arrange_nodes(&graph, &selection, operation).unwrap();
            for (id, expected) in ["a", "b", "c"].into_iter().zip(expected) {
                let actual = after(&graph, &positions, id);
                let original = graph.nodes[&id.into()].position;
                if horizontal {
                    assert_eq!(
                        actual,
                        Point::new(expected, original.y),
                        "{operation:?}: {id}"
                    );
                } else {
                    assert_eq!(
                        actual,
                        Point::new(original.x, expected),
                        "{operation:?}: {id}"
                    );
                }
            }
        }
    }

    #[test]
    fn unequal_sizes_distribute_edge_gaps_with_fixed_anchors() {
        let graph = sample();
        for (operation, expected) in [
            (Arrangement::DistributeHorizontal, Point::new(20.0, 50.0)),
            (Arrangement::DistributeVertical, Point::new(30.0, 55.0)),
        ] {
            let positions = arrange_nodes(&graph, &ids(&["a", "b", "c"]), operation).unwrap();
            assert_eq!(positions, BTreeMap::from([("b".into(), expected)]));
        }
    }

    #[test]
    fn equal_origins_use_stable_ids_independent_of_insertion_and_selection_order() {
        let mut graph = Graph::new();
        for (id, x) in [("d", 100.0), ("c", 5.0), ("b", 5.0), ("a", -20.0)] {
            add(&mut graph, id, x, 0.0, 20.0, 20.0);
        }
        let forward = arrange_nodes(
            &graph,
            &ids(&["a", "b", "c", "d"]),
            Arrangement::DistributeHorizontal,
        )
        .unwrap();
        let reverse = arrange_nodes(
            &graph,
            &ids(&["d", "c", "b", "a"]),
            Arrangement::DistributeHorizontal,
        )
        .unwrap();
        assert_eq!(forward, reverse);
        assert_eq!(forward[&"b".into()], Point::new(20.0, 0.0));
        assert_eq!(forward[&"c".into()], Point::new(60.0, 0.0));
    }

    #[test]
    fn zero_gap_is_supported_but_negative_gap_rejects_the_whole_operation() {
        let mut graph = Graph::new();
        add(&mut graph, "a", -20.0, 0.0, 30.0, 20.0);
        add(&mut graph, "b", 0.0, 0.0, 40.0, 20.0);
        add(&mut graph, "c", 50.0, 0.0, 20.0, 20.0);
        let selection = ids(&["a", "b", "c"]);
        let positions =
            arrange_nodes(&graph, &selection, Arrangement::DistributeHorizontal).unwrap();
        assert_eq!(positions[&"b".into()], Point::new(10.0, 0.0));
        graph.nodes.get_mut(&"c".into()).unwrap().position.x = 49.0;
        let before = graph.clone();
        let error =
            arrange_nodes(&graph, &selection, Arrangement::DistributeHorizontal).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("move the outer objects farther apart")
        );
        assert_eq!(graph, before);
    }

    #[test]
    fn missing_hidden_and_locked_anchors_reject_without_partial_positions() {
        let selection = ids(&["a", "b", "c"]);
        for case in 0..4 {
            let mut graph = sample();
            match case {
                0 => {
                    graph.nodes.remove(&"a".into());
                }
                1 => {
                    set_node_visible(&mut graph, &"a".into(), false).unwrap();
                }
                2 => graph.nodes.get_mut(&"a".into()).unwrap().locked = true,
                _ => {
                    let mut frame = Group::new(
                        "frame",
                        "Frame",
                        Rect::new(Point::new(-50.0, -50.0), Size::new(300.0, 300.0)),
                    )
                    .with_nodes(ids(&["a"]));
                    frame.locked = true;
                    graph.add_group(frame).unwrap();
                }
            }
            let before = graph.clone();
            for operation in [Arrangement::AlignLeft, Arrangement::DistributeHorizontal] {
                let error = arrange_nodes(&graph, &selection, operation).unwrap_err();
                match case {
                    0 => assert_eq!(error, GraphError::MissingNode("a".into())),
                    1 => assert!(error.to_string().contains("show the hidden object")),
                    _ => assert!(matches!(error, GraphError::Locked(_))),
                }
            }
            assert_eq!(graph, before);
        }
    }

    #[test]
    fn too_few_distinct_objects_are_actionable_errors() {
        let graph = sample();
        for values in [&[][..], &["a"][..], &["a", "a"][..]] {
            let error = arrange_nodes(&graph, &ids(values), Arrangement::AlignLeft).unwrap_err();
            assert!(error.to_string().contains("at least 2 objects"));
        }
        for operation in [
            Arrangement::DistributeHorizontal,
            Arrangement::DistributeVertical,
        ] {
            let error = arrange_nodes(&graph, &ids(&["a", "b"]), operation).unwrap_err();
            assert!(error.to_string().contains("at least 3 objects"));
        }
    }

    #[test]
    fn already_aligned_and_distributed_objects_return_no_changes() {
        let mut graph = Graph::new();
        add(&mut graph, "a", -40.0, 10.0, 20.0, 20.0);
        add(&mut graph, "b", 0.0, 10.0, 60.0, 20.0);
        add(&mut graph, "c", 80.0, 10.0, 40.0, 20.0);
        for operation in [Arrangement::AlignTop, Arrangement::DistributeHorizontal] {
            assert!(
                arrange_nodes(&graph, &ids(&["a", "b", "c"]), operation)
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[test]
    fn invalid_geometry_and_overflowing_bounds_are_rejected() {
        for (position, size) in [
            (Point::new(f32::NAN, 0.0), Size::new(20.0, 20.0)),
            (Point::new(0.0, f32::INFINITY), Size::new(20.0, 20.0)),
            (Point::ZERO, Size::new(0.0, 20.0)),
            (Point::ZERO, Size::new(20.0, -1.0)),
            (Point::ZERO, Size::new(f32::INFINITY, 20.0)),
            (Point::new(f32::MAX, 0.0), Size::new(f32::MAX, 20.0)),
        ] {
            let mut graph = sample();
            let node = graph.nodes.get_mut(&"b".into()).unwrap();
            node.position = position;
            node.size = size;
            assert!(matches!(
                arrange_nodes(&graph, &ids(&["a", "b", "c"]), Arrangement::AlignCenter),
                Err(GraphError::InvalidGeometry(_))
            ));
        }
    }

    #[test]
    fn wide_intermediates_handle_large_finite_coordinates() {
        let mut graph = Graph::new();
        add(&mut graph, "a", 2.0e38, 0.0, 1.0e32, 20.0);
        add(&mut graph, "b", 3.0e38, 0.0, 1.0e32, 20.0);
        graph.validate().unwrap();
        let positions = arrange_nodes(&graph, &ids(&["a", "b"]), Arrangement::AlignCenter).unwrap();
        assert_eq!(positions.len(), 2);
        for position in positions.values() {
            assert!(position.is_finite());
            assert!((position.x / 2.5e38 - 1.0).abs() < 1.0e-6);
        }
    }

    #[test]
    fn unsafe_float_conversion_and_aggregate_bounds_are_rejected() {
        let node = Node::new("a", "A", Point::ZERO);
        for x in [f64::NAN, f64::INFINITY, f64::from(f32::MAX) * 2.0] {
            assert!(insert_changed(&mut BTreeMap::new(), &node, x, 0.0).is_err());
        }
        let mut graph = Graph::new();
        add(&mut graph, "a", -3.0e38, 0.0, 20.0, 20.0);
        add(&mut graph, "b", 3.0e38, 0.0, 20.0, 20.0);
        // A no-op still validates the graph's combined bounds.
        assert!(arrange_nodes(&graph, &ids(&["a", "b"]), Arrangement::AlignTop).is_err());
    }

    #[test]
    fn one_set_positions_transaction_preserves_content_frames_and_history() {
        let mut graph = sample();
        add(&mut graph, "unselected", 1000.0, 1000.0, 500.0, 500.0);
        graph
            .nodes
            .get_mut(&"a".into())
            .unwrap()
            .ports
            .push(Port::output("out", "Image", DataType::Number));
        graph
            .nodes
            .get_mut(&"b".into())
            .unwrap()
            .ports
            .push(Port::input("in", "Image", DataType::Number));
        graph
            .connect("edge", PortRef::new("a", "out"), PortRef::new("b", "in"))
            .unwrap();
        graph.nodes.get_mut(&"a".into()).unwrap().data =
            json!({"asset": "assets/example.png", "text": "keep me"});
        graph.data =
            json!({"host": "unchanged", "flow_presentation": {"node_order": ["c", "b", "a"]}});
        graph
            .add_group(
                Group::new(
                    "frame",
                    "Frame",
                    Rect::new(Point::new(-50.0, -50.0), Size::new(300.0, 300.0)),
                )
                .with_nodes(ids(&["a", "b"])),
            )
            .unwrap();
        let before = graph.clone();
        let positions =
            arrange_nodes(&graph, &ids(&["a", "b", "c"]), Arrangement::AlignRight).unwrap();
        assert_eq!(graph, before);
        let mut expected = before.clone();
        expected.nodes.get_mut(&"a".into()).unwrap().position.x = 140.0;
        expected.nodes.get_mut(&"b".into()).unwrap().position.x = 100.0;
        let mut editor = Editor::new(graph).unwrap();
        let mut host = PluginHost::with_defaults();
        host.execute(
            &mut editor,
            CanvasCommand::SetPositions {
                nodes: positions,
                groups: BTreeMap::new(),
            },
        )
        .unwrap();
        assert_eq!(editor.graph(), &expected);
        assert!(editor.undo());
        assert_eq!(editor.graph(), &before);
        assert!(!editor.undo());
        assert!(editor.redo());
        assert_eq!(editor.graph(), &expected);
        assert!(!editor.redo());
        let no_op = arrange_nodes(
            editor.graph(),
            &ids(&["a", "b", "c"]),
            Arrangement::AlignRight,
        )
        .unwrap();
        assert!(no_op.is_empty());
        host.execute(
            &mut editor,
            CanvasCommand::SetPositions {
                nodes: no_op,
                groups: BTreeMap::new(),
            },
        )
        .unwrap();
        assert!(editor.undo());
        assert_eq!(editor.graph(), &before);
        assert!(!editor.undo());
    }
}

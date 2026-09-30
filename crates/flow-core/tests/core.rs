use gpui_flow_core::*;
use serde_json::json;

fn node(id: &str, x: f32) -> Node {
    Node::new(id, id, Point::new(x, 20.0))
        .with_port(Port::input("in", "Input", DataType::Number))
        .with_port(Port::output("out", "Output", DataType::Number))
}
fn graph() -> Graph {
    let mut graph = Graph::new();
    graph.add_node(node("a", 10.0)).unwrap();
    graph.add_node(node("b", 300.0)).unwrap();
    graph.add_node(node("c", 600.0)).unwrap();
    graph
}
fn edge(id: &str, source: &str, target: &str) -> Edge {
    Edge::new(id, PortRef::new(source, "out"), PortRef::new(target, "in"))
}
fn editor() -> Editor {
    Editor::new(graph()).unwrap()
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.001, "{a} != {b}");
}
fn close_point(a: Point, b: Point) {
    close(a.x, b.x);
    close(a.y, b.y);
}

#[test]
fn ids_serialize_as_strings_and_graph_is_deterministic() {
    assert_eq!(serde_json::to_value(NodeId::from("a")).unwrap(), json!("a"));
    let graph = graph();
    assert_eq!(graph.to_json().unwrap(), graph.to_json().unwrap());
    assert_eq!(Graph::from_json(&graph.to_json().unwrap()).unwrap(), graph);
}
#[test]
fn typed_connections_validate_direction_type_and_capacity() {
    let mut graph = graph();
    assert!(graph
        .validate_connection(&PortRef::new("a", "in"), &PortRef::new("b", "out"))
        .is_err());
    graph.add_edge(edge("ab", "a", "b")).unwrap();
    assert!(graph.add_edge(edge("cb", "c", "b")).is_err());
    assert!(graph.add_edge(edge("ab2", "a", "b")).is_err());
    graph.add_edge(edge("ac", "a", "c")).unwrap(); // output fanout is unlimited
    assert_eq!(graph.connection_count(&PortRef::new("a", "out")), 2);
    graph.nodes.get_mut(&"c".into()).unwrap().ports[0].data_type = DataType::String;
    assert!(graph.validate().is_err());
}
#[test]
fn named_and_any_port_types() {
    assert!(DataType::Any.compatible_with(&DataType::Named("image".into())));
    assert!(DataType::Named("image".into()).compatible_with(&DataType::Named("image".into())));
    assert!(!DataType::Named("image".into()).compatible_with(&DataType::Named("tensor".into())));
}
#[test]
fn missing_and_self_connections_rejected() {
    let mut graph = graph();
    assert!(matches!(
        graph.add_edge(edge("missing", "a", "nope")),
        Err(GraphError::MissingPort(_))
    ));
    assert!(graph.add_edge(edge("self", "a", "a")).is_err());
    graph.options.allow_self_connections = true;
    graph.add_edge(edge("self", "a", "a")).unwrap();
    graph.validate().unwrap();
}
#[test]
fn zero_capacity_ports_reject_edges() {
    let mut graph = graph();
    graph.nodes.get_mut(&"a".into()).unwrap().ports[1].max_connections = Some(0);
    assert!(graph.add_edge(edge("ab", "a", "b")).is_err());
}
#[test]
fn optional_dag_policy_rejects_cycle_without_mutation() {
    let mut graph = graph();
    graph.options.allow_cycles = false;
    graph.add_edge(edge("ab", "a", "b")).unwrap();
    graph.add_edge(edge("bc", "b", "c")).unwrap();
    let before = graph.clone();
    assert!(graph.add_edge(edge("ca", "c", "a")).is_err());
    assert_eq!(graph, before);
    graph.options.allow_cycles = true;
    graph.add_edge(edge("ca", "c", "a")).unwrap();
    graph.validate().unwrap();
    graph.options.allow_cycles = false;
    assert!(graph.validate().is_err());
}
#[test]
fn invalid_nodes_and_duplicate_ids_rejected() {
    let mut graph = graph();
    assert!(graph.add_node(node("a", 1.0)).is_err());
    assert!(graph.add_node(node("invalid", f32::NAN)).is_err());
    assert!(graph
        .add_node(node("zero", 0.0).with_size(Size::new(0.0, 4.0)))
        .is_err());
    assert!(graph
        .add_node(node("duplicate-port", 0.0).with_port(Port::input(
            "in",
            "Duplicate",
            DataType::Any
        )))
        .is_err());
    assert!(graph.add_node(node(" ", 1.0)).is_err());
    assert!(graph
        .add_node(node("empty-type", 0.0).with_port(Port::input(
            "other",
            "Other",
            DataType::Named(" ".into())
        )))
        .is_err());
}
#[test]
fn port_anchors_are_on_expected_edges() {
    let node = node("a", 10.0);
    let input = node.port_position(&"in".into()).unwrap();
    let output = node.port_position(&"out".into()).unwrap();
    close(input.x, 10.0);
    close(output.x, 234.0);
    assert!(node.bounds().contains(input));
    assert!(node.bounds().contains(output));
    assert!(node.port_position(&"missing".into()).is_none());
}
#[test]
fn remove_node_cascades_edges_and_membership() {
    let mut graph = graph();
    graph.add_edge(edge("ab", "a", "b")).unwrap();
    graph
        .add_group(
            Group::new(
                "g",
                "Group",
                Rect::new(Point::ZERO, Size::new(600.0, 200.0)),
            )
            .with_nodes(vec!["a".into(), "b".into()]),
        )
        .unwrap();
    graph.remove_node(&"a".into()).unwrap();
    assert!(graph.edges.is_empty());
    assert!(!graph.groups[&"g".into()].node_ids.contains(&"a".into()));
    graph.validate().unwrap();
}
#[test]
fn groups_require_real_exclusively_owned_members() {
    let mut graph = graph();
    let group = Group::new(
        "g",
        "Group",
        Rect::new(Point::ZERO, Size::new(400.0, 200.0)),
    )
    .with_nodes(vec!["a".into()]);
    graph.add_group(group.clone()).unwrap();
    assert!(graph
        .add_group(Group {
            id: "g2".into(),
            ..group
        })
        .is_err());
    assert!(graph
        .add_group(
            Group::new("bad", "Bad", Rect::new(Point::ZERO, Size::new(1.0, 1.0)))
                .with_nodes(vec!["missing".into()])
        )
        .is_err());
    assert_eq!(graph.groups.len(), 1);
    graph.remove_group(&"g".into()).unwrap();
    assert_eq!(graph.nodes.len(), 3);
}
#[test]
fn invalid_imports_report_paths_and_versions() {
    let mut value = serde_json::to_value(graph()).unwrap();
    value["schema_version"] = json!(99);
    assert!(Graph::from_json(&value.to_string())
        .unwrap_err()
        .to_string()
        .contains("schema_version"));
    value["schema_version"] = json!(1);
    value["nodes"]["a"]["id"] = json!("different");
    assert!(Graph::from_json(&value.to_string())
        .unwrap_err()
        .to_string()
        .contains("map key"));
    value["nodes"]["a"]["id"] = json!("a");
    value["nodes"]["a"]["size"]["width"] = json!(-5);
    assert!(Graph::from_json(&value.to_string()).is_err());
}
#[test]
fn transaction_error_and_validation_failure_are_atomic() {
    let mut editor = editor();
    let before = editor.graph().clone();
    let result: Result<(), GraphError> = editor.transaction("bad", |g| {
        g.remove_node(&"a".into())?;
        Err(GraphError::InvalidConnection("no".into()))
    });
    assert!(result.is_err());
    assert_eq!(editor.graph(), &before);
    assert_eq!(editor.history_len(), 0);
    assert!(editor
        .transaction("invalid", |g| {
            g.nodes.get_mut(&"a".into()).unwrap().position.x = f32::NAN;
            Ok(())
        })
        .is_err());
    assert_eq!(editor.graph(), &before);
}
#[test]
fn transaction_panic_leaves_original_intact() {
    let mut editor = editor();
    let before = editor.graph().clone();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Result<(), GraphError> = editor.transaction("panic", |g| {
            g.nodes.clear();
            panic!("simulate callback panic");
        });
    }));
    assert!(caught.is_err());
    assert_eq!(editor.graph(), &before);
}
#[test]
fn undo_redo_restore_graph_and_selection() {
    let mut editor = editor();
    editor.selection.select_node("a", false);
    editor.delete_selection().unwrap();
    assert!(!editor.graph().nodes.contains_key(&"a".into()));
    assert!(editor.selection.is_empty());
    assert!(editor.undo());
    assert!(editor.graph().nodes.contains_key(&"a".into()));
    assert!(editor.selection.nodes.contains(&"a".into()));
    assert!(editor.redo());
    assert!(!editor.graph().nodes.contains_key(&"a".into()));
    assert!(editor.selection.is_empty());
}
#[test]
fn gesture_coalesces_many_updates() {
    let mut editor = editor();
    editor.selection.select_node("a", false);
    editor.begin_transaction("Drag node").unwrap();
    assert!(editor.begin_transaction("Nested").is_err());
    for _ in 0..20 {
        editor.move_selection(Point::new(1.0, 2.0)).unwrap();
    }
    assert_eq!(editor.history_len(), 0);
    assert!(!editor.undo());
    assert!(editor.commit_transaction().unwrap());
    assert_eq!(editor.history_len(), 1);
    assert_eq!(editor.undo_label(), Some("Drag node"));
    assert!(editor.undo());
    assert_eq!(
        editor.graph().nodes[&"a".into()].position,
        Point::new(10.0, 20.0)
    );
    assert!(editor.redo());
    assert_eq!(
        editor.graph().nodes[&"a".into()].position,
        Point::new(30.0, 60.0)
    );
}
#[test]
fn cancelling_gesture_restores_selection_and_preserves_redo() {
    let mut editor = editor();
    editor.selection.select_node("a", false);
    editor.move_selection(Point::new(10.0, 0.0)).unwrap();
    editor.undo();
    editor.begin_transaction("Cancelled").unwrap();
    editor.move_selection(Point::new(500.0, 0.0)).unwrap();
    editor.selection.clear();
    editor.cancel_transaction().unwrap();
    assert_eq!(editor.graph().nodes[&"a".into()].position.x, 10.0);
    assert!(editor.selection.nodes.contains(&"a".into()));
    assert!(editor.can_redo());
}
#[test]
fn noop_does_not_erase_redo_but_new_edit_does() {
    let mut editor = editor();
    editor.selection.select_node("a", false);
    editor.move_selection(Point::new(1.0, 0.0)).unwrap();
    editor.undo();
    editor.transaction("no-op", |_| Ok(())).unwrap();
    assert!(editor.can_redo());
    editor.move_selection(Point::new(3.0, 0.0)).unwrap();
    assert!(!editor.can_redo());
}
#[test]
fn history_is_bounded_and_zero_disables_it() {
    let mut editor = editor();
    editor.selection.select_node("a", false);
    editor.set_history_limit(2);
    for _ in 0..5 {
        editor.move_selection(Point::new(1.0, 0.0)).unwrap();
    }
    assert_eq!(editor.history_len(), 2);
    editor.undo();
    editor.undo();
    assert!(!editor.undo());
    assert_eq!(editor.graph().nodes[&"a".into()].position.x, 13.0);
    editor.set_history_limit(0);
    editor.move_selection(Point::new(1.0, 0.0)).unwrap();
    assert!(!editor.can_undo());
    assert!(!editor.can_redo());
}
#[test]
fn locked_move_rolls_back_other_selected_nodes() {
    let mut editor = editor();
    editor
        .transaction("lock", |g| {
            g.nodes.get_mut(&"b".into()).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
    editor.selection.select_node("a", false);
    editor.selection.select_node("b", true);
    let before = editor.graph().clone();
    assert!(matches!(
        editor.move_selection(Point::new(50.0, 0.0)),
        Err(GraphError::Locked(_))
    ));
    assert_eq!(editor.graph(), &before);
}
#[test]
fn moving_group_and_member_selection_moves_each_once() {
    let mut editor = editor();
    editor.selection.select_node("a", false);
    editor.selection.select_node("b", true);
    let group = editor.group_selection("Example", 20.0).unwrap();
    editor.selection.select_node("a", true);
    let original = editor.graph().groups[&group].bounds;
    editor.move_selection(Point::new(10.0, 30.0)).unwrap();
    assert_eq!(
        editor.graph().nodes[&"a".into()].position,
        Point::new(20.0, 50.0)
    );
    assert_eq!(
        editor.graph().nodes[&"b".into()].position,
        Point::new(310.0, 50.0)
    );
    assert_eq!(
        editor.graph().groups[&group].bounds,
        original.translated(Point::new(10.0, 30.0))
    );
}
#[test]
fn grouping_reparents_nodes_and_is_undoable() {
    let mut editor = editor();
    editor.selection.select_node("a", false);
    let first = editor.group_selection("First", 10.0).unwrap();
    editor.selection.select_node("a", false);
    editor.selection.select_node("b", true);
    let second = editor.group_selection("Second", 10.0).unwrap();
    assert!(editor.graph().groups[&first].node_ids.is_empty());
    assert_eq!(editor.graph().groups[&second].node_ids.len(), 2);
    editor.undo();
    assert_eq!(editor.graph().groups[&first].node_ids.len(), 1);
    assert!(!editor.graph().groups.contains_key(&second));
}
#[test]
fn deleting_group_preserves_its_unselected_nodes() {
    let mut editor = editor();
    editor.selection.select_node("a", false);
    let group = editor.group_selection("Group", 12.0).unwrap();
    editor.delete_selection().unwrap();
    assert!(!editor.graph().groups.contains_key(&group));
    assert!(editor.graph().nodes.contains_key(&"a".into()));
}
#[test]
fn clipboard_remaps_all_internal_references_and_metadata() {
    let mut editor = editor();
    editor
        .transaction("connect", |g| {
            g.nodes.get_mut(&"a".into()).unwrap().data = json!({"prompt":"hello"});
            g.add_edge(edge("ab", "a", "b"))?;
            g.add_edge(edge("bc", "b", "c"))
        })
        .unwrap();
    editor.selection.select_node("a", false);
    editor.selection.select_node("b", true);
    editor.group_selection("Copied group", 20.0).unwrap();
    let clipboard = editor.copy_selection();
    assert_eq!(clipboard.nodes.len(), 2);
    assert_eq!(clipboard.edges.len(), 1);
    assert_eq!(clipboard.groups.len(), 1);
    let clipboard = Clipboard::from_json(&clipboard.to_json().unwrap()).unwrap();
    let selected = editor.paste(&clipboard, Point::new(40.0, 60.0)).unwrap();
    assert_eq!(selected.nodes.len(), 2);
    assert_eq!(editor.graph().nodes.len(), 5);
    let new_edge = &editor.graph().edges[selected.edges.iter().next().unwrap()];
    assert!(selected.nodes.contains(&new_edge.source.node));
    assert!(selected.nodes.contains(&new_edge.target.node));
    let copied_a = &editor.graph().nodes[&new_edge.source.node];
    assert_eq!(copied_a.position, Point::new(50.0, 80.0));
    assert_eq!(copied_a.data, json!({"prompt":"hello"}));
    assert_eq!(
        editor.graph().groups[selected.groups.iter().next().unwrap()].node_ids,
        selected.nodes
    );
    editor.undo();
    assert_eq!(editor.graph().nodes.len(), 3);
    editor.redo();
    assert_eq!(editor.graph().nodes.len(), 5);
    editor.graph().validate().unwrap();
}
#[test]
fn malformed_clipboard_and_paste_overflow_are_atomic() {
    let mut editor = editor();
    let before = editor.graph().clone();
    let bad = Clipboard {
        nodes: vec![node("a", 0.0)],
        edges: vec![edge("bad", "a", "missing")],
        ..Clipboard::default()
    };
    assert!(editor.paste(&bad, Point::ZERO).is_err());
    assert_eq!(editor.graph(), &before);
    let bad = Clipboard {
        nodes: vec![node("a", 0.0), node("a", 0.0)],
        ..Clipboard::default()
    };
    assert!(bad.validate().is_err());
    let overflow = Clipboard {
        nodes: vec![node("far", f32::MAX / 1.5)],
        ..Clipboard::default()
    };
    assert!(editor
        .paste(&overflow, Point::new(f32::MAX / 1.5, 0.0))
        .is_err());
    assert_eq!(editor.graph(), &before);
}
#[test]
fn clipboard_cannot_bypass_destination_cycle_policy() {
    let mut cyclic = graph();
    cyclic.add_edge(edge("ab", "a", "b")).unwrap();
    cyclic.add_edge(edge("ba", "b", "a")).unwrap();
    let mut source = Editor::new(cyclic).unwrap();
    source.select_all();
    let clipboard = source.copy_selection();
    let mut target_graph = Graph::default();
    target_graph.options.allow_cycles = false;
    let mut target = Editor::new(target_graph).unwrap();
    assert!(target.paste(&clipboard, Point::ZERO).is_err());
    assert!(target.graph().nodes.is_empty());
}
#[test]
fn document_roundtrip_preserves_camera_but_not_history() {
    let mut editor = editor();
    editor.selection.select_node("a", false);
    editor.move_selection(Point::new(10.0, 0.0)).unwrap();
    editor.viewport.pan = Point::new(50.0, 30.0);
    editor.viewport.zoom = 1.5;
    let restored = Editor::from_json(&editor.to_json().unwrap()).unwrap();
    assert_eq!(restored.graph(), editor.graph());
    assert_eq!(restored.viewport, editor.viewport);
    assert!(restored.selection.is_empty());
    assert!(!restored.can_undo());
    let mut value = serde_json::to_value(editor.document()).unwrap();
    value["viewport"]["zoom"] = json!(0);
    assert!(FlowDocument::from_json(&value.to_string()).is_err());
}
#[test]
fn camera_transform_zoom_and_fit_preserve_expected_geometry() {
    let mut view = Viewport {
        pan: Point::new(37.0, -82.0),
        zoom: 0.75,
        ..Viewport::default()
    };
    let world = Point::new(320.0, -43.0);
    close_point(view.screen_to_world(view.world_to_screen(world)), world);
    let anchor = Point::new(300.0, 200.0);
    let under = view.screen_to_world(anchor);
    assert!(view.zoom_at(anchor, 1.75));
    close_point(view.screen_to_world(anchor), under);
    assert!(view.zoom_at(anchor, 100.0));
    assert_eq!(view.zoom, view.max_zoom);
    let bounds = Rect::new(Point::new(-200.0, -50.0), Size::new(600.0, 400.0));
    assert!(view.fit(bounds, Size::new(1000.0, 800.0), 50.0));
    close_point(
        view.world_to_screen(bounds.center()),
        Point::new(500.0, 400.0),
    );
    assert!(Rect::new(Point::new(50.0, 50.0), Size::new(900.0, 700.0))
        .contains_rect(view.world_rect_to_screen(bounds)));
}
#[test]
fn invalid_camera_and_minimap_inputs_do_not_mutate() {
    let mut view = Viewport::default();
    let before = view;
    assert!(!view.zoom_at(Point::ZERO, f32::NAN));
    assert!(!view.pan_by(Point::new(f32::INFINITY, 0.0)));
    assert!(!view.fit(Rect::default(), Size::new(100.0, 100.0), 51.0));
    assert_eq!(view, before);
    assert!(MinimapTransform::new(Rect::default(), Rect::default(), 0.0).is_none());
}
#[test]
fn minimap_is_aspect_correct_and_invertible() {
    let editor = editor();
    let map = Rect::new(Point::new(5.0, 10.0), Size::new(200.0, 150.0));
    let data = editor.minimap(Size::new(1000.0, 700.0), map, 8.0).unwrap();
    assert_eq!(data.nodes.len(), 3);
    assert!(map.contains_rect(data.viewport));
    let p = Point::new(40.0, 25.0);
    close_point(
        data.transform.map_to_world(data.transform.world_to_map(p)),
        p,
    );
    for (_, rect) in data.nodes {
        assert!(map.contains_rect(rect));
    }
}
#[test]
fn marquee_handles_reverse_drags_and_full_containment() {
    let mut editor = editor();
    let rect = Rect::from_points(Point::new(250.0, 200.0), Point::ZERO);
    editor.select_rect(rect, false, true);
    assert_eq!(editor.selection.nodes.len(), 1);
    assert!(editor.selection.nodes.contains(&"a".into()));
    editor.select_rect(
        Rect::new(Point::new(280.0, 0.0), Size::new(30.0, 100.0)),
        false,
        true,
    );
    assert!(editor.selection.is_empty());
    editor.select_rect(
        Rect::new(Point::new(280.0, 0.0), Size::new(30.0, 100.0)),
        false,
        false,
    );
    assert!(editor.selection.nodes.contains(&"b".into()));
}
#[test]
fn node_resize_rejects_invalid_dimensions_and_undoes() {
    let mut editor = editor();
    assert!(editor
        .resize_node(&"a".into(), Size::new(-1.0, 200.0))
        .is_err());
    editor
        .resize_node(&"a".into(), Size::new(400.0, 300.0))
        .unwrap();
    assert_eq!(
        editor.graph().nodes[&"a".into()].size,
        Size::new(400.0, 300.0)
    );
    editor.undo();
    assert_eq!(
        editor.graph().nodes[&"a".into()].size,
        Size::new(224.0, 144.0)
    );
}
#[test]
fn fresh_ids_do_not_collide_across_item_kinds() {
    let mut graph = graph();
    graph.add_node(node("edge-1", 0.0)).unwrap();
    assert_eq!(graph.fresh_id("edge"), "edge-2");
}

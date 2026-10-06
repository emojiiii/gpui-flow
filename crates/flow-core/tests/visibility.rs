use gpui_flow_core::*;

fn graph() -> Graph {
    let mut graph = Graph::new();
    for (id, x) in [("a", 0.), ("b", 50_000.)] {
        graph
            .add_node(
                Node::new(id, id, Point::new(x, 0.))
                    .with_port(Port::input("in", "Input", DataType::Number))
                    .with_port(Port::output("out", "Output", DataType::Number)),
            )
            .unwrap();
    }
    graph
        .connect("wire", PortRef::new("a", "out"), PortRef::new("b", "in"))
        .unwrap();
    graph
}

#[test]
fn hiding_clears_direct_selection_and_history_restores_it_without_removing_edges() {
    let mut editor = Editor::new(graph()).unwrap();
    editor.selection.select_node("b", false);
    editor.selection.edges.insert("wire".into());
    editor
        .transaction("Hide", |graph| {
            set_node_visible(graph, &"b".into(), false)?;
            Ok(())
        })
        .unwrap();
    assert!(editor.selection.is_empty());
    assert_eq!(editor.graph().edges.len(), 1);
    editor.select_all();
    assert_eq!(editor.selection.nodes, [NodeId::from("a")].into());
    assert!(editor.selection.edges.is_empty());
    editor.select_rect(
        Rect::new(Point::ZERO, Size::new(100_000., 1000.)),
        false,
        false,
    );
    assert_eq!(editor.selection.nodes, [NodeId::from("a")].into());
    assert!(editor.undo());
    assert!(node_is_visible(editor.graph(), &"b".into()));
    assert!(editor.selection.nodes.contains(&"b".into()));
    assert!(editor.selection.edges.contains(&"wire".into()));
    assert!(editor.redo());
    assert!(editor.selection.is_empty());
}

#[test]
fn fit_and_minimap_ignore_hidden_nodes_but_complete_graph_bounds_do_not_change() {
    let mut graph = graph();
    let complete = graph.bounds();
    set_node_visible(&mut graph, &"b".into(), false).unwrap();
    let mut editor = Editor::new(graph).unwrap();
    let canvas = Size::new(1180., 812.);
    let mut expected = editor.viewport;
    expected.fit(editor.graph().nodes[&"a".into()].bounds(), canvas, 32.);
    editor.fit_to_content(canvas, 32.);
    assert_eq!(editor.viewport, expected);
    assert_eq!(editor.graph().bounds(), complete);
    let minimap = editor
        .minimap(canvas, Rect::new(Point::ZERO, Size::new(180., 120.)), 8.)
        .unwrap();
    assert_eq!(
        minimap
            .nodes
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>(),
        ["a"]
    );
}

#[test]
fn explicit_frame_movement_retains_hidden_members_and_copy_contains_only_visible_members() {
    let mut graph = graph();
    graph
        .add_group(
            Group::new("frame", "Frame", graph.bounds().unwrap())
                .with_nodes([NodeId::from("a"), NodeId::from("b")]),
        )
        .unwrap();
    set_node_visible(&mut graph, &"b".into(), false).unwrap();
    let mut editor = Editor::new(graph).unwrap();
    editor.selection.select_group("frame", false);
    let before = editor.graph().nodes[&"b".into()].position;
    editor.move_selection(Point::new(10., 20.)).unwrap();
    assert_eq!(
        editor.graph().nodes[&"b".into()].position,
        before + Point::new(10., 20.)
    );
    let clipboard = editor.copy_selection();
    clipboard.validate().unwrap();
    assert_eq!(clipboard.nodes.len(), 1);
    assert_eq!(clipboard.groups[0].node_ids, [NodeId::from("a")].into());
    assert!(clipboard.edges.is_empty());
    assert_eq!(editor.graph().edges.len(), 1);
}

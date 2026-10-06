use gpui_flow_interaction::{core::*, *};
use std::collections::BTreeMap;

fn editor() -> Editor {
    let mut graph = Graph::new();
    for (id, x) in [("a", 0.), ("b", 350.)] {
        graph
            .add_node(
                Node::new(id, id, Point::new(x, 0.))
                    .with_port(Port::input("in", "Input", DataType::Number))
                    .with_port(Port::output("out", "Output", DataType::Number)),
            )
            .unwrap();
    }
    set_node_visible(&mut graph, &"b".into(), false).unwrap();
    Editor::new(graph).unwrap()
}

#[test]
fn hidden_nodes_cannot_be_selected_or_targeted_by_port_snap() {
    let mut editor = editor();
    let mut host = PluginHost::with_defaults();
    assert!(
        host.execute(
            &mut editor,
            CanvasCommand::SelectNode {
                node: "b".into(),
                mode: SelectionMode::Replace
            }
        )
        .is_err()
    );
    let from = PortRef::new("a", "out");
    let hidden_anchor = editor
        .graph()
        .port_position(&PortRef::new("b", "in"))
        .unwrap();
    host.execute(
        &mut editor,
        CanvasCommand::PreviewConnection {
            from: from.clone(),
            position: hidden_anchor + Point::new(8., 0.),
            hovered: None,
        },
    )
    .unwrap();
    assert!(host.connection().target().is_none());
    host.execute(
        &mut editor,
        CanvasCommand::PreviewConnection {
            from: from.clone(),
            position: hidden_anchor,
            hovered: Some(PortRef::new("b", "in")),
        },
    )
    .unwrap();
    assert!(host.connection().target().is_none());
    host.execute(&mut editor, CanvasCommand::CompleteConnection { from })
        .unwrap();
    assert!(editor.graph().edges.is_empty());
    assert!(
        host.execute(
            &mut editor,
            CanvasCommand::PreviewConnection {
                from: PortRef::new("b", "out"),
                position: hidden_anchor,
                hovered: None,
            }
        )
        .is_err()
    );
}

#[test]
fn hidden_nodes_are_not_position_snap_targets() {
    let mut editor = editor();
    let mut host = PluginHost::with_defaults();
    host.execute(
        &mut editor,
        CanvasCommand::SetPositions {
            nodes: BTreeMap::from([(NodeId::from("a"), Point::new(347., 4.))]),
            groups: BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(
        editor.graph().nodes[&"a".into()].position,
        Point::new(347., 4.)
    );
    let mut graph = editor.graph().clone();
    set_node_visible(&mut graph, &"b".into(), true).unwrap();
    host.execute(&mut editor, CanvasCommand::ImportGraph(graph))
        .unwrap();
    host.execute(
        &mut editor,
        CanvasCommand::SetPositions {
            nodes: BTreeMap::from([(NodeId::from("a"), Point::new(347., 4.))]),
            groups: BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(
        editor.graph().nodes[&"a".into()].position,
        Point::new(350., 0.)
    );
}

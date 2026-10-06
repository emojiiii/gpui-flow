use gpui_flow_core::*;
use serde_json::json;

fn graph() -> Graph {
    let mut graph = Graph::new();
    for id in ["a", "b", "c"] {
        graph.add_node(Node::new(id, id, Point::ZERO)).unwrap();
    }
    graph
}

#[test]
fn reorder_delete_undo_redo_and_document_reopen_keep_one_visual_order() {
    let mut editor = Editor::new(graph()).unwrap();
    editor.selection.select_node("c", false);
    let selected = editor.selection.nodes.clone();
    editor
        .transaction("Send back", |graph| {
            reorder_nodes(graph, &selected, NodeOrderAction::SendToBack)?;
            Ok(())
        })
        .unwrap();
    let order: Vec<NodeId> = ["c", "a", "b"].map(Into::into).into();
    assert_eq!(ordered_node_ids(editor.graph()), order);
    assert!(editor.undo());
    assert_eq!(
        ordered_node_ids(editor.graph()),
        ["a", "b", "c"].map(Into::into)
    );
    assert!(editor.redo());
    editor.delete_selection().unwrap();
    assert_eq!(ordered_node_ids(editor.graph()), ["a", "b"].map(Into::into));
    assert!(editor.undo());
    assert_eq!(ordered_node_ids(editor.graph()), order);
    let reopened = FlowDocument::from_json(&editor.document().to_json().unwrap()).unwrap();
    assert_eq!(ordered_node_ids(&reopened.graph), order);
}

#[test]
fn reordered_copy_and_paste_keep_relative_layers_across_fresh_id_boundary() {
    let mut source = graph();
    write_node_order(&mut source, ["c", "a", "b"].map(Into::into)).unwrap();
    let mut source = Editor::new(source).unwrap();
    source.selection.nodes.extend(["a", "c"].map(NodeId::from));
    let clipboard = source.copy_selection();
    assert_eq!(
        clipboard
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .collect::<Vec<_>>(),
        ["c", "a"]
    );
    let mut target = Graph::new();
    for i in 1..=8 {
        target
            .add_node(Node::new(format!("node-{i}"), "existing", Point::ZERO))
            .unwrap();
    }
    target.data = json!({"host": "preserved"});
    let mut target = Editor::new(target).unwrap();
    let before = target.document();
    target.paste(&clipboard, Point::new(24., 24.)).unwrap();
    let order = ordered_node_ids(target.graph());
    assert_eq!(
        &order[order.len() - 2..],
        [NodeId::from("node-9"), NodeId::from("node-10")]
    );
    assert_eq!(target.graph().nodes[&"node-9".into()].title, "c");
    assert_eq!(target.graph().nodes[&"node-10".into()].title, "a");
    assert_eq!(target.graph().data["host"], "preserved");
    assert!(target.undo());
    assert_eq!(target.document(), before);
    assert!(target.redo());
    assert_eq!(ordered_node_ids(target.graph()), order);
}

#[test]
fn adding_after_explicit_order_is_on_top_and_legacy_data_is_untouched() {
    let mut ordered = graph();
    write_node_order(&mut ordered, ["c", "b", "a"].map(Into::into)).unwrap();
    ordered
        .add_node(Node::new("0", "new", Point::ZERO))
        .unwrap();
    assert_eq!(
        ordered_node_ids(&ordered),
        ["c", "b", "a", "0"].map(Into::into)
    );
    for data in [
        json!("legacy host data"),
        json!({"flow_presentation": false}),
    ] {
        let mut legacy = graph();
        legacy.data = data.clone();
        legacy.add_node(Node::new("0", "new", Point::ZERO)).unwrap();
        assert_eq!(legacy.data, data);
    }
}

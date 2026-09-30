use gpui_flow_core::{DataType, Editor, Graph, Node, Point, Port, PortRef};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = Graph::new();
    graph.options.allow_cycles = false;
    graph.add_node(
        Node::new("prompt", "Prompt", Point::new(40.0, 80.0))
            .with_kind("text")
            .with_port(Port::output("out", "Text", DataType::String))
            .with_data(json!({"text": "A quiet landscape"})),
    )?;
    graph.add_node(
        Node::new("agent", "Agent", Point::new(360.0, 80.0))
            .with_kind("agent")
            .with_port(Port::input("in", "Prompt", DataType::String))
            .with_port(Port::output("out", "Result", DataType::Object)),
    )?;
    graph.connect(
        "prompt-to-agent",
        PortRef::new("prompt", "out"),
        PortRef::new("agent", "in"),
    )?;
    assert!(
        graph
            .validate_connection(&PortRef::new("prompt", "out"), &PortRef::new("agent", "in"))
            .is_err(),
        "duplicate connections must fail"
    );

    let mut editor = Editor::new(graph)?;
    editor.selection.select_node("prompt", false);
    editor.begin_transaction("Drag prompt")?;
    editor.move_selection(Point::new(10.0, 0.0))?;
    editor.move_selection(Point::new(20.0, 15.0))?;
    assert!(editor.commit_transaction()?);
    assert_eq!(editor.history_len(), 1);
    assert!(editor.undo());
    assert_eq!(
        editor.graph().nodes[&"prompt".into()].position,
        Point::new(40.0, 80.0)
    );
    assert!(editor.redo());

    editor.select_all();
    let clipboard = editor.copy_selection();
    editor.paste(&clipboard, Point::new(0.0, 260.0))?;
    assert_eq!(editor.graph().nodes.len(), 4);
    assert_eq!(editor.graph().edges.len(), 2);
    let group = editor.group_selection("Copied pair", 20.0)?;
    assert_eq!(editor.graph().groups[&group].node_ids.len(), 2);

    editor.viewport.zoom_at(Point::new(320.0, 200.0), 1.25);
    let document_json = editor.to_json()?;
    let restored = Editor::from_json(&document_json)?;
    assert_eq!(restored.document(), editor.document());
    assert_eq!(restored.history_len(), 0);
    assert!(restored.selection.is_empty());
    println!("Graph validated; move/undo/redo/paste/group/document round-trip passed.");
    println!("{document_json}");
    Ok(())
}

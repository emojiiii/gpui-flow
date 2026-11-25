use gpui_flow_core::*;
use gpui_flow_interaction::*;
use serde_json::{Value, json};

fn request(editor: &Editor, operations: Value) -> AgentEditRequest {
    serde_json::from_value(json!({
        "schema_version": AGENT_EDIT_SCHEMA_VERSION,
        "expected_revision": editor.agent_snapshot().revision,
        "operations": operations,
    }))
    .unwrap()
}

fn add_node(id: &str) -> Value {
    json!({"op":"add_node","id":id,"title":id,"position":{"x":0,"y":0}})
}

struct ProjectPolicy;
impl CanvasPlugin for ProjectPolicy {
    fn id(&self) -> &str {
        "project-policy"
    }

    fn filter_command(
        &mut self,
        command: &mut CanvasCommand,
        cx: &PluginContext<'_>,
        effects: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        match command {
            CanvasCommand::RemoveNode { node } if node.as_str() == "protected" => {
                Err("protected node cannot be removed".into())
            }
            CanvasCommand::AddNode(_) if cx.editor.graph().nodes.len() >= 2 => {
                Err("project node limit reached".into())
            }
            CanvasCommand::AddNode(node) if node.id.as_str() == "rewritten" => {
                node.id = "redirected".into();
                Ok(())
            }
            CanvasCommand::AddNode(_) => {
                effects.push(CanvasEffect::Cursor(CanvasCursor::Grab));
                Ok(())
            }
            CanvasCommand::CompleteConnection { .. }
                if cx
                    .connection
                    .target()
                    .is_some_and(|target| target.node.as_str() == "protected") =>
            {
                Err("protected input cannot be connected".into())
            }
            _ => Ok(()),
        }
    }

    fn authorize_agent_operation(
        &mut self,
        operation: &AgentOperation,
        _: &PluginContext<'_>,
    ) -> Result<(), String> {
        match operation {
            AgentOperation::Disconnect { .. } => Err("edges cannot be disconnected".into()),
            _ => Ok(()),
        }
    }
}

#[test]
fn installed_middleware_checks_native_and_agent_edits_on_the_staged_graph() {
    let mut host = PluginHost::new();
    host.install(ProjectPolicy).unwrap();
    let mut graph = Graph::new();
    graph
        .add_node(Node::new("protected", "Protected", Point::ZERO))
        .unwrap();
    let mut editor = Editor::new(graph).unwrap();
    let revision = editor.document_revision();
    let before = editor.document();
    assert!(matches!(
        host.execute(
            &mut editor,
            CanvasCommand::RemoveNode {
                node: "protected".into()
            }
        ),
        Err(InteractionError::PluginRejected { .. })
    ));
    for (ops, index, message) in [
        (
            json!([add_node("temporary"), {"op":"remove_node","id":"protected"}]),
            1,
            "protected node",
        ),
        (
            json!([add_node("first"), add_node("second")]),
            1,
            "node limit",
        ),
        (
            json!([add_node("rewritten")]),
            0,
            "changed an explicit agent edit",
        ),
    ] {
        let batch = request(&editor, ops);
        let error = host
            .apply_agent_edit(&mut editor, batch, |_, _| Ok(()))
            .unwrap_err();
        assert_eq!(error.code, AgentEditErrorCode::PolicyDenied);
        assert_eq!(error.operation_index, Some(index));
        assert!(error.message.contains(message));
        assert_eq!(editor.document_revision(), revision);
        assert_eq!(editor.document(), before);
        assert!(!editor.can_undo());
        assert_eq!(host.cursor(), CanvasCursor::Default);
    }
    let allowed = request(&editor, json!([add_node("allowed")]));
    host.apply_agent_edit(&mut editor, allowed, |_, _| Ok(()))
        .unwrap();
    assert_eq!(editor.graph().nodes.len(), 2);
    assert_eq!(host.cursor(), CanvasCursor::Default);
    assert!(editor.undo());
    assert_eq!(editor.document(), before);
}

#[test]
fn explicit_connection_checks_legacy_and_agent_policies() {
    let mut host = PluginHost::new();
    host.install(ProjectPolicy).unwrap();
    let mut graph = Graph::new();
    graph
        .add_node(
            Node::new("protected", "Protected", Point::ZERO).with_port(Port::input(
                "in",
                "In",
                DataType::Number,
            )),
        )
        .unwrap();
    graph
        .add_node(
            Node::new("source", "Source", Point::ZERO).with_port(Port::output(
                "out",
                "Out",
                DataType::Number,
            )),
        )
        .unwrap();
    let mut editor = Editor::new(graph).unwrap();
    let connect = request(
        &editor,
        json!([
            {"op":"connect","id":"wire","source":{"node":"source","port":"out"},"target":{"node":"protected","port":"in"}}
        ]),
    );
    let error = host
        .apply_agent_edit(&mut editor, connect.clone(), |_, _| Ok(()))
        .unwrap_err();
    assert_eq!(error.code, AgentEditErrorCode::PolicyDenied);
    assert!(error.message.contains("protected input"));
    assert!(editor.graph().edges.is_empty());
    assert!(!host.connection().active());

    editor.apply_agent_edit(connect).unwrap();
    let disconnect = request(&editor, json!([{"op":"disconnect","id":"wire"}]));
    let error = host
        .apply_agent_edit(&mut editor, disconnect, |_, _| Ok(()))
        .unwrap_err();
    assert_eq!(error.code, AgentEditErrorCode::PolicyDenied);
    assert!(error.message.contains("cannot be disconnected"));
    assert_eq!(editor.graph().edges.len(), 1);
}

#[test]
fn denied_resurrection_and_panicking_authorization_restore_the_original_editor() {
    let mut graph = Graph::new();
    graph
        .add_node(Node::new("existing", "Original", Point::ZERO))
        .unwrap();
    let mut editor = Editor::new(graph).unwrap();
    editor.selection.select_node("existing", false);
    let before = editor.document();
    let selection = editor.selection.clone();
    let revision = editor.document_revision();
    let resurrection = request(
        &editor,
        json!([
            {"op":"remove_node","id":"existing"},
            add_node("existing")
        ]),
    );
    let err = editor.apply_agent_edit(resurrection).unwrap_err();
    assert_eq!(err.code, AgentEditErrorCode::DuplicateId);
    assert_eq!(err.operation_index, Some(1));
    assert_eq!(editor.document(), before);

    let batch = request(&editor, json!([add_node("first"), add_node("second")]));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        editor.apply_agent_edit_guarded(
            batch,
            |staged, _| {
                assert!(
                    !staged.graph().nodes.contains_key(&"first".into()),
                    "plugin panic"
                );
                Ok(())
            },
            |_, _| Ok(()),
        )
    }));
    assert!(result.is_err());
    assert_eq!(editor.document(), before);
    assert_eq!(editor.selection, selection);
    assert_eq!(editor.document_revision(), revision);
    assert!(!editor.can_undo());
}

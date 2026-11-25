use gpui_flow_core::*;
use serde_json::{json, Value};

fn request(editor: &Editor, operations: Value) -> AgentEditRequest {
    serde_json::from_value(json!({
        "schema_version": AGENT_EDIT_SCHEMA_VERSION,
        "expected_revision": editor.agent_snapshot().revision,
        "operations": operations,
    }))
    .unwrap()
}

#[test]
fn json_edit_creates_a_workflow_with_one_undo() {
    let mut editor = Editor::default();
    let before = serde_json::to_value(editor.agent_snapshot()).unwrap();
    assert_eq!(before["schema_version"], AGENT_EDIT_SCHEMA_VERSION);
    assert_eq!(before["graph"]["nodes"], json!({}));

    let request = request(
        &editor,
        json!([
            {
                "op": "add_node", "id": "source", "title": "Source",
                "position": { "x": 0, "y": 20 }, "data": { "value": 7 },
                "ports": [{
                    "id": "value", "label": "Value", "direction": "output",
                    "data_type": { "kind": "number" }
                }]
            },
            {
                "op": "add_node", "id": "sink", "title": "Sink",
                "position": { "x": 300, "y": 20 }, "kind": "preview",
                "ports": [{
                    "id": "value", "label": "Value", "direction": "input",
                    "data_type": { "kind": "number" }, "max_connections": 1
                }]
            },
            {
                "op": "connect", "id": "value-link",
                "source": { "node": "source", "port": "value" },
                "target": { "node": "sink", "port": "value" }
            },
            { "op": "update_node", "id": "sink", "title": "Preview" },
            { "op": "set_node_data", "id": "source", "data": null }
        ]),
    );
    let receipt = serde_json::to_value(editor.apply_agent_edit(request).unwrap()).unwrap();
    assert_eq!(
        receipt["changes"]["nodes"]["inserted"],
        json!(["sink", "source"])
    );
    assert_eq!(
        receipt["changes"]["edges"]["inserted"],
        json!(["value-link"])
    );
    assert_eq!(receipt["changes"]["nodes"]["updated"], json!([]));
    assert_ne!(receipt["revision"], before["revision"]);

    let snapshot = serde_json::to_value(editor.agent_snapshot()).unwrap();
    assert_eq!(snapshot["revision"], receipt["revision"]);
    assert_eq!(snapshot["graph"]["nodes"]["sink"]["title"], "Preview");
    assert_eq!(snapshot["graph"]["nodes"]["sink"]["kind"], "preview");
    assert_eq!(snapshot["graph"]["nodes"]["source"]["data"], Value::Null);
    assert!(editor.undo());
    assert_eq!(editor.graph().nodes.len(), 0);
    assert_eq!(editor.graph().edges.len(), 0);
    assert!(!editor.can_undo());
    assert!(editor.redo());
    assert_eq!(editor.graph().edges.len(), 1);
}

#[test]
fn wire_contract_rejects_unknown_operations_and_fields() {
    serde_json::from_str::<Value>(AGENT_EDIT_SCHEMA).unwrap();
    for operation in [
        json!({ "op": "select_all" }),
        json!({ "op": "update_node", "id": "node", "postion": { "x": 1, "y": 2 } }),
    ] {
        assert!(serde_json::from_value::<AgentEditRequest>(json!({
            "schema_version": AGENT_EDIT_SCHEMA_VERSION,
            "expected_revision": "1",
            "operations": [operation],
        }))
        .is_err());
    }
    let mut editor = Editor::default();
    let mut request = request(&editor, json!([]));
    request.schema_version = AGENT_EDIT_SCHEMA_VERSION + 1;
    assert_eq!(
        editor.apply_agent_edit(request).unwrap_err().code,
        AgentEditErrorCode::UnsupportedSchema
    );
}

#[test]
fn net_noop_keeps_revision_and_redo() {
    let mut editor = Editor::default();
    editor
        .transaction("Add node", |graph| {
            graph.add_node(Node::new("prior", "Prior", Point::ZERO))
        })
        .unwrap();
    assert!(editor.undo());
    let request = request(
        &editor,
        json!([
            {"op": "add_node", "id": "temp", "title": "Temporary", "position": {"x": 0, "y": 0}},
            {"op": "remove_node", "id": "temp"}
        ]),
    );
    let revision = request.expected_revision.clone();
    let receipt = editor.apply_agent_edit(request).unwrap();
    assert_eq!(receipt.revision, revision);
    assert!(receipt.changes.nodes.inserted.is_empty());
    assert!(receipt.changes.nodes.removed.is_empty());
    assert!(!editor.can_undo());
    assert!(editor.can_redo());
}

#[test]
fn revisions_are_scoped_to_one_editor_and_busy_gestures_block_edits() {
    let mut graph = Graph::new();
    graph
        .add_node(Node::new("shared", "Initial", Point::ZERO))
        .unwrap();
    let mut editor = Editor::new(graph.clone()).unwrap();
    let mut other = Editor::new(graph).unwrap();
    let operations = json!([{ "op": "update_node", "id": "shared", "title": "Edited" }]);
    let initial = request(&editor, operations.clone());

    let mut fork = editor.clone();
    assert_ne!(fork.document_revision(), editor.document_revision());
    let error = fork.apply_agent_edit(initial.clone()).unwrap_err();
    assert_eq!(error.code, AgentEditErrorCode::StaleRevision);
    assert_eq!(error.current_revision, Some(fork.agent_snapshot().revision));
    assert_eq!(fork.graph(), editor.graph());

    let err = other.apply_agent_edit(initial.clone()).unwrap_err();
    assert_eq!(err.code, AgentEditErrorCode::StaleRevision);
    assert_eq!(err.current_revision, Some(other.agent_snapshot().revision));
    assert_eq!(editor.graph(), other.graph());

    editor.begin_transaction("Drag").unwrap();
    assert_eq!(
        editor.apply_agent_edit(initial.clone()).unwrap_err().code,
        AgentEditErrorCode::Busy
    );
    assert!(editor.transaction_active());
    assert_eq!(editor.graph(), other.graph());
    editor.cancel_transaction().unwrap();

    let latest = request(&editor, operations);
    editor.apply_agent_edit(latest).unwrap();
    assert!(editor.undo());
    assert_eq!(editor.graph(), other.graph());
    assert_eq!(
        editor.apply_agent_edit(initial).unwrap_err().code,
        AgentEditErrorCode::StaleRevision
    );
}

#[test]
fn invalid_later_operations_roll_back_prior_edits() {
    let mut graph = Graph::new();
    graph
        .add_node(
            Node::new("editable", "Initial", Point::ZERO).with_port(Port::output(
                "value",
                "Value",
                DataType::Number,
            )),
        )
        .unwrap();
    let mut locked = Node::new("locked", "Locked", Point::ZERO);
    locked.locked = true;
    graph.add_node(locked).unwrap();
    graph
        .add_node(
            Node::new("sink", "Sink", Point::new(300.0, 0.0)).with_port(Port::input(
                "value",
                "Value",
                DataType::String,
            )),
        )
        .unwrap();
    let mut editor = Editor::new(graph.clone()).unwrap();
    let update = json!({ "op": "update_node", "id": "editable", "title": "Edited" });
    let operations = [
        (
            json!({ "op": "remove_node", "id": "locked" }),
            AgentEditErrorCode::Locked,
        ),
        (
            json!({ "op": "disconnect", "id": "unknown" }),
            AgentEditErrorCode::NotFound,
        ),
        (
            json!({
                "op": "connect", "id": "invalid-link",
                "source": { "node": "editable", "port": "value" },
                "target": { "node": "sink", "port": "value" }
            }),
            AgentEditErrorCode::InvalidConnection,
        ),
    ];
    for (operation, code) in operations {
        let request = request(&editor, json!([update, operation]));
        let revision = request.expected_revision.clone();
        let err = editor.apply_agent_edit(request).unwrap_err();
        let response = serde_json::to_value(&err).unwrap();
        assert_eq!(err.code, code);
        assert_eq!(response["operation_index"], 1);
        assert_eq!(editor.agent_snapshot().revision, revision);
        assert_eq!(editor.graph(), &graph);
        assert!(!editor.can_undo());
    }
    let invalid_size = request(
        &editor,
        json!([{
            "op": "update_node", "id": "editable", "size": { "width": 0, "height": 10 }
        }]),
    );
    let err = editor.apply_agent_edit(invalid_size).unwrap_err();
    assert_eq!(err.code, AgentEditErrorCode::Validation);
    assert!(!err.issues.is_empty());
    assert_eq!(editor.graph(), &graph);
}

#[test]
fn validator_observes_candidate_and_removal_reports_incident_changes() {
    let mut graph = Graph::new();
    graph
        .add_node(
            Node::new("source", "Source", Point::ZERO).with_port(Port::output(
                "value",
                "Value",
                DataType::Number,
            )),
        )
        .unwrap();
    let sink = Node::new("sink", "Sink", Point::new(300.0, 0.0)).with_port(Port::input(
        "value",
        "Value",
        DataType::Number,
    ));
    let group = Group::new("group", "Group", sink.bounds()).with_nodes([sink.id.clone()]);
    graph.add_node(sink).unwrap();
    graph.add_group(group).unwrap();
    graph
        .connect(
            "link",
            PortRef::new("source", "value"),
            PortRef::new("sink", "value"),
        )
        .unwrap();
    let mut editor = Editor::new(graph.clone()).unwrap();
    editor.selection.select_node("sink", false);
    let request = request(&editor, json!([{ "op": "remove_node", "id": "sink" }]));
    let err = editor
        .apply_agent_edit_checked(request.clone(), |candidate| {
            assert!(candidate.node(&"sink".into()).is_none());
            assert!(candidate.edges.is_empty());
            assert!(candidate.groups[&"group".into()].node_ids.is_empty());
            Err("Sink is required".into())
        })
        .unwrap_err();
    assert_eq!(err.code, AgentEditErrorCode::PolicyDenied);
    assert!(err.message.contains("Sink is required"));
    assert_eq!(editor.graph(), &graph);
    assert!(editor.selection.nodes.contains(&"sink".into()));
    assert!(!editor.can_undo());

    let receipt = editor.apply_agent_edit(request).unwrap();
    assert_eq!(receipt.changes.nodes.removed, [NodeId::from("sink")]);
    assert_eq!(receipt.changes.edges.removed, [EdgeId::from("link")]);
    assert_eq!(receipt.changes.groups.updated, [GroupId::from("group")]);
    assert!(editor.selection.is_empty());
    assert!(editor.undo());
    assert_eq!(editor.graph(), &graph);
    assert!(editor.selection.nodes.contains(&"sink".into()));
}

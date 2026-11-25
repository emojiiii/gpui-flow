use gpui_flow_core::{AgentEditRequest, AgentOperation, DataType, Graph, Node, Point, Port};
use serde_json::{json, Value};

fn edit(operation: Value) -> Value {
    json!({
        "schema_version": 1,
        "expected_revision": "test-revision",
        "operations": [operation],
    })
}

#[test]
fn rejects_unknown_fields_and_sequences_inside_the_agent_request() {
    let node = json!({
        "op": "add_node",
        "id": "source",
        "title": "Source",
        "position": { "x": 0, "y": 0 },
        "size": { "width": 224, "height": 144 },
        "ports": [{
            "id": "value",
            "label": "Value",
            "direction": "input",
            "data_type": { "kind": "number" },
            "max_connections": 1
        }]
    });
    let connect = json!({
        "op": "connect",
        "id": "edge",
        "source": { "node": "source", "port": "value" },
        "target": { "node": "target", "port": "value" }
    });
    for (operation, pointer, field) in [
        (&node, "/position", "relative_to"),
        (&node, "/size", "depth"),
        (&node, "/ports/0", "max_connection"),
        (&node, "/ports/0/data_type", "description"),
        (&connect, "/source", "direction"),
        (&connect, "/target", "label"),
    ] {
        let mut invalid = operation.clone();
        invalid.pointer_mut(pointer).unwrap()[field] = json!(1);
        let error = serde_json::from_value::<AgentEditRequest>(edit(invalid)).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }

    for (operation, pointer, value) in [
        (&node, "/position", json!([0, 0])),
        (&node, "/size", json!([224, 144])),
        (
            &node,
            "/ports/0",
            json!(["value", "Value", "input", {"kind": "number"}]),
        ),
        (&node, "/ports/0/data_type", json!(["number"])),
        (&connect, "/source", json!(["source", "value"])),
    ] {
        let mut invalid = operation.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        assert!(serde_json::from_value::<AgentEditRequest>(edit(invalid)).is_err());
    }
    assert!(serde_json::from_value::<AgentEditRequest>(json!([1, "test-revision", []])).is_err());
}

#[test]
fn keeps_wire_integer_forms_and_graph_import_compatibility() {
    let json = r#"{
        "schema_version": 1.0,
        "expected_revision": "test-revision",
        "operations": [{
            "op": "update_node",
            "id": "source",
            "position": null,
            "size": null,
            "ports": [{
                "id": "value",
                "label": "Value",
                "direction": "input",
                "data_type": { "kind": "named", "name": "custom" },
                "max_connections": 1e0
            }]
        }]
    }"#;
    let request: AgentEditRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.schema_version, 1);
    let [AgentOperation::UpdateNode {
        ports: Some(ports),
        position: None,
        size: None,
        ..
    }] = request.operations.as_slice()
    else {
        panic!("expected the decoded update");
    };
    assert_eq!(ports[0].data_type, DataType::Named("custom".into()));
    assert_eq!(ports[0].max_connections, Some(1));

    for invalid in ["1.5", "-1", "\"1\"", "1e20"] {
        assert!(serde_json::from_str::<AgentEditRequest>(&json.replace("1e0", invalid)).is_err());
        assert!(serde_json::from_str::<AgentEditRequest>(&json.replace("1.0", invalid)).is_err());
    }

    let mut graph = Graph::new();
    graph
        .add_node(
            Node::new("source", "Source", Point::ZERO).with_port(Port::input(
                "value",
                "Value",
                DataType::Number,
            )),
        )
        .unwrap();
    let mut saved = serde_json::to_value(&graph).unwrap();
    saved["nodes"]["source"]["position"]["relative_to"] = json!("legacy");
    saved["nodes"]["source"]["ports"][0]["legacy"] = json!(true);
    assert_eq!(serde_json::from_value::<Graph>(saved).unwrap(), graph);
}

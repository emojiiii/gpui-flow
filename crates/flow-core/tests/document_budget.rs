use gpui_flow_core::{Editor, FlowDocument, Graph, GraphError, Node, Point, Viewport};
use serde_json::json;

#[test]
fn exact_pretty_json_budget_rejects_growth_atomically() {
    let mut graph = Graph::new();
    graph
        .add_node(Node::new("draft", "画布\n草稿", Point::ZERO))
        .unwrap();
    let mut editor = Editor::new(graph).unwrap();
    editor.selection.select_node("draft", false);
    let before = editor.document();
    let limit = before.to_json().unwrap().len();
    before.validate_json_size(limit).unwrap();
    assert_eq!(
        before.validate_json_size(limit - 1),
        Err(GraphError::DocumentTooLarge {
            max_bytes: limit - 1
        })
    );
    editor.set_document_byte_limit(limit).unwrap();
    let revision = editor.document_revision();

    assert_eq!(
        editor.transaction("Grow", |graph| {
            graph.data = json!({"annotation": "新"});
            Ok(())
        }),
        Err(GraphError::DocumentTooLarge { max_bytes: limit })
    );
    let clipboard = editor.copy_selection();
    assert_eq!(
        editor.paste(&clipboard, Point::new(1., 1.)),
        Err(GraphError::DocumentTooLarge { max_bytes: limit })
    );
    assert_eq!(editor.document(), before);
    assert_eq!(editor.document_revision(), revision);
    assert!(editor.selection.nodes.contains(&"draft".into()));
    assert!(!editor.can_undo());
}

#[test]
fn current_viewport_and_gesture_restore_must_stay_within_budget() {
    let mut editor = Editor::default();
    editor
        .transaction("Add metadata", |graph| {
            graph.data = json!({"annotation": "long enough to make a difference"});
            Ok(())
        })
        .unwrap();
    let original = editor.document();
    let limit = original.to_json().unwrap().len();
    editor.set_document_byte_limit(limit).unwrap();

    editor.begin_transaction("Shrink graph").unwrap();
    editor
        .transaction("Remove metadata", |graph| {
            graph.data = json!(null);
            Ok(())
        })
        .unwrap();
    let next_view = Viewport {
        pan: Point::new(1234.5, 1234.5),
        ..original.viewport
    };
    assert_eq!(
        editor.set_viewport(next_view),
        Err(GraphError::DocumentTooLarge { max_bytes: limit })
    );
    editor.cancel_transaction().unwrap();
    assert_eq!(editor.document(), original);

    assert!(editor.undo());
    editor.set_viewport(next_view).unwrap();
    assert_eq!(
        editor.try_redo(),
        Err(GraphError::DocumentTooLarge { max_bytes: limit })
    );
    assert!(editor.can_redo());
    editor.set_viewport(original.viewport).unwrap();
    assert!(editor.try_redo().unwrap());
    assert_eq!(editor.document(), original);
}

#[test]
fn imports_check_graph_and_camera_together_and_keep_the_limit_on_load() {
    let mut imported = FlowDocument::default();
    imported.graph.data = json!({"annotation": "保存"});
    let limit = imported.to_json().unwrap().len();
    let mut editor = Editor::default();
    editor.set_document_byte_limit(limit).unwrap();
    editor
        .set_viewport(Viewport {
            pan: Point::new(1234.5, 0.),
            ..imported.viewport
        })
        .unwrap();
    editor.import_document(imported.clone(), "Import").unwrap();
    assert_eq!(editor.document(), imported);
    assert!(editor.can_undo());

    let mut oversized = imported.clone();
    oversized.graph.data = json!({"annotation": "保存容量超出"});
    assert_eq!(
        editor.load_document(oversized.clone()),
        Err(GraphError::DocumentTooLarge { max_bytes: limit })
    );
    assert_eq!(editor.document(), imported);
    assert!(editor.can_undo());
    editor.load_document(imported.clone()).unwrap();
    assert!(!editor.can_undo());
    assert_eq!(
        editor.import_document(oversized, "Reject"),
        Err(GraphError::DocumentTooLarge { max_bytes: limit })
    );
    assert_eq!(editor.document(), imported);
}

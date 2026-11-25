use gpui_flow_core::{Editor, GraphError, Node, Point};

#[test]
fn graph_stamps_distinguish_edits_history_replacement_and_forks() {
    let mut editor = Editor::default();
    let blank = editor.document_revision();
    assert_ne!(blank, editor.clone().document_revision());
    editor.viewport.pan.x = 5.;
    editor.transaction("Noop", |_| Ok(())).unwrap();
    assert_eq!(blank, editor.document_revision());

    editor
        .transaction("Add", |graph| {
            graph.add_node(Node::new("one", "One", Point::ZERO))
        })
        .unwrap();
    let added = editor.document_revision();
    assert_ne!(blank, added);
    assert!(editor.undo());
    let undone = editor.document_revision();
    assert_ne!(undone, blank);
    assert_ne!(undone, added);
    assert!(editor.redo());
    let redone = editor.document_revision();
    assert_ne!(redone, added);
    assert_ne!(redone, undone);
    editor.load_document(editor.document()).unwrap();
    assert_ne!(redone, editor.document_revision());
}

#[test]
fn pending_graph_edits_stamp_even_when_cancel_restores_the_original_graph() {
    let mut editor = Editor::default();
    editor
        .transaction("Add", |graph| {
            graph.add_node(Node::new("one", "One", Point::ZERO))
        })
        .unwrap();
    editor.selection.select_node("one", false);
    let clipboard = editor.copy_selection();
    let graph = editor.graph().clone();
    let before = editor.document_revision();
    editor.begin_transaction("Gesture").unwrap();
    editor.paste(&clipboard, Point::new(30., 40.)).unwrap();
    let pasted = editor.document_revision();
    assert_ne!(pasted, before);
    editor.group_selection("Group", 12.).unwrap();
    let grouped = editor.document_revision();
    assert_ne!(grouped, pasted);
    assert_eq!(editor.history_len(), 1);
    editor.cancel_transaction().unwrap();
    assert_ne!(editor.document_revision(), grouped);
    assert_ne!(editor.document_revision(), before);
    assert_eq!(editor.graph(), &graph);

    let restored = editor.document_revision();
    let error = editor.transaction("Rejected", |graph| {
        graph.add_node(Node::new("one", "Duplicate", Point::ZERO))
    });
    assert!(matches!(error, Err(GraphError::DuplicateId { .. })));
    assert_eq!(editor.document_revision(), restored);
}

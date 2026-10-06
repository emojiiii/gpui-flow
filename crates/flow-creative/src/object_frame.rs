//! Atomic insertion and initial Frame membership for newly drawn objects.
//! The caller commits the returned graph through its normal guarded transaction.

use gpui_flow_core::{Graph, GraphError, GroupId, Node};

/// Add a new node and attach it only when exactly one Frame fully contains its
/// positive bounds. Touching the Frame's edges counts as containment. Any fully
/// containing locked Frame rejects insertion, even among ambiguous containers.
/// Partial overlaps and multiple unlocked containers leave the node standalone.
///
/// Existing nodes are never reassigned, moved or restacked. Core insertion keeps
/// its usual presentation-order behavior, appending the new node when enabled.
/// This checks core graph validity; Creative payload checks remain on the
/// caller's guarded command path. The source graph is unchanged on every path.
pub fn placed_object_graph(
    graph: &Graph,
    node: Node,
) -> Result<(Graph, Option<GroupId>), GraphError> {
    let bounds = node.bounds();
    let id = node.id.clone();
    let mut candidate = graph.clone();
    candidate.add_node(node)?;
    candidate.validate()?;
    // A finite positive size can still disappear when added to an extreme
    // origin. Such collapsed bounds cannot establish complete containment.
    if bounds.max_x() <= bounds.min_x() || bounds.max_y() <= bounds.min_y() {
        return Err(GraphError::InvalidGeometry(
            "Object bounds must have representable positive width and height".into(),
        ));
    }

    let mut owner = None;
    let mut ambiguous = false;
    for frame in graph.groups.values() {
        if !frame.bounds.contains_rect(bounds) {
            continue;
        }
        if frame.locked {
            return Err(GraphError::Locked(format!(
                "该画框已锁定，请解锁后添加对象（{}）",
                frame.id
            )));
        }
        if owner.is_some() {
            ambiguous = true;
        } else {
            owner = Some(frame.id.clone());
        }
    }
    if ambiguous {
        owner = None;
    }
    if let Some(frame) = &owner {
        crate::frames::add_frame_members(&mut candidate, frame, &[id].into_iter().collect())?;
    }
    Ok((candidate, owner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pencil::{PencilStyle, create_pencil_node_from_points};
    use crate::shapes::{ShapeKind, create_shape_node_from_points};
    use gpui_flow_core::{Editor, Group, Point, Rect, Size};
    use gpui_flow_interaction::{CanvasCommand, PluginHost};
    use serde_json::json;
    use std::sync::Arc;

    fn frame(id: &str) -> Group {
        Group::new(id, "Frame", Rect::new(Point::ZERO, Size::new(400., 300.)))
    }

    fn object(id: &str, position: Point) -> Node {
        Node::new(id, "Object", position).with_size(Size::new(20., 20.))
    }

    #[test]
    fn small_object_joins_only_unique_frame_preserving_existing_content_and_order() {
        let mut graph = Graph::new();
        graph
            .add_node(object("member", Point::new(10., 10.)))
            .unwrap();
        graph
            .add_node(object("loose", Point::new(60., 60.)))
            .unwrap();
        let mut board = frame("board");
        board.node_ids.insert("member".into());
        board.data = json!({"host": "frame metadata"});
        graph.add_group(board).unwrap();
        let mut other = frame("other");
        other.bounds.origin.x = 600.;
        graph.add_group(other).unwrap();
        graph.data = json!({
            "host": {"keep": true},
            "flow_presentation": {"node_order": ["loose", "member"], "show_grid": false}
        });
        let before = graph.clone();
        let node = create_shape_node_from_points(
            "small",
            "Small rectangle",
            ShapeKind::Rectangle,
            Point::new(398., 298.),
            Point::new(400., 300.),
        )
        .unwrap();

        let (placed, owner) = placed_object_graph(&graph, node.clone()).unwrap();

        let mut expected = before.clone();
        expected.add_node(node.clone()).unwrap();
        expected
            .groups
            .get_mut(&"board".into())
            .unwrap()
            .node_ids
            .insert(node.id);
        assert_eq!(owner, Some("board".into()));
        assert_eq!(placed, expected);
        assert_eq!(graph, before);
    }

    #[test]
    fn drawn_kinds_follow_frame_moves_and_insertion_undo_restores_membership_atomically() {
        let registry = Arc::new(crate::standard_registry());
        let shape = create_shape_node_from_points(
            "shape",
            "Rectangle",
            ShapeKind::Rectangle,
            Point::new(10., 20.),
            Point::new(50., 60.),
        )
        .unwrap();
        let pencil = create_pencil_node_from_points(
            "pencil",
            "Stroke",
            &[Point::new(10., 20.), Point::new(50., 20.)],
            PencilStyle::default(),
        )
        .unwrap();
        let text = registry
            .make_node("text", "text", Point::new(10., 20.), None)
            .unwrap();
        for node in [shape, pencil, text] {
            let mut graph = Graph::new();
            graph.add_group(frame("board")).unwrap();
            let (placed, owner) = placed_object_graph(&graph, node.clone()).unwrap();
            assert_eq!(owner, Some("board".into()));
            let mut editor = Editor::new(graph.clone()).unwrap();
            let mut host = PluginHost::with_defaults();
            host.install(crate::CreativeGuard(registry.clone()))
                .unwrap();
            host.execute(&mut editor, CanvasCommand::ImportGraph(placed.clone()))
                .unwrap();
            editor.selection.select_group("board", false);
            let delta = Point::new(70., -30.);
            editor.move_selection(delta).unwrap();
            assert_eq!(
                editor.graph().node(&node.id).unwrap().position,
                node.position + delta
            );
            assert_eq!(
                editor.graph().groups[&"board".into()].bounds,
                frame("board").bounds.translated(delta)
            );
            assert!(editor.undo());
            assert_eq!(editor.graph(), &placed);
            host.execute(&mut editor, CanvasCommand::Undo).unwrap();
            assert_eq!(editor.graph(), &graph);
            assert!(!editor.undo());
            host.execute(&mut editor, CanvasCommand::Redo).unwrap();
            assert_eq!(editor.graph(), &placed);
        }
    }

    #[test]
    fn outside_partial_and_ambiguous_containment_leave_objects_standalone() {
        let mut outside = Graph::new();
        outside.add_group(frame("board")).unwrap();
        let mut partial = outside.clone();
        partial.groups.get_mut(&"board".into()).unwrap().locked = true;
        let mut ambiguous = outside.clone();
        let mut nested = frame("nested");
        nested.bounds = Rect::new(Point::new(5., 5.), Size::new(100., 100.));
        ambiguous.add_group(nested).unwrap();

        for (graph, position) in [
            (Graph::new(), Point::new(10., 10.)),
            (outside, Point::new(500., 500.)),
            (partial, Point::new(390., 20.)),
            (ambiguous, Point::new(10., 10.)),
        ] {
            let before = graph.clone();
            let node = object("new", position);
            let (placed, owner) = placed_object_graph(&graph, node.clone()).unwrap();
            let mut expected = before.clone();
            expected.add_node(node).unwrap();
            assert_eq!(owner, None);
            assert_eq!(placed, expected);
            assert_eq!(graph, before);
        }
    }

    #[test]
    fn containing_locked_frame_rejects_even_after_multiple_unlocked_containers() {
        for unlocked_count in 0..=2 {
            let mut graph = Graph::new();
            for index in 0..unlocked_count {
                graph
                    .add_group(frame(&format!("unlocked-{index}")))
                    .unwrap();
            }
            let mut locked = frame("z-locked");
            locked.locked = true;
            graph.add_group(locked).unwrap();
            let before = graph.clone();

            let error =
                placed_object_graph(&graph, object("new", Point::new(10., 10.))).unwrap_err();

            assert!(matches!(error, GraphError::Locked(_)));
            assert!(error.to_string().contains("该画框已锁定，请解锁后添加对象"));
            assert_eq!(graph, before);
        }
    }

    #[test]
    fn invalid_insertion_and_membership_failures_leave_document_and_history_unchanged() {
        let mut graph = Graph::new();
        graph.add_node(object("existing", Point::ZERO)).unwrap();
        graph.add_group(frame("board")).unwrap();
        let mut locked = object("locked", Point::new(10., 10.));
        locked.locked = true;
        for node in [
            object("existing", Point::new(10., 10.)),
            object("invalid", Point::new(10., 10.)).with_size(Size::new(0., 20.)),
            locked,
        ] {
            let mut editor = Editor::new(graph.clone()).unwrap();
            editor.selection.select_node("existing", false);
            let before = editor.document();
            let selection = editor.selection.clone();
            assert!(
                editor
                    .transaction("Place object", |current| {
                        let (candidate, _) = placed_object_graph(current, node)?;
                        *current = candidate;
                        Ok(())
                    })
                    .is_err()
            );
            assert_eq!(editor.document(), before);
            assert_eq!(editor.selection, selection);
            assert!(!editor.undo());
        }
        graph
            .groups
            .get_mut(&"board".into())
            .unwrap()
            .node_ids
            .insert("missing".into());
        let before = graph.clone();
        assert!(placed_object_graph(&graph, object("new", Point::new(500., 500.))).is_err());
        assert_eq!(graph, before);
    }
}

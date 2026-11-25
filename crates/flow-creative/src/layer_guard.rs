use gpui_flow_core::{Graph, NodeId};
use gpui_flow_interaction::{CanvasCommand, PluginContext};

pub(crate) fn node_is_locked(graph: &Graph, id: &NodeId) -> bool {
    graph.node(id).is_some_and(|n| n.locked)
        || graph
            .groups
            .values()
            .any(|g| g.locked && g.node_ids.contains(id))
}

fn reject_locked(graph: &Graph, id: &NodeId) -> Result<(), String> {
    if node_is_locked(graph, id) {
        Err(format!("对象 {id} 已锁定，请先解锁"))
    } else {
        Ok(())
    }
}

/// Creative's lock includes editing content, geometry, membership and links.
/// Selection, copying, navigation and history restoration remain available.
pub(crate) fn filter_layer_command(
    command: &mut CanvasCommand,
    cx: &PluginContext<'_>,
) -> Result<(), String> {
    let graph = cx.editor.graph();
    match command {
        CanvasCommand::SetNodeData { node, .. } | CanvasCommand::ResizeNode { node, .. } => {
            reject_locked(graph, node)?;
        }
        CanvasCommand::SetPositions { nodes, groups } => {
            for node in nodes.keys() {
                reject_locked(graph, node)?;
            }
            for id in groups.keys() {
                if let Some(group) = graph.groups.get(id) {
                    if group.locked {
                        return Err("Frame 已锁定".into());
                    }
                    for node in &group.node_ids {
                        reject_locked(graph, node)?;
                    }
                }
            }
        }
        CanvasCommand::DeleteSelection
        | CanvasCommand::GroupSelection { .. }
        | CanvasCommand::UngroupSelection => {
            for node in cx.editor.selection.expanded_nodes(graph) {
                reject_locked(graph, &node)?;
            }
            if matches!(command, CanvasCommand::DeleteSelection) {
                for id in &cx.editor.selection.edges {
                    if let Some(edge) = graph.edges.get(id) {
                        reject_locked(graph, &edge.source.node)?;
                        reject_locked(graph, &edge.target.node)?;
                    }
                }
            }
        }
        CanvasCommand::PreviewConnection { from, hovered, .. } => {
            reject_locked(graph, &from.node)?;
            if hovered
                .as_ref()
                .is_some_and(|p| node_is_locked(graph, &p.node))
            {
                // Clear the current target; returning early would leave an old
                // snapped endpoint live and could connect it on pointer-up.
                *hovered = None;
            }
        }
        CanvasCommand::CompleteConnection { from } => {
            reject_locked(graph, &from.node)?;
            if let Some(target) = cx.connection.target() {
                reject_locked(graph, &target.node)?;
            }
        }
        CanvasCommand::ImportGraph(candidate) => validate_locked_graph_changes(graph, candidate)?,
        CanvasCommand::ImportDocument(candidate) => {
            validate_locked_graph_changes(graph, &candidate.graph)?
        }
        _ => {}
    }
    Ok(())
}

fn validate_locked_graph_changes(before: &Graph, after: &Graph) -> Result<(), String> {
    for node in before
        .nodes
        .values()
        .filter(|n| node_is_locked(before, &n.id))
    {
        let Some(candidate) = after.node(&node.id) else {
            return Err(format!("对象 {} 已锁定，不能删除", node.id));
        };
        let mut candidate = candidate.clone();
        // Unlocking is allowed, but it cannot smuggle another edit into the
        // same transaction. A containing locked Frame still protects the flag.
        if !before
            .groups
            .values()
            .any(|g| g.locked && g.node_ids.contains(&node.id))
        {
            candidate.locked = node.locked;
        }
        if candidate != *node {
            return Err(format!("对象 {} 已锁定，不能编辑", node.id));
        }
        let old_links: Vec<_> = before
            .edges
            .values()
            .filter(|e| e.source.node == node.id || e.target.node == node.id)
            .collect();
        let new_links: Vec<_> = after
            .edges
            .values()
            .filter(|e| e.source.node == node.id || e.target.node == node.id)
            .collect();
        let old_groups: Vec<_> = before
            .groups
            .values()
            .filter(|g| g.node_ids.contains(&node.id))
            .map(|g| &g.id)
            .collect();
        let new_groups: Vec<_> = after
            .groups
            .values()
            .filter(|g| g.node_ids.contains(&node.id))
            .map(|g| &g.id)
            .collect();
        if old_links != new_links || old_groups != new_groups {
            return Err(format!("对象 {} 已锁定，不能修改连接或分组", node.id));
        }
    }
    for group in before.groups.values().filter(|g| g.locked) {
        let Some(candidate) = after.groups.get(&group.id) else {
            return Err("Frame 已锁定，不能删除".into());
        };
        let mut candidate = candidate.clone();
        candidate.locked = group.locked;
        if candidate != *group {
            return Err("Frame 已锁定，不能编辑".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CreativeGuard, standard_registry};
    use gpui_flow_core::{Editor, Point, PortRef};
    use gpui_flow_interaction::PluginHost;
    use std::sync::Arc;

    fn setup() -> (Editor, PluginHost) {
        let registry = Arc::new(standard_registry());
        let mut graph = Graph::new();
        for id in ["a", "b"] {
            graph
                .add_node(registry.make_node("image", id, Point::ZERO, None).unwrap())
                .unwrap();
        }
        graph
            .add_node(
                registry
                    .make_node("generate_image", "gen", Point::new(400., 0.), None)
                    .unwrap(),
            )
            .unwrap();
        graph
            .connect(
                "existing",
                PortRef::new("a", "image"),
                PortRef::new("gen", "reference"),
            )
            .unwrap();
        graph.nodes.get_mut(&"a".into()).unwrap().locked = true;
        let mut host = PluginHost::with_defaults();
        host.install(CreativeGuard(registry)).unwrap();
        (Editor::new(graph).unwrap(), host)
    }

    #[test]
    fn locked_import_cannot_edit_delete_reconnect_or_unlock_and_edit() {
        let (mut editor, mut host) = setup();
        let before = editor.document();
        for operation in 0..4 {
            let mut graph = editor.graph().clone();
            match operation {
                0 => graph.nodes.get_mut(&"a".into()).unwrap().size.width += 10.,
                1 => {
                    graph.remove_node(&"a".into()).unwrap();
                }
                2 => {
                    graph.edges.clear();
                }
                _ => {
                    let n = graph.nodes.get_mut(&"a".into()).unwrap();
                    n.locked = false;
                    n.title = "changed".into();
                }
            }
            let document = gpui_flow_core::FlowDocument {
                graph: graph.clone(),
                viewport: gpui_flow_core::Viewport {
                    pan: Point::new(20., 30.),
                    ..before.viewport
                },
            };
            assert!(
                host.execute(&mut editor, CanvasCommand::ImportDocument(document))
                    .is_err()
            );
            assert!(
                host.execute(&mut editor, CanvasCommand::ImportGraph(graph))
                    .is_err()
            );
            assert_eq!(editor.document(), before);
            assert!(!editor.undo());
        }
        let mut graph = editor.graph().clone();
        graph.nodes.get_mut(&"a".into()).unwrap().locked = false;
        host.execute(&mut editor, CanvasCommand::ImportGraph(graph))
            .unwrap();
        assert!(!editor.graph().nodes[&"a".into()].locked);
        host.execute(&mut editor, CanvasCommand::Undo).unwrap();
        assert_eq!(editor.document(), before);
    }

    #[test]
    fn locked_object_remains_selectable_and_copyable_but_cannot_group_or_delete() {
        let (mut editor, mut host) = setup();
        host.execute(
            &mut editor,
            CanvasCommand::SelectNode {
                node: "a".into(),
                mode: gpui_flow_interaction::SelectionMode::Replace,
            },
        )
        .unwrap();
        host.execute(&mut editor, CanvasCommand::CopySelection)
            .unwrap();
        assert_eq!(editor.copy_selection().nodes.len(), 1);
        assert!(
            host.execute(
                &mut editor,
                CanvasCommand::GroupSelection {
                    title: "Frame".into(),
                    padding: 16.
                }
            )
            .is_err()
        );
        assert!(
            host.execute(&mut editor, CanvasCommand::DeleteSelection)
                .is_err()
        );
        editor.selection.clear();
        editor.selection.edges.insert("existing".into());
        assert!(
            host.execute(&mut editor, CanvasCommand::DeleteSelection)
                .is_err()
        );
        assert_eq!(editor.graph().edges.len(), 1);
    }
    #[test]
    fn hovering_locked_socket_clears_previous_valid_connection_target() {
        let (editor, mut host) = setup();
        let mut graph = editor.graph().clone();
        graph.edges.clear();
        graph.nodes.get_mut(&"gen".into()).unwrap().locked = true;
        graph
            .add_node(
                standard_registry()
                    .make_node("generate_image", "other", Point::new(800., 0.), None)
                    .unwrap(),
            )
            .unwrap();
        let mut editor = Editor::new(graph).unwrap();
        let from = PortRef::new("b", "image");
        host.execute(
            &mut editor,
            CanvasCommand::PreviewConnection {
                from: from.clone(),
                position: Point::ZERO,
                hovered: Some(PortRef::new("other", "reference")),
            },
        )
        .unwrap();
        assert!(host.connection().target().is_some());
        host.execute(
            &mut editor,
            CanvasCommand::PreviewConnection {
                from: from.clone(),
                position: Point::ZERO,
                hovered: Some(PortRef::new("gen", "reference")),
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
                    from: PortRef::new("a", "image"),
                    position: Point::ZERO,
                    hovered: None,
                }
            )
            .is_err()
        );
    }
}

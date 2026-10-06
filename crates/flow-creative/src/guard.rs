use crate::{CreativeRegistry, MAX_CARDS, MAX_LINKS};
use gpui_flow_interaction::{CanvasCommand, CanvasEffect, CanvasPlugin, PluginContext};
use std::sync::Arc;

/// Keeps the creative type contract on Flow's real command path, including
/// keyboard paste and document loading. Direct trusted host writes are not a sandbox.
pub struct CreativeGuard(pub Arc<CreativeRegistry>);
impl CanvasPlugin for CreativeGuard {
    fn id(&self) -> &str {
        "creative-contract"
    }
    fn priority(&self) -> i32 {
        2000
    }
    fn filter_command(
        &mut self,
        command: &mut CanvasCommand,
        cx: &PluginContext<'_>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        crate::layer_guard::filter_layer_command(command, cx)?;
        let graph = cx.editor.graph();
        let registry = crate::custom_nodes::registry_with_custom_definitions(&self.0, graph)
            .map_err(|e| e.to_string())?;
        match command {
            CanvasCommand::AddNode(node) => {
                if graph.nodes.len() >= MAX_CARDS {
                    return Err("最多 128 张卡片".into());
                }
                registry.validate_node(node).map_err(|e| e.to_string())?;
            }
            CanvasCommand::ResizeNode { node, size } => {
                if let Some(current) = graph.node(node)
                    && current.kind == crate::shapes::SHAPE_NODE_KIND
                {
                    let mut candidate = current.clone();
                    candidate.size = *size;
                    crate::shapes::validate_shape_node(&candidate)?;
                }
            }
            CanvasCommand::SetNodeData { node, data } => {
                let mut candidate = graph.node(node).ok_or("card no longer exists")?.clone();
                candidate.data = data.clone();
                registry
                    .validate_node(&candidate)
                    .map_err(|e| e.to_string())?;
            }
            CanvasCommand::ImportGraph(graph) => {
                self.0.validate_graph(graph).map_err(|e| e.to_string())?
            }
            CanvasCommand::LoadDocument(document) => self
                .0
                .validate_document(document)
                .map_err(|e| e.to_string())?,
            CanvasCommand::Paste { clipboard, .. } => {
                clipboard.validate().map_err(|e| e.to_string())?;
                if graph.nodes.len() + clipboard.nodes.len() > MAX_CARDS
                    || graph.edges.len() + clipboard.edges.len() > MAX_LINKS
                    || graph.groups.len() + clipboard.groups.len() > MAX_CARDS
                {
                    return Err("粘贴超出画布容量".into());
                }
                for node in &clipboard.nodes {
                    registry.validate_node(node).map_err(|e| e.to_string())?;
                }
            }
            CanvasCommand::Duplicate { .. } => {
                let clipboard = cx.editor.copy_selection();
                if graph.nodes.len() + clipboard.nodes.len() > MAX_CARDS
                    || graph.edges.len() + clipboard.edges.len() > MAX_LINKS
                    || graph.groups.len() + clipboard.groups.len() > MAX_CARDS
                {
                    return Err("复制超出画布容量".into());
                }
                for node in &clipboard.nodes {
                    registry.validate_node(node).map_err(|e| e.to_string())?;
                }
            }
            CanvasCommand::CompleteConnection { .. } if graph.edges.len() >= MAX_LINKS => {
                return Err("最多 512 条连接".into());
            }
            CanvasCommand::GroupSelection { .. } if graph.groups.len() >= MAX_CARDS => {
                return Err("Frame 数量达到上限".into());
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standard_registry;
    use gpui_flow_core::{Editor, Graph, Group, Point, Rect, Size};
    use gpui_flow_interaction::PluginHost;
    #[test]
    fn duplicate_counts_implicit_frames_from_actual_clipboard() {
        let registry = Arc::new(standard_registry());
        let mut graph = Graph::new();
        graph
            .add_node(
                registry
                    .make_node("text", "one", Point::ZERO, None)
                    .unwrap(),
            )
            .unwrap();
        for index in 0..MAX_CARDS {
            let mut group = Group::new(
                format!("frame-{index}"),
                "Frame",
                Rect::new(Point::ZERO, Size::new(300., 200.)),
            );
            if index == 0 {
                group.node_ids.insert("one".into());
            }
            graph.add_group(group).unwrap();
        }
        let mut editor = Editor::new(graph).unwrap();
        editor.selection.select_node("one", false);
        assert_eq!(editor.copy_selection().groups.len(), 1);
        assert!(editor.selection.groups.is_empty());
        let before = editor.document();
        let selection = editor.selection.clone();
        let mut host = PluginHost::with_defaults();
        host.install(CreativeGuard(registry)).unwrap();
        assert!(
            host.execute(
                &mut editor,
                CanvasCommand::Duplicate {
                    offset: Point::new(24., 24.)
                }
            )
            .is_err()
        );
        assert_eq!(editor.document(), before);
        assert_eq!(editor.selection, selection);
        assert!(!editor.undo());
    }
}

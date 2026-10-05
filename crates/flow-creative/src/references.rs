//! One image reference per generator, either an existing canvas edge or a
//! project-owned attachment. Attachments never create invisible graph nodes.
use crate::CreativeError;
#[cfg(any(feature = "desktop", test))]
use crate::CreativeRegistry;
use gpui_flow_core::{DataType, Graph, Node, NodeId, PortDirection};
#[cfg(any(feature = "desktop", test))]
use serde_json::{Value, json};

pub(crate) fn accepts_image_reference(node: &Node) -> bool {
    node.ports.iter().any(|port| {
        port.id.as_str() == "reference"
            && port.direction == PortDirection::Input
            && port.data_type == DataType::Named("image".into())
    })
}
pub(crate) fn reference_asset(node: &Node) -> Option<&str> {
    node.data["settings"]["reference_asset"]
        .as_str()
        .filter(|key| !key.is_empty())
}
pub(crate) fn linked_reference<'a>(graph: &'a Graph, target: &NodeId) -> Option<&'a Node> {
    graph
        .edges
        .values()
        .find(|edge| edge.target.node == *target && edge.target.port.as_str() == "reference")
        .and_then(|edge| graph.node(&edge.source.node))
}
pub(crate) fn validate_reference_binding(graph: &Graph, node: &Node) -> Result<(), CreativeError> {
    if reference_asset(node).is_some()
        && accepts_image_reference(node)
        && linked_reference(graph, &node.id).is_some()
    {
        return Err(CreativeError(
            "参考图已有本地附件，请先移除附件再连接图片".into(),
        ));
    }
    Ok(())
}
#[cfg(any(feature = "desktop", test))]
pub(crate) fn replace_reference(
    registry: &CreativeRegistry,
    graph: &Graph,
    target: &NodeId,
    asset: Option<&str>,
) -> Result<Graph, CreativeError> {
    let mut result = graph.clone();
    let node = result
        .nodes
        .get_mut(target)
        .ok_or_else(|| CreativeError("参考图目标已不存在".into()))?;
    if node.locked || !accepts_image_reference(node) {
        return Err(CreativeError("此节点不接受可编辑的图片参考".into()));
    }
    let data = node
        .data
        .as_object_mut()
        .ok_or_else(|| CreativeError("invalid generation data".into()))?;
    let settings = data.entry("settings").or_insert_with(|| json!({}));
    let settings = settings
        .as_object_mut()
        .ok_or_else(|| CreativeError("invalid generation settings".into()))?;
    if let Some(asset) = asset.filter(|key| !key.is_empty()) {
        settings.insert("reference_asset".into(), Value::String(asset.into()));
    } else {
        settings.remove("reference_asset");
    }
    result.edges.retain(|_, edge| {
        !(edge.target.node == *target && edge.target.port.as_str() == "reference")
    });
    registry.validate_graph(&result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GenerationRequest, standard_registry};
    use gpui_flow_core::{Point, PortRef};
    #[test]
    fn attachment_replaces_only_the_reference_edge_without_deleting_canvas_objects() {
        let registry = standard_registry();
        let mut graph = Graph::new();
        for (kind, id) in [
            ("image", "photo"),
            ("text", "brief"),
            ("generate_image", "gen"),
        ] {
            graph
                .add_node(registry.make_node(kind, id, Point::ZERO, None).unwrap())
                .unwrap();
        }
        graph
            .connect(
                "image-ref",
                PortRef::new("photo", "image"),
                PortRef::new("gen", "reference"),
            )
            .unwrap();
        graph
            .connect(
                "prompt",
                PortRef::new("brief", "text"),
                PortRef::new("gen", "prompt"),
            )
            .unwrap();
        let next = replace_reference(&registry, &graph, &"gen".into(), Some("assets/a-local.png"))
            .unwrap();
        assert_eq!(graph.edges.len(), 2);
        assert_eq!(next.nodes.len(), 3);
        assert_eq!(next.edges.len(), 1);
        assert!(next.edges.contains_key(&"prompt".into()));
        let node = next.node(&"gen".into()).unwrap();
        assert_eq!(reference_asset(node), Some("assets/a-local.png"));
        assert_eq!(
            GenerationRequest::from_node(node, &next).unwrap().settings["reference_asset"],
            "assets/a-local.png"
        );
        let cleared = replace_reference(&registry, &next, &node.id, None).unwrap();
        assert_eq!(cleared.nodes, graph.nodes);
        assert_eq!(cleared.edges.len(), 1);
    }
    #[test]
    fn attachment_and_connected_reference_cannot_double_fill_the_input() {
        let registry = standard_registry();
        let mut graph = Graph::new();
        graph
            .add_node(
                registry
                    .make_node("image", "photo", Point::ZERO, None)
                    .unwrap(),
            )
            .unwrap();
        graph
            .add_node(
                registry
                    .make_node("generate_video", "gen", Point::ZERO, None)
                    .unwrap(),
            )
            .unwrap();
        let mut attached =
            replace_reference(&registry, &graph, &"gen".into(), Some("assets/a-local.png"))
                .unwrap();
        attached
            .connect(
                "ref",
                PortRef::new("photo", "image"),
                PortRef::new("gen", "reference"),
            )
            .unwrap();
        assert!(registry.validate_graph(&attached).is_err());
        let audio = registry
            .make_node("generate_audio", "audio", Point::ZERO, None)
            .unwrap();
        graph.add_node(audio).unwrap();
        assert!(
            replace_reference(
                &registry,
                &graph,
                &"audio".into(),
                Some("assets/a-local.png")
            )
            .is_err()
        );
        graph.nodes.get_mut(&"gen".into()).unwrap().locked = true;
        assert!(replace_reference(&registry, &graph, &"gen".into(), None).is_err());
    }
}

//! Image references are visible canvas nodes connected to a generator's existing
//! `reference` input. Hidden attachments are read only for local-project migration.
use crate::{CreativeError, CreativeRegistry, GenerationKind, ProjectAsset};
#[cfg(any(feature = "desktop", test))]
use gpui_flow_core::Graph;
use gpui_flow_core::{DataType, FlowDocument, Node, NodeId, Point, PortDirection, PortRef, Size};
use serde_json::json;

pub(crate) fn accepts_image_reference(node: &Node) -> bool {
    node.ports.iter().any(|port| {
        port.id.as_str() == "reference"
            && port.direction == PortDirection::Input
            && port.data_type == DataType::Named("image".into())
    })
}

/// Legacy lookup for the project reader and its home-card thumbnail only.
pub(crate) fn reference_asset(node: &Node) -> Option<&str> {
    node.data["settings"]["reference_asset"]
        .as_str()
        .filter(|key| !key.is_empty())
}

pub(crate) fn has_legacy_reference(node: &Node) -> bool {
    GenerationKind::from_node_kind(&node.kind).is_some()
        && node
            .data
            .get("settings")
            .and_then(|settings| settings.get("reference_asset"))
            .is_some()
}

pub(crate) fn reject_legacy_reference(node: &Node) -> Result<(), CreativeError> {
    if has_legacy_reference(node) {
        return Err(CreativeError(format!(
            "{}: 检测到旧版隐藏参考图，请从本地项目列表打开原项目，将它转换为图片输入节点；独立文件请先在原项目中转换后重新导出",
            node.id
        )));
    }
    Ok(())
}

#[cfg(any(feature = "desktop", test))]
pub(crate) fn linked_reference<'a>(graph: &'a Graph, target: &NodeId) -> Option<&'a Node> {
    graph
        .edges
        .values()
        .find(|edge| edge.target.node == *target && edge.target.port.as_str() == "reference")
        .and_then(|edge| graph.node(&edge.source.node))
}

/// Build a complete candidate in memory. The caller must validate the owned
/// image bytes first and atomically persist this document only after success.
/// No graph, asset, or filesystem state is changed on failure.
pub(crate) fn migrate_legacy_references(
    registry: &CreativeRegistry,
    document: &FlowDocument,
    assets: &[ProjectAsset],
) -> Result<(FlowDocument, usize), CreativeError> {
    let mut candidate = document.clone();
    let mut converted = 0;
    for legacy in document
        .graph
        .nodes
        .values()
        .filter(|node| has_legacy_reference(node))
    {
        let key = legacy.data["settings"]["reference_asset"]
            .as_str()
            .ok_or_else(|| CreativeError("旧版参考图资产键必须是字符串".into()))?;
        if !key.is_empty() {
            if !matches!(
                GenerationKind::from_node_kind(&legacy.kind),
                Some(GenerationKind::Image | GenerationKind::Video | GenerationKind::Model)
            ) || !accepts_image_reference(legacy)
            {
                return Err(CreativeError(format!(
                    "{}: 此节点不接受图片输入，旧版参考图无法转换",
                    legacy.id
                )));
            }
            if candidate.graph.edges.values().any(|edge| {
                edge.target.node == legacy.id && edge.target.port.as_str() == "reference"
            }) {
                return Err(CreativeError(format!(
                    "{}: 旧版参考图与现有图片输入连线冲突，原项目已保留，请先解决冲突",
                    legacy.id
                )));
            }
            let asset = assets
                .iter()
                .find(|asset| asset.key == key && asset.media_type.starts_with("image/"))
                .ok_or_else(|| {
                    CreativeError(format!("旧版参考图必须是本项目已有的图片素材: {key}"))
                })?;
            if asset.width == 0 || asset.height == 0 {
                return Err(CreativeError("旧版参考图尺寸无效".into()));
            }
            let source = if let Some(source) =
                candidate.graph.nodes.values().find(|source| {
                    source.kind == "image" && source.data["asset"].as_str() == Some(key)
                }) {
                source.id.clone()
            } else {
                let id = (0..)
                    .map(|index| NodeId::from(format!("reference-image-{index}")))
                    .find(|id| !candidate.graph.nodes.contains_key(id))
                    .expect("bounded graph has an unused node ID");
                let width = asset.width as f32;
                let height = asset.height as f32;
                let scale = 420. / width.max(height);
                let size = Size::new(width * scale, height * scale);
                let mut node = registry.make_node(
                    "image",
                    id.clone(),
                    Point::new(legacy.position.x - size.width - 96., legacy.position.y),
                    Some(json!({"asset": key, "caption": asset.name})),
                )?;
                node.title = asset.name.chars().take(80).collect();
                node.size = size;
                // Keep the source to the left, below any objects occupying its
                // intended space. Each pass moves past at least one object.
                for _ in 0..=candidate.graph.nodes.len() {
                    let bottom = candidate
                        .graph
                        .nodes
                        .values()
                        .filter(|other| node.bounds().expanded(24.).intersects(other.bounds()))
                        .map(|other| other.bounds().max_y() + 48.)
                        .max_by(f32::total_cmp);
                    match bottom {
                        Some(y) => node.position.y = y,
                        None => break,
                    }
                }
                candidate.graph.add_node(node)?;
                id
            };
            let edge = (0..)
                .map(|index| gpui_flow_core::EdgeId::from(format!("reference-input-{index}")))
                .find(|id| !candidate.graph.edges.contains_key(id))
                .expect("bounded graph has an unused edge ID");
            candidate.graph.connect(
                edge,
                PortRef::new(source, "image"),
                PortRef::new(legacy.id.clone(), "reference"),
            )?;
            converted += 1;
        }
        candidate
            .graph
            .nodes
            .get_mut(&legacy.id)
            .and_then(|node| node.data.get_mut("settings"))
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| CreativeError("旧版生成设置无效".into()))?
            .remove("reference_asset");
    }
    registry.validate_document(&candidate)?;
    Ok((candidate, converted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_node_metadata_is_not_interpreted_as_a_generator_attachment() {
        let registry = crate::standard_registry();
        let mut document = FlowDocument::default();
        let mut node = registry
            .make_node("image", "host-image", Point::ZERO, None)
            .unwrap();
        node.data["settings"] = json!({"reference_asset": "host-owned-setting"});
        document.graph.add_node(node).unwrap();
        let (migrated, count) = migrate_legacy_references(&registry, &document, &[]).unwrap();
        assert_eq!(count, 0);
        assert_eq!(migrated, document);
    }

    #[test]
    fn migration_preserves_image_ratio_and_existing_port_contracts() {
        let registry = crate::standard_registry();
        let mut document = FlowDocument::default();
        let mut generator = registry
            .make_node("generate_image", "gen", Point::new(900., 80.), None)
            .unwrap();
        generator.data["settings"]["reference_asset"] = json!("assets/landscape.png");
        let ports = generator.ports.clone();
        document.graph.add_node(generator).unwrap();
        let asset = ProjectAsset {
            key: "assets/landscape.png".into(),
            name: "Landscape".into(),
            media_type: "image/png".into(),
            byte_len: 100,
            width: 1600,
            height: 900,
        };
        let (migrated, count) = migrate_legacy_references(&registry, &document, &[asset]).unwrap();
        let generator = migrated.graph.node(&"gen".into()).unwrap();
        let source = linked_reference(&migrated.graph, &generator.id).unwrap();
        assert_eq!(count, 1);
        assert_eq!(generator.ports, ports);
        assert_eq!(source.kind, "image");
        assert!((source.size.width / source.size.height - 16. / 9.).abs() < 0.0001);
        assert!(source.bounds().max_x() < generator.position.x);
        assert!(!has_legacy_reference(generator));
        assert!(has_legacy_reference(
            document.graph.node(&"gen".into()).unwrap()
        ));
    }
}

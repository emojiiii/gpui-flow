//! Typed mask-node payload and source provenance, separate from the editor UI.
use crate::mask::{MaskSource, MaskState, MaskStroke};
use crate::{CreativeError, CreativeRegistry, NodeDefinition, ProjectAsset};
use gpui_flow_core::{DataType, Graph, Node, Port, Size};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MASK_MEDIA_TYPE: &str = "application/vnd.gpui-flow.mask+png";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskNodeData {
    pub state: MaskState,
    pub revision: u64,
    pub asset: String,
    pub output_revision: Option<u64>,
}

impl MaskNodeData {
    pub fn from_value(value: &Value) -> Result<Self, CreativeError> {
        let data: Self = serde_json::from_value(value.clone())?;
        data.validate()?;
        Ok(data)
    }
    pub fn validate(&self) -> Result<(), CreativeError> {
        self.state
            .validate()
            .map_err(|e| CreativeError(e.to_string()))?;
        if self.asset.is_empty() {
            if self.output_revision.is_some() {
                return Err(CreativeError("遮罩输出版本缺少对应素材".into()));
            }
        } else {
            if !self.asset.ends_with(".png")
                || self.output_revision != Some(self.revision)
                || self.state.source.is_none()
            {
                return Err(CreativeError("遮罩输出与当前笔画版本不一致".into()));
            }
            MaskSource::new(&self.asset, 1, 1).map_err(|e| CreativeError(e.to_string()))?;
        }
        Ok(())
    }
    fn changed(&mut self) -> Result<(), CreativeError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| CreativeError("遮罩编辑版本已达上限".into()))?;
        self.asset.clear();
        self.output_revision = None;
        Ok(())
    }
    pub fn reset_source(&mut self, source: MaskSource) -> Result<(), CreativeError> {
        let mut candidate = self.clone();
        candidate
            .state
            .reset_source(source)
            .map_err(|e| CreativeError(e.to_string()))?;
        candidate.changed()?;
        *self = candidate;
        Ok(())
    }
    pub fn push_stroke(&mut self, stroke: MaskStroke) -> Result<(), CreativeError> {
        let mut candidate = self.clone();
        candidate
            .state
            .push_stroke(stroke)
            .map_err(|e| CreativeError(e.to_string()))?;
        candidate.changed()?;
        *self = candidate;
        Ok(())
    }
    pub fn output_is_current(&self, source: &MaskSource) -> bool {
        !self.asset.is_empty()
            && self.output_revision == Some(self.revision)
            && self.state.source_matches(source)
    }
}

pub fn mask_source(
    graph: &Graph,
    node: &Node,
    assets: &[ProjectAsset],
) -> Result<MaskSource, CreativeError> {
    let edge = graph
        .edges
        .values()
        .find(|e| e.target.node == node.id && e.target.port.as_str() == "image")
        .ok_or_else(|| CreativeError("请先连接一个图片节点".into()))?;
    let value = crate::custom_nodes::resolve_graph_output(graph, &edge.source)?;
    value.validate_for(&DataType::Named("image".into()))?;
    let key = value
        .asset_key()
        .ok_or_else(|| CreativeError("图片输入尚无素材".into()))?;
    let asset = assets
        .iter()
        .find(|asset| asset.key == key)
        .ok_or_else(|| CreativeError("遮罩只编辑当前项目的本地图片".into()))?;
    if !matches!(asset.media_type.as_str(), "image/png" | "image/jpeg") {
        return Err(CreativeError("遮罩编辑当前支持静态 PNG/JPEG 图片".into()));
    }
    MaskSource::new(key, asset.width, asset.height).map_err(|e| CreativeError(e.to_string()))
}

/// A generation request must never use an old raster after its image input or
/// strokes changed. The project validator also checks the mask PNG's manifest.
pub(crate) fn validate_edit_inputs(node: &Node, graph: &Graph) -> Result<(), CreativeError> {
    if node.kind != "edit_image" {
        return Ok(());
    }
    let image_edge = graph
        .edges
        .values()
        .find(|e| e.target.node == node.id && e.target.port.as_str() == "image")
        .ok_or_else(|| CreativeError("图片编辑需要 Image 输入".into()))?;
    let image = crate::custom_nodes::resolve_graph_output(graph, &image_edge.source)?;
    image.validate_for(&DataType::Named("image".into()))?;
    let mask_edge = graph
        .edges
        .values()
        .find(|e| e.target.node == node.id && e.target.port.as_str() == "mask")
        .ok_or_else(|| CreativeError("图片编辑需要独立 Mask 输入".into()))?;
    let mask = crate::custom_nodes::resolve_graph_output(graph, &mask_edge.source)?;
    let crate::custom_nodes::CustomValue::Mask { source, .. } = mask else {
        return Err(CreativeError("Mask 输入必须是可验证的独立遮罩输出".into()));
    };
    if image.asset_key() != Some(source.asset_key.as_str()) {
        return Err(CreativeError("Mask 与 Image 输入的原图身份不一致".into()));
    }

    Ok(())
}

pub(crate) fn register_mask_nodes(registry: &mut CreativeRegistry) {
    registry
        .register(NodeDefinition::new(
            "mask",
            "遮罩",
            Size::new(400., 300.),
            vec![
                Port::input("image", "原图", DataType::Named("image".into())),
                Port::output("mask", "遮罩", DataType::Named("mask".into())),
            ],
            json!(MaskNodeData::default()),
            |value| {
                MaskNodeData::from_value(value)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
        ))
        .expect("valid mask node definition");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mask::{MaskMode, MaskPoint};
    use gpui_flow_core::{Point, PortRef};
    #[test]
    fn editing_a_published_mask_clears_its_output_without_losing_undo_data() {
        let source = MaskSource::new("assets/source.png", 10, 20).unwrap();
        let mut data = MaskNodeData::default();
        data.reset_source(source.clone()).unwrap();
        data.asset = "assets/mask.png".into();
        data.output_revision = Some(data.revision);
        let before = data.clone();
        data.push_stroke(
            MaskStroke::new(MaskMode::Paint, 0.1, vec![MaskPoint(32000, 32000)]).unwrap(),
        )
        .unwrap();
        assert!(data.asset.is_empty());
        assert!(!data.output_is_current(&source));
        assert!(before.output_is_current(&source));
        let changed = MaskSource::new("assets/replaced.png", 10, 20).unwrap();
        assert!(!before.output_is_current(&changed));
    }
    #[test]
    fn image_edit_requires_a_distinct_current_mask_of_the_same_image() {
        let registry = crate::standard_registry();
        let mut graph = Graph::new();
        let mut image = registry
            .make_node("image", "image", Point::ZERO, None)
            .unwrap();
        image.data["asset"] = json!("assets/original.png");
        graph.add_node(image).unwrap();
        let mut mask = registry
            .make_node("mask", "mask", Point::ZERO, None)
            .unwrap();
        let mut data = MaskNodeData::default();
        data.reset_source(MaskSource::new("assets/original.png", 10, 20).unwrap())
            .unwrap();
        data.asset = "assets/output.png".into();
        data.output_revision = Some(data.revision);
        mask.data = json!(data);
        graph.add_node(mask).unwrap();
        let mut edit = registry
            .make_node("edit_image", "edit", Point::ZERO, None)
            .unwrap();
        edit.data["prompt"] = json!("change this area");
        graph.add_node(edit).unwrap();
        assert!(
            graph
                .connect(
                    "wrong",
                    PortRef::new("image", "image"),
                    PortRef::new("edit", "mask")
                )
                .is_err()
        );
        graph
            .connect(
                "source",
                PortRef::new("image", "image"),
                PortRef::new("mask", "image"),
            )
            .unwrap();
        graph
            .connect(
                "image-in",
                PortRef::new("image", "image"),
                PortRef::new("edit", "image"),
            )
            .unwrap();
        graph
            .connect(
                "mask-in",
                PortRef::new("mask", "mask"),
                PortRef::new("edit", "mask"),
            )
            .unwrap();
        let request =
            crate::GenerationRequest::from_node(graph.node(&"edit".into()).unwrap(), &graph)
                .unwrap();
        assert!(request.inputs.iter().any(
            |input| input.port == "mask" && input.asset.as_deref() == Some("assets/output.png")
        ));
        graph.nodes.get_mut(&"image".into()).unwrap().data["asset"] =
            json!("assets/replacement.png");
        assert!(
            crate::GenerationRequest::from_node(graph.node(&"edit".into()).unwrap(), &graph)
                .is_err()
        );
    }
}

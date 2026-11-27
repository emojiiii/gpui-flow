//! Typed mask-node payload and source provenance, separate from the editor UI.
use crate::mask::{MaskSource, MaskState, MaskStroke};
use crate::tr;
use crate::{CreativeError, CreativeRegistry, NodeDefinition, ProjectAsset};
use gpui_flow_core::{DataType, Graph, Node, Port, Size};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MASK_MEDIA_TYPE: &str = "application/vnd.gpui-flow.mask+png";
pub(crate) const ORIENTATION_REVIEW_REQUIRED: &str = "orientation_review_required";

pub(crate) fn orientation_review_notice() -> &'static str {
    tr(
        "原图显示方向已修正，旧笔画仍保留；请重置遮罩后重新绘制",
        "The source image orientation was corrected. Reset the mask before drawing new strokes.",
    )
}

/// This migration-only extension deliberately stays outside the editable typed
/// payload. Only an explicit reset may serialize that payload back without it.
pub(crate) fn orientation_review_required(value: &Value) -> Result<bool, CreativeError> {
    match value.get(ORIENTATION_REVIEW_REQUIRED) {
        None => Ok(false),
        Some(Value::Bool(required)) => Ok(*required),
        Some(_) => Err(CreativeError(
            tr(
                "遮罩方向复核标记必须为布尔值",
                "The mask orientation review flag must be a boolean",
            )
            .into(),
        )),
    }
}

pub(crate) fn ensure_orientation_reviewed(value: &Value) -> Result<(), CreativeError> {
    if orientation_review_required(value)? {
        return Err(CreativeError(orientation_review_notice().into()));
    }
    Ok(())
}

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
        orientation_review_required(value)?;
        let mut payload = value.clone();
        if let Some(object) = payload.as_object_mut() {
            object.remove(ORIENTATION_REVIEW_REQUIRED);
        }
        let data: Self = serde_json::from_value(payload)?;
        data.validate()?;
        Ok(data)
    }
    pub fn validate(&self) -> Result<(), CreativeError> {
        self.state
            .validate()
            .map_err(|e| CreativeError(e.to_string()))?;
        if self.asset.is_empty() {
            if self.output_revision.is_some() {
                return Err(CreativeError(
                    tr(
                        "遮罩输出版本缺少对应素材",
                        "The mask output revision has no asset",
                    )
                    .into(),
                ));
            }
        } else {
            if !self.asset.ends_with(".png")
                || self.output_revision != Some(self.revision)
                || self.state.source.is_none()
            {
                return Err(CreativeError(
                    tr(
                        "遮罩输出与当前笔画版本不一致",
                        "The mask output revision does not match the current strokes",
                    )
                    .into(),
                ));
            }
            MaskSource::new(&self.asset, 1, 1).map_err(|e| CreativeError(e.to_string()))?;
        }
        Ok(())
    }
    fn changed(&mut self) -> Result<(), CreativeError> {
        self.revision = self.revision.checked_add(1).ok_or_else(|| {
            CreativeError(
                tr(
                    "遮罩编辑版本已达上限",
                    "The mask edit revision has reached its limit",
                )
                .into(),
            )
        })?;
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
        .ok_or_else(|| {
            CreativeError(tr("请先连接一个图片节点", "Connect an image node first").into())
        })?;
    let value = crate::custom_nodes::resolve_graph_output(graph, &edge.source)?;
    value.validate_for(&DataType::Named("image".into()))?;
    let key = value.asset_key().ok_or_else(|| {
        CreativeError(tr("图片输入尚无素材", "The image input has no asset yet").into())
    })?;
    let asset = assets
        .iter()
        .find(|asset| asset.key == key)
        .ok_or_else(|| {
            CreativeError(
                tr(
                    "遮罩只编辑当前项目的本地图片",
                    "Mask editing requires a local image from this project",
                )
                .into(),
            )
        })?;
    if !matches!(asset.media_type.as_str(), "image/png" | "image/jpeg") {
        return Err(CreativeError(
            tr(
                "遮罩编辑当前支持静态 PNG/JPEG 图片",
                "Mask editing supports static PNG and JPEG images",
            )
            .into(),
        ));
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
        .ok_or_else(|| {
            CreativeError(
                tr(
                    "图片编辑需要 Image 输入",
                    "Image editing requires an Image input",
                )
                .into(),
            )
        })?;
    let image = crate::custom_nodes::resolve_graph_output(graph, &image_edge.source)?;
    image.validate_for(&DataType::Named("image".into()))?;
    let mask_edge = graph
        .edges
        .values()
        .find(|e| e.target.node == node.id && e.target.port.as_str() == "mask")
        .ok_or_else(|| {
            CreativeError(
                tr(
                    "图片编辑需要独立 Mask 输入",
                    "Image editing requires a separate Mask input",
                )
                .into(),
            )
        })?;
    let mask = crate::custom_nodes::resolve_graph_output(graph, &mask_edge.source)?;
    let crate::custom_nodes::CustomValue::Mask { source, .. } = mask else {
        return Err(CreativeError(
            tr(
                "Mask 输入必须是可验证的独立遮罩输出",
                "The Mask input must come from a verifiable standalone mask",
            )
            .into(),
        ));
    };
    if image.asset_key() != Some(source.asset_key.as_str()) {
        return Err(CreativeError(
            tr(
                "Mask 与 Image 输入的原图身份不一致",
                "The Mask and Image inputs refer to different source images",
            )
            .into(),
        ));
    }

    Ok(())
}

pub(crate) fn register_mask_nodes(registry: &mut CreativeRegistry) {
    registry
        .register(NodeDefinition::new(
            "mask",
            tr("遮罩", "Mask"),
            Size::new(400., 300.),
            vec![
                Port::input(
                    "image",
                    tr("原图", "Source image"),
                    DataType::Named("image".into()),
                ),
                Port::output("mask", tr("遮罩", "Mask"), DataType::Named("mask".into())),
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
    fn mask_payload_accepts_only_the_boolean_orientation_extension() {
        let original = MaskNodeData::default();
        for required in [false, true] {
            let mut value = json!(original);
            value[ORIENTATION_REVIEW_REQUIRED] = json!(required);
            assert_eq!(MaskNodeData::from_value(&value).unwrap(), original);
            assert_eq!(orientation_review_required(&value).unwrap(), required);
            assert_eq!(ensure_orientation_reviewed(&value).is_err(), required);
            value["unrecognized_extension"] = json!(true);
            assert!(MaskNodeData::from_value(&value).is_err());
        }
        for invalid in [Value::Null, json!(0), json!("true"), json!({}), json!([])] {
            let mut value = json!(original);
            value[ORIENTATION_REVIEW_REQUIRED] = invalid;
            assert!(MaskNodeData::from_value(&value).is_err());
            assert!(ensure_orientation_reviewed(&value).is_err());
        }
    }
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
        graph.nodes.get_mut(&"mask".into()).unwrap().data[ORIENTATION_REVIEW_REQUIRED] =
            json!(true);
        assert!(
            crate::custom_nodes::resolve_node_output(graph.node(&"mask".into()).unwrap(), "mask")
                .is_err()
        );
        assert!(
            crate::GenerationRequest::from_node(graph.node(&"edit".into()).unwrap(), &graph)
                .is_err()
        );
        graph
            .nodes
            .get_mut(&"mask".into())
            .unwrap()
            .data
            .as_object_mut()
            .unwrap()
            .remove(ORIENTATION_REVIEW_REQUIRED);
        graph.nodes.get_mut(&"image".into()).unwrap().data["asset"] =
            json!("assets/replacement.png");
        assert!(
            crate::GenerationRequest::from_node(graph.node(&"edit".into()).unwrap(), &graph)
                .is_err()
        );
    }
}

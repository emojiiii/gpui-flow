//! Host-facing generation requests. This crate never embeds credentials or calls a service.
use crate::{CreativeError, CreativeRegistry, NodeDefinition};
use gpui_flow_core::{DataType, Graph, Node, NodeId, Port, Size};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationKind {
    Image,
    Video,
    Audio,
    Model,
}
impl GenerationKind {
    pub fn node_kind(self) -> &'static str {
        match self {
            Self::Image => "generate_image",
            Self::Video => "generate_video",
            Self::Audio => "generate_audio",
            Self::Model => "generate_model",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Image => "图片生成",
            Self::Video => "视频生成",
            Self::Audio => "音频生成",
            Self::Model => "模型生成",
        }
    }
    pub fn output_type(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Model => "model",
        }
    }
    pub fn from_node_kind(kind: &str) -> Option<Self> {
        if kind == "edit_image" {
            return Some(Self::Image);
        }
        [Self::Image, Self::Video, Self::Audio, Self::Model]
            .into_iter()
            .find(|k| k.node_kind() == kind)
    }
}
/// A snapshot for the explicitly connected host. Asset keys must be resolved through
/// the current project's manifest. A graph connection never implicitly executes work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationRequest {
    pub node_id: NodeId,
    pub kind: GenerationKind,
    pub prompt: String,
    pub model: String,
    /// The only source of reference images: explicit incoming canvas edges.
    pub inputs: Vec<GenerationInput>,
    pub settings: Value,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationInput {
    pub port: String,
    pub source: NodeId,
    pub asset: Option<String>,
    pub text: Option<String>,
}
impl GenerationRequest {
    pub fn from_node(node: &Node, graph: &Graph) -> Result<Self, CreativeError> {
        let kind = GenerationKind::from_node_kind(&node.kind)
            .ok_or_else(|| CreativeError("此节点不是生成节点".into()))?;
        crate::references::reject_legacy_reference(node)?;
        graph.validate()?;
        crate::mask_node::validate_edit_inputs(node, graph)?;
        for edge in graph
            .edges
            .values()
            .filter(|edge| edge.target.node == node.id)
        {
            if let Some(source) = graph.node(&edge.source.node)
                && crate::processing::ProcessingKind::from_node_kind(&source.kind).is_some()
            {
                crate::processing_node::processing_output(graph, source)?;
            }
        }
        let prompt = node.data["prompt"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        let inputs = graph
            .edges
            .values()
            .filter(|e| e.target.node == node.id)
            .filter_map(|e| {
                let source = graph.node(&e.source.node)?;
                Some(GenerationInput {
                    port: e.target.port.to_string(),
                    source: source.id.clone(),
                    asset: source.data["asset"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                    text: source.data["text"].as_str().map(str::to_string),
                })
            })
            .collect::<Vec<_>>();
        if prompt.is_empty()
            && !inputs
                .iter()
                .any(|i| i.text.as_ref().is_some_and(|t| !t.trim().is_empty()))
        {
            return Err(CreativeError("请先填写提示词或连接文字节点".into()));
        }
        Ok(Self {
            node_id: node.id.clone(),
            kind,
            prompt,
            model: node.data["model"].as_str().unwrap_or_default().to_string(),
            inputs,
            settings: node
                .data
                .get("settings")
                .cloned()
                .unwrap_or_else(|| json!({})),
        })
    }
}

pub(crate) fn register_generation_nodes(registry: &mut CreativeRegistry) {
    for kind in [
        GenerationKind::Image,
        GenerationKind::Video,
        GenerationKind::Audio,
        GenerationKind::Model,
    ] {
        let mut ports = vec![Port::input("prompt", "提示词", DataType::String)];
        if kind != GenerationKind::Audio {
            ports.push(Port::input(
                "reference",
                "参考图",
                DataType::Named("image".into()),
            ));
        }
        ports.push(Port::output(
            "output",
            kind.title(),
            DataType::Named(kind.output_type().into()),
        ));
        registry
            .register(NodeDefinition::new(
                kind.node_kind(),
                kind.title(),
                Size::new(
                    if kind == GenerationKind::Video { 480. } else { 400. },
                    if kind == GenerationKind::Audio {
                        160.
                    } else if kind == GenerationKind::Video {
                        270.
                    } else if kind == GenerationKind::Image {
                        400.
                    } else {
                        300.
                    },
                ),
                ports,
                json!({"prompt":"", "model":"", "status":"idle", "asset":"", "settings":{"aspect_ratio":if kind == GenerationKind::Video { "16:9" } else { "1:1" },"duration_seconds":5}}),
                move |v| {
                    for key in ["prompt", "model", "status", "asset"] {
                        let text = v
                            .get(key)
                            .and_then(Value::as_str)
                            .ok_or_else(|| format!("{key} must be a string"))?;
                        if text.len() > 16 * 1024 {
                            return Err(format!("{key} is too long"));
                        }
                    }
                    if !matches!(
                        v["status"].as_str(),
                        Some("idle" | "queued" | "running" | "complete" | "failed")
                    ) {
                        return Err("invalid generation status".into());
                    }
                    if let Some(settings)=v.get("settings") {
                        if !settings.is_object() { return Err("settings must be an object".into()); }
                        if settings.get("aspect_ratio").is_some_and(|ratio| !matches!(ratio.as_str(),Some("1:1"|"16:9"|"9:16"))) { return Err("unsupported aspect ratio".into()); }
                        if settings.get("duration_seconds").is_some_and(|duration| duration.as_u64().is_none_or(|value| value==0 || value>300)) { return Err("duration must be 1..300 seconds".into()); }
                    }
                    Ok(())
                },
            ))
            .expect("valid generation definition");
    }
    let mut edit = registry
        .definition("generate_image")
        .expect("registered image generator")
        .clone();
    edit.kind = "edit_image".into();
    edit.title = "图片编辑".into();
    edit.ports = vec![
        Port::input("prompt", "提示词", DataType::String),
        Port::input("image", "Image", DataType::Named("image".into())),
        Port::input("mask", "Mask", DataType::Named("mask".into())),
        Port::output("output", "图片", DataType::Named("image".into())),
    ];
    registry
        .register(edit)
        .expect("valid image-edit definition");
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_core::{Point, PortRef};
    #[test]
    fn four_generation_kinds_have_distinct_output_contracts() {
        let registry = crate::standard_registry();
        for kind in [
            GenerationKind::Image,
            GenerationKind::Video,
            GenerationKind::Audio,
            GenerationKind::Model,
        ] {
            let node = registry
                .make_node(kind.node_kind(), "gen", Point::ZERO, None)
                .unwrap();
            assert_eq!(
                node.ports.last().unwrap().data_type,
                DataType::Named(kind.output_type().into())
            );
            assert!(GenerationRequest::from_node(&node, &Graph::new()).is_err());
        }
    }
    #[test]
    fn request_captures_linked_text_and_explicit_prompt_without_execution() {
        let registry = crate::standard_registry();
        let mut graph = Graph::new();
        graph
            .add_node(
                registry
                    .make_node("text", "brief", Point::ZERO, None)
                    .unwrap(),
            )
            .unwrap();
        graph
            .add_node(
                registry
                    .make_node("generate_image", "gen", Point::ZERO, None)
                    .unwrap(),
            )
            .unwrap();
        graph
            .connect(
                "link",
                PortRef::new("brief", "text"),
                PortRef::new("gen", "prompt"),
            )
            .unwrap();
        let request =
            GenerationRequest::from_node(graph.node(&"gen".into()).unwrap(), &graph).unwrap();
        assert_eq!(request.kind, GenerationKind::Image);
        assert_eq!(request.inputs[0].source, NodeId::from("brief"));
    }
    #[test]
    fn reference_images_come_only_from_explicit_edges_and_text_to_image_needs_none() {
        let registry = crate::standard_registry();
        let mut graph = Graph::new();
        let mut generator = registry
            .make_node("generate_image", "gen", Point::ZERO, None)
            .unwrap();
        generator.data["prompt"] = json!("A quiet garden");
        graph.add_node(generator).unwrap();
        for (id, asset) in [
            ("photo", "assets/input.png"),
            ("unconnected", "assets/unused.png"),
        ] {
            graph
                .add_node(
                    registry
                        .make_node(
                            "image",
                            id,
                            Point::ZERO,
                            Some(json!({"asset":asset,"caption":"Image"})),
                        )
                        .unwrap(),
                )
                .unwrap();
        }
        let text_to_image =
            GenerationRequest::from_node(graph.node(&"gen".into()).unwrap(), &graph).unwrap();
        assert_eq!(text_to_image.kind, GenerationKind::Image);
        assert!(text_to_image.inputs.is_empty());
        graph
            .connect(
                "input",
                PortRef::new("photo", "image"),
                PortRef::new("gen", "reference"),
            )
            .unwrap();
        let image_to_image =
            GenerationRequest::from_node(graph.node(&"gen".into()).unwrap(), &graph).unwrap();
        assert_eq!(image_to_image.inputs.len(), 1);
        assert_eq!(image_to_image.inputs[0].source, NodeId::from("photo"));
        assert_eq!(image_to_image.inputs[0].port, "reference");
        assert_eq!(
            image_to_image.inputs[0].asset.as_deref(),
            Some("assets/input.png")
        );
        assert!(image_to_image.settings.get("reference_asset").is_none());
    }

    #[test]
    fn legacy_hidden_references_fail_even_with_a_valid_explicit_edge() {
        let registry = crate::standard_registry();
        let mut graph = Graph::new();
        let mut generator = registry
            .make_node("generate_image", "gen", Point::ZERO, None)
            .unwrap();
        generator.data["prompt"] = json!("A quiet garden");
        generator.data["settings"]["reference_asset"] = json!("assets/hidden.png");
        graph.add_node(generator).unwrap();
        graph
            .add_node(
                registry
                    .make_node("image", "photo", Point::ZERO, None)
                    .unwrap(),
            )
            .unwrap();
        for connected in [false, true] {
            if connected {
                graph
                    .connect(
                        "input",
                        PortRef::new("photo", "image"),
                        PortRef::new("gen", "reference"),
                    )
                    .unwrap();
            }
            let error = GenerationRequest::from_node(graph.node(&"gen".into()).unwrap(), &graph)
                .unwrap_err();
            assert!(error.to_string().contains("转换"));
        }
    }
}

//! Typed local processing nodes and provenance for completed outputs.
use crate::processing::{ProcessingKind, ProcessingSettings};
use crate::tr;
use crate::{CreativeError, CreativeRegistry, NodeDefinition, ProjectAsset};
use gpui_flow_core::{Graph, Node, Port, Size};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessingNodeData {
    pub settings: ProcessingSettings,
    pub asset: String,
    pub source_asset: Option<String>,
    pub output_settings: Option<ProcessingSettings>,
    pub error: Option<String>,
}
impl ProcessingNodeData {
    pub fn new(kind: ProcessingKind) -> Self {
        Self {
            settings: kind.default_settings(),
            asset: String::new(),
            source_asset: None,
            output_settings: None,
            error: None,
        }
    }
    pub fn from_value(value: &Value) -> Result<Self, CreativeError> {
        let data: Self = serde_json::from_value(value.clone())?;
        data.settings
            .validate()
            .map_err(|e| CreativeError(e.to_string()))?;
        if data.error.as_ref().is_some_and(|s| s.len() > 4096) {
            return Err(CreativeError(
                tr(
                    "处理错误信息过长",
                    "Processing error exceeds the length limit",
                )
                .into(),
            ));
        }
        if data.asset.is_empty() {
            if data.source_asset.is_some() || data.output_settings.is_some() {
                return Err(CreativeError(
                    tr(
                        "处理输出缺少对应素材",
                        "Processing output is missing its asset",
                    )
                    .into(),
                ));
            }
        } else {
            valid_key(&data.asset)?;
            valid_key(data.source_asset.as_deref().unwrap_or_default())?;
            if data.output_settings.as_ref() != Some(&data.settings) {
                return Err(CreativeError(
                    tr(
                        "处理参数已变更，请重新处理",
                        "Settings changed. Run processing again",
                    )
                    .into(),
                ));
            }
        }
        Ok(data)
    }
    pub fn set_settings(&mut self, settings: ProcessingSettings) -> Result<(), CreativeError> {
        settings
            .validate()
            .map_err(|e| CreativeError(e.to_string()))?;
        if settings.kind() != self.settings.kind() {
            return Err(CreativeError(
                tr(
                    "不能改变处理节点的媒体类型",
                    "The processing node's media type cannot change",
                )
                .into(),
            ));
        }
        if settings != self.settings {
            self.settings = settings;
            self.asset.clear();
            self.source_asset = None;
            self.output_settings = None;
            self.error = None;
        }
        Ok(())
    }
}
fn valid_key(key: &str) -> Result<(), CreativeError> {
    crate::project::validate_asset_key(key).map_err(|e| CreativeError(e.to_string()))
}
fn input_key(graph: &Graph, node: &Node, depth: usize) -> Result<String, CreativeError> {
    if depth > crate::MAX_CARDS {
        return Err(CreativeError(
            tr("处理输入存在循环", "Processing inputs contain a cycle").into(),
        ));
    }
    let kind = ProcessingKind::from_node_kind(&node.kind).ok_or_else(|| {
        CreativeError(tr("节点不是媒体处理节点", "This node does not process media").into())
    })?;
    let edge = graph
        .edges
        .values()
        .find(|e| e.target.node == node.id && e.target.port.as_str() == "input")
        .ok_or_else(|| {
            CreativeError(
                tr(
                    "请连接对应类型的媒体输入",
                    "Connect a media input of the matching type",
                )
                .into(),
            )
        })?;
    let source = graph
        .node(&edge.source.node)
        .ok_or_else(|| CreativeError(tr("输入节点不存在", "Input node not found").into()))?;
    if !source.ports.iter().any(|p| {
        p.id == edge.source.port
            && p.direction == gpui_flow_core::PortDirection::Output
            && p.data_type == kind.input_type()
    }) {
        return Err(CreativeError(
            tr(
                "输入端口媒体类型不匹配",
                "Input port media type does not match",
            )
            .into(),
        ));
    }
    if source.kind.starts_with("custom.") {
        let value = crate::custom_nodes::resolve_graph_output(graph, &edge.source)?;
        value.validate_for(&kind.input_type())?;
        value.asset_key().map(str::to_string).ok_or_else(|| {
            CreativeError(
                tr(
                    "自定义输入没有媒体输出",
                    "The custom input has no media output",
                )
                .into(),
            )
        })
    } else if ProcessingKind::from_node_kind(&source.kind).is_some() {
        if edge.source.port.as_str() != "output" {
            return Err(CreativeError(
                tr("处理节点输出端口无效", "Invalid processing output port").into(),
            ));
        }
        output_key(graph, source, depth + 1)
    } else {
        if crate::GenerationKind::from_node_kind(&source.kind).is_some()
            && source.data["status"].as_str() != Some("complete")
        {
            return Err(CreativeError(
                tr(
                    "输入生成节点尚未完成",
                    "The source generation node has not finished",
                )
                .into(),
            ));
        }
        let key = source.data["asset"].as_str().unwrap_or_default();
        valid_key(key)?;
        Ok(key.into())
    }
}
fn output_key(graph: &Graph, node: &Node, depth: usize) -> Result<String, CreativeError> {
    let data = ProcessingNodeData::from_value(&node.data)?;
    let source = input_key(graph, node, depth)?;
    if data.asset.is_empty() || data.source_asset.as_deref() != Some(source.as_str()) {
        return Err(CreativeError(
            tr(
                "输入已变更或尚未处理，请运行本地处理",
                "Run local processing for the current input",
            )
            .into(),
        ));
    }
    Ok(data.asset)
}
pub fn processing_output(graph: &Graph, node: &Node) -> Result<String, CreativeError> {
    output_key(graph, node, 0)
}
pub fn processing_source(
    graph: &Graph,
    node: &Node,
    assets: &[ProjectAsset],
) -> Result<ProjectAsset, CreativeError> {
    let key = input_key(graph, node, 0)?;
    let kind = ProcessingKind::from_node_kind(&node.kind).ok_or_else(|| {
        CreativeError(tr("节点不是媒体处理节点", "This node does not process media").into())
    })?;
    let asset = assets.iter().find(|a| a.key == key).ok_or_else(|| {
        CreativeError(
            tr(
                "输入素材不在当前本地项目",
                "The input asset is not in the current project",
            )
            .into(),
        )
    })?;
    let compatible = match kind {
        ProcessingKind::Image => matches!(asset.media_type.as_str(), "image/png" | "image/jpeg"),
        ProcessingKind::Video => asset.media_type.starts_with("video/"),
        ProcessingKind::Audio => asset.media_type.starts_with("audio/"),
    };
    if !compatible {
        return Err(CreativeError(
            tr(
                "输入格式不受此处理节点支持",
                "This processing node does not support the input format",
            )
            .into(),
        ));
    }
    Ok(asset.clone())
}
pub(crate) fn register_processing_nodes(registry: &mut CreativeRegistry) {
    for kind in [
        ProcessingKind::Image,
        ProcessingKind::Video,
        ProcessingKind::Audio,
    ] {
        registry
            .register(NodeDefinition::new(
                kind.node_kind(),
                kind.title(),
                Size::new(
                    400.,
                    if kind == ProcessingKind::Audio {
                        144.
                    } else {
                        300.
                    },
                ),
                vec![
                    Port::input("input", tr("输入", "Input"), kind.input_type()),
                    Port::output("output", tr("输出", "Output"), kind.output_type()),
                ],
                serde_json::to_value(ProcessingNodeData::new(kind)).expect("settings serialize"),
                move |value| {
                    let data = ProcessingNodeData::from_value(value).map_err(|e| e.to_string())?;
                    if data.settings.kind() != kind {
                        return Err(tr(
                            "处理参数类型不匹配",
                            "Processing settings do not match the node type",
                        )
                        .into());
                    }
                    Ok(())
                },
            ))
            .expect("valid local processing definition");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_core::{Point, PortRef};
    #[test]
    fn audio_video_inputs_use_project_media_keys_and_keep_types_separate() {
        let registry = crate::standard_registry();
        for (kind, source_kind, key, mime, width, height) in [
            (
                ProcessingKind::Video,
                "video",
                "assets/movie.mp4",
                "video/mp4",
                640,
                360,
            ),
            (
                ProcessingKind::Audio,
                "audio",
                "assets/sound.wav",
                "audio/wav",
                0,
                0,
            ),
        ] {
            let mut graph = Graph::default();
            graph
                .add_node(
                    registry
                        .make_node(
                            source_kind,
                            "source",
                            Point::ZERO,
                            Some(serde_json::json!({"asset":key,"caption":""})),
                        )
                        .unwrap(),
                )
                .unwrap();
            graph
                .add_node(
                    registry
                        .make_node(kind.node_kind(), "processor", Point::ZERO, None)
                        .unwrap(),
                )
                .unwrap();
            graph
                .connect(
                    "edge",
                    PortRef::new("source", source_kind),
                    PortRef::new("processor", "input"),
                )
                .unwrap();
            let asset = ProjectAsset {
                key: key.into(),
                name: "media".into(),
                media_type: mime.into(),
                byte_len: 128,
                width,
                height,
            };
            let node = graph.node(&"processor".into()).unwrap();
            assert_eq!(
                processing_source(&graph, node, std::slice::from_ref(&asset)).unwrap(),
                asset
            );
            let mut wrong = asset.clone();
            wrong.media_type = "image/png".into();
            assert!(processing_source(&graph, node, &[wrong]).is_err());
            assert!(valid_key("assets/../sound.wav").is_err());
            assert!(valid_key("https://example.com/sound.wav").is_err());
        }
    }
    #[cfg(feature = "desktop")]
    #[test]
    fn real_processed_asset_roundtrips_and_cannot_masquerade_as_another_format() {
        use image::ImageEncoder;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("flow-process-project-{nonce}"));
        std::fs::create_dir(&root).unwrap();
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&[120; 72], 4, 6, image::ExtendedColorType::Rgb8)
            .unwrap();
        let source_path = root.join("original.png");
        std::fs::write(&source_path, &png).unwrap();
        let registry = crate::standard_registry();
        let store = crate::ProjectStore::new(root.join("projects")).unwrap();
        let mut project = store
            .create_project(
                "Processing",
                &gpui_flow_core::FlowDocument::default(),
                &registry,
            )
            .unwrap();
        let source = project.import_image(&source_path).unwrap();
        let settings = ProcessingKind::Image.default_settings();
        let output_path = root.join("output.jpg");
        crate::processing::execute_processing(
            &settings,
            &project.resolve_asset(&source.key).unwrap(),
            &source,
            &output_path,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
        let output = project.import_image(&output_path).unwrap();
        let mut document = gpui_flow_core::FlowDocument::default();
        document
            .graph
            .add_node(
                registry
                    .make_node(
                        "image",
                        "source",
                        Point::ZERO,
                        Some(serde_json::json!({"asset":source.key,"caption":""})),
                    )
                    .unwrap(),
            )
            .unwrap();
        let data = ProcessingNodeData {
            settings: settings.clone(),
            asset: output.key.clone(),
            source_asset: Some(source.key.clone()),
            output_settings: Some(settings),
            error: None,
        };
        document
            .graph
            .add_node(
                registry
                    .make_node(
                        "process_image",
                        "processed",
                        Point::ZERO,
                        Some(serde_json::to_value(data).unwrap()),
                    )
                    .unwrap(),
            )
            .unwrap();
        document
            .graph
            .connect(
                "edge",
                PortRef::new("source", "image"),
                PortRef::new("processed", "input"),
            )
            .unwrap();
        project.save_document(&document, &registry).unwrap();
        let reopened = store
            .open_project(&project.metadata().id, &registry)
            .unwrap();
        assert_eq!(reopened.document(), &document);
        assert_eq!(std::fs::read(&source_path).unwrap(), png);
        let before = std::fs::read(project.directory().join("project.json")).unwrap();
        document
            .graph
            .nodes
            .get_mut(&"processed".into())
            .unwrap()
            .data["asset"] = Value::String(source.key);
        assert!(project.save_document(&document, &registry).is_err());
        assert_eq!(
            std::fs::read(project.directory().join("project.json")).unwrap(),
            before
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn completed_output_invalidates_when_the_connected_asset_changes() {
        let registry = crate::standard_registry();
        let mut graph = Graph::default();
        graph
            .add_node(
                registry
                    .make_node(
                        "image",
                        "source",
                        Point::ZERO,
                        Some(serde_json::json!({"asset":"assets/original.png","caption":""})),
                    )
                    .unwrap(),
            )
            .unwrap();
        let mut data = ProcessingNodeData::new(ProcessingKind::Image);
        data.asset = "assets/processed.jpg".into();
        data.source_asset = Some("assets/original.png".into());
        data.output_settings = Some(data.settings.clone());
        graph
            .add_node(
                registry
                    .make_node(
                        "process_image",
                        "process",
                        Point::ZERO,
                        Some(serde_json::to_value(data).unwrap()),
                    )
                    .unwrap(),
            )
            .unwrap();
        graph
            .connect(
                "in",
                PortRef::new("source", "image"),
                PortRef::new("process", "input"),
            )
            .unwrap();
        assert_eq!(
            processing_output(&graph, graph.node(&"process".into()).unwrap()).unwrap(),
            "assets/processed.jpg"
        );
        graph.nodes.get_mut(&"source".into()).unwrap().data["asset"] =
            Value::String("assets/replacement.png".into());
        assert!(processing_output(&graph, graph.node(&"process".into()).unwrap()).is_err());
    }
    #[test]
    fn settings_changes_clear_output_and_reject_cross_type_settings() {
        let mut data = ProcessingNodeData::new(ProcessingKind::Image);
        data.asset = "assets/output.jpg".into();
        data.source_asset = Some("assets/input.jpg".into());
        data.output_settings = Some(data.settings.clone());
        let old = data.clone();
        assert!(
            data.set_settings(ProcessingKind::Audio.default_settings())
                .is_err()
        );
        assert_eq!(data, old);
        let mut settings = data.settings.clone();
        if let ProcessingSettings::Image { quality, .. } = &mut settings {
            *quality = 60;
        }
        data.set_settings(settings).unwrap();
        assert!(data.asset.is_empty());
        assert!(data.source_asset.is_none());
    }
}

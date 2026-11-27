//! Invalidate direction-dependent results without transforming legacy edits.
use crate::custom_nodes::{
    CustomNodeData, CustomNodeStatus, CustomValue, custom_definitions_from_graph,
};
use crate::mask_node::ORIENTATION_REVIEW_REQUIRED;
use crate::processing::ProcessingKind;
use crate::processing_node::ProcessingNodeData;
use crate::{CreativeError, MaskNodeData, tr};
use gpui_flow_core::Graph;
use serde_json::Value;
use std::collections::BTreeSet;

/// `affected_assets` includes every nonidentity legacy EXIF orientation, even
/// reflections and half-turns that keep the encoded width and height unchanged.
/// The caller owns manifest migration and persistence; this never touches files,
/// asset records, graph geometry, or the old mask's recoverable stroke payload.
pub(super) fn guard_legacy_orientation(
    graph: &mut Graph,
    affected_assets: &BTreeSet<String>,
) -> Result<(), CreativeError> {
    if affected_assets.is_empty() {
        return Ok(());
    }
    let definitions = custom_definitions_from_graph(graph)?;
    let mut replacements = Vec::new();
    for node in graph.nodes.values() {
        if node.kind == "mask" {
            let data = MaskNodeData::from_value(&node.data)?;
            if data
                .state
                .source
                .as_ref()
                .is_some_and(|source| affected_assets.contains(&source.asset_key))
            {
                let mut value = node.data.clone();
                value[ORIENTATION_REVIEW_REQUIRED] = Value::Bool(true);
                replacements.push((node.id.clone(), value));
            }
        } else if node.kind == ProcessingKind::Image.node_kind() {
            let mut data = ProcessingNodeData::from_value(&node.data)?;
            if data.settings.kind() != ProcessingKind::Image {
                return Err(CreativeError(
                    tr(
                        "处理参数类型不匹配",
                        "Processing parameter types do not match",
                    )
                    .into(),
                ));
            }
            if data
                .source_asset
                .as_ref()
                .is_some_and(|key| affected_assets.contains(key))
            {
                data.asset.clear();
                data.source_asset = None;
                data.output_settings = None;
                data.error = Some(
                    tr(
                        "原图显示方向已修正，请重新运行本地处理",
                        "The source image orientation was corrected. Run local processing again.",
                    )
                    .into(),
                );
                replacements.push((node.id.clone(), serde_json::to_value(data)?));
            }
        } else if node.kind.starts_with("custom.") {
            let definition = definitions
                .iter()
                .find(|definition| definition.kind == node.kind)
                .ok_or_else(|| {
                    CreativeError(
                        tr(
                            "自定义节点缺少已保存的类型定义",
                            "The custom node is missing its saved type definition",
                        )
                        .into(),
                    )
                })?;
            let mut data = CustomNodeData::from_value(&node.data, definition)?;
            let affected_input = data.completed_request.as_ref().is_some_and(|request| {
                request.inputs.values().any(|input| match &input.value {
                    CustomValue::Image { asset_key } => affected_assets.contains(asset_key),
                    CustomValue::Mask { source, .. } => affected_assets.contains(&source.asset_key),
                    _ => false,
                })
            });
            let affected_mask_output = data.outputs.values().any(|value| {
                matches!(value, CustomValue::Mask { source, .. }
                    if affected_assets.contains(&source.asset_key))
            });
            if affected_input || affected_mask_output {
                data.outputs.clear();
                data.completed_request = None;
                data.status = CustomNodeStatus::Failed;
                data.error = Some(tr("输入原图显示方向已修正，请检查遮罩并重新运行节点", "The source image orientation was corrected. Check masks and run the node again.").into());
                replacements.push((node.id.clone(), serde_json::to_value(data)?));
            }
        }
    }
    // Do not partially migrate a graph when a later typed payload is invalid.
    for (id, value) in replacements {
        graph
            .nodes
            .get_mut(&id)
            .expect("validated existing node")
            .data = value;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom_nodes::{
        CUSTOM_NODE_TYPES_KEY, CustomNodeDefinition, CustomNodeInput, CustomNodeRequest,
        CustomPortDefinition, CustomPortType, resolve_graph_output,
    };
    use crate::mask::{MaskMode, MaskPoint, MaskSource, MaskStroke};
    use gpui_flow_core::{Node, Point, PortRef, Size};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn published_mask(key: &str) -> Node {
        let mut data = MaskNodeData::default();
        data.reset_source(MaskSource::new(key, 20, 10).unwrap())
            .unwrap();
        data.push_stroke(
            MaskStroke::new(MaskMode::Paint, 0.1, vec![MaskPoint(1234, 5678)]).unwrap(),
        )
        .unwrap();
        data.asset = "assets/old-mask.png".into();
        data.output_revision = Some(data.revision);
        crate::standard_registry()
            .make_node("mask", "mask", Point::new(41., 73.), Some(json!(data)))
            .unwrap()
    }

    #[test]
    fn mirror_and_quarter_turn_keep_legacy_masks_but_require_an_explicit_reset() {
        for (key, width, height) in [
            ("assets/mirrored.jpg", 20, 10),
            ("assets/quarter-turn.jpg", 10, 20),
        ] {
            let registry = crate::standard_registry();
            let mut graph = Graph::new();
            graph.add_node(published_mask(key)).unwrap();
            let mut image = registry
                .make_node("image", "image", Point::new(13., 19.), None)
                .unwrap();
            image.data["asset"] = json!(key);
            graph.add_node(image).unwrap();
            graph
                .connect(
                    "image-in",
                    PortRef::new("image", "image"),
                    PortRef::new("mask", "image"),
                )
                .unwrap();
            let before = graph.clone();
            let source = PortRef::new("mask", "mask");
            assert!(resolve_graph_output(&graph, &source).is_ok());
            let affected = BTreeSet::from([key.to_string()]);
            guard_legacy_orientation(&mut graph, &affected).unwrap();
            let mask = graph.node(&"mask".into()).unwrap().clone();
            assert!(crate::mask_node::orientation_review_required(&mask.data).unwrap());
            assert!(resolve_graph_output(&graph, &source).is_err());
            let mut recovered = graph.clone();
            recovered
                .nodes
                .get_mut(&"mask".into())
                .unwrap()
                .data
                .as_object_mut()
                .unwrap()
                .remove(ORIENTATION_REVIEW_REQUIRED);
            assert_eq!(recovered, before, "only the guard extension may change");

            let current = crate::mask_source(
                &graph,
                &mask,
                &[crate::ProjectAsset {
                    key: key.into(),
                    name: "original.jpg".into(),
                    media_type: "image/jpeg".into(),
                    byte_len: 100,
                    width,
                    height,
                }],
            )
            .unwrap();
            assert_eq!((current.width, current.height), (width, height));
            let guarded = graph.clone();
            guard_legacy_orientation(&mut graph, &affected).unwrap();
            assert_eq!(graph, guarded, "migration must be idempotent");

            let mut data = MaskNodeData::from_value(&mask.data).unwrap();
            data.reset_source(current.clone()).unwrap();
            let reset = json!(data);
            assert!(!crate::mask_node::orientation_review_required(&reset).unwrap());
            assert!(data.state.strokes.is_empty());
            assert_eq!(data.state.source.as_ref(), Some(&current));
            assert!(data.asset.is_empty());
            assert_eq!(data.output_revision, None);
            assert_eq!(data.revision, 3);
        }
    }

    fn custom_definition() -> CustomNodeDefinition {
        CustomNodeDefinition {
            schema_version: 1,
            kind: "custom.orientation-check".into(),
            title: "Orientation check".into(),
            size: Size::new(200., 100.),
            inputs: vec![CustomPortDefinition {
                id: "input".into(),
                label: "Input".into(),
                data_type: CustomPortType::Named("image".into()),
            }],
            outputs: vec![CustomPortDefinition {
                id: "output".into(),
                label: "Output".into(),
                data_type: CustomPortType::Text,
            }],
            executor: "host.orientation-check".into(),
            default_settings: json!({"keep": true}),
        }
    }

    fn completed_custom(definition: &CustomNodeDefinition, value: CustomValue) -> Node {
        let mut registry = crate::standard_registry();
        crate::custom_nodes::install_custom_definitions(
            &mut registry,
            std::slice::from_ref(definition),
        )
        .unwrap();
        let mut data = CustomNodeData::for_definition(definition);
        data.status = CustomNodeStatus::Complete;
        data.outputs = BTreeMap::from([(
            "output".into(),
            CustomValue::Text {
                text: "cached".into(),
            },
        )]);
        data.completed_request = Some(CustomNodeRequest {
            node_id: "custom".into(),
            kind: definition.kind.clone(),
            executor: definition.executor.clone(),
            settings: data.settings.clone(),
            inputs: BTreeMap::from([(
                "input".into(),
                CustomNodeInput {
                    source: PortRef::new("source", "output"),
                    value,
                },
            )]),
        });
        registry
            .make_node(&definition.kind, "custom", Point::ZERO, Some(json!(data)))
            .unwrap()
    }

    #[test]
    fn custom_image_and_mask_input_snapshots_are_invalidated_without_changing_settings() {
        for value in [
            CustomValue::Image {
                asset_key: "assets/changed.jpg".into(),
            },
            CustomValue::Mask {
                asset_key: "assets/old-mask.png".into(),
                source: MaskSource::new("assets/changed.jpg", 20, 10).unwrap(),
            },
        ] {
            let mut definition = custom_definition();
            definition.inputs[0].data_type = CustomPortType::Named(
                if matches!(value, CustomValue::Mask { .. }) {
                    "mask"
                } else {
                    "image"
                }
                .into(),
            );
            let mut graph = Graph::new();
            graph.data[CUSTOM_NODE_TYPES_KEY] = json!([definition.clone()]);
            graph
                .add_node(completed_custom(&definition, value))
                .unwrap();
            let before = graph.clone();
            guard_legacy_orientation(&mut graph, &BTreeSet::from(["assets/changed.jpg".into()]))
                .unwrap();
            let node = graph.node(&"custom".into()).unwrap();
            let data = CustomNodeData::from_value(&node.data, &definition).unwrap();
            assert_eq!(data.status, CustomNodeStatus::Failed);
            assert!(data.outputs.is_empty());
            assert!(data.completed_request.is_none());
            assert!(data.error.is_some());
            assert_eq!(data.settings, definition.default_settings);
            let mut geometry = graph.clone();
            geometry.nodes.get_mut(&"custom".into()).unwrap().data =
                before.node(&"custom".into()).unwrap().data.clone();
            assert_eq!(geometry, before);
        }
    }

    #[test]
    fn unrelated_custom_snapshots_and_processing_outputs_remain_intact() {
        let definition = custom_definition();
        let mut graph = Graph::new();
        graph.data[CUSTOM_NODE_TYPES_KEY] = json!([definition.clone()]);
        graph
            .add_node(completed_custom(
                &definition,
                CustomValue::Image {
                    asset_key: "assets/unchanged.jpg".into(),
                },
            ))
            .unwrap();
        let registry = crate::standard_registry();
        for (id, source) in [
            ("changed", "assets/changed.jpg"),
            ("unchanged", "assets/unchanged.jpg"),
        ] {
            let mut data = ProcessingNodeData::new(ProcessingKind::Image);
            data.asset = "assets/processed.png".into();
            data.source_asset = Some(source.into());
            data.output_settings = Some(data.settings.clone());
            graph
                .add_node(
                    registry
                        .make_node("process_image", id, Point::ZERO, Some(json!(data)))
                        .unwrap(),
                )
                .unwrap();
        }
        let before = graph.clone();
        guard_legacy_orientation(&mut graph, &BTreeSet::from(["assets/changed.jpg".into()]))
            .unwrap();
        for id in ["custom", "unchanged"] {
            assert_eq!(graph.node(&id.into()), before.node(&id.into()));
        }
        let changed =
            ProcessingNodeData::from_value(&graph.node(&"changed".into()).unwrap().data).unwrap();
        assert!(changed.asset.is_empty());
        assert!(changed.source_asset.is_none());
        assert!(changed.output_settings.is_none());
        assert!(changed.error.is_some());
        assert_eq!(changed.settings, ProcessingKind::Image.default_settings());
    }

    #[test]
    fn custom_mask_output_requires_review_even_without_an_affected_input() {
        let mut definition = custom_definition();
        definition.outputs[0].data_type = CustomPortType::Named("mask".into());
        let mut data = CustomNodeData::for_definition(&definition);
        data.status = CustomNodeStatus::Complete;
        data.outputs.insert(
            "output".into(),
            CustomValue::Mask {
                asset_key: "assets/old-mask.png".into(),
                source: MaskSource::new("assets/changed.jpg", 20, 10).unwrap(),
            },
        );
        data.completed_request = Some(CustomNodeRequest {
            node_id: "custom".into(),
            kind: definition.kind.clone(),
            executor: definition.executor.clone(),
            settings: data.settings.clone(),
            inputs: BTreeMap::from([(
                "input".into(),
                CustomNodeInput {
                    source: PortRef::new("source", "image"),
                    value: CustomValue::Image {
                        asset_key: "assets/unchanged.jpg".into(),
                    },
                },
            )]),
        });
        let mut registry = crate::standard_registry();
        crate::custom_nodes::install_custom_definitions(
            &mut registry,
            std::slice::from_ref(&definition),
        )
        .unwrap();
        let mut graph = Graph::new();
        graph.data[CUSTOM_NODE_TYPES_KEY] = json!([definition.clone()]);
        graph
            .add_node(
                registry
                    .make_node(&definition.kind, "custom", Point::ZERO, Some(json!(data)))
                    .unwrap(),
            )
            .unwrap();
        guard_legacy_orientation(&mut graph, &BTreeSet::from(["assets/changed.jpg".into()]))
            .unwrap();
        let data =
            CustomNodeData::from_value(&graph.node(&"custom".into()).unwrap().data, &definition)
                .unwrap();
        assert_eq!(data.status, CustomNodeStatus::Failed);
        assert!(data.outputs.is_empty());
        assert!(data.completed_request.is_none());
    }

    #[test]
    fn invalid_payload_aborts_migration_before_any_changes() {
        let mut graph = Graph::new();
        graph
            .add_node(published_mask("assets/changed.jpg"))
            .unwrap();
        let mut processing = crate::standard_registry()
            .make_node("process_image", "zz-invalid", Point::ZERO, None)
            .unwrap();
        processing.data["unexpected"] = json!(true);
        graph.add_node(processing).unwrap();
        let before = graph.clone();
        assert!(
            guard_legacy_orientation(&mut graph, &BTreeSet::from(["assets/changed.jpg".into()]))
                .is_err()
        );
        assert_eq!(graph, before);
    }
}

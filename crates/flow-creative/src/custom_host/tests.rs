use super::*;
use gpui_flow_core::{FlowDocument, Point, PortRef, Size};

fn definition() -> CustomNodeDefinition {
    let mut definition = CustomNodeDefinition {
        schema_version: 1,
        kind: "custom.echo".into(),
        title: "Echo".into(),
        size: Size::new(360., 240.),
        inputs: vec![CustomPortDefinition {
            id: "in".into(),
            label: "Input".into(),
            data_type: CustomPortType::Text,
        }],
        outputs: vec![CustomPortDefinition {
            id: "out".into(),
            label: "Output".into(),
            data_type: CustomPortType::Text,
        }],
        executor: "local.pass_through".into(),
        default_settings: json!({}),
    };
    configure_pass_through(&mut definition).unwrap();
    definition
}
fn document(definition: &CustomNodeDefinition) -> FlowDocument {
    let base = crate::standard_registry();
    let mut document = FlowDocument::default();
    document.graph.data = json!({CUSTOM_NODE_TYPES_KEY:[definition]});
    let registry = registry_with_custom_definitions(&base, &document.graph).unwrap();
    document
        .graph
        .add_node(
            base.make_node(
                "text",
                "source",
                Point::ZERO,
                Some(json!({"text":"Original"})),
            )
            .unwrap(),
        )
        .unwrap();
    document
        .graph
        .add_node(
            registry
                .make_node(&definition.kind, "custom", Point::new(400., 0.), None)
                .unwrap(),
        )
        .unwrap();
    document
        .graph
        .connect(
            "in",
            PortRef::new("source", "text"),
            PortRef::new("custom", "in"),
        )
        .unwrap();
    document
}
#[test]
fn local_execution_records_inputs_and_rejects_stale_values_and_settings() {
    let definition = definition();
    let mut document = document(&definition);
    let node = document.graph.node(&"custom".into()).unwrap();
    let request = CustomNodeRequest::from_node(node, &document.graph, &definition).unwrap();
    let result = local_custom_executors()
        .execute(&definition, &request, &AtomicBool::new(false))
        .unwrap();
    let mut data = CustomNodeData::from_value(&node.data, &definition).unwrap();
    data.apply_result(&definition, &request, result).unwrap();
    document.graph.nodes.get_mut(&"custom".into()).unwrap().data =
        serde_json::to_value(data).unwrap();
    let output = PortRef::new("custom", "out");
    assert_eq!(
        resolve_graph_output(&document.graph, &output)
            .unwrap()
            .text(),
        Some("Original")
    );
    let completed = document.clone();
    document.graph.nodes.get_mut(&"source".into()).unwrap().data["text"] = json!("Changed");
    assert!(resolve_graph_output(&document.graph, &output).is_err());
    document = completed;
    document.graph.nodes.get_mut(&"custom".into()).unwrap().data["settings"]["other"] = json!(true);
    assert!(resolve_graph_output(&document.graph, &output).is_err());
}
#[test]
fn local_project_reopens_its_custom_contract_and_registered_execution() {
    let definition = definition();
    let document = document(&definition);
    let base = crate::standard_registry();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("flow-custom-project-{nonce}"));
    let store = crate::ProjectStore::new(&root).unwrap();
    let project = store.create_project("Custom", &document, &base).unwrap();
    let reopened = store.open_project(&project.metadata().id, &base).unwrap();
    assert_eq!(reopened.document(), &document);
    let registry = registry_with_custom_definitions(&base, &document.graph).unwrap();
    registry.validate_document(&document).unwrap();
    let mut conflict = document.clone();
    conflict.graph.data[CUSTOM_NODE_TYPES_KEY][0]["title"] = json!("Changed contract");
    assert!(registry.validate_document(&conflict).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn unknown_interfaces_do_not_execute_and_cancellation_does_not_publish() {
    let mut definition = definition();
    let mut document = document(&definition);
    definition.executor = "host.unregistered".into();
    document.graph.data[CUSTOM_NODE_TYPES_KEY] = json!([definition]);
    let request = CustomNodeRequest::from_node(
        document.graph.node(&"custom".into()).unwrap(),
        &document.graph,
        &definition,
    )
    .unwrap();
    assert!(matches!(
        local_custom_executors().execute(&definition, &request, &AtomicBool::new(false)),
        Err(CustomExecutionError::Unavailable { .. })
    ));
    assert_eq!(
        local_custom_executors().execute(&definition, &request, &AtomicBool::new(true)),
        Err(CustomExecutionError::Cancelled)
    );
}
#[test]
fn image_assets_cannot_be_returned_as_masks_and_mask_sources_keep_dimensions() {
    let image = ProjectAsset {
        key: "assets/source.png".into(),
        name: "Source".into(),
        media_type: "image/png".into(),
        byte_len: 100,
        width: 10,
        height: 20,
    };
    let source = crate::mask::MaskSource::new(&image.key, 10, 20).unwrap();
    let mask = CustomValue::Mask {
        asset_key: "assets/mask.png".into(),
        source: source.clone(),
    };
    let mut raster = ProjectAsset {
        key: "assets/mask.png".into(),
        name: "Mask".into(),
        media_type: "image/png".into(),
        byte_len: 100,
        width: 10,
        height: 20,
    };
    assert!(validate_custom_value_assets(&mask, &[image.clone(), raster.clone()]).is_err());
    raster.media_type = crate::MASK_MEDIA_TYPE.into();
    validate_custom_value_assets(&mask, &[image.clone(), raster.clone()]).unwrap();
    raster.width = 11;
    assert!(validate_custom_value_assets(&mask, &[image, raster]).is_err());
}

#[test]
fn generation_consumes_the_connected_custom_output_port() {
    let mut definition = definition();
    definition.inputs.clear();
    definition.outputs = vec![
        CustomPortDefinition {
            id: "first".into(),
            label: "First".into(),
            data_type: CustomPortType::Named("image".into()),
        },
        CustomPortDefinition {
            id: "second".into(),
            label: "Second".into(),
            data_type: CustomPortType::Named("image".into()),
        },
    ];
    let mut document = FlowDocument::default();
    document.graph.data = json!({CUSTOM_NODE_TYPES_KEY:[definition]});
    let base = crate::standard_registry();
    let registry = registry_with_custom_definitions(&base, &document.graph).unwrap();
    let mut producer = registry
        .make_node(&definition.kind, "producer", Point::ZERO, None)
        .unwrap();
    let request = CustomNodeRequest {
        node_id: producer.id.clone(),
        kind: definition.kind.clone(),
        executor: definition.executor.clone(),
        settings: definition.default_settings.clone(),
        inputs: BTreeMap::new(),
    };
    let result = CustomNodeResult {
        outputs: BTreeMap::from([
            (
                "first".into(),
                CustomValue::Image {
                    asset_key: "assets/first.png".into(),
                },
            ),
            (
                "second".into(),
                CustomValue::Image {
                    asset_key: "assets/second.png".into(),
                },
            ),
        ]),
    };
    let mut data = CustomNodeData::for_definition(&definition);
    data.apply_result(&definition, &request, result).unwrap();
    producer.data = serde_json::to_value(data).unwrap();
    document.graph.add_node(producer).unwrap();
    let mut generator = base
        .make_node("generate_image", "generator", Point::ZERO, None)
        .unwrap();
    generator.data["prompt"] = json!("Edit the second image");
    document.graph.add_node(generator).unwrap();
    document
        .graph
        .connect(
            "edge",
            PortRef::new("producer", "second"),
            PortRef::new("generator", "reference"),
        )
        .unwrap();
    let request = crate::GenerationRequest::from_node(
        document.graph.node(&"generator".into()).unwrap(),
        &document.graph,
    )
    .unwrap();
    assert_eq!(
        request.inputs[0].asset.as_deref(),
        Some("assets/second.png")
    );
}

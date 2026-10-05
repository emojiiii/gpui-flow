use super::*;
use gpui_flow_core::Point;
use std::sync::atomic::AtomicUsize;

fn port(id: &str, data_type: CustomPortType) -> CustomPortDefinition {
    CustomPortDefinition {
        id: id.into(),
        label: id.into(),
        data_type,
    }
}

fn definition() -> CustomNodeDefinition {
    CustomNodeDefinition {
        schema_version: 1,
        kind: "custom.process_image".into(),
        title: "Process image".into(),
        size: Size::new(320.0, 220.0),
        inputs: vec![port("image", CustomPortType::Named("image".into()))],
        outputs: vec![
            port("preview", CustomPortType::Named("image".into())),
            port("caption", CustomPortType::Text),
        ],
        executor: "host.process_image".into(),
        default_settings: json!({"quality":85}),
    }
}

fn result() -> CustomNodeResult {
    CustomNodeResult {
        outputs: BTreeMap::from([
            (
                "preview".into(),
                CustomValue::Image {
                    asset_key: "assets/preview.png".into(),
                },
            ),
            (
                "caption".into(),
                CustomValue::Text {
                    text: "a real caption".into(),
                },
            ),
        ]),
    }
}

fn make_request(definition: &CustomNodeDefinition) -> CustomNodeRequest {
    CustomNodeRequest {
        node_id: "processor".into(),
        kind: definition.kind.clone(),
        executor: definition.executor.clone(),
        settings: definition.default_settings.clone(),
        inputs: BTreeMap::from([(
            "image".into(),
            CustomNodeInput {
                source: PortRef::new("original", "image"),
                value: CustomValue::Image {
                    asset_key: "assets/original.jpg".into(),
                },
            },
        )]),
    }
}

#[test]
fn declarations_round_trip_and_registry_installation_is_atomic() {
    let definition = definition();
    let encoded = serde_json::to_string(&definition).unwrap();
    assert_eq!(
        CustomNodeDefinition::from_json(&encoded).unwrap(),
        definition
    );
    let mut registry = crate::standard_registry();
    install_custom_definitions(&mut registry, std::slice::from_ref(&definition)).unwrap();
    let node = registry
        .make_node(&definition.kind, "one", Point::ZERO, None)
        .unwrap();
    assert_eq!(node.ports, definition.ports());
    assert_eq!(node.data["settings"], json!({"quality":85}));
    assert!(registry.definition("custom.process_image").is_some());

    let mut new_definition = definition.clone();
    new_definition.kind = "custom.second".into();
    assert!(install_custom_definitions(&mut registry, &[new_definition, definition]).is_err());
    assert!(registry.definition("custom.second").is_none());
}

#[test]
fn declarations_reject_duplicate_ports_types_and_execution_locations() {
    let base = definition();
    let mut duplicate = base.clone();
    duplicate.outputs[0].id = duplicate.inputs[0].id.clone();
    assert!(duplicate.validate().is_err());

    for invalid_type in [
        json!({"kind":"any"}),
        json!({"kind":"made_up"}),
        json!({"kind":"named","name":"Any"}),
        json!({"kind":"named","name":""}),
        json!({"kind":"named","name":"image/*"}),
    ] {
        let mut value = serde_json::to_value(&base).unwrap();
        value["outputs"][0]["data_type"] = invalid_type;
        assert!(CustomNodeDefinition::from_json(&value.to_string()).is_err());
    }
    for bad in ["image", "custom.", "custom..foo", "custom./foo"] {
        let mut candidate = base.clone();
        candidate.kind = bad.into();
        assert!(candidate.validate().is_err());
    }
    for bad in [
        "https://runner.example",
        "/bin/sh",
        "python script.py",
        "$(runner)",
    ] {
        let mut candidate = base.clone();
        candidate.executor = bad.into();
        assert!(candidate.validate().is_err());
    }
    let mut value = serde_json::to_value(&base).unwrap();
    value["command"] = json!("do-not-run");
    assert!(CustomNodeDefinition::from_json(&value.to_string()).is_err());
}

#[test]
fn templates_enforce_port_size_string_payload_and_catalog_budgets() {
    let mut candidate = definition();
    candidate.outputs = (0..MAX_CUSTOM_PORTS)
        .map(|index| port(&format!("output{index}"), CustomPortType::Text))
        .collect();
    assert!(candidate.validate().is_err());
    candidate = definition();
    candidate.title = "x".repeat(513);
    assert!(candidate.validate().is_err());
    candidate = definition();
    candidate.size.width = f32::INFINITY;
    assert!(candidate.validate().is_err());
    candidate = definition();
    candidate.default_settings = json!({"too_long":"x".repeat(MAX_CUSTOM_TEXT_BYTES + 1)});
    assert!(candidate.validate().is_err());
    candidate.default_settings = json!({"items":vec![0;MAX_CUSTOM_JSON_ITEMS]});
    assert!(candidate.validate().is_err());
    candidate.default_settings = json!({
        "one":"x".repeat(MAX_CUSTOM_TEXT_BYTES),
        "two":"x".repeat(MAX_CUSTOM_TEXT_BYTES),
        "three":"x".repeat(MAX_CUSTOM_TEXT_BYTES),
        "four":"x".repeat(MAX_CUSTOM_TEXT_BYTES),
    });
    assert!(candidate.validate().is_err());
    let mut nested = json!(true);
    for _ in 0..=MAX_CUSTOM_JSON_DEPTH {
        nested = json!({"child":nested});
    }
    candidate.default_settings = nested;
    assert!(candidate.validate().is_err());
    assert!(CustomNodeDefinition::from_json(&" ".repeat(MAX_CUSTOM_PAYLOAD_BYTES + 1)).is_err());

    let definitions: Vec<_> = (0..=MAX_CUSTOM_NODE_TYPES)
        .map(|index| {
            let mut definition = definition();
            definition.kind = format!("custom.type{index}");
            definition
        })
        .collect();
    assert!(install_custom_definitions(&mut CreativeRegistry::default(), &definitions).is_err());
    assert!(
        install_custom_definitions(
            &mut CreativeRegistry::default(),
            &[definition(), definition()]
        )
        .is_err()
    );
}

#[test]
fn graph_metadata_installs_without_changing_core_serialization() {
    let mut graph = Graph::new();
    assert!(custom_definitions_from_graph(&graph).unwrap().is_empty());
    graph.data = json!({"project":"unchanged","custom_node_types":[definition()]});
    let copy: Graph = serde_json::from_value(serde_json::to_value(&graph).unwrap()).unwrap();
    assert_eq!(copy, graph);
    let registry = registry_with_custom_definitions(&crate::standard_registry(), &copy).unwrap();
    assert!(registry.definition("custom.process_image").is_some());
    assert!(registry.definition("image").is_some());
    assert_eq!(graph.data["project"], "unchanged");
    graph.data[CUSTOM_NODE_TYPES_KEY] = json!({"not":"an array"});
    assert!(custom_definitions_from_graph(&graph).is_err());
}

#[test]
fn custom_ports_connect_by_exact_type_and_keep_mask_distinct() {
    let mut definition = definition();
    definition
        .inputs
        .push(port("mask", CustomPortType::Named("mask".into())));
    definition
        .inputs
        .push(port("strength", CustomPortType::Number));
    let mut registry = crate::standard_registry();
    install_custom_definitions(&mut registry, std::slice::from_ref(&definition)).unwrap();
    let mut graph = Graph::new();
    graph
        .add_node(
            registry
                .make_node("image", "image", Point::ZERO, None)
                .unwrap(),
        )
        .unwrap();
    graph
        .add_node(
            registry
                .make_node("text", "text", Point::ZERO, None)
                .unwrap(),
        )
        .unwrap();
    graph
        .add_node(
            registry
                .make_node(&definition.kind, "custom", Point::ZERO, None)
                .unwrap(),
        )
        .unwrap();
    assert!(
        graph
            .connect(
                "image-ok",
                PortRef::new("image", "image"),
                PortRef::new("custom", "image")
            )
            .is_ok()
    );
    assert!(
        graph
            .connect(
                "image-is-not-mask",
                PortRef::new("image", "image"),
                PortRef::new("custom", "mask")
            )
            .is_err()
    );
    assert!(
        graph
            .connect(
                "text-is-not-number",
                PortRef::new("text", "text"),
                PortRef::new("custom", "strength")
            )
            .is_err()
    );
}

#[test]
fn requests_resolve_specific_source_ports_and_builtin_text_image() {
    let mut producer = definition();
    producer.inputs.clear();
    let mut consumer = definition();
    consumer.kind = "custom.consume".into();
    consumer.outputs = vec![port("output", CustomPortType::Named("image".into()))];
    consumer.inputs = vec![
        port("photo", CustomPortType::Named("image".into())),
        port("caption", CustomPortType::Text),
        port("brief", CustomPortType::Text),
        port("original", CustomPortType::Named("image".into())),
    ];
    let mut registry = crate::standard_registry();
    install_custom_definitions(&mut registry, &[producer.clone(), consumer.clone()]).unwrap();
    let mut producer_node = registry
        .make_node(&producer.kind, "producer", Point::ZERO, None)
        .unwrap();
    let mut producer_data = CustomNodeData::for_definition(&producer);
    let producer_request = CustomNodeRequest {
        node_id: "producer".into(),
        kind: producer.kind.clone(),
        executor: producer.executor.clone(),
        settings: producer.default_settings.clone(),
        inputs: BTreeMap::new(),
    };
    producer_data
        .apply_result(&producer, &producer_request, result())
        .unwrap();
    producer_node.data = serde_json::to_value(producer_data).unwrap();
    let mut graph = Graph::new();
    graph.add_node(producer_node).unwrap();
    graph
        .add_node(
            registry
                .make_node(&consumer.kind, "consumer", Point::ZERO, None)
                .unwrap(),
        )
        .unwrap();
    graph
        .add_node(
            registry
                .make_node(
                    "text",
                    "brief",
                    Point::ZERO,
                    Some(json!({"text":"original brief"})),
                )
                .unwrap(),
        )
        .unwrap();
    graph
        .add_node(
            registry
                .make_node(
                    "image",
                    "original",
                    Point::ZERO,
                    Some(json!({"asset":"assets/original.jpg","caption":""})),
                )
                .unwrap(),
        )
        .unwrap();
    for (edge, source, source_port, target_port) in [
        ("photo", "producer", "preview", "photo"),
        ("caption", "producer", "caption", "caption"),
        ("brief", "brief", "text", "brief"),
        ("original", "original", "image", "original"),
    ] {
        graph
            .connect(
                edge,
                PortRef::new(source, source_port),
                PortRef::new("consumer", target_port),
            )
            .unwrap();
    }
    let request =
        CustomNodeRequest::from_node(graph.node(&"consumer".into()).unwrap(), &graph, &consumer)
            .unwrap();
    assert_eq!(
        request.inputs["photo"].source,
        PortRef::new("producer", "preview")
    );
    assert_eq!(
        request.inputs["photo"].value.asset_key(),
        Some("assets/preview.png")
    );
    assert_eq!(
        request.inputs["caption"].value.text(),
        Some("a real caption")
    );
    assert_eq!(request.inputs["brief"].value.text(), Some("original brief"));
    assert_eq!(
        request.inputs["original"].value.asset_key(),
        Some("assets/original.jpg")
    );
    assert_eq!(
        serde_json::from_value::<CustomNodeRequest>(serde_json::to_value(&request).unwrap())
            .unwrap(),
        request
    );
    graph.nodes.get_mut(&"producer".into()).unwrap().data["status"] = json!("running");
    assert!(
        CustomNodeRequest::from_node(graph.node(&"consumer".into()).unwrap(), &graph, &consumer)
            .is_err()
    );
}

#[test]
fn diamond_dependencies_expand_linearly_and_cached_errors_do_not_poison_other_roots() {
    let mut definition = definition();
    definition.inputs = vec![
        port("left", CustomPortType::Text),
        port("right", CustomPortType::Text),
    ];
    definition.outputs = vec![
        port("first", CustomPortType::Text),
        port("second", CustomPortType::Text),
    ];
    let mut registry = crate::standard_registry();
    install_custom_definitions(&mut registry, std::slice::from_ref(&definition)).unwrap();
    let mut graph = Graph::new();
    graph
        .add_node(
            registry
                .make_node("text", "base", Point::ZERO, Some(json!({"text":"shared"})))
                .unwrap(),
        )
        .unwrap();
    let mut previous = [PortRef::new("base", "text"), PortRef::new("base", "text")];
    for index in 0..32 {
        let id = format!("relay-{index}");
        let node = registry
            .make_node(&definition.kind, id.clone(), Point::ZERO, None)
            .unwrap();
        graph.add_node(node).unwrap();
        for (input, source) in ["left", "right"].into_iter().zip(&previous) {
            graph
                .connect(
                    format!("{id}-{input}"),
                    source.clone(),
                    PortRef::new(id.clone(), input),
                )
                .unwrap();
        }
        let request = CustomNodeRequest::from_node(
            graph.node(&id.as_str().into()).unwrap(),
            &graph,
            &definition,
        )
        .unwrap();
        let mut data = CustomNodeData::for_definition(&definition);
        data.apply_result(
            &definition,
            &request,
            CustomNodeResult {
                outputs: BTreeMap::from([
                    (
                        "first".into(),
                        CustomValue::Text {
                            text: "shared".into(),
                        },
                    ),
                    (
                        "second".into(),
                        CustomValue::Text {
                            text: "shared".into(),
                        },
                    ),
                ]),
            },
        )
        .unwrap();
        graph.nodes.get_mut(&id.as_str().into()).unwrap().data =
            serde_json::to_value(data).unwrap();
        previous = [
            PortRef::new(id.clone(), "first"),
            PortRef::new(id, "second"),
        ];
    }
    let mut resolver = CustomOutputResolver::new(&graph);
    assert_eq!(
        resolver.resolve(&previous[0]).unwrap().text(),
        Some("shared")
    );
    assert_eq!(
        resolver.resolve(&previous[1]).unwrap().text(),
        Some("shared")
    );
    assert!(
        resolver.steps <= 66,
        "{} steps for 32 diamonds",
        resolver.steps
    );
    assert_eq!(resolver.memo.len(), 65);
    assert!(resolver.resolve(&PortRef::new("missing", "out")).is_err());
    assert_eq!(
        resolver
            .resolve(&PortRef::new("base", "text"))
            .unwrap()
            .text(),
        Some("shared")
    );
}

#[test]
fn requests_reject_missing_input_forged_contract_and_oversized_combined_values() {
    let definition = definition();
    let mut request = make_request(&definition);
    request.inputs.clear();
    assert!(request.validate(&definition).is_err());
    request = make_request(&definition);
    request.executor = "host.different".into();
    assert!(request.validate(&definition).is_err());
    request = make_request(&definition);
    request.inputs.get_mut("image").unwrap().value = CustomValue::Text {
        text: "not an image".into(),
    };
    assert!(request.validate(&definition).is_err());

    let mut definition = definition.clone();
    definition.inputs = (0..5)
        .map(|n| port(&format!("text{n}"), CustomPortType::Text))
        .collect();
    let request = CustomNodeRequest {
        node_id: "node".into(),
        kind: definition.kind.clone(),
        executor: definition.executor.clone(),
        settings: json!({}),
        inputs: (0..5)
            .map(|n| {
                (
                    format!("text{n}"),
                    CustomNodeInput {
                        source: PortRef::new("source", "text"),
                        value: CustomValue::Text {
                            text: "x".repeat(MAX_CUSTOM_TEXT_BYTES),
                        },
                    },
                )
            })
            .collect(),
    };
    assert!(request.validate(&definition).is_err());
}

#[test]
fn unknown_executor_does_not_run_and_registration_is_explicit() {
    let definition = definition();
    let request = make_request(&definition);
    let called = Arc::new(AtomicUsize::new(0));
    let called_by_executor = called.clone();
    let mut executors = CustomNodeExecutors::default();
    executors
        .register(
            "host.other",
            Arc::new(move |_: &CustomNodeRequest, _: &AtomicBool| {
                called_by_executor.fetch_add(1, Ordering::Relaxed);
                Ok(result())
            }),
        )
        .unwrap();
    assert!(!executors.is_available(&definition.executor));
    assert_eq!(
        executors.execute(&definition, &request, &AtomicBool::new(false)),
        Err(CustomExecutionError::Unavailable {
            executor: definition.executor.clone()
        })
    );
    assert_eq!(called.load(Ordering::Relaxed), 0);
    assert!(
        executors
            .register(
                "https://example.invalid/runner",
                Arc::new(|_: &CustomNodeRequest, _: &AtomicBool| Ok(result()))
            )
            .is_err()
    );
}

#[test]
fn executors_return_real_typed_results_and_duplicate_registration_fails() {
    let definition = definition();
    let mut executors = CustomNodeExecutors::default();
    executors
        .register(
            &definition.executor,
            Arc::new(|request: &CustomNodeRequest, _: &AtomicBool| {
                assert_eq!(
                    request.inputs["image"].value.asset_key(),
                    Some("assets/original.jpg")
                );
                Ok(result())
            }),
        )
        .unwrap();
    assert_eq!(
        executors
            .execute(
                &definition,
                &make_request(&definition),
                &AtomicBool::new(false)
            )
            .unwrap(),
        result()
    );
    assert!(
        executors
            .register(
                &definition.executor,
                Arc::new(|_: &CustomNodeRequest, _: &AtomicBool| Ok(result()))
            )
            .is_err()
    );
}

#[test]
fn cancellation_prevents_entry_and_discards_late_success() {
    let definition = definition();
    let called = Arc::new(AtomicUsize::new(0));
    let called_by_executor = called.clone();
    let mut executors = CustomNodeExecutors::default();
    executors
        .register(
            &definition.executor,
            Arc::new(move |_: &CustomNodeRequest, cancelled: &AtomicBool| {
                called_by_executor.fetch_add(1, Ordering::Relaxed);
                cancelled.store(true, Ordering::Release);
                Ok(result())
            }),
        )
        .unwrap();
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        executors.execute(&definition, &make_request(&definition), &cancelled),
        Err(CustomExecutionError::Cancelled)
    );
    assert_eq!(called.load(Ordering::Relaxed), 0);
    cancelled.store(false, Ordering::Release);
    assert_eq!(
        executors.execute(&definition, &make_request(&definition), &cancelled),
        Err(CustomExecutionError::Cancelled)
    );
    assert_eq!(called.load(Ordering::Relaxed), 1);
}

#[test]
fn results_reject_partial_extra_wrong_type_and_nonfinite_values() {
    let definition = definition();
    let mut candidate = result();
    candidate.outputs.remove("caption");
    assert!(candidate.validate(&definition).is_err());
    candidate = result();
    candidate
        .outputs
        .insert("extra".into(), CustomValue::Bool { value: true });
    assert!(candidate.validate(&definition).is_err());
    candidate = result();
    candidate.outputs.insert(
        "preview".into(),
        CustomValue::Text {
            text: "fake image".into(),
        },
    );
    assert!(candidate.validate(&definition).is_err());
    assert!(CustomValue::Number { number: f64::NAN }.validate().is_err());
    assert!(CustomValue::Object { value: json!([]) }.validate().is_err());
    assert!(
        CustomValue::Named {
            name: "mask".into(),
            value: json!({"asset":"assets/fake.png"})
        }
        .validate()
        .is_err()
    );
    assert!(
        CustomValue::Text { text: "ok".into() }
            .validate_for(&DataType::Any)
            .is_err()
    );

    let before = CustomNodeData::for_definition(&definition);
    let mut data = before.clone();
    assert!(
        data.apply_result(&definition, &make_request(&definition), candidate)
            .is_err()
    );
    assert_eq!(data, before);
}

#[test]
fn asset_outputs_are_project_relative_and_media_specific() {
    for bad in [
        "",
        "https://example.com/image.png",
        "/tmp/image.png",
        "assets/../image.png",
        "assets/a/b.png",
        "assets/a%2fb.png",
        "assets/a.png?query=1",
        "assets/video.mp4",
        "assets/a.svg",
    ] {
        assert!(
            CustomValue::Image {
                asset_key: bad.into()
            }
            .validate()
            .is_err(),
            "accepted {bad}"
        );
    }
    for value in [
        CustomValue::Image {
            asset_key: "assets/image.webp".into(),
        },
        CustomValue::Video {
            asset_key: "assets/video.mp4".into(),
        },
        CustomValue::Audio {
            asset_key: "assets/audio.wav".into(),
        },
        CustomValue::Model {
            asset_key: "assets/model.glb".into(),
        },
        CustomValue::Named {
            name: "palette".into(),
            value: json!({"colors":["#ffffff"]}),
        },
    ] {
        value.validate().unwrap();
    }
}

#[test]
fn masks_require_source_provenance_and_do_not_accept_images() {
    let source = MaskSource::new("assets/original.png", 64, 32).unwrap();
    let mask = CustomValue::Mask {
        asset_key: "assets/mask.png".into(),
        source: source.clone(),
    };
    mask.validate_for(&DataType::Named("mask".into())).unwrap();
    assert!(mask.validate_for(&DataType::Named("image".into())).is_err());
    assert!(
        CustomValue::Image {
            asset_key: "assets/mask.png".into()
        }
        .validate_for(&DataType::Named("mask".into()))
        .is_err()
    );
    assert!(
        CustomValue::Mask {
            asset_key: source.asset_key.clone(),
            source: source.clone()
        }
        .validate()
        .is_err()
    );
    assert!(
        serde_json::from_value::<CustomValue>(json!({"type":"mask","asset_key":"assets/mask.png"}))
            .is_err()
    );
    let mut invalid_source = source;
    invalid_source.width = 0;
    assert!(
        CustomValue::Mask {
            asset_key: "assets/mask.png".into(),
            source: invalid_source
        }
        .validate()
        .is_err()
    );
}

#[test]
fn mask_resolution_rejects_changed_original_image_connection() {
    let registry = crate::standard_registry();
    let mut graph = Graph::new();
    graph
        .add_node(
            registry
                .make_node(
                    "image",
                    "original",
                    Point::ZERO,
                    Some(json!({"asset":"assets/original.png","caption":""})),
                )
                .unwrap(),
        )
        .unwrap();
    let mut node = registry
        .make_node("mask", "mask", Point::ZERO, None)
        .unwrap();
    let mut data = MaskNodeData::default();
    data.reset_source(MaskSource::new("assets/original.png", 64, 32).unwrap())
        .unwrap();
    data.asset = "assets/mask.png".into();
    data.output_revision = Some(data.revision);
    node.data = json!(data);
    graph.add_node(node).unwrap();
    graph
        .connect(
            "image-in",
            PortRef::new("original", "image"),
            PortRef::new("mask", "image"),
        )
        .unwrap();
    assert_eq!(
        resolve_graph_output(&graph, &PortRef::new("mask", "mask"))
            .unwrap()
            .asset_key(),
        Some("assets/mask.png")
    );
    graph.nodes.get_mut(&"original".into()).unwrap().data["asset"] = json!("assets/changed.png");
    assert!(resolve_graph_output(&graph, &PortRef::new("mask", "mask")).is_err());
}

#[test]
fn executor_outputs_are_validated_before_any_success_is_returned() {
    let definition = definition();
    let mut executors = CustomNodeExecutors::default();
    executors
        .register(
            &definition.executor,
            Arc::new(|_: &CustomNodeRequest, _: &AtomicBool| {
                let mut value = result();
                value.outputs.insert(
                    "preview".into(),
                    CustomValue::Image {
                        asset_key: "/tmp/not-imported.png".into(),
                    },
                );
                Ok(value)
            }),
        )
        .unwrap();
    assert!(matches!(
        executors.execute(
            &definition,
            &make_request(&definition),
            &AtomicBool::new(false)
        ),
        Err(CustomExecutionError::Invalid(_))
    ));
}

#[test]
fn builtin_mask_stroke_budget_does_not_inherit_custom_object_item_budget() {
    use crate::mask::{MaskMode, MaskPoint, MaskStroke};
    let registry = crate::standard_registry();
    let mut node = registry
        .make_node("mask", "mask", Point::ZERO, None)
        .unwrap();
    let mut data = MaskNodeData::default();
    data.reset_source(MaskSource::new("assets/original.png", 64, 32).unwrap())
        .unwrap();
    data.push_stroke(
        MaskStroke::new(MaskMode::Paint, 0.05, vec![MaskPoint(12000, 12000); 800]).unwrap(),
    )
    .unwrap();
    data.asset = "assets/mask.png".into();
    data.output_revision = Some(data.revision);
    node.data = json!(data);
    // Each compact point is an array plus two numbers, exceeding 2048 JSON
    // items, but this is a valid mask state under its own 2048-point budget.
    assert_eq!(
        resolve_node_output(&node, "mask").unwrap().asset_key(),
        Some("assets/mask.png")
    );
}

#[test]
fn primitive_and_named_object_outputs_keep_exact_contracts() {
    let mut definition = definition();
    definition.inputs.clear();
    definition.outputs = vec![
        port("amount", CustomPortType::Number),
        port("enabled", CustomPortType::Bool),
        port("details", CustomPortType::Object),
        port("palette", CustomPortType::Named("palette".into())),
    ];
    let result = CustomNodeResult {
        outputs: BTreeMap::from([
            ("amount".into(), CustomValue::Number { number: 0.5 }),
            ("enabled".into(), CustomValue::Bool { value: true }),
            (
                "details".into(),
                CustomValue::Object {
                    value: json!({"ok":true}),
                },
            ),
            (
                "palette".into(),
                CustomValue::Named {
                    name: "palette".into(),
                    value: json!({"colors":["#ffffff"]}),
                },
            ),
        ]),
    };
    result.validate(&definition).unwrap();
    assert_eq!(
        serde_json::from_value::<CustomNodeResult>(json!(result)).unwrap(),
        result
    );
    assert!(
        result.outputs["palette"]
            .validate_for(&DataType::Object)
            .is_err()
    );
    assert!(
        result.outputs["palette"]
            .validate_for(&DataType::Named("different".into()))
            .is_err()
    );
}

#[test]
fn combined_object_item_budget_is_checked_before_host_execution() {
    let mut definition = definition();
    definition.inputs = vec![
        port("first", CustomPortType::Object),
        port("second", CustomPortType::Object),
    ];
    let mut request = make_request(&definition);
    request.inputs.clear();
    for port in ["first", "second"] {
        request.inputs.insert(
            port.into(),
            CustomNodeInput {
                source: PortRef::new("source", port),
                value: CustomValue::Object {
                    value: json!({"items":vec![0;1100]}),
                },
            },
        );
    }
    let called = Arc::new(AtomicUsize::new(0));
    let called_by_executor = called.clone();
    let mut executors = CustomNodeExecutors::default();
    executors
        .register(
            &definition.executor,
            Arc::new(move |_: &CustomNodeRequest, _: &AtomicBool| {
                called_by_executor.fetch_add(1, Ordering::Relaxed);
                Ok(result())
            }),
        )
        .unwrap();
    assert!(matches!(
        executors.execute(&definition, &request, &AtomicBool::new(false)),
        Err(CustomExecutionError::Invalid(_))
    ));
    assert_eq!(called.load(Ordering::Relaxed), 0);
}

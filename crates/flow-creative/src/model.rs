use gpui_flow_core::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fmt, sync::Arc};

pub const MAX_CARDS: usize = 128;
pub const MAX_LINKS: usize = 512;
pub const MAX_JSON_BYTES: usize = 32 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: usize = 64 * 1024;
const MAX_TEXT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreativeError(pub String);
impl fmt::Display for CreativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for CreativeError {}
impl From<GraphError> for CreativeError {
    fn from(value: GraphError) -> Self {
        Self(value.to_string())
    }
}
impl From<serde_json::Error> for CreativeError {
    fn from(value: serde_json::Error) -> Self {
        Self(value.to_string())
    }
}
type PayloadValidator = Arc<dyn Fn(&Value) -> Result<(), String> + Send + Sync>;

/// One host-registered card type. Ports are a canonical, exact contract. `Any` is
/// intentionally disallowed in this layer while remaining unchanged in Flow core.
#[derive(Clone)]
pub struct NodeDefinition {
    pub kind: String,
    pub title: String,
    pub size: Size,
    pub ports: Vec<Port>,
    pub default_data: Value,
    validator: PayloadValidator,
}
impl NodeDefinition {
    pub fn new(
        kind: impl Into<String>,
        title: impl Into<String>,
        size: Size,
        ports: Vec<Port>,
        default_data: Value,
        validator: impl Fn(&Value) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind: kind.into(),
            title: title.into(),
            size,
            ports,
            default_data,
            validator: Arc::new(validator),
        }
    }
}
#[derive(Clone, Default)]
pub struct CreativeRegistry {
    definitions: BTreeMap<String, NodeDefinition>,
}
impl CreativeRegistry {
    pub fn register(&mut self, definition: NodeDefinition) -> Result<(), CreativeError> {
        if definition.kind.trim().is_empty() || definition.kind.len() > 128 {
            return Err(CreativeError("invalid card kind".into()));
        }
        if self.definitions.contains_key(&definition.kind) {
            return Err(CreativeError(format!(
                "card kind already registered: {}",
                definition.kind
            )));
        }
        if definition
            .ports
            .iter()
            .any(|p| matches!(p.data_type, DataType::Any))
        {
            return Err(CreativeError(
                "creative ports require an exact type; Any is not allowed".into(),
            ));
        }
        if definition.ports.len() > 16 || definition.ports.iter().any(|p| {
            p.id.as_str().len() > 128 || p.label.len() > 256 || matches!(&p.data_type, DataType::Named(name) if name.trim().is_empty() || name.len() > 128)
        }) { return Err(CreativeError("invalid or oversized port definition".into())); }
        let node = Node::new("definition", &definition.title, Point::ZERO)
            .with_kind(&definition.kind)
            .with_size(definition.size)
            .with_data(definition.default_data.clone());
        let node = Node {
            ports: definition.ports.clone(),
            ..node
        };
        let mut graph = Graph::new();
        graph.add_node(node)?;
        validate_payload(&definition, &definition.default_data)?;
        self.definitions.insert(definition.kind.clone(), definition);
        Ok(())
    }
    pub fn definition(&self, kind: &str) -> Option<&NodeDefinition> {
        self.definitions.get(kind)
    }
    pub fn definitions(&self) -> impl Iterator<Item = &NodeDefinition> {
        self.definitions.values()
    }
    pub fn make_node(
        &self,
        kind: &str,
        id: impl Into<NodeId>,
        position: Point,
        data: Option<Value>,
    ) -> Result<Node, CreativeError> {
        let def = self
            .definitions
            .get(kind)
            .ok_or_else(|| CreativeError(format!("unregistered card kind: {kind}")))?;
        let node = Node {
            ports: def.ports.clone(),
            ..Node::new(id, &def.title, position)
                .with_kind(kind)
                .with_size(def.size)
                .with_data(data.unwrap_or_else(|| def.default_data.clone()))
        };
        self.validate_node(&node)?;
        let mut graph = Graph::new();
        graph.add_node(node.clone())?;
        Ok(node)
    }
    pub fn validate_node(&self, node: &Node) -> Result<(), CreativeError> {
        let def = self
            .definitions
            .get(&node.kind)
            .ok_or_else(|| CreativeError(format!("unregistered card kind: {}", node.kind)))?;
        if node.ports != def.ports {
            return Err(CreativeError(format!(
                "{}: ports do not match the registered exact-type contract",
                node.id
            )));
        }
        if node.title.len() > 512 {
            return Err(CreativeError("card title exceeds 512 bytes".into()));
        }
        crate::references::reject_legacy_reference(node)?;
        if node.kind == crate::shapes::SHAPE_NODE_KIND {
            crate::shapes::validate_shape_node(node).map_err(CreativeError)?;
        }
        validate_payload(def, &node.data)
    }
    pub fn validate_graph(&self, graph: &Graph) -> Result<(), CreativeError> {
        if graph.nodes.len() > MAX_CARDS
            || graph.edges.len() > MAX_LINKS
            || graph.groups.len() > MAX_CARDS
        {
            return Err(CreativeError("creative board size limit exceeded".into()));
        }
        graph.validate()?;
        let registry = crate::custom_nodes::registry_with_custom_definitions(self, graph)?;
        for node in graph.nodes.values() {
            registry.validate_node(node)?;
        }
        Ok(())
    }
    pub fn validate_document(&self, document: &FlowDocument) -> Result<(), CreativeError> {
        document.validate()?;
        self.validate_graph(&document.graph)?;
        if serde_json::to_string_pretty(document)?.len() > MAX_JSON_BYTES {
            return Err(CreativeError("creative document exceeds 32 MiB".into()));
        }
        Ok(())
    }
    pub fn parse_document(&self, json: &str) -> Result<FlowDocument, CreativeError> {
        if json.len() > MAX_JSON_BYTES {
            return Err(CreativeError("creative document exceeds 32 MiB".into()));
        }
        let document = FlowDocument::from_json(json)?;
        self.validate_document(&document)?;
        Ok(document)
    }
    pub fn encode_document(&self, document: &FlowDocument) -> Result<String, CreativeError> {
        self.validate_document(document)?;
        Ok(document.to_json()?)
    }
    /// Validate before touching the existing editor. Camera is navigation state,
    /// outside core graph Undo; all imported graph changes form one Undo step.
    pub fn import_document(
        &self,
        editor: &mut Editor,
        document: FlowDocument,
    ) -> Result<(), CreativeError> {
        self.validate_document(&document)?;
        if editor.transaction_active() {
            return Err(GraphError::TransactionInProgress.into());
        }
        editor.transaction("Import creative board", |graph| {
            *graph = document.graph;
            Ok(())
        })?;
        editor.viewport = document.viewport;
        Ok(())
    }
    pub fn map_records(
        &self,
        records: CanvasRecords,
        layout: LayoutPolicy,
    ) -> Result<FlowDocument, CreativeError> {
        if records.schema_version != 1 {
            return Err(CreativeError("unsupported records schema version".into()));
        }
        if records.cards.len() > MAX_CARDS
            || records.links.len() > MAX_LINKS
            || records.frames.len() > MAX_CARDS
        {
            return Err(CreativeError("records size limit exceeded".into()));
        }
        if let LayoutPolicy::Grid { columns } = layout
            && !(1..=8).contains(&columns)
        {
            return Err(CreativeError("grid columns must be 1..8".into()));
        }
        let mut graph = Graph::new();
        graph.data = records.metadata;
        let registry = crate::custom_nodes::registry_with_custom_definitions(self, &graph)?;
        let mut cards = Vec::new();
        for record in records.cards {
            let mut node = registry.make_node(
                &record.kind,
                record.id,
                record.position.unwrap_or(Point::ZERO),
                record.data,
            )?;
            if let Some(title) = record.title {
                node.title = title;
            }
            if let Some(size) = record.size {
                node.size = size;
            }
            registry.validate_node(&node)?;
            let mut candidate = Graph::new();
            candidate.add_node(node.clone())?;
            cards.push((node, record.position.is_some()));
        }
        let cell = cards.iter().fold(Size::new(240., 180.), |a, (n, _)| {
            Size::new(a.width.max(n.size.width), a.height.max(n.size.height))
        });
        for (index, (mut node, placed)) in cards.into_iter().enumerate() {
            if !placed {
                let LayoutPolicy::Grid { columns } = layout else {
                    return Err(CreativeError(format!(
                        "{} needs an explicit position",
                        node.id
                    )));
                };
                node.position = Point::new(
                    50. + (index % columns) as f32 * (cell.width + 48.),
                    60. + (index / columns) as f32 * (cell.height + 64.),
                );
            }
            graph.add_node(node)?;
        }
        for link in records.links {
            graph.add_edge(Edge {
                id: link.id,
                source: link.source,
                target: link.target,
                label: link.label,
                data: link.data,
            })?;
        }
        for frame in records.frames {
            let mut bounds = None;
            for id in &frame.cards {
                let node = graph.node(id).ok_or_else(|| {
                    CreativeError(format!("frame {} contains missing card {id}", frame.id))
                })?;
                bounds = Some(bounds.map_or(node.bounds(), |b: Rect| b.union(node.bounds())));
            }
            let b =
                bounds.ok_or_else(|| CreativeError("frame needs at least one member".into()))?;
            let bounds = Rect::new(
                Point::new(b.origin.x - 24., b.origin.y - 42.),
                Size::new(b.size.width + 48., b.size.height + 66.),
            );
            graph.add_group(Group::new(frame.id, frame.title, bounds).with_nodes(frame.cards))?;
        }
        let document = FlowDocument {
            graph,
            viewport: Viewport::default(),
        };
        self.validate_document(&document)?;
        Ok(document)
    }
    pub fn parse_records(
        &self,
        json: &str,
        layout: LayoutPolicy,
    ) -> Result<FlowDocument, CreativeError> {
        if json.len() > MAX_JSON_BYTES {
            return Err(CreativeError("records exceed 32 MiB".into()));
        }
        self.map_records(serde_json::from_str(json)?, layout)
    }
}
fn validate_payload(def: &NodeDefinition, data: &Value) -> Result<(), CreativeError> {
    if serde_json::to_vec(data)?.len() > MAX_PAYLOAD_BYTES {
        return Err(CreativeError("card payload exceeds 64 KiB".into()));
    }
    (def.validator)(data).map_err(CreativeError)
}
#[derive(Clone, Copy, Debug)]
pub enum LayoutPolicy {
    RequirePositions,
    Grid { columns: usize },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasRecords {
    pub schema_version: u32,
    pub cards: Vec<CardRecord>,
    #[serde(default)]
    pub links: Vec<LinkRecord>,
    #[serde(default)]
    pub frames: Vec<FrameRecord>,
    #[serde(default)]
    pub metadata: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardRecord {
    pub id: NodeId,
    pub kind: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub position: Option<Point>,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default)]
    pub data: Option<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkRecord {
    pub id: EdgeId,
    pub source: PortRef,
    pub target: PortRef,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub data: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameRecord {
    pub id: GroupId,
    pub title: String,
    pub cards: Vec<NodeId>,
}

fn text_field(data: &Value, name: &str) -> Result<(), String> {
    let text = data
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{name} must be a string"))?;
    if text.len() > MAX_TEXT_BYTES {
        return Err(format!("{name} exceeds 16 KiB"));
    }
    Ok(())
}
/// Host-owned standard card contracts. The palette is an example custom type,
/// registered through exactly the same public API as an application's types.
pub fn standard_registry() -> CreativeRegistry {
    let mut registry = CreativeRegistry::default();
    let image = DataType::Named("image".into());
    let palette = DataType::Named("palette".into());
    for definition in [
        NodeDefinition::new(
            "text",
            "文字",
            Size::new(360., 120.),
            vec![Port::output("text", "文字", DataType::String)],
            json!({"text":"让创意从一句话开始"}),
            |v| {
                text_field(v, "text")?;
                crate::validate_text_style(v)
            },
        ),
        NodeDefinition::new(
            "image",
            "图片 / Reference",
            Size::new(260., 270.),
            vec![Port::output("image", "图像", image.clone())],
            json!({"asset":"botanical","caption":"注册的本地图像资源"}),
            |v| {
                text_field(v, "asset")?;
                text_field(v, "caption")
            },
        ),
        NodeDefinition::new(
            "video",
            "本地视频",
            Size::new(480., 270.),
            vec![Port::output(
                "video",
                "视频",
                DataType::Named("video".into()),
            )],
            json!({"asset":"","caption":""}),
            |v| {
                text_field(v, "asset")?;
                text_field(v, "caption")
            },
        ),
        NodeDefinition::new(
            "audio",
            "本地音频",
            Size::new(400., 144.),
            vec![Port::output(
                "audio",
                "音频",
                DataType::Named("audio".into()),
            )],
            json!({"asset":"","caption":""}),
            |v| {
                text_field(v, "asset")?;
                text_field(v, "caption")
            },
        ),
        NodeDefinition::new(
            "model",
            "本地 3D 模型",
            Size::new(420., 315.),
            vec![Port::output("model", "3D", DataType::Named("model".into()))],
            json!({"asset":"","caption":"本地 GLB · CPU 静态预览"}),
            |v| {
                text_field(v, "asset")?;
                text_field(v, "caption")?;
                if let Some(state) = v.get("camera") {
                    let state: crate::model_preview::OrbitCameraState =
                        serde_json::from_value(state.clone()).map_err(|e| e.to_string())?;
                    if !state.yaw_radians.is_finite()
                        || !(0.0..=std::f32::consts::TAU).contains(&state.yaw_radians)
                        || !state.pitch_radians.is_finite()
                        || state.pitch_radians.abs() > std::f32::consts::FRAC_PI_2 - 0.01
                        || !(0.05..=100.0).contains(&state.distance_factor)
                    {
                        return Err("invalid model camera".into());
                    }
                }
                Ok(())
            },
        ),
        NodeDefinition::new(
            "compose",
            "创作卡片 / Compose",
            Size::new(275., 270.),
            vec![
                Port::input("prompt", "文字", DataType::String),
                Port::input("reference", "图像", image.clone()),
                Port::input("palette", "色板", palette.clone()),
                Port::output("image", "图像", image),
            ],
            json!({"text":"静态创作方案，不调用模型","asset":"sunset"}),
            |v| {
                text_field(v, "text")?;
                text_field(v, "asset")
            },
        ),
        NodeDefinition::new(
            "palette",
            "自定义节点 / Palette",
            Size::new(235., 165.),
            vec![Port::output("palette", "色板", palette)],
            json!({"colors":["#719d8b","#ead8be","#dc927b"],"note":"自定义数据与渲染器"}),
            |v| {
                text_field(v, "note")?;
                let colors = v
                    .get("colors")
                    .and_then(Value::as_array)
                    .ok_or("colors must be an array")?;
                if colors.is_empty()
                    || colors.len() > 8
                    || colors.iter().any(|c| {
                        c.as_str().is_none_or(|s| {
                            s.len() != 7
                                || !s.starts_with('#')
                                || u32::from_str_radix(&s[1..], 16).is_err()
                        })
                    })
                {
                    return Err("colors must contain 1..8 #RRGGBB values".into());
                }
                Ok(())
            },
        ),
    ] {
        registry
            .register(definition)
            .expect("valid built-in creative definition");
    }
    registry
        .register(NodeDefinition::new(
            crate::shapes::SHAPE_NODE_KIND,
            "矩形",
            crate::shapes::DEFAULT_SHAPE_SIZE,
            vec![],
            crate::shapes::ShapeData::new(crate::shapes::ShapeKind::Rectangle)
                .to_data()
                .expect("valid default shape"),
            crate::shapes::validate_shape_data,
        ))
        .expect("valid shape definition");
    crate::generation::register_generation_nodes(&mut registry);
    crate::mask_node::register_mask_nodes(&mut registry);
    crate::processing_node::register_processing_nodes(&mut registry);
    registry
}
pub fn sample_records() -> CanvasRecords {
    let card = |id: &str, kind: &str, x, y| CardRecord {
        id: id.into(),
        kind: kind.into(),
        title: None,
        position: Some(Point::new(x, y)),
        size: None,
        data: None,
    };
    CanvasRecords {
        schema_version: 1,
        cards: vec![
            card("brief", "text", 70., 100.),
            card("reference", "image", 70., 380.),
            card("palette", "palette", 390., 480.),
            card("design", "compose", 700., 210.),
        ],
        links: vec![LinkRecord {
            id: "brief-design".into(),
            source: PortRef::new("brief", "text"),
            target: PortRef::new("design", "prompt"),
            label: "文案".into(),
            data: Value::Null,
        }],
        frames: vec![FrameRecord {
            id: "references".into(),
            title: "01 / 灵感与素材".into(),
            cards: vec!["brief".into(), "reference".into()],
        }],
        metadata: json!({"project":"Flow Creative · 本地示例"}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CreativeGuard;
    use gpui_flow_interaction::{CanvasCommand, PluginHost};
    fn document() -> FlowDocument {
        standard_registry()
            .map_records(sample_records(), LayoutPolicy::RequirePositions)
            .unwrap()
    }
    #[test]
    fn stable_document_roundtrip_preserves_geometry_ports_links_frames_custom_data() {
        let r = standard_registry();
        let d = document();
        let json = r.encode_document(&d).unwrap();
        assert_eq!(r.parse_document(&json).unwrap(), d);
        assert_eq!(d.graph.groups[&"references".into()].node_ids.len(), 2);
        assert_eq!(
            d.graph.nodes[&"palette".into()].data["colors"][0],
            "#719d8b"
        );
    }
    #[test]
    fn same_type_opposite_direction_only_and_capacity_is_real_core_policy() {
        let r = standard_registry();
        let mut d = document();
        assert!(
            d.graph
                .validate_connection(
                    &PortRef::new("reference", "image"),
                    &PortRef::new("design", "reference")
                )
                .is_ok()
        );
        assert!(
            d.graph
                .validate_connection(
                    &PortRef::new("reference", "image"),
                    &PortRef::new("design", "prompt")
                )
                .is_err()
        );
        assert!(
            d.graph
                .validate_connection(
                    &PortRef::new("reference", "image"),
                    &PortRef::new("design", "image")
                )
                .is_err()
        );
        d.graph
            .connect(
                "image-link",
                PortRef::new("reference", "image"),
                PortRef::new("design", "reference"),
            )
            .unwrap();
        assert!(
            d.graph
                .validate_connection(
                    &PortRef::new("reference", "image"),
                    &PortRef::new("design", "reference")
                )
                .is_err()
        );
        d.graph.nodes.get_mut(&"reference".into()).unwrap().ports[0].data_type = DataType::Any;
        assert!(r.validate_document(&d).is_err());
    }
    #[test]
    fn unknown_kind_any_definition_and_invalid_custom_payload_are_rejected() {
        let mut r = standard_registry();
        assert!(r.make_node("missing", "x", Point::ZERO, None).is_err());
        assert!(
            r.make_node(
                "palette",
                "x",
                Point::ZERO,
                Some(json!({"colors":["red"],"note":"bad"}))
            )
            .is_err()
        );
        let bad = NodeDefinition::new(
            "any",
            "Bad",
            Size::new(200., 150.),
            vec![Port::input("in", "Any", DataType::Any)],
            Value::Null,
            |_| Ok(()),
        );
        assert!(r.register(bad).is_err());
        let custom = NodeDefinition::new(
            "rating",
            "Rating",
            Size::new(200., 150.),
            vec![Port::output("out", "Score", DataType::Number)],
            json!({"score":5}),
            |v| {
                if v["score"].as_i64().is_some_and(|s| (0..=10).contains(&s)) {
                    Ok(())
                } else {
                    Err("invalid score".into())
                }
            },
        );
        r.register(custom).unwrap();
        assert_eq!(
            r.make_node("rating", "custom-id", Point::new(3., 4.), None)
                .unwrap()
                .data["score"],
            5
        );
    }
    #[test]
    fn invalid_import_preserves_current_document_selection_and_history() {
        let r = standard_registry();
        let original = document();
        let mut editor = Editor::new(original.graph.clone()).unwrap();
        editor.selection.select_node("brief", false);
        let mut invalid = original.clone();
        invalid.graph.nodes.remove(&"brief".into());
        let before = editor.document();
        let selection = editor.selection.clone();
        assert!(r.import_document(&mut editor, invalid).is_err());
        assert_eq!(editor.document(), before);
        assert_eq!(editor.selection, selection);
        assert!(!editor.undo());
        let mut next = original.clone();
        next.graph
            .nodes
            .get_mut(&"brief".into())
            .unwrap()
            .position
            .x += 100.;
        r.import_document(&mut editor, next.clone()).unwrap();
        assert_eq!(editor.graph(), &next.graph);
        assert!(editor.undo());
        assert_eq!(editor.graph(), &original.graph);
        assert!(editor.redo());
        assert_eq!(editor.graph(), &next.graph);
    }
    #[test]
    fn explicit_mapping_is_deterministic_and_never_guesses_arbitrary_json() {
        let r = standard_registry();
        let mut records = sample_records();
        records.cards[0].position = None;
        assert!(
            r.map_records(records.clone(), LayoutPolicy::RequirePositions)
                .is_err()
        );
        let a = r
            .map_records(records.clone(), LayoutPolicy::Grid { columns: 2 })
            .unwrap();
        assert_eq!(
            a,
            r.map_records(records, LayoutPolicy::Grid { columns: 2 })
                .unwrap()
        );
        assert_eq!(
            a.graph.nodes[&"reference".into()].position,
            Point::new(70., 380.)
        );
        assert!(
            r.parse_records(
                r#"{"nodes":[{"prompt":"guess me"}]}"#,
                LayoutPolicy::Grid { columns: 2 }
            )
            .is_err()
        );
        let mut duplicate = sample_records();
        duplicate.cards.push(duplicate.cards[0].clone());
        assert!(
            r.map_records(duplicate, LayoutPolicy::RequirePositions)
                .is_err()
        );
        let mut unsupported = sample_records();
        unsupported.schema_version = 2;
        assert!(
            r.map_records(unsupported, LayoutPolicy::RequirePositions)
                .is_err()
        );
    }
    #[test]
    fn command_guard_blocks_invalid_edit_and_load_on_shared_undo_pipeline() {
        let r = Arc::new(standard_registry());
        let d = document();
        let mut editor = Editor::new(d.graph).unwrap();
        let mut host = PluginHost::with_defaults();
        host.install(CreativeGuard(r)).unwrap();
        let before = editor.document();
        assert!(
            host.execute(
                &mut editor,
                CanvasCommand::SetNodeData {
                    node: "palette".into(),
                    data: json!({"colors":[],"note":"bad"})
                }
            )
            .is_err()
        );
        assert_eq!(editor.document(), before);
        host.execute(
            &mut editor,
            CanvasCommand::SetNodeData {
                node: "brief".into(),
                data: json!({"text":"new idea"}),
            },
        )
        .unwrap();
        host.execute(&mut editor, CanvasCommand::Undo).unwrap();
        assert_eq!(editor.document(), before);
        let mut invalid = before.clone();
        invalid.graph.nodes.get_mut(&"brief".into()).unwrap().kind = "missing".into();
        assert!(
            host.execute(&mut editor, CanvasCommand::LoadDocument(invalid))
                .is_err()
        );
        assert_eq!(editor.document(), before);
    }
    #[test]
    fn delete_card_removes_edges_and_undo_restores_them() {
        let mut editor = Editor::new(document().graph).unwrap();
        let mut host = PluginHost::with_defaults();
        editor.selection.select_node("brief", false);
        host.execute(&mut editor, CanvasCommand::DeleteSelection)
            .unwrap();
        assert!(editor.graph().edges.is_empty());
        host.execute(&mut editor, CanvasCommand::Undo).unwrap();
        assert_eq!(editor.graph().edges.len(), 1);
    }
    #[test]
    fn invalid_schema_geometry_size_budget_and_active_gesture_reject_atomically() {
        let r = standard_registry();
        let mut d = document();
        d.graph.schema_version = 999;
        assert!(r.validate_document(&d).is_err());
        let mut d = document();
        d.graph.nodes.get_mut(&"brief".into()).unwrap().size.width = -1.;
        assert!(r.validate_document(&d).is_err());
        assert!(r.parse_document(&" ".repeat(MAX_JSON_BYTES + 1)).is_err());
        let mut editor = Editor::new(document().graph).unwrap();
        editor.begin_transaction("drag").unwrap();
        let before = editor.document();
        assert!(r.import_document(&mut editor, document()).is_err());
        assert_eq!(editor.document(), before);
        assert!(editor.transaction_active());
    }
    #[test]
    fn standalone_legacy_documents_require_local_project_conversion_without_changing_editor() {
        let registry = standard_registry();
        let mut legacy = FlowDocument::default();
        let mut node = registry
            .make_node("generate_image", "gen", Point::ZERO, None)
            .unwrap();
        node.data["settings"]["reference_asset"] = json!("assets/old.png");
        legacy.graph.add_node(node).unwrap();
        let json = legacy.to_json().unwrap();
        let error = registry.parse_document(&json).unwrap_err();
        assert!(error.to_string().contains("本地项目"));
        assert!(error.to_string().contains("重新导出"));
        assert!(registry.encode_document(&legacy).is_err());
        let mut editor = Editor::new(document().graph).unwrap();
        let before = editor.document();
        assert!(registry.import_document(&mut editor, legacy).is_err());
        assert_eq!(editor.document(), before);
        assert!(!editor.undo());
    }
}

//! Declarative custom-node contracts and explicitly registered host execution.
//!
//! A document contains data, never runnable code. Executor IDs select host code
//! already registered in this process; they are not commands, scripts, or URLs.
//! The host runs executors off the UI thread and imports output files into the
//! current project before publishing their keys. This module does no I/O.

use crate::mask::MaskSource;
use crate::{CreativeError, CreativeRegistry, MaskNodeData, NodeDefinition};
use gpui_flow_core::{DataType, Graph, Node, NodeId, Port, PortDirection, PortRef, Size};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub const MAX_CUSTOM_NODE_TYPES: usize = 64;
pub const MAX_CUSTOM_PORTS: usize = 16;
pub const MAX_CUSTOM_PAYLOAD_BYTES: usize = 64 * 1024;
pub const MAX_CUSTOM_TEXT_BYTES: usize = 16 * 1024;
pub const MAX_CUSTOM_JSON_DEPTH: usize = 16;
pub const MAX_CUSTOM_JSON_ITEMS: usize = 2048;
pub const CUSTOM_NODE_TYPES_KEY: &str = "custom_node_types";

/// Text is the existing core String type. Named types match exactly, including
/// image/video/audio/model/mask; there is deliberately no wildcard variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "name",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CustomPortType {
    #[serde(alias = "string")]
    Text,
    Number,
    Bool,
    Object,
    Named(String),
}

impl CustomPortType {
    pub fn data_type(&self) -> DataType {
        match self {
            Self::Text => DataType::String,
            Self::Number => DataType::Number,
            Self::Bool => DataType::Bool,
            Self::Object => DataType::Object,
            Self::Named(name) => DataType::Named(name.clone()),
        }
    }

    fn validate(&self) -> Result<(), CreativeError> {
        if let Self::Named(name) = self {
            validate_named_type(name)?;
        }
        Ok(())
    }
}

/// Inputs accept exactly one connection and are required at execution time.
/// Successful results supply every output. Output fan-out remains unlimited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomPortDefinition {
    pub id: String,
    pub label: String,
    pub data_type: CustomPortType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomNodeDefinition {
    pub schema_version: u32,
    pub kind: String,
    pub title: String,
    pub size: Size,
    pub inputs: Vec<CustomPortDefinition>,
    pub outputs: Vec<CustomPortDefinition>,
    /// A stable host registration ID. This is never evaluated or dereferenced.
    pub executor: String,
    #[serde(default = "empty_object")]
    pub default_settings: Value,
}

fn empty_object() -> Value {
    json!({})
}

impl CustomNodeDefinition {
    pub fn from_json(text: &str) -> Result<Self, CreativeError> {
        check_json_bytes(text.len())?;
        let definition: Self = serde_json::from_str(text)?;
        definition.validate()?;
        Ok(definition)
    }

    pub fn validate(&self) -> Result<(), CreativeError> {
        if self.schema_version != 1 {
            return invalid("unsupported custom-node schema version");
        }
        if !self.kind.starts_with("custom.") || self.kind.len() <= "custom.".len() {
            return invalid("custom-node kinds must be in the custom. namespace");
        }
        validate_identifier(&self.kind, "custom-node kind")?;
        validate_identifier(&self.executor, "executor ID")?;
        validate_text(&self.title, 512, "custom-node title", false)?;
        if !self.size.is_positive() || self.size.width > 4096.0 || self.size.height > 4096.0 {
            return invalid("custom-node dimensions must be finite and within 0..=4096");
        }
        if self.outputs.is_empty() || self.inputs.len() + self.outputs.len() > MAX_CUSTOM_PORTS {
            return invalid("custom nodes require an output and at most 16 total ports");
        }
        let mut ids = BTreeSet::new();
        for port in self.inputs.iter().chain(&self.outputs) {
            validate_identifier(&port.id, "port ID")?;
            validate_text(&port.label, 256, "port label", false)?;
            port.data_type.validate()?;
            if !ids.insert(&port.id) {
                return invalid("duplicate custom-node port ID");
            }
        }
        validate_settings(&self.default_settings)?;
        check_payload(self)
    }

    pub fn ports(&self) -> Vec<Port> {
        self.inputs
            .iter()
            .map(|p| Port::input(p.id.clone(), &p.label, p.data_type.data_type()))
            .chain(
                self.outputs
                    .iter()
                    .map(|p| Port::output(p.id.clone(), &p.label, p.data_type.data_type())),
            )
            .collect()
    }

    pub fn to_node_definition(&self) -> Result<NodeDefinition, CreativeError> {
        self.validate()?;
        let contract = self.clone();
        Ok(NodeDefinition::new(
            &self.kind,
            &self.title,
            self.size,
            self.ports(),
            serde_json::to_value(CustomNodeData::for_definition(self))?,
            move |value| {
                CustomNodeData::from_value(value, &contract)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
        ))
    }
}

/// Install atomically: a duplicate or invalid definition leaves the registry
/// unchanged. Call with a fresh base registry when opening a different project.
pub fn install_custom_definitions(
    registry: &mut CreativeRegistry,
    definitions: &[CustomNodeDefinition],
) -> Result<(), CreativeError> {
    validate_definitions(definitions)?;
    let mut candidate = registry.clone();
    for definition in definitions {
        candidate.register(definition.to_node_definition()?)?;
    }
    *registry = candidate;
    Ok(())
}

/// An optional metadata slot; core Graph, records v1, and Leafer stay unchanged.
/// Missing metadata is an empty list. Present metadata must be an array.
pub fn custom_definitions_from_graph(
    graph: &Graph,
) -> Result<Vec<CustomNodeDefinition>, CreativeError> {
    let Some(value) = graph.data.get(CUSTOM_NODE_TYPES_KEY) else {
        return Ok(Vec::new());
    };
    validate_json(value)?;
    let definitions: Vec<CustomNodeDefinition> = serde_json::from_value(value.clone())?;
    validate_definitions(&definitions)?;
    Ok(definitions)
}

pub fn registry_with_custom_definitions(
    base: &CreativeRegistry,
    graph: &Graph,
) -> Result<CreativeRegistry, CreativeError> {
    let mut registry = base.clone();
    for definition in custom_definitions_from_graph(graph)? {
        let declared = definition.to_node_definition()?;
        if let Some(existing) = registry.definition(&definition.kind) {
            if existing.title != declared.title
                || existing.size != declared.size
                || existing.ports != declared.ports
                || existing.default_data != declared.default_data
            {
                return invalid("saved custom definition conflicts with a host-registered type");
            }
        } else {
            registry.register(declared)?;
        }
    }
    Ok(registry)
}

fn validate_definitions(definitions: &[CustomNodeDefinition]) -> Result<(), CreativeError> {
    if definitions.len() > MAX_CUSTOM_NODE_TYPES {
        return invalid("custom-node type count exceeds 64");
    }
    let mut kinds = BTreeSet::new();
    for definition in definitions {
        definition.validate()?;
        if !kinds.insert(&definition.kind) {
            return invalid("duplicate custom-node kind");
        }
    }
    // The entire saved template catalog has a bounded footprint, too.
    check_payload(definitions)
}

/// Typed wire values. Asset keys identify immutable files in the current project;
/// a valid key is not proof that the file exists. The project layer checks that.
/// Mask is a distinct PNG artifact with source image identity and dimensions.
/// Its pixels use the mask module's 0=preserve / 255=edit convention; a provider
/// requiring alpha or inverted values must explicitly adapt it in host code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CustomValue {
    Text {
        text: String,
    },
    Number {
        number: f64,
    },
    Bool {
        value: bool,
    },
    Object {
        value: Value,
    },
    Image {
        asset_key: String,
    },
    Video {
        asset_key: String,
    },
    Audio {
        asset_key: String,
    },
    Model {
        asset_key: String,
    },
    Mask {
        asset_key: String,
        source: MaskSource,
    },
    /// Other exact named domains carry bounded object data, never executables.
    Named {
        name: String,
        value: Value,
    },
}

impl CustomValue {
    pub fn data_type(&self) -> DataType {
        match self {
            Self::Text { .. } => DataType::String,
            Self::Number { .. } => DataType::Number,
            Self::Bool { .. } => DataType::Bool,
            Self::Object { .. } => DataType::Object,
            Self::Image { .. } => DataType::Named("image".into()),
            Self::Video { .. } => DataType::Named("video".into()),
            Self::Audio { .. } => DataType::Named("audio".into()),
            Self::Model { .. } => DataType::Named("model".into()),
            Self::Mask { .. } => DataType::Named("mask".into()),
            Self::Named { name, .. } => DataType::Named(name.clone()),
        }
    }

    pub fn asset_key(&self) -> Option<&str> {
        match self {
            Self::Image { asset_key }
            | Self::Video { asset_key }
            | Self::Audio { asset_key }
            | Self::Model { asset_key }
            | Self::Mask { asset_key, .. } => Some(asset_key),
            _ => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }

    pub fn validate(&self) -> Result<(), CreativeError> {
        match self {
            Self::Text { text } => validate_text(text, MAX_CUSTOM_TEXT_BYTES, "text value", true)?,
            Self::Number { number } if !number.is_finite() => return invalid("non-finite number"),
            Self::Object { value } => validate_settings(value)?,
            Self::Named { name, value } => {
                validate_named_type(name)?;
                if is_media_type(name) {
                    return invalid("media types require a typed asset value");
                }
                validate_settings(value)?;
            }
            Self::Image { asset_key } => validate_asset_key(asset_key, "image")?,
            Self::Video { asset_key } => validate_asset_key(asset_key, "video")?,
            Self::Audio { asset_key } => validate_asset_key(asset_key, "audio")?,
            Self::Model { asset_key } => validate_asset_key(asset_key, "model")?,
            Self::Mask { asset_key, source } => {
                validate_asset_key(asset_key, "mask")?;
                source
                    .validate()
                    .map_err(|e| CreativeError(e.to_string()))?;
                if asset_key == &source.asset_key {
                    return invalid("a mask artifact must be distinct from its source image");
                }
            }
            Self::Number { .. } | Self::Bool { .. } => {}
        }
        check_payload(self)
    }

    pub fn validate_for(&self, data_type: &DataType) -> Result<(), CreativeError> {
        if matches!(data_type, DataType::Any) || &self.data_type() != data_type {
            return invalid("value does not match the exact port type");
        }
        self.validate()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomNodeStatus {
    #[default]
    Idle,
    Queued,
    Running,
    Complete,
    Failed,
    Cancelled,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomNodeData {
    pub settings: Value,
    #[serde(default)]
    pub status: CustomNodeStatus,
    #[serde(default)]
    pub outputs: BTreeMap<String, CustomValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_request: Option<CustomNodeRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl CustomNodeData {
    pub fn for_definition(definition: &CustomNodeDefinition) -> Self {
        Self {
            settings: definition.default_settings.clone(),
            status: CustomNodeStatus::Idle,
            outputs: BTreeMap::new(),
            completed_request: None,
            error: None,
        }
    }

    pub fn from_value(
        value: &Value,
        definition: &CustomNodeDefinition,
    ) -> Result<Self, CreativeError> {
        validate_json(value)?;
        let data: Self = serde_json::from_value(value.clone())?;
        data.validate(definition)?;
        Ok(data)
    }

    pub fn validate(&self, definition: &CustomNodeDefinition) -> Result<(), CreativeError> {
        definition.validate()?;
        validate_settings(&self.settings)?;
        if let Some(error) = &self.error {
            validate_text(error, MAX_CUSTOM_TEXT_BYTES, "execution error", true)?;
        }
        if self.status == CustomNodeStatus::Complete {
            validate_outputs(&self.outputs, definition)?;
            let request = self.completed_request.as_ref().ok_or_else(|| {
                CreativeError("completed custom output lacks its input snapshot".into())
            })?;
            request.validate(definition)?;
            if request.settings != self.settings {
                return invalid("custom settings changed since execution");
            }
        } else if !self.outputs.is_empty() || self.completed_request.is_some() {
            return invalid("only completed custom nodes may publish outputs");
        }
        check_payload(self)
    }

    /// Call only after host/project validation and after rejecting stale work.
    /// The caller owns undo transactions and the current request/revision check.
    pub fn apply_result(
        &mut self,
        definition: &CustomNodeDefinition,
        request: &CustomNodeRequest,
        result: CustomNodeResult,
    ) -> Result<(), CreativeError> {
        result.validate(definition)?;
        let mut candidate = self.clone();
        candidate.outputs = result.outputs;
        candidate.completed_request = Some(request.clone());
        candidate.status = CustomNodeStatus::Complete;
        candidate.error = None;
        candidate.validate(definition)?;
        *self = candidate;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomNodeInput {
    /// Includes the original output port, so two outputs never collapse to a
    /// single source.data.asset or source.data.text field.
    pub source: PortRef,
    pub value: CustomValue,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomNodeRequest {
    pub node_id: NodeId,
    pub kind: String,
    pub executor: String,
    pub settings: Value,
    /// Keyed by destination input-port ID; every declared input must be present.
    pub inputs: BTreeMap<String, CustomNodeInput>,
}

impl CustomNodeRequest {
    pub fn from_node(
        node: &Node,
        graph: &Graph,
        definition: &CustomNodeDefinition,
    ) -> Result<Self, CreativeError> {
        definition.validate()?;
        graph.validate()?;
        if graph.node(&node.id) != Some(node) {
            return invalid("custom-node request must use the current graph node");
        }
        if node.kind != definition.kind || node.ports != definition.ports() {
            return invalid("custom node does not match its registered contract");
        }
        let data = CustomNodeData::from_value(&node.data, definition)?;
        let mut inputs = BTreeMap::new();
        let mut resolver = CustomOutputResolver::new(graph);
        for edge in graph
            .edges
            .values()
            .filter(|edge| edge.target.node == node.id)
        {
            let input = CustomNodeInput {
                source: edge.source.clone(),
                value: resolver.resolve(&edge.source)?,
            };
            if inputs.insert(edge.target.port.to_string(), input).is_some() {
                return invalid("custom input has more than one source");
            }
        }
        let request = Self {
            node_id: node.id.clone(),
            kind: definition.kind.clone(),
            executor: definition.executor.clone(),
            settings: data.settings,
            inputs,
        };
        request.validate(definition)?;
        Ok(request)
    }

    pub fn validate(&self, definition: &CustomNodeDefinition) -> Result<(), CreativeError> {
        definition.validate()?;
        validate_text(self.node_id.as_str(), 128, "node ID", false)?;
        if self.kind != definition.kind || self.executor != definition.executor {
            return invalid("request does not match its custom-node definition");
        }
        validate_settings(&self.settings)?;
        if self.inputs.len() != definition.inputs.len() {
            return invalid("every custom input requires one explicit connected value");
        }
        for port in &definition.inputs {
            let input = self
                .inputs
                .get(&port.id)
                .ok_or_else(|| CreativeError(format!("missing custom-node input: {}", port.id)))?;
            validate_text(input.source.node.as_str(), 128, "source node ID", false)?;
            validate_identifier(input.source.port.as_str(), "source port ID")?;
            input.value.validate_for(&port.data_type.data_type())?;
        }
        check_payload(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomNodeResult {
    pub outputs: BTreeMap<String, CustomValue>,
}

impl CustomNodeResult {
    pub fn validate(&self, definition: &CustomNodeDefinition) -> Result<(), CreativeError> {
        definition.validate()?;
        validate_outputs(&self.outputs, definition)?;
        check_payload(self)
    }
}

fn validate_outputs(
    outputs: &BTreeMap<String, CustomValue>,
    definition: &CustomNodeDefinition,
) -> Result<(), CreativeError> {
    if outputs.len() != definition.outputs.len() || outputs.len() > MAX_CUSTOM_PORTS {
        return invalid("result must contain every declared output and no extra output");
    }
    for port in &definition.outputs {
        outputs
            .get(&port.id)
            .ok_or_else(|| CreativeError(format!("missing custom-node output: {}", port.id)))?
            .validate_for(&port.data_type.data_type())?;
    }
    Ok(())
}

/// Read a single actual output. This never executes an upstream node. Custom
/// nodes must already be complete; missing values are errors, never mock assets.
pub fn resolve_node_output(node: &Node, port_id: &str) -> Result<CustomValue, CreativeError> {
    let port = node
        .ports
        .iter()
        .find(|port| port.id.as_str() == port_id && port.direction == PortDirection::Output)
        .ok_or_else(|| CreativeError(format!("missing output port: {port_id}")))?;
    if matches!(port.data_type, DataType::Any) {
        return invalid("output ports require an exact type");
    }
    let value = if node.kind.starts_with("custom.") {
        validate_json(&node.data)?;
        let data: CustomNodeData = serde_json::from_value(node.data.clone())?;
        if data.status != CustomNodeStatus::Complete {
            return invalid("upstream custom node has no completed result");
        }
        if data.outputs.len() != node.outputs().count() || data.outputs.len() > MAX_CUSTOM_PORTS {
            return invalid("upstream custom result does not match its output ports");
        }
        for output in node.outputs() {
            data.outputs
                .get(output.id.as_str())
                .ok_or_else(|| CreativeError("upstream custom output is missing".into()))?
                .validate_for(&output.data_type)?;
        }
        data.outputs.get(port_id).cloned().ok_or_else(|| {
            CreativeError(format!("upstream custom output has no value: {port_id}"))
        })?
    } else if node.kind == "text" && port_id == "text" {
        CustomValue::Text {
            text: node.data["text"]
                .as_str()
                .ok_or_else(|| CreativeError("text source has no text".into()))?
                .to_string(),
        }
    } else if node.kind == "mask" && port_id == "mask" {
        // Mask strokes have their own point/work budgets; do not impose the
        // smaller custom-JSON item count on a valid builtin mask editor state.
        check_serialized(&node.data)?;
        crate::mask_node::ensure_orientation_reviewed(&node.data)?;
        let data = MaskNodeData::from_value(&node.data)?;
        let source = data
            .state
            .source
            .as_ref()
            .ok_or_else(|| CreativeError("mask output has no source image".into()))?;
        if !data.output_is_current(source) {
            return invalid("mask output is stale or has not been applied");
        }
        CustomValue::Mask {
            asset_key: data.asset.clone(),
            source: source.clone(),
        }
    } else {
        let media_type = match (node.kind.as_str(), port_id) {
            ("image", "image") | ("compose", "image") => "image",
            ("video", "video") => "video",
            ("audio", "audio") => "audio",
            ("model", "model") => "model",
            ("generate_image" | "edit_image", "output") => "image",
            ("generate_video", "output") => "video",
            ("generate_audio", "output") => "audio",
            ("generate_model", "output") => "model",
            _ => return invalid("this host-defined output has no registered value adapter"),
        };
        if crate::GenerationKind::from_node_kind(&node.kind).is_some()
            && node.data["status"].as_str() != Some("complete")
        {
            return invalid("upstream generator has no completed result");
        }
        let asset_key = node.data["asset"]
            .as_str()
            .ok_or_else(|| CreativeError("media source has no asset key".into()))?
            .to_string();
        match media_type {
            "image" => CustomValue::Image { asset_key },
            "video" => CustomValue::Video { asset_key },
            "audio" => CustomValue::Audio { asset_key },
            "model" => CustomValue::Model { asset_key },
            _ => unreachable!(),
        }
    };
    value.validate_for(&port.data_type)?;
    Ok(value)
}

/// In addition to reading the exact output port, reject a builtin mask whose
/// original image connection changed since the mask was last applied.
pub struct CustomOutputResolver<'a> {
    graph: &'a Graph,
    visiting: BTreeSet<NodeId>,
    memo: BTreeMap<PortRef, Result<CustomValue, CreativeError>>,
    steps: usize,
}
impl<'a> CustomOutputResolver<'a> {
    pub fn new(graph: &'a Graph) -> Self {
        Self {
            graph,
            visiting: BTreeSet::new(),
            memo: BTreeMap::new(),
            steps: 0,
        }
    }
    pub fn resolve(&mut self, source: &PortRef) -> Result<CustomValue, CreativeError> {
        resolve_graph_output_inner(
            self.graph,
            source,
            &mut self.visiting,
            &mut self.memo,
            &mut self.steps,
        )
    }
}
pub fn resolve_graph_output(graph: &Graph, source: &PortRef) -> Result<CustomValue, CreativeError> {
    CustomOutputResolver::new(graph).resolve(source)
}
fn resolve_graph_output_inner(
    graph: &Graph,
    source: &PortRef,
    visiting: &mut BTreeSet<NodeId>,
    memo: &mut BTreeMap<PortRef, Result<CustomValue, CreativeError>>,
    steps: &mut usize,
) -> Result<CustomValue, CreativeError> {
    *steps += 1;
    if *steps > (crate::MAX_LINKS + crate::MAX_CARDS) * MAX_CUSTOM_PORTS {
        return invalid("output dependency resolution exceeds its work budget");
    }
    if let Some(value) = memo.get(source) {
        return value.clone();
    }
    if visiting.len() >= crate::MAX_CARDS || !visiting.insert(source.node.clone()) {
        return invalid("output dependencies contain a cycle or exceed the graph limit");
    }
    let result = resolve_graph_output_uncached(graph, source, visiting, memo, steps);
    visiting.remove(&source.node);
    // A custom node validates all outputs against the same immutable request.
    // Cache those ports together so a diamond graph expands each node once.
    if result.is_ok()
        && let Some(node) = graph
            .node(&source.node)
            .filter(|n| n.kind.starts_with("custom."))
        && let Ok(outputs) =
            serde_json::from_value::<BTreeMap<String, CustomValue>>(node.data["outputs"].clone())
    {
        for (port, value) in outputs {
            memo.insert(PortRef::new(node.id.clone(), port), Ok(value));
        }
    }
    memo.insert(source.clone(), result.clone());
    result
}
fn resolve_graph_output_uncached(
    graph: &Graph,
    source: &PortRef,
    visiting: &mut BTreeSet<NodeId>,
    memo: &mut BTreeMap<PortRef, Result<CustomValue, CreativeError>>,
    steps: &mut usize,
) -> Result<CustomValue, CreativeError> {
    let node = graph
        .node(&source.node)
        .ok_or_else(|| CreativeError("upstream node is missing".into()))?;
    let value = if let Some(kind) = crate::processing::ProcessingKind::from_node_kind(&node.kind) {
        if source.port.as_str() != "output" {
            return invalid("unknown processing output");
        }
        let data = crate::processing_node::ProcessingNodeData::from_value(&node.data)?;
        if data.settings.kind() != kind {
            return invalid("processing settings type does not match the node");
        }
        let edge = graph
            .edges
            .values()
            .find(|e| e.target.node == node.id && e.target.port.as_str() == "input")
            .ok_or_else(|| CreativeError("processing node has no input".into()))?;
        let input = resolve_graph_output_inner(graph, &edge.source, visiting, memo, steps)?;
        input.validate_for(&kind.input_type())?;
        if data.asset.is_empty() || data.source_asset.as_deref() != input.asset_key() {
            return invalid("processing output is stale or has not run");
        }
        match kind {
            crate::processing::ProcessingKind::Image => CustomValue::Image {
                asset_key: data.asset,
            },
            crate::processing::ProcessingKind::Video => CustomValue::Video {
                asset_key: data.asset,
            },
            crate::processing::ProcessingKind::Audio => CustomValue::Audio {
                asset_key: data.asset,
            },
        }
    } else {
        resolve_node_output(node, source.port.as_str())?
    };
    if node.kind.starts_with("custom.") {
        let data: CustomNodeData = serde_json::from_value(node.data.clone())?;
        let request = data
            .completed_request
            .as_ref()
            .ok_or_else(|| CreativeError("custom output is missing its input snapshot".into()))?;
        if request.node_id != node.id
            || request.kind != node.kind
            || request.settings != data.settings
        {
            return invalid("custom output no longer matches its node settings");
        }
        if request.inputs.len() != node.inputs().count() {
            return invalid("custom snapshot does not include every declared input");
        }
        for port in node.inputs() {
            request
                .inputs
                .get(port.id.as_str())
                .ok_or_else(|| CreativeError("custom input snapshot is missing".into()))?
                .value
                .validate_for(&port.data_type)?;
        }
        let incoming = graph
            .edges
            .values()
            .filter(|edge| edge.target.node == node.id)
            .collect::<Vec<_>>();
        if incoming.len() != request.inputs.len() {
            return invalid("custom input connections changed");
        }
        for edge in incoming {
            let expected = request
                .inputs
                .get(edge.target.port.as_str())
                .ok_or_else(|| CreativeError("custom input connection changed".into()))?;
            if expected.source != edge.source
                || expected.value
                    != resolve_graph_output_inner(graph, &edge.source, visiting, memo, steps)?
            {
                return invalid("custom input changed; run this node again");
            }
        }
    }
    if node.kind == "mask" {
        let CustomValue::Mask { source: origin, .. } = &value else {
            return invalid("mask node output is not a typed mask");
        };
        let mut sources = graph
            .edges
            .values()
            .filter(|edge| edge.target.node == node.id && edge.target.port.as_str() == "image");
        let edge = sources
            .next()
            .ok_or_else(|| CreativeError("mask has no connected original image".into()))?;
        if sources.next().is_some() {
            return invalid("mask has more than one original image");
        }
        let image = resolve_graph_output_inner(graph, &edge.source, visiting, memo, steps)?;
        if image.data_type() != DataType::Named("image".into())
            || image.asset_key() != Some(origin.asset_key.as_str())
        {
            return invalid("mask original image changed; apply a new mask before executing");
        }
    }
    let output = node
        .outputs()
        .find(|p| p.id == source.port)
        .ok_or_else(|| CreativeError("output port is missing".into()))?;
    value.validate_for(&output.data_type)?;
    Ok(value)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomExecutionError {
    Unavailable { executor: String },
    Cancelled,
    Invalid(CreativeError),
    Failed(String),
}

impl fmt::Display for CustomExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { executor } => write!(f, "custom executor unavailable: {executor}"),
            Self::Cancelled => f.write_str("custom-node execution cancelled"),
            Self::Invalid(error) => error.fmt(f),
            Self::Failed(error) => f.write_str(error),
        }
    }
}

impl std::error::Error for CustomExecutionError {}
impl From<CreativeError> for CustomExecutionError {
    fn from(error: CreativeError) -> Self {
        Self::Invalid(error)
    }
}

/// Cooperative cancellation: host implementations must poll `cancelled` during
/// expensive work. The registry also checks before entry and after return, so a
/// cancelled result can never be reported as success. Hosts own scheduling and
/// stale-result rejection; this synchronous trait does not block the UI itself.
pub trait CustomNodeExecutor: Send + Sync {
    fn execute(
        &self,
        request: &CustomNodeRequest,
        cancelled: &AtomicBool,
    ) -> Result<CustomNodeResult, CustomExecutionError>;
}

impl<F> CustomNodeExecutor for F
where
    F: Fn(&CustomNodeRequest, &AtomicBool) -> Result<CustomNodeResult, CustomExecutionError>
        + Send
        + Sync,
{
    fn execute(
        &self,
        request: &CustomNodeRequest,
        cancelled: &AtomicBool,
    ) -> Result<CustomNodeResult, CustomExecutionError> {
        self(request, cancelled)
    }
}

#[derive(Clone, Default)]
pub struct CustomNodeExecutors {
    executors: BTreeMap<String, Arc<dyn CustomNodeExecutor>>,
}

impl CustomNodeExecutors {
    /// Register trusted host code explicitly. Importing a template never calls
    /// this method and never enables an executable, URL, or command on its own.
    pub fn register(
        &mut self,
        id: impl Into<String>,
        executor: Arc<dyn CustomNodeExecutor>,
    ) -> Result<(), CreativeError> {
        let id = id.into();
        validate_identifier(&id, "executor ID")?;
        if self.executors.contains_key(&id) {
            return invalid("custom executor is already registered");
        }
        if self.executors.len() >= MAX_CUSTOM_NODE_TYPES {
            return invalid("custom executor count exceeds 64");
        }
        self.executors.insert(id, executor);
        Ok(())
    }

    pub fn is_available(&self, executor: &str) -> bool {
        self.executors.contains_key(executor)
    }
    pub fn ids(&self) -> Vec<String> {
        self.executors.keys().cloned().collect()
    }

    pub fn execute(
        &self,
        definition: &CustomNodeDefinition,
        request: &CustomNodeRequest,
        cancelled: &AtomicBool,
    ) -> Result<CustomNodeResult, CustomExecutionError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(CustomExecutionError::Cancelled);
        }
        request.validate(definition)?;
        let executor = self.executors.get(&definition.executor).ok_or_else(|| {
            CustomExecutionError::Unavailable {
                executor: definition.executor.clone(),
            }
        })?;
        if cancelled.load(Ordering::Acquire) {
            return Err(CustomExecutionError::Cancelled);
        }
        let result = executor.execute(request, cancelled);
        if cancelled.load(Ordering::Acquire) {
            return Err(CustomExecutionError::Cancelled);
        }
        if let Err(CustomExecutionError::Failed(message))
        | Err(CustomExecutionError::Invalid(CreativeError(message))) = &result
        {
            validate_text(message, MAX_CUSTOM_TEXT_BYTES, "execution error", true)?;
        }
        let result = result?;
        result.validate(definition)?;
        Ok(result)
    }
}

fn invalid<T>(message: impl Into<String>) -> Result<T, CreativeError> {
    Err(CreativeError(message.into()))
}

fn validate_identifier(value: &str, label: &str) -> Result<(), CreativeError> {
    if value.is_empty()
        || value.len() > 128
        || !value.as_bytes()[0].is_ascii_alphabetic()
        || value.split('.').any(str::is_empty)
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
    {
        return invalid(format!("invalid {label}"));
    }
    Ok(())
}

fn validate_named_type(name: &str) -> Result<(), CreativeError> {
    validate_identifier(name, "named port type")?;
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "any" | "text" | "string" | "number" | "bool" | "boolean" | "object"
    ) {
        return invalid("named types cannot impersonate wildcard or primitive types");
    }
    Ok(())
}

fn is_media_type(name: &str) -> bool {
    matches!(name, "image" | "video" | "audio" | "model" | "mask")
}

fn validate_text(
    text: &str,
    limit: usize,
    label: &str,
    allow_empty: bool,
) -> Result<(), CreativeError> {
    if text.len() > limit || (!allow_empty && text.trim().is_empty()) || text.contains('\0') {
        return invalid(format!("invalid or oversized {label}"));
    }
    Ok(())
}

fn validate_asset_key(key: &str, media_type: &str) -> Result<(), CreativeError> {
    let valid = key.strip_prefix("assets/").is_some_and(|name| {
        name.len() <= 128
            && name.rsplit_once('.').is_some_and(|(stem, extension)| {
                !stem.is_empty()
                    && stem
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                    && match media_type {
                        "image" => matches!(extension, "png" | "jpg" | "gif" | "webp"),
                        "video" => matches!(extension, "mp4" | "webm"),
                        "audio" => matches!(extension, "wav" | "mp3" | "flac" | "ogg" | "m4a"),
                        "model" => extension == "glb",
                        "mask" => extension == "png",
                        _ => false,
                    }
            })
    });
    if !valid {
        return invalid(format!("invalid project-relative {media_type} asset key"));
    }
    Ok(())
}

fn validate_settings(value: &Value) -> Result<(), CreativeError> {
    if !value.is_object() {
        return invalid("settings and object values must be JSON objects");
    }
    validate_json(value)
}

/// Check structure before serialization or cloning. The aggregate item and
/// string budgets apply equally to templates, settings, requests, and results.
fn validate_json(value: &Value) -> Result<(), CreativeError> {
    let mut pending = vec![(value, 0usize)];
    let mut items = 0usize;
    while let Some((value, depth)) = pending.pop() {
        items += 1;
        if depth > MAX_CUSTOM_JSON_DEPTH || items > MAX_CUSTOM_JSON_ITEMS {
            return invalid("custom JSON nesting or item budget exceeded");
        }
        match value {
            Value::String(text) => validate_text(text, MAX_CUSTOM_TEXT_BYTES, "JSON string", true)?,
            Value::Array(values) => {
                if values.len() > MAX_CUSTOM_JSON_ITEMS.saturating_sub(items + pending.len()) {
                    return invalid("custom JSON item budget exceeded");
                }
                pending.extend(values.iter().map(|value| (value, depth + 1)));
            }
            Value::Object(values) => {
                if values.len() > MAX_CUSTOM_JSON_ITEMS.saturating_sub(items + pending.len()) {
                    return invalid("custom JSON item budget exceeded");
                }
                for (key, value) in values {
                    validate_text(key, 128, "JSON key", true)?;
                    pending.push((value, depth + 1));
                }
            }
            _ => {}
        }
    }
    check_serialized(value)
}

fn check_json_bytes(bytes: usize) -> Result<(), CreativeError> {
    if bytes > MAX_CUSTOM_PAYLOAD_BYTES {
        return invalid("custom payload exceeds 64 KiB");
    }
    Ok(())
}

fn check_payload<T: Serialize + ?Sized>(value: &T) -> Result<(), CreativeError> {
    check_serialized(value)?;
    // Components are checked before entry. Once bytes are bounded, inspect the
    // complete wire object so nested settings and multiple inputs share budgets.
    validate_json(&serde_json::to_value(value)?)
}

fn check_serialized<T: Serialize + ?Sized>(value: &T) -> Result<(), CreativeError> {
    // Stop writing at the bound instead of allocating an arbitrarily large Vec.
    struct Budget(usize);
    impl std::io::Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_CUSTOM_PAYLOAD_BYTES.saturating_sub(self.0) {
                return Err(std::io::Error::other("custom payload exceeds 64 KiB"));
            }
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Budget(0), value).map_err(CreativeError::from)
}

#[cfg(test)]
#[path = "custom_nodes/tests.rs"]
mod tests;

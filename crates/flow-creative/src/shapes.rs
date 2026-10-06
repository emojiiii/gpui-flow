//! Persistent, bounded canvas primitives. These are zero-port `shape` nodes,
//! not image assets, mask inputs, SVG documents, scripts, or remote resources.
//!
//! Geometry lives in the core node. `data.shape` owns the version, primitive,
//! style and optional normalized line endpoints. Use `write_to_data` instead of
//! replacing `Node.data` with a serialized `ShapeData` to retain host metadata.
//! The caller owns editor transactions, history and containing-frame locks.

use gpui_flow_core::{Node, NodeId, Point, Size};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeStruct};
use serde_json::{Map, Value, json};

pub const SHAPE_NODE_KIND: &str = "shape";
pub const SHAPE_DATA_VERSION: u32 = 1;
pub const MIN_SHAPE_SIZE: f32 = 1.0;
pub const MAX_SHAPE_SIZE: f32 = 16_384.0;
pub const MAX_SHAPE_STROKE_WIDTH: f32 = 256.0;
pub const MAX_SHAPE_RADIUS: f32 = 8_192.0;
pub const DEFAULT_SHAPE_SIZE: Size = Size::new(240.0, 160.0);
pub const DEFAULT_LINE_SIZE: Size = Size::new(240.0, 1.0);
pub const MAX_SHAPE_PAYLOAD_BYTES: usize = 64 * 1024;
pub const MAX_SHAPE_JSON_DEPTH: usize = 16;
pub const MAX_SHAPE_JSON_ITEMS: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    Line,
}

impl ShapeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rectangle => "rectangle",
            Self::Ellipse => "ellipse",
            Self::Line => "line",
        }
    }

    pub const fn default_size(self) -> Size {
        match self {
            Self::Line => DEFAULT_LINE_SIZE,
            Self::Rectangle | Self::Ellipse => DEFAULT_SHAPE_SIZE,
        }
    }
}

/// RGB is an integer in `0x000000..=0xFFFFFF`; `None` means no paint.
/// Opacity applies to the whole primitive. Radius applies only to rectangles;
/// render it at `min(radius, width / 2, height / 2)`. Ellipses and lines ignore
/// radius, and lines ignore fill. A zero stroke width draws no stroke.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeStyle {
    pub fill: Option<u32>,
    pub stroke: Option<u32>,
    pub stroke_width: f32,
    pub radius: f32,
    pub opacity: f32,
}

impl Default for ShapeStyle {
    fn default() -> Self {
        Self {
            fill: Some(0xf2f0eb),
            stroke: Some(0x33343c),
            stroke_width: 2.0,
            radius: 0.0,
            opacity: 1.0,
        }
    }
}

impl ShapeStyle {
    pub fn validate(&self) -> Result<(), String> {
        for color in [self.fill, self.stroke].into_iter().flatten() {
            if color > 0xFFFFFF {
                return Err("shape colors must be RGB integers in 0..=16777215".into());
            }
        }
        finite_range(
            self.stroke_width,
            0.0,
            MAX_SHAPE_STROKE_WIDTH,
            "stroke_width",
        )?;
        finite_range(self.radius, 0.0, MAX_SHAPE_RADIUS, "radius")?;
        finite_range(self.opacity, 0.0, 1.0, "opacity")
    }

    fn from_value(value: &Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("shape.style must be an object")?;
        Ok(Self {
            fill: read_color(object, "fill")?,
            stroke: read_color(object, "stroke")?,
            stroke_width: read_number(object, "stroke_width", 0.0, MAX_SHAPE_STROKE_WIDTH)?,
            radius: read_number(object, "radius", 0.0, MAX_SHAPE_RADIUS)?,
            opacity: read_number(object, "opacity", 0.0, 1.0)?,
        })
    }
}

impl Serialize for ShapeStyle {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        let mut state = serializer.serialize_struct("ShapeStyle", 5)?;
        state.serialize_field("fill", &self.fill)?;
        state.serialize_field("stroke", &self.stroke)?;
        state.serialize_field("stroke_width", &self.stroke_width)?;
        state.serialize_field("radius", &self.radius)?;
        state.serialize_field("opacity", &self.opacity)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for ShapeStyle {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        validate_json_budget(&value).map_err(serde::de::Error::custom)?;
        Self::from_value(&value).map_err(serde::de::Error::custom)
    }
}

/// Endpoints relative to the node rectangle. Every coordinate is in `0..=1`.
/// Keeping both points preserves horizontal, vertical and reverse-drag lines
/// while the node retains a strictly positive, resizable bounding rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeLine {
    pub start: Point,
    pub end: Point,
}

impl Default for ShapeLine {
    fn default() -> Self {
        Self {
            start: Point::new(0.0, 0.5),
            end: Point::new(1.0, 0.5),
        }
    }
}

impl ShapeLine {
    pub fn validate(&self) -> Result<(), String> {
        for coordinate in [self.start.x, self.start.y, self.end.x, self.end.y] {
            finite_range(coordinate, 0.0, 1.0, "line endpoint")?;
        }
        if self.start == self.end {
            return Err("shape line endpoints must be distinct".into());
        }
        Ok(())
    }

    fn from_value(value: &Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("shape.line must be an object")?;
        let point = |name: &str| -> Result<Point, String> {
            let object = object
                .get(name)
                .and_then(Value::as_object)
                .ok_or_else(|| format!("shape.line.{name} must be an object"))?;
            Ok(Point::new(
                read_number(object, "x", 0.0, 1.0)?,
                read_number(object, "y", 0.0, 1.0)?,
            ))
        };
        let line = Self {
            start: point("start")?,
            end: point("end")?,
        };
        line.validate()?;
        Ok(line)
    }
}

impl Serialize for ShapeLine {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        let mut state = serializer.serialize_struct("ShapeLine", 2)?;
        state.serialize_field("start", &self.start)?;
        state.serialize_field("end", &self.end)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for ShapeLine {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        validate_json_budget(&value).map_err(serde::de::Error::custom)?;
        Self::from_value(&value).map_err(serde::de::Error::custom)
    }
}

/// The versioned object stored at `Node.data.shape`, not the whole node payload.
/// Known fields are required; unknown host fields are inert and preserved by
/// `write_to_data`. A line requires endpoints; other kinds allow no endpoints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeData {
    pub version: u32,
    pub kind: ShapeKind,
    pub style: ShapeStyle,
    pub line: Option<ShapeLine>,
}

impl ShapeData {
    pub fn new(kind: ShapeKind) -> Self {
        let mut style = ShapeStyle::default();
        if kind == ShapeKind::Line {
            style.fill = None;
        }
        Self {
            version: SHAPE_DATA_VERSION,
            kind,
            style,
            line: (kind == ShapeKind::Line).then(ShapeLine::default),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != SHAPE_DATA_VERSION {
            return Err("unsupported shape data version".into());
        }
        self.style.validate()?;
        match (self.kind, self.line) {
            (ShapeKind::Line, Some(line)) => line.validate(),
            (ShapeKind::Line, None) => Err("line shapes require normalized endpoints".into()),
            (_, Some(_)) => Err("only line shapes may contain endpoints".into()),
            (_, None) => Ok(()),
        }
    }

    /// Parse the complete `Node.data`, validating before any deep clone.
    pub fn from_data(data: &Value) -> Result<Self, String> {
        validate_json_budget(data)?;
        let data = data
            .as_object()
            .ok_or("shape node data must be an object")?;
        Self::from_value(
            data.get("shape")
                .ok_or("shape node data is missing shape")?,
        )
    }

    fn from_value(value: &Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("shape must be an object")?;
        let version = object
            .get("version")
            .and_then(Value::as_u64)
            .filter(|version| *version == u64::from(SHAPE_DATA_VERSION))
            .ok_or("shape.version must be the supported integer version 1")?;
        let kind = match object.get("kind").and_then(Value::as_str) {
            Some("rectangle") => ShapeKind::Rectangle,
            Some("ellipse") => ShapeKind::Ellipse,
            Some("line") => ShapeKind::Line,
            _ => return Err("shape.kind must be rectangle, ellipse, or line".into()),
        };
        let shape = Self {
            version: version as u32,
            kind,
            style: ShapeStyle::from_value(object.get("style").ok_or("shape.style is required")?)?,
            line: match object.get("line") {
                None | Some(Value::Null) => None,
                Some(value) => Some(ShapeLine::from_value(value)?),
            },
        };
        shape.validate()?;
        Ok(shape)
    }

    /// Also checks the zero-port kind contract and finite, bounded node geometry.
    pub fn from_node(node: &Node) -> Result<Self, String> {
        validate_node_shell(node)?;
        Self::from_data(&node.data)
    }

    pub fn to_data(&self) -> Result<Value, String> {
        self.write_to_data(&json!({}))
    }

    /// Return a replacement payload, retaining unknown root/shape/style/line
    /// fields. An absent shape may be initialized; an invalid existing shape is
    /// rejected rather than silently repaired or downgraded from a new version.
    pub fn write_to_data(&self, data: &Value) -> Result<Value, String> {
        self.validate()?;
        validate_json_budget(data)?;
        let data_object = data
            .as_object()
            .ok_or("shape node data must be an object")?;
        if data_object.contains_key("shape") {
            Self::from_data(data)?;
        }
        let owned = serde_json::to_value(self).map_err(|error| error.to_string())?;
        let mut result = data.clone();
        let shape = result
            .as_object_mut()
            .expect("validated object")
            .entry("shape")
            .or_insert_with(|| json!({}));
        merge_owned_fields(shape, &owned);
        // Changing a line into another primitive removes the owned geometry.
        if self.line.is_none() {
            shape
                .as_object_mut()
                .expect("validated shape")
                .remove("line");
        }
        validate_json_budget(&result)?;
        Ok(result)
    }

    /// Commit only after every check succeeds; returns whether the node changed.
    /// Group locks and editor Undo must be handled by the caller's command path.
    pub fn write_to_node(&self, node: &mut Node) -> Result<bool, String> {
        Self::from_node(node)?;
        if node.locked {
            return Err("shape node is locked".into());
        }
        let data = self.write_to_data(&node.data)?;
        if data == node.data {
            return Ok(false);
        }
        node.data = data;
        Ok(true)
    }
}

impl Serialize for ShapeData {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        let mut state =
            serializer.serialize_struct("ShapeData", if self.line.is_some() { 4 } else { 3 })?;
        state.serialize_field("version", &self.version)?;
        state.serialize_field("kind", &self.kind)?;
        state.serialize_field("style", &self.style)?;
        if let Some(line) = self.line {
            state.serialize_field("line", &line)?;
        }
        state.end()
    }
}

impl<'de> Deserialize<'de> for ShapeData {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        validate_json_budget(&value).map_err(serde::de::Error::custom)?;
        Self::from_value(&value).map_err(serde::de::Error::custom)
    }
}

/// Suitable as the Creative `NodeDefinition` payload-validator callback.
pub fn validate_shape_data(data: &Value) -> Result<(), String> {
    ShapeData::from_data(data).map(|_| ())
}

/// Include this in registry node validation to cover imported/pasted geometry.
pub fn validate_shape_node(node: &Node) -> Result<(), String> {
    ShapeData::from_node(node).map(|_| ())
}

pub fn create_shape_node(
    id: impl Into<NodeId>,
    title: impl Into<String>,
    position: Point,
    size: Size,
    shape: ShapeData,
) -> Result<Node, String> {
    let node = Node::new(id, title, position)
        .with_kind(SHAPE_NODE_KIND)
        .with_size(size)
        .with_data(shape.to_data()?);
    validate_shape_node(&node)?;
    Ok(node)
}

/// Normalize a drag without introducing negative/zero core node dimensions.
/// Lines retain the original endpoints even when drawn backwards or vertically.
/// A sub-unit minor axis receives symmetric padding; a line's length must be at
/// least one world unit. Rectangle/ellipse axes must each be at least one unit.
pub fn create_shape_node_from_points(
    id: impl Into<NodeId>,
    title: impl Into<String>,
    kind: ShapeKind,
    start: Point,
    end: Point,
) -> Result<Node, String> {
    if !start.is_finite() || !end.is_finite() {
        return Err("shape drag points must be finite".into());
    }
    let width = (end.x - start.x).abs();
    let height = (end.y - start.y).abs();
    let mut position = Point::new(start.x.min(end.x), start.y.min(end.y));
    let mut size = Size::new(width, height);
    let mut shape = ShapeData::new(kind);
    if kind == ShapeKind::Line {
        if !width.is_finite() || !height.is_finite() || start.distance(end) < MIN_SHAPE_SIZE {
            return Err("shape line must have finite bounds and length of at least 1".into());
        }
        size = Size::new(width.max(MIN_SHAPE_SIZE), height.max(MIN_SHAPE_SIZE));
        position -= Point::new((size.width - width) * 0.5, (size.height - height) * 0.5);
        let relative = |point: Point| {
            Point::new(
                (point.x - position.x) / size.width,
                (point.y - position.y) / size.height,
            )
        };
        shape.line = Some(ShapeLine {
            start: relative(start),
            end: relative(end),
        });
    }
    create_shape_node(id, title, position, size, shape)
}

fn validate_node_shell(node: &Node) -> Result<(), String> {
    if node.kind != SHAPE_NODE_KIND || !node.ports.is_empty() {
        return Err("shape nodes require kind shape and zero ports".into());
    }
    if node.id.as_str().trim().is_empty() || node.title.len() > 512 {
        return Err("shape node requires a nonempty ID and title of at most 512 bytes".into());
    }
    if !node.bounds().is_valid() {
        return Err("shape bounds must be finite".into());
    }
    finite_range(node.size.width, MIN_SHAPE_SIZE, MAX_SHAPE_SIZE, "width")?;
    finite_range(node.size.height, MIN_SHAPE_SIZE, MAX_SHAPE_SIZE, "height")
}

fn finite_range(value: f32, min: f32, max: f32, name: &str) -> Result<(), String> {
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(format!("shape {name} must be finite and in {min}..={max}"));
    }
    Ok(())
}

fn read_number(object: &Map<String, Value>, key: &str, min: f32, max: f32) -> Result<f32, String> {
    let value = object
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("shape {key} must be a number"))?;
    // Validate f64 before narrowing: e.g. 1.0000000001 must not become opacity 1.
    if !value.is_finite() || !(f64::from(min)..=f64::from(max)).contains(&value) {
        return Err(format!("shape {key} must be finite and in {min}..={max}"));
    }
    Ok(value as f32)
}

fn read_color(object: &Map<String, Value>, key: &str) -> Result<Option<u32>, String> {
    match object.get(key) {
        Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|value| *value <= 0xFFFFFF)
            .map(|value| Some(value as u32))
            .ok_or_else(|| format!("shape {key} must be an RGB integer or null")),
        None => Err(format!("shape {key} is required; use null for no paint")),
    }
}

fn merge_owned_fields(target: &mut Value, owned: &Value) {
    if let (Some(target), Some(owned)) = (target.as_object_mut(), owned.as_object()) {
        for (key, value) in owned {
            let target = target.entry(key.clone()).or_insert(Value::Null);
            if target.is_object() && value.is_object() {
                merge_owned_fields(target, value);
            } else {
                *target = value.clone();
            }
        }
    }
}

/// Limit unknown metadata as well as owned fields, before recursive cloning or
/// serialization. The wire-byte budget matches CreativeRegistry's 64 KiB cap.
fn validate_json_budget(value: &Value) -> Result<(), String> {
    let mut pending = vec![(value, 0usize)];
    let mut visited = 0usize;
    while let Some((value, depth)) = pending.pop() {
        visited += 1;
        if depth > MAX_SHAPE_JSON_DEPTH || visited > MAX_SHAPE_JSON_ITEMS {
            return Err("shape payload exceeds JSON depth or item limit".into());
        }
        match value {
            Value::String(value) if value.len() > MAX_SHAPE_PAYLOAD_BYTES => {
                return Err("shape payload exceeds 64 KiB".into());
            }
            Value::Array(values) => {
                if values.len() > MAX_SHAPE_JSON_ITEMS.saturating_sub(visited + pending.len()) {
                    return Err("shape payload exceeds JSON item limit".into());
                }
                pending.extend(values.iter().map(|value| (value, depth + 1)));
            }
            Value::Object(values) => {
                if values.len() > MAX_SHAPE_JSON_ITEMS.saturating_sub(visited + pending.len()) {
                    return Err("shape payload exceeds JSON item limit".into());
                }
                for (key, value) in values {
                    if key.len() > MAX_SHAPE_PAYLOAD_BYTES {
                        return Err("shape payload exceeds 64 KiB".into());
                    }
                    pending.push((value, depth + 1));
                }
            }
            _ => {}
        }
    }
    struct Budget(usize);
    impl std::io::Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_SHAPE_PAYLOAD_BYTES.saturating_sub(self.0) {
                return Err(std::io::Error::other("shape payload exceeds 64 KiB"));
            }
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Budget(0), value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_core::{DataType, Graph, Port};

    fn node(kind: ShapeKind) -> Node {
        create_shape_node(
            "shape-1",
            "Shape",
            Point::new(12.0, 24.0),
            kind.default_size(),
            ShapeData::new(kind),
        )
        .unwrap()
    }

    #[test]
    fn all_primitives_roundtrip_as_zero_port_nodes_with_explicit_paint() {
        for kind in [ShapeKind::Rectangle, ShapeKind::Ellipse, ShapeKind::Line] {
            let node = node(kind);
            assert_eq!(node.kind, "shape");
            assert!(node.ports.is_empty());
            assert_eq!(node.data["shape"]["version"], 1);
            assert_eq!(node.data["shape"]["kind"], kind.as_str());
            assert_eq!(node.data["shape"]["style"]["stroke"], 0x33343c);
            let encoded = serde_json::to_string(&node).unwrap();
            let decoded: Node = serde_json::from_str(&encoded).unwrap();
            assert_eq!(node, decoded);
            assert_eq!(
                ShapeData::from_node(&decoded).unwrap(),
                ShapeData::new(kind)
            );
            let shape: ShapeData = serde_json::from_value(node.data["shape"].clone()).unwrap();
            assert_eq!(shape.kind, kind);
            let mut graph = Graph::new();
            graph.add_node(node).unwrap();
        }
        let mut transparent = ShapeData::new(ShapeKind::Rectangle);
        transparent.style.fill = None;
        transparent.style.stroke = None;
        transparent.style.opacity = 0.0;
        let data = transparent.to_data().unwrap();
        assert!(data["shape"]["style"]["fill"].is_null());
        assert!(data["shape"]["style"]["stroke"].is_null());
        assert_eq!(ShapeData::from_data(&data).unwrap(), transparent);
    }

    #[test]
    fn malformed_and_future_payloads_cannot_be_parsed_or_overwritten() {
        let valid = node(ShapeKind::Rectangle).data;
        for (field, invalid) in [
            ("version", json!(2)),
            ("version", json!(1.0)),
            ("version", json!("1")),
            ("kind", json!("svg")),
            ("kind", json!("Rectangle")),
            ("kind", json!(null)),
            ("style", json!([])),
            ("line", json!(true)),
        ] {
            let mut data = valid.clone();
            data["shape"][field] = invalid;
            assert!(ShapeData::from_data(&data).is_err(), "{data}");
            assert!(
                ShapeData::new(ShapeKind::Rectangle)
                    .write_to_data(&data)
                    .is_err()
            );
            assert!(serde_json::from_value::<ShapeData>(data["shape"].clone()).is_err());
        }
        for field in ["version", "kind", "style"] {
            let mut data = valid.clone();
            data["shape"].as_object_mut().unwrap().remove(field);
            assert!(validate_shape_data(&data).is_err());
        }
        for field in ["fill", "stroke", "stroke_width", "radius", "opacity"] {
            let mut data = valid.clone();
            data["shape"]["style"]
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(validate_shape_data(&data).is_err());
        }
        for (field, invalid) in [
            ("fill", json!("#FFFFFF")),
            ("fill", json!(-1)),
            ("fill", json!(16777216)),
            ("stroke", json!(1.5)),
            ("stroke", json!(true)),
            ("opacity", json!("1")),
            ("radius", json!(null)),
            ("stroke_width", json!(false)),
        ] {
            let mut data = valid.clone();
            data["shape"]["style"][field] = invalid;
            assert!(validate_shape_data(&data).is_err(), "{data}");
        }
        for value in [Value::Null, json!([]), json!(true), json!("shape")] {
            assert!(ShapeData::from_data(&value).is_err());
            assert!(
                ShapeData::new(ShapeKind::Rectangle)
                    .write_to_data(&value)
                    .is_err()
            );
        }
        let mut missing_endpoints = node(ShapeKind::Line).data;
        missing_endpoints["shape"]
            .as_object_mut()
            .unwrap()
            .remove("line");
        assert!(validate_shape_data(&missing_endpoints).is_err());
    }

    #[test]
    fn numeric_limits_are_checked_before_rounding_and_before_serialization() {
        let mut data = node(ShapeKind::Rectangle).data;
        for (field, value) in [
            ("opacity", 1.0000000001),
            ("opacity", -0.0000000001),
            ("radius", 8192.000000001),
            ("radius", -1.0),
            ("stroke_width", 256.000000001),
            ("stroke_width", 1e100),
        ] {
            let mut invalid = data.clone();
            invalid["shape"]["style"][field] = json!(value);
            assert!(validate_shape_data(&invalid).is_err(), "{field}={value}");
            assert!(
                serde_json::from_value::<ShapeStyle>(invalid["shape"]["style"].clone()).is_err()
            );
        }
        data["shape"]["style"]["opacity"] = json!(0);
        data["shape"]["style"]["stroke_width"] = json!(256);
        data["shape"]["style"]["radius"] = json!(8192);
        data["shape"]["style"]["fill"] = json!(16777215);
        assert!(validate_shape_data(&data).is_ok());
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
            for field in 0..3 {
                let mut invalid = ShapeData::new(ShapeKind::Rectangle);
                match field {
                    0 => invalid.style.opacity = value,
                    1 => invalid.style.radius = value,
                    _ => invalid.style.stroke_width = value,
                }
                assert!(invalid.validate().is_err());
                assert!(serde_json::to_value(invalid).is_err());
                assert!(invalid.to_data().is_err());
            }
        }
        let mut line = node(ShapeKind::Line).data;
        line["shape"]["line"]["end"]["x"] = json!(1.0000000001);
        assert!(validate_shape_data(&line).is_err());
        line["shape"]["line"]["end"] = line["shape"]["line"]["start"].clone();
        assert!(validate_shape_data(&line).is_err());
    }

    #[test]
    fn edits_preserve_host_fields_and_fail_atomically() {
        let mut node = node(ShapeKind::Line);
        node.data["host"] = json!({"record_id":"draft-7","tags":["keep","all"]});
        node.data["shape"]["host"] = json!({"label":"extension"});
        node.data["shape"]["style"]["host"] = json!({"swatch_id":3});
        node.data["shape"]["line"]["host"] = json!("keep line metadata");
        node.data["shape"]["line"]["start"]["host"] = json!(true);
        let original = node.clone();
        let mut shape = ShapeData::from_node(&node).unwrap();
        shape.style.stroke = Some(0xff0000);
        shape.style.stroke_width = 9.0;
        shape.line.as_mut().unwrap().end.y = 1.0;
        assert!(shape.write_to_node(&mut node).unwrap());
        assert_eq!(node.data["host"], original.data["host"]);
        for path in ["host", "style", "line"] {
            if path == "host" {
                assert_eq!(node.data["shape"][path], original.data["shape"][path]);
            } else {
                assert_eq!(
                    node.data["shape"][path]["host"],
                    original.data["shape"][path]["host"]
                );
            }
        }
        assert_eq!(node.data["shape"]["line"]["start"]["host"], true);
        assert_eq!(node.position, original.position);
        assert_eq!(node.size, original.size);
        assert!(!shape.write_to_node(&mut node).unwrap());
        let before = node.clone();
        shape.style.opacity = f32::NAN;
        assert!(shape.write_to_node(&mut node).is_err());
        assert_eq!(node, before);
        node.locked = true;
        let locked = node.clone();
        assert!(
            ShapeData::new(ShapeKind::Line)
                .write_to_node(&mut node)
                .is_err()
        );
        assert_eq!(node, locked);
        node.locked = false;
        node.data["shape"]["version"] = json!(2);
        let future = node.clone();
        assert!(
            ShapeData::new(ShapeKind::Line)
                .write_to_node(&mut node)
                .is_err()
        );
        assert_eq!(node, future);
        for kind in ["image", "mask", "default"] {
            node.kind = kind.into();
            let other = node.clone();
            assert!(
                ShapeData::new(ShapeKind::Line)
                    .write_to_node(&mut node)
                    .is_err()
            );
            assert_eq!(node, other);
        }
    }

    #[test]
    fn drag_geometry_preserves_line_directions_and_rejects_invalid_nodes() {
        let rectangle = create_shape_node_from_points(
            "r",
            "Rectangle",
            ShapeKind::Rectangle,
            Point::new(80.0, 90.0),
            Point::new(20.0, 30.0),
        )
        .unwrap();
        assert_eq!(rectangle.position, Point::new(20.0, 30.0));
        assert_eq!(rectangle.size, Size::new(60.0, 60.0));
        for (start, end) in [
            (Point::new(100.0, 10.0), Point::new(10.0, 80.0)),
            (Point::new(10.0, 80.0), Point::new(100.0, 10.0)),
            (Point::new(100.0, 10.0), Point::new(10.0, 10.0)),
            (Point::new(10.0, 80.0), Point::new(10.0, 10.0)),
            (Point::new(10.0, 10.0), Point::new(100.0, 10.25)),
        ] {
            let node =
                create_shape_node_from_points("l", "Line", ShapeKind::Line, start, end).unwrap();
            assert!(node.size.is_positive());
            let line = ShapeData::from_node(&node).unwrap().line.unwrap();
            for (normalized, expected) in [(line.start, start), (line.end, end)] {
                let actual = node.position
                    + Point::new(
                        normalized.x * node.size.width,
                        normalized.y * node.size.height,
                    );
                assert_eq!(actual, expected);
            }
        }
        let mut node = node(ShapeKind::Ellipse);
        for size in [
            Size::new(0.0, 10.0),
            Size::new(-1.0, 10.0),
            Size::new(1.0, 16385.0),
            Size::new(f32::NAN, 10.0),
        ] {
            node.size = size;
            assert!(validate_shape_node(&node).is_err());
        }
        node.size = Size::new(1.0, 16384.0);
        assert!(validate_shape_node(&node).is_ok());
        node.position.x = f32::INFINITY;
        assert!(validate_shape_node(&node).is_err());
        node.position = Point::ZERO;
        node.ports.push(Port::output(
            "image",
            "Image",
            DataType::Named("image".into()),
        ));
        assert!(validate_shape_node(&node).is_err());
        for end in [
            Point::ZERO,
            Point::new(0.5, 0.0),
            Point::new(f32::NAN, 10.0),
            Point::new(f32::MAX, 0.0),
        ] {
            assert!(
                create_shape_node_from_points("bad", "Bad", ShapeKind::Line, Point::ZERO, end)
                    .is_err()
            );
        }
    }

    #[test]
    fn metadata_budgets_apply_before_clone_and_to_the_complete_edited_payload() {
        let shape = ShapeData::new(ShapeKind::Rectangle);
        let base = shape.to_data().unwrap();
        let mut too_large = base.clone();
        too_large["host"] = json!("x".repeat(MAX_SHAPE_PAYLOAD_BYTES));
        assert!(ShapeData::from_data(&too_large).is_err());
        assert!(shape.write_to_data(&too_large).is_err());
        let mut too_many = base.clone();
        too_many["host"] = json!(vec![0; MAX_SHAPE_JSON_ITEMS]);
        assert!(ShapeData::from_data(&too_many).is_err());
        let mut nested = Value::Null;
        for _ in 0..=MAX_SHAPE_JSON_DEPTH {
            nested = json!({"child": nested});
        }
        let mut too_deep = base.clone();
        too_deep["host"] = nested;
        assert!(shape.write_to_data(&too_deep).is_err());
        // This old payload fits exactly. Increasing a scalar must fail without
        // touching the source if it pushes the complete payload over 64 KiB.
        let mut full = base;
        full["host"] = json!("");
        let overhead = serde_json::to_vec(&full).unwrap().len();
        full["host"] = json!("x".repeat(MAX_SHAPE_PAYLOAD_BYTES - overhead));
        assert_eq!(
            serde_json::to_vec(&full).unwrap().len(),
            MAX_SHAPE_PAYLOAD_BYTES
        );
        assert!(ShapeData::from_data(&full).is_ok());
        let mut node = node(ShapeKind::Rectangle);
        node.data = full;
        let before = node.clone();
        let mut bigger = shape;
        bigger.style.stroke_width = 256.0;
        assert!(bigger.write_to_node(&mut node).is_err());
        assert_eq!(node, before);
    }
    #[test]
    fn registered_shape_resize_uses_guard_and_history_before_saving() {
        use gpui_flow_core::{Editor, Graph};
        use gpui_flow_interaction::{CanvasCommand, PluginHost};
        let registry = std::sync::Arc::new(crate::standard_registry());
        let node = registry
            .make_node("shape", "shape-a", Point::ZERO, None)
            .unwrap();
        assert!(node.ports.is_empty());
        let mut graph = Graph::new();
        graph.add_node(node).unwrap();
        let mut editor = Editor::new(graph).unwrap();
        let mut host = PluginHost::with_defaults();
        host.install(crate::CreativeGuard(registry.clone()))
            .unwrap();
        let before = editor.document();
        for size in [
            Size::new(0., 1.),
            Size::new(16385., 1.),
            Size::new(1., f32::NAN),
        ] {
            assert!(
                host.execute(
                    &mut editor,
                    CanvasCommand::ResizeNode {
                        node: "shape-a".into(),
                        size
                    }
                )
                .is_err()
            );
            assert_eq!(editor.document(), before);
        }
        host.execute(
            &mut editor,
            CanvasCommand::ResizeNode {
                node: "shape-a".into(),
                size: Size::new(1., 1.),
            },
        )
        .unwrap();
        registry.validate_document(&editor.document()).unwrap();
        let encoded = registry.encode_document(&editor.document()).unwrap();
        assert_eq!(
            registry.parse_document(&encoded).unwrap(),
            editor.document()
        );
        host.execute(&mut editor, CanvasCommand::Undo).unwrap();
        assert_eq!(editor.document(), before);
    }
}

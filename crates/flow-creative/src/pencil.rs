//! Independent, bounded freehand vector strokes stored as zero-port `pencil`
//! nodes. Core node geometry owns position and size; `data.pencil` owns the
//! version, style and ordered normalized points. A single point draws a dot.
//! The caller owns editor transactions, Undo and containing-frame locks.

use gpui_flow_core::{Node, NodeId, Point, Size};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeStruct};
use serde_json::{Map, Value, json};

pub const PENCIL_NODE_KIND: &str = "pencil";
pub const PENCIL_DATA_VERSION: u32 = 1;
pub const MIN_PENCIL_SIZE: f32 = 1.0;
pub const MAX_PENCIL_SIZE: f32 = 16_384.0;
pub const MIN_PENCIL_STROKE_WIDTH: f32 = 0.25;
pub const MAX_PENCIL_STROKE_WIDTH: f32 = 256.0;
pub const MAX_PENCIL_POINTS: usize = 8192;
pub const MAX_PENCIL_PAYLOAD_BYTES: usize = 256 * 1024;
pub const MAX_PENCIL_JSON_DEPTH: usize = 16;
// Each point is one object plus two coordinates, with room for host metadata.
pub const MAX_PENCIL_JSON_ITEMS: usize = MAX_PENCIL_POINTS * 3 + 2048;

/// Aggregate document guard keeps dense sketches bounded across paste/duplicate.
pub const MAX_PENCIL_GRAPH_POINTS: usize = 65_536;
pub const MAX_PENCIL_GRAPH_BYTES: usize = 2 * 1024 * 1024;
pub fn validate_pencil_collection<'a>(
    nodes: impl IntoIterator<Item = &'a Node>,
) -> Result<(), String> {
    let mut points = 0usize;
    let mut bytes = 0usize;
    for node in nodes.into_iter().filter(|n| n.kind == PENCIL_NODE_KIND) {
        points =
            points.saturating_add(node.data["pencil"]["points"].as_array().map_or(0, Vec::len));
        bytes = bytes.saturating_add(
            serde_json::to_vec(&node.data)
                .map_err(|e| e.to_string())?
                .len(),
        );
        if points > MAX_PENCIL_GRAPH_POINTS || bytes > MAX_PENCIL_GRAPH_BYTES {
            return Err("画布笔迹超过65536点或2 MiB，请先移除部分笔迹".into());
        }
    }
    Ok(())
}

/// RGB is an integer in `0x000000..=0xFFFFFF`. Width is in world units and
/// `0.25..=256`; opacity is in `0..=1`. Both floats must be finite. Render
/// points with round caps/joins, including a round dot for a one-point stroke.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PencilStyle {
    pub color: u32,
    pub stroke_width: f32,
    pub opacity: f32,
}

impl Default for PencilStyle {
    fn default() -> Self {
        Self {
            color: 0x33343c,
            stroke_width: 2.0,
            opacity: 1.0,
        }
    }
}

impl PencilStyle {
    pub fn validate(&self) -> Result<(), String> {
        if self.color > 0xFFFFFF {
            return Err("pencil color must be an RGB integer in 0..=16777215".into());
        }
        finite_range(
            self.stroke_width,
            MIN_PENCIL_STROKE_WIDTH,
            MAX_PENCIL_STROKE_WIDTH,
            "stroke_width",
        )?;
        finite_range(self.opacity, 0.0, 1.0, "opacity")
    }

    fn from_value(value: &Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("pencil.style must be an object")?;
        let color = object
            .get("color")
            .and_then(Value::as_u64)
            .filter(|color| *color <= 0xFFFFFF)
            .ok_or("pencil color must be an RGB integer in 0..=16777215")?;
        Ok(Self {
            color: color as u32,
            stroke_width: read_number(
                object,
                "stroke_width",
                MIN_PENCIL_STROKE_WIDTH,
                MAX_PENCIL_STROKE_WIDTH,
            )?,
            opacity: read_number(object, "opacity", 0.0, 1.0)?,
        })
    }
}

impl Serialize for PencilStyle {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        let mut state = serializer.serialize_struct("PencilStyle", 3)?;
        state.serialize_field("color", &self.color)?;
        state.serialize_field("stroke_width", &self.stroke_width)?;
        state.serialize_field("opacity", &self.opacity)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for PencilStyle {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        validate_json_budget(&value).map_err(serde::de::Error::custom)?;
        Self::from_value(&value).map_err(serde::de::Error::custom)
    }
}

/// The versioned object stored at `Node.data.pencil`, not the whole payload.
/// Every point coordinate is finite and in `0..=1`; order and repeated points
/// are retained. Both the 8192-point and 256 KiB JSON limits apply, so very
/// detailed coordinates or host metadata can exhaust bytes before point count.
#[derive(Debug, Clone, PartialEq)]
pub struct PencilData {
    pub version: u32,
    pub style: PencilStyle,
    pub points: Vec<Point>,
}

// A non-recursive serialization view lets validation count the owned JSON
// bytes before serializing public PencilData, without allocating a JSON tree.
#[derive(Serialize)]
struct PencilFields<'a> {
    version: u32,
    style: &'a PencilStyle,
    points: &'a [Point],
}

impl PencilData {
    pub fn new(points: Vec<Point>) -> Self {
        Self {
            version: PENCIL_DATA_VERSION,
            style: PencilStyle::default(),
            points,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != PENCIL_DATA_VERSION {
            return Err("unsupported pencil data version".into());
        }
        self.style.validate()?;
        validate_point_count(self.points.len())?;
        for point in &self.points {
            finite_range(point.x, 0.0, 1.0, "point x")?;
            finite_range(point.y, 0.0, 1.0, "point y")?;
        }
        serde_json::to_writer(JsonBudget(0), &self.fields()).map_err(|error| error.to_string())
    }

    fn fields(&self) -> PencilFields<'_> {
        PencilFields {
            version: self.version,
            style: &self.style,
            points: &self.points,
        }
    }

    /// Parse complete `Node.data`, checking unknown metadata before cloning.
    pub fn from_data(data: &Value) -> Result<Self, String> {
        validate_json_budget(data)?;
        let data = data
            .as_object()
            .ok_or("pencil node data must be an object")?;
        Self::from_value(
            data.get("pencil")
                .ok_or("pencil node data is missing pencil")?,
        )
    }

    fn from_value(value: &Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("pencil must be an object")?;
        let version = object
            .get("version")
            .and_then(Value::as_u64)
            .filter(|version| *version == u64::from(PENCIL_DATA_VERSION))
            .ok_or("pencil.version must be the supported integer version 1")?;
        let style =
            PencilStyle::from_value(object.get("style").ok_or("pencil.style is required")?)?;
        let points = object
            .get("points")
            .and_then(Value::as_array)
            .ok_or("pencil.points must be an array")?;
        validate_point_count(points.len())?;
        let points = points
            .iter()
            .map(|point| {
                let point = point.as_object().ok_or("pencil point must be an object")?;
                Ok(Point::new(
                    read_number(point, "x", 0.0, 1.0)?,
                    read_number(point, "y", 0.0, 1.0)?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let pencil = Self {
            version: version as u32,
            style,
            points,
        };
        pencil.validate()?;
        Ok(pencil)
    }

    /// Also checks zero ports, node kind, and finite, bounded node geometry.
    pub fn from_node(node: &Node) -> Result<Self, String> {
        validate_node_shell(node)?;
        Self::from_data(&node.data)
    }

    pub fn to_data(&self) -> Result<Value, String> {
        self.write_to_data(&json!({}))
    }

    /// Produce a replacement payload preserving unknown root/pencil/style
    /// fields. Points are an owned ordered geometry array; replacing geometry
    /// replaces its entries. When geometry is unchanged, point metadata is
    /// retained as well. Existing invalid or future schemas cannot be repaired
    /// or downgraded through this method.
    pub fn write_to_data(&self, data: &Value) -> Result<Value, String> {
        self.validate()?;
        validate_json_budget(data)?;
        let data_object = data
            .as_object()
            .ok_or("pencil node data must be an object")?;
        let existing = if data_object.contains_key("pencil") {
            Some(Self::from_data(data)?)
        } else {
            None
        };
        let mut owned = serde_json::to_value(self).map_err(|error| error.to_string())?;
        if existing
            .as_ref()
            .is_some_and(|old| old.points == self.points)
        {
            owned
                .as_object_mut()
                .expect("serialized pencil object")
                .remove("points");
        }
        let mut result = data.clone();
        let pencil = result
            .as_object_mut()
            .expect("validated object")
            .entry("pencil")
            .or_insert_with(|| json!({}));
        merge_owned_fields(pencil, &owned);
        validate_json_budget(&result)?;
        Ok(result)
    }

    /// Commit only after all checks succeed, returning whether data changed.
    /// The caller must use its command path for group locks and editor history.
    pub fn write_to_node(&self, node: &mut Node) -> Result<bool, String> {
        Self::from_node(node)?;
        if node.locked {
            return Err("pencil node is locked".into());
        }
        let data = self.write_to_data(&node.data)?;
        if data == node.data {
            return Ok(false);
        }
        node.data = data;
        Ok(true)
    }
}

impl Serialize for PencilData {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        self.fields().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PencilData {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        validate_json_budget(&value).map_err(serde::de::Error::custom)?;
        Self::from_value(&value).map_err(serde::de::Error::custom)
    }
}

/// Suitable for a Creative `NodeDefinition` payload-validator callback.
pub fn validate_pencil_data(data: &Value) -> Result<(), String> {
    PencilData::from_data(data).map(|_| ())
}

/// Include in registry validation to cover imported or pasted node geometry.
pub fn validate_pencil_node(node: &Node) -> Result<(), String> {
    PencilData::from_node(node).map(|_| ())
}

pub fn create_pencil_node(
    id: impl Into<NodeId>,
    title: impl Into<String>,
    position: Point,
    size: Size,
    pencil: PencilData,
) -> Result<Node, String> {
    let node = Node::new(id, title, position)
        .with_kind(PENCIL_NODE_KIND)
        .with_size(size)
        .with_data(pencil.to_data()?);
    validate_pencil_node(&node)?;
    Ok(node)
}

/// Preserve input order while normalizing world points into a positive node
/// rectangle. Sub-unit axes receive symmetric padding, so a horizontal or
/// vertical stroke has a one-unit minor axis and a single point is centered in
/// a one-unit square. Bounds describe the path centerline, excluding paint.
pub fn create_pencil_node_from_points(
    id: impl Into<NodeId>,
    title: impl Into<String>,
    points: &[Point],
    style: PencilStyle,
) -> Result<Node, String> {
    validate_point_count(points.len())?;
    style.validate()?;
    let mut minimum = points[0];
    let mut maximum = points[0];
    for point in points {
        if !point.is_finite() {
            return Err("pencil world points must be finite".into());
        }
        minimum.x = minimum.x.min(point.x);
        minimum.y = minimum.y.min(point.y);
        maximum.x = maximum.x.max(point.x);
        maximum.y = maximum.y.max(point.y);
    }
    // Widen subtraction before checking bounds, avoiding f32 overflow for
    // otherwise finite inputs and retaining padding for very small strokes.
    let span_x = f64::from(maximum.x) - f64::from(minimum.x);
    let span_y = f64::from(maximum.y) - f64::from(minimum.y);
    if span_x > f64::from(MAX_PENCIL_SIZE) || span_y > f64::from(MAX_PENCIL_SIZE) {
        return Err("pencil bounds must not exceed 16384 world units per axis".into());
    }
    let width = span_x.max(f64::from(MIN_PENCIL_SIZE));
    let height = span_y.max(f64::from(MIN_PENCIL_SIZE));
    let pad_x = (width - span_x) * 0.5;
    let pad_y = (height - span_y) * 0.5;
    let position = Point::new(
        (f64::from(minimum.x) - pad_x) as f32,
        (f64::from(minimum.y) - pad_y) as f32,
    );
    let normalized = points
        .iter()
        .map(|point| {
            Point::new(
                ((f64::from(point.x) - f64::from(minimum.x) + pad_x) / width) as f32,
                ((f64::from(point.y) - f64::from(minimum.y) + pad_y) / height) as f32,
            )
        })
        .collect();
    create_pencil_node(
        id,
        title,
        position,
        Size::new(width as f32, height as f32),
        PencilData {
            version: PENCIL_DATA_VERSION,
            style,
            points: normalized,
        },
    )
}

fn validate_node_shell(node: &Node) -> Result<(), String> {
    if node.kind != PENCIL_NODE_KIND || !node.ports.is_empty() {
        return Err("pencil nodes require kind pencil and zero ports".into());
    }
    if node.id.as_str().trim().is_empty() || node.title.len() > 512 {
        return Err("pencil node requires a nonempty ID and title of at most 512 bytes".into());
    }
    if !node.bounds().is_valid() {
        return Err("pencil bounds must be finite".into());
    }
    finite_range(node.size.width, MIN_PENCIL_SIZE, MAX_PENCIL_SIZE, "width")?;
    finite_range(node.size.height, MIN_PENCIL_SIZE, MAX_PENCIL_SIZE, "height")
}

fn validate_point_count(count: usize) -> Result<(), String> {
    if !(1..=MAX_PENCIL_POINTS).contains(&count) {
        return Err("pencil strokes require 1..=8192 points".into());
    }
    Ok(())
}

fn finite_range(value: f32, min: f32, max: f32, name: &str) -> Result<(), String> {
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(format!("pencil {name} must be finite and in {min}..={max}"));
    }
    Ok(())
}

fn read_number(object: &Map<String, Value>, key: &str, min: f32, max: f32) -> Result<f32, String> {
    let value = object
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("pencil {key} must be a number"))?;
    // Validate before narrowing so e.g. 1.0000000001 cannot become opacity 1.
    if !value.is_finite() || !(f64::from(min)..=f64::from(max)).contains(&value) {
        return Err(format!("pencil {key} must be finite and in {min}..={max}"));
    }
    Ok(value as f32)
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

struct JsonBudget(usize);

impl std::io::Write for JsonBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_PENCIL_PAYLOAD_BYTES.saturating_sub(self.0) {
            return Err(std::io::Error::other("pencil payload exceeds 256 KiB"));
        }
        self.0 += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Bound unknown metadata before recursive cloning and serialization. The
/// complete replacement payload, including host fields, shares the byte cap.
fn validate_json_budget(value: &Value) -> Result<(), String> {
    let mut pending = vec![(value, 0usize)];
    let mut visited = 0usize;
    while let Some((value, depth)) = pending.pop() {
        visited += 1;
        if depth > MAX_PENCIL_JSON_DEPTH || visited > MAX_PENCIL_JSON_ITEMS {
            return Err("pencil payload exceeds JSON depth or item limit".into());
        }
        match value {
            Value::String(value) if value.len() > MAX_PENCIL_PAYLOAD_BYTES => {
                return Err("pencil payload exceeds 256 KiB".into());
            }
            Value::Array(values) => {
                if values.len() > MAX_PENCIL_JSON_ITEMS.saturating_sub(visited + pending.len()) {
                    return Err("pencil payload exceeds JSON item limit".into());
                }
                pending.extend(values.iter().map(|value| (value, depth + 1)));
            }
            Value::Object(values) => {
                if values.len() > MAX_PENCIL_JSON_ITEMS.saturating_sub(visited + pending.len()) {
                    return Err("pencil payload exceeds JSON item limit".into());
                }
                for (key, value) in values {
                    if key.len() > MAX_PENCIL_PAYLOAD_BYTES {
                        return Err("pencil payload exceeds 256 KiB".into());
                    }
                    pending.push((value, depth + 1));
                }
            }
            _ => {}
        }
    }
    serde_json::to_writer(JsonBudget(0), value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_core::{DataType, Graph, Port};

    fn node() -> Node {
        create_pencil_node_from_points(
            "pencil-1",
            "Pencil",
            &[Point::new(100.0, 20.0), Point::new(10.0, 60.0)],
            PencilStyle::default(),
        )
        .unwrap()
    }

    #[test]
    fn independent_zero_port_stroke_and_typed_data_roundtrip() {
        let node = node();
        assert_eq!(node.kind, "pencil");
        assert!(node.ports.is_empty());
        assert_eq!(node.data["pencil"]["version"], 1);
        assert_eq!(node.data["pencil"]["style"]["color"], 0x33343c);
        let decoded: Node = serde_json::from_str(&serde_json::to_string(&node).unwrap()).unwrap();
        assert_eq!(decoded, node);
        let pencil = PencilData::from_node(&node).unwrap();
        let encoded = serde_json::to_string(&pencil).unwrap();
        assert_eq!(
            serde_json::from_str::<PencilData>(&encoded).unwrap(),
            pencil
        );
        assert_eq!(pencil.to_data().unwrap(), node.data);
        let mut graph = Graph::new();
        graph.add_node(node).unwrap();
    }

    #[test]
    fn reverse_horizontal_vertical_and_tiny_strokes_preserve_order() {
        for points in [
            vec![Point::new(100.0, 20.0), Point::new(10.0, 60.0)],
            vec![Point::new(10.0, 60.0), Point::new(100.0, 20.0)],
            vec![Point::new(100.0, 20.0), Point::new(10.0, 20.0)],
            vec![Point::new(10.0, 60.0), Point::new(10.0, 20.0)],
            vec![Point::new(10.0, 20.0), Point::new(10.25, 20.5)],
            vec![Point::new(-10.0, -20.0), Point::new(-10.0, -20.0)],
            vec![Point::ZERO, Point::new(20.0, 10.0), Point::ZERO],
        ] {
            let node =
                create_pencil_node_from_points("pencil", "Pencil", &points, PencilStyle::default())
                    .unwrap();
            assert!(node.size.is_positive());
            let normalized = PencilData::from_node(&node).unwrap().points;
            assert_eq!(normalized.len(), points.len());
            for (local, expected) in normalized.iter().zip(points) {
                let actual = node.position
                    + Point::new(local.x * node.size.width, local.y * node.size.height);
                assert_eq!(actual, expected);
            }
        }
        let horizontal = create_pencil_node_from_points(
            "h",
            "Horizontal",
            &[Point::new(10.0, 20.0), Point::new(30.0, 20.0)],
            PencilStyle::default(),
        )
        .unwrap();
        assert_eq!(horizontal.position, Point::new(10.0, 19.5));
        assert_eq!(horizontal.size, Size::new(20.0, 1.0));
        assert_eq!(
            PencilData::from_node(&horizontal).unwrap().points,
            vec![Point::new(0.0, 0.5), Point::new(1.0, 0.5)]
        );
    }

    #[test]
    fn single_point_is_a_centered_dot() {
        let node = create_pencil_node_from_points(
            "dot",
            "Dot",
            &[Point::new(10.0, 20.0)],
            PencilStyle::default(),
        )
        .unwrap();
        assert_eq!(node.position, Point::new(9.5, 19.5));
        assert_eq!(node.size, Size::new(1.0, 1.0));
        assert_eq!(
            PencilData::from_node(&node).unwrap().points,
            vec![Point::new(0.5, 0.5)]
        );
        assert!(
            create_pencil_node_from_points("empty", "Empty", &[], PencilStyle::default()).is_err()
        );
    }

    #[test]
    fn malformed_and_future_schemas_cannot_be_parsed_or_overwritten() {
        let valid = node().data;
        let pencil = PencilData::from_data(&valid).unwrap();
        for (field, invalid) in [
            ("version", json!(2)),
            ("version", json!(1.0)),
            ("version", json!("1")),
            ("style", json!([])),
            ("points", json!([])),
            ("points", json!(null)),
            ("points", json!([[0.0, 1.0]])),
            ("points", json!([{"x": 0}])),
        ] {
            let mut data = valid.clone();
            data["pencil"][field] = invalid;
            assert!(PencilData::from_data(&data).is_err(), "{data}");
            assert!(pencil.write_to_data(&data).is_err());
            assert!(serde_json::from_value::<PencilData>(data["pencil"].clone()).is_err());
        }
        for field in ["version", "style", "points"] {
            let mut data = valid.clone();
            data["pencil"].as_object_mut().unwrap().remove(field);
            assert!(validate_pencil_data(&data).is_err());
        }
        for field in ["color", "stroke_width", "opacity"] {
            let mut data = valid.clone();
            data["pencil"]["style"]
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(validate_pencil_data(&data).is_err());
        }
        for data in [Value::Null, json!([]), json!(true), json!({"pencil": null})] {
            assert!(PencilData::from_data(&data).is_err());
            assert!(pencil.write_to_data(&data).is_err());
        }
    }

    #[test]
    fn numeric_and_rgb_limits_are_checked_before_narrowing_or_serializing() {
        let valid = node().data;
        for (field, invalid) in [
            ("color", json!("#FFFFFF")),
            ("color", json!(-1)),
            ("color", json!(16777216)),
            ("color", json!(1.0)),
            ("color", json!(null)),
            ("opacity", json!(true)),
            ("opacity", json!(1.0000000001)),
            ("opacity", json!(-0.0000000001)),
            ("stroke_width", json!(0.24999999999)),
            ("stroke_width", json!(256.000000001)),
            ("stroke_width", json!(1e100)),
        ] {
            let mut data = valid.clone();
            data["pencil"]["style"][field] = invalid;
            assert!(validate_pencil_data(&data).is_err(), "{field}: {data}");
            assert!(
                serde_json::from_value::<PencilStyle>(data["pencil"]["style"].clone()).is_err()
            );
        }
        for value in [
            json!(1.0000000001),
            json!(-0.0000000001),
            json!(1e100),
            json!("0"),
            json!(null),
        ] {
            for axis in ["x", "y"] {
                let mut data = valid.clone();
                data["pencil"]["points"][0][axis] = value.clone();
                assert!(validate_pencil_data(&data).is_err());
            }
        }
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
            for field in 0..4 {
                let mut pencil = PencilData::new(vec![Point::new(0.5, 0.5)]);
                match field {
                    0 => pencil.style.stroke_width = value,
                    1 => pencil.style.opacity = value,
                    2 => pencil.points[0].x = value,
                    _ => pencil.points[0].y = value,
                }
                assert!(pencil.validate().is_err());
                assert!(serde_json::to_value(&pencil).is_err());
                assert!(pencil.to_data().is_err());
            }
        }
        for (color, width, opacity) in [(0, 0.25, 0.0), (0xFFFFFF, 256.0, 1.0)] {
            let style = PencilStyle {
                color,
                stroke_width: width,
                opacity,
            };
            assert!(style.validate().is_ok());
            assert_eq!(
                serde_json::from_value::<PencilStyle>(serde_json::to_value(style).unwrap())
                    .unwrap(),
                style
            );
        }
    }

    #[test]
    fn point_count_and_node_geometry_are_bounded() {
        let maximum = PencilData::new(vec![Point::ZERO; MAX_PENCIL_POINTS]);
        assert!(maximum.to_data().is_ok());
        // The point cap does not bypass the independent wire-byte budget.
        let detailed = PencilData::new(vec![Point::new(0.12345679, 0.12345679); MAX_PENCIL_POINTS]);
        assert!(detailed.to_data().is_err());
        let too_many = PencilData::new(vec![Point::ZERO; MAX_PENCIL_POINTS + 1]);
        assert!(too_many.validate().is_err());
        assert!(
            create_pencil_node_from_points(
                "many",
                "Many",
                &too_many.points,
                PencilStyle::default()
            )
            .is_err()
        );
        let mut data = maximum.to_data().unwrap();
        data["pencil"]["points"]
            .as_array_mut()
            .unwrap()
            .push(json!({"x": 0, "y": 0}));
        assert!(PencilData::from_data(&data).is_err());
        for point in [
            Point::new(f32::NAN, 0.0),
            Point::new(0.0, f32::INFINITY),
            Point::new(16_385.0, 0.0),
            Point::new(f32::MAX, 0.0),
        ] {
            assert!(
                create_pencil_node_from_points(
                    "bad",
                    "Bad",
                    &[Point::ZERO, point],
                    PencilStyle::default()
                )
                .is_err()
            );
        }
        let mut node = node();
        for size in [
            Size::new(0.0, 10.0),
            Size::new(-1.0, 10.0),
            Size::new(1.0, 16_385.0),
            Size::new(f32::NAN, 1.0),
        ] {
            node.size = size;
            assert!(validate_pencil_node(&node).is_err());
        }
        node.size = Size::new(1.0, 16_384.0);
        assert!(validate_pencil_node(&node).is_ok());
        node.position.x = f32::INFINITY;
        assert!(validate_pencil_node(&node).is_err());
        node.position = Point::ZERO;
        node.ports.push(Port::output(
            "image",
            "Image",
            DataType::Named("image".into()),
        ));
        assert!(validate_pencil_node(&node).is_err());
        node.ports.clear();
        node.kind = "mask".into();
        assert!(validate_pencil_node(&node).is_err());
        node.kind = PENCIL_NODE_KIND.into();
        node.id = " ".into();
        assert!(validate_pencil_node(&node).is_err());
        node.id = "valid".into();
        node.title = "x".repeat(513);
        assert!(validate_pencil_node(&node).is_err());
    }

    #[test]
    fn style_edits_preserve_host_metadata_and_fail_atomically() {
        let mut node = node();
        node.data["host"] = json!({"id": "draft-7", "tags": ["keep"]});
        node.data["pencil"]["host"] = json!({"label": "extension"});
        node.data["pencil"]["style"]["host"] = json!({"swatch": 3});
        node.data["pencil"]["points"][0]["host"] = json!({"sample": "keep"});
        let before = node.clone();
        let mut pencil = PencilData::from_node(&node).unwrap();
        pencil.style.color = 0xff0000;
        pencil.style.stroke_width = 8.0;
        pencil.style.opacity = 0.5;
        assert!(pencil.write_to_node(&mut node).unwrap());
        assert_eq!(node.data["host"], before.data["host"]);
        assert_eq!(node.data["pencil"]["host"], before.data["pencil"]["host"]);
        assert_eq!(
            node.data["pencil"]["style"]["host"],
            before.data["pencil"]["style"]["host"]
        );
        assert_eq!(
            node.data["pencil"]["points"],
            before.data["pencil"]["points"]
        );
        assert_eq!(node.position, before.position);
        assert_eq!(node.size, before.size);
        assert!(!pencil.write_to_node(&mut node).unwrap());
        let before = node.clone();
        pencil.points.clear();
        assert!(pencil.write_to_node(&mut node).is_err());
        assert_eq!(node, before);
        pencil.points = vec![Point::new(0.5, 0.5)];
        assert!(pencil.write_to_node(&mut node).unwrap());
        assert_eq!(node.data["pencil"]["points"], json!([{"x": 0.5, "y": 0.5}]));
        node.locked = true;
        let before = node.clone();
        assert!(pencil.write_to_node(&mut node).is_err());
        assert_eq!(node, before);
        node.locked = false;
        node.data["pencil"]["version"] = json!(2);
        let before = node.clone();
        assert!(pencil.write_to_node(&mut node).is_err());
        assert_eq!(node, before);
    }

    #[test]
    fn nested_metadata_and_complete_edited_payload_share_strict_budgets() {
        let pencil = PencilData::new(vec![Point::new(0.5, 0.5)]);
        let base = pencil.to_data().unwrap();
        let mut data = base.clone();
        data["host"] = json!("x".repeat(MAX_PENCIL_PAYLOAD_BYTES));
        assert!(pencil.write_to_data(&data).is_err());
        let mut data = base.clone();
        data["host"] = json!(vec![0; MAX_PENCIL_JSON_ITEMS]);
        assert!(PencilData::from_data(&data).is_err());
        let mut nested = Value::Null;
        for _ in 0..=MAX_PENCIL_JSON_DEPTH {
            nested = json!({"child": nested});
        }
        let mut data = base.clone();
        data["host"] = nested;
        assert!(pencil.write_to_data(&data).is_err());
        let mut data = base;
        data["host"] = json!("");
        let overhead = serde_json::to_vec(&data).unwrap().len();
        data["host"] = json!("x".repeat(MAX_PENCIL_PAYLOAD_BYTES - overhead));
        assert_eq!(
            serde_json::to_vec(&data).unwrap().len(),
            MAX_PENCIL_PAYLOAD_BYTES
        );
        assert!(PencilData::from_data(&data).is_ok());
        let mut node = node();
        node.data = data;
        let before = node.clone();
        let mut bigger = pencil;
        bigger.style.stroke_width = MAX_PENCIL_STROKE_WIDTH;
        assert!(bigger.write_to_node(&mut node).is_err());
        assert_eq!(node, before);
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use gpui_flow_core::{Editor, Graph};
    use gpui_flow_interaction::{CanvasCommand, PluginHost};
    #[test]
    fn registered_strokes_preserve_undo_and_reject_bad_resize_and_aggregate() {
        let registry = std::sync::Arc::new(crate::standard_registry());
        let mut node = create_pencil_node_from_points(
            "stroke",
            "笔迹",
            &[Point::ZERO, Point::new(50., 20.)],
            PencilStyle::default(),
        )
        .unwrap();
        node.data["host"] = json!("a".repeat(70 * 1024));
        registry.validate_node(&node).unwrap();
        let mut editor = Editor::new(Graph::new()).unwrap();
        let mut host = PluginHost::with_defaults();
        host.install(crate::CreativeGuard(registry.clone()))
            .unwrap();
        host.execute(&mut editor, CanvasCommand::AddNode(node.clone()))
            .unwrap();
        let saved = editor.document();
        assert!(
            host.execute(
                &mut editor,
                CanvasCommand::ResizeNode {
                    node: node.id.clone(),
                    size: Size::new(0., 20.)
                }
            )
            .is_err()
        );
        assert_eq!(editor.document(), saved);
        host.execute(&mut editor, CanvasCommand::Undo).unwrap();
        assert!(editor.graph().nodes.is_empty());
        host.execute(&mut editor, CanvasCommand::Redo).unwrap();
        assert_eq!(
            registry
                .parse_document(&registry.encode_document(&editor.document()).unwrap())
                .unwrap(),
            saved
        );
        assert!(validate_pencil_collection(std::iter::repeat_n(&node, 32)).is_err());
        assert!(validate_pencil_collection([&node]).is_ok());
    }
}

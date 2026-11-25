//! Strict JSON shapes at the Agent boundary. Persisted graph types retain their
//! existing deserialization behavior.

use super::{AgentEditRequest, AgentOperation};
use crate::{DataType, NodeId, Point, Port, PortDirection, PortId, PortRef, Size};
use serde::de::{value::MapAccessDeserializer, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Number, Value};
use std::{fmt, marker::PhantomData};

pub(super) struct Object<T>(T);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Serde structs normally accept both maps and sequences. The schema
        // specifies named fields, so only let the visitor consume maps.
        struct ObjectVisitor<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
            type Value = Object<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an object")
            }

            fn visit_map<M: MapAccess<'de>>(self, access: M) -> Result<Self::Value, M::Error> {
                T::deserialize(MapAccessDeserializer::new(access)).map(Object)
            }
        }
        deserializer.deserialize_map(ObjectVisitor(PhantomData))
    }
}

pub(super) fn object<'de, D, W, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    W: Deserialize<'de> + Into<T>,
{
    Object::<W>::deserialize(deserializer).map(|wire| wire.0.into())
}

pub(super) fn optional_object<'de, D, W, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    W: Deserialize<'de> + Into<T>,
{
    Option::<Object<W>>::deserialize(deserializer).map(|wire| wire.map(|wire| wire.0.into()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    schema_version: Unsigned,
    expected_revision: String,
    operations: Vec<Object<AgentOperation>>,
}

impl From<Object<Request>> for AgentEditRequest {
    fn from(Object(request): Object<Request>) -> Self {
        Self {
            schema_version: request.schema_version.0,
            expected_revision: request.expected_revision,
            operations: request.operations.into_iter().map(|wire| wire.0).collect(),
        }
    }
}

#[derive(Deserialize)]
#[serde(try_from = "Number")]
struct Unsigned<T: TryFrom<u64> = u32>(T);

impl<T: TryFrom<u64>> TryFrom<Number> for Unsigned<T> {
    type Error = &'static str;

    fn try_from(number: Number) -> Result<Self, Self::Error> {
        let error = "expected an unsigned integer in range";
        let value = match number.as_u64() {
            Some(value) => value,
            None => {
                // JSON Schema classifies 1.0 and 1e0 as integers.
                let value = number.as_f64().ok_or(error)?;
                if value.fract() != 0.0 || !(0.0..(u64::MAX as f64)).contains(&value) {
                    return Err(error);
                }
                value as u64
            }
        };
        T::try_from(value).map(Self).map_err(|_| error)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Position {
    x: f32,
    y: f32,
}

impl From<Position> for Point {
    fn from(value: Position) -> Self {
        Point::new(value.x, value.y)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Dimensions {
    width: f32,
    height: f32,
}

impl From<Dimensions> for Size {
    fn from(value: Dimensions) -> Self {
        Size::new(value.width, value.height)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Endpoint {
    node: NodeId,
    port: PortId,
}

impl From<Endpoint> for PortRef {
    fn from(value: Endpoint) -> Self {
        PortRef::new(value.node, value.port)
    }
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "name",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum DataKind {
    Any,
    Bool,
    Number,
    String,
    Object,
    Named(String),
}

impl From<DataKind> for DataType {
    fn from(kind: DataKind) -> Self {
        match kind {
            DataKind::Any => Self::Any,
            DataKind::Bool => Self::Bool,
            DataKind::Number => Self::Number,
            DataKind::String => Self::String,
            DataKind::Object => Self::Object,
            DataKind::Named(name) => Self::Named(name),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PortDefinition {
    id: PortId,
    label: String,
    direction: PortDirection,
    #[serde(deserialize_with = "object::<_, DataKind, _>")]
    data_type: DataType,
    max_connections: Option<Unsigned<usize>>,
    #[serde(default)]
    data: Value,
}

impl From<PortDefinition> for Port {
    fn from(value: PortDefinition) -> Self {
        Port {
            id: value.id,
            label: value.label,
            direction: value.direction,
            data_type: value.data_type,
            max_connections: value.max_connections.map(|value| value.0),
            data: value.data,
        }
    }
}

pub(super) fn ports<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Port>, D::Error> {
    let definitions = Vec::<Object<PortDefinition>>::deserialize(deserializer)?;
    Ok(definitions.into_iter().map(|wire| wire.0.into()).collect())
}

pub(super) fn optional_ports<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<Port>>, D::Error> {
    let definitions = Option::<Vec<Object<PortDefinition>>>::deserialize(deserializer)?;
    Ok(definitions.map(|defs| defs.into_iter().map(|wire| wire.0.into()).collect()))
}

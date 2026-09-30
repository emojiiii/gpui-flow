use crate::AnimationError;
use gpui_flow_engine::{ElementHandle, PropertyKey, PropertyType, SceneEngine};
use gpui_flow_scene::{Color, Paint, Point, Shape};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}
impl Easing {
    pub(crate) fn sample(self, t: f64) -> f64 {
        let t = t.clamp(0., 1.);
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t * t,
            Self::EaseOut => 1. - (1. - t).powi(3),
            Self::EaseInOut => t * t * (3. - 2. * t),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum TweenProperty {
    Translation,
    Opacity,
    FillColor,
    CustomNumber(String),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum TweenValue {
    Point(Point),
    Number(f64),
    Color(Color),
}
impl TweenValue {
    /// Sample a pure tween without mutating an engine or editor history.
    /// Finite progress is clamped to [0, 1]; exact endpoints are preserved.
    pub fn interpolate(
        &self,
        to: &Self,
        progress: f64,
        easing: Easing,
    ) -> Result<Self, AnimationError> {
        if !progress.is_finite() {
            return Err(invalid("interpolation progress must be finite"));
        }
        match (self, to) {
            (Self::Point(a), Self::Point(b)) if a.is_finite() && b.is_finite() => {}
            (Self::Number(a), Self::Number(b)) if a.is_finite() && b.is_finite() => {}
            (Self::Color(_), Self::Color(_)) => {}
            _ => {
                return Err(invalid(
                    "interpolation requires matching finite value types",
                ));
            }
        }
        interpolate(self, to, easing.sample(progress))
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tween {
    pub property: TweenProperty,
    pub to: TweenValue,
}
impl Tween {
    pub fn translation(to: Point) -> Self {
        Self {
            property: TweenProperty::Translation,
            to: TweenValue::Point(to),
        }
    }
    pub fn opacity(to: f32) -> Self {
        Self {
            property: TweenProperty::Opacity,
            to: TweenValue::Number(to as f64),
        }
    }
    pub fn fill_color(to: Color) -> Self {
        Self {
            property: TweenProperty::FillColor,
            to: TweenValue::Color(to),
        }
    }
    pub fn custom_number(name: impl Into<String>, to: f64) -> Self {
        Self {
            property: TweenProperty::CustomNumber(name.into()),
            to: TweenValue::Number(to),
        }
    }
}
fn invalid(message: &str) -> AnimationError {
    AnimationError::InvalidInput(message.into())
}
pub(crate) fn read(
    engine: &SceneEngine,
    target: ElementHandle,
    property: &TweenProperty,
) -> Result<TweenValue, AnimationError> {
    let object = engine.object(target)?;
    match property {
        TweenProperty::Translation => Ok(TweenValue::Point(Point::new(
            object.transform.tx,
            object.transform.ty,
        ))),
        TweenProperty::Opacity => Ok(TweenValue::Number(object.style.opacity as f64)),
        TweenProperty::FillColor => match object.style.fill {
            Some(Paint::Solid { color }) => Ok(TweenValue::Color(color)),
            _ => Err(invalid(
                "fill color tween requires an existing solid base fill",
            )),
        },
        TweenProperty::CustomNumber(name) => {
            let Shape::Custom(custom) = &object.shape else {
                return Err(invalid(
                    "custom numeric tween requires a registered custom element",
                ));
            };
            let definition = engine
                .registry()
                .get(&custom.kind)
                .ok_or_else(|| invalid("custom kind is not registered"))?;
            let schema = definition.schema();
            if !schema.descriptor(name).is_some_and(|descriptor| {
                matches!(descriptor.value_type, PropertyType::Number { .. })
            }) {
                return Err(invalid(
                    "custom tween requires a declared numeric schema property",
                ));
            }
            let properties = schema
                .resolve(&custom.data)
                .map_err(AnimationError::InvalidInput)?;
            let value = properties[name]
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| invalid("custom property must resolve to a finite number"))?;
            Ok(TweenValue::Number(value))
        }
    }
}
pub(crate) fn validate_pair(
    property: &TweenProperty,
    from: &TweenValue,
    to: &TweenValue,
) -> Result<(), AnimationError> {
    let valid = match (property, from, to) {
        (TweenProperty::Translation, TweenValue::Point(from), TweenValue::Point(to)) => {
            from.is_finite() && to.is_finite()
        }
        (TweenProperty::Opacity, TweenValue::Number(from), TweenValue::Number(to)) => {
            from.is_finite() && to.is_finite() && (0.0..=1.0).contains(to)
        }
        (TweenProperty::CustomNumber(_), TweenValue::Number(from), TweenValue::Number(to)) => {
            from.is_finite() && to.is_finite()
        }
        (TweenProperty::FillColor, TweenValue::Color(_), TweenValue::Color(_)) => true,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid("tween value type/range does not match property"))
    }
}
pub(crate) fn update(
    engine: &SceneEngine,
    target: ElementHandle,
    property: &TweenProperty,
    value: &TweenValue,
) -> Result<(PropertyKey, Value), AnimationError> {
    match (property, value) {
        (TweenProperty::Translation, TweenValue::Point(point)) => {
            let mut transform = engine.object(target)?.transform;
            transform.tx = point.x;
            transform.ty = point.y;
            Ok((
                PropertyKey::Transform,
                serde_json::to_value(transform)
                    .map_err(|error| AnimationError::InvalidInput(error.to_string()))?,
            ))
        }
        (TweenProperty::Opacity, TweenValue::Number(number)) => {
            Ok((PropertyKey::Opacity, json!(*number as f32)))
        }
        (TweenProperty::FillColor, TweenValue::Color(color)) => Ok((
            PropertyKey::Fill,
            serde_json::to_value(Paint::solid(*color))
                .map_err(|error| AnimationError::InvalidInput(error.to_string()))?,
        )),
        (TweenProperty::CustomNumber(name), TweenValue::Number(number)) => {
            Ok((PropertyKey::Custom(name.clone()), json!(number)))
        }
        _ => Err(invalid("tween value type does not match property")),
    }
}
pub(crate) fn interpolate(
    from: &TweenValue,
    to: &TweenValue,
    t: f64,
) -> Result<TweenValue, AnimationError> {
    // Exact endpoints avoid float drift and preserve the caller's color values.
    if t <= 0. {
        return Ok(from.clone());
    }
    if t >= 1. {
        return Ok(to.clone());
    }
    let lerp = |a: f64, b: f64| a * (1. - t) + b * t;
    match (from, to) {
        (TweenValue::Point(a), TweenValue::Point(b)) => {
            let value = Point::new(
                lerp(a.x as f64, b.x as f64) as f32,
                lerp(a.y as f64, b.y as f64) as f32,
            );
            if value.is_finite() {
                Ok(TweenValue::Point(value))
            } else {
                Err(invalid("point interpolation overflow"))
            }
        }
        (TweenValue::Number(a), TweenValue::Number(b)) => {
            let value = lerp(*a, *b);
            if value.is_finite() {
                Ok(TweenValue::Number(value))
            } else {
                Err(invalid("numeric interpolation overflow"))
            }
        }
        (TweenValue::Color(a), TweenValue::Color(b)) => {
            let channel = |a: u8, b: u8| lerp(a as f64, b as f64).round().clamp(0., 255.) as u8;
            Ok(TweenValue::Color(Color::rgba(
                channel(a.r, b.r),
                channel(a.g, b.g),
                channel(a.b, b.b),
                channel(a.a, b.a),
            )))
        }
        _ => Err(invalid("interpolation requires matching value types")),
    }
}

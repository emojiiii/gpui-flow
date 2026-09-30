use crate::{Affine, Bounds, Shape, Style};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, fmt};
#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DisplayId(pub String);
impl DisplayId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl From<&str> for DisplayId {
    fn from(id: &str) -> Self {
        Self(id.into())
    }
}
impl From<String> for DisplayId {
    fn from(id: String) -> Self {
        Self(id)
    }
}
impl fmt::Display for DisplayId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitPolicy {
    None,
    #[default]
    Painted,
    Geometry,
    Bounds,
}
fn yes() -> bool {
    true
}
/// Common retained-display state. Parent links and child ordering are owned by
/// SceneNode so primitive data cannot accidentally create multiple parents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayObject {
    pub id: DisplayId,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub classes: BTreeSet<String>,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub transform: Affine,
    #[serde(default)]
    pub style: Style,
    pub shape: Shape,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub z_index: i32,
    #[serde(default)]
    pub hit_policy: HitPolicy,
    #[serde(default = "yes")]
    pub hit_children: bool,
}
impl DisplayObject {
    pub fn new(id: impl Into<DisplayId>, shape: impl Into<Shape>) -> Self {
        Self {
            id: id.into(),
            name: String::new(),
            classes: BTreeSet::new(),
            data: Value::Null,
            transform: Affine::IDENTITY,
            style: Style::default(),
            shape: shape.into(),
            visible: true,
            locked: false,
            z_index: 0,
            hit_policy: HitPolicy::Painted,
            hit_children: true,
        }
    }
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
    pub fn with_class(mut self, class: impl Into<String>) -> Self {
        self.classes.insert(class.into());
        self
    }
    pub fn with_transform(mut self, transform: Affine) -> Self {
        self.transform = transform;
        self
    }
    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }
    pub fn paint_bounds(&self) -> Option<Bounds> {
        self.shape
            .local_bounds()
            .map(|b| b.expanded(self.style.stroke_extent()))
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.id.as_str().trim().is_empty() {
            return Err("display id cannot be empty".into());
        }
        if !self.transform.is_finite() {
            return Err("transform must be finite".into());
        }
        self.style.validate()?;
        if self.style.has_layers()
            && matches!(
                self.shape,
                Shape::Text(_) | Shape::Image(_) | Shape::Group(_)
            )
        {
            return Err(
                "layered paints require vector geometry; text, image and group are unsupported"
                    .into(),
            );
        }
        self.shape.validate()?;
        if self.paint_bounds().is_some_and(|b| !b.is_valid()) {
            return Err("paint bounds overflow".into());
        }
        Ok(())
    }
}

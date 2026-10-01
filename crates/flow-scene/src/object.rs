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
/// Alpha masks affect following z-ordered siblings until the next mask.
/// Only direct Group children may be masks. This changes rendering, not picking.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskMode {
    #[default]
    None,
    Alpha,
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
    #[serde(default)]
    pub mask: MaskMode,
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
            mask: MaskMode::None,
        }
    }
    pub fn with_mask(mut self, mask: MaskMode) -> Self {
        self.mask = mask;
        self
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
        let mut bounds = self.shape.local_bounds().map(|b| {
            // Preserve legacy centered conservative bounds. Aligned strokes
            // use their final mesh below, not a miter-limit-sized padding.
            let extent = self
                .style
                .strokes()
                .filter(|s| s.paint.is_visible() && s.alignment == crate::StrokeAlignment::Center)
                .map(crate::Stroke::extent)
                .fold(0.0, f32::max);
            b.expanded(extent)
        });
        for stroke in self.style.strokes().filter(|stroke| {
            stroke.width > 0.0 && stroke.paint.is_visible() && stroke.uses_advanced_geometry()
        }) {
            // Retain conservative local geometry bounds, including unpainted
            // dash gaps. Union the actual mesh as well, so f32 rounding of an
            // expanded AABB can never clip a square cap or miter tip.
            match self
                .shape
                .stroke_mesh_with_rule(stroke, self.style.fill_rule)
            {
                Ok(mesh) => {
                    if let Some(mesh_bounds) = mesh.bounds() {
                        bounds = Some(bounds.map_or(mesh_bounds, |b| b.union(mesh_bounds)));
                    }
                }
                Err(_) => {
                    return Some(Bounds::new(
                        crate::Point::new(f32::NAN, f32::NAN),
                        crate::Size::new(f32::NAN, f32::NAN),
                    ));
                }
            }
        }
        bounds
    }
    /// Conservative local visual bounds, including the outer shadow.
    /// Do not use these for picking, ItemBox::Stroke, layout or selection.
    pub fn effect_bounds(&self) -> Option<Bounds> {
        self.paint_bounds().map(|b| {
            self.style
                .shadow
                .map_or(b, |s| s.expand_bounds(b, Affine::IDENTITY))
        })
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
        for stroke in self
            .style
            .strokes()
            .filter(|stroke| stroke.uses_advanced_geometry())
        {
            if matches!(
                self.shape,
                Shape::Text(_) | Shape::Image(_) | Shape::Group(_)
            ) {
                return Err("advanced strokes require vector geometry".into());
            }
            if stroke.alignment != crate::StrokeAlignment::Center
                || (stroke.width > 0.0 && stroke.paint.is_visible())
            {
                // Scene insert/update/deserialization validate candidates before
                // committing, including all geometry budgets and finite output.
                self.shape
                    .stroke_mesh_with_rule(stroke, self.style.fill_rule)?;
            }
        }
        if self.effect_bounds().is_some_and(|b| !b.is_valid()) {
            return Err("paint bounds overflow".into());
        }
        Ok(())
    }
}

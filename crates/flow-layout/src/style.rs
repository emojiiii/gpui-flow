use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowDirection {
    #[default]
    Row,
    Column,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JustifyContent {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlignItems {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlignContent {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}
impl Insets {
    pub const fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }
}

/// Fixed-size Box/Frame content layout, in the container's local units.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FlowStyle {
    pub direction: FlowDirection,
    pub padding: Insets,
    /// Minimum gap between items on the main axis.
    pub gap: f32,
    /// Gap between wrapped rows/columns on the cross axis.
    pub line_gap: f32,
    pub wrap: bool,
    pub justify: JustifyContent,
    pub align_items: AlignItems,
    /// Placement of the complete wrapped line block on the cross axis.
    pub align_content: AlignContent,
}

/// Explicit layout input independent of the resulting shape size.
/// Constraints and basis use parent-local, axis-aligned box units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FlowItem {
    /// Excluded children retain their geometry/position and consume no slot.
    /// Hidden (`visible=false`) children still participate when this is true.
    pub in_flow: bool,
    /// Weight for positive free main-axis space. No implicit shrinking.
    pub grow: f32,
    /// Main-axis basis. A growing item defaults to zero (then its minimum);
    /// a fixed item defaults to its current measured box size.
    pub basis: Option<f32>,
    pub min_width: Option<f32>,
    pub max_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_height: Option<f32>,
    pub align_self: Option<AlignItems>,
}
impl Default for FlowItem {
    fn default() -> Self {
        Self {
            in_flow: true,
            grow: 0.0,
            basis: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            align_self: None,
        }
    }
}

pub(crate) fn validate_style(s: &FlowStyle) -> Result<(), String> {
    for (name, value) in [
        ("padding.top", s.padding.top),
        ("padding.right", s.padding.right),
        ("padding.bottom", s.padding.bottom),
        ("padding.left", s.padding.left),
        ("gap", s.gap),
        ("line_gap", s.line_gap),
    ] {
        if !value.is_finite() || value < 0.0 {
            return Err(format!("{name} must be finite and non-negative"));
        }
    }
    Ok(())
}
pub(crate) fn validate_item(s: &FlowItem) -> Result<(), String> {
    if !s.grow.is_finite() || s.grow < 0.0 {
        return Err("grow must be finite and non-negative".into());
    }
    if s.basis.is_some_and(|v| !v.is_finite() || v < 0.0) {
        return Err("basis must be finite and non-negative".into());
    }
    for (name, min, max) in [
        ("width", s.min_width, s.max_width),
        ("height", s.min_height, s.max_height),
    ] {
        if [min, max]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite() || v <= 0.0)
        {
            return Err(format!("{name} constraints must be finite and positive"));
        }
        if min.zip(max).is_some_and(|(a, b)| a > b) {
            return Err(format!("minimum {name} exceeds maximum"));
        }
    }
    Ok(())
}

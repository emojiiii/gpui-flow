use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowDirection {
    #[default]
    Row,
    Column,
    /// Visit children last-to-first, then place from the normal left edge.
    RowReverse,
    /// Visit children last-to-first, then place from the normal top edge.
    ColumnReverse,
}
impl FlowDirection {
    pub(crate) fn is_row(self) -> bool {
        matches!(self, Self::Row | Self::RowReverse)
    }
    pub(crate) fn is_reverse(self) -> bool {
        matches!(self, Self::RowReverse | Self::ColumnReverse)
    }
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

/// Ownership of one intrinsic container dimension. Content mode measures its
/// participating children; finite limits clamp the box, not the child geometry.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum AxisSizing {
    #[default]
    Fixed,
    Content {
        #[serde(default = "content_minimum")]
        min: f32,
        #[serde(default)]
        max: Option<f32>,
    },
}
fn content_minimum() -> f32 {
    1.0
}
impl AxisSizing {
    pub const fn content() -> Self {
        Self::Content {
            min: 1.0,
            max: None,
        }
    }
    pub const fn content_between(min: f32, max: Option<f32>) -> Self {
        Self::Content { min, max }
    }
    pub fn is_content(self) -> bool {
        matches!(self, Self::Content { .. })
    }
    pub(crate) fn resolve(self, natural: f64, fixed: f64) -> f64 {
        match self {
            Self::Fixed => fixed,
            Self::Content { min, max } => natural
                .max(min as f64)
                .min(max.map_or(f64::INFINITY, f64::from)),
        }
    }
    fn validate(self) -> Result<(), String> {
        if let Self::Content { min, max } = self
            && (!min.is_finite()
                || min <= 0.0
                || max.is_some_and(|max| !max.is_finite() || max < min))
        {
            return Err(
                "content sizing requires a finite positive minimum and maximum >= minimum".into(),
            );
        }
        Ok(())
    }
}

/// Spacing policy for items or wrapped lines on a fixed-size axis.
/// Automatic modes replace the numeric gap; they never resize child geometry.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapMode {
    /// Use the existing non-negative numeric gap.
    #[default]
    Fixed,
    /// Divide positive remaining space equally; overflowing items have zero gap.
    Auto,
    /// Divide remaining space equally, including negative gaps (overlap).
    Fit,
}
impl GapMode {
    pub(crate) fn is_fixed(&self) -> bool {
        *self == Self::Fixed
    }
    pub(crate) fn initial(self, fixed: f32) -> f64 {
        if self == Self::Fixed {
            f64::from(fixed)
        } else {
            0.0
        }
    }
    /// Upstream autoGap operates only on two or more slots. A singleton keeps
    /// normal alignment; there is no outer-edge spacing or division by zero.
    pub(crate) fn distribute(self, available: f64, occupied: f64, count: usize) -> Option<f64> {
        (count > 1 && (self == Self::Fit || (self == Self::Auto && available > occupied)))
            .then(|| (available - occupied) / (count - 1) as f64)
    }
}

/// Box/Frame content layout, in the container's intrinsic local units.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FlowStyle {
    pub width_sizing: AxisSizing,
    pub height_sizing: AxisSizing,
    pub direction: FlowDirection,
    pub padding: Insets,
    /// Minimum gap between items on the main axis, when gap_mode is Fixed.
    pub gap: f32,
    /// Optional automatic main-axis spacing. Missing JSON retains numeric gaps.
    #[serde(skip_serializing_if = "GapMode::is_fixed")]
    pub gap_mode: GapMode,
    /// Gap between wrapped rows/columns, when line_gap_mode is Fixed.
    pub line_gap: f32,
    /// Optional automatic cross-axis spacing between completed wrapped lines.
    #[serde(skip_serializing_if = "GapMode::is_fixed")]
    pub line_gap_mode: GapMode,
    pub wrap: bool,
    /// Reverse completed line placement on the cross axis. Ignored when actual
    /// wrapping is disabled; does not reverse children within a line.
    pub wrap_reverse: bool,
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
    s.width_sizing.validate()?;
    s.height_sizing.validate()?;
    let (main, cross) = if s.direction.is_row() {
        (s.width_sizing, s.height_sizing)
    } else {
        (s.height_sizing, s.width_sizing)
    };
    if !s.gap_mode.is_fixed() && main.is_content() {
        return Err("automatic item gaps require a fixed-size main axis".into());
    }
    if !s.line_gap_mode.is_fixed() && cross.is_content() {
        return Err("automatic line gaps require a fixed-size cross axis".into());
    }
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

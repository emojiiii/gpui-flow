use serde::{Deserialize, Serialize};

/// Placement of a Box/Frame's own paints relative to its children.
/// Missing document fields and existing constructors preserve the legacy order.
/// This explicit order does not reproduce Leafer's automatic isolation policy.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerPaintOrder {
    #[default]
    BeforeChildren,
    StrokeAfterChildren,
}
impl ContainerPaintOrder {
    /// Own paint before children, followed by an optional own paint after them.
    /// Each phase uses the container's ancestor clips, never its child clip.
    pub fn phases(self) -> (PaintPhase, Option<PaintPhase>) {
        match self {
            Self::BeforeChildren => (PaintPhase::All, None),
            Self::StrokeAfterChildren => (PaintPhase::Fill, Some(PaintPhase::Stroke)),
        }
    }
}

/// Runtime paint operation. Object identity, picking and event ancestry remain
/// one entry per object even when its renderer schedules two operations.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PaintPhase {
    #[default]
    All,
    Fill,
    Stroke,
}
impl PaintPhase {
    pub fn includes_fill(self) -> bool {
        matches!(self, Self::All | Self::Fill)
    }
    pub fn includes_stroke(self) -> bool {
        matches!(self, Self::All | Self::Stroke)
    }
}

//! Reusable plain-text measurement contracts with no default dependencies.
#![doc = include_str!("../README.md")]

#[cfg(feature = "gpui")]
pub mod gpui_backend;
#[cfg(feature = "gpui")]
pub use gpui_backend::GpuiTextMeasurer;
#[cfg(feature = "gpui")]
pub mod gpui_raster;
#[cfg(feature = "gpui")]
pub use gpui_raster::{GpuiTextRasterizer, RasterizedText, TextRasterRequest};

#[derive(Debug, Clone, PartialEq)]
pub struct TextMeasureRequest {
    pub content: String,
    pub font_family: String,
    pub font_size: f32,
    pub font_weight: u16,
    /// Absolute logical line-box height, not a glyph's ink bounds.
    pub line_height: f32,
    pub wrap_width: Option<f32>,
}
impl TextMeasureRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.content.len() > 65_536 || self.font_family.len() > 512 {
            return Err(
                "text measurement allows at most 64KiB UTF-8 content and 512-byte family".into(),
            );
        }
        if self.font_family.trim().is_empty()
            || !self.font_size.is_finite()
            || self.font_size <= 0.
            || !(1..=1000).contains(&self.font_weight)
            || !self.line_height.is_finite()
            || self.line_height <= 0.
            || self.wrap_width.is_some_and(|w| !w.is_finite() || w <= 0.)
        {
            return Err("invalid text measurement input".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct TextMetrics {
    /// May be zero for empty text. Scene geometry retains a minimum width of 1.
    pub width: f32,
    pub height: f32,
    pub line_height: f32,
    pub lines: usize,
}
impl TextMetrics {
    pub fn validate(&self) -> Result<(), String> {
        if !self.width.is_finite()
            || self.width < 0.
            || !self.height.is_finite()
            || self.height <= 0.
            || !self.line_height.is_finite()
            || self.line_height <= 0.
            || !(1..=65_537).contains(&self.lines)
        {
            return Err("backend returned invalid text metrics".into());
        }
        Ok(())
    }
}
/// A deterministic host service; the layout/model layer does not depend on GPUI.
/// Errors and panics abort the complete candidate before document/history writes.
/// A cache must include all request fields and discard entries on invalidate.
pub trait TextMeasurer: Send + Sync {
    fn measure(&self, request: &TextMeasureRequest) -> Result<TextMetrics, String>;
    fn invalidate(&self) {}
}

/// Alignment of a clipped text block. Overflow remains top-aligned, matching
/// the pinned clipped-overflow layout rather than shifting the first line away.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}
impl VerticalAlign {
    pub fn offset(self, available_height: f32, content_height: f32) -> f32 {
        if !available_height.is_finite()
            || !content_height.is_finite()
            || available_height <= content_height
        {
            return 0.;
        }
        let free = available_height - content_height;
        match self {
            Self::Top => 0.,
            Self::Middle => free * 0.5,
            Self::Bottom => free,
        }
    }
}

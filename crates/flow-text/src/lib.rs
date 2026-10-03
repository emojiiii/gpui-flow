//! Reusable plain-text measurement contracts with no default dependencies.
#![doc = include_str!("../README.md")]

#[cfg(feature = "gpui")]
pub mod gpui_backend;
#[cfg(feature = "gpui")]
pub mod gpui_decoration;
#[cfg(feature = "gpui")]
pub mod gpui_overflow;
#[cfg(feature = "gpui")]
pub use gpui_backend::GpuiTextMeasurer;
#[cfg(feature = "gpui")]
mod gpui_inline;
#[cfg(feature = "gpui")]
mod gpui_placement;
#[cfg(feature = "gpui")]
pub mod gpui_raster;
#[cfg(feature = "gpui")]
pub use gpui_raster::{GpuiTextRasterizer, RasterizedText, TextRasterRequest};

#[cfg(feature = "static-outlines")]
pub mod static_outlines;

#[cfg(feature = "static-outline-registration")]
pub mod gpui_outlines;
#[cfg(feature = "static-outline-registration")]
pub mod outline_registration;
#[cfg(feature = "static-outline-registration")]
pub use gpui_outlines::{OutlinedText, PositionedGlyphOutline, TextOutlineError};

mod writing_mode;
pub use writing_mode::{UnsupportedWritingMode, WritingMode, WritingModeError};
#[cfg(feature = "gpui")]
pub mod gpui_writing_mode;

mod text_case;
pub use text_case::{MAX_TEXT_BYTES, TextCase};

#[derive(Debug, Clone, PartialEq)]
pub struct TextMeasureRequest {
    pub content: String,
    pub font_family: String,
    pub font_size: f32,
    pub font_weight: u16,
    /// Request the native italic face; no synthetic geometry skew.
    pub italic: bool,
    /// Absolute logical line-box height, not a glyph's ink bounds.
    pub line_height: f32,
    pub text_case: TextCase,
    /// Additional absolute logical space between hard-newline paragraphs.
    pub para_spacing: f32,
    /// Extra logical advance after each native shaping cluster (including the last).
    pub letter_spacing: f32,
    /// Extra logical advance after each ASCII space cluster.
    pub word_spacing: f32,
    /// Reserved advance before the first visual row of each hard-newline paragraph.
    pub para_indent: f32,
    pub wrap_width: Option<f32>,
}
impl TextMeasureRequest {
    /// Same hard-newline paragraph contract as GPUI shape_text, including a
    /// final empty paragraph. Soft wraps never change the paragraph count.
    pub fn paragraph_breaks(&self) -> usize {
        self.content.bytes().filter(|b| *b == b'\n').count()
    }
    pub fn block_height(&self, visual_rows: usize) -> f32 {
        self.line_height * visual_rows as f32 + self.para_spacing * self.paragraph_breaks() as f32
    }
    pub(crate) fn has_inline_layout(&self) -> bool {
        self.letter_spacing != 0. || self.word_spacing != 0. || self.para_indent != 0.
    }
    pub(crate) fn validate_inline_layout(&self) -> Result<(), String> {
        if [self.letter_spacing, self.word_spacing, self.para_indent]
            .iter()
            .any(|n| !n.is_finite() || !(0. ..=1_000_000.).contains(n))
            || (self.has_inline_layout() && self.content.len() > MAX_TEXT_BYTES)
        {
            return Err("text spacing/indent must be finite within 0..=1000000; styled source at most 64KiB".into());
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        self.validate_inline_layout()?;
        if self.content.len() > 65_536 || self.font_family.len() > 512 {
            return Err(
                "text measurement allows at most 64KiB UTF-8 content and 512-byte family".into(),
            );
        }
        self.text_case.presentation(&self.content)?;
        if self.font_family.trim().is_empty()
            || !self.para_spacing.is_finite()
            || !(0. ..=1_000_000.).contains(&self.para_spacing)
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
    /// Opt-in axis layout. Existing hosts explicitly reject new modes.
    fn measure_writing_mode(
        &self,
        request: &TextMeasureRequest,
        mode: WritingMode,
    ) -> Result<TextMetrics, WritingModeError> {
        if mode == WritingMode::X {
            self.measure(request).map_err(Into::into)
        } else {
            Err(WritingModeError::Unsupported(
                UnsupportedWritingMode::Backend,
            ))
        }
    }
    /// Distance from the first logical line-box top to its alphabetic baseline.
    /// Legacy measurement implementations remain source-compatible and fail
    /// explicitly only when a caller opts into baseline alignment.
    fn first_baseline(&self, _request: &TextMeasureRequest) -> Result<f32, String> {
        Err("text backend does not provide first-baseline metrics".into())
    }
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

#[cfg(test)]
mod alignment_tests {
    use super::VerticalAlign;
    #[test]
    fn vertical_alignment_offsets_only_nonoverflowing_blocks() {
        assert_eq!(VerticalAlign::Top.offset(80., 20.), 0.);
        assert_eq!(VerticalAlign::Middle.offset(80., 20.), 30.);
        assert_eq!(VerticalAlign::Bottom.offset(80., 20.), 60.);
        for align in [
            VerticalAlign::Top,
            VerticalAlign::Middle,
            VerticalAlign::Bottom,
        ] {
            assert_eq!(align.offset(20., 40.), 0.);
            assert_eq!(align.offset(20., 20.), 0.);
            assert_eq!(align.offset(f32::INFINITY, 20.), 0.);
        }
    }
}

/// Fixed-box paint policy. Intrinsic measurement always uses original content.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TextOverflow {
    #[default]
    Clip,
    Ellipsis,
}

/// Plain text decoration, painted with the glyph fill color.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TextDecoration {
    #[default]
    None,
    Underline,
    Strikethrough,
    UnderlineStrikethrough,
}

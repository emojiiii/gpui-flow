use crate::{Bounds, Point, Size};
use serde::{Deserialize, Serialize};
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextVerticalAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextOverflow {
    #[default]
    Clip,
    Ellipsis,
}
fn is_clip(value: &TextOverflow) -> bool {
    *value == TextOverflow::Clip
}
fn is_top(value: &TextVerticalAlign) -> bool {
    *value == TextVerticalAlign::Top
}
fn no_padding(value: &[f32; 4]) -> bool {
    *value == [0.; 4]
}
/// Text with an explicit layout box. Font shaping/measurement is performed by
/// the host adapter, which can update size with actual measured dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Text {
    pub content: String,
    pub size: Size,
    pub font_family: String,
    pub font_size: f32,
    #[serde(default = "default_font_weight")]
    pub font_weight: u16,
    pub line_height: f32,
    pub align: TextAlign,
    pub wrap: bool,
    /// Top, right, bottom, left padding in local logical units.
    #[serde(default, skip_serializing_if = "no_padding")]
    pub padding: [f32; 4],
    #[serde(default, skip_serializing_if = "is_top")]
    pub vertical_align: TextVerticalAlign,
    #[serde(default, skip_serializing_if = "is_clip")]
    pub overflow: TextOverflow,
}
impl Text {
    pub fn new(content: impl Into<String>, size: Size) -> Self {
        Self {
            content: content.into(),
            size,
            font_family: "sans-serif".into(),
            font_size: 14.0,
            font_weight: default_font_weight(),
            line_height: 1.2,
            align: TextAlign::Start,
            wrap: true,
            padding: [0.; 4],
            vertical_align: TextVerticalAlign::Top,
            overflow: TextOverflow::Clip,
        }
    }
    /// The shared wrapping, alignment and glyph clipping box. Call validate at
    /// mutation boundaries; public fields can temporarily form invalid geometry.
    pub fn content_bounds(&self) -> Bounds {
        let [top, right, bottom, left] = self.padding;
        Bounds::new(
            Point::new(left, top),
            Size::new(
                (self.size.width as f64 - left as f64 - right as f64) as f32,
                (self.size.height as f64 - top as f64 - bottom as f64) as f32,
            ),
        )
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.overflow == TextOverflow::Ellipsis && self.content.len() > 65_536 {
            return Err("ellipsis text allows at most 64KiB UTF-8 content".into());
        }
        if !self.size.is_positive()
            || !self.font_size.is_finite()
            || self.font_size <= 0.
            || !(1..=1000).contains(&self.font_weight)
            || !self.line_height.is_finite()
            || self.line_height <= 0.
            || self.font_family.trim().is_empty()
        {
            return Err("invalid text metrics, size or font family".into());
        }
        if self
            .padding
            .iter()
            .any(|n| !n.is_finite() || *n < 0. || *n > 1_000_000.)
            || !self.content_bounds().is_valid()
            || !self.content_bounds().size.is_positive()
        {
            return Err("text padding must leave a finite positive content box".into());
        }
        Ok(())
    }
    pub fn bounds(&self) -> Bounds {
        Bounds::new(Point::ZERO, self.size)
    }
}

fn default_font_weight() -> u16 {
    400
}

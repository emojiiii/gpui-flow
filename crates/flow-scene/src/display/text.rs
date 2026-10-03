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
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextDecoration {
    #[default]
    None,
    Underline,
    Strikethrough,
    UnderlineStrikethrough,
}
/// Verified textCase subset. Title and font small-caps are not approximated.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextCase {
    #[default]
    None,
    Upper,
    Lower,
}
/// Pinned Leafer declares other modes but implements horizontal writing only.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextWritingMode {
    #[default]
    X,
}
fn horizontal(value: &TextWritingMode) -> bool {
    *value == TextWritingMode::X
}
fn zero_origin(value: &Point) -> bool {
    *value == Point::ZERO
}
fn unchanged_case(value: &TextCase) -> bool {
    *value == TextCase::None
}
fn no_spacing(value: &f32) -> bool {
    *value == 0.
}
fn undecorated(value: &TextDecoration) -> bool {
    *value == TextDecoration::None
}
fn is_false(value: &bool) -> bool {
    !*value
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
    /// Computed local offset owned by measured layout, not an object transform.
    #[serde(default, skip_serializing_if = "zero_origin")]
    pub layout_origin: Point,
    pub font_family: String,
    pub font_size: f32,
    #[serde(default = "default_font_weight")]
    pub font_weight: u16,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    pub line_height: f32,
    /// Presentation-only Unicode case conversion; content stays canonical.
    #[serde(default, skip_serializing_if = "unchanged_case")]
    pub text_case: TextCase,
    /// Additional local logical space between hard-newline paragraphs only.
    #[serde(default, skip_serializing_if = "no_spacing")]
    pub para_spacing: f32,
    /// Extra local advance after each native cluster, including the last.
    #[serde(default, skip_serializing_if = "no_spacing")]
    pub letter_spacing: f32,
    /// Extra local advance after each ASCII space cluster.
    #[serde(default, skip_serializing_if = "no_spacing")]
    pub word_spacing: f32,
    /// First-line indentation for each hard-newline paragraph.
    #[serde(default, skip_serializing_if = "no_spacing")]
    pub para_indent: f32,
    #[serde(default, skip_serializing_if = "horizontal")]
    pub writing_mode: TextWritingMode,
    /// Apply alignment to measured auto-size axes. Legacy Flow defaults to off.
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_size_align: bool,
    pub align: TextAlign,
    pub wrap: bool,
    /// Top, right, bottom, left padding in local logical units.
    #[serde(default, skip_serializing_if = "no_padding")]
    pub padding: [f32; 4],
    #[serde(default, skip_serializing_if = "is_top")]
    pub vertical_align: TextVerticalAlign,
    #[serde(default, skip_serializing_if = "is_clip")]
    pub overflow: TextOverflow,
    #[serde(default, skip_serializing_if = "undecorated")]
    pub decoration: TextDecoration,
}
impl Text {
    pub fn new(content: impl Into<String>, size: Size) -> Self {
        Self {
            content: content.into(),
            size,
            layout_origin: Point::ZERO,
            font_family: "sans-serif".into(),
            font_size: 14.0,
            font_weight: default_font_weight(),
            italic: false,
            line_height: 1.2,
            text_case: TextCase::None,
            para_spacing: 0.,
            letter_spacing: 0.,
            word_spacing: 0.,
            para_indent: 0.,
            writing_mode: TextWritingMode::X,
            auto_size_align: false,
            align: TextAlign::Start,
            wrap: true,
            padding: [0.; 4],
            vertical_align: TextVerticalAlign::Top,
            overflow: TextOverflow::Clip,
            decoration: TextDecoration::None,
        }
    }
    /// The shared wrapping, alignment and glyph clipping box. Call validate at
    /// mutation boundaries; public fields can temporarily form invalid geometry.
    pub fn content_bounds(&self) -> Bounds {
        let [top, right, bottom, left] = self.padding;
        Bounds::new(
            self.layout_origin + Point::new(left, top),
            Size::new(
                (self.size.width as f64 - left as f64 - right as f64) as f32,
                (self.size.height as f64 - top as f64 - bottom as f64) as f32,
            ),
        )
    }
    pub fn validate(&self) -> Result<(), String> {
        if (self.overflow == TextOverflow::Ellipsis
            || self.decoration != TextDecoration::None
            || self.text_case != TextCase::None
            || self.para_spacing != 0.
            || self.letter_spacing != 0.
            || self.word_spacing != 0.
            || self.para_indent != 0.
            || self.layout_origin != Point::ZERO)
            && self.content.len() > 65_536
        {
            return Err("styled text allows at most 64KiB UTF-8 content".into());
        }
        let converted_bytes: usize = match self.text_case {
            TextCase::None => 0,
            TextCase::Upper => self
                .content
                .chars()
                .flat_map(char::to_uppercase)
                .map(char::len_utf8)
                .sum(),
            TextCase::Lower => self
                .content
                .chars()
                .flat_map(char::to_lowercase)
                .map(char::len_utf8)
                .sum(),
        };
        if converted_bytes > 65_536 {
            return Err("case conversion output exceeds 64KiB UTF-8".into());
        }
        if !self.para_spacing.is_finite() || !(0. ..=1_000_000.).contains(&self.para_spacing) {
            return Err("paragraph spacing must be finite and within 0..=1000000".into());
        }
        for (name, value) in [
            ("letter spacing", self.letter_spacing),
            ("word spacing", self.word_spacing),
            ("paragraph indentation", self.para_indent),
        ] {
            if !value.is_finite() || !(0. ..=1_000_000.).contains(&value) {
                return Err(format!("{name} must be finite and within 0..=1000000"));
            }
        }
        if !self.layout_origin.is_finite()
            || self.layout_origin.x.abs() > 1_000_000.
            || self.layout_origin.y.abs() > 1_000_000.
        {
            return Err("text layout origin must be finite and within +/-1000000".into());
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
        Bounds::new(self.layout_origin, self.size)
    }
}

fn default_font_weight() -> u16 {
    400
}

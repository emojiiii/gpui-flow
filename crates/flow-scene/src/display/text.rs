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
/// Text with an explicit layout box. Font shaping/measurement is performed by
/// the host adapter, which can update size with actual measured dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Text {
    pub content: String,
    pub size: Size,
    pub font_family: String,
    pub font_size: f32,
    pub line_height: f32,
    pub align: TextAlign,
    pub wrap: bool,
}
impl Text {
    pub fn new(content: impl Into<String>, size: Size) -> Self {
        Self {
            content: content.into(),
            size,
            font_family: "sans-serif".into(),
            font_size: 14.0,
            line_height: 1.2,
            align: TextAlign::Start,
            wrap: true,
        }
    }
    pub fn bounds(&self) -> Bounds {
        Bounds::new(Point::ZERO, self.size)
    }
}

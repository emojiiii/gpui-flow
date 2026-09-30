use crate::{Bounds, Point, Size};
use serde::{Deserialize, Serialize};
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFit {
    #[default]
    Contain,
    Cover,
    Fill,
}
/// Image resource reference. This crate never fetches a URI or decodes bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Image {
    pub source: String,
    pub size: Size,
    pub natural_size: Option<Size>,
    pub fit: ImageFit,
}
impl Image {
    pub fn new(source: impl Into<String>, size: Size) -> Self {
        Self {
            source: source.into(),
            size,
            natural_size: None,
            fit: ImageFit::Contain,
        }
    }
    pub fn bounds(&self) -> Bounds {
        Bounds::new(Point::ZERO, self.size)
    }
    /// Destination for the intrinsic image. Cover may extend outside the box;
    /// render adapters must clip it to bounds.
    pub fn content_bounds(&self) -> Bounds {
        let Some(natural) = self.natural_size.filter(|s| s.is_positive()) else {
            return self.bounds();
        };
        if self.fit == ImageFit::Fill {
            return self.bounds();
        }
        let x = self.size.width / natural.width;
        let y = self.size.height / natural.height;
        let scale = if self.fit == ImageFit::Contain {
            x.min(y)
        } else {
            x.max(y)
        };
        let size = Size::new(natural.width * scale, natural.height * scale);
        Bounds::new(
            Point::new(
                (self.size.width - size.width) * 0.5,
                (self.size.height - size.height) * 0.5,
            ),
            size,
        )
    }
}

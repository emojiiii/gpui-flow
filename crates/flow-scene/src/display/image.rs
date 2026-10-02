use super::{CornerRadii, Rect};
use crate::{Bounds, Contour, Point, Size};
fn zero(value: &f32) -> bool {
    *value == 0.
}
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
    #[serde(default, skip_serializing_if = "zero")]
    pub corner_radius: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_radii: Option<CornerRadii>,
}
impl Image {
    pub fn new(source: impl Into<String>, size: Size) -> Self {
        Self {
            source: source.into(),
            size,
            natural_size: None,
            fit: ImageFit::Contain,
            corner_radius: 0.,
            corner_radii: None,
        }
    }
    pub fn rounded(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self.corner_radii = None;
        self
    }
    pub fn with_corners(mut self, radii: impl Into<CornerRadii>) -> Self {
        self.corner_radii = Some(radii.into());
        self
    }
    pub fn rect(&self) -> Rect {
        Rect {
            size: self.size,
            corner_radius: self.corner_radius,
            corner_radii: self.corner_radii,
        }
    }
    pub fn effective_corners(&self) -> CornerRadii {
        let ceiling = self.size.width.min(self.size.height) * 0.5;
        CornerRadii(
            self.corner_radii
                .unwrap_or(CornerRadii::uniform(self.corner_radius))
                .0
                .map(|r| r.max(0.).min(ceiling)),
        )
    }
    pub fn is_rounded(&self) -> bool {
        self.effective_corners().0.iter().any(|r| *r > 0.)
    }
    pub fn contains(&self, point: Point) -> bool {
        self.rect().contains(point)
    }
    pub fn flatten(&self, tolerance: f32) -> Contour {
        self.rect().flatten(tolerance)
    }
    pub fn validate(&self) -> Result<(), String> {
        self.rect().validate()?;
        if self.source.trim().is_empty() {
            return Err("image source cannot be empty".into());
        }
        if self.natural_size.is_some_and(|s| !s.is_positive()) {
            return Err("image natural size must be finite and positive".into());
        }
        if !self.content_bounds().is_valid() {
            return Err("image fit bounds overflow".into());
        }
        Ok(())
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

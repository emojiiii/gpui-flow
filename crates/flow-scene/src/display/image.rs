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
    /// Normalized source rectangle; None means the complete decoded frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_crop: Option<Bounds>,
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
            source_crop: None,
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
        if let Some(crop) = self.source_crop {
            Self::validate_source_crop(crop)?;
        }
        if self
            .mapping(self.natural_size.unwrap_or(Size::new(1., 1.)))
            .is_none()
        {
            return Err("image fit bounds overflow".into());
        }
        Ok(())
    }
    pub fn bounds(&self) -> Bounds {
        Bounds::new(Point::ZERO, self.size)
    }
    /// The complete source is represented exactly by the unit rectangle.
    pub fn normalized_crop(&self) -> Bounds {
        self.source_crop
            .unwrap_or(Bounds::new(Point::ZERO, Size::new(1., 1.)))
    }
    pub fn validate_source_crop(crop: Bounds) -> Result<(), String> {
        if !crop.is_valid()
            || !crop.size.is_positive()
            || crop.origin.x < 0.
            || crop.origin.y < 0.
            || crop.origin.x >= 1.
            || crop.origin.y >= 1.
            || crop.origin.x + crop.size.width > 1.
            || crop.origin.y + crop.size.height > 1.
        {
            return Err(
                "image source crop must be a positive normalized rectangle inside [0,1]".into(),
            );
        }
        Ok(())
    }
    /// Resolve normalized source coordinates against the actual decoded frame.
    /// Optional natural-size metadata is deliberately not consulted.
    pub fn source_bounds(&self, decoded: Size) -> Option<Bounds> {
        let crop = self.normalized_crop();
        Self::validate_source_crop(crop).ok()?;
        if !decoded.is_positive() {
            return None;
        }
        let source = Bounds::new(
            Point::new(
                crop.origin.x * decoded.width,
                crop.origin.y * decoded.height,
            ),
            Size::new(
                (crop.size.width * decoded.width)
                    .min(decoded.width - crop.origin.x * decoded.width),
                (crop.size.height * decoded.height)
                    .min(decoded.height - crop.origin.y * decoded.height),
            ),
        );
        (source.is_valid() && source.size.is_positive()).then_some(source)
    }
    /// Shared CPU-crop/alpha-hit mapping, without copying or decoding pixels.
    pub fn mapping(&self, decoded: Size) -> Option<ImageMapping> {
        if !self.size.is_positive() {
            return None;
        }
        let source = self.source_bounds(decoded)?;
        let (dw, dh) = (self.size.width as f64, self.size.height as f64);
        let (sw, sh) = (source.size.width as f64, source.size.height as f64);
        let (w, h) = if self.fit == ImageFit::Fill {
            (dw, dh)
        } else {
            let scale = if self.fit == ImageFit::Contain {
                (dw / sw).min(dh / sh)
            } else {
                (dw / sw).max(dh / sh)
            };
            (sw * scale, sh * scale)
        };
        let (x, y) = ((dw - w) * 0.5, (dh - h) * 0.5);
        let destination = Bounds::new(
            Point::new(x as f32, y as f32),
            Size::new(w as f32, h as f32),
        );
        let full_source = Bounds::new(
            Point::new(
                (x - source.origin.x as f64 * w / sw) as f32,
                (y - source.origin.y as f64 * h / sh) as f32,
            ),
            Size::new(
                (decoded.width as f64 * w / sw) as f32,
                (decoded.height as f64 * h / sh) as f32,
            ),
        );
        (destination.is_valid()
            && destination.size.is_positive()
            && full_source.is_valid()
            && full_source.size.is_positive())
        .then_some(ImageMapping {
            source,
            destination,
            full_source,
        })
    }
    /// Destination of the selected intrinsic source. Cover may extend outside
    /// the box. Runtime adapters must use `mapping` with actual decoded size.
    pub fn content_bounds(&self) -> Bounds {
        self.natural_size
            .and_then(|size| self.mapping(size))
            .map_or(self.bounds(), |m| m.destination)
    }
}
/// Geometry shared by CPU cropping and picking. `full_source` places the whole
/// texture, while `destination` restricts coverage to the selected source crop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageMapping {
    pub source: Bounds,
    pub destination: Bounds,
    pub full_source: Bounds,
}

//! Retained configuration for an imperative, runtime-owned pixel surface.
//! Pixel bytes and drawing state never enter Scene JSON or editor history.
use super::{CornerRadii, Path, Rect};
use crate::{Bounds, Contour, Point, Size};
use serde::{Deserialize, Serialize};
fn one() -> f32 {
    1.
}
fn yes() -> bool {
    true
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Canvas {
    pub size: Size,
    #[serde(default = "one")]
    pub pixel_ratio: f32,
    #[serde(default = "yes")]
    pub smooth: bool,
    #[serde(default)]
    pub safe_resize: bool,
    #[serde(default)]
    pub corner_radius: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_radii: Option<CornerRadii>,
}
impl Canvas {
    pub const MAX_DIMENSION: u32 = 4096;
    pub const MAX_PIXELS: u64 = 4_194_304;
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            size: Size::new(width, height),
            pixel_ratio: 1.,
            smooth: true,
            safe_resize: false,
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
    /// Ceil logical dimensions times DPR, with fixed admission before allocation.
    pub fn pixel_size(&self) -> Result<[u32; 2], String> {
        if !self.size.is_positive()
            || !self.pixel_ratio.is_finite()
            || self.pixel_ratio < 0.000001
            || self.size.width < 0.000001
            || self.size.height < 0.000001
            || self.pixel_ratio > 8.
            || self.size.width > 1_000_000.
            || self.size.height > 1_000_000.
        {
            return Err("invalid canvas size or pixel ratio".into());
        }
        let w = (self.size.width as f64 * self.pixel_ratio as f64).ceil();
        let h = (self.size.height as f64 * self.pixel_ratio as f64).ceil();
        if w > Self::MAX_DIMENSION as f64
            || h > Self::MAX_DIMENSION as f64
            || w * h > Self::MAX_PIXELS as f64
        {
            return Err("canvas pixel budget exceeded".into());
        }
        Ok([w as u32, h as u32])
    }
    pub fn validate(&self) -> Result<(), String> {
        self.pixel_size()?;
        self.rect().validate()
    }
    pub fn bounds(&self) -> Bounds {
        self.rect().bounds()
    }
    pub fn contains(&self, point: Point) -> bool {
        self.rect().contains(point)
    }
    pub fn flatten(&self, tolerance: f32) -> Contour {
        self.rect().flatten(tolerance)
    }
    pub fn to_path(&self) -> Result<Path, String> {
        self.rect().to_path()
    }
}
impl Default for Canvas {
    fn default() -> Self {
        Self::new(100., 100.)
    }
}

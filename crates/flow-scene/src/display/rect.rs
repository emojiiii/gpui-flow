use crate::{Bounds, Contour, Point, Size};
use serde::{Deserialize, Serialize};
/// A rectangle starting at local (0,0), with a uniform corner radius.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub size: Size,
    #[serde(default)]
    pub corner_radius: f32,
}
impl Rect {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            size: Size::new(width, height),
            corner_radius: 0.0,
        }
    }
    pub fn rounded(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }
    pub fn bounds(&self) -> Bounds {
        Bounds::new(Point::ZERO, self.size)
    }
    pub fn flatten(&self, tolerance: f32) -> Contour {
        crate::geometry::rectangle(self.size, self.corner_radius, tolerance)
    }
}

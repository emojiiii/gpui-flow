use crate::{Bounds, Contour, Point, Size};
use serde::{Deserialize, Serialize};
fn default_clip() -> bool {
    true
}
/// Artboard-like painted container. Descendants are clipped by default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub size: Size,
    #[serde(default)]
    pub corner_radius: f32,
    #[serde(default = "default_clip")]
    pub clip_children: bool,
}
impl Frame {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            size: Size::new(width, height),
            corner_radius: 0.0,
            clip_children: true,
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

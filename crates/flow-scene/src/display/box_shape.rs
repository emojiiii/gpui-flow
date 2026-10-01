use crate::{Bounds, ContainerPaintOrder, Contour, Point, Size};
use serde::{Deserialize, Serialize};
/// Painted layout container. Overflow is visible unless clip_children is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Box {
    pub size: Size,
    #[serde(default)]
    pub corner_radius: f32,
    #[serde(default)]
    pub clip_children: bool,
    #[serde(default)]
    pub paint_order: ContainerPaintOrder,
}
impl Box {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            size: Size::new(width, height),
            corner_radius: 0.0,
            clip_children: false,
            paint_order: ContainerPaintOrder::BeforeChildren,
        }
    }
    pub fn rounded(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }
    pub fn clipped(mut self, clip: bool) -> Self {
        self.clip_children = clip;
        self
    }
    pub fn with_paint_order(mut self, order: ContainerPaintOrder) -> Self {
        self.paint_order = order;
        self
    }
    pub fn bounds(&self) -> Bounds {
        Bounds::new(Point::ZERO, self.size)
    }
    pub fn flatten(&self, tolerance: f32) -> Contour {
        crate::geometry::rectangle(self.size, self.corner_radius, tolerance)
    }
}

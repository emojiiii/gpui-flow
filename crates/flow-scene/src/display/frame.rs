use super::{CornerRadii, Path, Rect};
use crate::{Bounds, ContainerPaintOrder, Contour, Point, Size};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_radii: Option<CornerRadii>,
    #[serde(default = "default_clip")]
    pub clip_children: bool,
    #[serde(default)]
    pub paint_order: ContainerPaintOrder,
}
impl Frame {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            size: Size::new(width, height),
            corner_radius: 0.0,
            corner_radii: None,
            clip_children: true,
            paint_order: ContainerPaintOrder::BeforeChildren,
        }
    }
    pub fn rounded(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self.corner_radii = None;
        self
    }
    pub fn with_paint_order(mut self, order: ContainerPaintOrder) -> Self {
        self.paint_order = order;
        self
    }
    pub fn with_corners(mut self, radii: impl Into<CornerRadii>) -> Self {
        self.corner_radii = Some(radii.into());
        self
    }
    fn rect(&self) -> Rect {
        Rect {
            size: self.size,
            corner_radius: self.corner_radius,
            corner_radii: self.corner_radii,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        self.rect().validate()
    }
    /// Own painted box geometry only; descendants are separate scene elements.
    pub fn to_path(&self) -> Result<Path, String> {
        self.rect().to_path()
    }
    pub fn contains(&self, point: Point) -> bool {
        self.rect().contains(point)
    }
    pub fn bounds(&self) -> Bounds {
        Bounds::new(Point::ZERO, self.size)
    }
    pub fn flatten(&self, tolerance: f32) -> Contour {
        self.rect().flatten(tolerance)
    }
}

use crate::{Bounds, Contour, Point, Size};
use serde::{Deserialize, Serialize};
/// Ellipse inscribed in a local (0,0,width,height) rectangle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ellipse {
    pub size: Size,
}
impl Ellipse {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            size: Size::new(width, height),
        }
    }
    pub fn circle(radius: f32) -> Self {
        Self::new(radius * 2.0, radius * 2.0)
    }
    pub fn bounds(&self) -> Bounds {
        Bounds::new(Point::ZERO, self.size)
    }
    pub fn flatten(&self, tolerance: f32) -> Contour {
        let count =
            crate::geometry::arc_segments(self.size.width.max(self.size.height) * 0.5, tolerance);
        let points = (0..count)
            .map(|i| {
                let angle = std::f32::consts::TAU * i as f32 / count as f32;
                Point::new(
                    (1.0 + angle.cos()) * self.size.width * 0.5,
                    (1.0 + angle.sin()) * self.size.height * 0.5,
                )
            })
            .collect();
        Contour {
            points,
            closed: true,
        }
    }
    pub fn contains(&self, p: Point) -> bool {
        let x = (p.x - self.size.width * 0.5) / (self.size.width * 0.5);
        let y = (p.y - self.size.height * 0.5) / (self.size.height * 0.5);
        x * x + y * y <= 1.0
    }
}

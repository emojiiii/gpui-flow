use crate::{Bounds, Contour, Point};
use serde::{Deserialize, Serialize};
/// Alternating inner/outer vertices; start_angle is in degrees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Star {
    pub center: Point,
    pub outer_radius: f32,
    pub inner_radius: f32,
    pub points: usize,
    pub start_angle: f32,
}
impl Star {
    pub fn new(points: usize, outer_radius: f32, inner_radius: f32) -> Self {
        Self {
            center: Point::new(outer_radius, outer_radius),
            outer_radius,
            inner_radius,
            points,
            start_angle: -90.0,
        }
    }
    pub fn flatten(&self) -> Contour {
        let count = self.points.clamp(2, 4096) * 2;
        let points = (0..count)
            .map(|i| {
                let angle =
                    self.start_angle.to_radians() + std::f32::consts::TAU * i as f32 / count as f32;
                let r = if i % 2 == 0 {
                    self.outer_radius
                } else {
                    self.inner_radius
                };
                self.center + Point::new(angle.cos() * r, angle.sin() * r)
            })
            .collect();
        Contour {
            points,
            closed: true,
        }
    }
    pub fn bounds(&self) -> Option<Bounds> {
        crate::geometry::point_bounds(self.flatten().points)
    }
}

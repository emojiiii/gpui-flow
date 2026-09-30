use crate::{Bounds, Contour, Point};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Polygon {
    pub points: Vec<Point>,
    pub closed: bool,
}
impl Polygon {
    pub fn new(points: Vec<Point>) -> Self {
        Self {
            points,
            closed: true,
        }
    }
    pub fn polyline(points: Vec<Point>) -> Self {
        Self {
            points,
            closed: false,
        }
    }
    pub fn bounds(&self) -> Option<Bounds> {
        crate::geometry::point_bounds(self.points.iter().copied())
    }
    pub fn flatten(&self) -> Contour {
        Contour {
            points: self.points.clone(),
            closed: self.closed,
        }
    }
}

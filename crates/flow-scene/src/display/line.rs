use crate::{Bounds, Contour, Point};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub start: Point,
    pub end: Point,
}
impl Line {
    pub fn new(start: Point, end: Point) -> Self {
        Self { start, end }
    }
    pub fn bounds(&self) -> Bounds {
        Bounds::from_points(self.start, self.end)
    }
    pub fn flatten(&self) -> Contour {
        Contour {
            points: vec![self.start, self.end],
            closed: false,
        }
    }
}

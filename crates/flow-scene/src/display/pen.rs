use super::{Path, PathCommand};
use crate::{Bounds, Contour, Point};
use serde::{Deserialize, Serialize};
/// Serializable fluent path builder. Pen is a first-class primitive so an
/// editor can distinguish authoring strokes from imported path geometry.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pen {
    pub path: Path,
}
impl Pen {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn move_to(mut self, to: Point) -> Self {
        self.path.commands.push(PathCommand::MoveTo { to });
        self
    }
    pub fn line_to(mut self, to: Point) -> Self {
        self.path.commands.push(PathCommand::LineTo { to });
        self
    }
    pub fn quadratic_to(mut self, control: Point, to: Point) -> Self {
        self.path
            .commands
            .push(PathCommand::QuadraticTo { control, to });
        self
    }
    pub fn cubic_to(mut self, control1: Point, control2: Point, to: Point) -> Self {
        self.path.commands.push(PathCommand::CubicTo {
            control1,
            control2,
            to,
        });
        self
    }
    pub fn close(mut self) -> Self {
        self.path.commands.push(PathCommand::Close);
        self
    }
    pub fn into_path(self) -> Path {
        self.path
    }
    pub fn bounds(&self) -> Option<Bounds> {
        self.path.bounds()
    }
    pub fn flatten(&self, tolerance: f32) -> Vec<Contour> {
        self.path.flatten(tolerance)
    }
}

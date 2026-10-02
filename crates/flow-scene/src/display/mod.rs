//! Local geometry primitives. Each primitive is implemented independently;
//! [`Shape`] provides dispatch for retained scene storage and serialization.
mod box_shape;
mod custom;
mod ellipse;
mod frame;
mod group;
mod image;
mod line;
mod path;
mod path_measure;
mod path_svg;
mod pen;
mod polygon;
mod rect;
mod star;
mod text;
use crate::{Bounds, Contour, Point, Style};
pub use box_shape::Box;
pub use custom::Custom;
pub use ellipse::Ellipse;
pub use frame::Frame;
pub use group::Group;
pub use image::{Image, ImageFit};
pub use line::Line;
pub use path::{Path, PathCommand};
pub use path_measure::{PathMeasure, PathSample};
pub use pen::Pen;
pub use polygon::Polygon;
pub use rect::{CornerRadii, Rect};
use serde::{Deserialize, Serialize};
pub use star::Star;
pub use text::{Text, TextAlign};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "geometry", rename_all = "snake_case")]
pub enum Shape {
    Rect(Rect),
    Ellipse(Ellipse),
    Line(Line),
    Polygon(Polygon),
    Star(Star),
    Path(Path),
    Pen(Pen),
    Text(Text),
    Image(Image),
    Group(Group),
    Box(Box),
    Frame(Frame),
    Custom(Custom),
}
macro_rules! conversions {($($name:ident),*)=>{$(impl From<$name> for Shape{fn from(value:$name)->Self{Self::$name(value)}})*};}
conversions!(
    Rect, Ellipse, Line, Polygon, Star, Path, Pen, Text, Image, Group, Box, Frame, Custom
);
impl Shape {
    /// The renderer's own-paint entry/exit phases; structural object order and
    /// picking do not change when a container moves its stroke after children.
    pub fn paint_phases(&self) -> (crate::PaintPhase, Option<crate::PaintPhase>) {
        match self {
            Self::Box(s) => s.paint_order.phases(),
            Self::Frame(s) => s.paint_order.phases(),
            _ => (crate::PaintPhase::All, None),
        }
    }
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Rect(_) => "rect",
            Self::Ellipse(_) => "ellipse",
            Self::Line(_) => "line",
            Self::Polygon(_) => "polygon",
            Self::Star(_) => "star",
            Self::Path(_) => "path",
            Self::Pen(_) => "pen",
            Self::Text(_) => "text",
            Self::Image(_) => "image",
            Self::Group(_) => "group",
            Self::Box(_) => "box",
            Self::Frame(_) => "frame",
            Self::Custom(_) => "custom",
        }
    }
    pub fn allows_children(&self) -> bool {
        matches!(self, Self::Group(_) | Self::Box(_) | Self::Frame(_))
    }
    pub fn local_bounds(&self) -> Option<Bounds> {
        match self {
            Self::Rect(s) => Some(s.bounds()),
            Self::Ellipse(s) => s.geometry_bounds(),
            Self::Line(s) => Some(s.bounds()),
            Self::Polygon(s) => s.bounds(),
            Self::Star(s) => s.bounds(),
            Self::Path(s) => s.bounds(),
            Self::Pen(s) => s.bounds(),
            Self::Text(s) => Some(s.bounds()),
            Self::Image(s) => Some(s.bounds()),
            Self::Group(_) => None,
            Self::Box(s) => Some(s.bounds()),
            Self::Frame(s) => Some(s.bounds()),
            Self::Custom(s) => Some(s.bounds),
        }
    }
    /// Flatten in local units. Text/image return their layout rectangles rather
    /// than glyph outlines or decoded pixels. Group has no intrinsic geometry.
    pub fn flatten(&self, tolerance: f32) -> Vec<Contour> {
        match self {
            Self::Rect(s) => vec![s.flatten(tolerance)],
            Self::Ellipse(s) => s.flatten(tolerance),
            Self::Line(s) => vec![s.flatten()],
            Self::Polygon(s) => vec![s.flatten()],
            Self::Star(s) => vec![s.flatten()],
            Self::Path(s) => s.flatten(tolerance),
            Self::Pen(s) => s.flatten(tolerance),
            Self::Text(s) => vec![crate::geometry::rectangle(s.size, 0.0, tolerance)],
            Self::Image(s) => vec![crate::geometry::rectangle(s.size, 0.0, tolerance)],
            Self::Group(_) => vec![],
            Self::Box(s) => vec![s.flatten(tolerance)],
            Self::Frame(s) => vec![s.flatten(tolerance)],
            Self::Custom(s) => s.contours.clone(),
        }
    }
    pub fn child_clip(&self) -> Option<(Bounds, f32)> {
        match self {
            Self::Box(s) if s.clip_children => Some((s.bounds(), s.corner_radius)),
            Self::Frame(s) if s.clip_children => Some((s.bounds(), s.corner_radius)),
            _ => None,
        }
    }
    pub fn contains_geometry(&self, point: Point, rule: crate::FillRule) -> bool {
        match self {
            Self::Rect(s) => s.contains(point),
            Self::Ellipse(s) => s.contains_with_rule(point, rule),
            Self::Box(s) => crate::geometry::rounded_contains(s.bounds(), s.corner_radius, point),
            Self::Frame(s) => crate::geometry::rounded_contains(s.bounds(), s.corner_radius, point),
            Self::Text(s) => s.bounds().contains(point),
            Self::Image(s) => s.bounds().contains(point),
            Self::Line(_) | Self::Group(_) => false,
            _ => crate::geometry::fill_hit(&self.flatten(0.1), point, rule),
        }
    }
    /// Painted picking uses fill winding and the same advanced-stroke mesh as
    /// rendering. Legacy solid round strokes retain distance picking. Text is
    /// picked by its layout box, images by content bounds (no alpha sampling).
    pub fn hit_test(&self, point: Point, style: &Style, tolerance: f32) -> bool {
        if !point.is_finite() || !tolerance.is_finite() || tolerance < 0.0 {
            return false;
        }
        if let Self::Image(image) = self {
            return image.bounds().contains(point) && image.content_bounds().contains(point);
        }
        let fill = style.fills().any(|paint| paint.is_visible());
        if fill && self.contains_geometry(point, style.fill_rule) {
            return true;
        }
        let extent = style
            .strokes()
            .filter(|s| s.paint.is_visible() && !s.uses_advanced_geometry())
            .map(|s| s.width * 0.5)
            .fold(0.0, f32::max);
        if extent > 0.0
            && crate::geometry::stroke_hit(&self.flatten(0.1), point, extent + tolerance)
        {
            return true;
        }
        style
            .strokes()
            .filter(|s| s.width > 0.0 && s.paint.is_visible() && s.uses_advanced_geometry())
            .any(|stroke| {
                self.stroke_mesh_with_rule(stroke, style.fill_rule)
                    .is_ok_and(|mesh| mesh.hit_test(point, tolerance))
            })
    }
    pub fn validate(&self) -> Result<(), String> {
        fn size(s: crate::Size) -> Result<(), String> {
            if s.is_positive() {
                Ok(())
            } else {
                Err("size must be finite and positive".into())
            }
        }
        fn radius(r: f32) -> Result<(), String> {
            if r.is_finite() && r >= 0.0 {
                Ok(())
            } else {
                Err("corner radius must be finite and non-negative".into())
            }
        }
        match self {
            Self::Rect(s) => s.validate()?,
            Self::Ellipse(s) => s.validate()?,
            Self::Line(s) => {
                if !s.start.is_finite() || !s.end.is_finite() {
                    return Err("line coordinates must be finite".into());
                }
            }
            Self::Polygon(s) => {
                if s.points.len() < (if s.closed { 3 } else { 2 })
                    || s.points.iter().any(|p| !p.is_finite())
                {
                    return Err("polygon requires finite vertices (3 closed / 2 open)".into());
                }
            }
            Self::Star(s) => {
                if !s.center.is_finite()
                    || !s.outer_radius.is_finite()
                    || !s.inner_radius.is_finite()
                    || s.outer_radius <= 0.0
                    || s.inner_radius < 0.0
                    || s.inner_radius > s.outer_radius
                    || !(2..=4096).contains(&s.points)
                    || !s.start_angle.is_finite()
                {
                    return Err("invalid star radii, count, center, or angle".into());
                }
            }
            Self::Path(s) => s.validate()?,
            Self::Pen(s) => s.path.validate()?,
            Self::Custom(s) => s.validate()?,
            Self::Text(s) => {
                size(s.size)?;
                if !s.font_size.is_finite()
                    || s.font_size <= 0.0
                    || !(1..=1000).contains(&s.font_weight)
                    || !s.line_height.is_finite()
                    || s.line_height <= 0.0
                    || s.font_family.trim().is_empty()
                {
                    return Err("invalid text metrics or font family".into());
                }
            }
            Self::Image(s) => {
                size(s.size)?;
                if s.source.trim().is_empty() {
                    return Err("image source cannot be empty".into());
                }
                if let Some(n) = s.natural_size {
                    size(n)?;
                }
                if !s.content_bounds().is_valid() {
                    return Err("image fit bounds overflow".into());
                }
            }
            Self::Group(_) => {}
            Self::Box(s) => {
                size(s.size)?;
                radius(s.corner_radius)?;
            }
            Self::Frame(s) => {
                size(s.size)?;
                radius(s.corner_radius)?;
            }
        }
        if self.local_bounds().is_some_and(|b| !b.is_valid()) {
            return Err("shape bounds overflow".into());
        }
        Ok(())
    }
}

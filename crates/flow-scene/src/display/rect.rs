use super::{Path, PathCommand};
use crate::{Bounds, Contour, FlattenOptions, Point, Size};
use kurbo::{PathEl, RoundedRect, RoundedRectRadii, Shape as _};
use serde::{Deserialize, Serialize};

/// Clockwise radii: top-left, top-right, bottom-right, bottom-left.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CornerRadii(pub [f32; 4]);
impl CornerRadii {
    pub const fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self {
        Self([top_left, top_right, bottom_right, bottom_left])
    }
    pub const fn uniform(radius: f32) -> Self {
        Self([radius; 4])
    }
    pub fn validate(self) -> Result<(), String> {
        if self.0.iter().any(|r| !r.is_finite() || *r < 0.) {
            Err("corner radii must be finite and non-negative".into())
        } else {
            Ok(())
        }
    }
}
impl From<[f32; 4]> for CornerRadii {
    fn from(value: [f32; 4]) -> Self {
        Self(value)
    }
}
/// A rectangle at local (0,0), with scalar or optional independent corner radii.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub size: Size,
    #[serde(default)]
    pub corner_radius: f32,
    /// Overrides the scalar when present. Old JSON/constructors retain None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_radii: Option<CornerRadii>,
}
impl Rect {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            size: Size::new(width, height),
            corner_radius: 0.,
            corner_radii: None,
        }
    }
    pub fn rounded(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self.corner_radii = None;
        self
    }
    pub fn with_corners(mut self, radii: impl Into<CornerRadii>) -> Self {
        self.corner_radii = Some(radii.into());
        self
    }
    pub fn bounds(&self) -> Bounds {
        Bounds::new(Point::ZERO, self.size)
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.size.is_positive() {
            return Err("rectangle size must be finite and positive".into());
        }
        CornerRadii::uniform(self.corner_radius).validate()?;
        if let Some(radii) = self.corner_radii {
            radii.validate()?;
            if self.size.width > 1_000_000. || self.size.height > 1_000_000. {
                return Err("per-corner rectangle size exceeds 1000000 local units".into());
            }
        }
        Ok(())
    }
    fn rounded_geometry(&self) -> Result<RoundedRect, String> {
        self.validate()?;
        let [tl, tr, br, bl] = self
            .corner_radii
            .unwrap_or(CornerRadii::uniform(self.corner_radius))
            .0
            .map(f64::from);
        // Kurbo clamps each corner to half the shortest side, matching the
        // pinned Leafer RectHelper fallback's fourNumber radius ceiling.
        Ok(RoundedRect::new(
            0.,
            0.,
            self.size.width as f64,
            self.size.height as f64,
            RoundedRectRadii::new(tl, tr, br, bl),
        ))
    }
    /// Convert to canonical curves with existing Kurbo geometry, not handwritten
    /// corner arcs. Nominal bounds remain the complete rectangle sizing box.
    pub fn to_path(&self) -> Result<Path, String> {
        let rounded = self.rounded_geometry()?;
        if self.size.width > 1_000_000. || self.size.height > 1_000_000. {
            return Err("rectangle path conversion size exceeds 1000000 local units".into());
        }
        let mut commands = Vec::new();
        let point = |p: kurbo::Point| Point::new(p.x as f32, p.y as f32);
        for element in rounded.path_elements(0.01) {
            if commands.len() >= 64 {
                return Err("rectangle curve command budget exceeded".into());
            }
            commands.push(match element {
                PathEl::MoveTo(to) => PathCommand::MoveTo { to: point(to) },
                PathEl::LineTo(to) => PathCommand::LineTo { to: point(to) },
                PathEl::QuadTo(control, to) => PathCommand::QuadraticTo {
                    control: point(control),
                    to: point(to),
                },
                PathEl::CurveTo(control1, control2, to) => PathCommand::CubicTo {
                    control1: point(control1),
                    control2: point(control2),
                    to: point(to),
                },
                PathEl::ClosePath => PathCommand::Close,
            });
        }
        let path = Path::new(commands);
        path.validate()?;
        Ok(path)
    }
    pub fn flatten(&self, tolerance: f32) -> Contour {
        if self.corner_radii.is_none() {
            return crate::geometry::rectangle(self.size, self.corner_radius, tolerance);
        }
        self.to_path().map_or_else(
            |_| Contour::default(),
            |path| {
                path.flatten_with(FlattenOptions {
                    tolerance,
                    max_depth: 7,
                })
                .pop()
                .unwrap_or_default()
            },
        )
    }
    pub fn contains(&self, point: Point) -> bool {
        if !point.is_finite() {
            return false;
        }
        if self.corner_radii.is_none() {
            return crate::geometry::rounded_contains(self.bounds(), self.corner_radius, point);
        }
        self.rounded_geometry().is_ok_and(|rounded| {
            rounded.winding(kurbo::Point::new(point.x as f64, point.y as f64)) != 0
        })
    }
}

use crate::Point;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}
impl Color {
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    pub const fn hex(rgb: u32) -> Self {
        Self::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub offset: f32,
    pub color: Color,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Paint {
    Solid {
        color: Color,
    },
    LinearGradient {
        start: Point,
        end: Point,
        stops: Vec<GradientStop>,
    },
    RadialGradient {
        center: Point,
        radius: f32,
        stops: Vec<GradientStop>,
    },
}
impl From<Color> for Paint {
    fn from(color: Color) -> Self {
        Self::Solid { color }
    }
}
impl Paint {
    pub fn solid(color: Color) -> Self {
        Self::Solid { color }
    }
    pub fn is_visible(&self) -> bool {
        match self {
            Self::Solid { color } => color.a > 0,
            Self::LinearGradient { stops, .. } | Self::RadialGradient { stops, .. } => {
                stops.iter().any(|s| s.color.a > 0)
            }
        }
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        let stops = match self {
            Self::Solid { .. } => return Ok(()),
            Self::LinearGradient { start, end, stops } => {
                if !start.is_finite() || !end.is_finite() || start == end {
                    return Err("gradient endpoints must be finite and distinct".into());
                }
                stops
            }
            Self::RadialGradient {
                center,
                radius,
                stops,
            } => {
                if !center.is_finite() || !radius.is_finite() || *radius <= 0.0 {
                    return Err(
                        "radial gradient requires a finite center and positive radius".into(),
                    );
                }
                stops
            }
        };
        if stops.len() < 2
            || stops
                .iter()
                .any(|s| !s.offset.is_finite() || s.offset < 0.0 || s.offset > 1.0)
            || stops.windows(2).any(|s| s[0].offset > s[1].offset)
        {
            return Err("gradient requires at least two ordered stops in [0,1]".into());
        }
        Ok(())
    }
}
/// Strokes use round caps and joins in geometric picking. Render adapters should
/// use the same convention; dash patterns are deliberately not part of this model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub paint: Paint,
    pub width: f32,
}
impl Stroke {
    pub fn new(color: Color, width: f32) -> Self {
        Self {
            paint: color.into(),
            width,
        }
    }
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillRule {
    #[default]
    NonZero,
    EvenOdd,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    /// Legacy/base fill, painted before `fill_layers`.
    pub fill: Option<Paint>,
    /// Legacy/base stroke, painted before `stroke_layers` and after all fills.
    pub stroke: Option<Stroke>,
    /// Additional vector fills in back-to-front order. Empty in older documents.
    pub fill_layers: Vec<Paint>,
    /// Additional centered vector strokes in back-to-front order.
    pub stroke_layers: Vec<Stroke>,
    pub opacity: f32,
    pub fill_rule: FillRule,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            fill: Some(Color::BLACK.into()),
            stroke: None,
            fill_layers: Vec::new(),
            stroke_layers: Vec::new(),
            opacity: 1.0,
            fill_rule: FillRule::NonZero,
        }
    }
}
/// Maximum combined base and additional fill/stroke paints per object.
/// This bounds tessellation and texture work without truncating user input.
pub const MAX_PAINT_LAYERS: usize = 32;

impl Style {
    /// Base fill followed by additional fills, in painter order.
    pub fn fills(&self) -> impl Iterator<Item = &Paint> {
        self.fill.iter().chain(self.fill_layers.iter())
    }
    /// Base stroke followed by additional strokes, in painter order.
    pub fn strokes(&self) -> impl Iterator<Item = &Stroke> {
        self.stroke.iter().chain(self.stroke_layers.iter())
    }
    /// Conservative centered-stroke extent. Fully transparent paints and zero
    /// widths neither render nor enlarge painted bounds or stroke picking.
    pub fn stroke_extent(&self) -> f32 {
        self.strokes()
            .filter(|stroke| stroke.paint.is_visible())
            .map(|stroke| stroke.width * 0.5)
            .fold(0.0, f32::max)
    }
    pub fn has_layers(&self) -> bool {
        !self.fill_layers.is_empty() || !self.stroke_layers.is_empty()
    }
    pub fn fill(color: Color) -> Self {
        Self {
            fill: Some(color.into()),
            ..Self::default()
        }
    }
    pub fn stroke(color: Color, width: f32) -> Self {
        Self {
            fill: None,
            stroke: Some(Stroke::new(color, width)),
            ..Self::default()
        }
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !self.opacity.is_finite() || self.opacity < 0.0 || self.opacity > 1.0 {
            return Err("opacity must be in [0,1]".into());
        }
        if self.fills().count() + self.strokes().count() > MAX_PAINT_LAYERS {
            return Err(format!(
                "appearance exceeds {MAX_PAINT_LAYERS} paint layers"
            ));
        }
        for fill in self.fills() {
            fill.validate()?;
        }
        for stroke in self.strokes() {
            if !stroke.width.is_finite() || stroke.width < 0.0 {
                return Err("stroke width must be finite and non-negative".into());
            }
            stroke.paint.validate()?;
        }
        Ok(())
    }
}

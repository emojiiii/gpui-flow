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
/// Shape of both ends of an open stroke or dash.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrokeCap {
    Butt,
    Square,
    #[default]
    Round,
}
/// Connection between consecutive segments of a stroke.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrokeJoin {
    Bevel,
    Miter,
    #[default]
    Round,
}
fn default_miter_limit() -> f32 {
    4.0
}
/// Maximum dash entries before odd patterns are repeated to form an even period.
pub const MAX_STROKE_DASH_ENTRIES: usize = 256;
/// Centered local-space stroke. Missing style fields retain the original solid,
/// round-cap, round-join appearance. Inside/outside and scale-fixed strokes are
/// not supported. Advanced geometry has the budgets documented on `StrokeMesh`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub paint: Paint,
    pub width: f32,
    #[serde(default)]
    pub cap: StrokeCap,
    #[serde(default)]
    pub join: StrokeJoin,
    #[serde(default = "default_miter_limit")]
    pub miter_limit: f32,
    /// Alternating positive on/off lengths, in local units. Empty means solid.
    /// Odd-length patterns repeat twice; zero and negative entries are rejected.
    #[serde(default)]
    pub dash_pattern: Vec<f32>,
    /// Signed distance into the pattern at each subpath start, in local units.
    #[serde(default)]
    pub dash_offset: f32,
}
impl Default for Stroke {
    fn default() -> Self {
        Self::new(Color::BLACK, 1.0)
    }
}
impl Stroke {
    pub fn new(color: Color, width: f32) -> Self {
        Self {
            paint: color.into(),
            width,
            cap: StrokeCap::Round,
            join: StrokeJoin::Round,
            miter_limit: default_miter_limit(),
            dash_pattern: Vec::new(),
            dash_offset: 0.0,
        }
    }
    pub fn with_cap(mut self, cap: StrokeCap) -> Self {
        self.cap = cap;
        self
    }
    pub fn with_join(mut self, join: StrokeJoin) -> Self {
        self.join = join;
        self
    }
    pub fn with_miter_limit(mut self, limit: f32) -> Self {
        self.miter_limit = limit;
        self
    }
    pub fn with_dashes(mut self, pattern: Vec<f32>, offset: f32) -> Self {
        self.dash_pattern = pattern;
        self.dash_offset = offset;
        self
    }
    /// Whether rendering and picking require the shared advanced-stroke mesh.
    /// An unused miter limit or dash offset does not change solid round geometry.
    pub fn uses_advanced_geometry(&self) -> bool {
        self.cap != StrokeCap::Round
            || self.join != StrokeJoin::Round
            || !self.dash_pattern.is_empty()
    }
    pub(crate) fn extent(&self) -> f32 {
        let mut factor = 0.5f64;
        if self.cap == StrokeCap::Square {
            factor = std::f64::consts::SQRT_2 * 0.5;
        }
        if self.join == StrokeJoin::Miter {
            // Lyon 1.0.22 limits normal length to 2 * miter_limit. With
            // half-width extrusion, width * limit is a conservative reach.
            factor = factor.max(self.miter_limit as f64);
        }
        (self.width as f64 * factor) as f32
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !self.width.is_finite() || self.width < 0.0 {
            return Err("stroke width must be finite and non-negative".into());
        }
        if !self.miter_limit.is_finite() || !(1.0..=10_000.0).contains(&self.miter_limit) {
            return Err("stroke miter limit must be in [1,10000]".into());
        }
        if !self.dash_offset.is_finite() {
            return Err("stroke dash offset must be finite".into());
        }
        if self.dash_pattern.len() > MAX_STROKE_DASH_ENTRIES
            || self
                .dash_pattern
                .iter()
                .any(|d| !d.is_finite() || *d <= 0.0)
        {
            return Err(format!(
                "stroke dash pattern requires at most {MAX_STROKE_DASH_ENTRIES} finite positive entries"
            ));
        }
        if !self.extent().is_finite() {
            return Err("stroke extent overflow".into());
        }
        self.paint.validate()
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
            .map(Stroke::extent)
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
            stroke.validate()?;
        }
        Ok(())
    }
}

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
/// Side of a closed vector boundary on which the stroke is painted.
/// Center is the legacy default, including when old JSON omits this field.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrokeAlignment {
    #[default]
    Center,
    Inside,
    Outside,
}
fn default_miter_limit() -> f32 {
    4.0
}
/// Maximum dash entries before odd patterns are repeated to form an even period.
pub const MAX_STROKE_DASH_ENTRIES: usize = 256;
/// Local-space stroke. Missing fields retain the original centered, solid,
/// round-cap, round-join appearance. Inside/outside require closed vector
/// contours. Advanced geometry has the budgets documented on `StrokeMesh`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub paint: Paint,
    pub width: f32,
    #[serde(default)]
    pub alignment: StrokeAlignment,
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
            alignment: StrokeAlignment::Center,
            cap: StrokeCap::Round,
            join: StrokeJoin::Round,
            miter_limit: default_miter_limit(),
            dash_pattern: Vec::new(),
            dash_offset: 0.0,
        }
    }
    pub fn with_alignment(mut self, alignment: StrokeAlignment) -> Self {
        self.alignment = alignment;
        self
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
        self.alignment != StrokeAlignment::Center
            || self.cap != StrokeCap::Round
            || self.join != StrokeJoin::Round
            || !self.dash_pattern.is_empty()
    }
    pub(crate) fn extent(&self) -> f32 {
        if self.alignment == StrokeAlignment::Inside {
            return 0.0;
        }
        let mut factor = 0.5f64;
        if self.cap == StrokeCap::Square {
            factor = std::f64::consts::SQRT_2 * 0.5;
        }
        if self.join == StrokeJoin::Miter {
            // Lyon 1.0.22 limits normal length to 2 * miter_limit. With
            // half-width extrusion, width * limit is a conservative reach.
            factor = factor.max(self.miter_limit as f64);
        }
        if self.alignment == StrokeAlignment::Outside {
            factor *= 2.0;
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
/// Group isolation and compositing inside a supported renderer domain.
/// `PassThrough` preserves the parent backdrop; `Normal` isolates children.
/// Document spellings use snake_case, not a Leafer JSON import schema.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    PassThrough,
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}
impl BlendMode {
    /// Artistic modes require actual destination pixels. A standalone domain
    /// cannot substitute transparent black for an unknown host backdrop.
    pub fn requires_backdrop(self) -> bool {
        !matches!(self, Self::PassThrough | Self::Normal)
    }
}
/// One alpha-derived shadow layer. Local-space `blur` maps to 2σ in
/// the bounded CPU renderer; this is not a browser-pixel-equivalence promise.
/// Its owning style array determines inner/outer placement. Spread, box,
/// scale-fixed and per-shadow blend fields are unsupported.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shadow {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub color: Color,
}
impl Shadow {
    pub const fn new(x: f32, y: f32, blur: f32, color: Color) -> Self {
        Self { x, y, blur, color }
    }
    pub fn is_visible(self) -> bool {
        self.color.a != 0
    }
    /// Conservative visual extent, excluding the renderer's device-pixel AA
    /// guard. Geometric paint/hit/layout/selection bounds remain unchanged.
    pub fn expand_bounds(self, bounds: crate::Bounds, transform: crate::Affine) -> crate::Bounds {
        if !self.is_visible() {
            return bounds;
        }
        let offset = transform.transform_vector(crate::Point::new(self.x, self.y));
        let halo = self.blur as f64 * 2. * transform.max_scale() as f64;
        if !halo.is_finite() || halo > f32::MAX as f64 {
            return crate::Bounds::new(crate::Point::new(f32::NAN, f32::NAN), bounds.size);
        }
        let shadow = crate::Bounds::new(bounds.origin + offset, bounds.size).expanded(halo as f32);
        bounds.union(shadow)
    }
    fn validate(self) -> Result<(), String> {
        if !self.x.is_finite()
            || !self.y.is_finite()
            || !self.blur.is_finite()
            || self.x.abs() > 4096.
            || self.y.abs() > 4096.
            || !(0.0..=256.0).contains(&self.blur)
        {
            return Err("shadow requires finite offsets within ±4096 and blur in [0,256]".into());
        }
        Ok(())
    }
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
    pub blend_mode: BlendMode,
    /// Legacy/base outer shadow, painted before `shadow_layers`.
    pub shadow: Option<Shadow>,
    /// Additional outer shadows in back-to-front order, sharing the same source
    /// alpha. Combined with `shadow`, at most MAX_SHADOW_LAYERS are accepted.
    pub shadow_layers: Vec<Shadow>,
    /// Inner shadows in back-to-front order on bounded leaf/container producers.
    /// Every layer uses the same original composed alpha before own effects.
    /// Effects precede own strokes and respect the container paint order.
    pub inner_shadow_layers: Vec<Shadow>,
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
            blend_mode: BlendMode::PassThrough,
            shadow: None,
            shadow_layers: Vec::new(),
            inner_shadow_layers: Vec::new(),
        }
    }
}
/// Maximum combined base and additional fill/stroke paints per object.
/// This bounds tessellation and texture work without truncating user input.
pub const MAX_PAINT_LAYERS: usize = 32;
/// Maximum combined base and additional outer shadows, including transparent layers.
pub const MAX_SHADOW_LAYERS: usize = 8;
/// Independent inner-shadow ceiling, including transparent entries. Actual
/// combined inner/outer work must also fit the compositor's cumulative budget.
pub const MAX_INNER_SHADOW_LAYERS: usize = 8;

impl Style {
    /// Base shadow followed by additional shadows, in painter order. Every layer
    /// samples the same source; earlier shadows never feed later shadows.
    pub fn shadows(&self) -> impl Iterator<Item = &Shadow> {
        self.shadow.iter().chain(self.shadow_layers.iter())
    }
    pub fn has_shadows(&self) -> bool {
        self.shadow.is_some() || !self.shadow_layers.is_empty()
    }
    pub fn has_visible_shadows(&self) -> bool {
        self.shadows().any(|s| s.is_visible())
    }
    pub fn has_inner_shadows(&self) -> bool {
        !self.inner_shadow_layers.is_empty()
    }
    pub fn has_visible_inner_shadows(&self) -> bool {
        self.inner_shadow_layers.iter().any(|s| s.is_visible())
    }
    /// Union each projection of the original source extent, never a chain of
    /// projections. Does not change geometric paint/hit/layout/selection bounds.
    pub fn expand_shadow_bounds(
        &self,
        source: crate::Bounds,
        transform: crate::Affine,
    ) -> crate::Bounds {
        let mut bounds = source;
        for shadow in self.shadows() {
            let expanded = shadow.expand_bounds(source, transform);
            // f32 min/max discard NaN; preserve overflow sentinels instead of
            // silently shrinking invalid visual extents to the source rectangle.
            if !expanded.is_valid() {
                return expanded;
            }
            bounds = bounds.union(expanded);
        }
        bounds
    }
    /// Base fill followed by additional fills, in painter order.
    pub fn fills(&self) -> impl Iterator<Item = &Paint> {
        self.fill.iter().chain(self.fill_layers.iter())
    }
    /// Base stroke followed by additional strokes, in painter order.
    pub fn strokes(&self) -> impl Iterator<Item = &Stroke> {
        self.stroke.iter().chain(self.stroke_layers.iter())
    }
    /// Conservative outward stroke extent. Inside strokes have zero extent.
    /// Fully transparent paints and zero widths neither render nor enlarge
    /// painted bounds or stroke picking.
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
        if self.shadows().count() > MAX_SHADOW_LAYERS {
            return Err(format!(
                "appearance exceeds {MAX_SHADOW_LAYERS} shadow layers"
            ));
        }
        if self.inner_shadow_layers.len() > MAX_INNER_SHADOW_LAYERS {
            return Err(format!(
                "appearance exceeds {MAX_INNER_SHADOW_LAYERS} inner shadow layers"
            ));
        }
        for shadow in self.shadows().chain(self.inner_shadow_layers.iter()) {
            shadow.validate()?;
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

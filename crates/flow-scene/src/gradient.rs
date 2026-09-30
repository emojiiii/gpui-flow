//! Deterministic gradient sampling and bounded coverage rasterization.
//!
//! Gradients are defined in local coordinates and sampled after inverse affine
//! mapping. Color stops interpolate premultiplied sRGB components, including
//! alpha. Duplicate offsets are hard stops; values outside the stop range pad
//! to the nearest endpoint. No decoder or graphics toolkit is involved.
use crate::{Affine, Bounds, Color, GradientStop, Paint, Point, Size};
use std::fmt;

/// Protects native texture upload limits even for extremely thin rectangles.
pub const MAX_GRADIENT_DIMENSION: u32 = 4096;

/// Straight-alpha sRGB sample, with components in [0, 1].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ColorSample {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}
impl ColorSample {
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    pub fn from_color(color: Color) -> Self {
        Self {
            r: color.r as f32 / 255.0,
            g: color.g as f32 / 255.0,
            b: color.b as f32 / 255.0,
            a: color.a as f32 / 255.0,
        }
    }
    pub fn premultiplied(self) -> [f32; 4] {
        [self.r * self.a, self.g * self.a, self.b * self.a, self.a]
    }
    fn from_premultiplied(p: [f32; 4]) -> Self {
        if p[3] <= 0.0 {
            return Self::TRANSPARENT;
        }
        Self {
            r: (p[0] / p[3]).clamp(0.0, 1.0),
            g: (p[1] / p[3]).clamp(0.0, 1.0),
            b: (p[2] / p[3]).clamp(0.0, 1.0),
            a: p[3].clamp(0.0, 1.0),
        }
    }
    pub fn to_color(self) -> Color {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color::rgba(byte(self.r), byte(self.g), byte(self.b), byte(self.a))
    }
}
#[derive(Debug, Clone)]
pub struct PaintSampler<'a> {
    paint: &'a Paint,
}
impl<'a> PaintSampler<'a> {
    pub fn new(paint: &'a Paint) -> Result<Self, GradientError> {
        paint.validate().map_err(GradientError::InvalidPaint)?;
        Ok(Self { paint })
    }
    pub fn sample(&self, point: Point) -> ColorSample {
        if !point.is_finite() {
            return ColorSample::TRANSPARENT;
        }
        match self.paint {
            Paint::Solid { color } => ColorSample::from_color(*color),
            Paint::LinearGradient { start, end, stops } => {
                let dx = end.x as f64 - start.x as f64;
                let dy = end.y as f64 - start.y as f64;
                let t = ((point.x as f64 - start.x as f64) * dx
                    + (point.y as f64 - start.y as f64) * dy)
                    / (dx * dx + dy * dy);
                sample_stops(stops, t)
            }
            Paint::RadialGradient {
                center,
                radius,
                stops,
            } => sample_stops(
                stops,
                (point.x as f64 - center.x as f64).hypot(point.y as f64 - center.y as f64)
                    / *radius as f64,
            ),
        }
    }
}
fn sample_stops(stops: &[GradientStop], t: f64) -> ColorSample {
    let upper = stops.partition_point(|stop| stop.offset as f64 <= t);
    if upper == 0 {
        return ColorSample::from_color(stops[0].color);
    }
    if upper == stops.len() {
        return ColorSample::from_color(stops[upper - 1].color);
    }
    let left = &stops[upper - 1];
    let right = &stops[upper];
    let t = ((t - left.offset as f64) / (right.offset as f64 - left.offset as f64)).clamp(0.0, 1.0)
        as f32;
    let a = ColorSample::from_color(left.color).premultiplied();
    let b = ColorSample::from_color(right.color).premultiplied();
    ColorSample::from_premultiplied(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GradientError {
    InvalidPaint(String),
    InvalidGeometry,
    NonInvertibleTransform,
    InvalidOptions,
    TooLarge {
        width: u64,
        height: u64,
        max_pixels: usize,
    },
}
impl fmt::Display for GradientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPaint(message) => write!(f, "invalid gradient: {message}"),
            Self::InvalidGeometry => write!(f, "non-finite raster geometry"),
            Self::NonInvertibleTransform => write!(f, "gradient transform cannot be inverted"),
            Self::InvalidOptions => write!(
                f,
                "invalid raster scale, opacity, sample count, or size budget"
            ),
            Self::TooLarge {
                width,
                height,
                max_pixels,
            } => write!(
                f,
                "gradient raster {width}×{height} exceeds budget ({max_pixels} pixels / {MAX_GRADIENT_DIMENSION} per axis)"
            ),
        }
    }
}
impl std::error::Error for GradientError {}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RasterOptions {
    /// Device pixels per screen point. Geometry and clip bounds use screen points.
    pub scale_factor: f32,
    pub opacity: f32,
    /// Crop before allocating. A viewport mask prevents zoom from causing huge images.
    pub clip_bounds: Option<Bounds>,
    pub max_pixels: usize,
    /// 1, 2, or 4 samples per axis (1/4/16 coverage samples per device pixel).
    pub samples_per_axis: u8,
}
impl Default for RasterOptions {
    fn default() -> Self {
        Self {
            scale_factor: 1.0,
            opacity: 1.0,
            clip_bounds: None,
            max_pixels: 4 * 1024 * 1024,
            samples_per_axis: 2,
        }
    }
}
/// Straight-alpha BGRA bytes, as required by GPUI RenderImage. Bounds are
/// device-grid aligned screen coordinates. No scaling/downsampling is hidden.
#[derive(Debug, Clone, PartialEq)]
pub struct GradientRaster {
    pub bounds: Bounds,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}
impl GradientRaster {
    pub fn pixel(&self, x: u32, y: u32) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y * self.width + x) * 4) as usize;
        Some(Color::rgba(
            self.bgra[i + 2],
            self.bgra[i + 1],
            self.bgra[i],
            self.bgra[i + 3],
        ))
    }
    pub fn byte_len(&self) -> usize {
        self.bgra.len()
    }
}
/// Rasterize an already-tessellated screen-space triangle union. Adjacent and
/// overlapping triangles share a coverage mask, so they never double-composite
/// alpha or introduce interior seams. Supply clipped triangles for arbitrary
/// nested clip shapes; clip_bounds additionally applies an exact viewport crop.
pub fn rasterize_paint(
    paint: &Paint,
    triangles: &[[Point; 3]],
    local_to_screen: Affine,
    options: RasterOptions,
) -> Result<Option<GradientRaster>, GradientError> {
    let sampler = PaintSampler::new(paint)?;
    if !options.scale_factor.is_finite()
        || options.scale_factor <= 0.0
        || !options.opacity.is_finite()
        || !(0.0..=1.0).contains(&options.opacity)
        || options.max_pixels == 0
        || !matches!(options.samples_per_axis, 1 | 2 | 4)
    {
        return Err(GradientError::InvalidOptions);
    }
    if !local_to_screen.is_finite()
        || triangles.iter().flatten().any(|p| !p.is_finite())
        || options.clip_bounds.is_some_and(|b| !b.is_valid())
    {
        return Err(GradientError::InvalidGeometry);
    }
    let inverse = local_to_screen
        .inverse()
        .ok_or(GradientError::NonInvertibleTransform)?;
    let Some(mut bounds) = crate::geometry::point_bounds(triangles.iter().flatten().copied())
    else {
        return Ok(None);
    };
    if !bounds.is_valid() {
        return Err(GradientError::InvalidGeometry);
    }
    if let Some(clip) = options.clip_bounds {
        let Some(cropped) = crate::geometry::intersection(bounds, clip) else {
            return Ok(None);
        };
        bounds = cropped;
    }
    if !bounds.size.is_positive() || options.opacity == 0.0 || !paint.is_visible() {
        return Ok(None);
    }
    let scale = options.scale_factor as f64;
    let left = (bounds.min_x() as f64 * scale).floor();
    let top = (bounds.min_y() as f64 * scale).floor();
    let right = (bounds.max_x() as f64 * scale).ceil();
    let bottom = (bounds.max_y() as f64 * scale).ceil();
    let width = right - left;
    let height = bottom - top;
    if ![left, top, right, bottom, width, height]
        .iter()
        .all(|v| v.is_finite())
        || width < 1.0
        || height < 1.0
    {
        return Err(GradientError::InvalidGeometry);
    }
    if width > MAX_GRADIENT_DIMENSION as f64
        || height > MAX_GRADIENT_DIMENSION as f64
        || width * height > options.max_pixels as f64
    {
        return Err(GradientError::TooLarge {
            width: width.min(u64::MAX as f64) as u64,
            height: height.min(u64::MAX as f64) as u64,
            max_pixels: options.max_pixels,
        });
    }
    let width = width as u32;
    let height = height as u32;
    let len = (width as usize)
        .checked_mul(height as usize)
        .ok_or(GradientError::InvalidGeometry)?;
    let samples = options.samples_per_axis as usize;
    let sample_count = samples * samples;
    let full_mask = if sample_count == 16 {
        u16::MAX
    } else {
        (1u16 << sample_count) - 1
    };
    let mut coverage = vec![0u16; len];
    for triangle in triangles {
        let vertices = triangle.map(|p| [p.x as f64 * scale - left, p.y as f64 * scale - top]);
        let edge = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| {
            (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
        };
        let area = edge(vertices[0], vertices[1], vertices[2]);
        if area == 0.0 {
            continue;
        }
        let sign = area.signum();
        let planes = std::array::from_fn::<_, 3, _>(|i| {
            let a = vertices[i];
            let b = vertices[(i + 1) % 3];
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            [-dy * sign, dx * sign, (dy * a[0] - dx * a[1]) * sign]
        });
        let x0 = vertices
            .iter()
            .map(|p| p[0])
            .fold(f64::INFINITY, f64::min)
            .floor()
            .clamp(0.0, width as f64) as u32;
        let x1 = vertices
            .iter()
            .map(|p| p[0])
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .clamp(0.0, width as f64) as u32;
        let y0 = vertices
            .iter()
            .map(|p| p[1])
            .fold(f64::INFINITY, f64::min)
            .floor()
            .clamp(0.0, height as f64) as u32;
        let y1 = vertices
            .iter()
            .map(|p| p[1])
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .clamp(0.0, height as f64) as u32;
        for y in y0..y1 {
            for x in x0..x1 {
                let mask = &mut coverage[y as usize * width as usize + x as usize];
                if *mask == full_mask {
                    continue;
                }
                let center = [x as f64 + 0.5, y as f64 + 0.5];
                let distances =
                    planes.map(|plane| plane[0] * center[0] + plane[1] * center[1] + plane[2]);
                let margins = planes.map(|plane| (plane[0].abs() + plane[1].abs()) * 0.5);
                if (0..3).any(|i| distances[i] + margins[i] < -1e-10) {
                    continue;
                }
                let fully_in_clip = options.clip_bounds.is_none_or(|clip| {
                    left + x as f64 >= clip.min_x() as f64 * scale
                        && top + y as f64 >= clip.min_y() as f64 * scale
                        && left + x as f64 + 1.0 <= clip.max_x() as f64 * scale
                        && top + y as f64 + 1.0 <= clip.max_y() as f64 * scale
                });
                if fully_in_clip && (0..3).all(|i| distances[i] - margins[i] >= 0.0) {
                    *mask = full_mask;
                    continue;
                }
                for sy in 0..samples {
                    for sx in 0..samples {
                        let bit = 1u16 << (sy * samples + sx);
                        if *mask & bit != 0 {
                            continue;
                        }
                        let point = [
                            x as f64 + (sx as f64 + 0.5) / samples as f64,
                            y as f64 + (sy as f64 + 0.5) / samples as f64,
                        ];
                        if (0..3).all(|i| {
                            sign * edge(vertices[i], vertices[(i + 1) % 3], point) >= -1e-10
                        }) {
                            let screen = Point::new(
                                ((left + point[0]) / scale) as f32,
                                ((top + point[1]) / scale) as f32,
                            );
                            if options.clip_bounds.is_none_or(|clip| clip.contains(screen)) {
                                *mask |= bit;
                            }
                        }
                    }
                }
            }
        }
    }
    let mut bgra = vec![0u8; len.checked_mul(4).ok_or(GradientError::InvalidGeometry)?];
    for (index, mask) in coverage.into_iter().enumerate() {
        if mask == 0 {
            continue;
        }
        let x = index % width as usize;
        let y = index / width as usize;
        // Like a native fragment shader, shade fully covered pixels once at
        // their centers. Supersampling is retained at geometry/clip boundaries.
        if mask == full_mask {
            let screen = Point::new(
                ((left + x as f64 + 0.5) / scale) as f32,
                ((top + y as f64 + 0.5) / scale) as f32,
            );
            let mut sample = sampler.sample(inverse.transform_point(screen));
            sample.a *= options.opacity;
            let color = sample.to_color();
            bgra[index * 4..index * 4 + 4].copy_from_slice(&[color.b, color.g, color.r, color.a]);
            continue;
        }
        let mut accumulated = [0.0f32; 4];
        for sy in 0..samples {
            for sx in 0..samples {
                if mask & (1u16 << (sy * samples + sx)) == 0 {
                    continue;
                }
                let screen = Point::new(
                    ((left + x as f64 + (sx as f64 + 0.5) / samples as f64) / scale) as f32,
                    ((top + y as f64 + (sy as f64 + 0.5) / samples as f64) / scale) as f32,
                );
                let color = sampler
                    .sample(inverse.transform_point(screen))
                    .premultiplied();
                for i in 0..4 {
                    accumulated[i] += color[i] * options.opacity / sample_count as f32;
                }
            }
        }
        let color = ColorSample::from_premultiplied(accumulated).to_color();
        bgra[index * 4..index * 4 + 4].copy_from_slice(&[color.b, color.g, color.r, color.a]);
    }
    let bounds = Bounds::new(
        Point::new((left / scale) as f32, (top / scale) as f32),
        Size::new(
            width as f32 / options.scale_factor,
            height as f32 / options.scale_factor,
        ),
    );
    Ok(Some(GradientRaster {
        bounds,
        width,
        height,
        bgra,
    }))
}

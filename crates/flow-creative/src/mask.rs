//! Non-destructive, source-bound mask strokes and bounded CPU rasterization.
//!
//! Mask pixels always mean `0 = preserve` and `255 = edit`. There is no alpha
//! contract, antialiasing, feathering, image decoding, or image mutation here.
//! Providers that need a different convention must adapt the exported pixels.
//! The host owns undo transactions, asynchronous execution, and asset encoding.

use serde::{Deserialize, Serialize};
use std::{
    fmt,
    sync::atomic::{AtomicBool, Ordering},
};

pub const MAX_MASK_STROKES: usize = 128;
pub const MAX_MASK_POINTS: usize = 2048;
pub const MAX_MASK_DIMENSION: u32 = 32_768;
pub const MAX_MASK_PIXELS: u64 = 64 * 1024 * 1024;
/// Includes zero-initializing the output and every candidate pixel sampled.
/// Bounding boxes are charged even when strokes overlap or erase clear pixels.
pub const MAX_MASK_WORK: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaskError {
    MissingSource,
    SourceMismatch,
    InvalidSource(&'static str),
    InvalidPoint,
    InvalidStroke(&'static str),
    TooManyStrokes,
    TooManyPoints,
    InvalidRasterSize,
    WorkLimitExceeded,
    AllocationFailed,
    Cancelled,
}

impl fmt::Display for MaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSource => f.write_str("mask has no source image"),
            Self::SourceMismatch => {
                f.write_str("mask source changed; explicitly reset the mask before drawing")
            }
            Self::InvalidSource(reason) | Self::InvalidStroke(reason) => f.write_str(reason),
            Self::InvalidPoint => f.write_str("mask point must be finite and within 0..=1"),
            Self::TooManyStrokes => f.write_str("mask exceeds the 128 stroke limit"),
            Self::TooManyPoints => f.write_str("mask exceeds the 2048 point limit"),
            Self::InvalidRasterSize => f.write_str("invalid mask preview size"),
            Self::WorkLimitExceeded => f.write_str("mask exceeds the raster work limit"),
            Self::AllocationFailed => f.write_str("could not allocate the mask raster"),
            Self::Cancelled => f.write_str("mask rasterization cancelled"),
        }
    }
}

impl std::error::Error for MaskError {}

/// Identity includes the project asset key, not just its dimensions. Project
/// asset keys must identify immutable bytes; replacing an image needs a new key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskSource {
    pub asset_key: String,
    pub width: u32,
    pub height: u32,
}

impl MaskSource {
    pub fn new(asset_key: impl Into<String>, width: u32, height: u32) -> Result<Self, MaskError> {
        let source = Self {
            asset_key: asset_key.into(),
            width,
            height,
        };
        source.validate()?;
        Ok(source)
    }

    pub fn validate(&self) -> Result<(), MaskError> {
        // This is an identity token, never a path to open. Match the project's
        // image-key grammar while excluding unrelated audio/video/model assets.
        let valid_key = self.asset_key.strip_prefix("assets/").is_some_and(|name| {
            name.len() <= 128
                && name.rsplit_once('.').is_some_and(|(stem, extension)| {
                    !stem.is_empty()
                        && stem
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                        && matches!(extension, "png" | "jpg" | "gif" | "webp")
                })
        });
        if !valid_key {
            return Err(MaskError::InvalidSource(
                "invalid mask source image asset key",
            ));
        }
        if self.width == 0
            || self.height == 0
            || self.width > MAX_MASK_DIMENSION
            || self.height > MAX_MASK_DIMENSION
            || u64::from(self.width) * u64::from(self.height) > MAX_MASK_PIXELS
        {
            return Err(MaskError::InvalidSource(
                "invalid mask source image dimensions",
            ));
        }
        Ok(())
    }

    /// Preview dimensions fit the bound without upscaling. Rounding is to the
    /// nearest pixel, with at least one pixel on either axis. Raster samples
    /// always map back to source space, including for non-square images.
    pub fn raster_dimensions(&self, size: MaskRasterSize) -> Result<(u32, u32), MaskError> {
        self.validate()?;
        match size {
            MaskRasterSize::Source => Ok((self.width, self.height)),
            MaskRasterSize::Preview { max_edge } => {
                if max_edge == 0 || max_edge > MAX_MASK_DIMENSION {
                    return Err(MaskError::InvalidRasterSize);
                }
                let source_edge = self.width.max(self.height);
                if source_edge <= max_edge {
                    return Ok((self.width, self.height));
                }
                let scaled = |axis: u32| {
                    ((u64::from(axis) * u64::from(max_edge) + u64::from(source_edge) / 2)
                        / u64::from(source_edge))
                    .max(1) as u32
                };
                Ok((scaled(self.width), scaled(self.height)))
            }
        }
    }
}

/// Compact JSON `[x, y]`. Values span the source image edges, so 0 is the
/// left/top edge and 65535 is the right/bottom edge. Pixel tests use centers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskPoint(pub u16, pub u16);

impl MaskPoint {
    /// Reject rather than clamp invalid UI coordinates. The caller must map
    /// pointer positions into the actual image content rectangle first.
    pub fn from_normalized(x: f32, y: f32) -> Result<Self, MaskError> {
        if !x.is_finite()
            || !y.is_finite()
            || !(0.0..=1.0).contains(&x)
            || !(0.0..=1.0).contains(&y)
        {
            return Err(MaskError::InvalidPoint);
        }
        Ok(Self(
            (x * u16::MAX as f32).round() as u16,
            (y * u16::MAX as f32).round() as u16,
        ))
    }

    pub fn normalized(self) -> (f32, f32) {
        (
            f32::from(self.0) / f32::from(u16::MAX),
            f32::from(self.1) / f32::from(u16::MAX),
        )
    }

    fn source_position(self, source: &MaskSource) -> (f64, f64) {
        (
            f64::from(self.0) * f64::from(source.width) / f64::from(u16::MAX),
            f64::from(self.1) * f64::from(source.height) / f64::from(u16::MAX),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskMode {
    Paint,
    Erase,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskStroke {
    pub mode: MaskMode,
    /// Brush radius as a fraction of the source image's shorter edge (0, 1].
    /// It is a radius, not a diameter. Finite, strictly positive values only.
    pub radius: f32,
    /// A one-point stroke is a circle; every adjacent pair is a round-capped
    /// continuous segment. Repeated points are valid stationary brush samples.
    pub points: Vec<MaskPoint>,
}

impl MaskStroke {
    pub fn new(mode: MaskMode, radius: f32, points: Vec<MaskPoint>) -> Result<Self, MaskError> {
        let stroke = Self {
            mode,
            radius,
            points,
        };
        stroke.validate()?;
        Ok(stroke)
    }

    pub fn validate(&self) -> Result<(), MaskError> {
        if !self.radius.is_finite() || self.radius <= 0.0 || self.radius > 1.0 {
            return Err(MaskError::InvalidStroke(
                "mask brush radius must be finite and within (0, 1]",
            ));
        }
        if self.points.is_empty() {
            return Err(MaskError::InvalidStroke("mask stroke must contain a point"));
        }
        if self.points.len() > MAX_MASK_POINTS {
            return Err(MaskError::TooManyPoints);
        }
        Ok(())
    }
}

/// Store this data in the node payload; keep source image bytes in the project.
/// Undo removes/restores whole strokes via the host editor's transaction model.
/// Validate after deserializing: fields are public for editor/serde integration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskState {
    pub source: Option<MaskSource>,
    pub strokes: Vec<MaskStroke>,
}

impl MaskState {
    pub fn new(source: MaskSource) -> Result<Self, MaskError> {
        source.validate()?;
        Ok(Self {
            source: Some(source),
            strokes: Vec::new(),
        })
    }

    pub fn source_matches(&self, source: &MaskSource) -> bool {
        self.source.as_ref() == Some(source)
    }

    pub fn validate(&self) -> Result<(), MaskError> {
        match &self.source {
            Some(source) => source.validate()?,
            None if !self.strokes.is_empty() => return Err(MaskError::MissingSource),
            None => {}
        }
        if self.strokes.len() > MAX_MASK_STROKES {
            return Err(MaskError::TooManyStrokes);
        }
        let mut points = 0;
        for stroke in &self.strokes {
            stroke.validate()?;
            points += stroke.points.len();
            if points > MAX_MASK_POINTS {
                return Err(MaskError::TooManyPoints);
            }
        }
        Ok(())
    }

    /// Explicitly discard the previous strokes and bind to an image. Merely
    /// reconnecting an input must not call this implicitly or reuse old strokes.
    pub fn reset_source(&mut self, source: MaskSource) -> Result<(), MaskError> {
        source.validate()?;
        self.source = Some(source);
        self.strokes.clear();
        Ok(())
    }

    /// Failure leaves the state unchanged. The UI should commit one complete
    /// stroke per editor transaction rather than a transaction per pointer move.
    pub fn push_stroke(&mut self, stroke: MaskStroke) -> Result<(), MaskError> {
        self.validate()?;
        if self.source.is_none() {
            return Err(MaskError::MissingSource);
        }
        stroke.validate()?;
        if self.strokes.len() >= MAX_MASK_STROKES {
            return Err(MaskError::TooManyStrokes);
        }
        if self.strokes.iter().map(|s| s.points.len()).sum::<usize>() + stroke.points.len()
            > MAX_MASK_POINTS
        {
            return Err(MaskError::TooManyPoints);
        }
        self.strokes.push(stroke);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskRasterSize {
    Source,
    Preview { max_edge: u32 },
}

/// Row-major, one-byte grayscale pixels, with no stride padding or alpha plane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskRaster {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Rasterize every stroke at one output size using its original source-space
/// geometry. No downsampled mask is ever reused as a full-resolution mask.
///
/// Resource bounds: at most 64 MiB for pixel storage, no auxiliary raster, and
/// `MAX_MASK_WORK` zero-fill/sampling operations. A conservative bounding-box
/// preflight rejects excess work before allocation. Cancellation is checked in
/// preflight, during zero-fill, per row, and at most every 4096 pixel samples.
/// Errors never return a partial raster; the caller must keep its prior output.
pub fn rasterize(
    state: &MaskState,
    source: &MaskSource,
    size: MaskRasterSize,
    cancelled: &AtomicBool,
) -> Result<MaskRaster, MaskError> {
    check_cancelled(cancelled)?;
    state.validate()?;
    source.validate()?;
    if state.source.is_none() {
        return Err(MaskError::MissingSource);
    }
    if !state.source_matches(source) {
        return Err(MaskError::SourceMismatch);
    }
    let (width, height) = source.raster_dimensions(size)?;
    let pixel_count = u64::from(width) * u64::from(height);
    let mut work = pixel_count;
    for stroke in &state.strokes {
        for_segments(stroke, source, |segment| {
            check_cancelled(cancelled)?;
            let bounds = segment.bounds(source, width, height);
            work += u64::from(bounds.x1 - bounds.x0) * u64::from(bounds.y1 - bounds.y0);
            if work > MAX_MASK_WORK {
                return Err(MaskError::WorkLimitExceeded);
            }
            Ok(())
        })?;
    }

    let pixel_count = usize::try_from(pixel_count).map_err(|_| MaskError::AllocationFailed)?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(pixel_count)
        .map_err(|_| MaskError::AllocationFailed)?;
    while pixels.len() < pixel_count {
        check_cancelled(cancelled)?;
        pixels.resize((pixels.len() + 65_536).min(pixel_count), 0);
    }
    let x_step = f64::from(source.width) / f64::from(width);
    let y_step = f64::from(source.height) / f64::from(height);
    for stroke in &state.strokes {
        let value = match stroke.mode {
            MaskMode::Paint => 255,
            MaskMode::Erase => 0,
        };
        for_segments(stroke, source, |segment| {
            check_cancelled(cancelled)?;
            let bounds = segment.bounds(source, width, height);
            for y in bounds.y0..bounds.y1 {
                check_cancelled(cancelled)?;
                let py = (f64::from(y) + 0.5) * y_step;
                let row = y as usize * width as usize;
                for x in bounds.x0..bounds.x1 {
                    if (x - bounds.x0) % 4096 == 0 {
                        check_cancelled(cancelled)?;
                    }
                    let px = (f64::from(x) + 0.5) * x_step;
                    if segment.contains(px, py) {
                        pixels[row + x as usize] = value;
                    }
                }
            }
            Ok(())
        })?;
    }
    check_cancelled(cancelled)?;
    Ok(MaskRaster {
        width,
        height,
        pixels,
    })
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), MaskError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(MaskError::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Segment {
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
    radius: f64,
}

struct Bounds {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

impl Segment {
    fn bounds(self, source: &MaskSource, width: u32, height: u32) -> Bounds {
        let sx = f64::from(width) / f64::from(source.width);
        let sy = f64::from(height) / f64::from(source.height);
        let lower = |value: f64, scale: f64, limit: u32| {
            (value * scale).floor().clamp(0.0, f64::from(limit)) as u32
        };
        let upper = |value: f64, scale: f64, limit: u32| {
            (value * scale).ceil().clamp(0.0, f64::from(limit)) as u32
        };
        Bounds {
            x0: lower(self.ax.min(self.bx) - self.radius, sx, width),
            y0: lower(self.ay.min(self.by) - self.radius, sy, height),
            x1: upper(self.ax.max(self.bx) + self.radius, sx, width),
            y1: upper(self.ay.max(self.by) + self.radius, sy, height),
        }
    }

    fn contains(self, px: f64, py: f64) -> bool {
        let dx = self.bx - self.ax;
        let dy = self.by - self.ay;
        let length_squared = dx * dx + dy * dy;
        let projection = if length_squared == 0.0 {
            0.0
        } else {
            (((px - self.ax) * dx + (py - self.ay) * dy) / length_squared).clamp(0.0, 1.0)
        };
        let distance_x = px - (self.ax + projection * dx);
        let distance_y = py - (self.ay + projection * dy);
        distance_x * distance_x + distance_y * distance_y <= self.radius * self.radius
    }
}

fn for_segments(
    stroke: &MaskStroke,
    source: &MaskSource,
    mut visit: impl FnMut(Segment) -> Result<(), MaskError>,
) -> Result<(), MaskError> {
    let radius = f64::from(stroke.radius) * f64::from(source.width.min(source.height));
    // All callers validate first. One point is a zero-length capsule (a circle).
    let mut start = stroke.points[0].source_position(source);
    let ends = if stroke.points.len() == 1 {
        &stroke.points[..]
    } else {
        &stroke.points[1..]
    };
    for point in ends {
        let end = point.source_position(source);
        visit(Segment {
            ax: start.0,
            ay: start.1,
            bx: end.0,
            by: end.1,
            radius,
        })?;
        start = end;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(width: u32, height: u32) -> MaskSource {
        MaskSource::new("assets/source.png", width, height).unwrap()
    }

    fn stroke(mode: MaskMode, radius: f32, points: &[(f32, f32)]) -> MaskStroke {
        MaskStroke::new(
            mode,
            radius,
            points
                .iter()
                .map(|&(x, y)| MaskPoint::from_normalized(x, y).unwrap())
                .collect(),
        )
        .unwrap()
    }

    fn render(state: &MaskState) -> MaskRaster {
        rasterize(
            state,
            state.source.as_ref().unwrap(),
            MaskRasterSize::Source,
            &AtomicBool::new(false),
        )
        .unwrap()
    }

    fn pixel(raster: &MaskRaster, x: u32, y: u32) -> u8 {
        raster.pixels[(y * raster.width + x) as usize]
    }

    #[test]
    fn empty_mask_preserves_every_pixel_and_one_point_paints_a_hard_circle() {
        let mut state = MaskState::new(source(16, 16)).unwrap();
        assert_eq!(render(&state).pixels, vec![0; 256]);
        state
            .push_stroke(stroke(MaskMode::Paint, 0.2, &[(0.5, 0.5)]))
            .unwrap();
        let mask = render(&state);
        assert_eq!(pixel(&mask, 8, 8), 255);
        assert_eq!(pixel(&mask, 10, 8), 255);
        assert_eq!(pixel(&mask, 11, 8), 0);
        assert_eq!(pixel(&mask, 10, 10), 0);
        assert!(mask.pixels.iter().all(|&value| value == 0 || value == 255));
    }

    #[test]
    fn sparse_points_draw_continuous_round_capped_segments() {
        let mut state = MaskState::new(source(32, 16)).unwrap();
        state
            .push_stroke(stroke(MaskMode::Paint, 0.1, &[(0.25, 0.5), (0.75, 0.5)]))
            .unwrap();
        let mask = render(&state);
        for x in 7..25 {
            assert_eq!(pixel(&mask, x, 8), 255, "gap at x={x}");
        }
        assert_eq!(pixel(&mask, 6, 8), 255);
        assert_eq!(pixel(&mask, 25, 8), 255);
        assert_eq!(pixel(&mask, 5, 8), 0);
        assert_eq!(pixel(&mask, 26, 8), 0);
        assert_eq!(pixel(&mask, 16, 10), 0);
    }

    #[test]
    fn diagonal_and_bent_paths_are_continuous_without_filling_their_interior() {
        let mut state = MaskState::new(source(32, 32)).unwrap();
        state
            .push_stroke(stroke(
                MaskMode::Paint,
                0.04,
                &[(0.1, 0.1), (0.5, 0.5), (0.9, 0.1)],
            ))
            .unwrap();
        let mask = render(&state);
        for p in 3..16 {
            assert_eq!(pixel(&mask, p, p), 255);
            assert_eq!(pixel(&mask, 31 - p, p), 255);
        }
        assert_eq!(pixel(&mask, 16, 5), 0);
    }

    #[test]
    fn erase_is_ordered_and_removing_a_stroke_restores_the_prior_mask() {
        let mut state = MaskState::new(source(16, 16)).unwrap();
        state
            .push_stroke(stroke(MaskMode::Paint, 0.4, &[(0.5, 0.5)]))
            .unwrap();
        let painted = render(&state);
        state
            .push_stroke(stroke(MaskMode::Erase, 0.15, &[(0.5, 0.2), (0.5, 0.8)]))
            .unwrap();
        let erased = render(&state);
        assert_eq!(pixel(&painted, 8, 8), 255);
        assert_eq!(pixel(&erased, 8, 8), 0);
        assert_eq!(pixel(&erased, 3, 8), 255);
        let erase = state.strokes.pop().unwrap();
        assert_eq!(render(&state), painted);
        state.push_stroke(erase).unwrap();
        assert_eq!(render(&state), erased);
        state
            .push_stroke(stroke(MaskMode::Paint, 0.05, &[(0.5, 0.5)]))
            .unwrap();
        assert_eq!(pixel(&render(&state), 8, 8), 255);
    }

    #[test]
    fn preview_uses_source_geometry_for_all_strokes_and_preserves_aspect_ratio() {
        let source = source(120, 60);
        let mut state = MaskState::new(source.clone()).unwrap();
        state
            .push_stroke(stroke(MaskMode::Paint, 0.3, &[(0.2, 0.5), (0.8, 0.5)]))
            .unwrap();
        state
            .push_stroke(stroke(MaskMode::Erase, 0.12, &[(0.5, 0.0), (0.5, 1.0)]))
            .unwrap();
        let full = render(&state);
        let preview = rasterize(
            &state,
            &source,
            MaskRasterSize::Preview { max_edge: 40 },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!((preview.width, preview.height), (40, 20));
        // A 3:1 ratio makes preview centers coincide with full-size centers.
        for y in 0..preview.height {
            for x in 0..preview.width {
                assert_eq!(pixel(&preview, x, y), pixel(&full, x * 3 + 1, y * 3 + 1));
            }
        }
        assert_eq!(pixel(&preview, 10, 10), 255);
        assert_eq!(pixel(&preview, 20, 10), 0);
        assert_eq!(
            source
                .raster_dimensions(MaskRasterSize::Preview { max_edge: 300 })
                .unwrap(),
            (120, 60)
        );
    }

    #[test]
    fn replacing_image_even_at_same_size_invalidates_the_existing_mask() {
        let original = source(16, 16);
        let mut state = MaskState::new(original.clone()).unwrap();
        state
            .push_stroke(stroke(MaskMode::Paint, 0.1, &[(0.5, 0.5)]))
            .unwrap();
        assert!(state.source_matches(&original));
        for changed in [
            MaskSource::new("assets/replacement.png", 16, 16).unwrap(),
            MaskSource::new("assets/source.png", 32, 16).unwrap(),
            MaskSource::new("assets/source.png", 16, 32).unwrap(),
        ] {
            assert!(!state.source_matches(&changed));
            assert_eq!(
                rasterize(
                    &state,
                    &changed,
                    MaskRasterSize::Source,
                    &AtomicBool::new(false)
                ),
                Err(MaskError::SourceMismatch)
            );
        }
        let changed = MaskSource::new("assets/replacement.png", 16, 16).unwrap();
        state.reset_source(changed.clone()).unwrap();
        assert!(state.source_matches(&changed));
        assert!(state.strokes.is_empty());
        assert!(render(&state).pixels.iter().all(|&value| value == 0));
    }

    #[test]
    fn invalid_points_radii_keys_and_dimensions_are_rejected() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.1, 1.1] {
            assert_eq!(
                MaskPoint::from_normalized(value, 0.5),
                Err(MaskError::InvalidPoint)
            );
            assert_eq!(
                MaskPoint::from_normalized(0.5, value),
                Err(MaskError::InvalidPoint)
            );
        }
        for radius in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.1, 0.0, 1.01] {
            assert!(MaskStroke::new(MaskMode::Paint, radius, vec![MaskPoint(0, 0)]).is_err());
        }
        assert!(MaskStroke::new(MaskMode::Paint, 0.1, vec![]).is_err());
        for key in [
            "",
            "source.png",
            "assets/../image.png",
            "assets/x/y.png",
            "assets/a.glb",
            "assets/a.png?x",
            "https://example.com/a.png",
        ] {
            assert!(MaskSource::new(key, 16, 16).is_err(), "accepted {key}");
        }
        for (width, height) in [(0, 1), (1, 0), (32_769, 1), (1, 32_769), (8193, 8192)] {
            assert!(MaskSource::new("assets/source.png", width, height).is_err());
        }
        assert!(MaskSource::new("assets/source.png", 8192, 8192).is_ok());
        assert!(MaskSource::new("assets/source.png", 32_768, 2048).is_ok());
    }

    #[test]
    fn boundaries_stationary_points_and_one_pixel_previews_are_supported() {
        let mut state = MaskState::new(source(16, 16)).unwrap();
        state
            .push_stroke(stroke(MaskMode::Paint, 0.1, &[(0.0, 0.0), (0.0, 0.0)]))
            .unwrap();
        state
            .push_stroke(stroke(MaskMode::Paint, 0.1, &[(1.0, 1.0)]))
            .unwrap();
        let raster = render(&state);
        assert_eq!(pixel(&raster, 0, 0), 255);
        assert_eq!(pixel(&raster, 15, 15), 255);
        assert_eq!(pixel(&raster, 8, 8), 0);
        let thin = source(32_768, 1);
        assert_eq!(
            thin.raster_dimensions(MaskRasterSize::Preview { max_edge: 1 })
                .unwrap(),
            (1, 1)
        );
        for max_edge in [0, 32_769, u32::MAX] {
            assert_eq!(
                thin.raster_dimensions(MaskRasterSize::Preview { max_edge }),
                Err(MaskError::InvalidRasterSize)
            );
        }
        let mut single = MaskState::new(source(1, 1)).unwrap();
        single
            .push_stroke(stroke(MaskMode::Paint, 1.0, &[(0.5, 0.5)]))
            .unwrap();
        assert_eq!(render(&single).pixels, [255]);
    }

    #[test]
    fn unbound_states_fail_to_draw_and_invalid_mutations_are_atomic() {
        let mut state = MaskState::default();
        assert!(state.validate().is_ok());
        assert_eq!(
            state.push_stroke(stroke(MaskMode::Paint, 0.1, &[(0.5, 0.5)])),
            Err(MaskError::MissingSource)
        );
        assert_eq!(
            rasterize(
                &state,
                &source(16, 16),
                MaskRasterSize::Source,
                &AtomicBool::new(false)
            ),
            Err(MaskError::MissingSource)
        );
        state.reset_source(source(16, 16)).unwrap();
        let before = state.clone();
        assert!(
            state
                .reset_source(MaskSource {
                    asset_key: "bad".into(),
                    width: 16,
                    height: 16
                })
                .is_err()
        );
        assert_eq!(state, before);
        assert!(
            state
                .push_stroke(MaskStroke {
                    mode: MaskMode::Paint,
                    radius: 0.1,
                    points: vec![]
                })
                .is_err()
        );
        assert_eq!(state, before);
    }

    #[test]
    fn total_point_and_stroke_limits_are_enforced_before_mutation() {
        let mut state = MaskState::new(source(16, 16)).unwrap();
        for _ in 0..MAX_MASK_STROKES {
            state
                .push_stroke(stroke(MaskMode::Paint, 0.1, &[(0.5, 0.5)]))
                .unwrap();
        }
        let before = state.clone();
        assert_eq!(
            state.push_stroke(stroke(MaskMode::Paint, 0.1, &[(0.5, 0.5)])),
            Err(MaskError::TooManyStrokes)
        );
        assert_eq!(state, before);
        state.strokes.clear();
        for _ in 0..2 {
            state
                .push_stroke(
                    MaskStroke::new(
                        MaskMode::Paint,
                        0.1,
                        vec![MaskPoint(0, 0); MAX_MASK_POINTS / 2],
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let before = state.clone();
        assert_eq!(
            state.push_stroke(stroke(MaskMode::Paint, 0.1, &[(0.5, 0.5)])),
            Err(MaskError::TooManyPoints)
        );
        assert_eq!(state, before);
        state.strokes[0].points.push(MaskPoint(0, 0));
        assert_eq!(state.validate(), Err(MaskError::TooManyPoints));
    }

    #[test]
    fn cancellation_and_work_limit_abort_without_returning_partial_pixels() {
        let mut state = MaskState::new(source(8192, 8192)).unwrap();
        assert_eq!(
            rasterize(
                &state,
                state.source.as_ref().unwrap(),
                MaskRasterSize::Source,
                &AtomicBool::new(true)
            ),
            Err(MaskError::Cancelled)
        );
        // Two full-image bounding boxes plus zero-fill exceed 128 Mi operations;
        // preflight returns before allocating the 64 MiB full-resolution raster.
        for _ in 0..2 {
            state
                .push_stroke(stroke(MaskMode::Paint, 1.0, &[(0.5, 0.5)]))
                .unwrap();
        }
        assert_eq!(
            rasterize(
                &state,
                state.source.as_ref().unwrap(),
                MaskRasterSize::Source,
                &AtomicBool::new(false)
            ),
            Err(MaskError::WorkLimitExceeded)
        );
        assert!(
            rasterize(
                &state,
                state.source.as_ref().unwrap(),
                MaskRasterSize::Preview { max_edge: 16 },
                &AtomicBool::new(false)
            )
            .is_ok()
        );
        // A long path with individually legal points cannot bypass the budget.
        state.strokes = vec![
            MaskStroke::new(
                MaskMode::Paint,
                0.01,
                (0..MAX_MASK_POINTS)
                    .map(|i| {
                        if i % 2 == 0 {
                            MaskPoint(0, 0)
                        } else {
                            MaskPoint(u16::MAX, u16::MAX)
                        }
                    })
                    .collect(),
            )
            .unwrap(),
        ];
        assert_eq!(
            rasterize(
                &state,
                state.source.as_ref().unwrap(),
                MaskRasterSize::Source,
                &AtomicBool::new(false)
            ),
            Err(MaskError::WorkLimitExceeded)
        );
    }

    #[test]
    fn maximal_state_roundtrips_with_room_for_the_rest_of_a_64_kib_node() {
        let mut state = MaskState::new(
            MaskSource::new(format!("assets/{}.png", "a".repeat(124)), 32_768, 2048).unwrap(),
        )
        .unwrap();
        for _ in 0..MAX_MASK_STROKES {
            state
                .push_stroke(
                    MaskStroke::new(
                        MaskMode::Erase,
                        0.12345678,
                        vec![MaskPoint(u16::MAX, u16::MAX); MAX_MASK_POINTS / MAX_MASK_STROKES],
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let encoded = serde_json::to_vec(&state).unwrap();
        assert!(
            encoded.len() < 48 * 1024,
            "{} bytes leave insufficient node headroom",
            encoded.len()
        );
        let decoded: MaskState = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, state);
        decoded.validate().unwrap();
        let node_payload = serde_json::json!({"caption": "Mask", "mask": state});
        assert!(serde_json::to_vec(&node_payload).unwrap().len() < 64 * 1024);
        assert_eq!(
            serde_json::to_string(&MaskPoint(123, 456)).unwrap(),
            "[123,456]"
        );
        assert!(serde_json::from_str::<MaskPoint>("[65536,0]").is_err());
        assert!(serde_json::from_str::<MaskPoint>("[-1,0]").is_err());
        assert!(serde_json::from_str::<MaskPoint>("[0,0,0]").is_err());
    }
}

//! Width-only compensation. Scale provenance follows each local orthogonal
//! transform's columns through the hierarchy, not the composed world matrix.
use crate::{Affine, DisplayObject, Stroke};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser};

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum StrokeScaleFixed {
    #[default]
    Disabled,
    Fixed,
    Threshold(f32),
    /// Derived presentation geometry only. Preserves shared-mesh rendering of
    /// solid round strokes; never accepted from or emitted to document JSON.
    #[doc(hidden)]
    Resolved,
}
impl StrokeScaleFixed {
    pub fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }
    pub fn is_active(self) -> bool {
        matches!(self, Self::Fixed | Self::Threshold(_))
    }
    pub(crate) fn validate(self) -> Result<(), String> {
        if let Self::Threshold(value) = self
            && (!value.is_finite() || value <= 0.)
        {
            return Err("scale-fixed threshold must be finite and positive".into());
        }
        Ok(())
    }
}
impl Serialize for StrokeScaleFixed {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(ser::Error::custom)?;
        match self {
            Self::Disabled => s.serialize_bool(false),
            Self::Fixed => s.serialize_bool(true),
            Self::Threshold(value) => s.serialize_f32(*value),
            Self::Resolved => Err(ser::Error::custom(
                "resolved stroke geometry is not document data",
            )),
        }
    }
}
impl<'de> Deserialize<'de> for StrokeScaleFixed {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Input {
            Bool(bool),
            Number(f32),
            Alias(String),
        }
        let result = match Input::deserialize(d)? {
            Input::Bool(false) | Input::Number(0.) => Self::Disabled,
            Input::Bool(true) => Self::Fixed,
            Input::Number(value) => Self::Threshold(value),
            Input::Alias(value) if value == "zoom-in" => Self::Threshold(1.),
            Input::Alias(_) => return Err(de::Error::custom("unknown scale-fixed policy")),
        };
        result.validate().map_err(de::Error::custom)?;
        Ok(result)
    }
}

/// Positive magnitudes accumulated from local orthogonal columns. Independent
/// from world-matrix singular values, camera placement and physical pixel scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeScaleData {
    pub x: f64,
    pub y: f64,
    // Admission uncertainty, not a tolerance used to select a width branch.
    // Matrix-only rotated columns cannot recover all authored f32 TRS scales.
    y_threshold_error: f64,
}
impl Default for StrokeScaleData {
    fn default() -> Self {
        Self::IDENTITY
    }
}
impl StrokeScaleData {
    pub const IDENTITY: Self = Self {
        x: 1.,
        y: 1.,
        y_threshold_error: 0.,
    };
    /// Trusted positive scale provenance, without matrix reconstruction. Exact
    /// above/equal/below threshold comparisons remain unchanged for this data.
    pub fn exact(x: f64, y: f64) -> Result<Self, String> {
        Self {
            x,
            y,
            y_threshold_error: 0.,
        }
        .with_camera(1.)
    }
    /// Conservative per-local-column admission band for ambiguous f32 rotated
    /// scale reconstruction. It never selects or smooths a threshold branch.
    pub const SCALE_RECONSTRUCTION_RELATIVE_ERROR: f64 = 8. * f32::EPSILON as f64;
    /// Eight f32 epsilons admit roundoff in authored TRS matrices. A materially
    /// sheared local matrix is rejected; sheared products of admitted locals work.
    pub const ORTHOGONAL_TOLERANCE: f64 = 8. * f32::EPSILON as f64;
    pub fn then(self, local: Affine) -> Result<Self, String> {
        let (a, b, c, d) = (
            local.a as f64,
            local.b as f64,
            local.c as f64,
            local.d as f64,
        );
        let (x, y) = (a.hypot(b), c.hypot(d));
        if !local.is_finite()
            || local.inverse().is_none()
            || x <= 0.
            || y <= 0.
            || (a * c + b * d).abs() > Self::ORTHOGONAL_TOLERANCE * x * y
        {
            return Err(
                "scale-fixed strokes require invertible orthogonal local transform columns".into(),
            );
        }
        // Canonical local magnitudes have the same precision as authored
        // matrix coefficients; then ancestry products use f64. In particular,
        // a quarter-turn's tiny cos residue cannot manufacture a Y > m test.
        let (x, y) = (x as f32 as f64, y as f32 as f64);
        // If hypot rounds to the dominant component, the other component has
        // no representable magnitude contribution (axis/quarter-turn case).
        // Otherwise retain uncertainty instead of guessing authored TRS scale.
        let local_y_error = if y == c.abs().max(d.abs()) {
            0.
        } else {
            y * Self::SCALE_RECONSTRUCTION_RELATIVE_ERROR
        };
        let result = Self {
            x: self.x * x,
            y: self.y * y,
            y_threshold_error: self.y_threshold_error * y
                + self.y * local_y_error
                + self.y_threshold_error * local_y_error,
        };
        result.with_camera(1.)
    }
    pub fn with_camera(self, camera: f32) -> Result<Self, String> {
        let result = Self {
            x: self.x * camera as f64,
            y: self.y * camera as f64,
            y_threshold_error: self.y_threshold_error * camera as f64,
        };
        if !camera.is_finite()
            || camera <= 0.
            || !result.x.is_finite()
            || !result.y.is_finite()
            || result.x <= 0.
            || result.y <= 0.
            || !result.y_threshold_error.is_finite()
            || result.y_threshold_error < 0.
        {
            return Err("scale-fixed stroke scale must be finite and positive".into());
        }
        Ok(result)
    }
}
impl Stroke {
    /// Pinned scalar-X policy. Only width changes: dash lengths and phase remain
    /// in authored local units, including the numeric policy's Y-only trigger.
    pub fn resolved(&self, world: StrokeScaleData, camera: f32) -> Result<Self, String> {
        self.scale_fixed.validate()?;
        if !self.scale_fixed.is_active() {
            return Ok(self.clone());
        }
        let scale = world.with_camera(camera)?;
        if let StrokeScaleFixed::Threshold(m) = self.scale_fixed
            && scale.x < m as f64
            && scale.y_threshold_error > 0.
            && scale.y - scale.y_threshold_error <= m as f64
            && scale.y + scale.y_threshold_error >= m as f64
        {
            return Err(
                "numeric scale-fixed Y threshold is ambiguous at rotated f32 scale precision"
                    .into(),
            );
        }
        let divisor = match self.scale_fixed {
            StrokeScaleFixed::Fixed => scale.x,
            StrokeScaleFixed::Threshold(m) if scale.x > m as f64 || scale.y > m as f64 => scale.x,
            StrokeScaleFixed::Threshold(m) => m as f64,
            _ => unreachable!(),
        };
        let width = (self.width as f64 / divisor) as f32;
        if !width.is_finite() || (self.width > 0. && width <= 0.) {
            return Err("resolved stroke width overflow or underflow".into());
        }
        let mut result = self.clone();
        result.width = width;
        result.scale_fixed = StrokeScaleFixed::Resolved;
        result.validate()?;
        Ok(result)
    }
}
impl DisplayObject {
    pub fn resolved_strokes(&self, world: StrokeScaleData, camera: f32) -> Result<Self, String> {
        let mut result = self.clone();
        for stroke in result
            .style
            .stroke
            .iter_mut()
            .chain(result.style.stroke_layers.iter_mut())
        {
            *stroke = stroke.resolved(world, camera)?;
        }
        Ok(result)
    }
}

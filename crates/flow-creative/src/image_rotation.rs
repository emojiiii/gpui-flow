//! Bounded custom-angle image rotation, separate from lossless crop/flip edits.
//!
//! Positive angles turn clockwise in image coordinates (x right, y down).
//! Rotation is about the image center. The output is centered on that same
//! point and expands to the ceiling of the rotated pixel-edge rectangle, so
//! the source corners are not silently cropped. Cardinal angles copy RGBA
//! channels exactly; other angles use premultiplied-alpha bilinear sampling.

use crate::processing::{MAX_PROCESS_IMAGE_PIXELS, ProcessingError};
use serde::{Deserialize, Serialize};

#[cfg(feature = "desktop")]
mod pixels;
#[cfg(feature = "desktop")]
pub use pixels::{rotate_preview, write_rotated};

pub const IMAGE_ROTATION_MAX_DEGREES: f64 = 360.0;

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageRotationSpec {
    /// Finite clockwise degrees in -360..=360; negative values turn left.
    pub degrees: f64,
}

impl ImageRotationSpec {
    pub fn validate(&self) -> Result<(), ProcessingError> {
        if !self.degrees.is_finite() || self.degrees.abs() > IMAGE_ROTATION_MAX_DEGREES {
            return Err(ProcessingError::InvalidSettings(
                "image rotation needs a finite angle between -360 and 360 degrees".into(),
            ));
        }
        Ok(())
    }

    /// Equivalent clockwise angle in [0, 360). This does not mutate the draft.
    pub fn normalized_degrees(&self) -> Result<f64, ProcessingError> {
        self.validate()?;
        let degrees = self.degrees.rem_euclid(360.0);
        // rem_euclid can round a tiny negative remainder up to the modulus.
        Ok(if degrees == 0.0 || degrees == 360.0 {
            0.0
        } else {
            degrees
        })
    }

    /// Input and expanded output must each fit 16384 per edge and 16 MP.
    /// Cardinal dimensions are exact, with no floating-point ceil padding.
    pub fn output_dimensions(
        &self,
        width: u32,
        height: u32,
    ) -> Result<(u32, u32), ProcessingError> {
        self.validate()?;
        validate_dimensions(width, height)?;
        let dimensions = if let Some(turns) = self.quarter_turns() {
            if turns.is_multiple_of(2) {
                (width, height)
            } else {
                (height, width)
            }
        } else {
            let (sin, cos) = self.degrees.to_radians().sin_cos();
            (
                (f64::from(width) * cos.abs() + f64::from(height) * sin.abs()).ceil() as u32,
                (f64::from(width) * sin.abs() + f64::from(height) * cos.abs()).ceil() as u32,
            )
        };
        validate_dimensions(dimensions.0, dimensions.1)?;
        Ok(dimensions)
    }

    // Call only after validation. Do not snap nearby custom angles to cardinals.
    fn quarter_turns(&self) -> Option<u8> {
        (self.degrees % 90.0 == 0.0).then(|| (self.degrees / 90.0).rem_euclid(4.0) as u8)
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), ProcessingError> {
    if width == 0
        || height == 0
        || width.max(height) > 16_384
        || u64::from(width) * u64::from(height) > MAX_PROCESS_IMAGE_PIXELS
    {
        return Err(ProcessingError::ResourceLimit(
            "image rotation source and expanded output must each fit 16384 per edge and 16 MP"
                .into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_bounded_angles_normalize_without_mutating() {
        for (degrees, normalized) in [
            (-360.0, 0.0),
            (-270.0, 90.0),
            (-45.5, 314.5),
            (-0.0, 0.0),
            (45.5, 45.5),
            (360.0, 0.0),
        ] {
            let spec = ImageRotationSpec { degrees };
            assert_eq!(spec.normalized_degrees().unwrap(), normalized);
            assert_eq!(spec.degrees.to_bits(), degrees.to_bits());
        }
        for degrees in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -360.1, 360.1] {
            let spec = ImageRotationSpec { degrees };
            assert!(matches!(
                spec.validate(),
                Err(ProcessingError::InvalidSettings(_))
            ));
            assert!(spec.normalized_degrees().is_err());
            assert!(spec.output_dimensions(2, 3).is_err());
        }
    }

    #[test]
    fn expanded_dimensions_are_centered_and_cardinals_stay_exact() {
        for (degrees, dimensions) in [
            (0.0, (2, 3)),
            (90.0, (3, 2)),
            (180.0, (2, 3)),
            (270.0, (3, 2)),
            (-90.0, (3, 2)),
            (-360.0, (2, 3)),
            (45.0, (4, 4)),
            (-45.0, (4, 4)),
            (30.0, (4, 4)),
        ] {
            assert_eq!(
                ImageRotationSpec { degrees }
                    .output_dimensions(2, 3)
                    .unwrap(),
                dimensions
            );
        }
        assert_eq!(
            ImageRotationSpec { degrees: 45.0 }
                .output_dimensions(768, 768)
                .unwrap(),
            (1087, 1087)
        );
        assert_eq!(
            ImageRotationSpec { degrees: 90.0 }
                .output_dimensions(16_384, 1)
                .unwrap(),
            (1, 16_384)
        );
        // Only exact cardinals bypass interpolation and bounding-box expansion.
        assert_eq!(
            ImageRotationSpec { degrees: 0.01 }
                .output_dimensions(2, 3)
                .unwrap(),
            (3, 4)
        );
    }

    #[test]
    fn source_and_output_budgets_are_checked_before_allocation() {
        let identity = ImageRotationSpec::default();
        assert_eq!(
            identity.output_dimensions(4096, 4096).unwrap(),
            (4096, 4096)
        );
        for (width, height) in [
            (0, 1),
            (1, 0),
            (16_385, 1),
            (4097, 4096),
            (u32::MAX, u32::MAX),
        ] {
            assert!(matches!(
                identity.output_dimensions(width, height),
                Err(ProcessingError::ResourceLimit(_))
            ));
        }
        assert!(matches!(
            ImageRotationSpec { degrees: 45.0 }.output_dimensions(4096, 4096),
            Err(ProcessingError::ResourceLimit(_))
        ));
        // Fits the pixel budget, but the expanded horizontal edge exceeds 16384.
        assert!(matches!(
            ImageRotationSpec { degrees: 0.001 }.output_dimensions(16_384, 1),
            Err(ProcessingError::ResourceLimit(_))
        ));
    }
}

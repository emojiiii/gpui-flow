//! Bounded local image adjustments, independent of canvas geometry and UI.
//!
//! The desktop implementation applies exposure, light, saturation, vibrance,
//! then vignette in linear sRGB. These are explicitly defined local controls,
//! not a claim of algorithmic equivalence to another image editor. Dimensions
//! and alpha stay unchanged; fully transparent pixels retain their hidden RGB.
//! Neutral settings copy every RGBA pixel byte without color conversion.

use crate::processing::{MAX_PROCESS_IMAGE_PIXELS, ProcessingError};
use serde::{Deserialize, Serialize};

#[cfg(feature = "desktop")]
mod pixels;
#[cfg(feature = "desktop")]
pub use pixels::{adjust_preview, write_adjusted};

pub const IMAGE_ADJUST_PREVIEW_EDGE: u32 = 768;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageAdjustSpec {
    /// Finite -1..=1, neutral 0. Blend linear RGB toward black or white.
    pub light: f32,
    /// Finite -4..=4 stops, neutral 0. Multiply linear RGB by 2^exposure.
    pub exposure: f32,
    /// Finite 0..=2, neutral 1. Scale distance from Rec.709 luminance.
    pub saturation: f32,
    /// Finite -1..=1, neutral 0. Adapt saturation to protect saturated colors.
    pub vibrance: f32,
    /// Finite 0..=1, neutral 0. Darken the elliptical periphery smoothly.
    pub vignette: f32,
}

impl Default for ImageAdjustSpec {
    fn default() -> Self {
        Self {
            light: 0.0,
            exposure: 0.0,
            saturation: 1.0,
            vibrance: 0.0,
            vignette: 0.0,
        }
    }
}

impl ImageAdjustSpec {
    pub fn validate(&self) -> Result<(), ProcessingError> {
        if ![
            (self.light, -1.0, 1.0),
            (self.exposure, -4.0, 4.0),
            (self.saturation, 0.0, 2.0),
            (self.vibrance, -1.0, 1.0),
            (self.vignette, 0.0, 1.0),
        ]
        .into_iter()
        .all(|(value, minimum, maximum)| value.is_finite() && (minimum..=maximum).contains(&value))
        {
            return Err(ProcessingError::InvalidSettings(
                "image adjustments need finite light/vibrance in -1..=1, exposure in -4..=4, saturation in 0..=2, and vignette in 0..=1".into(),
            ));
        }
        Ok(())
    }

    /// Exact neutral settings, including signed zero. No approximate snapping.
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// Adjustments never resize. Validate before allocating source-sized output.
    pub fn output_dimensions(
        &self,
        width: u32,
        height: u32,
    ) -> Result<(u32, u32), ProcessingError> {
        self.validate()?;
        if width == 0
            || height == 0
            || width.max(height) > 16_384
            || u64::from(width) * u64::from(height) > MAX_PROCESS_IMAGE_PIXELS
        {
            return Err(ProcessingError::ResourceLimit(
                "image adjustments accept positive dimensions up to 16384 per edge and 16 MP"
                    .into(),
            ));
        }
        Ok((width, height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_settings_roundtrip_without_approximate_identity() {
        let neutral = ImageAdjustSpec::default();
        assert!(neutral.validate().is_ok());
        assert!(neutral.is_identity());
        assert!(
            ImageAdjustSpec {
                light: -0.0,
                exposure: -0.0,
                vibrance: -0.0,
                vignette: -0.0,
                ..neutral
            }
            .is_identity()
        );
        assert!(
            !ImageAdjustSpec {
                exposure: f32::EPSILON,
                ..neutral
            }
            .is_identity()
        );
        let spec = ImageAdjustSpec {
            light: 0.25,
            exposure: -1.0,
            saturation: 1.5,
            vibrance: -0.5,
            vignette: 0.75,
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert_eq!(
            serde_json::from_str::<ImageAdjustSpec>(&json).unwrap(),
            spec
        );
        assert!(serde_json::from_str::<ImageAdjustSpec>(
            r#"{"light":0,"exposure":0,"saturation":1,"vibrance":0,"vignette":0,"path":"external"}"#
        )
        .is_err());
    }

    #[test]
    fn every_control_rejects_nonfinite_and_out_of_range_values() {
        for (field, minimum, maximum) in [
            (0, -1.0, 1.0),
            (1, -4.0, 4.0),
            (2, 0.0, 2.0),
            (3, -1.0, 1.0),
            (4, 0.0, 1.0),
        ] {
            for value in [
                minimum,
                maximum,
                minimum - 0.01,
                maximum + 0.01,
                f32::NAN,
                f32::INFINITY,
                f32::NEG_INFINITY,
            ] {
                let mut spec = ImageAdjustSpec::default();
                *match field {
                    0 => &mut spec.light,
                    1 => &mut spec.exposure,
                    2 => &mut spec.saturation,
                    3 => &mut spec.vibrance,
                    _ => &mut spec.vignette,
                } = value;
                if value == minimum || value == maximum {
                    assert!(spec.validate().is_ok());
                } else {
                    assert!(matches!(
                        spec.validate(),
                        Err(ProcessingError::InvalidSettings(_))
                    ));
                    assert!(spec.output_dimensions(1, 1).is_err());
                    assert!(!spec.is_identity());
                }
            }
        }
    }

    #[test]
    fn source_dimensions_are_preserved_within_the_pixel_budget() {
        let spec = ImageAdjustSpec::default();
        for dimensions in [(1, 1), (2, 3), (16_384, 1), (4096, 4096)] {
            assert_eq!(
                spec.output_dimensions(dimensions.0, dimensions.1).unwrap(),
                dimensions
            );
        }
        for (width, height) in [
            (0, 1),
            (1, 0),
            (16_385, 1),
            (4097, 4096),
            (u32::MAX, u32::MAX),
        ] {
            assert!(matches!(
                spec.output_dimensions(width, height),
                Err(ProcessingError::ResourceLimit(_))
            ));
        }
    }
}

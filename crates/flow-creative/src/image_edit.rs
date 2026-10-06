//! Lossless orthogonal image edits. Coordinates refer to the oriented image:
//! rotate the source clockwise, flip on the resulting axes, then crop.
//!
//! The specification is independent of GPUI and codecs. Desktop pixel helpers
//! operate on host-resolved project assets and never modify their source file.

use crate::processing::{MAX_PROCESS_IMAGE_PIXELS, ProcessingError};
use gpui_flow_core::{Point, Rect, Size};
use serde::{Deserialize, Serialize};

#[cfg(feature = "desktop")]
mod pixels;
#[cfg(feature = "desktop")]
pub use pixels::{load_preview, orient_preview, write_edited};

pub const IMAGE_EDIT_PREVIEW_EDGE: u32 = 768;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageEditSpec {
    /// Clockwise quarter turns, strictly 0..=3.
    pub quarter_turns: u8,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    /// Positive rectangle within [0, 1] on the rotated/flipped image.
    pub crop: Rect,
}

impl Default for ImageEditSpec {
    fn default() -> Self {
        Self {
            quarter_turns: 0,
            flip_horizontal: false,
            flip_vertical: false,
            crop: Rect::new(Point::ZERO, Size::new(1.0, 1.0)),
        }
    }
}

impl ImageEditSpec {
    pub fn validate(&self) -> Result<(), ProcessingError> {
        if self.quarter_turns > 3
            || !self.crop.is_valid()
            || !self.crop.size.is_positive()
            || self.crop.min_x() < 0.0
            || self.crop.min_y() < 0.0
            || self.crop.max_x() > 1.0
            || self.crop.max_y() > 1.0
        {
            return Err(ProcessingError::InvalidSettings(
                "image edit needs 0–3 quarter turns and a positive crop within the image".into(),
            ));
        }
        Ok(())
    }

    /// Input dimensions always refer to the unedited source asset.
    pub fn oriented_dimensions(
        &self,
        width: u32,
        height: u32,
    ) -> Result<(u32, u32), ProcessingError> {
        self.validate()?;
        validate_dimensions(width, height)?;
        Ok(if self.quarter_turns.is_multiple_of(2) {
            (width, height)
        } else {
            (height, width)
        })
    }

    /// Return (x, y, width, height) in the oriented image, given original source
    /// dimensions. Edges snap to the nearest whole pixel. Subpixel selections
    /// that contain no output pixels are rejected rather than silently enlarged.
    pub fn crop_pixels(
        &self,
        width: u32,
        height: u32,
    ) -> Result<(u32, u32, u32, u32), ProcessingError> {
        let (width, height) = self.oriented_dimensions(width, height)?;
        let edge = |value: f32, axis: u32| {
            (f64::from(value) * f64::from(axis))
                .round()
                .clamp(0.0, f64::from(axis)) as u32
        };
        let left = edge(self.crop.min_x(), width);
        let right = edge(self.crop.max_x(), width);
        let top = edge(self.crop.min_y(), height);
        let bottom = edge(self.crop.max_y(), height);
        if right <= left || bottom <= top {
            return Err(ProcessingError::InvalidSettings(
                "crop must contain at least one source pixel in each dimension".into(),
            ));
        }
        Ok((left, top, right - left, bottom - top))
    }

    /// Set an exact pixel size around the current crop center. Snap the origin
    /// to the source pixel grid first: rounding two half-pixel edges separately
    /// can otherwise lose one pixel on odd-sized images.
    pub fn resize_crop_pixels(
        &mut self,
        source_width: u32,
        source_height: u32,
        width: u32,
        height: u32,
    ) -> Result<(), ProcessingError> {
        let (sw, sh) = self.oriented_dimensions(source_width, source_height)?;
        if width == 0 || height == 0 || width > sw || height > sh {
            return Err(ProcessingError::InvalidSettings(
                "crop dimensions must be positive and fit the oriented source".into(),
            ));
        }
        let center = self.crop.center();
        let start = |c: f32, axis: u32, length: u32| {
            (f64::from(c) * f64::from(axis) - f64::from(length) / 2.)
                .round()
                .clamp(0., f64::from(axis - length)) as u32
        };
        let x = start(center.x, sw, width);
        let y = start(center.y, sh, height);
        let origin = Point::new(x as f32 / sw as f32, y as f32 / sh as f32);
        let mut next = *self;
        next.crop = Rect::new(
            origin,
            Size::new(
                (x + width) as f32 / sw as f32 - origin.x,
                (y + height) as f32 / sh as f32 - origin.y,
            ),
        );
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// Rotate the currently displayed image, keeping the selected source area.
    /// Reflections exchange axes because they are applied after the rotation.
    pub fn rotate_cw(&mut self) -> Result<(), ProcessingError> {
        self.validate()?;
        let mut next = *self;
        next.quarter_turns = (self.quarter_turns + 1) % 4;
        std::mem::swap(&mut next.flip_horizontal, &mut next.flip_vertical);
        next.crop = Rect::new(
            Point::new(1.0 - self.crop.max_y(), self.crop.min_x()),
            Size::new(self.crop.size.height, self.crop.size.width),
        );
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn rotate_ccw(&mut self) -> Result<(), ProcessingError> {
        self.validate()?;
        let mut next = *self;
        next.quarter_turns = (self.quarter_turns + 3) % 4;
        std::mem::swap(&mut next.flip_horizontal, &mut next.flip_vertical);
        next.crop = Rect::new(
            Point::new(self.crop.min_y(), 1.0 - self.crop.max_x()),
            Size::new(self.crop.size.height, self.crop.size.width),
        );
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn flip_horizontal(&mut self) -> Result<(), ProcessingError> {
        self.validate()?;
        let mut next = *self;
        next.flip_horizontal = !self.flip_horizontal;
        next.crop.origin.x = 1.0 - self.crop.max_x();
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn flip_vertical(&mut self) -> Result<(), ProcessingError> {
        self.validate()?;
        let mut next = *self;
        next.flip_vertical = !self.flip_vertical;
        next.crop.origin.y = 1.0 - self.crop.max_y();
        next.validate()?;
        *self = next;
        Ok(())
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), ProcessingError> {
    if width == 0
        || height == 0
        || width.max(height) > 16_384
        || u64::from(width) * u64::from(height) > MAX_PROCESS_IMAGE_PIXELS
    {
        return Err(ProcessingError::ResourceLimit(
            "image edits accept positive dimensions up to 16384 per edge and 16 MP".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_bounds_and_pixel_budget_are_enforced() {
        let spec = ImageEditSpec::default();
        assert_eq!(spec.crop_pixels(2, 3).unwrap(), (0, 0, 2, 3));
        assert_eq!(spec.crop_pixels(4096, 4096).unwrap(), (0, 0, 4096, 4096));
        for (width, height) in [
            (0, 1),
            (1, 0),
            (16_385, 1),
            (4097, 4096),
            (u32::MAX, u32::MAX),
        ] {
            assert!(spec.crop_pixels(width, height).is_err());
        }
        for crop in [
            Rect::new(Point::ZERO, Size::new(0.0, 1.0)),
            Rect::new(Point::new(-0.1, 0.0), Size::new(1.0, 1.0)),
            Rect::new(Point::new(0.5, 0.0), Size::new(0.6, 1.0)),
            Rect::new(Point::new(f32::NAN, 0.0), Size::new(1.0, 1.0)),
            Rect::new(Point::ZERO, Size::new(f32::INFINITY, 1.0)),
        ] {
            assert!(ImageEditSpec { crop, ..spec }.validate().is_err());
        }
        assert!(
            ImageEditSpec {
                quarter_turns: 4,
                ..spec
            }
            .validate()
            .is_err()
        );
        let tiny = ImageEditSpec {
            crop: Rect::new(Point::ZERO, Size::new(0.01, 1.0)),
            ..spec
        };
        assert!(tiny.crop_pixels(2, 3).is_err());
    }

    #[test]
    fn rotations_move_crop_and_exchange_reflection_axes() {
        let mut spec = ImageEditSpec {
            flip_horizontal: true,
            crop: Rect::new(Point::new(0.0, 0.0), Size::new(0.5, 2.0 / 3.0)),
            ..ImageEditSpec::default()
        };
        let before = spec;
        spec.rotate_cw().unwrap();
        assert_eq!(spec.quarter_turns, 1);
        assert!(!spec.flip_horizontal);
        assert!(spec.flip_vertical);
        assert_eq!(spec.oriented_dimensions(2, 3).unwrap(), (3, 2));
        assert_eq!(spec.crop_pixels(2, 3).unwrap(), (1, 0, 2, 1));
        spec.rotate_ccw().unwrap();
        assert_eq!(
            spec.crop_pixels(2, 3).unwrap(),
            before.crop_pixels(2, 3).unwrap()
        );
        assert_eq!(spec.quarter_turns, before.quarter_turns);
        assert_eq!(spec.flip_horizontal, before.flip_horizontal);
        assert_eq!(spec.flip_vertical, before.flip_vertical);
        spec.flip_horizontal().unwrap();
        assert_eq!(spec.crop_pixels(2, 3).unwrap(), (1, 0, 1, 2));
        spec.flip_vertical().unwrap();
        assert_eq!(spec.crop_pixels(2, 3).unwrap(), (1, 1, 1, 2));
    }

    #[test]
    fn exact_dimensions_survive_odd_sources_orientation_and_edge_clamping() {
        for turns in 0..4 {
            let mut spec = ImageEditSpec {
                quarter_turns: turns,
                ..ImageEditSpec::default()
            };
            spec.resize_crop_pixels(2513, 2011, 2000, 1400).unwrap();
            let (_, _, w, h) = spec.crop_pixels(2513, 2011).unwrap();
            assert_eq!((w, h), (2000, 1400));
            spec.flip_horizontal().unwrap();
            spec.rotate_cw().unwrap();
            let (_, _, w, h) = spec.crop_pixels(2513, 2011).unwrap();
            assert_eq!((w, h), (1400, 2000));
            spec.resize_crop_pixels(2513, 2011, 1, 1).unwrap();
            let (sw, sh) = spec.oriented_dimensions(2513, 2011).unwrap();
            spec.resize_crop_pixels(2513, 2011, sw, sh).unwrap();
            assert_eq!(spec.crop_pixels(2513, 2011).unwrap(), (0, 0, sw, sh));
            let before = spec;
            assert!(spec.resize_crop_pixels(2513, 2011, sw + 1, sh).is_err());
            assert_eq!(spec, before);
        }
    }

    #[test]
    fn invalid_edits_do_not_mutate_the_specification() {
        let mut spec = ImageEditSpec {
            quarter_turns: 255,
            ..ImageEditSpec::default()
        };
        let before = spec;
        assert!(spec.rotate_cw().is_err());
        assert!(spec.rotate_ccw().is_err());
        assert!(spec.flip_horizontal().is_err());
        assert!(spec.flip_vertical().is_err());
        assert_eq!(spec, before);
    }
}

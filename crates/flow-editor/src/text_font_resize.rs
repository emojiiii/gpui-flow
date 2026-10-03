//! Intrinsic font-size resizing with the same signed handles and fixed anchor.
use crate::{MAX_TEXT_BOX_DIMENSION, ResizeHandle, ResizeOptions, resize_transform_with_options};
use gpui_flow_scene::{Affine, Bounds, Point, Size, display::Text};

pub const MAX_TEXT_RESIZE_FONT_SIZE: f32 = 512.;
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextFontResizeGeometry {
    size: Size,
    origin: Point,
    padding: [f32; 4],
    font_size: f32,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextFontResize {
    pub size: Size,
    pub padding: [f32; 4],
    pub font_size: f32,
    pub transform: Affine,
}
impl TextFontResizeGeometry {
    pub fn new(text: &Text) -> Result<Self, String> {
        text.validate()?;
        if text.font_size > MAX_TEXT_RESIZE_FONT_SIZE {
            return Err("font resize exceeds its 512 logical-unit ceiling".into());
        }
        Ok(Self {
            size: text.size,
            origin: text.layout_origin,
            padding: text.padding,
            font_size: text.font_size,
        })
    }
    /// Top/bottom use Y; all other named handles use X, matching the fixed
    /// Leafer font-size scaler. This mode is uniform regardless of Shift.
    /// Box and padding scale once; Flow's dimensionless line-height stays fixed.
    pub fn resize(
        self,
        original: Affine,
        start: Point,
        current: Point,
        mut options: ResizeOptions,
    ) -> Option<TextFontResize> {
        original.inverse()?;
        options.uniform = false;
        let bounds = Bounds::new(self.origin, self.size);
        let delta =
            resize_transform_with_options(Affine::IDENTITY, bounds, start, current, options)?;
        let factor = if matches!(options.handle, ResizeHandle::Top | ResizeHandle::Bottom) {
            delta.d
        } else {
            delta.a
        };
        let font_size = self.font_size * factor;
        let size = Size::new(self.size.width * factor, self.size.height * factor);
        let padding = self.padding.map(|v| v * factor);
        if !font_size.is_finite()
            || font_size <= 0.
            || font_size > MAX_TEXT_RESIZE_FONT_SIZE
            || !size.is_positive()
            || size.width > MAX_TEXT_BOX_DIMENSION
            || size.height > MAX_TEXT_BOX_DIMENSION
            || padding.iter().any(|v| !v.is_finite() || *v < 0.)
            || size.width as f64 <= padding[1] as f64 + padding[3] as f64
            || size.height as f64 <= padding[0] as f64 + padding[2] as f64
        {
            return None;
        }
        let old_anchor = options.handle.anchor(bounds);
        let new_anchor = options.handle.anchor(Bounds::new(self.origin, size));
        let transform = if factor == 1. {
            original
        } else {
            original * Affine::translation(old_anchor.x - new_anchor.x, old_anchor.y - new_anchor.y)
        };
        transform.inverse()?;
        Some(TextFontResize {
            size,
            padding,
            font_size,
            transform,
        })
    }
}

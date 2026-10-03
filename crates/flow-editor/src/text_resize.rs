//! Fixed Text-box resizing; font and affine linear terms are never scaled.
use crate::{ResizeOptions, resize_transform_with_options};
use gpui_flow_scene::{Affine, Bounds, Point, Size, display::Text};
/// Local box dimensions above this ceiling are rejected by this resize API.
pub const MAX_TEXT_BOX_DIMENSION: f32 = 1_000_000.;
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBoxResizeGeometry {
    size: Size,
    origin: Point,
    padding: [f32; 4],
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBoxResize {
    pub size: Size,
    pub transform: Affine,
}
impl TextBoxResizeGeometry {
    pub fn new(text: &Text) -> Result<Self, String> {
        text.validate()?;
        Ok(Self {
            size: text.size,
            origin: text.layout_origin,
            padding: text.padding,
        })
    }
    /// Opposite local corner/edge midpoint is stationary in parent/world space.
    /// Preserve at least one content unit, or an already-smaller initial box.
    pub fn resize(
        self,
        original: Affine,
        start: Point,
        current: Point,
        mut options: ResizeOptions,
    ) -> Option<TextBoxResize> {
        original.inverse()?;
        if !options.minimum_scale.is_finite()
            || !(0.05..=1.).contains(&options.minimum_scale.x)
            || !(0.05..=1.).contains(&options.minimum_scale.y)
        {
            return None;
        }
        let [top, right, bottom, left] = self.padding;
        let floor = |a: f32, b: f32, dimension: f32| {
            (((a as f64 + b as f64 + 1.) / dimension as f64).clamp(0.05, 1.)) as f32
        };
        options.minimum_scale.x = options
            .minimum_scale
            .x
            .max(floor(left, right, self.size.width));
        options.minimum_scale.y = options
            .minimum_scale
            .y
            .max(floor(top, bottom, self.size.height));
        let delta = resize_transform_with_options(
            Affine::IDENTITY,
            Bounds::new(self.origin, self.size),
            start,
            current,
            options,
        )?;
        let size = Size::new(
            (self.size.width as f64 * delta.a as f64) as f32,
            (self.size.height as f64 * delta.d as f64) as f32,
        );
        if !size.is_positive()
            || size.width > MAX_TEXT_BOX_DIMENSION
            || size.height > MAX_TEXT_BOX_DIMENSION
            || size.width as f64 <= left as f64 + right as f64
            || size.height as f64 <= top as f64 + bottom as f64
        {
            return None;
        }
        let transform = if delta == Affine::IDENTITY {
            original
        } else {
            let moved = delta.transform_point(self.origin);
            original * Affine::translation(moved.x - self.origin.x, moved.y - self.origin.y)
        };
        transform.inverse()?;
        Some(TextBoxResize { size, transform })
    }
}

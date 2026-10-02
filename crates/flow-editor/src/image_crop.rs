//! Fixed normalized source-frame editing. The image box and transform never change.
use crate::ResizeHandle;
use gpui_flow_scene::{Bounds, Point, Size, display::Image};

/// Frame body pans the selection; eight edges/corners resize it in source axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageCropPart {
    Move,
    Handle(ResizeHandle),
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageCropGeometry {
    pub crop: Bounds,
    pub preview_size: Size,
}
impl ImageCropGeometry {
    pub fn new(image: &Image) -> Result<Self, String> {
        image.validate()?;
        Ok(Self {
            crop: image.normalized_crop(),
            preview_size: image.size,
        })
    }
    pub fn frame(self) -> Bounds {
        Bounds::new(
            Point::new(
                self.crop.origin.x * self.preview_size.width,
                self.crop.origin.y * self.preview_size.height,
            ),
            Size::new(
                self.crop.size.width * self.preview_size.width,
                self.crop.size.height * self.preview_size.height,
            ),
        )
    }
    /// Frozen-local delta avoids jumping on off-center presses. Clamp to the
    /// full source and a 1/4096 minimum axis (or an already-smaller initial axis).
    /// Shift locks a corner's initial ratio; body motion uses its dominant axis.
    pub fn drag(
        self,
        part: ImageCropPart,
        start: Point,
        current: Point,
        shift: bool,
    ) -> Option<Bounds> {
        if !start.is_finite() || !current.is_finite() {
            return None;
        }
        if start == current {
            return Some(self.crop);
        }
        let dx = (current.x as f64 - start.x as f64) / self.preview_size.width as f64;
        let dy = (current.y as f64 - start.y as f64) / self.preview_size.height as f64;
        let (x, y, w, h) = (
            self.crop.origin.x as f64,
            self.crop.origin.y as f64,
            self.crop.size.width as f64,
            self.crop.size.height as f64,
        );
        let (nx, ny, nw, nh) = match part {
            ImageCropPart::Move => {
                let (dx, dy) = if shift {
                    if dx.abs() > dy.abs() {
                        (dx, 0.)
                    } else {
                        (0., dy)
                    }
                } else {
                    (dx, dy)
                };
                ((x + dx).clamp(0., 1. - w), (y + dy).clamp(0., 1. - h), w, h)
            }
            ImageCropPart::Handle(handle) => {
                use ResizeHandle::*;
                let left = matches!(handle, TopLeft | Left | BottomLeft);
                let right = matches!(handle, TopRight | Right | BottomRight);
                let top = matches!(handle, TopLeft | Top | TopRight);
                let bottom = matches!(handle, BottomLeft | Bottom | BottomRight);
                let minw = w.min(1. / 4096.);
                let minh = h.min(1. / 4096.);
                let mut x0 = if left {
                    (x + dx).clamp(0., x + w - minw)
                } else {
                    x
                };
                let mut x1 = if right {
                    (x + w + dx).clamp(x + minw, 1.)
                } else {
                    x + w
                };
                let mut y0 = if top {
                    (y + dy).clamp(0., y + h - minh)
                } else {
                    y
                };
                let mut y1 = if bottom {
                    (y + h + dy).clamp(y + minh, 1.)
                } else {
                    y + h
                };
                if shift && (left || right) && (top || bottom) {
                    let scale = ((x1 - x0) / w)
                        .min((y1 - y0) / h)
                        .max((minw / w).max(minh / h));
                    if left {
                        x0 = x + w - w * scale
                    } else {
                        x1 = x + w * scale
                    };
                    if top {
                        y0 = y + h - h * scale
                    } else {
                        y1 = y + h * scale
                    };
                }
                (x0, y0, x1 - x0, y1 - y0)
            }
        };
        // Rounding independently can push x+width just beyond 1; constrain
        // the represented f32 width to its exactly representable remainder.
        let x = nx as f32;
        let y = ny as f32;
        let crop = Bounds::new(
            Point::new(x, y),
            Size::new((nw as f32).min(1. - x), (nh as f32).min(1. - y)),
        );
        Image::validate_source_crop(crop).ok()?;
        Some(crop)
    }
}

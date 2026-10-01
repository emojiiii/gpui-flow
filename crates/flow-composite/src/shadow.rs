//! Admission parameters for the locked image 0.25.10 A8 Gaussian filter.
//! The filter's own allocation failure is not recoverable, but every known
//! explicit allocation and convolution visit is bounded before calling it.
use crate::CompositeError;
use gpui_flow_scene::{Affine, DisplayId, Shadow};

#[derive(Clone, Copy)]
pub(crate) struct PreparedShadow {
    pub effect: Shadow,
    pub world: Affine,
    pub ink_bounds: [f64; 4],
}

pub(crate) fn similarity_scale(a: Affine) -> Result<f64, CompositeError> {
    let x = (a.a as f64).hypot(a.b as f64);
    let y = (a.c as f64).hypot(a.d as f64);
    let dot = a.a as f64 * a.c as f64 + a.b as f64 * a.d as f64;
    if !a.is_finite()
        || x <= 0.
        || y <= 0.
        || a.determinant() <= 0.
        || (x - y).abs() > x.max(y) * 0.00001
        || dot.abs() > x * y * 0.00001
    {
        return Err(CompositeError::UnsupportedTransform);
    }
    Ok(x)
}

pub(crate) struct FilterPlan {
    pub sigma: f32,
    pub halo: f64,
    pub offset: [f64; 2],
    /// Conservative kernel width, not a private image API clone.
    kernel: u64,
}
impl FilterPlan {
    pub fn new(shadow: PreparedShadow, world_to_layer: Affine) -> Result<Self, CompositeError> {
        let matrix = world_to_layer * shadow.world;
        let scale = similarity_scale(matrix)?;
        let sigma = shadow.effect.blur as f64 * scale * 0.5;
        // Prevent Gaussian exponent/normalization underflow, huge kernels and
        // integer narrowing. Zero is a real no-filter branch, not image's 0.8σ.
        if !sigma.is_finite() || sigma > 64. || (sigma != 0. && sigma < 0.001) {
            return Err(CompositeError::TooLarge);
        }
        let dx =
            matrix.a as f64 * shadow.effect.x as f64 + matrix.c as f64 * shadow.effect.y as f64;
        let dy =
            matrix.b as f64 * shadow.effect.x as f64 + matrix.d as f64 * shadow.effect.y as f64;
        if !dx.is_finite() || !dy.is_finite() || dx.abs() > 8192. || dy.abs() > 8192. {
            return Err(CompositeError::TooLarge);
        }
        // image's actual radius is at most ceil(4σ), with minimum one.
        // Two extra transparent pixels prevent clamp/bilinear edge extension.
        let halo = (4. * sigma).ceil() + 2.;
        Ok(Self {
            sigma: sigma as f32,
            halo,
            offset: [dx, dy],
            kernel: (2. * halo + 1.) as u64,
        })
    }
    /// Source RGBA + A8 input/output + image's float intermediates, row/ring or
    /// full-plane scratch, padding, kernels and row-reference tables. Deliberate
    /// overestimate covers either internal filter path in locked image 0.25.10.
    pub fn bytes(&self, pixels: u64, width: u32) -> u64 {
        if self.sigma == 0. {
            return pixels * 5;
        }
        pixels * 18 + 16 * width as u64 * self.kernel + 128 * self.kernel + 32 * width as u64
    }
    /// Charge convolution taps, conversions, alpha extraction, tint and scratch
    /// initialization. Cancellation is checked before and after the bounded,
    /// non-interruptible library filter, not within each convolution tap.
    pub fn work(&self, pixels: u64) -> u64 {
        pixels
            * if self.sigma == 0. {
                12
            } else {
                4 * self.kernel + 24
            }
    }
}

pub(crate) fn validate_shape(
    id: &DisplayId,
    shape: &gpui_flow_scene::Shape,
) -> Result<(), CompositeError> {
    if shape.allows_children() || matches!(shape, gpui_flow_scene::Shape::Custom(_)) {
        return Err(CompositeError::UnsupportedShadow(id.clone()));
    }
    Ok(())
}

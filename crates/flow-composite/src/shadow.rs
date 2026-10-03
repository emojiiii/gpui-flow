//! Admission parameters for the locked image 0.25.10 A8 Gaussian filter.
//! The filter's own allocation failure is not recoverable, but every known
//! explicit allocation and convolution visit is bounded before calling it.
use crate::CompositeError;
use gpui_flow_scene::{Affine, Bounds, DisplayId, Shadow};

#[derive(Clone, Copy)]
pub(crate) struct PreparedShadow {
    pub effect: Shadow,
    pub world: Affine,
    pub ink_bounds: Option<[f64; 4]>,
    pub reference: Option<Bounds>,
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

#[derive(Clone)]
pub(crate) struct FilterPlan {
    pub sigma: f32,
    pub halo: f64,
    pub offset: [f64; 2],
    /// Conservative kernel width, not a private image API clone.
    kernel: u64,
}
impl FilterPlan {
    pub fn new(shadow: PreparedShadow, world_to_layer: Affine) -> Result<Self, CompositeError> {
        Self::matrix(shadow, world_to_layer * shadow.world)
    }
    pub fn matrix(shadow: PreparedShadow, matrix: Affine) -> Result<Self, CompositeError> {
        let scale = similarity_scale(matrix)?;
        Self::parameters(
            shadow,
            [
                matrix.a as f64,
                matrix.b as f64,
                matrix.c as f64,
                matrix.d as f64,
            ],
            scale,
            scale,
        )
    }
    fn parameters(
        shadow: PreparedShadow,
        linear: [f64; 4],
        scale: f64,
        logical_scale: f64,
    ) -> Result<Self, CompositeError> {
        let factor = if shadow.effect.scale_fixed {
            1. / logical_scale.max(1.)
        } else {
            1.
        };
        let resolved_sigma = shadow.effect.blur as f64 * scale * factor * 0.5;
        // Preserve the original f64 admission range before narrowing. A positive
        // authored blur may never underflow into the genuine zero-blur bypass.
        if !resolved_sigma.is_finite()
            || resolved_sigma > 64.
            || (shadow.effect.blur != 0. && resolved_sigma < 0.001)
        {
            return Err(CompositeError::TooLarge);
        }
        // Support and the exact one-tap predicate use the actual image input.
        let sigma = resolved_sigma as f32;
        let dx = (linear[0] * shadow.effect.x as f64 + linear[2] * shadow.effect.y as f64) * factor;
        let dy = (linear[1] * shadow.effect.x as f64 + linear[3] * shadow.effect.y as f64) * factor;
        if !dx.is_finite() || !dy.is_finite() || dx.abs() > 8192. || dy.abs() > 8192. {
            return Err(CompositeError::TooLarge);
        }
        // image's actual radius is at most ceil(4σ), with minimum one.
        // Two extra transparent pixels prevent clamp/bilinear edge extension.
        let halo = (4. * f64::from(sigma)).ceil() + 2.;
        Ok(Self {
            sigma,
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
            + if self.sigma == 0. {
                0
            } else {
                64 * self.kernel * self.kernel
            }
    }
}

pub(crate) fn validate_shape(
    id: &DisplayId,
    shape: &gpui_flow_scene::Shape,
) -> Result<(), CompositeError> {
    if matches!(shape, gpui_flow_scene::Shape::Custom(_)) {
        return Err(CompositeError::UnsupportedShadow(id.clone()));
    }
    Ok(())
}

pub(crate) fn validate_inner_shape(
    id: &DisplayId,
    shape: &gpui_flow_scene::Shape,
) -> Result<(), CompositeError> {
    if matches!(shape, gpui_flow_scene::Shape::Custom(_)) {
        return Err(CompositeError::UnsupportedInnerShadow(id.clone()));
    }
    Ok(())
}

/// The same checked source extent is used in recursive admission and painting.
/// It includes descendant effects but excludes this node's own inner/outer effects.
#[derive(Clone)]
pub(crate) struct SourcePlan {
    pub filter: FilterPlan,
    pub origin: [f64; 2],
    pub width: u32,
    pub height: u32,
    pub pixels: u64,
    pub world_to_layer: Affine,
    pub mode: Option<ModePlan>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpreadMode {
    Identity,
    Resample,
    Collapsed,
}

#[derive(Clone)]
pub(crate) struct ModePlan {
    pub kind: SpreadMode,
    pub center: [f64; 2],
    pub factors: [f64; 2],
    pub transformed: Option<crate::blur::SurfacePlan>,
    pub output: Option<crate::blur::SurfacePlan>,
}
impl SourcePlan {
    pub fn bytes(&self) -> u64 {
        match &self.mode {
            None => self.filter.bytes(self.pixels, self.width),
            Some(mode) if mode.output.is_none() => 0,
            Some(mode) => {
                5 * self.pixels
                    + mode
                        .transformed
                        .as_ref()
                        .map_or(0, |s| self.filter.bytes(s.pixels, s.width))
                    + mode.output.as_ref().map_or(0, |s| 4 * s.pixels)
            }
        }
    }
    pub fn work(&self, inner: bool) -> u64 {
        match &self.mode {
            None => self.filter.work(self.pixels) + if inner { self.pixels * 2 } else { 0 },
            Some(mode) if mode.output.is_none() => 0,
            Some(mode) => {
                // Original zero/extraction, transformed zero/resampling,
                // destination sampling/tint/zeroing; recursive paints and the
                // final destination composite are separately replayed.
                12 * self.pixels
                    + mode
                        .transformed
                        .as_ref()
                        .map_or(0, |s| self.filter.work(s.pixels) + 16 * s.pixels)
                    + mode.output.as_ref().map_or(0, |s| 16 * s.pixels)
            }
        }
    }
    fn modes(
        shadow: PreparedShadow,
        a: crate::blur::PhysicalSpace,
        original: [f64; 4],
        limits: crate::CompositeLimits,
        inner: bool,
        logical_scale: f64,
        filter: FilterPlan,
    ) -> Result<Self, CompositeError> {
        use crate::blur::SurfacePlan;
        let capture = SurfacePlan::new(
            [
                original[0] - 1.,
                original[1] - 1.,
                original[2] + 1.,
                original[3] + 1.,
            ],
            a,
            limits,
        )?;
        let mut center = [0., 0.];
        let mut factors = [1., 1.];
        if shadow.effect.spread != 0. {
            let reference = shadow
                .reference
                .filter(|r| r.is_valid() && r.size.is_positive())
                .ok_or(CompositeError::InvalidShadowReference)?;
            let m = a.local(shadow.world, [0., 0.]);
            let r = crate::raster::transformed(
                [
                    reference.min_x() as f64,
                    reference.min_y() as f64,
                    reference.max_x() as f64,
                    reference.max_y() as f64,
                ],
                m,
            );
            center = [(r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5];
            let compensation = if shadow.effect.scale_fixed {
                1. / logical_scale.max(1.)
            } else {
                1.
            };
            let spread = shadow.effect.spread as f64 * compensation * if inner { -2. } else { 2. };
            factors = [
                1. + spread / reference.size.width as f64,
                1. + spread / reference.size.height as f64,
            ];
        }
        if center.iter().chain(factors.iter()).any(|n| !n.is_finite()) {
            return Err(CompositeError::TooLarge);
        }
        let kind = if factors.contains(&0.) {
            SpreadMode::Collapsed
        } else if factors == [1., 1.] {
            SpreadMode::Identity
        } else {
            SpreadMode::Resample
        };
        let transformed = if kind == SpreadMode::Collapsed {
            None
        } else {
            let b = capture.rect();
            let x = [
                center[0] + (b[0] - center[0]) * factors[0],
                center[0] + (b[2] - center[0]) * factors[0],
            ];
            let y = [
                center[1] + (b[1] - center[1]) * factors[1],
                center[1] + (b[3] - center[1]) * factors[1],
            ];
            if x.iter().chain(y.iter()).any(|n| !n.is_finite()) {
                return Err(CompositeError::TooLarge);
            }
            let rect = [
                x[0].min(x[1]).floor() - filter.halo,
                y[0].min(y[1]).floor() - filter.halo,
                x[0].max(x[1]).ceil() + filter.halo,
                y[0].max(y[1]).ceil() + filter.halo,
            ];
            // Inverse mapping is never applied to a singular or overflowing
            // coefficient. Large finite outside samples remain transparent.
            for axis in 0..2 {
                for q in [rect[axis], rect[axis + 2]] {
                    if !(center[axis] + (q - center[axis]) / factors[axis]).is_finite() {
                        return Err(CompositeError::TooLarge);
                    }
                }
            }
            Some(SurfacePlan::new(rect, a, limits)?)
        };
        let output = if inner {
            Some(capture.clone())
        } else {
            transformed
                .as_ref()
                .map(|s| {
                    let b = s.rect();
                    SurfacePlan::new(
                        [
                            (b[0] + filter.offset[0]).floor() - 1.,
                            (b[1] + filter.offset[1]).floor() - 1.,
                            (b[2] + filter.offset[0]).ceil() + 1.,
                            (b[3] + filter.offset[1]).ceil() + 1.,
                        ],
                        a,
                        limits,
                    )
                })
                .transpose()?
        };
        Ok(Self {
            filter,
            origin: capture.origin,
            width: capture.width,
            height: capture.height,
            pixels: capture.pixels,
            world_to_layer: capture.world_to_layer,
            mode: Some(ModePlan {
                kind,
                center,
                factors,
                transformed,
                output,
            }),
        })
    }
    pub fn physical(
        shadow: PreparedShadow,
        a: crate::blur::PhysicalSpace,
        original: [f64; 4],
        limits: crate::CompositeLimits,
        inner: bool,
    ) -> Result<Self, CompositeError> {
        if shadow.effect.spread != 0.
            || shadow.effect.scale_fixed
            || (!inner && shadow.effect.r#box)
        {
            // Active modes retain composed f64 parameters and an explicitly
            // logical camera. Packed export/Canvas DPR never enters the cap.
            similarity_scale(a.local(shadow.world, [0., 0.]))?;
            let logical_linear = crate::blur::PhysicalSpace {
                camera: a.logical_camera,
                ..a
            }
            .linear(shadow.world);
            let logical_scale = logical_linear[0].hypot(logical_linear[1]);
            let physical_linear = a.linear(shadow.world).map(|x| x * a.scale);
            let filter = FilterPlan::parameters(
                shadow,
                physical_linear,
                physical_linear[0].hypot(physical_linear[1]),
                logical_scale,
            )?;
            return Self::modes(shadow, a, original, limits, inner, logical_scale, filter);
        }
        // Mode-default shadows retain their legacy physical-matrix narrowing.
        let filter = FilterPlan::matrix(shadow, a.local(shadow.world, [0., 0.]))?;
        let halo = filter.halo;
        let surface = crate::blur::SurfacePlan::new(
            [
                original[0] - halo,
                original[1] - halo,
                original[2] + halo,
                original[3] + halo,
            ],
            a,
            limits,
        )?;
        Ok(Self {
            filter,
            origin: surface.origin,
            width: surface.width,
            height: surface.height,
            pixels: surface.pixels,
            world_to_layer: surface.world_to_layer,
            mode: None,
        })
    }

    pub fn new(
        shadow: PreparedShadow,
        world_to_layer: Affine,
        limits: crate::CompositeLimits,
    ) -> Result<Self, CompositeError> {
        let filter = FilterPlan::new(shadow, world_to_layer)?;
        let ink = crate::raster::transformed(
            shadow.ink_bounds.ok_or_else(|| {
                CompositeError::InvalidGeometry("empty legacy shadow source".into())
            })?,
            world_to_layer,
        );
        let x0 = ink[0].floor() - filter.halo;
        let y0 = ink[1].floor() - filter.halo;
        let w = ink[2].ceil() + filter.halo - x0;
        let h = ink[3].ceil() + filter.halo - y0;
        if [x0, y0, w, h].iter().any(|n| !n.is_finite())
            || w <= 0.
            || h <= 0.
            || w > limits.max_dimension as f64
            || h > limits.max_dimension as f64
            || w * h > limits.max_pixels as f64
            || x0.abs() > 1_000_000.
            || y0.abs() > 1_000_000.
        {
            return Err(CompositeError::TooLarge);
        }
        let (width, height) = (w as u32, h as u32);
        Ok(Self {
            filter,
            origin: [x0, y0],
            width,
            height,
            pixels: width as u64 * height as u64,
            world_to_layer: Affine::translation(-x0 as f32, -y0 as f32) * world_to_layer,
            mode: None,
        })
    }
}

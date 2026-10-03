//! Full-source foreground filtering on one absolute physical-pixel lattice.
//! Used only when the prepared domain contains positive foreground blur; the
//! zero-blur path retains its existing surfaces, bounds and byte rounding.
use crate::{CompositeError, CompositeLimits, shadow, vector::PreparedNode};
use gpui_flow_scene::{Affine, EraserMode};
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) type Rect = [f64; 4];

/// Preserve camera and DPR provenance until local transforms cancel large
/// world translations. Subtract each integer origin only after composition.
#[derive(Clone, Copy)]
pub(crate) struct PhysicalSpace {
    pub camera: Affine,
    pub scale: f64,
}
impl PhysicalSpace {
    pub fn local(self, world: Affine, origin: [f64; 2]) -> Affine {
        let a = self.camera;
        let scale = self.scale;
        Affine::new(
            ((a.a as f64 * world.a as f64 + a.c as f64 * world.b as f64) * scale) as f32,
            ((a.b as f64 * world.a as f64 + a.d as f64 * world.b as f64) * scale) as f32,
            ((a.a as f64 * world.c as f64 + a.c as f64 * world.d as f64) * scale) as f32,
            ((a.b as f64 * world.c as f64 + a.d as f64 * world.d as f64) * scale) as f32,
            ((a.a as f64 * world.tx as f64 + a.c as f64 * world.ty as f64 + a.tx as f64) * scale
                - origin[0]) as f32,
            ((a.b as f64 * world.tx as f64 + a.d as f64 * world.ty as f64 + a.ty as f64) * scale
                - origin[1]) as f32,
        )
    }
}

#[derive(Clone)]
pub(crate) struct SurfacePlan {
    pub origin: [f64; 2],
    pub width: u32,
    pub height: u32,
    pub pixels: u64,
    pub world_to_layer: Affine,
}
impl SurfacePlan {
    pub fn new(
        rect: Rect,
        a: PhysicalSpace,
        limits: CompositeLimits,
    ) -> Result<Self, CompositeError> {
        let width = rect[2] - rect[0];
        let height = rect[3] - rect[1];
        if rect
            .iter()
            .any(|n| !n.is_finite() || n.abs() > 1_000_000. || n.fract() != 0.)
            || width <= 0.
            || height <= 0.
            || width > limits.max_dimension as f64
            || height > limits.max_dimension as f64
            || width * height > limits.max_pixels as f64
        {
            return Err(CompositeError::TooLarge);
        }
        let world_to_layer = a.local(Affine::IDENTITY, [rect[0], rect[1]]);
        if world_to_layer.inverse().is_none() {
            return Err(CompositeError::UnsupportedTransform);
        }
        Ok(Self {
            origin: [rect[0], rect[1]],
            width: width as u32,
            height: height as u32,
            pixels: width as u64 * height as u64,
            world_to_layer,
        })
    }
    pub fn rect(&self) -> Rect {
        [
            self.origin[0],
            self.origin[1],
            self.origin[0] + self.width as f64,
            self.origin[1] + self.height as f64,
        ]
    }
}
#[derive(Clone)]
pub(crate) struct BlurPlan {
    pub surface: SurfacePlan,
    pub sigma: f32,
    kernel: u64,
}
impl BlurPlan {
    pub fn parameters(amount: f32, matrix: Affine) -> Result<(f32, f64, u64), CompositeError> {
        let sigma = (amount as f64 * shadow::similarity_scale(matrix)?) as f32;
        if !sigma.is_finite() || !(0.001..=64.).contains(&sigma) {
            return Err(CompositeError::TooLarge);
        }
        let halo = (4. * f64::from(sigma)).ceil() + 2.;
        Ok((sigma, halo, (2. * halo + 1.) as u64))
    }
    /// Locked image 0.25.10: RGBA source/output (8P), two f32 RGBA
    /// conversion planes (32P), full-plane alternative (16P), ring/padding,
    /// row arena, simultaneous kernels and 64-bit row-reference tables.
    /// Source ownership transfers into image and output transfers into Pixmap.
    pub fn bytes(&self) -> u64 {
        56 * self.surface.pixels
            + 32 * self.surface.width as u64 * self.kernel
            + 32 * self.surface.width as u64
            + 128 * self.kernel
    }
    /// Four-channel taps, warmup rows, conversions/initialization, row tables,
    /// premultiplication checks and worst-case symmetric-kernel trimming.
    pub fn work(&self) -> u64 {
        self.surface.pixels * (16 * self.kernel + 96) + 64 * self.kernel * self.kernel
    }
}
#[derive(Default)]
pub(crate) struct DeviceNode {
    pub completed: Option<Rect>,
    pub blur: Option<BlurPlan>,
    pub shadows: Vec<shadow::SourcePlan>,
    pub inner_shadows: Vec<shadow::SourcePlan>,
}
fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some([
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]),
        (a, b) => a.or(b),
    }
}
fn rounded(b: Rect) -> Result<Rect, CompositeError> {
    if b.iter().any(|x| !x.is_finite() || x.abs() > 1_000_000.) {
        return Err(CompositeError::UnsupportedTransform);
    }
    Ok([
        b[0].floor() - 1.,
        b[1].floor() - 1.,
        b[2].ceil() + 1.,
        b[3].ceil() + 1.,
    ])
}
fn path_bounds(
    path: &tiny_skia::Path,
    world: Affine,
    a: PhysicalSpace,
) -> Result<Rect, CompositeError> {
    let b = path.bounds();
    rounded(crate::raster::transformed(
        [
            b.left() as f64,
            b.top() as f64,
            b.right() as f64,
            b.bottom() as f64,
        ],
        a.local(world, [0., 0.]),
    ))
}
fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let c = [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ];
    (c[2] > c[0] && c[3] > c[1]).then_some(c)
}
/// Resolve every nested source before any pixel allocation. Ancestor shadows
/// sample descendant completed support, including each discrete filter guard.
pub(crate) fn resolve(
    node: &mut PreparedNode,
    a: PhysicalSpace,
    limits: CompositeLimits,
    cancelled: &AtomicBool,
) -> Result<(), CompositeError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(CompositeError::Superseded);
    }
    let mut device = DeviceNode::default();
    if !node.visible || node.opacity == 0. {
        node.device = Some(device);
        return Ok(());
    }
    let mut own = None;
    for paint in node.paints.iter().chain(node.erase_path.iter()) {
        own = union(own, Some(path_bounds(&paint.path, paint.world, a)?));
    }
    if let Some(image) = &node.image {
        own = union(own, Some(path_bounds(&image.path, image.world, a)?));
    }
    let mut children = None;
    for child in &mut node.children {
        resolve(child, a, limits, cancelled)?;
        if child.eraser == EraserMode::None {
            children = union(children, child.device.as_ref().and_then(|d| d.completed));
        }
    }
    if let Some(clip) = &node.child_clip {
        let b = clip.local_bounds;
        let clip = rounded(crate::raster::transformed(
            [
                b.origin.x as f64,
                b.origin.y as f64,
                b.origin.x as f64 + b.size.width as f64,
                b.origin.y as f64 + b.size.height as f64,
            ],
            a.local(clip.world_transform, [0., 0.]),
        ))?;
        children = children.and_then(|b| intersect(b, clip));
    }
    let original = union(own, children);
    let mut completed = original;
    if let Some(original) = original {
        for &effect in &node.shadows {
            let plan = shadow::SourcePlan::physical(effect, a, original, limits)?;
            let x = (plan.origin[0] + plan.filter.offset[0]).floor();
            let y = (plan.origin[1] + plan.filter.offset[1]).floor();
            completed = union(
                completed,
                Some([x, y, x + plan.width as f64, y + plan.height as f64]),
            );
            device.shadows.push(plan);
        }
        for &effect in &node.inner_shadows {
            device
                .inner_shadows
                .push(shadow::SourcePlan::physical(effect, a, original, limits)?);
        }
    }
    if node.blur > 0. {
        // Validate even an empty required source: transform/sigma support is a
        // request contract, not conditional on a lucky producer's alpha.
        let (sigma, halo, kernel) = BlurPlan::parameters(node.blur, a.local(node.world, [0., 0.]))?;
        if let Some(b) = completed {
            let rect = [b[0] - halo, b[1] - halo, b[2] + halo, b[3] + halo];
            let surface = SurfacePlan::new(rect, a, limits)?;
            completed = Some(surface.rect());
            device.blur = Some(BlurPlan {
                surface,
                sigma,
                kernel,
            });
        }
    }
    device.completed = completed;
    node.device = Some(device);
    Ok(())
}

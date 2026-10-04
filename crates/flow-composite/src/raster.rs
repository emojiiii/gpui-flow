mod admission;
mod background;
use crate::{
    CanvasBackdrop, CompositeError, CompositeLimits, CustomInputs, DomainSource, ImageInputs,
    TextInputs,
    vector::{CompositeOperation, Preparation, PreparedNode, VectorPaint, VectorSource, transform},
};
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, ClipRegion, EraserMode, MaskMode, Point, Shape, Size,
};
use std::sync::atomic::{AtomicBool, Ordering};
use tiny_skia::{Mask, PathBuilder, Pixmap, PixmapPaint, Transform};

/// Absolute logical canvas coordinates, matching GPUI's paint-image bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RasterViewport {
    pub world_to_screen: Affine,
    pub canvas_bounds: Bounds,
    pub content_mask: Bounds,
    pub device_scale: f32,
}
#[derive(Debug)]
pub struct CpuFrame {
    pub screen_bounds: Bounds,
    pub width: u32,
    pub height: u32,
    /// tiny-skia's premultiplied RGBA. Convert exactly once at the GPUI boundary.
    pub premul_rgba: Vec<u8>,
}
/// Shared across all domains in one render request. Explicit surfaces and
/// retained outputs are accounted, including scratch RGBA and 8-bit masks.
/// tiny-skia's private scan-conversion allocations and GPU atlas aren't a
/// process-wide memory guarantee; input geometry is independently bounded.
#[derive(Clone)]
pub struct PixelBudget {
    limits: CompositeLimits,
    in_use: u64,
    peak: u64,
    writes: u64,
}
impl PixelBudget {
    pub fn new(limits: CompositeLimits, retained_bytes: u64) -> Result<Self, CompositeError> {
        let limits = limits.bounded();
        if retained_bytes > limits.max_surface_bytes {
            return Err(CompositeError::WorkingSetLimit);
        }
        Ok(Self {
            limits,
            in_use: retained_bytes,
            peak: retained_bytes,
            writes: 0,
        })
    }
    pub fn peak_bytes(&self) -> u64 {
        self.peak
    }
    pub fn surface_writes(&self) -> u64 {
        self.writes
    }
    pub fn retained_bytes(&self) -> u64 {
        self.in_use
    }
    /// Discard an output produced using this same budget. Consuming the frame
    /// releases its retained-byte charge while preserving cumulative work/peak.
    pub fn discard_frame(&mut self, frame: CpuFrame) {
        self.in_use = self.in_use.saturating_sub(frame.premul_rgba.len() as u64);
    }
    fn reserve(&mut self, n: u64) -> Result<(), CompositeError> {
        let next = self
            .in_use
            .checked_add(n)
            .ok_or(CompositeError::WorkingSetLimit)?;
        if next > self.limits.max_surface_bytes {
            return Err(CompositeError::WorkingSetLimit);
        }
        self.in_use = next;
        self.peak = self.peak.max(next);
        Ok(())
    }
    /// Charge producer work performed before rasterization (for example glyph pixels).
    pub fn charge_input_work(&mut self, n: u64) -> Result<(), CompositeError> {
        self.write(n)
    }
    fn write(&mut self, n: u64) -> Result<(), CompositeError> {
        self.writes = self
            .writes
            .checked_add(n)
            .ok_or(CompositeError::WorkLimit)?;
        if self.writes > self.limits.max_surface_writes {
            return Err(CompositeError::WorkLimit);
        }
        Ok(())
    }
}
fn bytes(n: u64) -> Result<Vec<u8>, CompositeError> {
    let n = usize::try_from(n).map_err(|_| CompositeError::AllocationFailed)?;
    let mut data = Vec::new();
    data.try_reserve_exact(n)
        .map_err(|_| CompositeError::AllocationFailed)?;
    data.resize(n, 0);
    Ok(data)
}
// The caller owns admission and charges the entire pass before this loop.
// A row checkpoint also makes bounded cancellation independent of pixel count.
fn grayscale_pixels(
    layer: &mut Pixmap,
    amount: f32,
    mut check: impl FnMut() -> Result<(), CompositeError>,
) -> Result<(), CompositeError> {
    let width = layer.width();
    for row in layer.data_mut().chunks_exact_mut(width as usize * 4) {
        check()?;
        for pixel in row.as_chunks_mut::<4>().0 {
            let y = (2126. * pixel[0] as f64 + 7152. * pixel[1] as f64 + 722. * pixel[2] as f64)
                / 10000.;
            let alpha = pixel[3] as f64;
            for channel in &mut pixel[..3] {
                let value = *channel as f64;
                *channel = (value + amount as f64 * (y - value))
                    .round()
                    .clamp(0., alpha) as u8;
            }
        }
    }
    Ok(())
}

// image 0.25.10 trims exact-zero Gaussian ends. Its one-tap ring path
// incorrectly reads its forced one-pixel row padding as input. Specialize that
// exact identity kernel; never clamp sigma, alter nonzero weights or change the
// positive effect's source, isolation or conservative admission.
fn one_tap_gaussian(sigma: f32) -> bool {
    (-0.5f32 * (1f32 / sigma).powf(2.)).exp()
        * (1f32 / ((2f32 * std::f32::consts::PI).sqrt() * sigma))
        == 0.
}
fn blur_alpha(
    source: image::GrayImage,
    sigma: f32,
    mut check: impl FnMut() -> Result<(), CompositeError>,
) -> Result<image::GrayImage, CompositeError> {
    check()?;
    let output = if sigma == 0. || one_tap_gaussian(sigma) {
        source
    } else {
        image::imageops::blur(&source, sigma)
    };
    check()?;
    Ok(output)
}

/// image's A8 bilinear rule, with a finite f64 transparent-domain check before
/// narrowing. Guards around complete ink keep edge clamping on transparent ink.
fn sample_alpha(source: &image::GrayImage, x: f64, y: f64) -> u8 {
    if !x.is_finite()
        || !y.is_finite()
        || x < 0.
        || y < 0.
        || x > f64::from(source.width() - 1)
        || y > f64::from(source.height() - 1)
    {
        return 0;
    }
    image::imageops::interpolate_bilinear(source, x as f32, y as f32).map_or(0, |p| p[0])
}

fn blur_pixels(
    source: Pixmap,
    sigma: f32,
    mut check: impl FnMut() -> Result<(), CompositeError>,
) -> Result<Pixmap, CompositeError> {
    check()?;
    if one_tap_gaussian(sigma) {
        check()?;
        return Ok(source);
    }
    let (width, height) = (source.width(), source.height());
    let input = image::RgbaImage::from_raw(width, height, source.take())
        .ok_or(CompositeError::AllocationFailed)?;
    // Explicitly encoded-sRGB premultiplied RGBA, including alpha. Ownership
    // transfers avoid a second full byte-plane or straight-alpha roundtrip.
    let output = image::imageops::blur(&input, sigma);
    check()?;
    drop(input);
    let mut output = output.into_raw();
    for row in output.chunks_exact_mut(width as usize * 4) {
        check()?;
        for pixel in row.as_chunks_mut::<4>().0 {
            let alpha = pixel[3];
            for c in &mut pixel[..3] {
                *c = (*c).min(alpha);
            }
        }
    }
    Pixmap::from_vec(
        output,
        tiny_skia::IntSize::from_wh(width, height).ok_or(CompositeError::TooLarge)?,
    )
    .ok_or(CompositeError::AllocationFailed)
}

struct GridLayout {
    scale: f64,
    gap: f64,
    start: [f64; 2],
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
}
#[allow(clippy::too_many_arguments)]
fn grid_layout(
    backdrop: CanvasBackdrop,
    viewport: RasterViewport,
    origin: [f64; 2],
    dimensions: [u32; 2],
    prepared_points: usize,
    limits: CompositeLimits,
    extend: bool,
) -> Result<Option<GridLayout>, CompositeError> {
    let Some(grid) = backdrop.grid.filter(|g| g.color.a != 0) else {
        return Ok(None);
    };
    let scale = viewport.device_scale as f64;
    let gap = grid.spacing as f64;
    let canvas = bounds(viewport.canvas_bounds);
    let start = [
        canvas[0] + (grid.offset.x as f64).rem_euclid(gap),
        canvas[1] + (grid.offset.y as f64).rem_euclid(gap),
    ];
    let range = |axis: usize| {
        let low = origin[axis] / scale;
        let high = (origin[axis] + dimensions[axis] as f64) / scale;
        let first = ((low - start[axis] - 1.) / gap).ceil();
        let first = if extend { first } else { first.max(0.) };
        let high = if extend {
            high
        } else {
            high.min(canvas[axis + 2])
        };
        (first, ((high - start[axis]) / gap).ceil().max(first))
    };
    let (x0, x1) = range(0);
    let (y0, y1) = range(1);
    let dots = (x1 - x0) * (y1 - y0);
    if !dots.is_finite() || dots * 4. + prepared_points as f64 > limits.max_total_points as f64 {
        return Err(CompositeError::InputLimit);
    }
    Ok(Some(GridLayout {
        scale,
        gap,
        start,
        x0,
        x1,
        y0,
        y1,
    }))
}

struct Raster<'a> {
    track_initialization: bool,
    owner: Option<crate::background::Owner>,
    width: u32,
    height: u32,
    pixels: u64,
    world_to_layer: Affine,
    origin: [f64; 2],
    physical: Option<crate::blur::PhysicalSpace>,
    budget: &'a mut PixelBudget,
    cancelled: &'a AtomicBool,
}
impl Raster<'_> {
    fn local_to_layer(&self, world: Affine) -> Affine {
        self.physical.map_or_else(
            || self.world_to_layer * world,
            |a| a.local(world, self.origin),
        )
    }

    fn backdrop(
        &mut self,
        target: &mut Pixmap,
        backdrop: CanvasBackdrop,
        viewport: RasterViewport,
        origin: [f64; 2],
        prepared_points: usize,
        extend: bool,
    ) -> Result<(), CompositeError> {
        self.check()?;
        self.budget.write(self.pixels)?;
        let c = backdrop.color;
        target.fill(tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a));
        let Some(grid) = backdrop.grid.filter(|g| g.color.a != 0) else {
            return Ok(());
        };
        let Some(layout) = grid_layout(
            backdrop,
            viewport,
            origin,
            [self.width, self.height],
            prepared_points,
            self.budget.limits,
            extend,
        )?
        else {
            return Ok(());
        };
        let GridLayout {
            scale,
            gap,
            start,
            x0,
            x1,
            y0,
            y1,
        } = layout;
        let mut path = PathBuilder::new();
        for ix in 0..(x1 - x0) as usize {
            self.check()?;
            let x = (start[0] + (x0 + ix as f64) * gap) * scale - origin[0];
            for iy in 0..(y1 - y0) as usize {
                let y = (start[1] + (y0 + iy as f64) * gap) * scale - origin[1];
                // A legal tiny logical viewport can have an enormous DPR.
                // Clip these axis-aligned dots before constructing geometry,
                // retaining a device-pixel guard for antialiased edge coverage.
                let left = x.max(-1.);
                let top = y.max(-1.);
                let right = (x + scale).min(self.width as f64 + 1.);
                let bottom = (y + scale).min(self.height as f64 + 1.);
                if right <= left || bottom <= top {
                    continue;
                }
                let rect = tiny_skia::Rect::from_xywh(
                    left as f32,
                    top as f32,
                    (right - left) as f32,
                    (bottom - top) as f32,
                )
                .ok_or(CompositeError::UnsupportedTransform)?;
                path.push_rect(rect);
            }
        }
        if extend {
            self.budget.write(self.pixels)?;
        }
        if let Some(path) = path.finish() {
            if !extend {
                self.budget.write(self.pixels)?;
            }
            let c = grid.color;
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(c.r, c.g, c.b, c.a);
            target.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        Ok(())
    }
    fn check(&self) -> Result<(), CompositeError> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err(CompositeError::Superseded)
        } else {
            Ok(())
        }
    }
    fn surface(&mut self) -> Result<Pixmap, CompositeError> {
        self.check()?;
        self.budget.reserve(self.pixels * 4)?;
        if self.track_initialization {
            self.budget.write(self.pixels)?;
        }
        let data = bytes(self.pixels * 4)?;
        Pixmap::from_vec(
            data,
            tiny_skia::IntSize::from_wh(self.width, self.height).ok_or(CompositeError::TooLarge)?,
        )
        .ok_or(CompositeError::AllocationFailed)
    }
    fn release(&mut self, surface: Pixmap) {
        self.budget.in_use -= surface.data().len() as u64;
        drop(surface);
    }
    fn composite(
        &mut self,
        target: &mut Pixmap,
        source: &Pixmap,
        opacity: f32,
        operation: CompositeOperation,
        clip: Option<&Mask>,
    ) -> Result<(), CompositeError> {
        self.composite_at(target, source, opacity, operation, clip, [0, 0])
    }
    fn composite_at(
        &mut self,
        target: &mut Pixmap,
        source: &Pixmap,
        opacity: f32,
        operation: CompositeOperation,
        clip: Option<&Mask>,
        offset: [i32; 2],
    ) -> Result<(), CompositeError> {
        self.check()?;
        self.budget.write(self.pixels)?;
        let blend = match operation {
            CompositeOperation::DestinationOut => {
                for y in 0..self.height {
                    self.check()?;
                    let sy = y as i64 - offset[1] as i64;
                    if sy < 0 || sy >= source.height() as i64 {
                        continue;
                    }
                    for x in 0..self.width {
                        let sx = x as i64 - offset[0] as i64;
                        if sx < 0 || sx >= source.width() as i64 {
                            continue;
                        }
                        let index = (y * self.width + x) as usize;
                        let si = (sy as u32 * source.width() + sx as u32) as usize;
                        let coverage = clip.map_or(255, |m| m.data()[index]);
                        let keep = 1.
                            - f64::from(source.data()[si * 4 + 3]) / 255.
                                * f64::from(opacity)
                                * f64::from(coverage)
                                / 255.;
                        for channel in &mut target.data_mut()[index * 4..index * 4 + 4] {
                            *channel = (f64::from(*channel) * keep).round() as u8;
                        }
                    }
                }
                return Ok(());
            }
            CompositeOperation::Blend(blend) => blend,
        };
        if crate::nonseparable::is_nonseparable(blend) {
            for y in 0..self.height {
                self.check()?;
                let sy = y as i64 - offset[1] as i64;
                if sy < 0 || sy >= source.height() as i64 {
                    continue;
                }
                for x in 0..self.width {
                    let sx = x as i64 - offset[0] as i64;
                    if sx < 0 || sx >= source.width() as i64 {
                        continue;
                    }
                    let index = (y * self.width + x) as usize;
                    let si = (sy as u32 * source.width() + sx as u32) as usize;
                    let src: &[u8; 4] =
                        source.data()[si * 4..si * 4 + 4].try_into().expect("pixel");
                    let dst: &mut [u8; 4] = (&mut target.data_mut()[index * 4..index * 4 + 4])
                        .try_into()
                        .expect("pixel");
                    crate::nonseparable::pixel(
                        dst,
                        src,
                        opacity,
                        clip.map_or(255, |mask| mask.data()[index]),
                        blend,
                    );
                }
            }
            return Ok(());
        }
        target.draw_pixmap(
            offset[0],
            offset[1],
            source.as_ref(),
            &PixmapPaint {
                opacity,
                blend_mode: match blend {
                    BlendMode::Multiply => tiny_skia::BlendMode::Multiply,
                    BlendMode::Screen => tiny_skia::BlendMode::Screen,
                    BlendMode::Overlay => tiny_skia::BlendMode::Overlay,
                    BlendMode::Darken => tiny_skia::BlendMode::Darken,
                    BlendMode::Lighten => tiny_skia::BlendMode::Lighten,
                    BlendMode::ColorDodge => tiny_skia::BlendMode::ColorDodge,
                    BlendMode::ColorBurn => tiny_skia::BlendMode::ColorBurn,
                    BlendMode::HardLight => tiny_skia::BlendMode::HardLight,
                    BlendMode::SoftLight => tiny_skia::BlendMode::SoftLight,
                    BlendMode::Difference => tiny_skia::BlendMode::Difference,
                    BlendMode::Exclusion => tiny_skia::BlendMode::Exclusion,
                    BlendMode::Hue => tiny_skia::BlendMode::Hue,
                    BlendMode::Saturation => tiny_skia::BlendMode::Saturation,
                    BlendMode::Color => tiny_skia::BlendMode::Color,
                    BlendMode::Luminosity => tiny_skia::BlendMode::Luminosity,
                    BlendMode::PassThrough | BlendMode::Normal => tiny_skia::BlendMode::SourceOver,
                },
                ..Default::default()
            },
            Transform::identity(),
            clip,
        );
        Ok(())
    }
    fn node(
        &mut self,
        node: &PreparedNode,
        target: &mut Pixmap,
        depth: usize,
        clip: Option<&Mask>,
    ) -> Result<(), CompositeError> {
        self.check()?;
        if !node.visible || node.opacity == 0. {
            return Ok(());
        }
        // Conservative, prepared ink bounds include strokes and descendants.
        // Keep an AA guard; skipping a mask's ink still leaves its run marker
        // in children(), so an offscreen alpha mask correctly masks everything.
        if node.outside(self.world_to_layer, self.origin, self.width, self.height) {
            return Ok(());
        }
        if node.eraser == EraserMode::Path {
            return self.erase_path(node, target, clip);
        }
        if let Some(plan) = node.device.as_ref().and_then(|d| d.background.as_ref()) {
            return self.background_blur(node, plan, target, depth, clip);
        }
        if let Some(plan) = node.device.as_ref().and_then(|d| d.blur.as_ref()) {
            return self.blur(node, plan, target, depth, clip, false);
        }
        if node.isolated() {
            if depth >= self.budget.limits.max_depth {
                return Err(CompositeError::NestingLimit);
            }
            let mut layer = self.surface()?;
            let previous_owner = self
                .owner
                .replace(crate::background::Owner::Transparent(node.id.clone()));
            self.contents(node, &mut layer, depth + 1, None, true)?;
            self.owner = previous_owner;
            if node.grayscale > 0. {
                self.grayscale(&mut layer, node.grayscale)?;
            }
            self.composite(target, &layer, node.opacity, node.operation(), clip)?;
            self.release(layer);
        } else {
            self.contents(node, target, depth, clip, true)?;
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn blur(
        &mut self,
        node: &PreparedNode,
        plan: &crate::blur::BlurPlan,
        target: &mut Pixmap,
        depth: usize,
        clip: Option<&Mask>,
        foreground: bool,
    ) -> Result<(), CompositeError> {
        if depth >= self.budget.limits.max_depth {
            return Err(CompositeError::NestingLimit);
        }
        self.check()?;
        self.budget.reserve(plan.bytes())?;
        self.budget.write(plan.work())?;
        let p = &plan.surface;
        let mut source = Pixmap::from_vec(
            bytes(p.pixels * 4)?,
            tiny_skia::IntSize::from_wh(p.width, p.height).ok_or(CompositeError::TooLarge)?,
        )
        .ok_or(CompositeError::AllocationFailed)?;
        let mut scratch = Raster {
            track_initialization: self.track_initialization,
            owner: Some(crate::background::Owner::Transparent(node.id.clone())),
            width: p.width,
            height: p.height,
            pixels: p.pixels,
            world_to_layer: p.world_to_layer,
            origin: p.origin,
            physical: self.physical,
            budget: self.budget,
            cancelled: self.cancelled,
        };
        // Own shadow captures continue to omit own filters; descendants execute
        // their complete node paths inside both original and final phase streams.
        scratch.contents(node, &mut source, depth + 1, None, true)?;
        let mut output = blur_pixels(source, plan.sigma, || scratch.check())?;
        if node.grayscale > 0. {
            scratch.grayscale(&mut output, node.grayscale)?;
        }
        self.composite_at(
            target,
            &output,
            if foreground { 1. } else { node.opacity },
            if foreground {
                CompositeOperation::Blend(BlendMode::Normal)
            } else {
                node.operation()
            },
            clip,
            [
                (p.origin[0] - self.origin[0]) as i32,
                (p.origin[1] - self.origin[1]) as i32,
            ],
        )?;
        drop(output);
        self.budget.in_use -= plan.bytes();
        Ok(())
    }
    /// Homogeneous sRGB matrix on premultiplied bytes: alpha commutes with
    /// this matrix, so avoid a lossy straight-alpha byte roundtrip.
    fn grayscale(&mut self, layer: &mut Pixmap, amount: f32) -> Result<(), CompositeError> {
        self.budget.write(self.pixels)?;
        grayscale_pixels(layer, amount, || self.check())
    }
    fn erase_path(
        &mut self,
        node: &PreparedNode,
        target: &mut Pixmap,
        clip: Option<&Mask>,
    ) -> Result<(), CompositeError> {
        let Some(path) = &node.erase_path else {
            return Ok(());
        };
        let a = self.local_to_layer(path.world);
        if a.inverse().is_none() || transform(a).invert().is_none_or(|t| !t.is_finite()) {
            return Err(CompositeError::UnsupportedTransform);
        }
        for p in path.path.points() {
            let q = a.transform_point(Point::new(p.x, p.y));
            if !q.is_finite() || q.x.abs() > 1_000_000. || q.y.abs() > 1_000_000. {
                return Err(CompositeError::UnsupportedTransform);
            }
        }
        self.check()?;
        self.budget.reserve(self.pixels)?;
        if self.track_initialization {
            self.budget.write(self.pixels)?;
        }
        let mut mask = Mask::from_vec(
            bytes(self.pixels)?,
            tiny_skia::IntSize::from_wh(self.width, self.height).ok_or(CompositeError::TooLarge)?,
        )
        .ok_or(CompositeError::AllocationFailed)?;
        self.budget.write(self.pixels * 2)?;
        mask.fill_path(&path.path, path.rule, true, transform(a));
        if !node.erase_clips.is_empty() {
            let intersection = self.path_mask(&node.erase_clips, Some(&mask))?;
            self.budget.in_use -= self.pixels;
            mask = intersection;
        }
        for (y, row) in target
            .data_mut()
            .chunks_mut(self.width as usize * 4)
            .enumerate()
        {
            self.check()?;
            for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let index = y * self.width as usize + x;
                let coverage = u32::from(mask.data()[index])
                    * u32::from(clip.map_or(255, |m| m.data()[index]));
                for channel in pixel {
                    *channel =
                        ((u32::from(*channel) * (65_025 - coverage) + 32_512) / 65_025) as u8;
                }
            }
        }
        self.budget.in_use -= self.pixels;
        Ok(())
    }
    fn contents(
        &mut self,
        node: &PreparedNode,
        target: &mut Pixmap,
        depth: usize,
        clip: Option<&Mask>,
        draw_shadow: bool,
    ) -> Result<(), CompositeError> {
        if draw_shadow {
            for (index, &shadow) in node.shadows.iter().enumerate() {
                self.shadow(node, shadow, index, target, depth, false)?;
            }
        }
        let (before, after) = node.paints.split_at(node.fill_count);
        self.paints(before, target, clip)?;
        if let Some(p) = &node.image {
            self.check()?;
            let a = self.local_to_layer(p.world);
            let texture = a * p.texture_to_local;
            for matrix in [a, texture] {
                if matrix.inverse().is_none()
                    || transform(matrix).invert().is_none_or(|t| !t.is_finite())
                {
                    return Err(CompositeError::UnsupportedTransform);
                }
            }
            for point in p.path.points() {
                let q = a.transform_point(Point::new(point.x, point.y));
                if !q.is_finite() || q.x.abs() > 1_000_000. || q.y.abs() > 1_000_000. {
                    return Err(CompositeError::UnsupportedTransform);
                }
            }
            let paint = tiny_skia::Paint {
                shader: tiny_skia::Pattern::new(
                    p.image.pixmap.as_ref(),
                    tiny_skia::SpreadMode::Pad,
                    crate::canvas_surface::quality(p.smooth),
                    1.,
                    transform(p.texture_to_local),
                ),
                ..Default::default()
            };
            let own_clip = p
                .own_clip
                .as_ref()
                .map(|region| self.child_clip(region, clip))
                .transpose()?;
            self.budget.write(self.pixels)?;
            target.fill_path(
                &p.path,
                &paint,
                tiny_skia::FillRule::Winding,
                transform(a),
                own_clip.as_ref().or(clip),
            );
            if own_clip.is_some() {
                self.budget.in_use -= self.pixels;
            }
        }
        if node.children_before_inner {
            self.clipped_children(node, target, depth, clip)?;
        }
        // Inner effects always precede own strokes. BeforeChildren containers
        // keep both operations before children; opaque children may hide them.
        // Every layer samples the complete original phase stream, not whatever
        // has already been painted at this insertion point.
        if draw_shadow {
            for (index, &shadow) in node.inner_shadows.iter().enumerate() {
                self.shadow(node, shadow, index, target, depth, true)?;
            }
        }
        self.paints(after, target, clip)?;
        if !node.children_before_inner {
            self.clipped_children(node, target, depth, clip)?;
        }
        Ok(())
    }
    fn clipped_children(
        &mut self,
        node: &PreparedNode,
        target: &mut Pixmap,
        depth: usize,
        clip: Option<&Mask>,
    ) -> Result<(), CompositeError> {
        if let Some(region) = &node.child_clip {
            if !node.children.is_empty() {
                let child_clip = self.child_clip(region, clip)?;
                self.children(node, target, depth, Some(&child_clip))?;
                self.budget.in_use -= self.pixels;
            }
        } else {
            self.children(node, target, depth, clip)?;
        }
        // The temporary child clip never clips own strokes or inner effects.
        Ok(())
    }
    fn paints(
        &mut self,
        paints: &[VectorPaint],
        target: &mut Pixmap,
        clip: Option<&Mask>,
    ) -> Result<(), CompositeError> {
        for p in paints {
            self.check()?;
            let a = self.local_to_layer(p.world);
            if a.inverse().is_none() || transform(a).invert().is_none_or(|t| !t.is_finite()) {
                return Err(CompositeError::UnsupportedTransform);
            }
            // Check every control point, not just a clipped allocation rectangle.
            for point in p.path.points() {
                let q = a.transform_point(Point::new(point.x, point.y));
                if !q.is_finite() || q.x.abs() > 1_000_000. || q.y.abs() > 1_000_000. {
                    return Err(CompositeError::UnsupportedTransform);
                }
            }
            self.budget.write(self.pixels)?;
            let pattern_paint;
            let paint = match &p.paint {
                VectorSource::Scene(paint) => paint,
                VectorSource::Pattern {
                    image,
                    texture_to_local,
                    smooth,
                } => {
                    let texture = a * *texture_to_local;
                    // Check the exact composition tiny-skia will use too: its
                    // scale/translation fast path rounds in f32, unlike Affine.
                    let shader_transform = transform(*texture_to_local).post_concat(transform(a));
                    let inverse = shader_transform
                        .invert()
                        .filter(|t| t.is_finite())
                        .ok_or(CompositeError::UnsupportedTransform)?;
                    if texture.inverse().is_none()
                        || [
                            texture.a, texture.b, texture.c, texture.d, texture.tx, texture.ty,
                        ]
                        .iter()
                        .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
                    {
                        return Err(CompositeError::UnsupportedTransform);
                    }
                    // Bound source coordinates as well as inverse coefficients:
                    // f32 stops representing every integer texel above 2^24,
                    // and tiny-skia's bilinear floor/fract uses i32 conversion.
                    // Merely finite extreme scales can silently paint transparent
                    // even from an opaque tile. The padded domain also covers
                    // SIMD/filter work; this is not a fractional-precision promise.
                    for (x, y) in [
                        (-16., -16.),
                        (self.width as f32 + 16., -16.),
                        (-16., self.height as f32 + 16.),
                        (self.width as f32 + 16., self.height as f32 + 16.),
                    ] {
                        let mut sample = tiny_skia::Point::from_xy(x, y);
                        inverse.map_point(&mut sample);
                        if !sample.x.is_finite()
                            || !sample.y.is_finite()
                            || sample.x.abs() > 16_777_216.
                            || sample.y.abs() > 16_777_216.
                        {
                            return Err(CompositeError::UnsupportedTransform);
                        }
                    }
                    pattern_paint = tiny_skia::Paint {
                        shader: tiny_skia::Pattern::new(
                            image.pixmap.as_ref(),
                            tiny_skia::SpreadMode::Repeat,
                            crate::canvas_surface::quality(*smooth),
                            1.,
                            transform(*texture_to_local),
                        ),
                        ..Default::default()
                    };
                    &pattern_paint
                }
            };
            target.fill_path(&p.path, paint, p.rule, transform(a), clip);
        }
        Ok(())
    }
    fn shadow(
        &mut self,
        node: &PreparedNode,
        shadow: crate::shadow::PreparedShadow,
        index: usize,
        target: &mut Pixmap,
        depth: usize,
        inner: bool,
    ) -> Result<(), CompositeError> {
        let plan = if let Some(device) = &node.device {
            let Some(plan) = (if inner {
                &device.inner_shadows
            } else {
                &device.shadows
            })
            .get(index) else {
                return Ok(()); // empty foreground has no shadow source
            };
            plan.clone()
        } else {
            crate::shadow::SourcePlan::new(shadow, self.world_to_layer, self.budget.limits)?
        };
        if plan.mode.is_some() {
            return self.shadow_modes(node, shadow, target, depth, inner, &plan);
        }
        let (width, height, pixels) = (plan.width, plan.height, plan.pixels);
        let [x0, y0] = if node.device.is_some() {
            [
                plan.origin[0] - self.origin[0],
                plan.origin[1] - self.origin[1],
            ]
        } else {
            plan.origin
        };
        let filter = &plan.filter;
        let scratch_bytes = plan.bytes();
        let filter_work = plan.work(inner);
        self.check()?;
        // Complete recursive scratch/work admission already succeeded before
        // the domain output allocation. Runtime accounting remains a backstop.
        self.budget.reserve(scratch_bytes)?;
        self.budget.write(filter_work)?;
        let mut source = Pixmap::from_vec(
            bytes(pixels * 4)?,
            tiny_skia::IntSize::from_wh(width, height).ok_or(CompositeError::TooLarge)?,
        )
        .ok_or(CompositeError::AllocationFailed)?;
        let mut scratch = Raster {
            track_initialization: self.track_initialization,
            owner: Some(crate::background::Owner::Transparent(node.id.clone())),
            width,
            height,
            pixels,
            world_to_layer: plan.world_to_layer,
            origin: plan.origin,
            physical: self.physical,
            budget: self.budget,
            cancelled: self.cancelled,
        };
        scratch.contents(node, &mut source, depth, None, false)?;
        let mut alpha = bytes(pixels)?;
        for (row, input) in alpha
            .chunks_mut(width as usize)
            .zip(source.data().chunks(width as usize * 4))
        {
            scratch.check()?;
            for (a, p) in row.iter_mut().zip(input.as_chunks::<4>().0) {
                *a = p[3];
            }
        }
        let alpha = image::GrayImage::from_raw(width, height, alpha)
            .ok_or(CompositeError::AllocationFailed)?;
        scratch.check()?;
        let alpha = blur_alpha(alpha, filter.sigma, || scratch.check())?;
        scratch.check()?;
        let c = shadow.effect.color;
        let position = if inner {
            [x0, y0]
        } else {
            [x0 + filter.offset[0], y0 + filter.offset[1]]
        };
        let origin = [position[0].floor(), position[1].floor()];
        let fraction = [
            (position[0] - origin[0]) as f32,
            (position[1] - origin[1]) as f32,
        ];
        // tiny-skia 0.11.4 unconditionally lowers translate-only patterns to
        // nearest, even for fractional offsets. Reuse image's bilinear A8
        // sampler into our already-admitted RGBA scratch, then integer-blit.
        for (y, row) in source.data_mut().chunks_mut(width as usize * 4).enumerate() {
            scratch.check()?;
            for (x, p) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let sampled = image::imageops::interpolate_bilinear(
                    &alpha,
                    x as f32
                        - if inner {
                            filter.offset[0] as f32
                        } else {
                            fraction[0]
                        },
                    y as f32
                        - if inner {
                            filter.offset[1] as f32
                        } else {
                            fraction[1]
                        },
                )
                .map_or(0, |p| p[0]);
                // Keep original source coverage, including holes and edge AA.
                // With zero blur/offset, translucent ink deliberately yields
                // A*(1-A); only fully opaque ink has zero inner coverage.
                let coverage = if inner {
                    ((p[3] as u32 * (255 - sampled as u32) + 127) / 255) as u8
                } else {
                    sampled
                };
                let a = (coverage as u32 * c.a as u32 + 127) / 255;
                *p = [
                    ((c.r as u32 * a + 127) / 255) as u8,
                    ((c.g as u32 * a + 127) / 255) as u8,
                    ((c.b as u32 * a + 127) / 255) as u8,
                    a as u8,
                ];
            }
        }
        self.budget.write(self.pixels)?;
        target.draw_pixmap(
            0,
            0,
            source.as_ref(),
            &PixmapPaint::default(),
            Transform::from_translate(origin[0] as f32, origin[1] as f32),
            // Every visible effect creates an isolation boundary. Its ancestor
            // clip is applied there, once, after source and all own effects.
            None,
        );
        self.check()?;
        drop(alpha);
        drop(source);
        self.budget.in_use -= scratch_bytes;
        Ok(())
    }
    fn shadow_modes(
        &mut self,
        node: &PreparedNode,
        shadow: crate::shadow::PreparedShadow,
        target: &mut Pixmap,
        depth: usize,
        inner: bool,
        plan: &crate::shadow::SourcePlan,
    ) -> Result<(), CompositeError> {
        let mode = plan.mode.as_ref().expect("mode source plan");
        let Some(output_plan) = &mode.output else {
            return Ok(());
        };
        self.check()?;
        let scratch_bytes = plan.bytes();
        self.budget.reserve(scratch_bytes)?;
        self.budget.write(plan.work(inner))?;
        let mut source = Pixmap::from_vec(
            bytes(plan.pixels * 4)?,
            tiny_skia::IntSize::from_wh(plan.width, plan.height).ok_or(CompositeError::TooLarge)?,
        )
        .ok_or(CompositeError::AllocationFailed)?;
        let mut scratch = Raster {
            track_initialization: self.track_initialization,
            owner: Some(crate::background::Owner::Transparent(node.id.clone())),
            width: plan.width,
            height: plan.height,
            pixels: plan.pixels,
            world_to_layer: plan.world_to_layer,
            origin: plan.origin,
            physical: self.physical,
            budget: self.budget,
            cancelled: self.cancelled,
        };
        scratch.contents(node, &mut source, depth, None, false)?;
        let mut original = bytes(plan.pixels)?;
        for (row, input) in original
            .chunks_mut(plan.width as usize)
            .zip(source.data().chunks(plan.width as usize * 4))
        {
            scratch.check()?;
            for (a, p) in row.iter_mut().zip(input.as_chunks::<4>().0) {
                *a = p[3];
            }
        }
        let original = image::GrayImage::from_raw(plan.width, plan.height, original)
            .ok_or(CompositeError::AllocationFailed)?;
        let projected = if let Some(surface) = &mode.transformed {
            let mut alpha = bytes(surface.pixels)?;
            for (y, row) in alpha.chunks_mut(surface.width as usize).enumerate() {
                scratch.check()?;
                for (x, pixel) in row.iter_mut().enumerate() {
                    let q = [
                        surface.origin[0] + x as f64 + 0.5,
                        surface.origin[1] + y as f64 + 0.5,
                    ];
                    let p = match mode.kind {
                        crate::shadow::SpreadMode::Identity => q,
                        crate::shadow::SpreadMode::Resample => [
                            mode.center[0] + (q[0] - mode.center[0]) / mode.factors[0],
                            mode.center[1] + (q[1] - mode.center[1]) / mode.factors[1],
                        ],
                        crate::shadow::SpreadMode::Collapsed => unreachable!("collapsed filter"),
                    };
                    *pixel = sample_alpha(
                        &original,
                        p[0] - plan.origin[0] - 0.5,
                        p[1] - plan.origin[1] - 0.5,
                    );
                }
            }
            let alpha = image::GrayImage::from_raw(surface.width, surface.height, alpha)
                .ok_or(CompositeError::AllocationFailed)?;
            Some(blur_alpha(alpha, plan.filter.sigma, || scratch.check())?)
        } else {
            None
        };
        let mut output = Pixmap::from_vec(
            bytes(output_plan.pixels * 4)?,
            tiny_skia::IntSize::from_wh(output_plan.width, output_plan.height)
                .ok_or(CompositeError::TooLarge)?,
        )
        .ok_or(CompositeError::AllocationFailed)?;
        let c = shadow.effect.color;
        for (y, row) in output
            .data_mut()
            .chunks_mut(output_plan.width as usize * 4)
            .enumerate()
        {
            scratch.check()?;
            for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let q = [
                    output_plan.origin[0] + x as f64,
                    output_plan.origin[1] + y as f64,
                ];
                let b = projected.as_ref().zip(mode.transformed.as_ref()).map_or(
                    0,
                    |(alpha, surface)| {
                        sample_alpha(
                            alpha,
                            q[0] - plan.filter.offset[0] - surface.origin[0],
                            q[1] - plan.filter.offset[1] - surface.origin[1],
                        )
                    },
                );
                let coverage = if inner || shadow.effect.r#box {
                    // Original untransformed, unoffset alpha at final q.
                    let a = sample_alpha(&original, q[0] - plan.origin[0], q[1] - plan.origin[1]);
                    if inner {
                        (u32::from(a) * u32::from(255 - b) + 127) / 255
                    } else {
                        (u32::from(b) * u32::from(255 - a) + 127) / 255
                    }
                } else {
                    u32::from(b)
                };
                let a = (coverage * u32::from(c.a) + 127) / 255;
                *pixel = [
                    ((u32::from(c.r) * a + 127) / 255) as u8,
                    ((u32::from(c.g) * a + 127) / 255) as u8,
                    ((u32::from(c.b) * a + 127) / 255) as u8,
                    a as u8,
                ];
            }
        }
        self.budget.write(self.pixels)?;
        target.draw_pixmap(
            0,
            0,
            output.as_ref(),
            &PixmapPaint::default(),
            Transform::from_translate(
                (output_plan.origin[0] - self.origin[0]) as f32,
                (output_plan.origin[1] - self.origin[1]) as f32,
            ),
            None,
        );
        self.check()?;
        drop(projected);
        drop(original);
        drop(source);
        drop(output);
        self.budget.in_use -= scratch_bytes;
        Ok(())
    }
    fn children(
        &mut self,
        node: &PreparedNode,
        target: &mut Pixmap,
        depth: usize,
        clip: Option<&Mask>,
    ) -> Result<(), CompositeError> {
        // Only a visible, positive-opacity marker enables mask scheduling.
        // Once enabled, all markers (including hidden ones) split runs.
        if !node
            .children
            .iter()
            .any(|child| child.mask != MaskMode::None && child.visible && child.opacity > 0.)
        {
            for child in &node.children {
                self.node(child, target, depth, clip)?;
            }
            return Ok(());
        }
        let mut i = 0;
        while i < node.children.len() {
            let child = &node.children[i];
            if child.mask == MaskMode::None {
                self.node(child, target, depth, clip)?;
                i += 1;
                continue;
            }
            let mask_node = child;
            if mask_node.mask.paints_self() {
                self.node(mask_node, target, depth, clip)?;
            }
            if mask_node.mask.is_path() {
                let end = node.children[i + 1..]
                    .iter()
                    .position(|c| c.mask != MaskMode::None)
                    .map_or(node.children.len(), |n| i + 1 + n);
                if mask_node.opacity > 0. {
                    let opaque = mask_node.opacity == 1.;
                    let mask =
                        self.path_mask(&mask_node.mask_paths, if opaque { clip } else { None })?;
                    if opaque {
                        // A fully opaque path mask clips the actual parent;
                        // artistic blending and erasers must see its backdrop.
                        for child in &node.children[i + 1..end] {
                            self.node(child, target, depth, Some(&mask))?;
                        }
                    } else {
                        let mut content = self.surface()?;
                        for child in &node.children[i + 1..end] {
                            self.node(child, &mut content, depth, Some(&mask))?;
                        }
                        self.composite(
                            target,
                            &content,
                            mask_node.opacity,
                            CompositeOperation::Blend(BlendMode::Normal),
                            clip,
                        )?;
                        self.release(content);
                    }
                    self.budget.in_use -= self.pixels;
                }
                i = end;
                continue;
            }
            let mut mask_surface = self.surface()?;
            self.node(mask_node, &mut mask_surface, depth, None)?;
            self.budget.reserve(self.pixels)?;
            if self.track_initialization {
                self.budget.write(self.pixels)?;
            }
            let mut alpha = bytes(self.pixels)?;
            self.budget.write(self.pixels)?;
            for (row, input) in alpha
                .chunks_mut(self.width as usize)
                .zip(mask_surface.data().chunks(self.width as usize * 4))
            {
                self.check()?;
                for (a, p) in row.iter_mut().zip(input.as_chunks::<4>().0) {
                    *a = if mask_node.mask == MaskMode::Grayscale {
                        ((299 * u32::from(p[0])
                            + 587 * u32::from(p[1])
                            + 114 * u32::from(p[2])
                            + 500)
                            / 1000) as u8
                    } else {
                        p[3]
                    };
                }
            }
            let mask = Mask::from_vec(
                alpha,
                tiny_skia::IntSize::from_wh(self.width, self.height)
                    .ok_or(CompositeError::TooLarge)?,
            )
            .ok_or(CompositeError::AllocationFailed)?;
            self.release(mask_surface);
            let mut content = self.surface()?;
            i += 1;
            while i < node.children.len() && node.children[i].mask == MaskMode::None {
                let c = &node.children[i];
                // Leafer flushes independent blend children through an opaque
                // mask to the parent's actual CPU backdrop. A translucent mask
                // instead composites the entire run before applying alpha.
                let flush = c.flushes_mask(mask_node.opacity);
                if flush {
                    self.masked(
                        target,
                        &mut content,
                        &mask,
                        CompositeOperation::Blend(BlendMode::Normal),
                        clip,
                    )?;
                }
                self.node(c, &mut content, depth, None)?;
                if flush {
                    self.masked(target, &mut content, &mask, c.operation(), clip)?;
                }
                i += 1;
            }
            self.masked(
                target,
                &mut content,
                &mask,
                CompositeOperation::Blend(BlendMode::Normal),
                clip,
            )?;
            self.release(content);
            self.budget.in_use -= self.pixels;
            drop(mask);
        }
        Ok(())
    }
    fn path_mask(
        &mut self,
        paths: &[Option<crate::vector::VectorPaint>],
        inherited: Option<&Mask>,
    ) -> Result<Mask, CompositeError> {
        self.check()?;
        self.budget.reserve(self.pixels)?;
        if self.track_initialization {
            self.budget.write(self.pixels)?;
        }
        let mut mask = Mask::from_vec(
            bytes(self.pixels)?,
            tiny_skia::IntSize::from_wh(self.width, self.height).ok_or(CompositeError::TooLarge)?,
        )
        .ok_or(CompositeError::AllocationFailed)?;
        self.budget.write(self.pixels)?;
        if let Some(inherited) = inherited {
            mask.data_mut().copy_from_slice(inherited.data());
        } else {
            mask.data_mut().fill(255);
        }
        for path in paths {
            self.check()?;
            if let Some(path) = path {
                let a = self.local_to_layer(path.world);
                if a.inverse().is_none() || transform(a).invert().is_none_or(|t| !t.is_finite()) {
                    return Err(CompositeError::UnsupportedTransform);
                }
                for p in path.path.points() {
                    let q = a.transform_point(Point::new(p.x, p.y));
                    if !q.is_finite() || q.x.abs() > 1_000_000. || q.y.abs() > 1_000_000. {
                        return Err(CompositeError::UnsupportedTransform);
                    }
                }
                self.budget.reserve(self.pixels)?;
                if self.track_initialization {
                    self.budget.write(self.pixels)?;
                }
                self.budget.write(self.pixels * 2)?;
                mask.intersect_path(&path.path, path.rule, true, transform(a));
                self.budget.in_use -= self.pixels;
            } else {
                self.budget.write(self.pixels)?;
                mask.data_mut().fill(0);
            }
        }
        Ok(mask)
    }
    fn masked(
        &mut self,
        target: &mut Pixmap,
        content: &mut Pixmap,
        mask: &Mask,
        operation: CompositeOperation,
        clip: Option<&Mask>,
    ) -> Result<(), CompositeError> {
        self.check()?;
        self.budget.write(self.pixels)?;
        // Exact common dimensions/origin are guaranteed by this raster owner.
        content.apply_mask(mask);
        self.composite(target, content, 1., operation, clip)?;
        self.budget.write(self.pixels)?;
        content.fill(tiny_skia::Color::TRANSPARENT);
        Ok(())
    }
    // A clip is not an isolation boundary: pass-through children must still
    // blend against preceding pixels in the actual parent surface.
    fn child_clip(
        &mut self,
        region: &ClipRegion,
        inherited: Option<&Mask>,
    ) -> Result<Mask, CompositeError> {
        self.check()?;
        self.budget.reserve(self.pixels)?;
        if self.track_initialization {
            self.budget.write(self.pixels)?;
        }
        let mut mask = Mask::from_vec(
            bytes(self.pixels)?,
            tiny_skia::IntSize::from_wh(self.width, self.height).ok_or(CompositeError::TooLarge)?,
        )
        .ok_or(CompositeError::AllocationFailed)?;
        let shape = Shape::Rect(region.local_rect());
        let Some(path) = crate::vector::path(&shape)? else {
            return Ok(mask);
        };
        let a = self.local_to_layer(region.world_transform)
            * Affine::translation(region.local_bounds.origin.x, region.local_bounds.origin.y);
        if a.inverse().is_none() || transform(a).invert().is_none_or(|t| !t.is_finite()) {
            return Err(CompositeError::UnsupportedTransform);
        }
        for p in path.points() {
            let q = a.transform_point(Point::new(p.x, p.y));
            if !q.is_finite() || q.x.abs() > 1_000_000. || q.y.abs() > 1_000_000. {
                return Err(CompositeError::UnsupportedTransform);
            }
        }
        self.budget.write(self.pixels)?;
        if let Some(inherited) = inherited {
            // Copy, temporary fill and intersection each touch the surface.
            self.budget.write(self.pixels * 2)?;
            mask.data_mut().copy_from_slice(inherited.data());
            // tiny-skia intersect_path has a temporary one-byte-per-pixel mask.
            self.budget.reserve(self.pixels)?;
            if self.track_initialization {
                self.budget.write(self.pixels)?;
            }
            mask.intersect_path(&path, tiny_skia::FillRule::Winding, true, transform(a));
            self.budget.in_use -= self.pixels;
        } else {
            mask.fill_path(&path, tiny_skia::FillRule::Winding, true, transform(a));
        }
        Ok(mask)
    }
    fn clip(&mut self, output: &mut Pixmap, source: &DomainSource) -> Result<(), CompositeError> {
        if source.clips.is_empty() {
            return Ok(());
        }
        self.budget.reserve(self.pixels)?;
        if self.track_initialization {
            self.budget.write(self.pixels)?;
        }
        let mut mask = Mask::from_vec(
            bytes(self.pixels)?,
            tiny_skia::IntSize::from_wh(self.width, self.height).ok_or(CompositeError::TooLarge)?,
        )
        .ok_or(CompositeError::AllocationFailed)?;
        for (index, clip) in source.clips.iter().enumerate() {
            let shape = Shape::Rect(clip.local_rect());
            let path = crate::vector::path(&shape)?
                .ok_or_else(|| CompositeError::InvalidGeometry("empty clip".into()))?;
            let a = self.local_to_layer(clip.world_transform)
                * Affine::translation(clip.local_bounds.origin.x, clip.local_bounds.origin.y);
            if a.inverse().is_none() || transform(a).invert().is_none_or(|t| !t.is_finite()) {
                return Err(CompositeError::UnsupportedTransform);
            }
            for p in path.points() {
                let q = a.transform_point(Point::new(p.x, p.y));
                if !q.is_finite() || q.x.abs() > 1_000_000. || q.y.abs() > 1_000_000. {
                    return Err(CompositeError::UnsupportedTransform);
                }
            }
            self.check()?;
            self.budget.write(self.pixels)?;
            if index == 0 {
                mask.fill_path(&path, tiny_skia::FillRule::Winding, true, transform(a));
            } else {
                // intersect_path fills a temporary mask and intersects it.
                self.budget.write(self.pixels)?;
                self.budget.reserve(self.pixels)?;
                if self.track_initialization {
                    self.budget.write(self.pixels)?;
                }
                mask.intersect_path(&path, tiny_skia::FillRule::Winding, true, transform(a));
                self.budget.in_use -= self.pixels;
            }
        }
        self.budget.write(self.pixels)?;
        output.apply_mask(&mask);
        self.budget.in_use -= self.pixels;
        Ok(())
    }
}
fn intersect(a: [f64; 4], b: [f64; 4]) -> Option<[f64; 4]> {
    let r = [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ];
    (r[2] > r[0] && r[3] > r[1]).then_some(r)
}
fn bounds(b: Bounds) -> [f64; 4] {
    [
        b.origin.x as f64,
        b.origin.y as f64,
        b.origin.x as f64 + b.size.width as f64,
        b.origin.y as f64 + b.size.height as f64,
    ]
}
pub(crate) fn transformed(b: [f64; 4], a: Affine) -> [f64; 4] {
    let mut r = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (x, y) in [(b[0], b[1]), (b[2], b[1]), (b[2], b[3]), (b[0], b[3])] {
        let (x, y) = (
            a.a as f64 * x + a.c as f64 * y + a.tx as f64,
            a.b as f64 * x + a.d as f64 * y + a.ty as f64,
        );
        r[0] = r[0].min(x);
        r[1] = r[1].min(y);
        r[2] = r[2].max(x);
        r[3] = r[3].max(y);
    }
    r
}
/// Rasterize atomically on a worker. The caller may publish only the returned
/// complete frame. On error all temporary surfaces are released; work already
/// performed still counts toward this request's cumulative budget.
pub fn rasterize(
    source: &DomainSource,
    viewport: RasterViewport,
    budget: &mut PixelBudget,
    cancelled: &AtomicBool,
) -> Result<Option<CpuFrame>, CompositeError> {
    rasterize_with_images(source, &ImageInputs::default(), viewport, budget, cancelled)
}
/// Rasterize with current static pixels. Inputs share the explicit working-set
/// budget with retained outputs and all temporary raster surfaces.
pub fn rasterize_with_images(
    source: &DomainSource,
    images: &ImageInputs,
    viewport: RasterViewport,
    budget: &mut PixelBudget,
    cancelled: &AtomicBool,
) -> Result<Option<CpuFrame>, CompositeError> {
    rasterize_with_inputs(
        source,
        images,
        &TextInputs::default(),
        viewport,
        budget,
        cancelled,
    )
}
/// Rasterize the same complete domain with immutable image AND text ink inputs.
/// All input buffers, masks, intermediate layers and outputs share one budget.
pub fn rasterize_with_inputs(
    source: &DomainSource,
    images: &ImageInputs,
    texts: &TextInputs,
    viewport: RasterViewport,
    budget: &mut PixelBudget,
    cancelled: &AtomicBool,
) -> Result<Option<CpuFrame>, CompositeError> {
    rasterize_with_custom_inputs(
        source,
        images,
        texts,
        &CustomInputs::default(),
        viewport,
        budget,
        cancelled,
    )
}
/// Rasterize a complete domain with explicitly supplied host Custom pixels.
/// Inputs and retained outputs share the same cumulative surface/work budget.
pub fn rasterize_with_custom_inputs(
    source: &DomainSource,
    images: &ImageInputs,
    texts: &TextInputs,
    customs: &CustomInputs,
    viewport: RasterViewport,
    budget: &mut PixelBudget,
    cancelled: &AtomicBool,
) -> Result<Option<CpuFrame>, CompositeError> {
    let before = budget.in_use;
    budget.reserve(
        images
            .byte_len()
            .saturating_add(texts.byte_len())
            .saturating_add(customs.byte_len()),
    )?;
    let result = rasterize_inner(source, images, texts, customs, viewport, budget, cancelled);
    budget.in_use = before
        + result
            .as_ref()
            .ok()
            .and_then(|f| f.as_ref())
            .map_or(0, |f| f.premul_rgba.len() as u64);
    result
}
fn rasterize_inner(
    source: &DomainSource,
    images: &ImageInputs,
    texts: &TextInputs,
    customs: &CustomInputs,
    viewport: RasterViewport,
    budget: &mut PixelBudget,
    cancelled: &AtomicBool,
) -> Result<Option<CpuFrame>, CompositeError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(CompositeError::Superseded);
    }
    if source.root.object.eraser != EraserMode::None
        || source.root.object.style.blend_mode.requires_backdrop()
    {
        return Err(CompositeError::UnsupportedExternalBackdrop);
    }
    if !viewport.device_scale.is_finite()
        || viewport.device_scale <= 0.
        || viewport.world_to_screen.inverse().is_none()
        || !viewport.canvas_bounds.is_valid()
        || !viewport.content_mask.is_valid()
    {
        return Err(CompositeError::UnsupportedTransform);
    }
    for (_, world) in source.texts() {
        let m = viewport.world_to_screen * world;
        if m.inverse().is_none() {
            return Err(CompositeError::UnsupportedTransform);
        }
    }
    let sites = crate::background::validate_cancelled(source, cancelled)?;
    let active_background = !sites.is_empty();
    let mut prep = Preparation::with_lowered_geometry(source.lowered_geometry.clone());
    prep.background_sites = sites;
    if source.has_scale_fixed_strokes() || source.resolved_camera.is_some() {
        // A raw rotated camera matrix has the same lost f32 scale provenance
        // as rotated local columns. The supported logical camera is explicit
        // positive uniform pan/zoom; arbitrary element transforms remain valid.
        if !viewport
            .world_to_screen
            .is_positive_uniform_scale_translation()
        {
            return Err(CompositeError::UnsupportedScaleFixed(
                "fixed-stroke camera requires positive uniform scale and translation".into(),
            ));
        }
        let camera = source.logical_camera.unwrap_or(viewport.world_to_screen.a);
        if source.resolved_camera.is_some_and(|bound| bound != camera) {
            return Err(CompositeError::UnsupportedScaleFixed(
                "resolved stroke camera mismatch".into(),
            ));
        }
        prep.logical_camera = Some(camera);
    }
    let mut root = prep.node_with_custom(&source.root, budget.limits, images, texts, customs)?;
    let physical = crate::blur::PhysicalSpace {
        camera: viewport.world_to_screen,
        scale: viewport.device_scale as f64,
        logical_camera: source
            .logical_effect_camera
            .unwrap_or(viewport.world_to_screen),
    };
    let physical_effects = prep.has_blur || prep.has_shadow_modes || active_background;
    if physical_effects {
        crate::blur::resolve(&mut root, physical, budget.limits, cancelled)?;
    }
    let scale = viewport.device_scale as f64;
    let b = if source.canvas.is_some() {
        bounds(viewport.canvas_bounds)
    } else if physical_effects {
        let Some(b) = root.device.as_ref().and_then(|d| d.completed) else {
            return Ok(None);
        };
        b.map(|v| v / scale)
    } else {
        let Some(world_bounds) = prep.bounds else {
            return Ok(None);
        };
        let b = transformed(world_bounds, viewport.world_to_screen);
        let guard = if prep.has_shadows { 3. } else { 1. };
        [
            b[0] - guard / scale,
            b[1] - guard / scale,
            b[2] + guard / scale,
            b[3] + guard / scale,
        ]
    };
    let Some(mut b) = intersect(b, bounds(viewport.canvas_bounds))
        .and_then(|b| intersect(b, bounds(viewport.content_mask)))
    else {
        return Ok(None);
    };
    for clip in &source.clips {
        let cb = transformed(
            bounds(clip.local_bounds),
            viewport.world_to_screen * clip.world_transform,
        );
        if cb[2] <= cb[0] || cb[3] <= cb[1] {
            return Ok(None);
        }
        // Retain one physical pixel around ancestor paths, as for ordinary
        // geometry. Cutting the mask surface exactly at a curved path's AABB
        // changes tiny-skia's edge coverage relative to an internal clip.
        // The real mask still removes every outside pixel before presentation.
        let guard = 1. / scale;
        let clipped_bounds = [cb[0] - guard, cb[1] - guard, cb[2] + guard, cb[3] + guard];
        let Some(next) = intersect(b, clipped_bounds) else {
            return Ok(None);
        };
        b = next;
    }
    let x0 = (b[0] * scale).floor();
    let y0 = (b[1] * scale).floor();
    let x1 = (b[2] * scale).ceil();
    let y1 = (b[3] * scale).ceil();
    let w = x1 - x0;
    let h = y1 - y0;
    if [x0, y0, x1, y1, w, h].iter().any(|x| !x.is_finite())
        || x0 < i32::MIN as f64
        || y0 < i32::MIN as f64
        || x1 > i32::MAX as f64
        || y1 > i32::MAX as f64
        || w <= 0.
        || h <= 0.
        || w > budget.limits.max_dimension as f64
        || h > budget.limits.max_dimension as f64
        || w * h > budget.limits.max_pixels as f64
    {
        return Err(CompositeError::TooLarge);
    }
    let (width, height) = (w as u32, h as u32);
    let pixels = width as u64 * height as u64;
    let a = viewport.world_to_screen;
    let world_to_layer = if physical_effects {
        physical.local(Affine::IDENTITY, [x0, y0])
    } else {
        Affine::new(
            (a.a as f64 * scale) as f32,
            (a.b as f64 * scale) as f32,
            (a.c as f64 * scale) as f32,
            (a.d as f64 * scale) as f32,
            (a.tx as f64 * scale - x0) as f32,
            (a.ty as f64 * scale - y0) as f32,
        )
    };
    let final_output = if active_background {
        Some(crate::blur::SurfacePlan::new(
            [x0, y0, x1, y1],
            physical,
            budget.limits,
        )?)
    } else {
        None
    };
    let work = if active_background {
        Some(crate::blur::SurfacePlan::new(
            crate::blur::background_work(&root, [x0, y0, x1, y1]),
            physical,
            budget.limits,
        )?)
    } else {
        None
    };
    let (width, height, pixels, world_to_layer, origin) = work
        .as_ref()
        .map_or((width, height, pixels, world_to_layer, [x0, y0]), |p| {
            (p.width, p.height, p.pixels, p.world_to_layer, p.origin)
        });
    let mut raster = Raster {
        track_initialization: active_background,
        owner: source.canvas.map(|_| crate::background::Owner::Canvas),
        width,
        height,
        pixels,
        world_to_layer,
        origin,
        physical: physical_effects.then_some(physical),
        budget,
        cancelled,
    };
    if let Some(backdrop) = source.canvas {
        grid_layout(
            backdrop,
            viewport,
            origin,
            [width, height],
            prep.points,
            raster.budget.limits,
            active_background,
        )?;
    }
    if prep.has_shadows
        || prep.has_inner_shadows
        || prep.has_image_clips
        || prep.has_erasers
        || prep.has_masks
        || prep.has_grayscale
        || prep.has_blur
        || active_background
    {
        // No domain RGBA, mask, or filter scratch has been allocated yet.
        admission::admit(&raster, &root, source, final_output.as_ref())?;
    }
    let mut output = raster.surface()?;
    if let Some(backdrop) = source.canvas {
        raster.backdrop(
            &mut output,
            backdrop,
            viewport,
            origin,
            prep.points,
            active_background,
        )?;
    }
    raster.node(&root, &mut output, 0, None)?;
    if let Some(p) = final_output {
        raster.budget.reserve(p.pixels * 4)?;
        raster.budget.write(p.pixels * 2)?;
        let mut cropped = Pixmap::from_vec(
            bytes(p.pixels * 4)?,
            tiny_skia::IntSize::from_wh(p.width, p.height).ok_or(CompositeError::TooLarge)?,
        )
        .ok_or(CompositeError::AllocationFailed)?;
        let ox = (p.origin[0] - origin[0]) as usize;
        let oy = (p.origin[1] - origin[1]) as usize;
        background::copy_window(&output, &mut cropped, [ox, oy], || raster.check())?;
        let mut final_raster = Raster {
            track_initialization: true,
            owner: raster.owner.clone(),
            width: p.width,
            height: p.height,
            pixels: p.pixels,
            world_to_layer: p.world_to_layer,
            origin: p.origin,
            physical: Some(physical),
            budget: raster.budget,
            cancelled,
        };
        final_raster.clip(&mut cropped, source)?;
        final_raster.check()?;
        final_raster.budget.in_use -= pixels * 4;
        return Ok(Some(CpuFrame {
            screen_bounds: Bounds::new(
                Point::new((x0 / scale) as f32, (y0 / scale) as f32),
                Size::new((w / scale) as f32, (h / scale) as f32),
            ),
            width: p.width,
            height: p.height,
            premul_rgba: cropped.take(),
        }));
    }
    raster.clip(&mut output, source)?;
    raster.check()?;
    Ok(Some(CpuFrame {
        screen_bounds: Bounds::new(
            Point::new((x0 / scale) as f32, (y0 / scale) as f32),
            Size::new((w / scale) as f32, (h / scale) as f32),
        ),
        width,
        height,
        premul_rgba: output.take(),
    }))
}

#[cfg(test)]
mod grayscale_loop_tests {
    use super::*;
    #[test]
    fn grayscale_row_cancellation_stops_before_touching_the_next_row() {
        let mut layer = Pixmap::new(5, 4).unwrap();
        layer.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 128));
        let original = layer.data().to_vec();
        let cancelled = AtomicBool::new(false);
        let mut row = 0;
        let result = grayscale_pixels(&mut layer, 1., || {
            row += 1;
            if row == 3 {
                cancelled.store(true, Ordering::Relaxed);
            }
            if cancelled.load(Ordering::Relaxed) {
                Err(CompositeError::Superseded)
            } else {
                Ok(())
            }
        });
        assert_eq!(result, Err(CompositeError::Superseded));
        assert_eq!(row, 3);
        for pixel in layer.data()[..40].as_chunks::<4>().0 {
            assert_eq!(*pixel, [27, 27, 27, 128]);
        }
        assert_eq!(&layer.data()[40..], &original[40..]);
    }
}

#[cfg(test)]
mod blur_loop_tests {
    use super::*;
    fn source() -> Pixmap {
        let mut p = Pixmap::new(8, 8).unwrap();
        p.fill(tiny_skia::Color::from_rgba8(255, 40, 80, 128));
        p
    }
    #[test]
    fn blur_exact_f32_one_tap_transition_corrects_identity_and_preserves_nonzero_kernel() {
        let adjacent = |sigma: f32| {
            (-0.5f32 * (1f32 / sigma).powf(2.)).exp()
                * (1f32 / ((2f32 * std::f32::consts::PI).sqrt() * sigma))
        };
        let (mut lo, mut hi) = (0.001f32.to_bits(), 0.5f32.to_bits());
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if adjacent(f32::from_bits(mid)) == 0. {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let zero = f32::from_bits(lo);
        let nonzero = f32::from_bits(hi);
        assert_eq!(adjacent(zero), 0.);
        assert!(adjacent(nonzero) > 0.);
        assert_eq!(nonzero.to_bits(), zero.to_bits() + 1);
        // Distinct first row and horizontal interior expose both image bugs.
        let mut input = source();
        input.data_mut()[0..4].copy_from_slice(&[7, 3, 2, 11]);
        let corrected = blur_pixels(input.clone(), zero, || Ok(())).unwrap();
        assert_eq!(corrected.data(), input.data());
        let raw = image::RgbaImage::from_raw(8, 8, input.data().to_vec()).unwrap();
        let raw_zero = image::imageops::blur(&raw, zero);
        assert_ne!(raw_zero.as_raw(), input.data());
        let expected = image::imageops::blur(&raw, nonzero);
        let actual = blur_pixels(input, nonzero, || Ok(())).unwrap();
        assert_eq!(actual.data(), expected.as_raw());
    }
    #[test]
    fn blur_checks_cancellation_before_after_gaussian_and_each_output_row() {
        for stop in [1, 2, 3, 7] {
            let mut count = 0;
            let result = blur_pixels(source(), 1., || {
                count += 1;
                if count == stop {
                    Err(CompositeError::Superseded)
                } else {
                    Ok(())
                }
            });
            assert!(matches!(result, Err(CompositeError::Superseded)));
            assert_eq!(count, stop);
        }
        let mut count = 0;
        assert!(matches!(
            blur_pixels(source(), 0.001, || {
                count += 1;
                if count == 2 {
                    Err(CompositeError::Superseded)
                } else {
                    Ok(())
                }
            }),
            Err(CompositeError::Superseded)
        ));
        assert_eq!(count, 2);
    }
}

#[cfg(test)]
mod a8_shadow_transition_tests {
    use super::*;
    #[test]
    fn a8_tiny_sigma_identity_regression() {
        let input =
            image::GrayImage::from_raw(8, 8, (0..64).map(|n| (n * 3 + 1) as u8).collect()).unwrap();
        // Retain the locked dependency defect as a discriminating oracle.
        let wrong = image::imageops::blur(&input, 0.001);
        assert_eq!(wrong.get_pixel(3, 3)[0], input.get_pixel(2, 3)[0]);
        assert_eq!(wrong.get_pixel(3, 0)[0], 0);
        let output = blur_alpha(input.clone(), 0.001, || Ok(())).unwrap();
        assert_eq!(output, input);
        let (mut lo, mut hi) = (0.001f32.to_bits(), 0.5f32.to_bits());
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if one_tap_gaussian(f32::from_bits(mid)) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        assert_eq!(hi, lo + 1);
        assert_eq!(
            blur_alpha(input.clone(), f32::from_bits(lo), || Ok(())).unwrap(),
            input
        );
        assert_eq!(
            blur_alpha(input.clone(), f32::from_bits(hi), || Ok(())).unwrap(),
            image::imageops::blur(&input, f32::from_bits(hi))
        );
        let mut checks = 0;
        assert!(matches!(
            blur_alpha(input, 0.001, || {
                checks += 1;
                if checks == 2 {
                    Err(CompositeError::Superseded)
                } else {
                    Ok(())
                }
            }),
            Err(CompositeError::Superseded)
        ));
        assert_eq!(checks, 2);
    }
}

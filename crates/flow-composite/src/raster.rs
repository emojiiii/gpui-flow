mod admission;
use crate::{
    CanvasBackdrop, CompositeError, CompositeLimits, DomainSource, ImageInputs, TextInputs,
    vector::{Preparation, PreparedNode, VectorPaint, transform},
};
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, ClipRegion, MaskMode, Point, Shape, Size, display::Rect,
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
struct Raster<'a> {
    width: u32,
    height: u32,
    pixels: u64,
    world_to_layer: Affine,
    budget: &'a mut PixelBudget,
    cancelled: &'a AtomicBool,
}
impl Raster<'_> {
    fn backdrop(
        &mut self,
        target: &mut Pixmap,
        backdrop: CanvasBackdrop,
        viewport: RasterViewport,
        origin: [f64; 2],
        prepared_points: usize,
    ) -> Result<(), CompositeError> {
        self.check()?;
        self.budget.write(self.pixels)?;
        let c = backdrop.color;
        target.fill(tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a));
        let Some(grid) = backdrop.grid.filter(|g| g.color.a != 0) else {
            return Ok(());
        };
        let scale = viewport.device_scale as f64;
        let gap = grid.spacing as f64;
        let canvas = bounds(viewport.canvas_bounds);
        let start = [
            canvas[0] + (grid.offset.x as f64).rem_euclid(gap),
            canvas[1] + (grid.offset.y as f64).rem_euclid(gap),
        ];
        // Restrict generation to the actual allocated visible crop, while
        // retaining the original canvas grid anchor after scrolling/clipping.
        let range = |axis: usize, pixels: u32| {
            let low = origin[axis] / scale;
            let high = (origin[axis] + pixels as f64) / scale;
            let first = ((low - start[axis] - 1.) / gap).ceil().max(0.);
            let end = ((high.min(canvas[axis + 2]) - start[axis]) / gap)
                .ceil()
                .max(first);
            (first, end)
        };
        let (x0, x1) = range(0, self.width);
        let (y0, y1) = range(1, self.height);
        let dots = (x1 - x0) * (y1 - y0);
        if !dots.is_finite()
            || dots * 4. + prepared_points as f64 > self.budget.limits.max_total_points as f64
        {
            return Err(CompositeError::InputLimit);
        }
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
        if let Some(path) = path.finish() {
            self.budget.write(self.pixels)?;
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
        blend: BlendMode,
        clip: Option<&Mask>,
    ) -> Result<(), CompositeError> {
        self.check()?;
        self.budget.write(self.pixels)?;
        if crate::nonseparable::is_nonseparable(blend) {
            for (index, (dst, src)) in target
                .data_mut()
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(source.data().as_chunks::<4>().0.iter())
                .enumerate()
            {
                if index % self.width as usize == 0 {
                    self.check()?;
                }
                crate::nonseparable::pixel(
                    dst,
                    src,
                    opacity,
                    clip.map_or(255, |mask| mask.data()[index]),
                    blend,
                );
            }
            return Ok(());
        }
        target.draw_pixmap(
            0,
            0,
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
        if node.bounds.is_none_or(|b| {
            let b = transformed(b, self.world_to_layer);
            b[2] < -1.
                || b[3] < -1.
                || b[0] > self.width as f64 + 1.
                || b[1] > self.height as f64 + 1.
        }) {
            return Ok(());
        }
        let isolated = !node.shadows.is_empty()
            || !node.inner_shadows.is_empty()
            || node.opacity < 1.
            || node.blend != BlendMode::PassThrough;
        if isolated {
            if depth >= self.budget.limits.max_depth {
                return Err(CompositeError::NestingLimit);
            }
            let mut layer = self.surface()?;
            self.contents(node, &mut layer, depth + 1, None, true)?;
            self.composite(target, &layer, node.opacity, node.blend, clip)?;
            self.release(layer);
        } else {
            self.contents(node, target, depth, clip, true)?;
        }
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
            for &shadow in &node.shadows {
                self.shadow(node, shadow, target, depth, false)?;
            }
        }
        let (before, after) = node.paints.split_at(node.before_children);
        self.paints(before, target, clip)?;
        if let Some(p) = &node.image {
            self.check()?;
            let a = self.world_to_layer * p.world;
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
                    tiny_skia::FilterQuality::Bilinear,
                    1.,
                    transform(p.texture_to_local),
                ),
                ..Default::default()
            };
            self.budget.write(self.pixels)?;
            target.fill_path(
                &p.path,
                &paint,
                tiny_skia::FillRule::Winding,
                transform(a),
                clip,
            );
        }
        if let Some(region) = &node.child_clip {
            if !node.children.is_empty() {
                let child_clip = self.child_clip(region, clip)?;
                self.children(node, target, depth, Some(&child_clip))?;
                self.budget.in_use -= self.pixels;
            }
        } else {
            self.children(node, target, depth, clip)?;
        }
        // Restore the ancestor clip after children. The container's child clip
        // must never cut away its own outside/centered stroke.
        if draw_shadow {
            for &shadow in &node.inner_shadows {
                self.shadow(node, shadow, target, depth, true)?;
            }
        }
        self.paints(after, target, clip)?;
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
            let a = self.world_to_layer * p.world;
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
            target.fill_path(&p.path, &p.paint, p.rule, transform(a), clip);
        }
        Ok(())
    }
    fn shadow(
        &mut self,
        node: &PreparedNode,
        shadow: crate::shadow::PreparedShadow,
        target: &mut Pixmap,
        depth: usize,
        inner: bool,
    ) -> Result<(), CompositeError> {
        let plan = crate::shadow::SourcePlan::new(shadow, self.world_to_layer, self.budget.limits)?;
        let (width, height, pixels) = (plan.width, plan.height, plan.pixels);
        let [x0, y0] = plan.origin;
        let filter = &plan.filter;
        let scratch_bytes = filter.bytes(pixels, width);
        let filter_work = filter.work(pixels) + if inner { pixels * 2 } else { 0 };
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
            width,
            height,
            pixels,
            world_to_layer: plan.world_to_layer,
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
        let alpha = if filter.sigma == 0. {
            alpha
        } else {
            image::imageops::blur(&alpha, filter.sigma)
        };
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
    fn children(
        &mut self,
        node: &PreparedNode,
        target: &mut Pixmap,
        depth: usize,
        clip: Option<&Mask>,
    ) -> Result<(), CompositeError> {
        let mut i = 0;
        while i < node.children.len() {
            let child = &node.children[i];
            if child.mask == MaskMode::None {
                self.node(child, target, depth, clip)?;
                i += 1;
                continue;
            }
            let mask_node = child;
            let mut mask_surface = self.surface()?;
            self.node(mask_node, &mut mask_surface, depth, None)?;
            self.budget.reserve(self.pixels)?;
            let mut alpha = bytes(self.pixels)?;
            self.budget.write(self.pixels)?;
            for (row, input) in alpha
                .chunks_mut(self.width as usize)
                .zip(mask_surface.data().chunks(self.width as usize * 4))
            {
                self.check()?;
                for (a, p) in row.iter_mut().zip(input.as_chunks::<4>().0) {
                    *a = p[3];
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
                let flush = mask_node.opacity == 1. && c.blend != BlendMode::PassThrough;
                if flush {
                    self.masked(target, &mut content, &mask, BlendMode::Normal, clip)?;
                }
                self.node(c, &mut content, depth, None)?;
                if flush {
                    self.masked(target, &mut content, &mask, c.blend, clip)?;
                }
                i += 1;
            }
            self.masked(target, &mut content, &mask, BlendMode::Normal, clip)?;
            self.release(content);
            self.budget.in_use -= self.pixels;
            drop(mask);
        }
        Ok(())
    }
    fn masked(
        &mut self,
        target: &mut Pixmap,
        content: &mut Pixmap,
        mask: &Mask,
        blend: BlendMode,
        clip: Option<&Mask>,
    ) -> Result<(), CompositeError> {
        self.check()?;
        self.budget.write(self.pixels)?;
        // Exact common dimensions/origin are guaranteed by this raster owner.
        content.apply_mask(mask);
        self.composite(target, content, 1., blend, clip)?;
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
        let mut mask = Mask::from_vec(
            bytes(self.pixels)?,
            tiny_skia::IntSize::from_wh(self.width, self.height).ok_or(CompositeError::TooLarge)?,
        )
        .ok_or(CompositeError::AllocationFailed)?;
        let shape = Shape::Rect(
            Rect::new(
                region.local_bounds.size.width,
                region.local_bounds.size.height,
            )
            .rounded(region.corner_radius),
        );
        let Some(path) = crate::vector::path(&shape)? else {
            return Ok(mask);
        };
        let a = self.world_to_layer
            * region.world_transform
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
        let mut mask = Mask::from_vec(
            bytes(self.pixels)?,
            tiny_skia::IntSize::from_wh(self.width, self.height).ok_or(CompositeError::TooLarge)?,
        )
        .ok_or(CompositeError::AllocationFailed)?;
        for (index, clip) in source.clips.iter().enumerate() {
            let rect = Shape::Rect(
                Rect::new(clip.local_bounds.size.width, clip.local_bounds.size.height)
                    .rounded(clip.corner_radius),
            );
            let contour = rect.flatten(0.1).remove(0);
            let mut builder = PathBuilder::new();
            if let Some(p) = contour.points.first() {
                builder.move_to(p.x, p.y);
                for p in &contour.points[1..] {
                    builder.line_to(p.x, p.y);
                }
                builder.close();
            }
            let path = builder
                .finish()
                .ok_or_else(|| CompositeError::InvalidGeometry("empty clip".into()))?;
            let a = self.world_to_layer
                * clip.world_transform
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
    let before = budget.in_use;
    budget.reserve(images.byte_len().saturating_add(texts.byte_len()))?;
    let result = rasterize_inner(source, images, texts, viewport, budget, cancelled);
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
    viewport: RasterViewport,
    budget: &mut PixelBudget,
    cancelled: &AtomicBool,
) -> Result<Option<CpuFrame>, CompositeError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(CompositeError::Superseded);
    }
    if source.root.object.style.blend_mode.requires_backdrop() {
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
        if m.a <= 0. || m.d <= 0. || m.b != 0. || m.c != 0. || (m.a - m.d).abs() > m.a * 0.00001 {
            return Err(CompositeError::UnsupportedTransform);
        }
    }
    let mut prep = Preparation::default();
    let root = prep.node(&source.root, budget.limits, images, texts)?;
    let scale = viewport.device_scale as f64;
    let b = if source.canvas.is_some() {
        bounds(viewport.canvas_bounds)
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
        let Some(next) = intersect(b, cb) else {
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
    let world_to_layer = Affine::new(
        (a.a as f64 * scale) as f32,
        (a.b as f64 * scale) as f32,
        (a.c as f64 * scale) as f32,
        (a.d as f64 * scale) as f32,
        (a.tx as f64 * scale - x0) as f32,
        (a.ty as f64 * scale - y0) as f32,
    );
    let mut raster = Raster {
        width,
        height,
        pixels,
        world_to_layer,
        budget,
        cancelled,
    };
    if prep.has_shadows || prep.has_inner_shadows {
        // No domain RGBA, mask, or filter scratch has been allocated yet.
        admission::admit(&raster, &root, source)?;
    }
    let mut output = raster.surface()?;
    if let Some(backdrop) = source.canvas {
        raster.backdrop(&mut output, backdrop, viewport, [x0, y0], prep.points)?;
    }
    raster.node(&root, &mut output, 0, None)?;
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

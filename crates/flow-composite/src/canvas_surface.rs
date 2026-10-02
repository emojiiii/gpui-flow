//! Imperative, bounded Canvas pixels. All writes publish one immutable snapshot;
//! failed admission/rendering leaves both pixels and generation unchanged.
use crate::{
    CompositeError, CompositeLimits, DomainSource, ImageInputs, PixelBudget, RasterViewport,
    StaticImage, TextInputs, rasterize_with_inputs,
};
use gpui_flow_scene::{Affine, Bounds, Color, Point, Size, display::Canvas};
use std::sync::{Arc, atomic::AtomicBool};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

#[derive(Clone, Debug)]
pub struct CanvasSurface {
    config: Canvas,
    pixels: Arc<StaticImage>,
    generation: u64,
    limits: CompositeLimits,
}
impl CanvasSurface {
    pub fn new(config: Canvas, limits: CompositeLimits) -> Result<Self, CompositeError> {
        config.validate().map_err(CompositeError::InvalidGeometry)?;
        let limits = limits.bounded();
        let [w, h] = config
            .pixel_size()
            .map_err(CompositeError::InvalidGeometry)?;
        let pixels = Arc::new(StaticImage {
            pixmap: allocate(w, h, limits, 0)?,
        });
        Ok(Self {
            config,
            pixels,
            generation: 1,
            limits,
        })
    }
    pub fn config(&self) -> &Canvas {
        &self.config
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn snapshot(&self) -> Arc<StaticImage> {
        self.pixels.clone()
    }
    pub fn byte_len(&self) -> u64 {
        self.pixels.byte_len()
    }
    fn next(&self) -> Result<u64, CompositeError> {
        self.generation
            .checked_add(1)
            .ok_or(CompositeError::WorkLimit)
    }
    fn publish(&mut self, pixmap: Pixmap, generation: u64) {
        self.pixels = Arc::new(StaticImage { pixmap });
        self.generation = generation;
    }
    fn scale(&self) -> Affine {
        Affine::scale(
            self.pixels.width() as f32 / self.config.size.width,
            self.pixels.height() as f32 / self.config.size.height,
        )
    }
    fn copy(&self, retained: u64) -> Result<Pixmap, CompositeError> {
        let mut target = allocate(
            self.pixels.width(),
            self.pixels.height(),
            self.limits,
            self.byte_len().saturating_add(retained),
        )?;
        target
            .data_mut()
            .copy_from_slice(self.pixels.premultiplied_rgba());
        Ok(target)
    }
    /// Same logical size/DPR preserves pixels. A changed size resets by default;
    /// safe_resize copies the top-left physical-pixel intersection, without scaling.
    pub fn reconfigure(&mut self, config: Canvas) -> Result<bool, CompositeError> {
        config.validate().map_err(CompositeError::InvalidGeometry)?;
        if self.config == config {
            return Ok(false);
        }
        let generation = self.next()?;
        if self.config.size != config.size || self.config.pixel_ratio != config.pixel_ratio {
            let [w, h] = config
                .pixel_size()
                .map_err(CompositeError::InvalidGeometry)?;
            let mut target = allocate(w, h, self.limits, self.byte_len())?;
            if config.safe_resize {
                let count = w.min(self.pixels.width()) as usize * 4;
                for (dst, src) in target.data_mut().chunks_mut(w as usize * 4).zip(
                    self.pixels
                        .premultiplied_rgba()
                        .chunks(self.pixels.width() as usize * 4),
                ) {
                    dst[..count].copy_from_slice(&src[..count]);
                }
            }
            self.publish(target, generation);
        } else {
            self.generation = generation;
        }
        self.config = config;
        Ok(true)
    }
    /// Keep only a top-left physical-pixel rectangle without resizing. Used to
    /// honor intermediate safe resizes even when no view render occurred.
    pub fn retain_pixels(&mut self, width: u32, height: u32) -> Result<(), CompositeError> {
        if width >= self.pixels.width() && height >= self.pixels.height() {
            return Ok(());
        }
        let generation = self.next()?;
        let mut target = allocate(
            self.pixels.width(),
            self.pixels.height(),
            self.limits,
            self.byte_len(),
        )?;
        let row_bytes = self.pixels.width() as usize * 4;
        let copy_bytes = width.min(self.pixels.width()) as usize * 4;
        for (dst, src) in target
            .data_mut()
            .chunks_mut(row_bytes)
            .zip(self.pixels.premultiplied_rgba().chunks(row_bytes))
            .take(height as usize)
        {
            dst[..copy_bytes].copy_from_slice(&src[..copy_bytes]);
        }
        self.publish(target, generation);
        Ok(())
    }
    pub fn clear(&mut self) -> Result<(), CompositeError> {
        let generation = self.next()?;
        let target = allocate(
            self.pixels.width(),
            self.pixels.height(),
            self.limits,
            self.byte_len(),
        )?;
        self.publish(target, generation);
        Ok(())
    }
    /// Solid source-over fill in logical Canvas coordinates.
    pub fn fill_rect(&mut self, bounds: Bounds, color: Color) -> Result<(), CompositeError> {
        validate_bounds(bounds)?;
        let generation = self.next()?;
        let mut target = self.copy(0)?;
        let rect = tiny_skia::Rect::from_xywh(
            bounds.origin.x,
            bounds.origin.y,
            bounds.size.width,
            bounds.size.height,
        )
        .ok_or(CompositeError::InvalidImage)?;
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        target.fill_rect(rect, &paint, crate::vector::transform(self.scale()), None);
        self.publish(target, generation);
        Ok(())
    }
    /// Draw an immutable decoded frame into a logical destination rectangle.
    /// Decoding/URL authorization stays with the host; no resource is fetched.
    pub fn draw_image(
        &mut self,
        image: &StaticImage,
        destination: Bounds,
        opacity: f32,
    ) -> Result<(), CompositeError> {
        validate_bounds(destination)?;
        if !opacity.is_finite() || !(0. ..=1.).contains(&opacity) {
            return Err(CompositeError::InvalidImage);
        }
        let generation = self.next()?;
        let mut target = self.copy(image.byte_len())?;
        let matrix = self.scale()
            * Affine::translation(destination.origin.x, destination.origin.y)
            * Affine::scale(
                destination.size.width / image.width() as f32,
                destination.size.height / image.height() as f32,
            );
        target.draw_pixmap(
            0,
            0,
            image.pixmap.as_ref(),
            &PixmapPaint {
                opacity,
                quality: quality(self.config.smooth),
                ..Default::default()
            },
            crate::vector::transform(matrix),
            None,
        );
        self.publish(target, generation);
        Ok(())
    }
    /// Draw a captured subtree with its root's world transform removed, then
    /// apply a caller-supplied local-to-Canvas transform. Captured root opacity,
    /// descendants and effects are retained; ancestor opacity/clips are excluded.
    /// Missing images/text or unsupported effects reject the entire operation.
    pub fn draw(
        &mut self,
        source: &DomainSource,
        local_to_canvas: Affine,
        images: &ImageInputs,
        texts: &TextInputs,
    ) -> Result<(), CompositeError> {
        let inverse = source
            .root_transform()
            .inverse()
            .ok_or(CompositeError::UnsupportedTransform)?;
        let matrix = self.scale() * local_to_canvas * inverse;
        if matrix.inverse().is_none() {
            return Err(CompositeError::UnsupportedTransform);
        }
        let generation = self.next()?;
        let rect = Bounds::new(
            Point::ZERO,
            Size::new(self.pixels.width() as f32, self.pixels.height() as f32),
        );
        // Current + candidate surface are admitted before compositor scratch.
        let mut budget = PixelBudget::new(self.limits, self.byte_len().saturating_mul(2))?;
        budget.charge_input_work(self.pixels.width() as u64 * self.pixels.height() as u64)?;
        let frame = rasterize_with_inputs(
            source,
            images,
            texts,
            RasterViewport {
                world_to_screen: matrix,
                canvas_bounds: rect,
                content_mask: rect,
                device_scale: 1.,
            },
            &mut budget,
            &AtomicBool::new(false),
        )?;
        let Some(frame) = frame else {
            return Ok(());
        };
        let retained = images
            .byte_len()
            .saturating_add(texts.byte_len())
            .saturating_add(frame.premul_rgba.len() as u64);
        let mut target = self.copy(retained)?;
        let size = tiny_skia::IntSize::from_wh(frame.width, frame.height)
            .ok_or(CompositeError::InvalidImage)?;
        let input =
            Pixmap::from_vec(frame.premul_rgba, size).ok_or(CompositeError::InvalidImage)?;
        target.draw_pixmap(
            0,
            0,
            input.as_ref(),
            &PixmapPaint::default(),
            Transform::from_translate(frame.screen_bounds.origin.x, frame.screen_bounds.origin.y),
            None,
        );
        self.publish(target, generation);
        Ok(())
    }
}
fn validate_bounds(bounds: Bounds) -> Result<(), CompositeError> {
    if !bounds.is_valid()
        || !bounds.size.is_positive()
        || [
            bounds.origin.x,
            bounds.origin.y,
            bounds.size.width,
            bounds.size.height,
        ]
        .iter()
        .any(|n| n.abs() > 1_000_000.)
    {
        Err(CompositeError::InvalidGeometry(
            "canvas drawing rectangle".into(),
        ))
    } else {
        Ok(())
    }
}
pub(crate) fn quality(smooth: bool) -> tiny_skia::FilterQuality {
    if smooth {
        tiny_skia::FilterQuality::Bilinear
    } else {
        tiny_skia::FilterQuality::Nearest
    }
}
fn allocate(
    width: u32,
    height: u32,
    limits: CompositeLimits,
    retained: u64,
) -> Result<Pixmap, CompositeError> {
    let pixels = width as u64 * height as u64;
    let bytes = pixels * 4;
    if width == 0
        || height == 0
        || width > limits.max_dimension
        || height > limits.max_dimension
        || pixels > limits.max_pixels
    {
        return Err(CompositeError::TooLarge);
    }
    if pixels > limits.max_surface_writes {
        return Err(CompositeError::WorkLimit);
    }
    if bytes.saturating_add(retained) > limits.max_surface_bytes {
        return Err(CompositeError::WorkingSetLimit);
    }
    let mut data = Vec::new();
    data.try_reserve_exact(bytes as usize)
        .map_err(|_| CompositeError::AllocationFailed)?;
    data.resize(bytes as usize, 0);
    Pixmap::from_vec(
        data,
        tiny_skia::IntSize::from_wh(width, height).ok_or(CompositeError::InvalidImage)?,
    )
    .ok_or(CompositeError::AllocationFailed)
}

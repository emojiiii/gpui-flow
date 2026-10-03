//! Fixed world-region raster export through the owned-domain compositor, and
//! mixed SVG export preserving vector geometry with embedded producer pixels.
//! No resource loading, scene mutation, viewport capture, or filesystem effects.
use crate::{
    CanvasBackdrop, CanvasTextOutput, CanvasTextRequest, CompositeError, CompositeLimits,
    DomainSource, ImageInputs, PixelBudget, RasterViewport, StaticImage, TextInputs, owned_canvas,
    rasterize_with_inputs,
};
use gpui_flow_scene::{Affine, Bounds, ClipRegion, Color, Point, Scene, Size};
use std::{
    io::{self, Write},
    sync::atomic::{AtomicBool, Ordering},
};

/// Raster codecs from the locked dependency graph and self-contained mixed SVG.
/// WebP is VP8L lossless; no lossy WebP quality setting is offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneExportFormat {
    Png,
    /// Quality must be 1..=100, and the request background must be opaque.
    Jpeg {
        quality: u8,
    },
    WebPLossless,
    /// Self-contained mixed SVG: geometry stays vector; Image, Canvas and
    /// same-backend Text ink are individually embedded lossless PNG images.
    Svg,
}
impl SceneExportFormat {
    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg { .. } => "image/jpeg",
            Self::WebPLossless => "image/webp",
            Self::Svg => "image/svg+xml",
        }
    }
}

/// Explicit world crop, independent of any editor pan, zoom, grid or overlays.
/// Physical dimensions are ceil(world extent * scale * pixel_ratio). Geometry
/// is clipped to the exact world crop, including coverage of fractional edges.
/// PNG/WebP/SVG preserve alpha. JPEG flattens fractional crop-edge alpha onto the
/// required opaque background; there is no implicit white-background policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneExportRequest {
    pub world_bounds: Bounds,
    pub scale: f32,
    pub pixel_ratio: f32,
    pub background: Color,
    pub format: SceneExportFormat,
}

/// Complete encoded output only. The host owns any delivery and must check its
/// resource/font epoch again before publishing an asynchronously produced file.
#[derive(Debug)]
pub struct SceneExport {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub request: SceneExportRequest,
}

/// Host preparation charges, in the same units as PixelBudget. Input buffers
/// in ImageInputs/TextInputs are counted automatically, so exclude them here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneExportPreparation {
    /// Actual glyph raster pixel writes already performed for this request.
    pub pixel_work: u64,
    /// Other buffers kept alive by the host during this operation.
    pub retained_bytes: u64,
}

/// Immutable whole-scene snapshot. Visible unsupported or missing producers
/// fail the complete export, even if their geometry is outside the crop.
#[derive(Clone, Debug)]
pub struct SceneExportPlan {
    source: DomainSource,
    viewport: RasterViewport,
    request: SceneExportRequest,
    limits: CompositeLimits,
    width: u32,
    height: u32,
}
impl SceneExportPlan {
    pub fn new(
        scene: &Scene,
        request: SceneExportRequest,
        limits: CompositeLimits,
    ) -> Result<Self, CompositeError> {
        let limits = limits.bounded();
        if scene.resolved_stroke_camera().is_some() || scene.has_transfer_context() {
            return Err(CompositeError::UnsupportedScaleFixed(
                "export requires authored scene geometry".into(),
            ));
        }
        if request.format == SceneExportFormat::Svg
            && scene.nodes().any(|n| {
                n.object
                    .style
                    .strokes()
                    .any(|s| !s.scale_fixed.is_disabled())
            })
        {
            return Err(CompositeError::UnsupportedScaleFixed("SVG export".into()));
        }
        let bounds = request.world_bounds;
        if !bounds.is_valid()
            || !bounds.size.is_positive()
            || [
                bounds.min_x(),
                bounds.min_y(),
                bounds.max_x(),
                bounds.max_y(),
            ]
            .iter()
            .any(|v| v.abs() > 1_000_000.)
            || !request.scale.is_finite()
            || request.scale <= 0.
            || !request.pixel_ratio.is_finite()
            || request.pixel_ratio <= 0.
        {
            return Err(CompositeError::InvalidGeometry(
                "scene export region/scale".into(),
            ));
        }
        if let SceneExportFormat::Jpeg { quality } = request.format
            && (!(1..=100).contains(&quality) || request.background.a != 255)
        {
            return Err(CompositeError::InvalidGeometry(
                "JPEG requires quality 1..=100 and an opaque export background".into(),
            ));
        }
        let scale = f64::from(request.scale) * f64::from(request.pixel_ratio);
        let width = (f64::from(bounds.size.width) * scale).ceil();
        let height = (f64::from(bounds.size.height) * scale).ceil();
        if !width.is_finite()
            || !height.is_finite()
            || width < 1.
            || height < 1.
            || width > f64::from(limits.max_dimension)
            || height > f64::from(limits.max_dimension)
            || width * height > limits.max_pixels as f64
        {
            return Err(CompositeError::TooLarge);
        }
        let world_to_screen = Affine::new(
            scale as f32,
            0.,
            0.,
            scale as f32,
            (-f64::from(bounds.origin.x) * scale) as f32,
            (-f64::from(bounds.origin.y) * scale) as f32,
        );
        if world_to_screen.inverse().is_none() {
            return Err(CompositeError::UnsupportedTransform);
        }
        // Reuse full-scene capture/admission, including tree order, masks and
        // backdrop checks. The public owned_canvas contract remains opaque.
        // This separate export-owned surface has an explicit alpha backdrop;
        // it never captures or relies on a host/native external backdrop.
        // Resolve the logical export scale before DPR is folded into the matrix.
        let resolved = scene
            .resolved_for_view(request.scale)
            .map_err(|e| CompositeError::InvalidGeometry(e.to_string()))?;
        let mut source = owned_canvas(
            &resolved,
            CanvasBackdrop {
                color: Color::WHITE,
                grid: None,
            },
            limits,
        )?;
        source.logical_camera = Some(request.scale);
        source.logical_effect_camera = Some(Affine::scale(request.scale, request.scale));
        source.canvas = Some(CanvasBackdrop {
            color: request.background,
            grid: None,
        });
        source.clips.push(ClipRegion {
            local_bounds: bounds,
            corner_radius: 0.,
            corner_radii: None,
            world_transform: Affine::IDENTITY,
        });
        let canvas_bounds = Bounds::new(Point::ZERO, Size::new(width as f32, height as f32));
        let plan = Self {
            source,
            viewport: RasterViewport {
                world_to_screen,
                canvas_bounds,
                content_mask: canvas_bounds,
                // Fold both scale factors into the matrix so physical extents
                // are integers and never rounded a second time by the raster.
                device_scale: 1.,
            },
            request,
            limits,
            width: width as u32,
            height: height as u32,
        };
        if request.format == SceneExportFormat::Svg {
            crate::scene_svg::validate(&plan.source)?;
        } else {
            plan.encoding_limits(0)?;
        }
        Ok(plan)
    }
    /// Existing images(), canvases(), text_sources() and ids() supply the host
    /// with the exact immutable resource producers for this plan.
    pub fn source(&self) -> &DomainSource {
        &self.source
    }
    /// Physical-pixel viewport; scale and DPR are already folded into its matrix.
    pub fn viewport(&self) -> RasterViewport {
        self.viewport
    }
    pub fn limits(&self) -> CompositeLimits {
        self.limits
    }
    pub fn request(&self) -> SceneExportRequest {
        self.request
    }
    pub fn pixel_size(&self) -> [u32; 2] {
        [self.width, self.height]
    }
    fn pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
    fn image_bytes(&self) -> u64 {
        self.pixels() * 4
    }
    fn encoding_work(&self) -> u64 {
        match self.request.format {
            SceneExportFormat::Svg => 0,
            SceneExportFormat::Png => self.pixels() * 3,
            // The JPEG encoder reads full edge-padded 8x8 blocks.
            SceneExportFormat::Jpeg { .. } => {
                u64::from(self.width.div_ceil(8)) * u64::from(self.height.div_ceil(8)) * 64 * 4
            }
            SceneExportFormat::WebPLossless => self.pixels() * 8,
        }
    }
    fn codec_scratch(&self) -> u64 {
        const FIXED: u64 = 8 * 1024 * 1024;
        match self.request.format {
            SceneExportFormat::Svg => 0,
            SceneExportFormat::Png | SceneExportFormat::Jpeg { .. } => FIXED,
            // Locked image-webp 0.2.4 copies RGBA and builds an internal Vec
            // frame. Four <=15-bit Huffman codes/pixel plus header/run slack
            // fit 8*pixels+64KiB. Allow 3x for Vec growth and the straight RGBA
            // input. This is conservative admission, not a global allocator cap.
            SceneExportFormat::WebPLossless => {
                FIXED + self.image_bytes() * 2 + (self.pixels() * 8 + 65536) * 3
            }
        }
    }
    fn encoding_limits(&self, retained: u64) -> Result<CompositeLimits, CompositeError> {
        let available = self
            .limits
            .max_surface_bytes
            .checked_sub(retained)
            .ok_or(CompositeError::WorkingSetLimit)?;
        if self.image_bytes().saturating_add(self.codec_scratch()) >= available {
            return Err(CompositeError::WorkingSetLimit);
        }
        if self.encoding_work() > self.limits.max_surface_writes {
            return Err(CompositeError::WorkLimit);
        }
        Ok(CompositeLimits {
            max_surface_bytes: available,
            max_surface_writes: self.encoding_work(),
            ..self.limits
        })
    }
    /// Reuse the existing same-backend Canvas text producer contract. Other
    /// retained bytes exclude ImageInputs/TextInputs and the future output.
    /// Raster encoding work is withheld from the producer's remaining budget.
    /// SVG has no full-frame raster reservation; host work, per-producer PNG
    /// codecs and bounded XML serialization are charged cumulatively at encode.
    pub fn text_request<'a>(
        &'a self,
        images: &ImageInputs,
        retained_bytes: u64,
    ) -> Result<CanvasTextRequest<'a>, CompositeError> {
        let retained = retained_bytes.saturating_add(images.byte_len());
        if self.request.format == SceneExportFormat::Svg {
            if retained > self.limits.max_surface_bytes {
                return Err(CompositeError::WorkingSetLimit);
            }
            return Ok(CanvasTextRequest {
                source: &self.source,
                viewport: self.viewport,
                limits: self.limits,
                retained_bytes: retained,
            });
        }
        self.encoding_limits(retained)?;
        Ok(CanvasTextRequest {
            source: &self.source,
            viewport: self.viewport,
            limits: CompositeLimits {
                max_surface_writes: self.limits.max_surface_writes - self.encoding_work(),
                ..self.limits
            },
            retained_bytes: retained
                .saturating_add(self.image_bytes())
                .saturating_add(self.codec_scratch()),
        })
    }
    /// Prepare glyph ink once, then rasterize/encode atomically. The callback
    /// receives the same request shape as CanvasSurface's text commands.
    pub fn encode_with_text(
        &self,
        images: &ImageInputs,
        retained_bytes: u64,
        cancelled: &AtomicBool,
        prepare: impl FnOnce(CanvasTextRequest<'_>) -> Result<CanvasTextOutput, CompositeError>,
    ) -> Result<SceneExport, CompositeError> {
        check(cancelled)?;
        let text = prepare(self.text_request(images, retained_bytes)?)?;
        self.encode_with_inputs(
            images,
            &text.inputs,
            SceneExportPreparation {
                pixel_work: text.pixel_work,
                retained_bytes,
            },
            cancelled,
        )
    }
    /// Input pixels remain immutable. All inputs, other host retention, glyph
    /// work, compositor work and encoding allowance share the same hard limits.
    /// Codec calls are synchronous; cancellation is checked before/after them
    /// and on external writes, not inside a codec's private computation.
    pub fn encode_with_inputs(
        &self,
        images: &ImageInputs,
        texts: &TextInputs,
        preparation: SceneExportPreparation,
        cancelled: &AtomicBool,
    ) -> Result<SceneExport, CompositeError> {
        check(cancelled)?;
        if self.request.format == SceneExportFormat::Svg {
            return crate::scene_svg::encode(self, images, texts, preparation, cancelled);
        }
        let retained = preparation
            .retained_bytes
            .saturating_add(images.byte_len())
            .saturating_add(texts.byte_len());
        let encoding_limits = self.encoding_limits(retained)?;
        let mut budget = PixelBudget::new(self.limits, preparation.retained_bytes)?;
        budget.charge_input_work(preparation.pixel_work)?;
        budget.charge_input_work(self.encoding_work())?;
        let frame = rasterize_with_inputs(
            &self.source,
            images,
            texts,
            self.viewport,
            &mut budget,
            cancelled,
        )?
        .ok_or_else(|| CompositeError::InvalidGeometry("empty export frame".into()))?;
        if frame.width != self.width
            || frame.height != self.height
            || frame.screen_bounds.origin != Point::ZERO
        {
            return Err(CompositeError::InvalidGeometry(
                "export frame extent mismatch".into(),
            ));
        }
        // Zero-copy transfer of the complete compositor output into the
        // existing immutable image/PNG API, never another rendering path.
        let image = StaticImage::from_premultiplied_rgba(
            frame.width,
            frame.height,
            frame.premul_rgba,
            self.limits,
        )?;
        check(cancelled)?;
        let bytes = match self.request.format {
            SceneExportFormat::Png => image.encode_png(encoding_limits)?,
            format => self.encode_codec(&image, format, encoding_limits, cancelled)?,
        };
        check(cancelled)?;
        Ok(SceneExport {
            bytes,
            width: self.width,
            height: self.height,
            request: self.request,
        })
    }
    fn encode_codec(
        &self,
        image: &StaticImage,
        format: SceneExportFormat,
        limits: CompositeLimits,
        cancelled: &AtomicBool,
    ) -> Result<Vec<u8>, CompositeError> {
        let available = limits.max_surface_bytes - self.image_bytes() - self.codec_scratch();
        let maximum = available.min(self.image_bytes() * 2 + 65536) as usize;
        let mut output = ExportOutput {
            bytes: Vec::new(),
            maximum,
            error: None,
            cancelled,
        };
        let result = match format {
            SceneExportFormat::Jpeg { quality } => {
                let view = OpaqueImage {
                    image,
                    background: self.request.background,
                };
                ::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, quality)
                    .encode_image(&view)
            }
            SceneExportFormat::WebPLossless => {
                let rgba = image.read_pixels(0, 0, self.width, self.height, limits)?;
                check(cancelled)?;
                ::image::codecs::webp::WebPEncoder::new_lossless(&mut output).encode(
                    rgba.rgba(),
                    self.width,
                    self.height,
                    ::image::ExtendedColorType::Rgba8,
                )
            }
            SceneExportFormat::Png => unreachable!("PNG uses StaticImage::encode_png"),
            SceneExportFormat::Svg => unreachable!("SVG uses scene_svg::encode"),
        };
        if let Some(error) = output.error {
            return Err(error);
        }
        result.map_err(|e| CompositeError::ImageEncoding(e.to_string()))?;
        Ok(output.bytes)
    }
}
fn check(cancelled: &AtomicBool) -> Result<(), CompositeError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(CompositeError::Superseded)
    } else {
        Ok(())
    }
}
/// JPEG's streaming GenericImageView avoids allocating another full RGB image.
struct OpaqueImage<'a> {
    image: &'a StaticImage,
    background: Color,
}
impl ::image::GenericImageView for OpaqueImage<'_> {
    type Pixel = ::image::Rgb<u8>;
    fn dimensions(&self) -> (u32, u32) {
        (self.image.width(), self.image.height())
    }
    fn get_pixel(&self, x: u32, y: u32) -> Self::Pixel {
        let p = self
            .image
            .pixmap
            .pixel(x, y)
            .expect("validated JPEG image extent");
        let flatten = |channel: u8, background: u8| {
            (u16::from(channel) + (u16::from(background) * u16::from(255 - p.alpha()) + 127) / 255)
                .min(255) as u8
        };
        ::image::Rgb([
            flatten(p.red(), self.background.r),
            flatten(p.green(), self.background.g),
            flatten(p.blue(), self.background.b),
        ])
    }
}
struct ExportOutput<'a> {
    bytes: Vec<u8>,
    maximum: usize,
    error: Option<CompositeError>,
    cancelled: &'a AtomicBool,
}
impl Write for ExportOutput<'_> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let needed = self.bytes.len().saturating_add(input.len());
        // JPEG emits many tiny writes. Grow geometrically but never reserve
        // beyond the admitted output cap, avoiding a reallocation per byte.
        let capacity = needed
            .max(self.bytes.capacity().saturating_mul(2))
            .max(4096)
            .min(self.maximum);
        let error = if self.cancelled.load(Ordering::Relaxed) {
            Some(CompositeError::Superseded)
        } else if needed > self.maximum {
            Some(CompositeError::WorkingSetLimit)
        } else if needed > self.bytes.capacity()
            && self
                .bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .is_err()
        {
            Some(CompositeError::AllocationFailed)
        } else {
            None
        };
        if let Some(error) = error {
            self.error = Some(error);
            return Err(io::Error::other("scene export output rejected"));
        }
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

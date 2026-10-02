//! Explicit same-platform raster access. No font lookup, shaping, or fallback replacement.
use crate::{TextMeasureRequest, gpui_overflow::shape_text_in_box};
use gpui::{
    Application, Bounds, DevicePixels, IsZero, Platform, PlatformTextSystem, RenderGlyphParams,
    TextAlign, TextSystem, WindowTextSystem, point,
};
use std::{
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const MAX_GLYPHS: usize = 8192;
const MAX_CACHE_ENTRIES: usize = 256;
const MAX_CACHE_BYTES: usize = 4 * 1024 * 1024;
const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_PIXELS: usize = 4_194_304;
const MAX_WRITES: u64 = 32_000_000;

/// Raster-logical plain-text paint input. Uniform producers use screen metrics;
/// affine producers choose an isotropic local raster space. Font size, line
/// height, bounds, clip and wrap width must all use that same space.
#[derive(Clone, Debug)]
pub struct TextRasterRequest {
    pub text: TextMeasureRequest,
    pub align: TextAlign,
    pub vertical_align: crate::VerticalAlign,
    pub overflow: crate::TextOverflow,
    pub decoration: crate::TextDecoration,
    /// Full alignment/clip box, before viewport clipping: [x, y, width, height].
    pub bounds: [f32; 4],
    /// Viewport/host intersection in the chosen raster space: [x, y, width, height].
    pub clip: [f32; 4],
    pub device_scale: f32,
    /// Straight RGBA. Emoji preserve their RGB and use this alpha.
    pub color: [u8; 4],
}
/// A clipped text box containing actual glyph ink, never a rectangle substitute.
#[derive(Debug)]
pub struct RasterizedText {
    pub width: u32,
    pub height: u32,
    /// Absolute device-pixel origin. Divide by device_scale for raster-space coordinates.
    pub origin: [i32; 2],
    pub premul_rgba: Vec<u8>,
    pub glyphs: usize,
    pub color_glyphs: usize,
    pub rows: usize,
    /// Includes the returned bitmap, Flow's job-local glyph cache and scratch.
    pub peak_bytes: usize,
    pub pixel_writes: u64,
}

/// Retains the exact raster backend used by the Application's native TextSystem.
///
/// Construct with `application`, then explicitly install on each SceneView.
/// Existing App/Window instances cannot expose their private raster backend.
/// PlatformTextSystem is Send + Sync; only these Arc handles enter workers.
/// GPUI/platform font and shaping caches remain owned/accounted by the host.
pub struct GpuiTextRasterizer {
    shaping: Arc<TextSystem>,
    raster: Arc<dyn PlatformTextSystem>,
    epoch: AtomicU64,
}
impl GpuiTextRasterizer {
    /// Capture the platform's text-system Arc before passing that SAME platform
    /// to Application. Locked GPUI desktop platforms return a clone of that Arc.
    /// No independent text system is created and FontId/GlyphId never cross backends.
    pub fn application(platform: Rc<dyn Platform>) -> (Application, Arc<Self>) {
        let raster = platform.text_system();
        let app = Application::with_platform(platform);
        let bridge = Arc::new(Self {
            shaping: app.text_system(),
            raster,
            epoch: AtomicU64::new(0),
        });
        (app, bridge)
    }
    pub fn matches_text_system(&self, system: &Arc<TextSystem>) -> bool {
        Arc::ptr_eq(&self.shaping, system)
    }
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    /// Invalidate outstanding snapshots after host font changes. Notify every
    /// consuming view (SceneView::invalidate_text_resources does both).
    pub fn invalidate(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
    }
    pub fn rasterize(
        &self,
        request: &TextRasterRequest,
        epoch: u64,
        cancelled: &AtomicBool,
        max_bytes: usize,
        remaining_work: &mut u64,
    ) -> Result<Option<RasterizedText>, String> {
        if cfg!(target_family = "wasm") {
            return Err("text raster bridge requires a native desktop platform".into());
        }
        catch_unwind(AssertUnwindSafe(|| {
            self.rasterize_inner(request, epoch, cancelled, max_bytes, remaining_work)
        }))
        .map_err(|_| "native text rasterization panicked".to_string())?
    }
    fn check(&self, epoch: u64, cancelled: &AtomicBool) -> Result<(), String> {
        if self.epoch() != epoch || cancelled.load(Ordering::Relaxed) {
            Err("text snapshot superseded".into())
        } else {
            Ok(())
        }
    }
    fn rasterize_inner(
        &self,
        r: &TextRasterRequest,
        epoch: u64,
        cancelled: &AtomicBool,
        max_bytes: usize,
        remaining_work: &mut u64,
    ) -> Result<Option<RasterizedText>, String> {
        self.check(epoch, cancelled)?;
        r.text.validate()?;
        if !r.device_scale.is_finite()
            || r.device_scale <= 0.
            || r.text.font_size * r.device_scale > 512.
            || r.bounds
                .iter()
                .chain(&r.clip)
                .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
            || r.bounds[2..].iter().chain(&r.clip[2..]).any(|v| *v <= 0.)
        {
            return Err(
                "unsupported text raster dimensions (font at most 512 device pixels)".into(),
            );
        }
        let Some([x0, y0, x1, y1]) = clipped_box(r)? else {
            return Ok(None);
        };
        let width = (x1 - x0) as usize;
        let height = (y1 - y0) as usize;
        let pixels = width.checked_mul(height).ok_or("text bitmap too large")?;
        if width > 4096 || height > 4096 || pixels > MAX_PIXELS {
            return Err("text bitmap too large".into());
        }
        let output_bytes = pixels.checked_mul(4).ok_or("text bitmap too large")?;
        let max_bytes = max_bytes.min(MAX_BYTES);
        if output_bytes > max_bytes {
            return Err("text working-set limit".into());
        }
        charge_work(remaining_work, pixels as u64)?;
        let mut out = RasterizedText {
            width: width as u32,
            height: height as u32,
            origin: [x0, y0],
            premul_rgba: allocate(output_bytes)?,
            glyphs: 0,
            color_glyphs: 0,
            rows: 0,
            peak_bytes: output_bytes,
            pixel_writes: pixels as u64,
        };
        let color = gpui::Rgba {
            r: r.color[0] as f32 / 255.,
            g: r.color[1] as f32 / 255.,
            b: r.color[2] as f32 / 255.,
            a: r.color[3] as f32 / 255.,
        }
        .into();
        let system = WindowTextSystem::new(self.shaping.clone());
        let shaped = shape_text_in_box(
            &system,
            &r.text,
            color,
            r.overflow,
            r.bounds[2],
            r.bounds[3],
        )?;
        // Flow-owned paragraph/ellipsis offsets stay live for the whole raster
        // job, so account them beside bitmap and decoration storage.
        let offset_bytes = shaped.line_offsets.capacity() * std::mem::size_of::<f32>();
        let shape_bytes = output_bytes
            .checked_add(offset_bytes)
            .ok_or("text working-set limit")?;
        if shape_bytes > max_bytes {
            return Err("text working-set limit".into());
        }
        let decorations = crate::gpui_decoration::layout_decorations(
            &shaped,
            &crate::gpui_decoration::DecorationRequest {
                decoration: r.decoration,
                font_size: r.text.font_size,
                line_height: r.text.line_height,
                bounds: r.bounds,
                align: r.align,
                vertical_align: r.vertical_align,
                device_scale: r.device_scale,
            },
            max_bytes.saturating_sub(shape_bytes),
        )?;
        let base_bytes = shape_bytes + decorations.allocated_bytes;
        out.peak_bytes = base_bytes;
        let line_offsets = shaped.line_offsets;
        let lines = shaped.lines;
        let count: usize = lines
            .iter()
            .flat_map(|l| &l.unwrapped_layout.runs)
            .map(|run| run.glyphs.len())
            .sum();
        if count > MAX_GLYPHS {
            return Err("text glyph limit (8192)".into());
        }
        let mut cache: HashMap<RenderGlyphParams, Arc<GlyphBitmap>> = HashMap::new();
        let mut cache_bytes = 0;
        let block_height = shaped.block_height;
        let block_y = r.bounds[1] + r.vertical_align.offset(r.bounds[3], block_height);
        for (line, offset) in lines.into_iter().zip(line_offsets) {
            let line_y = block_y + offset;
            let layout = &line.unwrapped_layout;
            let baseline =
                (r.text.line_height - f32::from(layout.ascent) - f32::from(layout.descent)) / 2.
                    + f32::from(layout.ascent);
            let mut wraps = line.wrap_boundaries.iter().peekable();
            let aligned = |start: f32, next: Option<&&gpui::WrapBoundary>| {
                let end = next.map_or(f32::from(layout.width), |w| {
                    f32::from(layout.runs[w.run_ix].glyphs[w.glyph_ix].position.x)
                });
                aligned_x(r.bounds[0], r.bounds[2], end - start, r.align)
            };
            let mut x = aligned(0., wraps.peek());
            let mut y = line_y;
            let mut previous_x = 0.;
            for (run_ix, run) in layout.runs.iter().enumerate() {
                for (glyph_ix, glyph) in run.glyphs.iter().enumerate() {
                    self.check(epoch, cancelled)?;
                    let glyph_x = f32::from(glyph.position.x);
                    x += glyph_x - previous_x;
                    if wraps.peek() == Some(&&gpui::WrapBoundary { run_ix, glyph_ix }) {
                        wraps.next();
                        x = aligned(glyph_x, wraps.peek());
                        y += r.text.line_height;
                    }
                    previous_x = glyph_x;
                    let origin = [
                        x * r.device_scale,
                        (y + baseline + f32::from(glyph.position.y)) * r.device_scale,
                    ];
                    if origin
                        .iter()
                        .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
                    {
                        return Err("invalid native glyph position".into());
                    }
                    let (integer, variant) = quantize(origin, glyph.is_emoji);
                    let params = RenderGlyphParams {
                        font_id: run.font_id,
                        glyph_id: glyph.id,
                        font_size: layout.font_size,
                        subpixel_variant: point(variant[0], variant[1]),
                        scale_factor: r.device_scale,
                        is_emoji: glyph.is_emoji,
                        subpixel_rendering: false,
                        dilation: if glyph.is_emoji {
                            0
                        } else {
                            self.raster.glyph_dilation_for_color(color)
                        },
                    };
                    let bitmap = if let Some(found) = cache.get(&params) {
                        found.clone()
                    } else {
                        let bounds = self
                            .raster
                            .glyph_raster_bounds(&params)
                            .map_err(|e| e.to_string())?;
                        let estimate = glyph_capacity(bounds)?;
                        if cache.len() >= MAX_CACHE_ENTRIES
                            || cache_bytes + estimate > MAX_CACHE_BYTES
                            || base_bytes + cache_bytes + estimate > max_bytes
                        {
                            cache.clear();
                            cache_bytes = 0;
                        }
                        if base_bytes + estimate > max_bytes {
                            return Err("text working-set limit".into());
                        }
                        out.peak_bytes = out.peak_bytes.max(base_bytes + cache_bytes + estimate);
                        let raster_work = if bounds.is_zero() {
                            0
                        } else {
                            (estimate / 4) as u64
                        };
                        charge_work(remaining_work, raster_work)?;
                        out.pixel_writes = out.pixel_writes.saturating_add(raster_work);
                        let bitmap = Arc::new(if bounds.is_zero() {
                            GlyphBitmap::empty(bounds)
                        } else {
                            let (size, data) = self
                                .raster
                                .rasterize_glyph(&params, bounds)
                                .map_err(|e| e.to_string())?;
                            GlyphBitmap::new(
                                bounds,
                                size.width.0,
                                size.height.0,
                                data,
                                glyph.is_emoji,
                            )?
                        });
                        if bitmap.data.len() > estimate {
                            return Err("native glyph exceeded admitted bounds".into());
                        }
                        if bitmap.data.len() <= MAX_CACHE_BYTES {
                            cache_bytes += bitmap.data.len();
                            cache.insert(params, bitmap.clone());
                        }
                        bitmap
                    };
                    out.glyphs += 1;
                    if bitmap.color {
                        out.color_glyphs += 1;
                    }
                    paint_bitmap(&mut out, &bitmap, integer, r.color, remaining_work)?;
                }
            }
            let rows = line.wrap_boundaries.len() + 1;
            out.rows += rows;
        }
        for rect in decorations.rects {
            self.check(epoch, cancelled)?;
            paint_decoration(&mut out, rect, r.device_scale, r.color, remaining_work)?;
        }
        self.check(epoch, cancelled)?;
        Ok(Some(out))
    }
}
fn allocate(len: usize) -> Result<Vec<u8>, String> {
    let mut v = Vec::new();
    v.try_reserve_exact(len)
        .map_err(|_| "text allocation failed")?;
    v.resize(len, 0);
    Ok(v)
}
fn round_half_zero(v: f32) -> f32 {
    (v.abs() - 0.5).ceil().copysign(v)
}
fn quantize(origin: [f32; 2], emoji: bool) -> ([i32; 2], [u8; 2]) {
    if emoji {
        return (origin.map(|v| round_half_zero(v) as i32), [0, 0]);
    }
    let q = [
        round_half_zero(origin[0] * 4.) / 4.,
        round_half_zero(origin[1]),
    ];
    // Match locked GPUI's truncation, including negative-origin behavior.
    (
        q.map(|v| v.trunc() as i32),
        [(q[0].fract() * 4.) as u8, q[1].fract() as u8],
    )
}
pub(crate) fn aligned_x(x: f32, width: f32, line: f32, align: TextAlign) -> f32 {
    match align {
        TextAlign::Left => x,
        TextAlign::Center => (x * 2. + width - line) / 2.,
        TextAlign::Right => x + width - line,
    }
}
fn clipped_box(r: &TextRasterRequest) -> Result<Option<[i32; 4]>, String> {
    let a = r.bounds;
    let b = r.clip;
    let values = [
        a[0].max(b[0]),
        a[1].max(b[1]),
        (a[0] + a[2]).min(b[0] + b[2]),
        (a[1] + a[3]).min(b[1] + b[3]),
    ];
    let pixels = values.map(|v| round_half_zero(v * r.device_scale));
    if pixels
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
    {
        return Err("text origin out of range".into());
    }
    Ok((pixels[2] > pixels[0] && pixels[3] > pixels[1]).then(|| pixels.map(|v| v as i32)))
}
fn glyph_capacity(bounds: Bounds<DevicePixels>) -> Result<usize, String> {
    let (w, h) = (bounds.size.width.0, bounds.size.height.0);
    if w < 0 || h < 0 || w > 1024 || h > 1024 {
        return Err("native glyph dimensions exceed limit".into());
    }
    // macOS may add one pixel for fractional x/y. All native formats at most BGRA.
    Ok((w as usize + 1) * (h as usize + 1) * 4)
}
struct GlyphBitmap {
    bounds: Bounds<DevicePixels>,
    width: usize,
    height: usize,
    data: Vec<u8>,
    color: bool,
}
impl GlyphBitmap {
    fn empty(bounds: Bounds<DevicePixels>) -> Self {
        Self {
            bounds,
            width: 0,
            height: 0,
            data: vec![],
            color: false,
        }
    }
    fn new(
        bounds: Bounds<DevicePixels>,
        width: i32,
        height: i32,
        data: Vec<u8>,
        emoji: bool,
    ) -> Result<Self, String> {
        if width <= 0 || height <= 0 || width > 1025 || height > 1025 {
            return Err("invalid native glyph size".into());
        }
        if width < bounds.size.width.0
            || width > bounds.size.width.0 + 1
            || height < bounds.size.height.0
            || height > bounds.size.height.0 + 1
        {
            return Err("native glyph bitmap differs from admitted bounds".into());
        }
        let pixels = width as usize * height as usize;
        // Grayscale is A8. Color emoji are straight BGRA. Linux can return A8
        // for an emoji with only a monochrome outline; handle that explicitly.
        let color = if data.len() == pixels {
            false
        } else if emoji && data.len() == pixels * 4 {
            true
        } else {
            return Err("unsupported native glyph bitmap format".into());
        };
        Ok(Self {
            bounds,
            width: width as usize,
            height: height as usize,
            data,
            color,
        })
    }
}
fn mul(a: u8, b: u8) -> u8 {
    ((a as u16 * b as u16 + 127) / 255) as u8
}
fn paint_bitmap(
    out: &mut RasterizedText,
    bitmap: &GlyphBitmap,
    origin: [i32; 2],
    color: [u8; 4],
    remaining_work: &mut u64,
) -> Result<(), String> {
    let ox = origin[0] as i64 + bitmap.bounds.origin.x.0 as i64 - out.origin[0] as i64;
    let oy = origin[1] as i64 + bitmap.bounds.origin.y.0 as i64 - out.origin[1] as i64;
    let x0 = 0.max(-ox).min(bitmap.width as i64) as usize;
    let y0 = 0.max(-oy).min(bitmap.height as i64) as usize;
    let x1 = (out.width as i64 - ox).clamp(0, bitmap.width as i64) as usize;
    let y1 = (out.height as i64 - oy).clamp(0, bitmap.height as i64) as usize;
    let writes = (x1.saturating_sub(x0) * y1.saturating_sub(y0)) as u64;
    charge_work(remaining_work, writes)?;
    out.pixel_writes = out.pixel_writes.saturating_add(writes);
    if out.pixel_writes > MAX_WRITES {
        return Err("text pixel work limit".into());
    }
    for y in y0..y1 {
        for x in x0..x1 {
            let i = y * bitmap.width + x;
            let src = if bitmap.color {
                let p = &bitmap.data[i * 4..i * 4 + 4];
                let a = mul(p[3], color[3]);
                [mul(p[2], a), mul(p[1], a), mul(p[0], a), a]
            } else {
                let a = mul(bitmap.data[i], color[3]);
                [mul(color[0], a), mul(color[1], a), mul(color[2], a), a]
            };
            let j = ((y as i64 + oy) as usize * out.width as usize + (x as i64 + ox) as usize) * 4;
            for (dst, src) in out.premul_rgba[j..j + 4].iter_mut().zip(src) {
                *dst = src.saturating_add(mul(*dst, 255 - src_alpha(bitmap, i, color[3])));
            }
        }
    }
    Ok(())
}
fn paint_decoration(
    out: &mut RasterizedText,
    rect: [f32; 4],
    scale: f32,
    color: [u8; 4],
    remaining_work: &mut u64,
) -> Result<(), String> {
    let x0 = ((rect[0] * scale).round() as i64 - out.origin[0] as i64).clamp(0, out.width as i64)
        as usize;
    let y0 = ((rect[1] * scale).round() as i64 - out.origin[1] as i64).clamp(0, out.height as i64)
        as usize;
    let x1 = (((rect[0] + rect[2]) * scale).round() as i64 - out.origin[0] as i64)
        .clamp(0, out.width as i64) as usize;
    let y1 = (((rect[1] + rect[3]) * scale).round() as i64 - out.origin[1] as i64)
        .clamp(0, out.height as i64) as usize;
    let writes = (x1.saturating_sub(x0) * y1.saturating_sub(y0)) as u64;
    charge_work(remaining_work, writes)?;
    out.pixel_writes = out.pixel_writes.saturating_add(writes);
    if out.pixel_writes > MAX_WRITES {
        return Err("text pixel work limit".into());
    }
    let source = [
        mul(color[0], color[3]),
        mul(color[1], color[3]),
        mul(color[2], color[3]),
        color[3],
    ];
    for y in y0..y1 {
        for x in x0..x1 {
            let index = (y * out.width as usize + x) * 4;
            for (dst, src) in out.premul_rgba[index..index + 4].iter_mut().zip(source) {
                *dst = src.saturating_add(mul(*dst, 255 - color[3]));
            }
        }
    }
    Ok(())
}
fn src_alpha(bitmap: &GlyphBitmap, i: usize, opacity: u8) -> u8 {
    mul(
        if bitmap.color {
            bitmap.data[i * 4 + 3]
        } else {
            bitmap.data[i]
        },
        opacity,
    )
}

fn charge_work(remaining: &mut u64, n: u64) -> Result<(), String> {
    *remaining = remaining.checked_sub(n).ok_or("text pixel work limit")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;
    fn bounds(w: i32, h: i32) -> Bounds<DevicePixels> {
        Bounds::new(
            point(DevicePixels(0), DevicePixels(0)),
            size(DevicePixels(w), DevicePixels(h)),
        )
    }
    fn output(width: u32, height: u32) -> RasterizedText {
        RasterizedText {
            width,
            height,
            origin: [0, 0],
            premul_rgba: vec![0; width as usize * height as usize * 4],
            glyphs: 0,
            color_glyphs: 0,
            rows: 0,
            peak_bytes: 0,
            pixel_writes: 0,
        }
    }
    #[test]
    fn alpha_coverage_preserves_ink_holes_and_source_over() {
        let b = GlyphBitmap::new(bounds(3, 1), 3, 1, vec![0, 128, 255], false).unwrap();
        let mut out = output(3, 1);
        let mut work = 100;
        paint_bitmap(&mut out, &b, [0, 0], [255, 0, 0, 128], &mut work).unwrap();
        assert_eq!(out.premul_rgba, [0, 0, 0, 0, 64, 0, 0, 64, 128, 0, 0, 128]);
        paint_bitmap(&mut out, &b, [0, 0], [255, 0, 0, 128], &mut work).unwrap();
        assert_eq!(&out.premul_rgba[8..], [192, 0, 0, 192]);
        assert_eq!(work, 94);
    }
    #[test]
    fn color_bgra_is_straight_and_alpha_is_applied_once() {
        let b = GlyphBitmap::new(bounds(1, 1), 1, 1, vec![200, 100, 50, 128], true).unwrap();
        let mut out = output(1, 1);
        let mut work = 10;
        paint_bitmap(&mut out, &b, [0, 0], [255, 0, 255, 128], &mut work).unwrap();
        assert_eq!(out.premul_rgba, [13, 25, 50, 64]);
        assert!(b.color);
    }
    #[test]
    fn monochrome_emoji_and_fractional_backend_padding_are_accepted_but_unknown_formats_reject() {
        assert!(
            !GlyphBitmap::new(bounds(1, 1), 1, 1, vec![128], true)
                .unwrap()
                .color
        );
        assert!(GlyphBitmap::new(bounds(1, 1), 2, 1, vec![128, 64], false).is_ok());
        assert!(GlyphBitmap::new(bounds(1, 1), 1, 1, vec![1, 2, 3, 4], false).is_err());
        assert!(GlyphBitmap::new(bounds(1, 1), 1, 1, vec![1, 2, 3], true).is_err());
        assert!(GlyphBitmap::new(bounds(1, 1), -1, 1, vec![], false).is_err());
    }
    #[test]
    fn integer_and_quarter_pixel_origin_matches_locked_gpui() {
        assert_eq!(quantize([12.125, 13.5], false), ([12, 13], [0, 0]));
        assert_eq!(quantize([12.376, 13.51], false), ([12, 14], [2, 0]));
        assert_eq!(quantize([-1.75, -2.5], false), ([-1, -2], [0, 0]));
        assert_eq!(quantize([12.75, 13.5], true), ([13, 13], [0, 0]));
        assert_eq!(aligned_x(10., 100., 40., TextAlign::Left), 10.);
        assert_eq!(aligned_x(10., 100., 40., TextAlign::Center), 40.);
        assert_eq!(aligned_x(10., 100., 40., TextAlign::Right), 70.);
    }
    #[test]
    fn bitmap_clipping_does_not_shift_baseline_and_work_limit_is_atomic() {
        let b = GlyphBitmap::new(bounds(3, 2), 3, 2, vec![255; 6], false).unwrap();
        let mut out = output(2, 2);
        let mut work = 1;
        assert!(paint_bitmap(&mut out, &b, [-1, 0], [0, 255, 0, 255], &mut work).is_err());
        assert!(out.premul_rgba.iter().all(|p| *p == 0));
        work = 10;
        paint_bitmap(&mut out, &b, [-1, 0], [0, 255, 0, 255], &mut work).unwrap();
        assert_eq!(out.premul_rgba, [0, 255, 0, 255].repeat(4));
        assert_eq!(work, 6);
    }
    #[test]
    fn invalidated_or_cancelled_backend_snapshot_is_rejected_before_shaping() {
        let raster: Arc<dyn PlatformTextSystem> = Arc::new(gpui::NoopTextSystem);
        let bridge = GpuiTextRasterizer {
            shaping: Arc::new(TextSystem::new(raster.clone())),
            raster,
            epoch: AtomicU64::new(0),
        };
        let cancel = AtomicBool::new(false);
        assert!(bridge.check(0, &cancel).is_ok());
        bridge.invalidate();
        assert!(bridge.check(0, &cancel).is_err());
        assert!(bridge.check(1, &cancel).is_ok());
        cancel.store(true, Ordering::Relaxed);
        assert!(bridge.check(1, &cancel).is_err());
    }
}

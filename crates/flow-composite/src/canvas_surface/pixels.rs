//! Physical-pixel interoperability. Straight RGBA is never confused with internal
//! premultiplied snapshots, and export never writes a path or fetches a resource.
use super::CanvasSurface;
use crate::{CompositeError, CompositeLimits, StaticImage};
use std::io::{self, Write};

/// Owned, tightly packed, straight-alpha RGBA8 in physical pixels.
/// Hidden RGB under alpha zero is lost when stored in a premultiplied surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasPixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}
impl CanvasPixels {
    pub fn from_rgba(
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        limits: CompositeLimits,
    ) -> Result<Self, CompositeError> {
        let bytes = dimensions(width, height, limits.bounded())?;
        if rgba.len() as u64 != bytes {
            return Err(CompositeError::InvalidImage);
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
    /// Dimensions/length stay invariant while the host changes channels.
    pub fn rgba_mut(&mut self) -> &mut [u8] {
        &mut self.rgba
    }
    pub fn into_rgba(self) -> Vec<u8> {
        self.rgba
    }
    pub fn byte_len(&self) -> u64 {
        self.rgba.len() as u64
    }
}
fn dimensions(w: u32, h: u32, limits: CompositeLimits) -> Result<u64, CompositeError> {
    let pixels = u64::from(w) * u64::from(h);
    if w == 0
        || h == 0
        || w > limits.max_dimension
        || h > limits.max_dimension
        || pixels > limits.max_pixels
    {
        return Err(CompositeError::TooLarge);
    }
    let bytes = pixels * 4;
    if bytes > limits.max_surface_bytes {
        return Err(CompositeError::WorkingSetLimit);
    }
    Ok(bytes)
}
fn intersection(x: i32, y: i32, w: u32, h: u32, width: u32, height: u32) -> Option<[u32; 4]> {
    let x0 = i64::from(x).max(0);
    let y0 = i64::from(y).max(0);
    let x1 = (i64::from(x) + i64::from(w)).min(i64::from(width));
    let y1 = (i64::from(y) + i64::from(h)).min(i64::from(height));
    (x1 > x0 && y1 > y0).then_some([x0 as u32, y0 as u32, x1 as u32, y1 as u32])
}
impl CanvasSurface {
    /// Read physical pixels, independent of logical size/DPR/draw smoothing.
    /// Out-of-bounds pixels are transparent; zero extents and excess budgets fail.
    pub fn read_pixels(
        &self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<CanvasPixels, CompositeError> {
        self.pixels.read_pixels(x, y, width, height, self.limits)
    }
    /// Replace physical pixels, including alpha-zero clears. This is not a
    /// source-over draw; logical transforms, opacity and smoothing do not apply.
    /// A wholly outside write is a no-op, preserving generation and snapshot.
    pub fn write_pixels(
        &mut self,
        pixels: &CanvasPixels,
        x: i32,
        y: i32,
    ) -> Result<bool, CompositeError> {
        dimensions(pixels.width, pixels.height, self.limits)?;
        let Some([x0, y0, x1, y1]) = intersection(
            x,
            y,
            pixels.width,
            pixels.height,
            self.pixels.width(),
            self.pixels.height(),
        ) else {
            return Ok(false);
        };
        let generation = self.next()?;
        let mut target = self.copy(pixels.byte_len())?;
        for ty in y0..y1 {
            let sy = (i64::from(ty) - i64::from(y)) as usize;
            let sx = (i64::from(x0) - i64::from(x)) as usize;
            let count = (x1 - x0) as usize;
            let source = &pixels.rgba[(sy * pixels.width as usize + sx) * 4..][..count * 4];
            let dest = &mut target.data_mut()
                [(ty as usize * self.pixels.width() as usize + x0 as usize) * 4..][..count * 4];
            for (d, s) in dest
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(source.as_chunks::<4>().0)
            {
                let c = tiny_skia::ColorU8::from_rgba(s[0], s[1], s[2], s[3]).premultiply();
                *d = [c.red(), c.green(), c.blue(), c.alpha()];
            }
        }
        self.publish(target, generation);
        Ok(true)
    }
    /// Encode raw owned physical pixels only, excluding retained component paint,
    /// corner masks, transforms and ancestors. The caller owns any file delivery.
    pub fn encode_png(&self) -> Result<Vec<u8>, CompositeError> {
        self.pixels.encode_png(self.limits)
    }
}
impl StaticImage {
    pub fn read_pixels(
        &self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        limits: CompositeLimits,
    ) -> Result<CanvasPixels, CompositeError> {
        let limits = limits.bounded();
        let bytes = dimensions(width, height, limits)?;
        if bytes / 4 * 2 > limits.max_surface_writes {
            return Err(CompositeError::WorkLimit);
        }
        if self.byte_len().saturating_add(bytes) > limits.max_surface_bytes {
            return Err(CompositeError::WorkingSetLimit);
        }
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(bytes as usize)
            .map_err(|_| CompositeError::AllocationFailed)?;
        rgba.resize(bytes as usize, 0);
        if let Some([x0, y0, x1, y1]) =
            intersection(x, y, width, height, self.width(), self.height())
        {
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let source = self
                        .pixmap
                        .pixel(sx, sy)
                        .ok_or(CompositeError::InvalidImage)?
                        .demultiply();
                    let dx = (i64::from(sx) - i64::from(x)) as usize;
                    let dy = (i64::from(sy) - i64::from(y)) as usize;
                    rgba[(dy * width as usize + dx) * 4..][..4].copy_from_slice(&[
                        source.red(),
                        source.green(),
                        source.blue(),
                        source.alpha(),
                    ]);
                }
            }
        }
        Ok(CanvasPixels {
            width,
            height,
            rgba,
        })
    }
    /// Streaming PNG through the already locked codec. Rows are demultiplied
    /// independently; no second full RGBA surface or file-system side effect.
    pub fn encode_png(&self, limits: CompositeLimits) -> Result<Vec<u8>, CompositeError> {
        let limits = limits.bounded();
        dimensions(self.width(), self.height(), limits)?;
        let pixels = u64::from(self.width()) * u64::from(self.height());
        if pixels.saturating_mul(3) > limits.max_surface_writes {
            return Err(CompositeError::WorkLimit);
        }
        // Conservative fixed codec/scratch allowance (stream state, row buffers,
        // deflate state, chunk buffer), separate from source and bounded output.
        const SCRATCH: u64 = 8 * 1024 * 1024;
        let available = limits
            .max_surface_bytes
            .checked_sub(self.byte_len().saturating_add(SCRATCH))
            .ok_or(CompositeError::WorkingSetLimit)?;
        let maximum = available.min(self.byte_len().saturating_mul(2).saturating_add(65536));
        if maximum == 0 {
            return Err(CompositeError::WorkingSetLimit);
        }
        let mut output = BoundedOutput {
            bytes: Vec::new(),
            maximum: maximum as usize,
            error: None,
        };
        let result = (|| -> Result<(), png::EncodingError> {
            let mut encoder = png::Encoder::new(&mut output, self.width(), self.height());
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_compression(png::Compression::Fast);
            let mut writer = encoder.write_header()?;
            {
                let mut stream = writer.stream_writer_with_size(16 * 1024)?;
                let mut row = vec![0u8; self.width() as usize * 4];
                for y in 0..self.height() {
                    for (x, p) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                        let c = self
                            .pixmap
                            .pixel(x as u32, y)
                            .expect("validated image extent")
                            .demultiply();
                        *p = [c.red(), c.green(), c.blue(), c.alpha()];
                    }
                    stream.write_all(&row)?;
                }
                stream.finish()?;
            }
            writer.finish()
        })();
        if let Some(error) = output.error {
            return Err(error);
        }
        result.map_err(|e| CompositeError::ImageEncoding(e.to_string()))?;
        Ok(output.bytes)
    }
}
struct BoundedOutput {
    bytes: Vec<u8>,
    maximum: usize,
    error: Option<CompositeError>,
}
impl Write for BoundedOutput {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(input.len()) > self.maximum {
            self.error = Some(CompositeError::WorkingSetLimit);
            return Err(io::Error::other("PNG output budget"));
        }
        if self.bytes.try_reserve_exact(input.len()).is_err() {
            self.error = Some(CompositeError::AllocationFailed);
            return Err(io::Error::other("PNG output allocation"));
        }
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

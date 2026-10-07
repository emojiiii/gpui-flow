//! Bounded embedded base-color images and CPU sampling for the static preview.
//!
//! glTF defines UV (0, 0) at the upper-left encoded pixel. Its image rules require
//! ignoring embedded colorspace information and recommend ignoring EXIF, including
//! orientation: <https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#images>.
//! Consequently this module never invokes the Creative image-orientation helper,
//! decoder metadata accessors, or color management. A separate bounded decode
//! copy omits nonessential metadata; the caller's GLB bytes remain untouched.

use super::{PreviewError, Result, invalid, unsupported};
use gltf::texture::{MagFilter, MinFilter, WrappingMode};
use image::{ColorType, ImageDecoder, Limits, codecs::jpeg::JpegDecoder, codecs::png::PngDecoder};
use std::{collections::BTreeMap, io::Cursor, sync::Arc, sync::OnceLock};

const MAX_IMAGES: usize = 8;
const MAX_DIMENSION: u32 = 1_024;
const MAX_TOTAL_PIXELS: usize = 4_194_304;
const MAX_ENCODED_BYTES: usize = 8 * 1024 * 1024;
const MAX_DOCUMENT_OBJECTS: usize = 2_048;
const MAX_CONTAINER_PARTS: usize = 4_096;
const MAX_JPEG_SCANS: usize = 128;
const MAX_DECODER_WORK_BYTES: usize = 64 * 1024 * 1024;
const MAX_PEAK_BYTES: usize = 128 * 1024 * 1024;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[derive(Clone, Debug, Default)]
pub(super) struct TextureSet {
    textures: Vec<Option<Texture>>,
    image_count: usize,
    byte_len: usize,
    uses_mipmaps: bool,
}

#[derive(Clone, Debug)]
pub(super) struct Texture {
    image: Arc<DecodedImage>,
    wrap_s: WrappingMode,
    wrap_t: WrappingMode,
    min_filter: Filter,
    mag_filter: Filter,
}

#[derive(Debug)]
struct DecodedImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
enum Filter {
    Nearest,
    Linear,
}

impl TextureSet {
    /// Decode only material base-color references. The document must already have
    /// passed glTF schema/reference validation. No URI is ever opened or decoded.
    pub(super) fn decode(document: &gltf::Document, bin: &[u8]) -> Result<Self> {
        if [
            document.textures().count(),
            document.images().count(),
            document.samplers().count(),
            document.materials().count(),
        ]
        .into_iter()
        .any(|count| count > MAX_DOCUMENT_OBJECTS)
        {
            return Err(PreviewError::BudgetExceeded(
                "texture document object limit",
            ));
        }
        let mut result = Self {
            textures: vec![None; document.textures().count()],
            ..Self::default()
        };
        let mut images: BTreeMap<usize, Arc<DecodedImage>> = BTreeMap::new();
        for material in document.materials() {
            let Some(info) = material.pbr_metallic_roughness().base_color_texture() else {
                continue;
            };
            let texture = info.texture();
            if result.textures[texture.index()].is_some() {
                continue;
            }
            let source = texture.source();
            let image = if let Some(image) = images.get(&source.index()) {
                Arc::clone(image)
            } else {
                if images.len() >= MAX_IMAGES {
                    return Err(PreviewError::BudgetExceeded("8 embedded base-color images"));
                }
                let (encoded, mime) = embedded_bytes(&source, bin)?;
                let image = Arc::new(decode_image(encoded, mime, result.byte_len)?);
                result.byte_len += image.rgba.len();
                images.insert(source.index(), Arc::clone(&image));
                image
            };
            let sampler = texture.sampler();
            let (min_filter, mipmaps) = min_filter(sampler.min_filter());
            result.uses_mipmaps |= mipmaps;
            result.textures[texture.index()] = Some(Texture {
                image,
                wrap_s: sampler.wrap_s(),
                wrap_t: sampler.wrap_t(),
                min_filter,
                mag_filter: match sampler.mag_filter() {
                    Some(MagFilter::Nearest) => Filter::Nearest,
                    Some(MagFilter::Linear) | None => Filter::Linear,
                },
            });
        }
        result.image_count = images.len();
        Ok(result)
    }

    pub(super) fn get(&self, gltf_texture_index: usize) -> Option<&Texture> {
        self.textures.get(gltf_texture_index)?.as_ref()
    }

    /// Unique retained RGBA8 bytes, excluding small sampler/Arc bookkeeping.
    pub(super) fn byte_len(&self) -> usize {
        self.byte_len
    }

    pub(super) fn image_count(&self) -> usize {
        self.image_count
    }

    /// True when an authored mip filter was collapsed to its base-level nearest
    /// or linear fallback. The caller must display this approximation warning.
    pub(super) fn uses_mipmaps(&self) -> bool {
        self.uses_mipmaps
    }
}

fn min_filter(filter: Option<MinFilter>) -> (Filter, bool) {
    match filter {
        Some(MinFilter::Nearest) => (Filter::Nearest, false),
        Some(MinFilter::Linear) | None => (Filter::Linear, false),
        Some(MinFilter::NearestMipmapNearest | MinFilter::NearestMipmapLinear) => {
            (Filter::Nearest, true)
        }
        Some(MinFilter::LinearMipmapNearest | MinFilter::LinearMipmapLinear) => {
            (Filter::Linear, true)
        }
    }
}

fn embedded_bytes<'a, 'b>(
    source: &gltf::image::Image<'a>,
    bin: &'b [u8],
) -> Result<(&'b [u8], &'a str)> {
    let gltf::image::Source::View { view, mime_type } = source.source() else {
        return Err(unsupported(
            "base-color images must use embedded buffer views, not URIs",
        ));
    };
    let buffer = view.buffer();
    if buffer.index() != 0 || !matches!(buffer.source(), gltf::buffer::Source::Bin) {
        return Err(unsupported(
            "base-color images require the embedded BIN buffer",
        ));
    }
    if view.stride().is_some() || view.target().is_some() {
        return Err(invalid(
            "image buffer views cannot have byteStride or target",
        ));
    }
    if view.length() > MAX_ENCODED_BYTES {
        return Err(PreviewError::BudgetExceeded("8 MiB encoded texture image"));
    }
    let end = view
        .offset()
        .checked_add(view.length())
        .ok_or_else(|| invalid("image buffer view offset overflow"))?;
    if end > buffer.length() {
        return Err(invalid("image buffer view exceeds declared BIN length"));
    }
    let bytes = bin
        .get(view.offset()..end)
        .ok_or_else(|| invalid("image buffer view exceeds embedded BIN data"))?;
    Ok((bytes, mime_type))
}

fn decode_image(encoded: &[u8], mime: &str, retained_bytes: usize) -> Result<DecodedImage> {
    if encoded.len() > MAX_ENCODED_BYTES {
        return Err(PreviewError::BudgetExceeded("8 MiB encoded texture image"));
    }
    let mut copy = Vec::new();
    copy.try_reserve_exact(encoded.len())
        .map_err(|_| PreviewError::BudgetExceeded("texture decode allocation"))?;
    let dimensions = match mime {
        "image/png" if encoded.starts_with(PNG_SIGNATURE) => prepare_png(encoded, &mut copy)?,
        "image/jpeg" if encoded.starts_with(b"\xff\xd8\xff") => prepare_jpeg(encoded, &mut copy)?,
        "image/png" | "image/jpeg" => return Err(invalid("texture MIME type/header mismatch")),
        _ => return Err(unsupported("base-color images must be PNG or JPEG")),
    };
    let pixels = checked_pixels(dimensions)?;
    let rgba_bytes = pixels * 4;
    if retained_bytes > MAX_TOTAL_PIXELS * 4 || rgba_bytes > MAX_TOTAL_PIXELS * 4 - retained_bytes {
        return Err(PreviewError::BudgetExceeded(
            "16 MiB decoded texture pixels",
        ));
    }
    // image 0.25's JPEG constructor copies its input, and its max_alloc is not a
    // strict working-memory limit. Bound encoded bytes, frame dimensions,
    // components, sampling factors and scan count *before* creating it. Reserve
    // a conservative 64 MiB codec allowance (including coefficient planes), two
    // encoded copies, two RGBA-sized pixel buffers, and 1 MiB bookkeeping. The
    // caller's already bounded borrowed GLB/JSON/geometry are outside this cap.
    let peak = retained_bytes
        .checked_add(MAX_DECODER_WORK_BYTES)
        .and_then(|n| n.checked_add(encoded.len() * 2))
        .and_then(|n| n.checked_add(rgba_bytes * 2))
        .and_then(|n| n.checked_add(1024 * 1024))
        .ok_or(PreviewError::BudgetExceeded("texture working memory"))?;
    if peak > MAX_PEAK_BYTES {
        return Err(PreviewError::BudgetExceeded(
            "128 MiB texture working memory",
        ));
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODER_WORK_BYTES as u64);
    if mime == "image/png" {
        // with_limits constrains PNG header and compressed-chunk processing as
        // well as pixels. set_limits alone would be too late for this decoder.
        let decoder = PngDecoder::with_limits(Cursor::new(copy), limits).map_err(image_error)?;
        decode_pixels(decoder, dimensions)
    } else {
        let mut decoder = JpegDecoder::new(Cursor::new(copy)).map_err(image_error)?;
        decoder.set_limits(limits).map_err(image_error)?;
        decode_pixels(decoder, dimensions)
    }
}

fn checked_pixels((width, height): (u32, u32)) -> Result<usize> {
    if width == 0 || height == 0 {
        return Err(invalid("texture dimensions must be nonzero"));
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(PreviewError::BudgetExceeded(
            "1024-pixel texture axis limit",
        ));
    }
    Ok(width as usize * height as usize)
}

fn decode_pixels(decoder: impl ImageDecoder, dimensions: (u32, u32)) -> Result<DecodedImage> {
    if decoder.dimensions() != dimensions {
        return Err(invalid("texture decoder/header dimensions differ"));
    }
    let pixels = checked_pixels(dimensions)?;
    let channels = match decoder.color_type() {
        ColorType::L8 => 1,
        ColorType::La8 => 2,
        ColorType::Rgb8 => 3,
        ColorType::Rgba8 => 4,
        _ => return Err(unsupported("base-color textures require 8-bit channels")),
    };
    if decoder.total_bytes() != (pixels * channels) as u64 {
        return Err(invalid("unexpected decoded texture byte length"));
    }
    let mut raw = allocate_pixels(pixels * channels)?;
    decoder.read_image(&mut raw).map_err(image_error)?;
    let rgba = if channels == 4 {
        raw
    } else {
        let mut rgba = allocate_pixels(pixels * 4)?;
        for (source, target) in raw.chunks_exact(channels).zip(rgba.as_chunks_mut::<4>().0) {
            if channels <= 2 {
                target[..3].fill(source[0]);
            } else {
                target[..3].copy_from_slice(source);
            }
            target[3] = if channels == 2 { source[1] } else { 255 };
        }
        rgba
    };
    Ok(DecodedImage {
        width: dimensions.0,
        height: dimensions.1,
        rgba,
    })
}

fn allocate_pixels(length: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| PreviewError::BudgetExceeded("texture pixel allocation"))?;
    bytes.resize(length, 0);
    Ok(bytes)
}

fn image_error(error: image::ImageError) -> PreviewError {
    match error {
        image::ImageError::Limits(_) => PreviewError::BudgetExceeded("texture decoder limit"),
        image::ImageError::Unsupported(_) => unsupported("unsupported texture image encoding"),
        _ => invalid(format!("texture image decode failed: {error}")),
    }
}

/// Keep only pixel-relevant PNG chunks. In particular, never hand a compressed
/// ICC/text payload to the decoder: its decoded metadata could dwarf the file.
/// Retain the ordinary IDAT image of APNG, as glTF recommends ignoring animation.
fn prepare_png(encoded: &[u8], copy: &mut Vec<u8>) -> Result<(u32, u32)> {
    copy.extend_from_slice(PNG_SIGNATURE);
    let mut offset = PNG_SIGNATURE.len();
    let mut dimensions = None;
    let mut saw_data = false;
    let mut parts = 0;
    while offset < encoded.len() {
        parts += 1;
        if parts > MAX_CONTAINER_PARTS {
            return Err(PreviewError::BudgetExceeded("texture PNG chunk limit"));
        }
        let header = encoded
            .get(offset..offset + 8)
            .ok_or_else(|| invalid("truncated PNG chunk header"))?;
        let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let kind = &header[4..8];
        if !kind.iter().all(u8::is_ascii_alphabetic) {
            return Err(invalid("invalid PNG chunk type"));
        }
        let end = offset
            .checked_add(length)
            .and_then(|n| n.checked_add(12))
            .ok_or_else(|| invalid("PNG chunk length overflow"))?;
        let chunk = encoded
            .get(offset..end)
            .ok_or_else(|| invalid("truncated PNG chunk"))?;
        if dimensions.is_none() && kind != b"IHDR" {
            return Err(invalid("PNG must start with IHDR"));
        }
        let keep = match kind {
            b"IHDR" => {
                if dimensions.is_some() || length != 13 {
                    return Err(invalid("invalid or duplicate PNG IHDR"));
                }
                let width = u32::from_be_bytes(chunk[8..12].try_into().unwrap());
                let height = u32::from_be_bytes(chunk[12..16].try_into().unwrap());
                checked_pixels((width, height))?;
                // Indexed PNG uses 8-bit palette colors even when indices are
                // packed; lower-depth grayscale is expanded by the PNG codec.
                if !matches!(chunk[16], 1 | 2 | 4 | 8) {
                    return Err(unsupported("base-color PNG must not use 16-bit channels"));
                }
                dimensions = Some((width, height));
                true
            }
            b"PLTE" => {
                if length > 768 {
                    return Err(invalid("oversized PNG palette"));
                }
                true
            }
            b"tRNS" => {
                if length > 256 {
                    return Err(invalid("oversized PNG transparency table"));
                }
                true
            }
            b"IDAT" => {
                saw_data = true;
                true
            }
            b"IEND" => {
                if length != 0 || !saw_data || end != encoded.len() {
                    return Err(invalid("invalid PNG end or trailing image bytes"));
                }
                copy.extend_from_slice(chunk);
                return dimensions.ok_or_else(|| invalid("missing PNG dimensions"));
            }
            _ if kind[0].is_ascii_lowercase() => false,
            _ => return Err(unsupported("unknown critical PNG chunk")),
        };
        if keep {
            copy.extend_from_slice(chunk);
        }
        offset = end;
    }
    Err(invalid("PNG is missing IEND"))
}

/// Parse markers without allocating metadata, including between progressive
/// scans. JPEG APP/COM payloads are skipped, except the required JFIF APP0.
fn prepare_jpeg(encoded: &[u8], copy: &mut Vec<u8>) -> Result<(u32, u32)> {
    copy.extend_from_slice(b"\xff\xd8");
    let mut offset = 2;
    let mut dimensions = None;
    let mut parts = 0;
    let mut scans = 0;
    while offset < encoded.len() {
        let start = offset;
        if encoded[offset] != 0xff {
            return Err(invalid("expected JPEG marker"));
        }
        while encoded.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *encoded
            .get(offset)
            .ok_or_else(|| invalid("truncated JPEG marker"))?;
        offset += 1;
        parts += 1;
        if parts > MAX_CONTAINER_PARTS {
            return Err(PreviewError::BudgetExceeded("texture JPEG marker limit"));
        }
        if marker == 0xd9 {
            if scans == 0 || offset != encoded.len() {
                return Err(invalid("invalid JPEG end or trailing image bytes"));
            }
            copy.extend_from_slice(&encoded[start..offset]);
            return dimensions.ok_or_else(|| invalid("JPEG is missing a frame"));
        }
        if matches!(marker, 0x00 | 0x01 | 0xd0..=0xd8) {
            return Err(invalid("unexpected standalone JPEG marker"));
        }
        let length_bytes = encoded
            .get(offset..offset + 2)
            .ok_or_else(|| invalid("truncated JPEG segment length"))?;
        let length = u16::from_be_bytes(length_bytes.try_into().unwrap()) as usize;
        if length < 2 {
            return Err(invalid("invalid JPEG segment length"));
        }
        let end = offset
            .checked_add(length)
            .ok_or_else(|| invalid("JPEG segment length overflow"))?;
        let payload = encoded
            .get(offset + 2..end)
            .ok_or_else(|| invalid("truncated JPEG segment"))?;
        if parts == 1 && (marker != 0xe0 || payload.len() < 14 || !payload.starts_with(b"JFIF\0")) {
            return Err(unsupported(
                "glTF JPEG textures require a leading JFIF APP0 marker",
            ));
        }
        let keep = match marker {
            0xc0..=0xc2 => {
                if dimensions.is_some() || payload.len() < 6 {
                    return Err(invalid("invalid or duplicate JPEG frame"));
                }
                let components = usize::from(payload[5]);
                if payload[0] != 8 || !matches!(components, 1 | 3) {
                    return Err(unsupported(
                        "JPEG base color requires one or three 8-bit components",
                    ));
                }
                if payload.len() != 6 + components * 3 {
                    return Err(invalid("invalid JPEG component table"));
                }
                let mut blocks = 0;
                for component in payload[6..].as_chunks::<3>().0 {
                    let horizontal = component[1] >> 4;
                    let vertical = component[1] & 15;
                    if !(1..=4).contains(&horizontal) || !(1..=4).contains(&vertical) {
                        return Err(unsupported("unsupported JPEG component sampling"));
                    }
                    blocks += usize::from(horizontal) * usize::from(vertical);
                }
                if blocks > 10 {
                    return Err(unsupported("unsupported JPEG MCU block count"));
                }
                let width = u32::from(u16::from_be_bytes(payload[3..5].try_into().unwrap()));
                let height = u32::from(u16::from_be_bytes(payload[1..3].try_into().unwrap()));
                checked_pixels((width, height))?;
                dimensions = Some((width, height));
                true
            }
            0xda => {
                if dimensions.is_none() {
                    return Err(invalid("JPEG scan precedes frame dimensions"));
                }
                scans += 1;
                if scans > MAX_JPEG_SCANS {
                    return Err(PreviewError::BudgetExceeded("texture JPEG scan limit"));
                }
                true
            }
            0xc4 | 0xdb | 0xdd => true, // Huffman, quantization, restart interval.
            0xe0 => parts == 1,
            0xe1..=0xef | 0xfe => false, // EXIF, ICC, XMP and other metadata.
            _ => return Err(unsupported("unsupported JPEG coding marker")),
        };
        if keep {
            copy.extend_from_slice(&encoded[start..end]);
        }
        offset = end;
        if marker == 0xda {
            let entropy_start = offset;
            loop {
                let byte = *encoded
                    .get(offset)
                    .ok_or_else(|| invalid("truncated JPEG scan"))?;
                if byte != 0xff {
                    offset += 1;
                    continue;
                }
                let marker_start = offset;
                while encoded.get(offset) == Some(&0xff) {
                    offset += 1;
                }
                let next = *encoded
                    .get(offset)
                    .ok_or_else(|| invalid("truncated JPEG scan marker"))?;
                if next == 0x00 || (0xd0..=0xd7).contains(&next) {
                    offset += 1;
                    continue;
                }
                offset = marker_start;
                break;
            }
            copy.extend_from_slice(&encoded[entropy_start..offset]);
        }
    }
    Err(invalid("JPEG is missing EOI"))
}

impl Texture {
    pub(super) fn dimensions(&self) -> (u32, u32) {
        (self.image.width, self.image.height)
    }

    /// RGB is linear before interpolation. OPAQUE materials ignore the encoded
    /// alpha channel, even when it is zero; the parent rejects other alpha modes.
    pub(super) fn sample(&self, uv: [f64; 2], minifying: bool) -> [f64; 3] {
        if !uv.into_iter().all(f64::is_finite) {
            return [0.0; 3];
        }
        let u = wrap_coordinate(uv[0], self.wrap_s);
        let v = wrap_coordinate(uv[1], self.wrap_t);
        let width = f64::from(self.image.width);
        let height = f64::from(self.image.height);
        let filter = if minifying {
            self.min_filter
        } else {
            self.mag_filter
        };
        match filter {
            Filter::Nearest => self.texel((u * width).floor() as i64, (v * height).floor() as i64),
            Filter::Linear => {
                // Normalized coordinates address texel edges; centers are half
                // a texel inward. Wrap each neighbor separately across seams.
                let x = u * width - 0.5;
                let y = v * height - 0.5;
                let left = x.floor();
                let top = y.floor();
                let tx = x - left;
                let ty = y - top;
                let a = self.texel(left as i64, top as i64);
                let b = self.texel(left as i64 + 1, top as i64);
                let c = self.texel(left as i64, top as i64 + 1);
                let d = self.texel(left as i64 + 1, top as i64 + 1);
                std::array::from_fn(|channel| {
                    let upper = a[channel] * (1.0 - tx) + b[channel] * tx;
                    let lower = c[channel] * (1.0 - tx) + d[channel] * tx;
                    upper * (1.0 - ty) + lower * ty
                })
            }
        }
    }

    fn texel(&self, x: i64, y: i64) -> [f64; 3] {
        let x = wrap_index(x, self.image.width, self.wrap_s);
        let y = wrap_index(y, self.image.height, self.wrap_t);
        let offset = (y * self.image.width as usize + x) * 4;
        let table = srgb_table();
        std::array::from_fn(|channel| table[usize::from(self.image.rgba[offset + channel])])
    }
}

fn wrap_coordinate(value: f64, mode: WrappingMode) -> f64 {
    match mode {
        WrappingMode::ClampToEdge => value.clamp(0.0, 1.0),
        // rem_euclid can round a tiny negative input up to exactly its modulus.
        // Preserve the final texel for nearest sampling on that side of a seam.
        WrappingMode::Repeat => value.rem_euclid(1.0).min(1.0_f64.next_down()),
        WrappingMode::MirroredRepeat => {
            let value = value.rem_euclid(2.0);
            if value <= 1.0 { value } else { 2.0 - value }
        }
    }
}

fn wrap_index(value: i64, length: u32, mode: WrappingMode) -> usize {
    let length = i64::from(length);
    let value = match mode {
        WrappingMode::ClampToEdge => value.clamp(0, length - 1),
        WrappingMode::Repeat => value.rem_euclid(length),
        WrappingMode::MirroredRepeat => {
            let value = value.rem_euclid(length * 2);
            if value < length {
                value
            } else {
                length * 2 - value - 1
            }
        }
    };
    value as usize
}

fn srgb_table() -> &'static [f64; 256] {
    static TABLE: OnceLock<[f64; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        std::array::from_fn(|value| {
            let encoded = value as f64 / 255.0;
            if encoded <= 0.04045 {
                encoded / 12.92
            } else {
                ((encoded + 0.055) / 1.055).powf(2.4)
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{
        ExtendedColorType, ImageEncoder, codecs::jpeg::JpegEncoder, codecs::png::PngEncoder,
    };
    use serde_json::{Value, json};

    fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::new();
        PngEncoder::new(&mut encoded)
            .write_image(rgba, width, height, ExtendedColorType::Rgba8)
            .unwrap();
        encoded
    }

    fn jpeg(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::new();
        JpegEncoder::new_with_quality(&mut encoded, 95)
            .encode(rgb, width, height, ExtendedColorType::Rgb8)
            .unwrap();
        encoded
    }

    fn root(encoded: &[u8], mime: &str) -> Value {
        json!({
            "asset": { "version": "2.0" },
            "buffers": [{ "byteLength": encoded.len() }],
            "bufferViews": [{ "buffer": 0, "byteLength": encoded.len() }],
            "images": [{ "bufferView": 0, "mimeType": mime }],
            "textures": [{ "source": 0 }],
            "materials": [{ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } } }]
        })
    }

    fn document(root: Value) -> gltf::Document {
        gltf::Document::from_json(serde_json::from_value(root).unwrap()).unwrap()
    }

    fn fixture(wrap: WrappingMode, filter: Filter) -> Texture {
        Texture {
            image: Arc::new(DecodedImage {
                width: 2,
                height: 2,
                rgba: vec![
                    255, 0, 0, 0, // Upper-left red, alpha deliberately zero.
                    0, 255, 0, 255, // Upper-right green.
                    0, 0, 255, 127, // Lower-left blue.
                    255, 255, 255, 255, // Lower-right white.
                ],
            }),
            wrap_s: wrap,
            wrap_t: wrap,
            min_filter: filter,
            mag_filter: filter,
        }
    }

    fn assert_rgb(actual: [f64; 3], expected: [f64; 3]) {
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 1.0e-12,
                "{actual} != {expected}"
            );
        }
    }

    #[test]
    fn asymmetric_uv_origin_is_upper_left_and_opaque_ignores_alpha() {
        let texture = fixture(WrappingMode::ClampToEdge, Filter::Nearest);
        assert_rgb(texture.sample([0.25, 0.25], false), [1.0, 0.0, 0.0]);
        assert_rgb(texture.sample([0.75, 0.25], false), [0.0, 1.0, 0.0]);
        assert_rgb(texture.sample([0.25, 0.75], false), [0.0, 0.0, 1.0]);
        assert_rgb(texture.sample([0.75, 0.75], false), [1.0, 1.0, 1.0]);
    }

    #[test]
    fn nearest_wrap_modes_cover_negative_coordinates_and_exact_edges() {
        let repeat = fixture(WrappingMode::Repeat, Filter::Nearest);
        assert_rgb(repeat.sample([1.0, 0.25], false), [1.0, 0.0, 0.0]);
        assert_rgb(repeat.sample([-0.25, 0.25], false), [0.0, 1.0, 0.0]);
        assert_rgb(
            repeat.sample([-f64::MIN_POSITIVE, 0.25], false),
            [0.0, 1.0, 0.0],
        );
        let mirrored = fixture(WrappingMode::MirroredRepeat, Filter::Nearest);
        assert_rgb(mirrored.sample([1.0, 0.25], false), [0.0, 1.0, 0.0]);
        assert_rgb(mirrored.sample([1.75, 0.25], false), [1.0, 0.0, 0.0]);
        assert_rgb(mirrored.sample([-0.25, 0.25], false), [1.0, 0.0, 0.0]);
        let clamped = fixture(WrappingMode::ClampToEdge, Filter::Nearest);
        assert_rgb(clamped.sample([-100.0, 100.0], false), [0.0, 0.0, 1.0]);
        assert_rgb(clamped.sample([1.0, 1.0], false), [1.0; 3]);
    }

    #[test]
    fn bilinear_neighbors_wrap_individually_at_repeat_and_mirror_seams() {
        let repeat = fixture(WrappingMode::Repeat, Filter::Linear);
        assert_rgb(repeat.sample([0.0, 0.25], false), [0.5, 0.5, 0.0]);
        assert_rgb(repeat.sample([1.0, 0.25], false), [0.5, 0.5, 0.0]);
        assert_rgb(repeat.sample([0.0, 0.0], false), [0.5; 3]);
        let mirrored = fixture(WrappingMode::MirroredRepeat, Filter::Linear);
        assert_rgb(mirrored.sample([0.0, 0.25], false), [1.0, 0.0, 0.0]);
        assert_rgb(mirrored.sample([1.0, 0.25], false), [0.0, 1.0, 0.0]);
        assert_rgb(mirrored.sample([2.0, 0.25], false), [1.0, 0.0, 0.0]);
        let clamped = fixture(WrappingMode::ClampToEdge, Filter::Linear);
        assert_rgb(clamped.sample([-1.0, 2.0], false), [0.0, 0.0, 1.0]);
    }

    #[test]
    fn srgb_is_decoded_before_linear_filtering_and_min_mag_are_independent() {
        let mut texture = fixture(WrappingMode::ClampToEdge, Filter::Nearest);
        texture.min_filter = Filter::Linear;
        texture.image = Arc::new(DecodedImage {
            width: 2,
            height: 1,
            rgba: vec![0, 0, 0, 255, 255, 255, 255, 255],
        });
        assert_rgb(texture.sample([0.5, 0.5], false), [1.0; 3]);
        assert_rgb(texture.sample([0.5, 0.5], true), [0.5; 3]);
        assert!((srgb_table()[128] - 0.215_860_500_113_899_26).abs() < 1.0e-12);
        assert!((srgb_table()[10] - (10.0 / 255.0 / 12.92)).abs() < 1.0e-12);
    }

    #[test]
    fn finite_extreme_and_nonfinite_uvs_are_safe() {
        for wrap in [
            WrappingMode::Repeat,
            WrappingMode::MirroredRepeat,
            WrappingMode::ClampToEdge,
        ] {
            let texture = fixture(wrap, Filter::Linear);
            assert!(
                texture
                    .sample([f64::MAX, -f64::MAX], true)
                    .into_iter()
                    .all(f64::is_finite)
            );
            assert_eq!(texture.sample([f64::NAN, 0.0], true), [0.0; 3]);
            assert_eq!(texture.sample([0.0, f64::INFINITY], false), [0.0; 3]);
        }
    }

    #[test]
    fn png_decode_preserves_pixels_source_and_sampler_image_sharing() {
        let expected = fixture(WrappingMode::Repeat, Filter::Nearest);
        let encoded = png(2, 2, &expected.image.rgba);
        let original = encoded.clone();
        let mut value = root(&encoded, "image/png");
        value["samplers"] = json!([
            { "magFilter": 9728, "minFilter": 9728, "wrapS": 10497, "wrapT": 33071 },
            { "magFilter": 9729, "minFilter": 9987, "wrapS": 33648, "wrapT": 10497 }
        ]);
        value["textures"] = json!([{ "source": 0, "sampler": 0 }, { "source": 0, "sampler": 1 }]);
        value["materials"] = json!([
            { "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } } },
            { "pbrMetallicRoughness": { "baseColorTexture": { "index": 1 } } }
        ]);
        let textures = TextureSet::decode(&document(value), &encoded).unwrap();
        assert_eq!(textures.image_count(), 1);
        assert_eq!(textures.byte_len(), 16);
        assert!(textures.uses_mipmaps());
        let a = textures.get(0).unwrap();
        let b = textures.get(1).unwrap();
        assert!(Arc::ptr_eq(&a.image, &b.image));
        assert_eq!(a.image.rgba, expected.image.rgba);
        assert_eq!(a.dimensions(), (2, 2));
        assert!(textures.get(2).is_none());
        assert_eq!(encoded, original);
        assert!(Arc::ptr_eq(
            &textures.clone().get(0).unwrap().image,
            &a.image
        ));
    }

    #[test]
    fn all_mip_filters_use_the_correct_base_level_fallback() {
        for filter in [
            MinFilter::NearestMipmapNearest,
            MinFilter::NearestMipmapLinear,
        ] {
            assert!(matches!(min_filter(Some(filter)), (Filter::Nearest, true)));
        }
        for filter in [
            MinFilter::LinearMipmapNearest,
            MinFilter::LinearMipmapLinear,
        ] {
            assert!(matches!(min_filter(Some(filter)), (Filter::Linear, true)));
        }
        assert!(matches!(min_filter(None), (Filter::Linear, false)));
    }

    fn exif_orientation_six() -> Vec<u8> {
        vec![
            b'I', b'I', 42, 0, 8, 0, 0, 0, // Little-endian TIFF, IFD at offset 8.
            1, 0, // One entry.
            0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, // Orientation SHORT = 6.
            0, 0, 0, 0, // No following IFD.
        ]
    }

    fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
        chunk.extend_from_slice(kind);
        chunk.extend_from_slice(data);
        let mut crc = !0_u32;
        for byte in &chunk[4..] {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
            }
        }
        chunk.extend_from_slice(&(!crc).to_be_bytes());
        chunk
    }

    #[test]
    fn png_exif_and_colorspace_metadata_do_not_rotate_or_recolor_texture_uvs() {
        let rgba = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255, 0, 255, 255, 255,
            255, 0, 255, 255,
        ];
        let original = png(2, 3, &rgba);
        let mut with_metadata = original[..33].to_vec();
        with_metadata.extend(png_chunk(b"eXIf", &exif_orientation_six()));
        with_metadata.extend(png_chunk(b"gAMA", &100_000_u32.to_be_bytes()));
        with_metadata.extend_from_slice(&original[33..]);
        // Prove the fixture actually carries a rotation another image workflow
        // would apply. Production texture decoding never calls this accessor.
        let mut ordinary = PngDecoder::new(Cursor::new(&with_metadata)).unwrap();
        assert_eq!(
            ordinary.orientation().unwrap(),
            image::metadata::Orientation::Rotate90
        );
        let decoded = decode_image(&with_metadata, "image/png", 0).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 3));
        assert_eq!(decoded.rgba, rgba);
        let mut texture = fixture(WrappingMode::ClampToEdge, Filter::Nearest);
        texture.image = Arc::new(decoded);
        assert_rgb(texture.sample([0.25, 1.0 / 6.0], false), [1.0, 0.0, 0.0]);
        assert_rgb(texture.sample([0.75, 5.0 / 6.0], false), [1.0, 0.0, 1.0]);
    }

    #[test]
    fn jpeg_exif_orientation_and_icc_are_ignored_without_touching_encoded_source() {
        let original = jpeg(
            2,
            3,
            &[
                255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 255, 255, 255, 0, 255,
            ],
        );
        let app0_end = 4 + u16::from_be_bytes(original[4..6].try_into().unwrap()) as usize;
        let mut exif = b"Exif\0\0".to_vec();
        exif.extend(exif_orientation_six());
        let mut encoded = original[..app0_end].to_vec();
        encoded.extend_from_slice(b"\xff\xe1");
        encoded.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        encoded.extend(exif);
        encoded.extend_from_slice(b"\xff\xe2\0\x10ICC_PROFILE\0\x01\x01");
        encoded.extend_from_slice(&original[app0_end..]);
        let snapshot = encoded.clone();
        let mut ordinary = JpegDecoder::new(Cursor::new(&encoded)).unwrap();
        assert_eq!(
            ordinary.orientation().unwrap(),
            image::metadata::Orientation::Rotate90
        );
        let expected = decode_image(&original, "image/jpeg", 0).unwrap();
        let actual = decode_image(&encoded, "image/jpeg", 0).unwrap();
        assert_eq!((actual.width, actual.height), (2, 3));
        assert_eq!(actual.rgba, expected.rgba);
        assert_eq!(encoded, snapshot);
    }

    #[test]
    fn ignored_compressed_png_metadata_never_reaches_decoder() {
        let original = png(1, 1, &[128, 64, 32, 255]);
        let mut encoded = original[..33].to_vec();
        // Intentionally invalid compressed ICC/text payloads are irrelevant to
        // glTF pixels and must not trigger decompression or metadata allocation.
        encoded.extend(png_chunk(b"iCCP", b"profile\0\0invalid-zlib"));
        encoded.extend(png_chunk(b"zTXt", b"comment\0\0invalid-zlib"));
        encoded.extend_from_slice(&original[33..]);
        assert_eq!(
            decode_image(&encoded, "image/png", 0).unwrap().rgba,
            [128, 64, 32, 255]
        );
    }

    #[test]
    fn mime_codec_depth_and_truncated_images_are_rejected() {
        let encoded = png(1, 1, &[255; 4]);
        assert!(matches!(
            decode_image(&encoded, "image/jpeg", 0),
            Err(PreviewError::Invalid(_))
        ));
        assert!(matches!(
            decode_image(&encoded, "image/webp", 0),
            Err(PreviewError::Unsupported(_))
        ));
        assert!(decode_image(&encoded[..encoded.len() - 1], "image/png", 0).is_err());
        let mut sixteen_bit = Vec::new();
        PngEncoder::new(&mut sixteen_bit)
            .write_image(&[0; 8], 1, 1, ExtendedColorType::Rgba16)
            .unwrap();
        assert!(matches!(
            decode_image(&sixteen_bit, "image/png", 0),
            Err(PreviewError::Unsupported(_))
        ));
        let jpeg = jpeg(1, 1, &[255; 3]);
        assert!(decode_image(&jpeg[..jpeg.len() - 2], "image/jpeg", 0).is_err());
        let mut bad_range = encoded.clone();
        bad_range[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(
            decode_image(&bad_range, "image/png", 0),
            Err(PreviewError::Invalid(_))
        ));
    }

    #[test]
    fn declared_buffer_and_actual_bin_ranges_are_both_checked() {
        let encoded = png(1, 1, &[255; 4]);
        let valid = document(root(&encoded, "image/png"));
        assert!(TextureSet::decode(&valid, &encoded[..encoded.len() - 1]).is_err());
        let mut value = root(&encoded, "image/png");
        value["buffers"][0]["byteLength"] = json!(encoded.len() - 1);
        assert!(TextureSet::decode(&document(value), &encoded).is_err());
        let mut value = root(&encoded, "image/png");
        value["bufferViews"][0]["byteOffset"] = json!(encoded.len());
        assert!(TextureSet::decode(&document(value), &encoded).is_err());
        for property in ["byteStride", "target"] {
            let mut value = root(&encoded, "image/png");
            value["bufferViews"][0][property] =
                json!(if property == "byteStride" { 4 } else { 34962 });
            assert!(TextureSet::decode(&document(value), &encoded).is_err());
        }
    }

    #[test]
    fn external_and_data_uris_are_rejected_and_unused_images_are_not_decoded() {
        let encoded = png(1, 1, &[255; 4]);
        for uri in [
            "https://example.invalid/texture.png",
            "data:image/png;base64,AAAA",
            "texture.png",
        ] {
            let mut value = root(&encoded, "image/png");
            value["images"][0] = json!({ "uri": uri });
            assert!(matches!(
                TextureSet::decode(&document(value), &encoded),
                Err(PreviewError::Unsupported(_))
            ));
        }
        let mut value = root(&encoded, "image/png");
        value["materials"] = json!([]);
        let textures = TextureSet::decode(&document(value), &[]).unwrap();
        assert_eq!(textures.image_count(), 0);
        assert_eq!(textures.byte_len(), 0);
        assert!(textures.get(0).is_none());
        assert!(!textures.uses_mipmaps());
    }

    #[test]
    fn encoded_axis_total_pixel_and_image_count_budgets_are_enforced() {
        assert!(matches!(
            decode_image(&vec![0; MAX_ENCODED_BYTES + 1], "image/png", 0),
            Err(PreviewError::BudgetExceeded(_))
        ));
        let encoded = png(1, 1, &[255; 4]);
        assert!(decode_image(&encoded, "image/png", MAX_TOTAL_PIXELS * 4 - 4).is_ok());
        assert!(matches!(
            decode_image(&encoded, "image/png", MAX_TOTAL_PIXELS * 4),
            Err(PreviewError::BudgetExceeded(_))
        ));
        let mut oversized = encoded.clone();
        oversized[16..20].copy_from_slice(&(MAX_DIMENSION + 1).to_be_bytes());
        assert!(matches!(
            decode_image(&oversized, "image/png", 0),
            Err(PreviewError::BudgetExceeded(_))
        ));
        oversized[16..20].copy_from_slice(&0_u32.to_be_bytes());
        assert!(matches!(
            decode_image(&oversized, "image/png", 0),
            Err(PreviewError::Invalid(_))
        ));
        let mut value = root(&encoded, "image/png");
        value["images"] = json!(vec![
            json!({ "bufferView": 0, "mimeType": "image/png" });
            MAX_IMAGES + 1
        ]);
        value["textures"] = json!(
            (0..=MAX_IMAGES)
                .map(|index| json!({ "source": index }))
                .collect::<Vec<_>>()
        );
        value["materials"] = json!((0..=MAX_IMAGES).map(|index| json!({ "pbrMetallicRoughness": { "baseColorTexture": { "index": index } } })).collect::<Vec<_>>());
        assert!(matches!(
            TextureSet::decode(&document(value), &encoded),
            Err(PreviewError::BudgetExceeded(_))
        ));
    }

    #[test]
    fn jpeg_frame_bounds_and_precision_are_checked_before_decoder_construction() {
        let encoded = jpeg(1, 1, &[255; 3]);
        let frame = encoded
            .windows(2)
            .position(|bytes| bytes == b"\xff\xc0")
            .unwrap();
        let mut oversized = encoded.clone();
        oversized[frame + 7..frame + 9].copy_from_slice(&1025_u16.to_be_bytes());
        assert!(matches!(
            decode_image(&oversized, "image/jpeg", 0),
            Err(PreviewError::BudgetExceeded(_))
        ));
        let mut unsupported_depth = encoded.clone();
        unsupported_depth[frame + 4] = 12;
        assert!(matches!(
            decode_image(&unsupported_depth, "image/jpeg", 0),
            Err(PreviewError::Unsupported(_))
        ));
    }

    #[test]
    fn metadata_chunk_count_is_bounded_without_retaining_metadata() {
        let original = png(1, 1, &[255; 4]);
        let mut encoded = original[..33].to_vec();
        let chunk = png_chunk(b"tEXt", b"x\0");
        for _ in 0..MAX_CONTAINER_PARTS {
            encoded.extend_from_slice(&chunk);
        }
        encoded.extend_from_slice(&original[33..]);
        assert!(matches!(
            decode_image(&encoded, "image/png", 0),
            Err(PreviewError::BudgetExceeded(_))
        ));
    }
}

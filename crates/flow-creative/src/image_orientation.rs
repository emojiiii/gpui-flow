//! Shared, non-destructive image orientation for asset metadata and raster consumers.
//!
//! Encoded bytes and canvas geometry are never changed here. Read metadata before
//! consuming the decoder, validate encoded dimensions against decode budgets, and
//! use oriented dimensions for the resulting image's logical size.

#[cfg(any(feature = "desktop", test))]
use image::RgbaImage;
use image::{
    ImageDecoder, ImageError, ImageResult,
    error::{DecodingError, ImageFormatHint},
    metadata::Orientation,
};
#[cfg(any(feature = "desktop", test))]
use std::fmt;

#[cfg(any(feature = "desktop", test))]
const CHECKPOINT_PIXELS: usize = 4096;
const MAX_EXIF_BYTES: usize = 64 * 1024;
const MAX_EXIF_FIELDS: usize = 4096;
const MAX_EXIF_VALUE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ImageOrientation {
    pub raw_dimensions: (u32, u32),
    pub oriented_dimensions: (u32, u32),
    pub orientation: Orientation,
}

impl ImageOrientation {
    /// Inspect an already admitted decoder without decoding its pixel buffer.
    ///
    /// Callers must bound the encoded source and configure decoder limits before
    /// construction. Metadata extraction can allocate codec-owned data and an
    /// EXIF copy; it is not covered by `orient_rgba`'s separate pixel allowance.
    /// Check cancellation/deadlines before and after this codec operation (and
    /// use a cancellable reader when supported by the codec).
    ///
    /// A strict EXIF parser distinguishes malformed or ambiguous orientation
    /// from a valid chunk with no orientation tag. Only the latter is upright.
    /// Raw EXIF is limited to 64 KiB, parsed field count to 4096, and materialized
    /// field values to 8 MiB. The decoder's EXIF copy and the parser's entry/index
    /// tables are separate from that value allowance. `read_raw` has no allocator
    /// or cancellation hook; configure the caller's codec/metadata budget too.
    pub(crate) fn from_decoder<D: ImageDecoder + ?Sized>(decoder: &mut D) -> ImageResult<Self> {
        let raw_dimensions = decoder.dimensions();
        if raw_dimensions.0 == 0 || raw_dimensions.1 == 0 {
            return Err(invalid_metadata("image dimensions must be nonzero"));
        }
        let exif_orientation = match decoder.exif_metadata()? {
            Some(exif) => Some(parse_exif_orientation(exif)?),
            None => None,
        };
        // The EXIF copy above has been dropped before asking the decoder for its
        // canonical orientation. This respects formats with their own metadata
        // handling and uses JPEG's orientation cache in the locked image crate.
        let orientation = decoder.orientation()?;
        if exif_orientation.is_some_and(|exif| exif != orientation) {
            return Err(invalid_metadata(
                "decoder orientation disagrees with its EXIF metadata",
            ));
        }
        Ok(Self {
            raw_dimensions,
            oriented_dimensions: oriented_dimensions(raw_dimensions, orientation),
            orientation,
        })
    }
}

fn invalid_metadata(message: &'static str) -> ImageError {
    ImageError::Decoding(DecodingError::new(ImageFormatHint::Unknown, message))
}

fn parse_exif_orientation(bytes: Vec<u8>) -> ImageResult<Orientation> {
    if bytes.len() > MAX_EXIF_BYTES {
        return Err(invalid_metadata("EXIF metadata exceeds 64 KiB"));
    }
    // Retain strict defaults: a partial parse must not turn a damaged or hidden
    // orientation into identity. The mature parser owns all TIFF interpretation.
    let exif = exif::Reader::new()
        .read_raw(bytes)
        .map_err(|_| invalid_metadata("EXIF metadata is malformed"))?;
    let mut fields = exif.fields();
    if fields.len() > MAX_EXIF_FIELDS {
        return Err(invalid_metadata("EXIF metadata has too many fields"));
    }
    // fields() materializes values lazily, retaining each inside Exif. An ASCII
    // value can expand into a Vec<Vec<u8>>; 64 times the bounded raw bytes plus a
    // small Vec allowance conservatively covers one value before it is read.
    // Recheck actual retained capacities after each field, so multiple fields
    // aliasing the same raw payload cannot accumulate unchecked copies.
    let next_value_allowance = exif.buf().len() as u64 * 64 + 128;
    let mut value_bytes = 0u64;
    let mut orientation = None;
    while fields.len() != 0 {
        if value_bytes.saturating_add(next_value_allowance) > MAX_EXIF_VALUE_BYTES {
            return Err(invalid_metadata(
                "EXIF field values exceed their memory allowance",
            ));
        }
        let field = fields.next().expect("remaining EXIF field");
        value_bytes = value_bytes.saturating_add(exif_value_bytes(&field.value));
        if value_bytes > MAX_EXIF_VALUE_BYTES {
            return Err(invalid_metadata(
                "EXIF field values exceed their memory allowance",
            ));
        }
        if field.tag != exif::Tag::Orientation || field.ifd_num != exif::In::PRIMARY {
            continue;
        }
        if orientation.is_some() {
            return Err(invalid_metadata(
                "EXIF has duplicate primary orientation tags",
            ));
        }
        // EXIF specifies one SHORT. Reject other types/counts explicitly: the
        // image decoder/GPUI would otherwise silently ignore those encodings.
        let exif::Value::Short(values) = &field.value else {
            return Err(invalid_metadata("EXIF orientation must be one SHORT value"));
        };
        if values.len() != 1 {
            return Err(invalid_metadata("EXIF orientation must be one SHORT value"));
        }
        let value = u8::try_from(values[0])
            .ok()
            .and_then(Orientation::from_exif)
            .ok_or_else(|| invalid_metadata("EXIF orientation must be between 1 and 8"))?;
        orientation = Some(value);
    }
    Ok(orientation.unwrap_or(Orientation::NoTransforms))
}

fn exif_value_bytes(value: &exif::Value) -> u64 {
    fn bytes<T>(values: &Vec<T>) -> u64 {
        (values.capacity() as u64).saturating_mul(std::mem::size_of::<T>() as u64)
    }
    match value {
        exif::Value::Byte(values) | exif::Value::Undefined(values, _) => bytes(values),
        exif::Value::Ascii(values) => values.iter().fold(bytes(values), |total, value| {
            total.saturating_add(bytes(value))
        }),
        exif::Value::Short(values) => bytes(values),
        exif::Value::Long(values) => bytes(values),
        exif::Value::Rational(values) => bytes(values),
        exif::Value::SByte(values) => bytes(values),
        exif::Value::SShort(values) => bytes(values),
        exif::Value::SLong(values) => bytes(values),
        exif::Value::SRational(values) => bytes(values),
        exif::Value::Float(values) => bytes(values),
        exif::Value::Double(values) => bytes(values),
        exif::Value::Unknown(_, _, _) => 0,
    }
}

/// Rotation/mirroring never changes pixel count. Only the four quarter-turn
/// orientations exchange width and height (including both diagonal mirrors).
pub(crate) fn oriented_dimensions(dimensions: (u32, u32), orientation: Orientation) -> (u32, u32) {
    match orientation {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => (dimensions.1, dimensions.0),
        Orientation::NoTransforms
        | Orientation::Rotate180
        | Orientation::FlipHorizontal
        | Orientation::FlipVertical => dimensions,
    }
}

/// Caller errors retain their type, so cancellation, deadlines and memory
/// admission do not become indistinguishable codec failures.
#[derive(Debug)]
#[cfg(any(feature = "desktop", test))]
pub(crate) enum RgbaOrientationError<E> {
    Caller(E),
    InvalidBuffer,
    SizeOverflow,
    AllocationFailed,
    ColorSpace(ImageError),
}

#[cfg(any(feature = "desktop", test))]
impl<E: fmt::Display> fmt::Display for RgbaOrientationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Caller(error) => error.fmt(formatter),
            Self::InvalidBuffer => {
                formatter.write_str("image orientation requires exact, nonempty RGBA pixels")
            }
            Self::SizeOverflow => {
                formatter.write_str("image orientation pixel allowance overflows")
            }
            Self::AllocationFailed => {
                formatter.write_str("could not allocate bounded image orientation pixels")
            }
            Self::ColorSpace(error) => {
                write!(formatter, "image orientation color space failed: {error}")
            }
        }
    }
}

/// Apply all eight EXIF orientations to owned, straight or premultiplied RGBA.
/// Channels and color-space metadata are preserved exactly.
///
/// `checkpoint` runs before admission/allocation, after allocation, at least
/// every 4096 visited pixels, and before returning. Codec work and the allocator
/// itself cannot be interrupted here. An error drops the owned pixels; no
/// partial transformed image is returned and no external state is modified.
///
/// `admit_peak` receives simultaneously live RGBA buffer capacities in bytes.
/// The caller must add all of its other retained allocations and apply its own
/// limit. Identity/mirrors/180 degrees reuse the original allocation. Quarter
/// turns admit source capacity plus one exact-sized output before allocation,
/// then recheck the actual output capacity before writing pixels. This bounds
/// requested pixel storage, not allocator bookkeeping or process RSS. No other
/// full-size pixel allocation occurs. Drop decoded intermediates/encoded bytes
/// first or include them in the caller's retained allowance.
#[cfg(any(feature = "desktop", test))]
pub(crate) fn orient_rgba<E>(
    mut source: RgbaImage,
    orientation: Orientation,
    mut checkpoint: impl FnMut() -> Result<(), E>,
    mut admit_peak: impl FnMut(u64) -> Result<(), E>,
) -> Result<RgbaImage, RgbaOrientationError<E>> {
    checkpoint().map_err(RgbaOrientationError::Caller)?;
    let (width, height) = source.dimensions();
    let length = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or(RgbaOrientationError::SizeOverflow)?;
    if length == 0 || source.as_raw().len() != length {
        return Err(RgbaOrientationError::InvalidBuffer);
    }
    let capacity = source.as_raw().capacity() as u64;
    let quarter_turn = matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    );
    let peak = capacity
        .checked_add(if quarter_turn { length as u64 } else { 0 })
        .ok_or(RgbaOrientationError::SizeOverflow)?;
    admit_peak(peak).map_err(RgbaOrientationError::Caller)?;
    checkpoint().map_err(RgbaOrientationError::Caller)?;
    if orientation == Orientation::NoTransforms {
        return Ok(source);
    }

    let width = width as usize;
    let height = height as usize;
    let pixels = length / 4;
    if !quarter_turn {
        let bytes: &mut [u8] = source.as_mut();
        for index in 0..pixels {
            if index % CHECKPOINT_PIXELS == 0 {
                checkpoint().map_err(RgbaOrientationError::Caller)?;
            }
            let paired = match orientation {
                Orientation::Rotate180 => pixels - 1 - index,
                Orientation::FlipHorizontal => {
                    let x = index % width;
                    index - x + (width - 1 - x)
                }
                Orientation::FlipVertical => {
                    let x = index % width;
                    let y = index / width;
                    (height - 1 - y) * width + x
                }
                _ => unreachable!("non-quarter-turn orientations handled above"),
            };
            // Each pair is swapped once; odd middle rows/columns stay intact.
            if index < paired {
                for channel in 0..4 {
                    bytes.swap(index * 4 + channel, paired * 4 + channel);
                }
            }
        }
        checkpoint().map_err(RgbaOrientationError::Caller)?;
        return Ok(source);
    }

    let mut output = Vec::new();
    output
        .try_reserve_exact(length)
        .map_err(|_| RgbaOrientationError::AllocationFailed)?;
    checkpoint().map_err(RgbaOrientationError::Caller)?;
    let actual_peak = capacity
        .checked_add(output.capacity() as u64)
        .ok_or(RgbaOrientationError::SizeOverflow)?;
    admit_peak(actual_peak).map_err(RgbaOrientationError::Caller)?;
    checkpoint().map_err(RgbaOrientationError::Caller)?;
    let raw = source.as_raw();
    // Iterate destination pixels so writes are contiguous and need no zero-fill
    // pass. Inverse mappings match DynamicImage::apply_orientation exactly.
    for index in 0..pixels {
        if index % CHECKPOINT_PIXELS == 0 {
            checkpoint().map_err(RgbaOrientationError::Caller)?;
        }
        let x = index % height;
        let y = index / height;
        let (source_x, source_y) = match orientation {
            Orientation::Rotate90 => (y, height - 1 - x),
            Orientation::Rotate270 => (width - 1 - y, x),
            Orientation::Rotate90FlipH => (y, x),
            Orientation::Rotate270FlipH => (width - 1 - y, height - 1 - x),
            _ => unreachable!("only quarter turns allocate a destination"),
        };
        let offset = (source_y * width + source_x) * 4;
        output.extend_from_slice(&raw[offset..offset + 4]);
    }
    checkpoint().map_err(RgbaOrientationError::Caller)?;
    let color_space = source.color_space();
    drop(source);
    let mut output = RgbaImage::from_raw(height as u32, width as u32, output)
        .ok_or(RgbaOrientationError::InvalidBuffer)?;
    output
        .set_color_space(color_space)
        .map_err(RgbaOrientationError::ColorSpace)?;
    checkpoint().map_err(RgbaOrientationError::Caller)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, ImageReader, Rgba, metadata::Cicp};
    use std::{cell::Cell, io::Cursor};

    fn asymmetric_pixels() -> RgbaImage {
        // Unique values in every channel also catch corruption of transparency.
        RgbaImage::from_fn(3, 2, |x, y| {
            let id = (1 + y * 3 + x) as u8;
            Rgba([id, id * 11, 255 - id, id * 31])
        })
    }

    fn transform(source: RgbaImage, orientation: Orientation) -> RgbaImage {
        orient_rgba(
            source,
            orientation,
            || Ok::<_, &'static str>(()),
            |_| Ok(()),
        )
        .unwrap()
    }

    #[test]
    fn all_eight_orientations_match_asymmetric_pixels_and_image_crate() {
        let cases: [(u8, (u32, u32), [u8; 6]); 8] = [
            (1, (3, 2), [1, 2, 3, 4, 5, 6]),
            (2, (3, 2), [3, 2, 1, 6, 5, 4]),
            (3, (3, 2), [6, 5, 4, 3, 2, 1]),
            (4, (3, 2), [4, 5, 6, 1, 2, 3]),
            (5, (2, 3), [1, 4, 2, 5, 3, 6]),
            (6, (2, 3), [4, 1, 5, 2, 6, 3]),
            (7, (2, 3), [6, 3, 5, 2, 4, 1]),
            (8, (2, 3), [3, 6, 2, 5, 1, 4]),
        ];
        for (exif, dimensions, expected) in cases {
            let orientation = Orientation::from_exif(exif).unwrap();
            let transformed = transform(asymmetric_pixels(), orientation);
            assert_eq!(transformed.dimensions(), dimensions, "EXIF {exif}");
            assert_eq!(
                transformed
                    .pixels()
                    .map(|pixel| pixel[0])
                    .collect::<Vec<_>>(),
                expected,
                "EXIF {exif}"
            );
            let mut reference = DynamicImage::ImageRgba8(asymmetric_pixels());
            reference.apply_orientation(orientation);
            assert_eq!(transformed, reference.into_rgba8(), "EXIF {exif}");
        }
    }

    #[test]
    fn logical_dimensions_swap_only_for_quarter_turns() {
        for exif in 1..=8 {
            let orientation = Orientation::from_exif(exif).unwrap();
            let expected = if exif >= 5 { (2, 3) } else { (3, 2) };
            assert_eq!(oriented_dimensions((3, 2), orientation), expected);
            assert_eq!(oriented_dimensions((5, 5), orientation), (5, 5));
        }
    }

    fn jpeg_with_exif(exif: Option<&[u8]>) -> Vec<u8> {
        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut encoded)
            .encode_image(&DynamicImage::ImageRgba8(asymmetric_pixels()).into_rgb8())
            .unwrap();
        if let Some(exif) = exif {
            let mut segment = vec![0xff, 0xe1];
            segment.extend_from_slice(&((exif.len() + 8) as u16).to_be_bytes());
            segment.extend_from_slice(b"Exif\0\0");
            segment.extend_from_slice(exif);
            encoded.splice(2..2, segment);
        }
        encoded
    }

    fn exif_with_orientation(orientation: u8) -> Vec<u8> {
        let mut exif = b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x01\0\0\0\0\0\0\0".to_vec();
        exif[18] = orientation;
        exif
    }

    fn metadata(bytes: &[u8]) -> ImageResult<ImageOrientation> {
        let mut decoder =
            ImageReader::with_format(Cursor::new(bytes), ImageFormat::Jpeg).into_decoder()?;
        ImageOrientation::from_decoder(&mut decoder)
    }

    #[test]
    fn jpeg_metadata_preserves_raw_dimensions_and_all_eight_orientations() {
        for exif in 1..=8 {
            let bytes = jpeg_with_exif(Some(&exif_with_orientation(exif)));
            let original = bytes.clone();
            let info = metadata(&bytes).unwrap();
            assert_eq!(info.raw_dimensions, (3, 2));
            assert_eq!(info.orientation, Orientation::from_exif(exif).unwrap());
            assert_eq!(
                info.oriented_dimensions,
                if exif >= 5 { (2, 3) } else { (3, 2) }
            );
            assert_eq!(bytes, original);
        }
        let info = metadata(&jpeg_with_exif(None)).unwrap();
        assert_eq!(info.orientation, Orientation::NoTransforms);
        assert_eq!(info.raw_dimensions, info.oriented_dimensions);
    }

    #[test]
    fn invalid_or_ambiguous_exif_is_never_silently_identity() {
        for value in [0, 9, 255] {
            assert!(metadata(&jpeg_with_exif(Some(&exif_with_orientation(value)))).is_err());
        }
        for exif in [
            b"broken EXIF".as_slice(),
            b"II*\0\x08\0\0\0\x01\0\x12\x01", // Truncated entry.
        ] {
            assert!(metadata(&jpeg_with_exif(Some(exif))).is_err());
        }
        assert!(metadata(b"not a JPEG").is_err());
    }

    #[test]
    fn valid_exif_without_orientation_is_upright() {
        let no_fields = b"II*\0\x08\0\0\0\0\0\0\0\0\0";
        let mut other_field = exif_with_orientation(6);
        other_field[10..12].copy_from_slice(&0x0100u16.to_le_bytes()); // ImageWidth.
        for exif in [no_fields.as_slice(), other_field.as_slice()] {
            let info = metadata(&jpeg_with_exif(Some(exif))).unwrap();
            assert_eq!(info.orientation, Orientation::NoTransforms);
            assert_eq!(info.raw_dimensions, info.oriented_dimensions);
        }
    }

    #[test]
    fn big_endian_and_thumbnail_ifds_use_primary_orientation() {
        let big_endian = b"MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x06\0\0\0\0\0\0";
        let info = metadata(&jpeg_with_exif(Some(big_endian))).unwrap();
        assert_eq!(info.orientation, Orientation::Rotate90);
        assert_eq!(info.oriented_dimensions, (2, 3));

        // An empty primary IFD points at an independently oriented thumbnail.
        let mut thumbnail = b"II*\0\x08\0\0\0\0\0\x0e\0\0\0".to_vec();
        thumbnail.extend_from_slice(&exif_with_orientation(6)[8..]);
        let info = metadata(&jpeg_with_exif(Some(&thumbnail))).unwrap();
        assert_eq!(info.orientation, Orientation::NoTransforms);
        assert_eq!(info.oriented_dimensions, (3, 2));
    }

    #[test]
    fn duplicate_orientation_and_invalid_types_or_counts_fail() {
        let mut duplicate = exif_with_orientation(6);
        duplicate[8..10].copy_from_slice(&2u16.to_le_bytes());
        let entry = duplicate[10..22].to_vec();
        duplicate.splice(22..22, entry);
        assert!(metadata(&jpeg_with_exif(Some(&duplicate))).is_err());
        duplicate[30] = 8; // Conflicting duplicate is rejected too.
        assert!(metadata(&jpeg_with_exif(Some(&duplicate))).is_err());
        for (field_type, count) in [(1u16, 1u32), (4, 1), (3, 0), (3, 2)] {
            let mut exif = exif_with_orientation(6);
            exif[12..14].copy_from_slice(&field_type.to_le_bytes());
            exif[14..18].copy_from_slice(&count.to_le_bytes());
            assert!(metadata(&jpeg_with_exif(Some(&exif))).is_err());
        }
        assert!(parse_exif_orientation(vec![0; MAX_EXIF_BYTES + 1]).is_err());
    }

    fn exif_with_shared_payload(fields: u16, payload_bytes: u32) -> Vec<u8> {
        let mut raw = b"II*\0\x08\0\0\0".to_vec();
        raw.extend_from_slice(&fields.to_le_bytes());
        let payload_offset = 8 + 2 + u32::from(fields) * 12 + 4;
        for _ in 0..fields {
            raw.extend_from_slice(&0xc001u16.to_le_bytes()); // Unrelated private tag.
            raw.extend_from_slice(&7u16.to_le_bytes()); // UNDEFINED bytes.
            raw.extend_from_slice(&payload_bytes.to_le_bytes());
            raw.extend_from_slice(&payload_offset.to_le_bytes());
        }
        raw.extend_from_slice(&0u32.to_le_bytes());
        raw.resize((payload_offset + payload_bytes) as usize, 42);
        raw
    }

    #[test]
    fn metadata_field_count_and_aliased_value_allocations_are_bounded() {
        let excessive_fields = exif_with_shared_payload(MAX_EXIF_FIELDS as u16 + 1, 8);
        assert!(excessive_fields.len() < MAX_EXIF_BYTES);
        assert!(parse_exif_orientation(excessive_fields).is_err());

        let aliased_values = exif_with_shared_payload(128, 60_000);
        assert!(aliased_values.len() < MAX_EXIF_BYTES);
        assert!(parse_exif_orientation(aliased_values).is_err());
        // A normal large field remains supported within the same raw limit.
        assert_eq!(
            parse_exif_orientation(exif_with_shared_payload(1, 60_000)).unwrap(),
            Orientation::NoTransforms
        );
    }

    #[test]
    fn admission_counts_source_capacity_and_only_quarter_turn_destination() {
        for exif in 1..=8 {
            let orientation = Orientation::from_exif(exif).unwrap();
            let mut raw = asymmetric_pixels().into_raw();
            raw.reserve_exact(100);
            let capacity = raw.capacity() as u64;
            let source = RgbaImage::from_raw(3, 2, raw).unwrap();
            let pointer = source.as_raw().as_ptr();
            let mut peaks = Vec::new();
            let output = orient_rgba(
                source,
                orientation,
                || Ok::<_, &'static str>(()),
                |peak| {
                    peaks.push(peak);
                    Ok(())
                },
            )
            .unwrap();
            let expected = capacity + if exif >= 5 { 24 } else { 0 };
            assert_eq!(peaks[0], expected);
            if exif < 5 {
                assert_eq!(peaks, [capacity]);
                assert_eq!(output.as_raw().as_ptr(), pointer);
            } else {
                assert_eq!(peaks.len(), 2);
                assert_eq!(peaks[1], capacity + output.as_raw().capacity() as u64);
            }
        }
    }

    #[test]
    fn rejection_and_cancellation_preserve_caller_errors() {
        let admitted = Cell::new(false);
        let cancelled = orient_rgba(
            asymmetric_pixels(),
            Orientation::Rotate90,
            || Err("cancelled before work"),
            |_| {
                admitted.set(true);
                Ok(())
            },
        );
        assert!(matches!(
            cancelled,
            Err(RgbaOrientationError::Caller("cancelled before work"))
        ));
        assert!(!admitted.get());

        let rejected = orient_rgba(
            asymmetric_pixels(),
            Orientation::Rotate90,
            || Ok(()),
            |_| Err("peak exceeds budget"),
        );
        assert!(matches!(
            rejected,
            Err(RgbaOrientationError::Caller("peak exceeds budget"))
        ));

        // Interrupt after transformation begins, including in-place variants.
        for orientation in [Orientation::FlipHorizontal, Orientation::Rotate90] {
            let admitted = Cell::new(false);
            let checks_after_admission = Cell::new(0);
            let result = orient_rgba(
                RgbaImage::from_pixel(257, 129, Rgba([1, 2, 3, 4])),
                orientation,
                || {
                    if admitted.get() {
                        checks_after_admission.set(checks_after_admission.get() + 1);
                    }
                    if checks_after_admission.get() == 7 {
                        Err("deadline")
                    } else {
                        Ok(())
                    }
                },
                |_| {
                    admitted.set(true);
                    Ok(())
                },
            );
            assert!(matches!(
                result,
                Err(RgbaOrientationError::Caller("deadline"))
            ));
        }
    }

    #[test]
    fn single_axes_odd_dimensions_and_color_space_are_preserved() {
        for dimensions in [(1, 1), (1, 5), (5, 1), (5, 3)] {
            for exif in 1..=8 {
                let orientation = Orientation::from_exif(exif).unwrap();
                let mut source = RgbaImage::from_fn(dimensions.0, dimensions.1, |x, y| {
                    Rgba([x as u8, y as u8, 123, 79])
                });
                source.set_color_space(Cicp::DISPLAY_P3).unwrap();
                let mut reference = DynamicImage::ImageRgba8(source.clone());
                reference.apply_orientation(orientation);
                let output = transform(source, orientation);
                assert_eq!(output.as_raw(), reference.into_rgba8().as_raw());
                assert_eq!(output.color_space(), Cicp::DISPLAY_P3);
            }
        }
    }

    #[test]
    fn empty_and_surplus_pixel_buffers_are_rejected() {
        for source in [
            RgbaImage::new(0, 1),
            RgbaImage::from_raw(1, 1, vec![0; 8]).unwrap(),
        ] {
            let result = orient_rgba(
                source,
                Orientation::NoTransforms,
                || Ok::<_, &'static str>(()),
                |_| panic!("invalid storage is not admitted"),
            );
            assert!(matches!(result, Err(RgbaOrientationError::InvalidBuffer)));
        }
    }
}

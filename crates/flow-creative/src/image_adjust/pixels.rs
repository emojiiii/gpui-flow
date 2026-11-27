use super::{IMAGE_ADJUST_PREVIEW_EDGE, ImageAdjustSpec};
use crate::{
    ProjectAsset,
    processing::{
        MAX_PROCESS_IMAGE_BYTES, PROCESS_IMAGE_TIMEOUT, ProcessingError, ProcessingOutput,
        image as processing_image,
    },
};
use ::image::RgbaImage;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

/// Adjust a decoded base preview of at most 768 pixels per edge. Load the base
/// with image_edit::load_preview, then reuse it for each slider change; never
/// repeatedly adjust the previous result. There is no file I/O or resizing.
/// Run on the host's background preview worker and discard stale results.
pub fn adjust_preview(
    base: &RgbaImage,
    spec: &ImageAdjustSpec,
    cancel: &AtomicBool,
) -> Result<RgbaImage, ProcessingError> {
    if base.width().max(base.height()) > IMAGE_ADJUST_PREVIEW_EDGE {
        return Err(ProcessingError::ResourceLimit(
            "image adjustment preview bases must be at most 768 pixels per edge".into(),
        ));
    }
    adjust_pixels(base, spec, cancel, Instant::now())
}

/// Adjust the full-resolution immutable source and write a fresh RGBA8 PNG.
/// The shared decoder checks host paths, static PNG/JPEG, 32 MiB encoded input,
/// 128 MiB decoder storage, 16 MP, the processing gate, and a whole-job 60-second
/// deadline. Preview pixels are never used as export input. Decoded RGB bytes
/// are treated as sRGB; this operation adds no ICC color-profile conversion.
///
/// Dimensions and alpha are preserved. The shared writer refuses existing
/// files and source aliases, limits encoded output to 32 MiB, and removes new
/// partial files on errors/cancellation. The completed file belongs to the
/// caller, which must import it before changing any project reference. Neutral
/// settings preserve decoded RGBA bytes, not original PNG/JPEG encoding bytes.
pub fn write_adjusted(
    path: &Path,
    asset: &ProjectAsset,
    spec: &ImageAdjustSpec,
    output: &Path,
    cancel: &AtomicBool,
) -> Result<ProcessingOutput, ProcessingError> {
    spec.output_dimensions(asset.width, asset.height)?;
    // Reject impossible asset metadata before acquiring the shared processor.
    // The shared decoder independently checks the actual file and byte count.
    if asset.byte_len == 0 || asset.byte_len > MAX_PROCESS_IMAGE_BYTES {
        return Err(ProcessingError::ResourceLimit(
            "image adjustment sources must contain at most 32 MiB of encoded bytes".into(),
        ));
    }
    processing_image::with_decoded_source(path, asset, cancel, |rgba, source, start| {
        adjust_and_write(rgba, source, spec, output, cancel, start)
    })
}

fn adjust_and_write(
    rgba: RgbaImage,
    source: &Path,
    spec: &ImageAdjustSpec,
    output: &Path,
    cancel: &AtomicBool,
    start: Instant,
) -> Result<ProcessingOutput, ProcessingError> {
    let adjusted = adjust_pixels(&rgba, spec, cancel, start)?;
    drop(rgba);
    processing_image::write_rgba_png(source, &adjusted, output, cancel, start)
}

// One fallible bounded output allocation, no per-pixel allocations, and no
// full-resolution intermediate image. The 256-entry input table is on stack.
fn adjust_pixels(
    source: &RgbaImage,
    spec: &ImageAdjustSpec,
    cancel: &AtomicBool,
    start: Instant,
) -> Result<RgbaImage, ProcessingError> {
    check_work(cancel, start)?;
    let (width, height) = spec.output_dimensions(source.width(), source.height())?;
    let row_bytes = width as usize * 4;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(row_bytes * height as usize)
        .map_err(|_| {
            ProcessingError::ResourceLimit("could not allocate bounded adjusted pixels".into())
        })?;
    let identity = spec.is_identity();
    let exposure = f64::from(spec.exposure).exp2();
    let light = f64::from(spec.light);
    let mut input = [0.0; 256];
    if !identity {
        for (byte, value) in input.iter_mut().enumerate() {
            let exposed = (srgb_to_linear(byte as u8) * exposure).clamp(0.0, 1.0);
            *value = if light >= 0.0 {
                exposed + (1.0 - exposed) * light
            } else {
                exposed * (1.0 + light)
            };
        }
    }
    for y in 0..height {
        check_work(cancel, start)?;
        let offset = y as usize * row_bytes;
        let row = &source.as_raw()[offset..offset + row_bytes];
        if identity {
            pixels.extend_from_slice(row);
            continue;
        }
        let ny = centered_coordinate(y, height);
        for (x, pixel) in row.as_chunks::<4>().0.iter().enumerate() {
            // Keep hidden RGB exactly, even when visible colors are adjusted.
            if pixel[3] == 0 {
                pixels.extend_from_slice(pixel);
                continue;
            }
            let mut rgb = [
                input[usize::from(pixel[0])],
                input[usize::from(pixel[1])],
                input[usize::from(pixel[2])],
            ];
            if spec.saturation != 1.0 {
                scale_saturation(&mut rgb, f64::from(spec.saturation));
            }
            if spec.vibrance != 0.0 {
                let maximum = rgb[0].max(rgb[1]).max(rgb[2]);
                let minimum = rgb[0].min(rgb[1]).min(rgb[2]);
                let chroma = if maximum > 0.0 {
                    (maximum - minimum) / maximum
                } else {
                    0.0
                };
                scale_saturation(&mut rgb, 1.0 + f64::from(spec.vibrance) * (1.0 - chroma));
            }
            if spec.vignette != 0.0 {
                let nx = centered_coordinate(x as u32, width);
                let radius_squared = (nx * nx + ny * ny).min(1.0);
                let falloff = radius_squared * radius_squared * (3.0 - 2.0 * radius_squared);
                let gain = 1.0 - 0.8 * f64::from(spec.vignette) * falloff;
                for channel in &mut rgb {
                    *channel *= gain;
                }
            }
            pixels.extend_from_slice(&[
                linear_to_srgb(rgb[0]),
                linear_to_srgb(rgb[1]),
                linear_to_srgb(rgb[2]),
                pixel[3],
            ]);
        }
    }
    check_work(cancel, start)?;
    RgbaImage::from_raw(width, height, pixels).ok_or_else(|| {
        ProcessingError::Process("adjusted pixel storage has unexpected dimensions".into())
    })
}

// IEC sRGB transfer functions, with normalized linear-light Rec.709 luminance.
// Exposure clips before light; saturation and vibrance each clip after scaling.
// Clipping is intentional and can change luminance for out-of-gamut colors.
fn srgb_to_linear(byte: u8) -> f64 {
    let value = f64::from(byte) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f64) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.0031308 {
        12.92 * value
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

fn luminance(rgb: [f64; 3]) -> f64 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}

fn scale_saturation(rgb: &mut [f64; 3], scale: f64) {
    let gray = luminance(*rgb);
    for channel in rgb {
        *channel = (gray + (*channel - gray) * scale).clamp(0.0, 1.0);
    }
}

// The outer pixel centers map to +/-1; a one-pixel axis stays at zero.
// Vignette uses smoothstep on squared elliptical radius. At or outside the
// unit ellipse, strength 1 removes 80% of linear light, without altering alpha.
fn centered_coordinate(pixel: u32, extent: u32) -> f64 {
    if extent == 1 {
        0.0
    } else {
        2.0 * f64::from(pixel) / f64::from(extent - 1) - 1.0
    }
}

fn check_work(cancel: &AtomicBool, start: Instant) -> Result<(), ProcessingError> {
    if cancel.load(Ordering::Acquire) {
        Err(ProcessingError::Cancelled)
    } else if start.elapsed() >= PROCESS_IMAGE_TIMEOUT {
        Err(ProcessingError::Timeout)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::image::Rgba;
    use std::{fs, path::PathBuf, sync::atomic::AtomicU64};

    fn apply(source: &RgbaImage, spec: ImageAdjustSpec) -> RgbaImage {
        adjust_preview(source, &spec, &AtomicBool::new(false)).unwrap()
    }

    fn pixel_luminance(pixel: &Rgba<u8>) -> f64 {
        luminance([
            srgb_to_linear(pixel[0]),
            srgb_to_linear(pixel[1]),
            srgb_to_linear(pixel[2]),
        ])
    }

    #[test]
    fn neutral_copies_all_byte_values_and_alpha_without_mutating_the_base() {
        let source = RgbaImage::from_fn(256, 2, |x, y| {
            Rgba([x as u8, 255 - x as u8, (x / 2) as u8, (x * y) as u8])
        });
        let original = source.clone();
        let result = apply(&source, ImageAdjustSpec::default());
        assert_eq!(result.dimensions(), source.dimensions());
        assert_eq!(result.as_raw(), source.as_raw());
        assert_eq!(source, original);
    }

    #[test]
    fn exposure_is_in_linear_light_stops_and_light_reaches_black_or_white() {
        let source = RgbaImage::from_pixel(1, 1, Rgba([64, 128, 255, 123]));
        for (exposure, expected) in [(1.0, [90, 176, 255, 123]), (-1.0, [44, 92, 188, 123])] {
            assert_eq!(
                apply(
                    &source,
                    ImageAdjustSpec {
                        exposure,
                        ..ImageAdjustSpec::default()
                    }
                )
                .get_pixel(0, 0)
                .0,
                expected
            );
        }
        for (light, expected) in [(-1.0, [0, 0, 0, 123]), (1.0, [255, 255, 255, 123])] {
            assert_eq!(
                apply(
                    &source,
                    ImageAdjustSpec {
                        light,
                        ..ImageAdjustSpec::default()
                    }
                )
                .get_pixel(0, 0)
                .0,
                expected
            );
        }
        for (byte, light) in [(0, 0.5), (255, -0.5)] {
            let source = RgbaImage::from_pixel(1, 1, Rgba([byte, byte, byte, 255]));
            assert_eq!(
                apply(
                    &source,
                    ImageAdjustSpec {
                        light,
                        ..ImageAdjustSpec::default()
                    }
                )
                .get_pixel(0, 0)
                .0,
                [188, 188, 188, 255]
            );
        }
    }

    #[test]
    fn desaturation_uses_rec709_luminance_and_preserves_unclipped_luminance() {
        let source = RgbaImage::from_raw(
            4,
            1,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 128, 128, 128, 255,
            ],
        )
        .unwrap();
        let gray = apply(
            &source,
            ImageAdjustSpec {
                saturation: 0.0,
                ..ImageAdjustSpec::default()
            },
        );
        for (x, byte) in [127, 220, 76, 128].into_iter().enumerate() {
            assert_eq!(gray.get_pixel(x as u32, 0).0, [byte, byte, byte, 255]);
        }
        let muted = RgbaImage::from_pixel(1, 1, Rgba([130, 120, 110, 255]));
        let vivid = apply(
            &muted,
            ImageAdjustSpec {
                saturation: 2.0,
                ..ImageAdjustSpec::default()
            },
        );
        assert!(vivid.get_pixel(0, 0)[0] > 130);
        assert!(vivid.get_pixel(0, 0)[2] < 110);
        assert!(
            (pixel_luminance(muted.get_pixel(0, 0)) - pixel_luminance(vivid.get_pixel(0, 0))).abs()
                < 0.004
        );
    }

    #[test]
    fn vibrance_changes_muted_color_while_protecting_primaries_and_gray() {
        let protected = RgbaImage::from_raw(
            4,
            1,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 128, 128, 128, 255,
            ],
        )
        .unwrap();
        let muted = RgbaImage::from_pixel(1, 1, Rgba([150, 130, 120, 255]));
        for vibrance in [-1.0, 1.0] {
            let spec = ImageAdjustSpec {
                vibrance,
                ..ImageAdjustSpec::default()
            };
            assert_eq!(apply(&protected, spec), protected);
            let result = apply(&muted, spec);
            let pixel = result.get_pixel(0, 0);
            let spread = i16::from(pixel[0]) - i16::from(pixel[2]);
            assert!(if vibrance > 0.0 {
                spread > 30
            } else {
                spread < 30
            });
            assert!(
                (pixel_luminance(muted.get_pixel(0, 0)) - pixel_luminance(pixel)).abs() < 0.004
            );
        }
    }

    #[test]
    fn vignette_is_symmetric_elliptical_and_leaves_the_center_unchanged() {
        let spec = ImageAdjustSpec {
            vignette: 1.0,
            ..ImageAdjustSpec::default()
        };
        let source = RgbaImage::from_pixel(9, 5, Rgba([255, 255, 255, 127]));
        let result = apply(&source, spec);
        assert_eq!(result.get_pixel(4, 2).0, [255, 255, 255, 127]);
        assert_eq!(result.get_pixel(0, 2), result.get_pixel(4, 0));
        assert_eq!(result.get_pixel(0, 0).0, [124, 124, 124, 127]);
        assert!(result.get_pixel(2, 2)[0] > result.get_pixel(0, 2)[0]);
        assert!(result.get_pixel(2, 2)[0] < result.get_pixel(4, 2)[0]);
        for y in 0..5 {
            for x in 0..9 {
                assert_eq!(result.get_pixel(x, y), result.get_pixel(8 - x, 4 - y));
            }
        }
        let singleton = RgbaImage::from_pixel(1, 1, Rgba([90, 150, 220, 42]));
        assert_eq!(apply(&singleton, spec), singleton);
    }

    #[test]
    fn combined_adjustments_preserve_all_alpha_and_hidden_rgb() {
        let source = RgbaImage::from_fn(256, 1, |x, _| Rgba([90, 150, 220, x as u8]));
        let original = source.clone();
        for spec in [
            ImageAdjustSpec {
                light: 0.2,
                exposure: 0.5,
                saturation: 0.75,
                vibrance: 0.6,
                vignette: 0.8,
            },
            ImageAdjustSpec {
                light: -1.0,
                exposure: -4.0,
                saturation: 0.0,
                vibrance: -1.0,
                vignette: 1.0,
            },
            ImageAdjustSpec {
                light: 1.0,
                exposure: 4.0,
                saturation: 2.0,
                vibrance: 1.0,
                vignette: 0.0,
            },
        ] {
            let result = apply(&source, spec);
            assert_eq!(result.dimensions(), source.dimensions());
            assert_eq!(result.get_pixel(0, 0), source.get_pixel(0, 0));
            for (before, after) in source.pixels().zip(result.pixels()) {
                assert_eq!(before[3], after[3]);
            }
            assert_ne!(result.get_pixel(128, 0), source.get_pixel(128, 0));
        }
        assert_eq!(source, original);
    }

    #[test]
    fn preview_limits_cancellation_and_whole_job_deadline_are_enforced() {
        let cancel = AtomicBool::new(false);
        for spec in [
            ImageAdjustSpec::default(),
            ImageAdjustSpec {
                exposure: 1.0,
                ..ImageAdjustSpec::default()
            },
        ] {
            for dimensions in [(0, 0), (769, 1), (1, 769)] {
                assert!(matches!(
                    adjust_preview(&RgbaImage::new(dimensions.0, dimensions.1), &spec, &cancel),
                    Err(ProcessingError::ResourceLimit(_))
                ));
            }
            let source = RgbaImage::from_pixel(2, 3, Rgba([100, 150, 200, 255]));
            assert!(matches!(
                adjust_preview(&source, &spec, &AtomicBool::new(true)),
                Err(ProcessingError::Cancelled)
            ));
            assert!(matches!(
                adjust_pixels(
                    &source,
                    &spec,
                    &cancel,
                    Instant::now() - PROCESS_IMAGE_TIMEOUT
                ),
                Err(ProcessingError::Timeout)
            ));
        }
        let maximum = RgbaImage::new(IMAGE_ADJUST_PREVIEW_EDGE, IMAGE_ADJUST_PREVIEW_EDGE);
        assert_eq!(
            apply(&maximum, ImageAdjustSpec::default()).dimensions(),
            maximum.dimensions()
        );
        assert!(matches!(
            adjust_preview(
                &RgbaImage::new(1, 1),
                &ImageAdjustSpec {
                    exposure: f32::NAN,
                    ..ImageAdjustSpec::default()
                },
                &cancel
            ),
            Err(ProcessingError::InvalidSettings(_))
        ));
    }

    #[test]
    fn export_rejects_invalid_metadata_before_opening_or_allocating() {
        let cancel = AtomicBool::new(false);
        let spec = ImageAdjustSpec::default();
        let path = Path::new("unused-source.png");
        let output = Path::new("unused-output.png");
        for (width, height, byte_len) in [
            (4097, 4096, 1),
            (16_385, 1, 1),
            (0, 1, 1),
            (1, 1, 0),
            (1, 1, MAX_PROCESS_IMAGE_BYTES + 1),
        ] {
            let asset = ProjectAsset {
                key: "assets/source.png".into(),
                name: "source".into(),
                media_type: "image/png".into(),
                byte_len,
                width,
                height,
            };
            assert!(matches!(
                write_adjusted(path, &asset, &spec, output, &cancel),
                Err(ProcessingError::ResourceLimit(_))
            ));
        }
    }

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "flow-image-adjust-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn adjusted_png_roundtrips_exposure_and_neutral_rgba() {
        let directory = TestDirectory::new();
        let source = directory.0.join("source.png");
        let base = RgbaImage::from_raw(
            2,
            2,
            vec![
                10, 20, 30, 0, 64, 128, 255, 63, 130, 120, 110, 128, 255, 0, 0, 255,
            ],
        )
        .unwrap();
        base.save_with_format(&source, ::image::ImageFormat::Png)
            .unwrap();
        let cancel = AtomicBool::new(false);
        let spec = ImageAdjustSpec {
            exposure: 1.0,
            ..ImageAdjustSpec::default()
        };
        let output = directory.0.join("adjusted.png");
        // Exercise the same decoded-source callback as export without competing
        // for the process-wide try-lock with independent media tests.
        let result = adjust_and_write(
            base.clone(),
            &source,
            &spec,
            &output,
            &cancel,
            Instant::now(),
        )
        .unwrap();
        assert_eq!(result.media_type, "image/png");
        assert_eq!((result.width, result.height), base.dimensions());
        assert_eq!(result.byte_len, fs::metadata(&output).unwrap().len());
        let decoded = ::image::open(&output).unwrap();
        assert_eq!(decoded.color(), ::image::ColorType::Rgba8);
        assert_eq!(decoded.into_rgba8(), apply(&base, spec));
        let neutral_output = directory.0.join("neutral.png");
        adjust_and_write(
            base.clone(),
            &source,
            &ImageAdjustSpec::default(),
            &neutral_output,
            &cancel,
            Instant::now(),
        )
        .unwrap();
        assert_eq!(::image::open(&neutral_output).unwrap().into_rgba8(), base);
        let cancelled = directory.0.join("cancelled.png");
        let asset = ProjectAsset {
            key: "assets/source.png".into(),
            name: "source".into(),
            media_type: "image/png".into(),
            byte_len: fs::metadata(&source).unwrap().len(),
            width: 2,
            height: 2,
        };
        assert!(matches!(
            write_adjusted(&source, &asset, &spec, &cancelled, &AtomicBool::new(true)),
            Err(ProcessingError::Cancelled)
        ));
        assert!(!cancelled.exists());
        let expired = directory.0.join("expired.png");
        assert!(matches!(
            adjust_and_write(
                base,
                &source,
                &spec,
                &expired,
                &cancel,
                Instant::now() - PROCESS_IMAGE_TIMEOUT
            ),
            Err(ProcessingError::Timeout)
        ));
        assert!(!expired.exists());
    }
}

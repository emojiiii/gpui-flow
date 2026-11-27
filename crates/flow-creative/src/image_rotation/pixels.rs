use super::ImageRotationSpec;
use crate::{
    ProjectAsset,
    image_edit::IMAGE_EDIT_PREVIEW_EDGE,
    processing::{
        PROCESS_IMAGE_TIMEOUT, ProcessingError, ProcessingOutput, image as processing_image,
    },
};
use ::image::RgbaImage;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

/// Rotate an already-decoded, unedited base preview. Load that base using
/// image_edit::load_preview on a background worker, and reuse it for every
/// angle change; never repeatedly resample the previous rotated result.
/// Inputs are limited to 768 per edge; the expanded result can reach 1087.
/// This function does no file I/O. Hosts should run its bounded CPU work on
/// their existing preview worker and discard stale/cancelled results.
pub fn rotate_preview(
    base: &RgbaImage,
    spec: &ImageRotationSpec,
    cancel: &AtomicBool,
) -> Result<RgbaImage, ProcessingError> {
    if base.width().max(base.height()) > IMAGE_EDIT_PREVIEW_EDGE {
        return Err(ProcessingError::ResourceLimit(
            "image rotation preview bases must be at most 768 pixels per edge".into(),
        ));
    }
    rotate_pixels(base, spec, cancel, Instant::now())
}

/// Rotate the full-resolution immutable asset on a background worker and
/// write a fresh RGBA PNG, never a preview export. The shared decoder enforces
/// static PNG/JPEG, checked host paths, 32 MiB input, 128 MiB decoded storage,
/// the processing gate and a whole-job 60-second deadline. Rotation separately
/// bounds both source and expanded output to 16 MP / 16384 per edge.
///
/// Existing files, source aliases and originals are never overwritten. The
/// shared writer limits encoded output to 32 MiB and removes new partial files
/// on failure/cancellation. A completed file belongs to the host, which must
/// import it before changing any project reference.
pub fn write_rotated(
    path: &Path,
    asset: &ProjectAsset,
    spec: &ImageRotationSpec,
    output: &Path,
    cancel: &AtomicBool,
) -> Result<ProcessingOutput, ProcessingError> {
    spec.output_dimensions(asset.width, asset.height)?;
    processing_image::with_decoded_source(path, asset, cancel, |rgba, source, start| {
        let rotated = rotate_pixels(&rgba, spec, cancel, start)?;
        drop(rgba);
        processing_image::write_rgba_png(source, &rotated, output, cancel, start)
    })
}

// One bounded output allocation (at most 64 MiB), four stack-only sampling
// taps, and no per-pixel allocation or intermediate full-resolution image.
fn rotate_pixels(
    source: &RgbaImage,
    spec: &ImageRotationSpec,
    cancel: &AtomicBool,
    start: Instant,
) -> Result<RgbaImage, ProcessingError> {
    check_work(cancel, start)?;
    let (width, height) = spec.output_dimensions(source.width(), source.height())?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(width as usize * height as usize * 4)
        .map_err(|_| {
            ProcessingError::ResourceLimit("could not allocate bounded rotated pixels".into())
        })?;
    let turns = spec.quarter_turns();
    let (sin, cos) = spec.degrees.to_radians().sin_cos();
    let source_center_x = f64::from(source.width()) / 2.0;
    let source_center_y = f64::from(source.height()) / 2.0;
    let output_center_x = f64::from(width) / 2.0;
    let output_center_y = f64::from(height) / 2.0;
    for y in 0..height {
        check_work(cancel, start)?;
        for x in 0..width {
            if let Some(turns) = turns {
                let (source_x, source_y) = match turns {
                    0 => (x, y),
                    1 => (y, source.height() - 1 - x),
                    2 => (source.width() - 1 - x, source.height() - 1 - y),
                    3 => (source.width() - 1 - y, x),
                    _ => unreachable!("validated cardinal rotation"),
                };
                pixels.extend_from_slice(&source.get_pixel(source_x, source_y).0);
            } else {
                // Output centers are (x + .5, y + .5). Inverse clockwise
                // rotation maps to source centers; subtract .5 to index them.
                let dx = f64::from(x) + 0.5 - output_center_x;
                let dy = f64::from(y) + 0.5 - output_center_y;
                let sx = cos * dx + sin * dy + source_center_x - 0.5;
                let sy = -sin * dx + cos * dy + source_center_y - 0.5;
                pixels.extend_from_slice(&sample_bilinear(source, sx, sy));
            }
        }
    }
    check_work(cancel, start)?;
    RgbaImage::from_raw(width, height, pixels).ok_or_else(|| {
        ProcessingError::Process("rotated pixel storage has unexpected dimensions".into())
    })
}

// Missing taps are transparent black. Interpolate alpha and alpha-weighted
// sRGB byte values, then unpremultiply and round to the nearest byte. This is
// deterministic byte-space filtering, not linear-light color correction.
// Fully transparent results have zero RGB, so hidden source colors cannot
// bleed into visible edges. Exact cardinal copies retain hidden RGB instead.
fn sample_bilinear(source: &RgbaImage, x: f64, y: f64) -> [u8; 4] {
    let left = x.floor() as i64;
    let top = y.floor() as i64;
    let fx = x - left as f64;
    let fy = y - top as f64;
    let mut alpha = 0.0;
    let mut premultiplied = [0.0; 3];
    for (sx, sy, weight) in [
        (left, top, (1.0 - fx) * (1.0 - fy)),
        (left + 1, top, fx * (1.0 - fy)),
        (left, top + 1, (1.0 - fx) * fy),
        (left + 1, top + 1, fx * fy),
    ] {
        if sx < 0 || sy < 0 || sx >= i64::from(source.width()) || sy >= i64::from(source.height()) {
            continue;
        }
        let pixel = source.get_pixel(sx as u32, sy as u32);
        let weighted_alpha = f64::from(pixel[3]) * weight;
        alpha += weighted_alpha;
        for channel in 0..3 {
            premultiplied[channel] += f64::from(pixel[channel]) * weighted_alpha;
        }
    }
    let output_alpha = alpha.round().clamp(0.0, 255.0) as u8;
    if output_alpha == 0 {
        return [0; 4];
    }
    [
        (premultiplied[0] / alpha).round().clamp(0.0, 255.0) as u8,
        (premultiplied[1] / alpha).round().clamp(0.0, 255.0) as u8,
        (premultiplied[2] / alpha).round().clamp(0.0, 255.0) as u8,
        output_alpha,
    ]
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

    const COLORS: [[u8; 4]; 6] = [
        [10, 11, 12, 0],
        [20, 21, 22, 63],
        [30, 31, 32, 127],
        [40, 41, 42, 128],
        [50, 51, 52, 192],
        [60, 61, 62, 255],
    ];

    fn asymmetric_source() -> RgbaImage {
        RgbaImage::from_raw(2, 3, COLORS.concat()).unwrap()
    }

    #[test]
    fn cardinal_rotations_copy_every_channel_in_clockwise_order() {
        let source = asymmetric_source();
        for (degrees, dimensions, ids) in [
            (0.0, (2, 3), [0, 1, 2, 3, 4, 5]),
            (90.0, (3, 2), [4, 2, 0, 5, 3, 1]),
            (180.0, (2, 3), [5, 4, 3, 2, 1, 0]),
            (270.0, (3, 2), [1, 3, 5, 0, 2, 4]),
            (-90.0, (3, 2), [1, 3, 5, 0, 2, 4]),
            (-360.0, (2, 3), [0, 1, 2, 3, 4, 5]),
            (360.0, (2, 3), [0, 1, 2, 3, 4, 5]),
        ] {
            let result = rotate_preview(
                &source,
                &ImageRotationSpec { degrees },
                &AtomicBool::new(false),
            )
            .unwrap();
            assert_eq!(result.dimensions(), dimensions);
            let expected: Vec<u8> = ids.into_iter().flat_map(|id| COLORS[id]).collect();
            assert_eq!(result.as_raw(), &expected);
        }
    }

    #[test]
    fn diagonal_rotation_expands_and_keeps_transparent_corners() {
        let source = RgbaImage::from_pixel(2, 3, Rgba([240, 30, 10, 255]));
        let result = rotate_preview(
            &source,
            &ImageRotationSpec { degrees: 45.0 },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(result.dimensions(), (4, 4));
        for (x, y) in [(0, 0), (3, 0), (0, 3), (3, 3)] {
            assert_eq!(result.get_pixel(x, y).0, [0; 4]);
        }
        assert!(result.pixels().any(|pixel| pixel[3] == 255));
        assert!(result.pixels().any(|pixel| (1..255).contains(&pixel[3])));
        for pixel in result.pixels().filter(|pixel| pixel[3] != 0) {
            assert_eq!(&pixel.0[..3], &[240, 30, 10]);
        }
    }

    #[test]
    fn custom_positive_angles_move_the_top_marker_clockwise() {
        let mut source = RgbaImage::new(3, 3);
        source.put_pixel(1, 0, Rgba([255, 0, 0, 255]));
        source.put_pixel(1, 2, Rgba([0, 0, 255, 255]));
        let result = rotate_preview(
            &source,
            &ImageRotationSpec { degrees: 45.0 },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(result.dimensions(), (5, 5));
        assert_eq!(result.get_pixel(3, 1).0, [255, 0, 0, 149]);
        assert_eq!(result.get_pixel(1, 3).0, [0, 0, 255, 149]);
        assert_eq!(result.get_pixel(2, 2).0, [0; 4]);
    }

    #[test]
    fn interpolation_weights_alpha_and_ignores_hidden_rgb() {
        let source = RgbaImage::from_raw(2, 1, vec![255, 0, 0, 128, 0, 255, 0, 0]).unwrap();
        assert_eq!(sample_bilinear(&source, 0.5, 0.0), [255, 0, 0, 64]);
        assert_eq!(sample_bilinear(&source, -0.5, 0.0), [255, 0, 0, 64]);
        assert_eq!(sample_bilinear(&source, 1.0, 0.0), [0; 4]);
        let two_visible = RgbaImage::from_raw(2, 1, vec![255, 0, 0, 128, 0, 0, 255, 64]).unwrap();
        assert_eq!(sample_bilinear(&two_visible, 0.5, 0.0), [170, 0, 85, 96]);
        let rotated = rotate_preview(
            &source,
            &ImageRotationSpec { degrees: 45.0 },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(rotated.pixels().any(|pixel| pixel[3] > 0));
        for pixel in rotated.pixels() {
            if pixel[3] > 0 {
                assert_eq!(&pixel.0[..3], &[255, 0, 0]);
                assert!(pixel[3] <= 128);
            } else {
                assert_eq!(pixel.0, [0; 4]);
            }
        }
    }

    #[test]
    fn previews_reject_oversize_empty_cancelled_and_expired_work() {
        let spec = ImageRotationSpec { degrees: 37.0 };
        let cancel = AtomicBool::new(false);
        assert!(rotate_preview(&RgbaImage::new(769, 1), &spec, &cancel).is_err());
        assert!(rotate_preview(&RgbaImage::new(0, 0), &spec, &cancel).is_err());
        assert!(matches!(
            rotate_preview(&asymmetric_source(), &spec, &AtomicBool::new(true)),
            Err(ProcessingError::Cancelled)
        ));
        assert!(matches!(
            rotate_pixels(
                &asymmetric_source(),
                &spec,
                &cancel,
                Instant::now() - PROCESS_IMAGE_TIMEOUT
            ),
            Err(ProcessingError::Timeout)
        ));
        let maximum = rotate_preview(
            &RgbaImage::new(768, 768),
            &ImageRotationSpec { degrees: 45.0 },
            &cancel,
        )
        .unwrap();
        assert_eq!(maximum.dimensions(), (1087, 1087));
    }

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "flow-image-rotation-{}-{}",
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
    fn rotated_png_roundtrips_at_arbitrary_angle() {
        let directory = TestDirectory::new();
        let source = directory.0.join("source.png");
        let base = asymmetric_source();
        base.save_with_format(&source, ::image::ImageFormat::Png)
            .unwrap();
        let spec = ImageRotationSpec { degrees: 37.0 };
        let cancel = AtomicBool::new(false);
        let rotated = rotate_pixels(&base, &spec, &cancel, Instant::now()).unwrap();
        let output = directory.0.join("rotated.png");
        let result =
            processing_image::write_rgba_png(&source, &rotated, &output, &cancel, Instant::now())
                .unwrap();
        assert_eq!(result.media_type, "image/png");
        assert_eq!((result.width, result.height), rotated.dimensions());
        assert_eq!(::image::open(&output).unwrap().into_rgba8(), rotated);
        let cancelled = directory.0.join("cancelled.png");
        let asset = ProjectAsset {
            key: "assets/source.png".into(),
            name: "source".into(),
            media_type: "image/png".into(),
            byte_len: fs::metadata(&source).unwrap().len(),
            width: 2,
            height: 3,
        };
        assert!(matches!(
            write_rotated(&source, &asset, &spec, &cancelled, &AtomicBool::new(true)),
            Err(ProcessingError::Cancelled)
        ));
        assert!(!cancelled.exists());
    }
}

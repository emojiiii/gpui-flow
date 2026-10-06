use super::{IMAGE_EDIT_PREVIEW_EDGE, ImageEditSpec};
use crate::{
    ProjectAsset,
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

/// Decode an immutable project image on a background worker. This deliberately
/// uses the export decoder (static PNG/JPEG, 32 MiB encoded, 16 MP, 60 seconds),
/// so preview success never promises support for an unsupported source format.
/// The result is an unrotated base preview, at most 768 pixels on either axis.
pub fn load_preview(
    path: &Path,
    asset: &ProjectAsset,
    cancel: &AtomicBool,
) -> Result<RgbaImage, ProcessingError> {
    processing_image::with_decoded_source(path, asset, cancel, |rgba, _, start| {
        let dimensions =
            processing_image::fit_dimensions(rgba.width(), rgba.height(), IMAGE_EDIT_PREVIEW_EDGE);
        let preview = if dimensions == rgba.dimensions() {
            rgba
        } else {
            ::image::imageops::resize(
                &rgba,
                dimensions.0,
                dimensions.1,
                ::image::imageops::FilterType::Triangle,
            )
        };
        check_work(cancel, start)?;
        Ok(preview)
    })
}

/// Orient the original preview without applying its crop. The host draws crop
/// geometry over this full image; repeated UI edits must reuse the base preview.
pub fn orient_preview(
    base: &RgbaImage,
    spec: &ImageEditSpec,
) -> Result<RgbaImage, ProcessingError> {
    if base.width().max(base.height()) > IMAGE_EDIT_PREVIEW_EDGE {
        return Err(ProcessingError::ResourceLimit(
            "image editor previews must be at most 768 pixels per edge".into(),
        ));
    }
    let (width, height) = spec.oriented_dimensions(base.width(), base.height())?;
    copy_oriented_region(
        base,
        spec,
        (0, 0, width, height),
        &AtomicBool::new(false),
        Instant::now(),
    )
}

/// Apply an edit to the full-resolution asset and write a fresh lossless RGBA
/// PNG. The preview is never used as export input. A completed file belongs to
/// the caller; import it into the project before replacing the selected asset.
/// Cancellation, deadlines, failed encodes and file limits remove partial
/// output. Existing files and source aliases are never overwritten.
pub fn write_edited(
    path: &Path,
    asset: &ProjectAsset,
    spec: &ImageEditSpec,
    output: &Path,
    cancel: &AtomicBool,
) -> Result<ProcessingOutput, ProcessingError> {
    let region = spec.crop_pixels(asset.width, asset.height)?;
    processing_image::with_decoded_source(path, asset, cancel, |rgba, source, start| {
        let edited = copy_oriented_region(&rgba, spec, region, cancel, start)?;
        drop(rgba);
        processing_image::write_rgba_png(source, &edited, output, cancel, start)
    })
}

// Fuse orientation and crop into one bounded allocation. Orthogonal edits copy
// all four channels exactly (including RGB underneath transparent pixels).
// There is no full-resolution intermediate rotated or flipped image.
fn copy_oriented_region(
    source: &RgbaImage,
    spec: &ImageEditSpec,
    region: (u32, u32, u32, u32),
    cancel: &AtomicBool,
    start: Instant,
) -> Result<RgbaImage, ProcessingError> {
    check_work(cancel, start)?;
    let (oriented_width, oriented_height) =
        spec.oriented_dimensions(source.width(), source.height())?;
    let (left, top, width, height) = region;
    if width == 0
        || height == 0
        || left
            .checked_add(width)
            .is_none_or(|right| right > oriented_width)
        || top
            .checked_add(height)
            .is_none_or(|bottom| bottom > oriented_height)
    {
        return Err(ProcessingError::InvalidSettings(
            "pixel crop lies outside the oriented image".into(),
        ));
    }
    let length = width as usize * height as usize * 4;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(length).map_err(|_| {
        ProcessingError::ResourceLimit("could not allocate bounded edited pixels".into())
    })?;
    for y in top..top + height {
        check_work(cancel, start)?;
        for x in left..left + width {
            // Undo the oriented-axis reflections, then the rotation.
            let x = if spec.flip_horizontal {
                oriented_width - 1 - x
            } else {
                x
            };
            let y = if spec.flip_vertical {
                oriented_height - 1 - y
            } else {
                y
            };
            let (source_x, source_y) = match spec.quarter_turns {
                0 => (x, y),
                1 => (y, source.height() - 1 - x),
                2 => (source.width() - 1 - x, source.height() - 1 - y),
                3 => (source.width() - 1 - y, x),
                _ => unreachable!("validated quarter turns"),
            };
            pixels.extend_from_slice(&source.get_pixel(source_x, source_y).0);
        }
    }
    check_work(cancel, start)?;
    RgbaImage::from_raw(width, height, pixels).ok_or_else(|| {
        ProcessingError::Process("edited pixel storage has unexpected dimensions".into())
    })
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
    use gpui_flow_core::{Point, Rect, Size};
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

    fn assert_pixels(pixels: &RgbaImage, expected_ids: &[usize]) {
        let expected: Vec<u8> = expected_ids.iter().flat_map(|id| COLORS[id - 1]).collect();
        assert_eq!(pixels.as_raw(), &expected);
    }

    fn cropped(spec: &ImageEditSpec) -> RgbaImage {
        copy_oriented_region(
            &asymmetric_source(),
            spec,
            spec.crop_pixels(2, 3).unwrap(),
            &AtomicBool::new(false),
            Instant::now(),
        )
        .unwrap()
    }

    #[test]
    fn asymmetric_rotations_and_mirrors_preserve_exact_rgba() {
        let source = asymmetric_source();
        for (turns, dimensions, expected) in [
            (0, (2, 3), [1, 2, 3, 4, 5, 6]),
            (1, (3, 2), [5, 3, 1, 6, 4, 2]),
            (2, (2, 3), [6, 5, 4, 3, 2, 1]),
            (3, (3, 2), [2, 4, 6, 1, 3, 5]),
        ] {
            let spec = ImageEditSpec {
                quarter_turns: turns,
                ..ImageEditSpec::default()
            };
            let pixels = orient_preview(&source, &spec).unwrap();
            assert_eq!(pixels.dimensions(), dimensions);
            assert_pixels(&pixels, &expected);
        }
        let mut spec = ImageEditSpec::default();
        spec.flip_horizontal().unwrap();
        assert_pixels(
            &orient_preview(&source, &spec).unwrap(),
            &[2, 1, 4, 3, 6, 5],
        );
        // Rotating an already-flipped view must rotate those displayed pixels.
        spec.rotate_cw().unwrap();
        assert_pixels(
            &orient_preview(&source, &spec).unwrap(),
            &[6, 4, 2, 5, 3, 1],
        );
        spec = ImageEditSpec::default();
        spec.flip_vertical().unwrap();
        assert_pixels(
            &orient_preview(&source, &spec).unwrap(),
            &[5, 6, 3, 4, 1, 2],
        );
    }

    #[test]
    fn crop_tracks_original_content_across_sequential_edits() {
        let mut spec = ImageEditSpec {
            crop: Rect::new(Point::ZERO, Size::new(0.5, 2.0 / 3.0)),
            ..ImageEditSpec::default()
        };
        assert_eq!(cropped(&spec).dimensions(), (1, 2));
        assert_pixels(&cropped(&spec), &[1, 3]);
        spec.rotate_cw().unwrap();
        assert_eq!(cropped(&spec).dimensions(), (2, 1));
        assert_pixels(&cropped(&spec), &[3, 1]);
        // orient_preview intentionally leaves all six source pixels visible.
        assert_eq!(
            orient_preview(&asymmetric_source(), &spec)
                .unwrap()
                .dimensions(),
            (3, 2)
        );
        spec.flip_horizontal().unwrap();
        assert_pixels(&cropped(&spec), &[1, 3]);
        spec.rotate_ccw().unwrap();
        assert_eq!(cropped(&spec).dimensions(), (1, 2));
        assert_pixels(&cropped(&spec), &[3, 1]);
        spec.flip_vertical().unwrap();
        assert_pixels(&cropped(&spec), &[1, 3]);
    }

    #[test]
    fn preview_and_copy_reject_bad_bounds_and_cancelled_work() {
        let base = asymmetric_source();
        let spec = ImageEditSpec::default();
        assert!(orient_preview(&RgbaImage::new(769, 1), &spec).is_err());
        assert!(orient_preview(&RgbaImage::new(0, 0), &spec).is_err());
        for region in [
            (0, 0, 0, 1),
            (1, 1, 2, 1),
            (0, 2, 1, 2),
            (u32::MAX, 0, 2, 1),
        ] {
            assert!(
                copy_oriented_region(
                    &base,
                    &spec,
                    region,
                    &AtomicBool::new(false),
                    Instant::now()
                )
                .is_err()
            );
        }
        assert!(matches!(
            copy_oriented_region(
                &base,
                &spec,
                (0, 0, 2, 3),
                &AtomicBool::new(true),
                Instant::now()
            ),
            Err(ProcessingError::Cancelled)
        ));
        assert!(matches!(
            copy_oriented_region(
                &base,
                &spec,
                (0, 0, 2, 3),
                &AtomicBool::new(false),
                Instant::now() - PROCESS_IMAGE_TIMEOUT
            ),
            Err(ProcessingError::Timeout)
        ));
    }

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let directory = std::env::temp_dir().join(format!(
                "flow-image-edit-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&directory).unwrap();
            Self(directory.canonicalize().unwrap())
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn edited_png_roundtrips_alpha_and_refuses_source_aliases_or_existing_output() {
        let directory = TestDirectory::new();
        let source = directory.0.join("source.png");
        asymmetric_source()
            .save_with_format(&source, ::image::ImageFormat::Png)
            .unwrap();
        let original = fs::read(&source).unwrap();
        let spec = ImageEditSpec {
            quarter_turns: 1,
            crop: Rect::new(Point::new(1.0 / 3.0, 0.0), Size::new(2.0 / 3.0, 0.5)),
            ..ImageEditSpec::default()
        };
        let edited = cropped(&spec);
        let output = directory.0.join("edited.png");
        let cancel = AtomicBool::new(false);
        let result =
            processing_image::write_rgba_png(&source, &edited, &output, &cancel, Instant::now())
                .unwrap();
        assert_eq!(result.media_type, "image/png");
        assert_eq!((result.width, result.height), (2, 1));
        assert_eq!(result.byte_len, fs::metadata(&output).unwrap().len());
        assert_pixels(&::image::open(&output).unwrap().into_rgba8(), &[3, 1]);
        let output_bytes = fs::read(&output).unwrap();
        assert!(
            processing_image::write_rgba_png(&source, &edited, &output, &cancel, Instant::now())
                .is_err()
        );
        assert_eq!(fs::read(&output).unwrap(), output_bytes);
        assert!(
            processing_image::write_rgba_png(&source, &edited, &source, &cancel, Instant::now())
                .is_err()
        );
        let alias = directory.0.join("alias.png");
        fs::hard_link(&source, &alias).unwrap();
        assert!(
            processing_image::write_rgba_png(&source, &edited, &alias, &cancel, Instant::now())
                .is_err()
        );
        #[cfg(unix)]
        {
            let link = directory.0.join("symlink.png");
            std::os::unix::fs::symlink(&source, &link).unwrap();
            assert!(
                processing_image::write_rgba_png(&source, &edited, &link, &cancel, Instant::now())
                    .is_err()
            );
        }
        let cancelled = directory.0.join("cancelled.png");
        assert!(matches!(
            processing_image::write_rgba_png(
                &source,
                &edited,
                &cancelled,
                &AtomicBool::new(true),
                Instant::now()
            ),
            Err(ProcessingError::Cancelled)
        ));
        assert!(!cancelled.exists());
        assert_eq!(fs::read(&source).unwrap(), original);
    }
}

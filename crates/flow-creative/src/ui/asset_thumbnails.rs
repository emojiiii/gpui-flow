//! Small, project-scoped previews. Only explicitly supplied local assets are read.
use crate::image_orientation::{
    ImageOrientation, RgbaOrientationError, orient_rgba, oriented_dimensions,
};
use gpui::{App, Context, RenderImage};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, RgbaImage};
use std::{
    collections::VecDeque,
    fs::File,
    io::{Cursor, Read},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_REQUESTS: usize = 8;
const MAX_CACHE: usize = 32;
const MAX_BYTES: u64 = crate::project::MAX_IMAGE_BYTES;
const MAX_PIXELS: u64 = crate::project::MAX_IMAGE_PIXELS;
const MAX_DIMENSION: u32 = 32768;
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;
const THUMB_WIDTH: u32 = 112;
const THUMB_HEIGHT: u32 = 84;

// Even a closing project's unfinished decoder cannot overlap another decoder.
// Each loader has only one task and a coalesced, bounded list of desired assets.
static DECODE_GATE: Mutex<()> = Mutex::new(());

#[derive(Clone, PartialEq, Eq)]
pub(super) struct ThumbnailRequest {
    pub key: String,
    pub path: PathBuf,
    pub media_type: String,
    pub byte_len: u64,
}

struct CachedThumbnail {
    request: ThumbnailRequest,
    // Cache failures too: repeated renders must not repeatedly read a bad file.
    image: Option<Arc<RenderImage>>,
}

struct InFlight {
    request: ThumbnailRequest,
    cancelled: Arc<AtomicBool>,
}

pub(super) struct AssetThumbnails {
    desired: Vec<ThumbnailRequest>,
    cache: VecDeque<CachedThumbnail>,
    running: Option<InFlight>,
}

impl AssetThumbnails {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        cx.on_release(|this: &mut Self, cx| {
            if let Some(running) = this.running.take() {
                running.cancelled.store(true, Ordering::Release);
            }
            for cached in this.cache.drain(..) {
                release_image(cached.image, cx);
            }
        })
        .detach();
        Self {
            desired: Vec::new(),
            cache: VecDeque::new(),
            running: None,
        }
    }

    /// Replace pending work with the current picker page. The caller resolves
    /// project-owned asset keys to paths; this loader never resolves keys/URLs.
    pub(super) fn request(&mut self, items: Vec<ThumbnailRequest>, cx: &mut Context<Self>) {
        let mut desired: Vec<ThumbnailRequest> = Vec::with_capacity(MAX_REQUESTS);
        for item in items.into_iter().take(MAX_REQUESTS) {
            if !desired.iter().any(|old| old.key == item.key) {
                desired.push(item);
            }
        }
        if self.desired == desired {
            return;
        }
        if let Some(running) = &self.running
            && !desired.contains(&running.request)
        {
            running.cancelled.store(true, Ordering::Release);
        }

        // Invalidate changes to any part of a key's source, and touch current
        // entries so older pages are evicted before the visible picker page.
        for request in &desired {
            if let Some(index) = self
                .cache
                .iter()
                .position(|cached| cached.request.key == request.key)
            {
                let cached = self.cache.remove(index).expect("existing cache entry");
                if cached.request == *request {
                    self.cache.push_back(cached);
                } else {
                    release_image(cached.image, cx);
                }
            }
        }
        self.desired = desired;
        self.start(cx);
    }

    pub(super) fn image(&self, key: &str) -> Option<Arc<RenderImage>> {
        let request = self.desired.iter().find(|request| request.key == key)?;
        self.cache
            .iter()
            .find(|cached| cached.request == *request)
            .and_then(|cached| cached.image.clone())
    }

    pub(super) fn is_loading(&self, key: &str) -> bool {
        self.desired.iter().any(|request| {
            request.key == key && !self.cache.iter().any(|cached| cached.request == *request)
        })
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        let Some(request) = self
            .desired
            .iter()
            .find(|request| !self.cache.iter().any(|cached| cached.request == **request))
            .cloned()
        else {
            return;
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        self.running = Some(InFlight {
            request: request.clone(),
            cancelled: cancelled.clone(),
        });
        let work = request.clone();
        let work_cancelled = cancelled.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { load_thumbnail(&work, &work_cancelled) })
                .await;
            // `this` is weak: releasing a project never keeps its loader alive.
            let _ = this.update(cx, |this, cx| {
                this.running = None;
                if !cancelled.load(Ordering::Acquire) && this.desired.contains(&request) {
                    // Construct a GPUI image only once the result is still wanted.
                    let image = result
                        .ok()
                        .map(|pixels| Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])));
                    this.cache.push_back(CachedThumbnail { request, image });
                    while this.cache.len() > MAX_CACHE {
                        if let Some(old) = this.cache.pop_front() {
                            release_image(old.image, cx);
                        }
                    }
                    cx.notify();
                }
                this.start(cx);
            });
        })
        .detach();
    }
}

fn release_image(image: Option<Arc<RenderImage>>, cx: &mut App) {
    if let Some(image) = image {
        // A request can arrive during window rendering, when that window is
        // temporarily outside App.windows. Defer until it is returned there.
        cx.defer(move |cx| cx.drop_image(image, None));
    }
}

fn format_for_mime(media_type: &str) -> Result<ImageFormat, &'static str> {
    match media_type {
        "image/png" => Ok(ImageFormat::Png),
        "image/jpeg" => Ok(ImageFormat::Jpeg),
        "image/gif" => Ok(ImageFormat::Gif),
        "image/webp" => Ok(ImageFormat::WebP),
        _ => Err("unsupported thumbnail format"),
    }
}

fn check_active(cancelled: &AtomicBool) -> Result<(), &'static str> {
    if cancelled.load(Ordering::Acquire) {
        Err("thumbnail request cancelled")
    } else {
        Ok(())
    }
}

fn load_thumbnail(
    request: &ThumbnailRequest,
    cancelled: &AtomicBool,
) -> Result<RgbaImage, &'static str> {
    check_active(cancelled)?;
    let format = format_for_mime(&request.media_type)?;
    if request.byte_len == 0 || request.byte_len > MAX_BYTES {
        return Err("thumbnail file exceeds byte limit");
    }
    let _gate = DECODE_GATE
        .lock()
        .map_err(|_| "thumbnail decoder stopped")?;
    check_active(cancelled)?;
    let metadata =
        std::fs::symlink_metadata(&request.path).map_err(|_| "thumbnail file is unavailable")?;
    if !metadata.is_file() || metadata.len() != request.byte_len {
        return Err("thumbnail file is not the expected regular file");
    }
    let file = File::open(&request.path).map_err(|_| "thumbnail file cannot be opened")?;
    let opened = file
        .metadata()
        .map_err(|_| "thumbnail metadata unavailable")?;
    if !opened.is_file() || opened.len() != request.byte_len {
        return Err("thumbnail file changed");
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(request.byte_len as usize + 1)
        .map_err(|_| "thumbnail input allocation failed")?;
    file.take(request.byte_len + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "thumbnail file cannot be read")?;
    if bytes.len() as u64 != request.byte_len {
        return Err("thumbnail file changed while reading");
    }
    check_active(cancelled)?;
    decode_thumbnail(bytes, format, cancelled)
}

fn check_dimensions(width: u32, height: u32, output_bytes: u64) -> Result<(), &'static str> {
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_PIXELS
        || output_bytes > MAX_DECODE_BYTES
    {
        Err("thumbnail image exceeds decoding limits")
    } else {
        Ok(())
    }
}

fn decode_thumbnail(
    bytes: Vec<u8>,
    format: ImageFormat,
    cancelled: &AtomicBool,
) -> Result<RgbaImage, &'static str> {
    check_active(cancelled)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    let mut reader = ImageReader::with_format(Cursor::new(bytes.as_slice()), format);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| "invalid thumbnail image")?;
    let (width, height) = decoder.dimensions();
    // Image's max_alloc is best-effort for some formats. Independently bound
    // the actual decoded pixel buffer before DynamicImage allocates it.
    check_dimensions(width, height, decoder.total_bytes())?;
    check_active(cancelled)?;
    let geometry = ImageOrientation::from_decoder(&mut decoder)
        .map_err(|_| "invalid thumbnail orientation metadata")?;
    check_active(cancelled)?;
    // Decode one still image only, including for animated GIF/WebP assets.
    let decoded = DynamicImage::from_decoder(decoder).map_err(|_| "thumbnail decoding failed")?;
    drop(bytes);
    check_active(cancelled)?;
    // Resize in encoded coordinates, exchanging the target axes for a quarter
    // turn so the final logical image still fits the thumbnail rectangle.
    // Convert/orient only the small pixels; never rotate a full decoded image.
    let (target_width, target_height) =
        oriented_dimensions((THUMB_WIDTH, THUMB_HEIGHT), geometry.orientation);
    let thumbnail = decoded.thumbnail(target_width, target_height).into_rgba8();
    drop(decoded);
    let mut thumbnail = orient_rgba(
        thumbnail,
        geometry.orientation,
        || check_active(cancelled),
        |peak| {
            // Encoded bytes, decoder and full pixels have been dropped. These
            // actual RGBA capacities are the only remaining image allocations.
            if peak > MAX_DECODE_BYTES {
                Err("thumbnail orientation exceeds allocation limit")
            } else {
                Ok(())
            }
        },
    )
    .map_err(|error| match error {
        RgbaOrientationError::Caller(error) => error,
        RgbaOrientationError::InvalidBuffer => "invalid thumbnail orientation pixels",
        RgbaOrientationError::SizeOverflow | RgbaOrientationError::AllocationFailed => {
            "thumbnail orientation allocation failed"
        }
        RgbaOrientationError::ColorSpace(_) => "thumbnail orientation color space failed",
    })?;
    check_active(cancelled)?;
    // RenderImage uses BGRA pixels, matching the model/media native bridges.
    for (index, pixel) in thumbnail.pixels_mut().enumerate() {
        if index % 4096 == 0 {
            check_active(cancelled)?;
        }
        pixel.0.swap(0, 2);
    }
    check_active(cancelled)?;
    Ok(thumbnail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;

    fn exif_with_orientation(orientation: u8) -> Vec<u8> {
        let mut exif = b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x01\0\0\0\0\0\0\0".to_vec();
        exif[18] = orientation;
        exif
    }

    fn asymmetric_png(dimensions: (u32, u32), exif: Vec<u8>) -> Vec<u8> {
        let source = RgbaImage::from_fn(dimensions.0, dimensions.1, |x, y| {
            let id = (1 + (y * 2 / dimensions.1) * 3 + x * 3 / dimensions.0) as u8;
            image::Rgba([id * 20, id * 25, 255 - id * 20, id * 31])
        });
        let mut encoded = Vec::new();
        let mut encoder = image::codecs::png::PngEncoder::new(&mut encoded);
        encoder.set_exif_metadata(exif).unwrap();
        encoder
            .write_image(
                source.as_raw(),
                dimensions.0,
                dimensions.1,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        encoded
    }

    #[test]
    fn exif_rotation_and_reflection_preserve_small_bgra_pixels_and_bounds() {
        for (orientation, source_dimensions, expected_dimensions, columns, rows, expected_ids) in [
            (6, (336, 224), (56, 84), 2, 3, [4, 1, 5, 2, 6, 3]),
            (6, (224, 336), (112, 75), 2, 3, [4, 1, 5, 2, 6, 3]),
            (2, (336, 224), (112, 75), 3, 2, [3, 2, 1, 6, 5, 4]),
        ] {
            let encoded = asymmetric_png(source_dimensions, exif_with_orientation(orientation));
            let thumbnail =
                decode_thumbnail(encoded, ImageFormat::Png, &AtomicBool::new(false)).unwrap();
            assert_eq!(thumbnail.dimensions(), expected_dimensions);
            assert!(thumbnail.width() <= THUMB_WIDTH && thumbnail.height() <= THUMB_HEIGHT);
            for (index, id) in expected_ids.into_iter().enumerate() {
                let column = index as u32 % columns;
                let row = index as u32 / columns;
                let x = (2 * column + 1) * thumbnail.width() / (2 * columns);
                let y = (2 * row + 1) * thumbnail.height() / (2 * rows);
                assert_eq!(
                    thumbnail.get_pixel(x, y).0,
                    [255 - id * 20, id * 25, id * 20, id * 31]
                );
            }
        }
    }

    #[test]
    fn exif_thumbnail_metadata_and_raw_geometry_fail_closed() {
        let cancel = AtomicBool::new(false);
        for exif in [
            exif_with_orientation(0),
            exif_with_orientation(9),
            b"broken TIFF".to_vec(),
        ] {
            let encoded = asymmetric_png((3, 2), exif);
            assert_eq!(
                decode_thumbnail(encoded, ImageFormat::Png, &cancel).unwrap_err(),
                "invalid thumbnail orientation metadata"
            );
        }
        let encoded = asymmetric_png((MAX_DIMENSION + 1, 1), exif_with_orientation(6));
        assert!(decode_thumbnail(encoded, ImageFormat::Png, &cancel).is_err());
        let encoded = asymmetric_png((336, 224), exif_with_orientation(6));
        assert_eq!(
            decode_thumbnail(encoded, ImageFormat::Png, &AtomicBool::new(true)).unwrap_err(),
            "thumbnail request cancelled"
        );
    }

    struct TestSource(PathBuf);

    impl TestSource {
        fn new(bytes: &[u8]) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "flow-thumbnail-exif-{}-{}.png",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap();
            std::io::Write::write_all(&mut file, bytes).unwrap();
            Self(path)
        }
    }

    impl Drop for TestSource {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn loading_oriented_thumbnail_keeps_original_and_byte_checks() {
        let original = asymmetric_png((336, 224), exif_with_orientation(6));
        let source = TestSource::new(&original);
        let mut request = ThumbnailRequest {
            key: "assets/oriented.png".into(),
            path: source.0.clone(),
            media_type: "image/png".into(),
            byte_len: original.len() as u64,
        };
        assert_eq!(
            load_thumbnail(&request, &AtomicBool::new(false))
                .unwrap()
                .dimensions(),
            (56, 84)
        );
        assert_eq!(std::fs::read(&source.0).unwrap(), original);
        request.byte_len += 1;
        assert!(load_thumbnail(&request, &AtomicBool::new(false)).is_err());
        request.byte_len = MAX_BYTES + 1;
        assert_eq!(
            load_thumbnail(&request, &AtomicBool::new(false)).unwrap_err(),
            "thumbnail file exceeds byte limit"
        );
        assert_eq!(std::fs::read(&source.0).unwrap(), original);
    }

    #[test]
    fn dimensions_and_output_are_bounded_before_pixel_allocation() {
        assert!(check_dimensions(8192, 8192, MAX_DECODE_BYTES).is_ok());
        assert!(check_dimensions(32768, 1, 131072).is_ok());
        assert!(check_dimensions(0, 1, 0).is_err());
        assert!(check_dimensions(32769, 1, 131076).is_err());
        assert!(check_dimensions(16384, 8192, MAX_DECODE_BYTES).is_err());
        assert!(check_dimensions(8192, 8192, MAX_DECODE_BYTES + 1).is_err());
    }

    #[test]
    fn thumbnails_preserve_aspect_and_use_bgra() {
        let source = RgbaImage::from_pixel(224, 112, image::Rgba([231, 17, 42, 255]));
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(source)
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();
        let thumbnail = decode_thumbnail(
            encoded.into_inner(),
            ImageFormat::Png,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(thumbnail.dimensions(), (112, 56));
        assert_eq!(thumbnail.get_pixel(0, 0).0, [42, 17, 231, 255]);
    }

    #[test]
    fn unsupported_corrupt_and_cancelled_previews_fail_closed() {
        assert!(format_for_mime("image/svg+xml").is_err());
        assert!(format_for_mime("https://example.com/image.png").is_err());
        assert!(
            decode_thumbnail(
                b"bad image".to_vec(),
                ImageFormat::Png,
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert!(decode_thumbnail(Vec::new(), ImageFormat::Png, &AtomicBool::new(true)).is_err());
    }
}

//! Small, project-scoped previews. Only explicitly supplied local assets are read.
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
    decode_thumbnail(&bytes, format, cancelled)
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
    bytes: &[u8],
    format: ImageFormat,
    cancelled: &AtomicBool,
) -> Result<RgbaImage, &'static str> {
    check_active(cancelled)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|_| "invalid thumbnail image")?;
    let (width, height) = decoder.dimensions();
    // Image's max_alloc is best-effort for some formats. Independently bound
    // the actual decoded pixel buffer before DynamicImage allocates it.
    check_dimensions(width, height, decoder.total_bytes())?;
    check_active(cancelled)?;
    // Decode one still image only, including for animated GIF/WebP assets.
    let decoded = DynamicImage::from_decoder(decoder).map_err(|_| "thumbnail decoding failed")?;
    check_active(cancelled)?;
    // Convert channels only after downsampling; never allocate a full RGBA copy.
    let mut thumbnail = decoded.thumbnail(THUMB_WIDTH, THUMB_HEIGHT).into_rgba8();
    check_active(cancelled)?;
    // RenderImage uses BGRA pixels, matching the model/media native bridges.
    for pixel in thumbnail.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok(thumbnail)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let thumbnail =
            decode_thumbnail(encoded.get_ref(), ImageFormat::Png, &AtomicBool::new(false)).unwrap();
        assert_eq!(thumbnail.dimensions(), (112, 56));
        assert_eq!(thumbnail.get_pixel(0, 0).0, [42, 17, 231, 255]);
    }

    #[test]
    fn unsupported_corrupt_and_cancelled_previews_fail_closed() {
        assert!(format_for_mime("image/svg+xml").is_err());
        assert!(format_for_mime("https://example.com/image.png").is_err());
        assert!(decode_thumbnail(b"bad image", ImageFormat::Png, &AtomicBool::new(false)).is_err());
        assert!(decode_thumbnail(b"", ImageFormat::Png, &AtomicBool::new(true)).is_err());
    }
}

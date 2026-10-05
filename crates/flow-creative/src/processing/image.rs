use super::*;
use ::image::{DynamicImage, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Limits};
use std::io::{BufRead, Cursor, Seek, SeekFrom, Write};

const MAX_DIMENSION: u32 = 16_384;
const MAX_DECODE_BYTES: u64 = 128 * 1024 * 1024;

pub(super) fn execute(
    settings: &ProcessingSettings,
    source: &Path,
    asset: &ProjectAsset,
    destination: &Path,
    cancel: &AtomicBool,
) -> Result<ProcessingOutput, ProcessingError> {
    let ProcessingSettings::Image {
        format,
        quality,
        max_edge,
    } = *settings
    else {
        return Err(ProcessingError::InvalidSettings(
            "expected image settings".into(),
        ));
    };
    let start = Instant::now();
    let bytes = read_bounded(source, MAX_PROCESS_IMAGE_BYTES, cancel)?;
    if bytes.len() as u64 != asset.byte_len {
        return Err(ProcessingError::InvalidSource(
            "image changed while reading it".into(),
        ));
    }
    let source_format = static_format(&bytes, &asset.media_type)?;
    let mut reader = ImageReader::with_format(
        DeadlineReader {
            cursor: Cursor::new(bytes.as_slice()),
            cancel,
            start,
        },
        source_format,
    );
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|error| codec_error(error, cancel, start))?;
    let (width, height) = decoder.dimensions();
    if width == 0
        || height == 0
        || u64::from(width) * u64::from(height) > MAX_PROCESS_IMAGE_PIXELS
        || decoder.total_bytes() > MAX_DECODE_BYTES
    {
        return Err(ProcessingError::ResourceLimit(
            "image processing accepts at most 16 MP and 128 MiB of decoded pixels".into(),
        ));
    }
    if (width, height) != (asset.width, asset.height) {
        return Err(ProcessingError::InvalidSource(
            "decoded dimensions do not match the selected image asset".into(),
        ));
    }
    check_deadline(cancel, start, PROCESS_IMAGE_TIMEOUT)?;
    let decoded =
        DynamicImage::from_decoder(decoder).map_err(|error| codec_error(error, cancel, start))?;
    drop(bytes);
    check_deadline(cancel, start, PROCESS_IMAGE_TIMEOUT)?;
    // Explicit bound: <=128 MiB decoded + <=64 MiB RGBA conversion. After the
    // conversion, resize uses <=64 MiB each for source/output and <=256 MiB
    // for the image crate's intermediate RGBA32F image (all axes shrink).
    let rgba = decoded.into_rgba8();
    let dimensions = fit_dimensions(width, height, max_edge);
    let rgba = if dimensions != (width, height) {
        ::image::imageops::resize(
            &rgba,
            dimensions.0,
            dimensions.1,
            ::image::imageops::FilterType::Triangle,
        )
    } else {
        rgba
    };
    check_deadline(cancel, start, PROCESS_IMAGE_TIMEOUT)?;
    let mut output = OutputGuard::create(destination)?;
    let mut writer = BudgetWriter {
        file: output.file.as_mut().expect("new output"),
        written: 0,
        cancel,
        start,
    };
    match format {
        ImageOutputFormat::Png => {
            ::image::codecs::png::PngEncoder::new(&mut writer)
                .write_image(
                    rgba.as_raw(),
                    dimensions.0,
                    dimensions.1,
                    ::image::ExtendedColorType::Rgba8,
                )
                .map_err(|error| codec_error(error, cancel, start))?;
        }
        ImageOutputFormat::Jpeg => {
            // JPEG has no alpha. Composite every pixel over opaque white,
            // rather than silently discarding alpha and exposing hidden RGB.
            let mut rgb = Vec::new();
            let length = dimensions.0 as usize * dimensions.1 as usize * 3;
            rgb.try_reserve_exact(length).map_err(|_| {
                ProcessingError::ResourceLimit("could not allocate bounded JPEG pixels".into())
            })?;
            for (index, pixel) in rgba.pixels().enumerate() {
                if index % 4096 == 0 {
                    check_deadline(cancel, start, PROCESS_IMAGE_TIMEOUT)?;
                }
                let alpha = u32::from(pixel[3]);
                for channel in &pixel.0[..3] {
                    rgb.push(
                        ((u32::from(*channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8,
                    );
                }
            }
            drop(rgba);
            ::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, quality)
                .encode(
                    &rgb,
                    dimensions.0,
                    dimensions.1,
                    ::image::ExtendedColorType::Rgb8,
                )
                .map_err(|error| codec_error(error, cancel, start))?;
        }
    }
    writer.flush()?;
    check_deadline(cancel, start, PROCESS_IMAGE_TIMEOUT)?;
    output.finish(settings, dimensions, MAX_PROCESS_IMAGE_BYTES, cancel)
}

fn codec_error(error: ::image::ImageError, cancel: &AtomicBool, start: Instant) -> ProcessingError {
    match check_deadline(cancel, start, PROCESS_IMAGE_TIMEOUT) {
        Err(error) => error,
        Ok(()) => ProcessingError::Process(format!("image processing failed: {error}")),
    }
}

fn fit_dimensions(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= max_edge {
        return (width, height);
    }
    let scale = |axis: u32| {
        ((u64::from(axis) * u64::from(max_edge) + u64::from(longest) / 2) / u64::from(longest))
            .max(1) as u32
    };
    (scale(width), scale(height))
}

fn static_format(bytes: &[u8], media_type: &str) -> Result<ImageFormat, ProcessingError> {
    let format = ::image::guess_format(bytes)
        .map_err(|error| ProcessingError::InvalidSource(error.to_string()))?;
    let expected = match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        _ => return Err(ProcessingError::Unsupported("image processing currently accepts static PNG and JPEG only; GIF and WebP are not flattened".into())),
    };
    if media_type != expected {
        return Err(ProcessingError::InvalidSource(
            "image header and project media type do not match".into(),
        ));
    }
    if format == ImageFormat::Png {
        let mut position = 8usize;
        let mut chunks = 0;
        let mut ended = false;
        while position < bytes.len() {
            chunks += 1;
            if chunks > 65536 || bytes.len() - position < 12 {
                return Err(ProcessingError::InvalidSource(
                    "invalid or oversized PNG chunk table".into(),
                ));
            }
            let length = u32::from_be_bytes(
                bytes[position..position + 4]
                    .try_into()
                    .expect("checked PNG header"),
            ) as usize;
            let next = position
                .checked_add(12)
                .and_then(|start| start.checked_add(length))
                .filter(|next| *next <= bytes.len())
                .ok_or_else(|| ProcessingError::InvalidSource("truncated PNG chunk".into()))?;
            let kind = &bytes[position + 4..position + 8];
            if matches!(kind, b"acTL" | b"fcTL" | b"fdAT") {
                return Err(ProcessingError::Unsupported(
                    "APNG animation is not flattened; choose a static PNG or JPEG".into(),
                ));
            }
            if kind == b"IEND" {
                if length != 0 || next != bytes.len() {
                    return Err(ProcessingError::InvalidSource("invalid PNG ending".into()));
                }
                ended = true;
                break;
            }
            position = next;
        }
        if !ended {
            return Err(ProcessingError::InvalidSource(
                "PNG has no complete ending".into(),
            ));
        }
    }
    Ok(format)
}

struct DeadlineReader<'a> {
    cursor: Cursor<&'a [u8]>,
    cancel: &'a AtomicBool,
    start: Instant,
}

impl DeadlineReader<'_> {
    fn check(&self) -> io::Result<()> {
        check_deadline(self.cancel, self.start, PROCESS_IMAGE_TIMEOUT).map_err(io::Error::other)
    }
}
impl Read for DeadlineReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        self.cursor.read(buffer)
    }
}
impl BufRead for DeadlineReader<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.check()?;
        self.cursor.fill_buf()
    }
    fn consume(&mut self, amount: usize) {
        self.cursor.consume(amount);
    }
}
impl Seek for DeadlineReader<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.check()?;
        self.cursor.seek(position)
    }
}

struct BudgetWriter<'a> {
    file: &'a mut File,
    written: u64,
    cancel: &'a AtomicBool,
    start: Instant,
}
impl Write for BudgetWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        check_deadline(self.cancel, self.start, PROCESS_IMAGE_TIMEOUT).map_err(io::Error::other)?;
        if bytes.len() as u64 > MAX_PROCESS_IMAGE_BYTES.saturating_sub(self.written) {
            return Err(io::Error::other("encoded image exceeds 32 MiB"));
        }
        let count = self.file.write(bytes)?;
        self.written += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::tests::TestDirectory;

    fn png_source(directory: &TestDirectory) -> (PathBuf, ProjectAsset) {
        let path = directory.0.join("source.png");
        let pixels = ::image::RgbaImage::from_pixel(8, 4, ::image::Rgba([255, 0, 0, 0]));
        pixels.save_with_format(&path, ImageFormat::Png).unwrap();
        let asset = ProjectAsset {
            key: "assets/source.png".into(),
            name: "source".into(),
            media_type: "image/png".into(),
            byte_len: fs::metadata(&path).unwrap().len(),
            width: 8,
            height: 4,
        };
        (path, asset)
    }

    #[test]
    fn actual_png_to_jpeg_composites_white_resizes_and_never_upscales() {
        // Serialize against other processor tests; execute() is the inner
        // image routine so this does not recursively acquire PROCESS_GATE.
        let _gate = PROCESS_GATE.lock().unwrap();
        let directory = TestDirectory::new();
        let (source, asset) = png_source(&directory);
        let original = fs::read(&source).unwrap();
        let cancel = AtomicBool::new(false);
        let settings = ProcessingSettings::Image {
            format: ImageOutputFormat::Jpeg,
            quality: 100,
            max_edge: 4,
        };
        let output = execute(
            &settings,
            &source,
            &asset,
            &directory.0.join("output.jpg"),
            &cancel,
        )
        .unwrap();
        assert_eq!((output.width, output.height), (4, 2));
        assert_eq!(output.media_type, "image/jpeg");
        let decoded = ::image::open(&output.path).unwrap().into_rgb8();
        assert!(
            decoded
                .pixels()
                .all(|pixel| pixel.0.iter().all(|channel| *channel >= 250))
        );
        let settings = ProcessingSettings::Image {
            format: ImageOutputFormat::Png,
            quality: 85,
            max_edge: 8192,
        };
        let output = execute(
            &settings,
            &source,
            &asset,
            &directory.0.join("output.png"),
            &cancel,
        )
        .unwrap();
        assert_eq!((output.width, output.height), (8, 4));
        assert!(
            ::image::open(output.path)
                .unwrap()
                .into_rgba8()
                .pixels()
                .all(|pixel| pixel[3] == 0)
        );
        assert_eq!(fs::read(source).unwrap(), original);
    }

    #[test]
    fn image_cancel_and_existing_output_leave_sources_intact() {
        let _gate = PROCESS_GATE.lock().unwrap();
        let directory = TestDirectory::new();
        let (source, asset) = png_source(&directory);
        let output = directory.0.join("output.jpg");
        let settings = ProcessingKind::Image.default_settings();
        assert!(matches!(
            execute(&settings, &source, &asset, &output, &AtomicBool::new(true)),
            Err(ProcessingError::Cancelled)
        ));
        assert!(!output.exists());
        fs::write(&output, b"existing").unwrap();
        assert!(execute(&settings, &source, &asset, &output, &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(output).unwrap(), b"existing");
    }

    #[test]
    fn jpeg_half_alpha_is_blended_over_white() {
        let _gate = PROCESS_GATE.lock().unwrap();
        let directory = TestDirectory::new();
        let (source, mut asset) = png_source(&directory);
        ::image::RgbaImage::from_pixel(8, 4, ::image::Rgba([255, 0, 0, 128]))
            .save_with_format(&source, ImageFormat::Png)
            .unwrap();
        asset.byte_len = fs::metadata(&source).unwrap().len();
        let settings = ProcessingSettings::Image {
            format: ImageOutputFormat::Jpeg,
            quality: 100,
            max_edge: 8,
        };
        let output = execute(
            &settings,
            &source,
            &asset,
            &directory.0.join("half.jpg"),
            &AtomicBool::new(false),
        )
        .unwrap();
        let pixels = ::image::open(output.path).unwrap().into_rgb8();
        for pixel in pixels.pixels() {
            assert!(pixel[0] >= 250);
            assert!((122..=132).contains(&pixel[1]));
            assert!((122..=132).contains(&pixel[2]));
        }
    }

    #[test]
    fn animation_and_wrong_type_are_not_silently_flattened() {
        let directory = TestDirectory::new();
        let (source, _) = png_source(&directory);
        let bytes = fs::read(source).unwrap();
        assert!(static_format(&bytes, "image/jpeg").is_err());
        let mut animated = bytes[..33].to_vec();
        animated.extend_from_slice(&[0, 0, 0, 8]);
        animated.extend_from_slice(b"acTL");
        animated.extend_from_slice(&[0; 12]);
        animated.extend_from_slice(&bytes[33..]);
        assert!(matches!(
            static_format(&animated, "image/png"),
            Err(ProcessingError::Unsupported(_))
        ));
        assert!(static_format(b"GIF89a", "image/gif").is_err());
    }
}

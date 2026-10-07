//! Clipboard bytes are decoded and bounded before creating a project-owned file.
//! The host must call this on a background worker, never the UI thread.
#![cfg(feature = "desktop")]

use super::*;
use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use std::{
    io::{BufRead, Cursor, Seek, SeekFrom},
    sync::atomic::AtomicBool,
};

const MAX_CLIPBOARD_PIXELS: u64 = 4_194_304;
const MAX_CLIPBOARD_DIMENSION: u32 = 4096;
const MAX_DECODE_BYTES: u64 = 128 * 1024 * 1024;
const CODEC_FIXED_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CONTAINER_PARTS: usize = 65_536;

impl LocalProject {
    /// Validate and store an actual static PNG/JPEG clipboard payload, preserving
    /// its encoded bytes. Neither strings nor clipboard filenames are opened.
    ///
    /// Decode on a background worker. Cancellation is honored until the manifest
    /// commit starts; a successful commit always returns its new asset. The host
    /// binds the returned asset to its canvas in a separate editor transaction.
    pub fn import_clipboard_image(
        &mut self,
        bytes: &[u8],
        expected_media_type: &str,
        cancelled: &AtomicBool,
    ) -> Result<ProjectAsset, ProjectError> {
        check_active(cancelled)?;
        check_project_capacity(&self.project, bytes.len() as u64)?;
        let info = validate_clipboard_image(bytes, expected_media_type, cancelled)?;

        // Decoding does not hold the project lock. Recheck the persisted revision
        // under the normal storage transaction before creating any file.
        let _transaction = transaction()?;
        check_active(cancelled)?;
        self.check_revision()?;
        check_project_capacity(&self.project, bytes.len() as u64)?;
        let asset = ProjectAsset {
            key: format!("assets/{}.{}", unique_name("clipboard"), info.extension),
            name: format!(
                "Clipboard image {}×{}.{}",
                info.width, info.height, info.extension
            ),
            media_type: info.media_type.into(),
            byte_len: bytes.len() as u64,
            width: info.width,
            height: info.height,
        };
        let mut candidate = self.next_revision()?;
        candidate.assets.push(asset.clone());
        validate_project(&candidate, &candidate.metadata.id)?;
        checked_directory(&self.directory.join("assets"), false)?;
        check_active(cancelled)?;
        let path = self.directory.join(&asset.key);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let result = (|| {
            for chunk in bytes.chunks(16 * 1024) {
                check_active(cancelled)?;
                file.write_all(chunk)?;
            }
            file.sync_all()?;
            drop(file);
            #[cfg(unix)]
            File::open(self.directory.join("assets"))?.sync_all()?;
            check_active(cancelled)?;
            write_project(&self.directory, &candidate)?;
            self.project = candidate;
            Ok(asset)
        })();
        if result.is_err() {
            // Only this call's successfully created, uncommitted file is ours
            // to remove. A create_new collision never reaches this branch.
            let _ = fs::remove_file(path);
        }
        result
    }
}

fn check_active(cancelled: &AtomicBool) -> Result<(), ProjectError> {
    if cancelled.load(Ordering::Acquire) {
        Err(ProjectError::Invalid(
            "clipboard image import cancelled".into(),
        ))
    } else {
        Ok(())
    }
}

fn invalid(message: &str) -> ProjectError {
    ProjectError::Invalid(message.into())
}

fn check_project_capacity(project: &ProjectFile, bytes: u64) -> Result<(), ProjectError> {
    if bytes == 0 {
        return Err(invalid("clipboard image is empty"));
    }
    if bytes > MAX_IMAGE_BYTES {
        return Err(ProjectError::TooLarge(
            "clipboard image exceeds 32 MiB".into(),
        ));
    }
    if project.assets.len() >= MAX_PROJECT_ASSETS {
        return Err(ProjectError::TooLarge(
            "project has reached its 512 asset limit".into(),
        ));
    }
    let total = project
        .assets
        .iter()
        .fold(bytes, |total, asset| total.saturating_add(asset.byte_len));
    if total > MAX_PROJECT_ASSET_BYTES {
        return Err(ProjectError::TooLarge(
            "project assets exceed 512 MiB".into(),
        ));
    }
    Ok(())
}

fn validate_clipboard_image(
    bytes: &[u8],
    expected_media_type: &str,
    cancelled: &AtomicBool,
) -> Result<AssetInfo, ProjectError> {
    check_active(cancelled)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(ProjectError::TooLarge(
            "clipboard image must contain 1 byte to 32 MiB".into(),
        ));
    }
    let format = match expected_media_type {
        "image/png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => ImageFormat::Png,
        "image/jpeg" if bytes.starts_with(&[0xff, 0xd8]) => ImageFormat::Jpeg,
        _ => {
            return Err(invalid(
                "clipboard image must contain matching static PNG/JPEG bytes",
            ));
        }
    };
    let info = image_info(bytes)?;
    let pixels = u64::from(info.width) * u64::from(info.height);
    if info.width > MAX_CLIPBOARD_DIMENSION
        || info.height > MAX_CLIPBOARD_DIMENSION
        || pixels > MAX_CLIPBOARD_PIXELS
    {
        return Err(ProjectError::TooLarge(
            "clipboard image exceeds 4096 pixels per side or 4,194,304 total pixels".into(),
        ));
    }
    match format {
        ImageFormat::Png => validate_png_container(bytes, cancelled)?,
        ImageFormat::Jpeg => validate_jpeg_container(bytes, cancelled)?,
        _ => unreachable!(),
    }
    let (scratch, encoded_peak) = admit_decode(&info, format, bytes.len() as u64)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_CLIPBOARD_DIMENSION);
    limits.max_image_height = Some(MAX_CLIPBOARD_DIMENSION);
    limits.max_alloc = Some(scratch);
    let mut reader = ImageReader::with_format(
        CancelReader {
            cursor: Cursor::new(bytes),
            cancelled,
        },
        format,
    );
    // ImageReader passes PNG's limits into its constructor, before metadata or
    // scanline allocation. Setting limits only after construction is too late.
    reader.limits(limits);
    check_active(cancelled)?;
    let mut decoder = reader
        .into_decoder()
        .map_err(|error| invalid(&format!("invalid clipboard image header: {error}")))?;
    let geometry = crate::image_orientation::ImageOrientation::from_decoder(&mut decoder)
        .map_err(|error| invalid(&format!("invalid image orientation: {error}")))?;
    check_active(cancelled)?;
    let output_bytes = decoder.total_bytes();
    if geometry.oriented_dimensions != (info.width, info.height) || output_bytes > pixels * 8 {
        return Err(invalid(
            "clipboard image decoded geometry disagrees with its header",
        ));
    }
    check_memory(
        encoded_peak
            .saturating_add(scratch)
            .saturating_add(output_bytes),
    )?;
    check_active(cancelled)?;
    let output_len = usize::try_from(output_bytes)
        .map_err(|_| invalid("clipboard image allocation overflow"))?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(output_len)
        .map_err(|_| ProjectError::TooLarge("clipboard image allocation failed".into()))?;
    check_memory(
        encoded_peak
            .saturating_add(scratch)
            .saturating_add(output.capacity() as u64),
    )?;
    output.resize(output_len, 0);
    decoder
        .read_image(&mut output)
        .map_err(|error| invalid(&format!("clipboard image decoding failed: {error}")))?;
    check_active(cancelled)?;
    Ok(info)
}

fn admit_decode(
    info: &AssetInfo,
    format: ImageFormat,
    encoded: u64,
) -> Result<(u64, u64), ProjectError> {
    let pixels = u64::from(info.width) * u64::from(info.height);
    let (scratch, encoded_peak) = if format == ImageFormat::Jpeg {
        // image 0.25's JPEG decoder does not enforce max_alloc internally.
        // Admit padded coefficient/upsampling planes and compressed input /
        // metadata copies before constructing it, as in frame PNG export.
        let padded =
            u64::from(info.width.div_ceil(32) * 32) * u64::from(info.height.div_ceil(32) * 32);
        (padded * 16 + CODEC_FIXED_BYTES, encoded.saturating_mul(4))
    } else {
        (pixels * 8 + CODEC_FIXED_BYTES, encoded)
    };
    check_memory(
        encoded_peak
            .saturating_add(scratch)
            .saturating_add(pixels * 8),
    )?;
    Ok((scratch, encoded_peak))
}

fn check_memory(bytes: u64) -> Result<(), ProjectError> {
    if bytes > MAX_DECODE_BYTES {
        Err(ProjectError::TooLarge(
            "clipboard image and decoder scratch exceed 128 MiB".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_png_container(bytes: &[u8], cancelled: &AtomicBool) -> Result<(), ProjectError> {
    let mut offset = 8usize;
    let mut saw_data = false;
    let mut data_ended = false;
    for _ in 0..MAX_CONTAINER_PARTS {
        check_active(cancelled)?;
        let header = bytes
            .get(offset..offset + 8)
            .ok_or_else(|| invalid("truncated clipboard PNG chunk"))?;
        let length = u32::from_be_bytes(header[..4].try_into().expect("PNG chunk size")) as usize;
        let end = offset
            .checked_add(12)
            .and_then(|start| start.checked_add(length))
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| invalid("truncated clipboard PNG chunk"))?;
        let kind = &header[4..];
        if matches!(kind, b"acTL" | b"fcTL" | b"fdAT") {
            return Err(invalid("animated PNG clipboard images are not supported"));
        }
        if !kind.iter().all(u8::is_ascii_alphabetic)
            || !kind[2].is_ascii_uppercase()
            || (kind[0].is_ascii_uppercase()
                && !matches!(kind, b"IHDR" | b"PLTE" | b"IDAT" | b"IEND"))
            || (kind == b"IHDR" && offset != 8)
            || (kind == b"PLTE" && saw_data)
            || (kind == b"IDAT" && data_ended)
        {
            return Err(invalid("invalid clipboard PNG chunk order or type"));
        }
        let expected_crc = u32::from_be_bytes(bytes[end - 4..end].try_into().expect("PNG CRC"));
        if png_crc(&bytes[offset + 4..end - 4], cancelled)? != expected_crc {
            return Err(invalid("clipboard PNG chunk checksum is invalid"));
        }
        if kind == b"IEND" {
            return if length == 0 && end == bytes.len() && saw_data {
                Ok(())
            } else {
                Err(invalid("invalid clipboard PNG ending"))
            };
        }
        if kind == b"IDAT" {
            saw_data = true;
        } else if saw_data {
            data_ended = true;
        }
        offset = end;
    }
    Err(invalid("clipboard PNG has too many chunks"))
}

// Validate the PNG format's required CRCs, including chunks after IDAT which a
// single-frame image decoder need not read. This is not an asset identity hash.
fn png_crc(bytes: &[u8], cancelled: &AtomicBool) -> Result<u32, ProjectError> {
    const TABLE: [u32; 256] = {
        let mut table = [0; 256];
        let mut index = 0;
        while index < 256 {
            let mut value = index as u32;
            let mut bit = 0;
            while bit < 8 {
                value = (value >> 1) ^ if value & 1 == 0 { 0 } else { 0xedb8_8320 };
                bit += 1;
            }
            table[index] = value;
            index += 1;
        }
        table
    };
    let mut crc = u32::MAX;
    for chunk in bytes.chunks(16 * 1024) {
        check_active(cancelled)?;
        for byte in chunk {
            crc = TABLE[((crc ^ u32::from(*byte)) & 255) as usize] ^ (crc >> 8);
        }
    }
    Ok(!crc)
}

fn validate_jpeg_container(bytes: &[u8], cancelled: &AtomicBool) -> Result<(), ProjectError> {
    let mut offset = 2usize;
    let mut in_scan = false;
    let mut scan_has_data = false;
    let mut saw_frame = false;
    let mut saw_scan = false;
    for _ in 0..MAX_CONTAINER_PARTS {
        check_active(cancelled)?;
        if in_scan {
            while bytes.get(offset).is_some_and(|byte| *byte != 0xff) {
                check_active(cancelled)?;
                let end = bytes.len().min(offset.saturating_add(16 * 1024));
                offset += bytes[offset..end]
                    .iter()
                    .position(|byte| *byte == 0xff)
                    .unwrap_or(end - offset);
                scan_has_data = true;
            }
        }
        if bytes.get(offset) != Some(&0xff) {
            return Err(invalid("clipboard JPEG is missing a complete ending"));
        }
        while bytes.get(offset) == Some(&0xff) {
            offset += 1;
            if offset.is_multiple_of(16 * 1024) {
                check_active(cancelled)?;
            }
        }
        let marker = *bytes
            .get(offset)
            .ok_or_else(|| invalid("truncated clipboard JPEG marker"))?;
        offset += 1;
        if in_scan && matches!(marker, 0x00 | 0xd0..=0xd7) {
            scan_has_data |= marker == 0x00;
            continue;
        }
        if in_scan && !scan_has_data {
            return Err(invalid("clipboard JPEG has an empty compressed scan"));
        }
        in_scan = false;
        if marker == 0xd9 {
            return if saw_frame && saw_scan && offset == bytes.len() {
                Ok(())
            } else {
                Err(invalid("invalid clipboard JPEG ending"))
            };
        }
        if matches!(marker, 0x00 | 0x01 | 0xd0..=0xd8 | 0xdc) {
            return Err(invalid("unsupported clipboard JPEG marker"));
        }
        let length = bytes
            .get(offset..offset + 2)
            .ok_or_else(|| invalid("truncated clipboard JPEG segment"))?;
        let length = u16::from_be_bytes(length.try_into().expect("JPEG segment size")) as usize;
        let end = offset
            .checked_add(length)
            .filter(|end| length >= 2 && *end <= bytes.len())
            .ok_or_else(|| invalid("invalid clipboard JPEG segment"))?;
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            let header = &bytes[offset + 2..end];
            if saw_frame || !matches!(marker, 0xc0..=0xc2) || header.len() < 6 || header[0] != 8 {
                return Err(invalid("unsupported clipboard JPEG coding mode"));
            }
            let components = usize::from(header[5]);
            if !(1..=4).contains(&components)
                || header.len() != 6 + 3 * components
                || header[6..].as_chunks::<3>().0.iter().any(|component| {
                    !(1..=4).contains(&(component[1] >> 4))
                        || !(1..=4).contains(&(component[1] & 15))
                })
            {
                return Err(invalid("invalid clipboard JPEG component geometry"));
            }
            saw_frame = true;
        }
        if marker == 0xda {
            if !saw_frame {
                return Err(invalid("clipboard JPEG scan has no size header"));
            }
            let header = &bytes[offset + 2..end];
            if header.len() < 6
                || !(1..=4).contains(&header[0])
                || header.len() != 4 + 2 * usize::from(header[0])
            {
                return Err(invalid("invalid clipboard JPEG scan header"));
            }
            saw_scan = true;
            in_scan = true;
            scan_has_data = false;
        }
        offset = end;
    }
    Err(invalid("clipboard JPEG has too many segments"))
}

struct CancelReader<'a> {
    cursor: Cursor<&'a [u8]>,
    cancelled: &'a AtomicBool,
}
impl Read for CancelReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        check_active(self.cancelled).map_err(io::Error::other)?;
        let length = buffer.len().min(16 * 1024);
        self.cursor.read(&mut buffer[..length])
    }
}
impl BufRead for CancelReader<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        check_active(self.cancelled).map_err(io::Error::other)?;
        self.cursor.fill_buf()
    }
    fn consume(&mut self, amount: usize) {
        self.cursor.consume(amount);
    }
}
impl Seek for CancelReader<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        check_active(self.cancelled).map_err(io::Error::other)?;
        self.cursor.seek(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;

    struct TestHome(PathBuf);
    impl TestHome {
        fn new() -> Self {
            let base = fs::canonicalize(std::env::temp_dir()).unwrap();
            let path = base.join(unique_name("flow-clipboard-test"));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn project(&self) -> (ProjectStore, LocalProject) {
            let store = ProjectStore::new(self.0.join("projects")).unwrap();
            let project = store
                .create_project(
                    "Clipboard test",
                    &FlowDocument::default(),
                    &crate::standard_registry(),
                )
                .unwrap();
            (store, project)
        }
    }
    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(&[90; 18], 2, 3, image::ExtendedColorType::Rgb8)
            .unwrap();
        bytes
    }
    fn jpeg() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut bytes)
            .encode(&[90; 18], 2, 3, image::ExtendedColorType::Rgb8)
            .unwrap();
        bytes
    }
    fn entries(path: &Path) -> Vec<PathBuf> {
        let mut names: Vec<_> = fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into())
            .collect();
        names.sort();
        names
    }
    fn snapshot(project: &LocalProject) -> (Vec<u8>, Vec<PathBuf>, Vec<PathBuf>, Vec<u8>) {
        (
            fs::read(project.directory.join(PROJECT_FILE)).unwrap(),
            entries(&project.directory),
            entries(&project.directory.join("assets")),
            serde_json::to_vec(&project.project).unwrap(),
        )
    }
    fn assert_rejected_unchanged(project: &mut LocalProject, bytes: &[u8], media_type: &str) {
        let before = snapshot(project);
        assert!(
            project
                .import_clipboard_image(bytes, media_type, &AtomicBool::new(false))
                .is_err()
        );
        assert_eq!(snapshot(project), before);
    }

    #[test]
    fn png_and_jpeg_preserve_exact_bytes_and_reopen() {
        let home = TestHome::new();
        let (store, mut project) = home.project();
        for (bytes, media_type) in [(png(), "image/png"), (jpeg(), "image/jpeg")] {
            let document = serde_json::to_vec(project.document()).unwrap();
            let asset = project
                .import_clipboard_image(&bytes, media_type, &AtomicBool::new(false))
                .unwrap();
            assert_eq!(
                (asset.width, asset.height, asset.byte_len),
                (2, 3, bytes.len() as u64)
            );
            assert_eq!(asset.media_type, media_type);
            validate_asset_key(&asset.key).unwrap();
            assert_eq!(
                fs::read(project.resolve_asset(&asset.key).unwrap()).unwrap(),
                bytes
            );
            let reopened = store
                .open_project(&project.metadata().id, &crate::standard_registry())
                .unwrap();
            assert_eq!(reopened.assets(), project.assets());
            assert_eq!(
                fs::read(reopened.resolve_asset(&asset.key).unwrap()).unwrap(),
                bytes
            );
            assert_eq!(serde_json::to_vec(reopened.document()).unwrap(), document);
        }
    }

    #[test]
    fn unsupported_mismatched_corrupt_and_truncated_inputs_never_mutate_project() {
        let home = TestHome::new();
        let (_, mut project) = home.project();
        for bytes in [
            b"".as_slice(),
            b"<svg></svg>",
            b"GIF89a",
            b"https://example.com/image.png",
            b"/tmp/image.png",
            b"file:///tmp/image.jpg",
            b"unknown bytes",
        ] {
            assert_rejected_unchanged(&mut project, bytes, "image/png");
        }
        assert_rejected_unchanged(&mut project, &png(), "image/jpeg");
        assert_rejected_unchanged(&mut project, &jpeg(), "image/png");
        assert_rejected_unchanged(&mut project, &png(), "image/PNG");
        assert_rejected_unchanged(&mut project, &png(), "image/png; charset=utf-8");
        let mut broken_png = png();
        broken_png[29] ^= 1;
        assert_rejected_unchanged(&mut project, &broken_png, "image/png");
        let bytes = png();
        assert_rejected_unchanged(&mut project, &bytes[..bytes.len() - 1], "image/png");
        let bytes = jpeg();
        assert_rejected_unchanged(&mut project, &bytes[..bytes.len() - 2], "image/jpeg");
        // A complete container with a valid CRC but invalid zlib payload must
        // reach the full decoder and still leave storage untouched.
        let mut bytes = png();
        let mut offset = 8;
        while &bytes[offset + 4..offset + 8] != b"IDAT" {
            offset +=
                12 + u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        }
        let end = offset
            + 12
            + u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        bytes[offset + 8..end - 4].fill(0);
        let crc = png_crc(&bytes[offset + 4..end - 4], &AtomicBool::new(false)).unwrap();
        bytes[end - 4..end].copy_from_slice(&crc.to_be_bytes());
        assert_rejected_unchanged(&mut project, &bytes, "image/png");
    }

    #[test]
    fn animation_and_resource_limits_fail_before_any_write() {
        let home = TestHome::new();
        let (_, mut project) = home.project();
        let mut animated = png();
        let mut chunk = 8u32.to_be_bytes().to_vec();
        chunk.extend_from_slice(b"acTL");
        chunk.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0]);
        let crc = png_crc(&chunk[4..], &AtomicBool::new(false)).unwrap();
        chunk.extend_from_slice(&crc.to_be_bytes());
        animated.splice(33..33, chunk);
        assert_rejected_unchanged(&mut project, &animated, "image/png");
        assert_rejected_unchanged(
            &mut project,
            &vec![0; MAX_IMAGE_BYTES as usize + 1],
            "image/png",
        );
        for (width, height) in [(4097u32, 1u32), (4096, 1025), (0, 1)] {
            let mut bytes = png();
            bytes[16..20].copy_from_slice(&width.to_be_bytes());
            bytes[20..24].copy_from_slice(&height.to_be_bytes());
            let crc = png_crc(&bytes[12..29], &AtomicBool::new(false)).unwrap();
            bytes[29..33].copy_from_slice(&crc.to_be_bytes());
            assert_rejected_unchanged(&mut project, &bytes, "image/png");
        }
        let info = AssetInfo {
            extension: "jpg",
            media_type: "image/jpeg",
            width: 4096,
            height: 1024,
        };
        assert!(admit_decode(&info, ImageFormat::Jpeg, MAX_IMAGE_BYTES).is_err());
        assert!(admit_decode(&info, ImageFormat::Jpeg, 1024).is_ok());
        assert!(admit_decode(&info, ImageFormat::Png, MAX_IMAGE_BYTES).is_ok());
        assert!(check_memory(MAX_DECODE_BYTES).is_ok());
        assert!(check_memory(MAX_DECODE_BYTES + 1).is_err());
    }

    #[test]
    fn oriented_jpeg_keeps_original_bytes_and_uses_logical_geometry() {
        let home = TestHome::new();
        let (_, mut project) = home.project();
        let jpeg = jpeg();
        let exif = b"Exif\0\0II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut rotated = jpeg[..2].to_vec();
        rotated.extend_from_slice(&[0xff, 0xe1]);
        rotated.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        rotated.extend_from_slice(exif);
        rotated.extend_from_slice(&jpeg[2..]);
        let raw = image::load_from_memory(&rotated).unwrap();
        let asset = project
            .import_clipboard_image(&rotated, "image/jpeg", &AtomicBool::new(false))
            .unwrap();
        assert_eq!((asset.width, asset.height), (raw.height(), raw.width()));
        assert_eq!(
            fs::read(project.directory().join(&asset.key)).unwrap(),
            rotated
        );
    }

    #[test]
    fn stale_handle_and_cancellation_leave_files_manifest_and_handle_unchanged() {
        let home = TestHome::new();
        let (_, mut project) = home.project();
        let mut stale = project.clone();
        project
            .import_clipboard_image(&png(), "image/png", &AtomicBool::new(false))
            .unwrap();
        let before = snapshot(&stale);
        assert!(matches!(
            stale.import_clipboard_image(&png(), "image/png", &AtomicBool::new(false)),
            Err(ProjectError::Conflict)
        ));
        assert_eq!(snapshot(&stale), before);
        let before = snapshot(&project);
        assert!(
            project
                .import_clipboard_image(&png(), "image/png", &AtomicBool::new(true))
                .is_err()
        );
        assert_eq!(snapshot(&project), before);
    }

    #[test]
    fn exhausted_asset_count_and_project_bytes_leave_storage_untouched() {
        for (count, byte_len) in [(MAX_PROJECT_ASSETS, 1), (16, MAX_IMAGE_BYTES)] {
            let home = TestHome::new();
            let (_, mut project) = home.project();
            project.project.assets = (0..count)
                .map(|index| ProjectAsset {
                    key: format!("assets/existing-{index}.png"),
                    name: "Existing image".into(),
                    media_type: "image/png".into(),
                    byte_len,
                    width: 1,
                    height: 1,
                })
                .collect();
            write_project(&project.directory, &project.project).unwrap();
            assert_rejected_unchanged(&mut project, &png(), "image/png");
        }
    }
}

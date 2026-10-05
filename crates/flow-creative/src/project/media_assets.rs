//! Explicit local audio/video import. Files are copied before probing so the
//! inspected bytes are exactly the project-owned bytes that will be committed.
use super::*;
use gpui_flow_media::{FfmpegBackend, MediaLimits};
use std::time::Duration;

pub const MAX_MEDIA_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_MEDIA_DURATION_SECONDS: u64 = 30 * 60;

pub fn creative_media_backend() -> FfmpegBackend {
    FfmpegBackend {
        limits: MediaLimits {
            max_input_bytes: MAX_MEDIA_BYTES,
            max_source_pixels: 4096 * 4096,
            max_duration: Duration::from_secs(MAX_MEDIA_DURATION_SECONDS),
            ..MediaLimits::default()
        },
        ..FfmpegBackend::default()
    }
}

impl LocalProject {
    /// Import an explicit user-selected local clip; never persist its source
    /// path. Requires host-installed FFprobe and must run off the UI thread.
    pub fn import_media(&mut self, source: impl AsRef<Path>) -> Result<ProjectAsset, ProjectError> {
        let _transaction = transaction()?;
        self.check_revision()?;
        if self.project.assets.len() >= MAX_PROJECT_ASSETS {
            return Err(ProjectError::TooLarge(
                "project has reached its 512 asset limit".into(),
            ));
        }
        let source = absolute_path(source.as_ref())?;
        let bytes = read_regular(&source, MAX_MEDIA_BYTES)?;
        let mut info = media_info(&bytes, None)?;
        let total = self.project.assets.iter().map(|a| a.byte_len).sum::<u64>();
        if total.saturating_add(bytes.len() as u64) > MAX_PROJECT_ASSET_BYTES {
            return Err(ProjectError::TooLarge(
                "project assets exceed 512 MiB".into(),
            ));
        }
        checked_directory(&self.directory.join("assets"), false)?;
        let key = format!("assets/{}.{}", unique_name("a"), info.extension);
        let path = self.directory.join(&key);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            verify_media(&path, &mut info)?;
            let name = source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .chars()
                .filter(|c| !c.is_control())
                .take(200)
                .collect::<String>();
            let asset = ProjectAsset {
                key,
                name: if name.is_empty() {
                    "Imported media".into()
                } else {
                    name
                },
                media_type: info.media_type.into(),
                byte_len: bytes.len() as u64,
                width: info.width,
                height: info.height,
            };
            #[cfg(unix)]
            File::open(self.directory.join("assets"))?.sync_all()?;
            let mut candidate = self.next_revision()?;
            candidate.assets.push(asset.clone());
            write_project(&self.directory, &candidate)?;
            self.project = candidate;
            Ok(asset)
        })();
        if result.is_err() {
            let _ = fs::remove_file(path);
        }
        result
    }
}

pub(super) fn verify_media(path: &Path, info: &mut AssetInfo) -> Result<(), ProjectError> {
    let metadata = creative_media_backend()
        .probe(path)
        .map_err(|e| ProjectError::Invalid(e.to_string()))?;
    let expected = match info.extension {
        "mp4" | "m4a" => "mov",
        "webm" => "webm",
        "mp3" => "mp3",
        "wav" => "wav",
        "flac" => "flac",
        "ogg" => "ogg",
        _ => return Err(ProjectError::Invalid("unsupported media container".into())),
    };
    if !metadata.format.split(',').any(|format| format == expected) {
        return Err(ProjectError::Invalid(
            "media header and decoded container do not match".into(),
        ));
    }
    if info.media_type.starts_with("video/") {
        if !metadata.has_video {
            return Err(ProjectError::Invalid(
                "video file has no video stream; import audio as WAV, MP3, FLAC, OGG or M4A".into(),
            ));
        }
        info.width = metadata.width.unwrap_or(0);
        info.height = metadata.height.unwrap_or(0);
        validate_dimensions(info.width, info.height)?;
    } else if !metadata.has_audio {
        return Err(ProjectError::Invalid(
            "audio file has no audio stream".into(),
        ));
    }
    Ok(())
}

pub(super) fn media_info(
    bytes: &[u8],
    required: Option<AssetKind>,
) -> Result<AssetInfo, ProjectError> {
    let (extension, media_type) =
        if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
            ("wav", "audio/wav")
        } else if bytes.starts_with(b"fLaC") {
            ("flac", "audio/flac")
        } else if bytes.starts_with(b"OggS") {
            ("ogg", "audio/ogg")
        } else if bytes.starts_with(b"ID3")
            || (bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0)
        {
            ("mp3", "audio/mpeg")
        } else if bytes.len() >= 16 && &bytes[4..8] == b"ftyp" {
            if &bytes[8..12] == b"M4A " {
                ("m4a", "audio/mp4")
            } else {
                ("mp4", "video/mp4")
            }
        } else if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
            ("webm", "video/webm")
        } else {
            return Err(ProjectError::Invalid(
                "choose a local MP4/WebM video or WAV/MP3/FLAC/OGG/M4A audio file".into(),
            ));
        };
    if required.is_some_and(|kind| match kind {
        AssetKind::Video => !media_type.starts_with("video/"),
        AssetKind::Audio => !media_type.starts_with("audio/"),
        _ => true,
    }) {
        return Err(ProjectError::Invalid(
            "media container has the wrong asset type".into(),
        ));
    }
    Ok(AssetInfo {
        extension,
        media_type,
        width: 0,
        height: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn playlists_urls_and_wrong_media_kinds_are_rejected_before_probe() {
        for bytes in [
            b"#EXTM3U\nfile:///etc/passwd".as_slice(),
            b"https://example.com/video.mp4",
            b"<MPD></MPD>",
        ] {
            assert!(media_info(bytes, None).is_err());
        }
        let wav = b"RIFF\x00\x00\x00\x00WAVE";
        assert!(media_info(wav, Some(AssetKind::Audio)).is_ok());
        assert!(media_info(wav, Some(AssetKind::Video)).is_err());
    }
}

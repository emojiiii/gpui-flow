//! Asset metadata uses the same oriented pixel coordinates as GPUI. Originals
//! remain encoded byte-for-byte; only legacy manifests are reconciled on open.
use super::*;
use crate::image_orientation::ImageOrientation;
use image::{ImageReader, Limits, metadata::Orientation};
use std::io::Cursor;

pub(super) fn inspect(bytes: &[u8], info: &AssetInfo) -> Result<ImageOrientation, ProjectError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(ProjectError::Io)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    // Header/metadata admission only; this function never decodes image pixels.
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(image_error)?;
    let geometry = ImageOrientation::from_decoder(&mut decoder).map_err(image_error)?;
    if geometry.raw_dimensions != (info.width, info.height) {
        return Err(ProjectError::Invalid(
            tr(
                "解码后的图片尺寸与文件头部不符",
                "image decoder disagrees with encoded dimensions",
            )
            .into(),
        ));
    }
    Ok(geometry)
}

fn image_error(error: image::ImageError) -> ProjectError {
    ProjectError::Invalid(tr_format!(
        "图片方向或图像头无效：{error}",
        "Invalid image orientation or header: {error}"
    ))
}

fn reconcile_image(
    directory: &Path,
    asset: &mut ProjectAsset,
    legacy: bool,
) -> Result<bool, ProjectError> {
    let bytes = read_regular(&directory.join(&asset.key), MAX_IMAGE_BYTES)?;
    let raw = encoded_image_info(&bytes)?;
    let geometry = inspect(&bytes, &raw)?;
    let saved = (asset.width, asset.height);
    if bytes.len() as u64 != asset.byte_len
        || raw.media_type != asset.media_type
        || (saved != geometry.oriented_dimensions && !(legacy && saved == geometry.raw_dimensions))
    {
        return Err(ProjectError::Invalid(tr_format!(
            "素材与项目清单不匹配：{}",
            "asset does not match its manifest: {}",
            asset.key
        )));
    }
    (asset.width, asset.height) = geometry.oriented_dimensions;
    Ok(legacy && geometry.orientation != Orientation::NoTransforms)
}

pub(super) fn reconcile_assets(
    directory: &Path,
    project: &mut ProjectFile,
    mut checkpoint: impl FnMut() -> Result<(), ProjectError>,
) -> Result<BTreeSet<String>, ProjectError> {
    let legacy = project.schema_version == 1;
    let mut changed = BTreeSet::new();
    for asset in &mut project.assets {
        checkpoint()?;
        if matches!(
            AssetKind::from_media_type(&asset.media_type)?,
            AssetKind::Image
        ) {
            if reconcile_image(directory, asset, legacy)? {
                changed.insert(asset.key.clone());
            }
        } else {
            verify_asset(&directory.join(&asset.key), asset)?;
        }
        checkpoint()?;
    }
    Ok(changed)
}

#[cfg(feature = "desktop")]
pub(super) fn reconcile_preview(
    directory: &Path,
    project: &mut ProjectFile,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<(), ProjectError> {
    // Only images used by the chosen visible artwork can affect its aspect
    // ratio. Missing unrelated library files must not break a read-only cover.
    if project.schema_version != 1 {
        return Ok(());
    }
    let graph = &project.document.graph;
    let hidden = gpui_flow_core::hidden_node_ids(graph);
    let frame = graph
        .groups
        .values()
        .find(|frame| frame.node_ids.iter().any(|id| !hidden.contains(id)));
    let keys = graph
        .nodes
        .values()
        .filter(|node| {
            !hidden.contains(&node.id)
                && (graph.groups.is_empty()
                    || frame.is_some_and(|frame| frame.node_ids.contains(&node.id)))
        })
        .filter_map(|node| node.data.get("asset").and_then(serde_json::Value::as_str))
        .collect::<BTreeSet<_>>();
    let start = std::time::Instant::now();
    for asset in &mut project.assets {
        if !keys.contains(asset.key.as_str()) || !asset.media_type.starts_with("image/") {
            continue;
        }
        if cancelled.load(Ordering::Acquire) || start.elapsed() > std::time::Duration::from_secs(4)
        {
            return Err(ProjectError::Invalid(
                tr(
                    "项目预览读取已取消或超时",
                    "Project preview metadata cancelled or timed out",
                )
                .into(),
            ));
        }
        reconcile_image(directory, asset, true)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_core::{Point, Size};
    use image::ImageEncoder;

    fn png(orientation: u8) -> Vec<u8> {
        let mut exif = b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x01\0\0\0\0\0\0\0".to_vec();
        exif[18] = orientation;
        let pixels = image::RgbaImage::from_fn(3, 2, |x, y| {
            image::Rgba([(x * 70) as u8, (y * 90) as u8, 45, 255])
        });
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
        encoder.set_exif_metadata(exif).unwrap();
        encoder
            .write_image(pixels.as_raw(), 3, 2, image::ExtendedColorType::Rgba8)
            .unwrap();
        bytes
    }
    struct Home(PathBuf);
    impl Home {
        fn new() -> Self {
            let path = std::env::temp_dir().join(unique_name("flow-orientation"));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn exif_import_and_legacy_open_preserve_originals_geometry_and_migrate_once() {
        for orientation in 1..=8 {
            let home = Home::new();
            let registry = crate::standard_registry();
            let store = ProjectStore::new(home.0.join("projects")).unwrap();
            let mut project = store
                .create_project("Legacy EXIF", &FlowDocument::default(), &registry)
                .unwrap();
            let bytes = png(orientation);
            let path = home.0.join("original.png");
            fs::write(&path, &bytes).unwrap();
            let asset = project.import_image(&path).unwrap();
            let expected = if orientation >= 5 { (2, 3) } else { (3, 2) };
            assert_eq!((asset.width, asset.height), expected);
            let mut document = project.document().clone();
            let mut node = registry
                .make_node("image", "image", Point::new(42., 71.), None)
                .unwrap();
            node.size = Size::new(617., 219.);
            node.data["asset"] = serde_json::json!(&asset.key);
            document.graph.add_node(node).unwrap();
            let mut mask = registry
                .make_node("mask", "mask", Point::new(900., 70.), None)
                .unwrap();
            let mut data = crate::MaskNodeData::default();
            data.reset_source(crate::mask::MaskSource::new(&asset.key, 3, 2).unwrap())
                .unwrap();
            mask.data = serde_json::to_value(data).unwrap();
            document.graph.add_node(mask).unwrap();
            // Reproduce the old envelope without calling the new validator.
            project.project.document = document.clone();
            project.project.schema_version = 1;
            project.project.assets[0].width = 3;
            project.project.assets[0].height = 2;
            fs::write(
                project.directory.join(PROJECT_FILE),
                serde_json::to_vec(&project.project).unwrap(),
            )
            .unwrap();
            let old_revision = project.revision();
            let opened = store
                .open_project(&project.metadata().id, &registry)
                .unwrap();
            assert_eq!(
                (opened.assets()[0].width, opened.assets()[0].height),
                expected
            );
            assert_eq!(
                opened.document().graph.node(&"image".into()),
                document.graph.node(&"image".into())
            );
            assert_eq!(
                opened.migrated_orientation_count(),
                usize::from(orientation != 1)
            );
            assert_eq!(
                opened.revision(),
                old_revision + u64::from(orientation != 1)
            );
            assert_eq!(
                crate::mask_node::orientation_review_required(
                    &opened.document().graph.node(&"mask".into()).unwrap().data
                )
                .unwrap(),
                orientation != 1
            );
            let reopened = store
                .open_project(&project.metadata().id, &registry)
                .unwrap();
            assert_eq!(reopened.revision(), opened.revision());
            assert_eq!(reopened.migrated_orientation_count(), 0);
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert_eq!(fs::read(opened.directory.join(&asset.key)).unwrap(), bytes);
        }
    }

    #[test]
    fn invalid_exif_and_failed_legacy_manifest_leave_project_and_files_unchanged() {
        let home = Home::new();
        let registry = crate::standard_registry();
        let store = ProjectStore::new(home.0.join("projects")).unwrap();
        let mut project = store
            .create_project("Invalid EXIF", &FlowDocument::default(), &registry)
            .unwrap();
        let path = home.0.join("invalid.png");
        fs::write(&path, png(9)).unwrap();
        let before = fs::read(project.directory.join(PROJECT_FILE)).unwrap();
        assert!(project.import_image(&path).is_err());
        assert_eq!(
            fs::read(project.directory.join(PROJECT_FILE)).unwrap(),
            before
        );
        assert_eq!(
            fs::read_dir(project.directory.join("assets"))
                .unwrap()
                .count(),
            0
        );
        fs::write(&path, png(6)).unwrap();
        project.import_image(&path).unwrap();
        project.project.schema_version = 1;
        project.project.assets[0].width = 8;
        let before = serde_json::to_vec(&project.project).unwrap();
        fs::write(project.directory.join(PROJECT_FILE), &before).unwrap();
        assert!(
            store
                .open_project(&project.metadata().id, &registry)
                .is_err()
        );
        assert_eq!(
            fs::read(project.directory.join(PROJECT_FILE)).unwrap(),
            before
        );
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn legacy_preview_normalizes_only_used_images_in_memory_and_can_cancel() {
        let home = Home::new();
        let registry = crate::standard_registry();
        let store = ProjectStore::new(home.0.join("projects")).unwrap();
        let mut project = store
            .create_project("Preview", &FlowDocument::default(), &registry)
            .unwrap();
        let path = home.0.join("image.png");
        fs::write(&path, png(6)).unwrap();
        let asset = project.import_image(&path).unwrap();
        let mut node = registry
            .make_node("image", "image", Point::ZERO, None)
            .unwrap();
        node.data["asset"] = serde_json::json!(asset.key);
        project.project.document.graph.add_node(node).unwrap();
        project.project.schema_version = 1;
        project.project.assets[0].width = 3;
        project.project.assets[0].height = 2;
        let before = serde_json::to_vec(&project.project).unwrap();
        fs::write(project.directory.join(PROJECT_FILE), &before).unwrap();
        let preview = store
            .read_preview_project(&project.metadata().id, &registry)
            .unwrap();
        assert_eq!(
            (preview.assets()[0].width, preview.assets()[0].height),
            (2, 3)
        );
        assert_eq!(preview.document(), project.document());
        assert!(
            store
                .read_preview_project_cancellable(
                    &project.metadata().id,
                    &registry,
                    &std::sync::atomic::AtomicBool::new(true)
                )
                .is_err()
        );
        assert_eq!(
            fs::read(project.directory.join(PROJECT_FILE)).unwrap(),
            before
        );
    }
}

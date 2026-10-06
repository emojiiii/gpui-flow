//! Explicit local exports of completed, project-owned media outputs.
use crate::custom_nodes::{CustomValue, resolve_graph_output};
use crate::{CreativeError, LocalProject, ProjectAsset, ProjectError};
use gpui_flow_core::{DataType, Graph, Node, NodeId, PortRef};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq)]
pub struct ExportAsset {
    pub port: String,
    pub label: String,
    pub asset: ProjectAsset,
    pub suggested_name: String,
    pub value: CustomValue,
}

pub fn has_media_output(node: &Node) -> bool {
    node.outputs().any(|p| {
        matches!(&p.data_type, DataType::Named(name)
        if matches!(name.as_str(), "image" | "video" | "audio" | "model" | "mask"))
    })
}

/// Resolve actual output ports, including source/parameter invalidation of local
/// processors, masks and custom nodes. Missing providers produce no fake result.
pub fn export_assets(
    graph: &Graph,
    node: &NodeId,
    assets: &[ProjectAsset],
) -> Result<Vec<ExportAsset>, CreativeError> {
    let node = graph
        .node(node)
        .ok_or_else(|| CreativeError("对象已不存在".into()))?;
    let mut outputs = Vec::new();
    let mut error = None;
    for port in node.outputs() {
        if !matches!(&port.data_type, DataType::Named(name)
            if matches!(name.as_str(), "image" | "video" | "audio" | "model" | "mask"))
        {
            continue;
        }
        let resolved = (|| {
            let value =
                resolve_graph_output(graph, &PortRef::new(node.id.clone(), port.id.clone()))?;
            crate::custom_host::validate_custom_value_assets(&value, assets)?;
            let key = value
                .asset_key()
                .ok_or_else(|| CreativeError("输出尚无媒体素材".into()))?;
            let asset = assets
                .iter()
                .find(|asset| asset.key == key)
                .ok_or_else(|| CreativeError("素材不属于当前项目".into()))?
                .clone();
            let extension = Path::new(&asset.key)
                .extension()
                .and_then(|s| s.to_str())
                .ok_or_else(|| CreativeError("素材缺少原格式后缀".into()))?;
            let stem = Path::new(&node.title)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Export");
            let stem = safe_stem(stem);
            let suffix = if node.outputs().count() > 1 {
                format!("-{}", safe_stem(port.id.as_str()))
            } else {
                String::new()
            };
            Ok::<_, CreativeError>(ExportAsset {
                port: port.id.to_string(),
                label: port.label.clone(),
                suggested_name: format!("{stem}{suffix}.{extension}"),
                asset,
                value,
            })
        })();
        match resolved {
            Ok(output) => outputs.push(output),
            Err(reason) => error = Some(reason),
        }
    }
    if outputs.is_empty() {
        return Err(error.unwrap_or_else(|| CreativeError("没有已完成的本地媒体输出可导出".into())));
    }
    Ok(outputs)
}

fn safe_stem(value: &str) -> String {
    let name: String = value
        .chars()
        .take(96)
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim_matches([' ', '.']);
    if name.is_empty() {
        "Export".into()
    } else {
        name.into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportResult {
    pub path: PathBuf,
    pub byte_len: u64,
}

fn cancelled(cancel: &AtomicBool, started: Instant) -> Result<(), ProjectError> {
    if cancel.load(Ordering::Acquire) {
        return Err(ProjectError::Invalid("素材导出已取消".into()));
    }
    if started.elapsed() > Duration::from_secs(60) {
        return Err(ProjectError::Invalid(
            "素材导出超过60秒，请选择可写的本地目录".into(),
        ));
    }
    Ok(())
}

fn export_destination(project: &LocalProject, destination: &Path) -> Result<PathBuf, ProjectError> {
    if !destination.is_absolute() {
        return Err(ProjectError::Invalid("请选择导出文件的完整绝对路径".into()));
    }
    let filename = destination
        .file_name()
        .ok_or_else(|| ProjectError::Invalid("请选择文件名".into()))?;
    let parent = destination
        .parent()
        .ok_or_else(|| ProjectError::Invalid("请选择已有的导出目录".into()))?
        .canonicalize()?;
    if !parent.is_dir() {
        return Err(ProjectError::Invalid("导出目录不存在".into()));
    }
    let destination = parent.join(filename);
    if destination.starts_with(project.directory().canonicalize()?) {
        return Err(ProjectError::Invalid(
            "请导出到项目目录之外，项目原始素材会保留".into(),
        ));
    }
    Ok(destination)
}

/// Write a validated vector snapshot on a worker, without adding an asset to
/// the project. The caller invalidates the snapshot on selection/data changes.
/// A cancelled or failed write removes only its newly created incomplete file.
pub fn export_vector(
    project: &LocalProject,
    vector: &crate::vector_export::VectorExport,
    destination: &Path,
    cancel: &AtomicBool,
) -> Result<ExportResult, ProjectError> {
    let started = Instant::now();
    cancelled(cancel, started)?;
    let destination = export_destination(project, destination)?;
    if !destination
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
    {
        return Err(ProjectError::Invalid("矢量导出请使用 .svg 后缀".into()));
    }
    let bytes = vector.to_svg().map_err(ProjectError::Invalid)?;
    cancelled(cancel, started)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ProjectError::Invalid("目标文件已存在，未覆盖；请换一个文件名".into())
            } else {
                error.into()
            }
        })?;
    let result = (|| {
        for chunk in bytes.chunks(64 * 1024) {
            cancelled(cancel, started)?;
            output.write_all(chunk)?;
        }
        output.sync_all()?;
        cancelled(cancel, started)?;
        Ok(ExportResult {
            path: destination.clone(),
            byte_len: bytes.len() as u64,
        })
    })();
    drop(output);
    if result.is_err()
        && let Err(error) = fs::remove_file(&destination)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(ProjectError::Invalid(format!(
            "导出未完成，部分文件无法移除，请检查 {}：{error}",
            destination.display()
        )));
    }
    result
}

/// Copy original bytes on a worker. Never overwrite a destination, never export
/// into the current project, and clean up only the new incomplete output owned
/// by this operation. No transcoding, network access or project mutation occurs.
pub fn export_asset(
    project: &LocalProject,
    key: &str,
    destination: &Path,
    cancel: &AtomicBool,
) -> Result<ExportResult, ProjectError> {
    let started = Instant::now();
    cancelled(cancel, started)?;
    let asset = project
        .assets()
        .iter()
        .find(|a| a.key == key)
        .ok_or_else(|| ProjectError::Invalid("素材不属于当前项目".into()))?;
    if asset.byte_len > crate::MAX_MEDIA_BYTES {
        return Err(ProjectError::TooLarge("单次导出最多128 MiB".into()));
    }
    let destination = export_destination(project, destination)?;
    let extension = destination
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let valid_extension = match asset.media_type.as_str() {
        "image/png" | crate::MASK_MEDIA_TYPE => extension == "png",
        "image/jpeg" => matches!(extension.as_str(), "jpg" | "jpeg"),
        "image/gif" => extension == "gif",
        "image/webp" => extension == "webp",
        "video/mp4" => extension == "mp4",
        "video/webm" => extension == "webm",
        "audio/mpeg" => extension == "mp3",
        "audio/wav" => extension == "wav",
        "audio/flac" => extension == "flac",
        "audio/ogg" => matches!(extension.as_str(), "ogg" | "opus"),
        "audio/mp4" => matches!(extension.as_str(), "m4a" | "mp4"),
        "model/gltf-binary" => extension == "glb",
        _ => false,
    };
    if !valid_extension {
        return Err(ProjectError::Invalid(
            "导出保留原格式，请使用建议的文件后缀；转码请用处理节点".into(),
        ));
    }
    // Media verification can probe a local container, so this function must not
    // run on the UI thread. Project resolution rejects URLs and symbolic paths.
    let source = project.resolve_asset(key)?;
    cancelled(cancel, started)?;
    let mut input = File::open(source)?;
    let before = input.metadata()?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                ProjectError::Invalid("目标文件已存在，未覆盖；请换一个文件名".into())
            } else {
                e.into()
            }
        })?;
    let result = (|| {
        let mut buffer = [0_u8; 64 * 1024];
        let mut copied = 0_u64;
        loop {
            cancelled(cancel, started)?;
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            copied += count as u64;
            if copied > asset.byte_len {
                return Err(ProjectError::Invalid("源素材在导出时改变".into()));
            }
            output.write_all(&buffer[..count])?;
        }
        let after = input.metadata()?;
        if copied != asset.byte_len
            || before.len() != after.len()
            || before.modified()? != after.modified()?
        {
            return Err(ProjectError::Invalid("源素材在导出时改变".into()));
        }
        output.sync_all()?;
        cancelled(cancel, started)?;
        Ok(ExportResult {
            path: destination.clone(),
            byte_len: copied,
        })
    })();
    drop(output);
    if result.is_err()
        && let Err(error) = fs::remove_file(&destination)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(ProjectError::Invalid(format!(
            "导出未完成，部分文件无法移除，请检查 {}：{error}",
            destination.display()
        )));
    }
    result
}

#[cfg(all(test, feature = "desktop"))]
mod tests {
    use super::*;
    use crate::{ProjectStore, standard_registry};
    use gpui_flow_core::{FlowDocument, Point};
    use serde_json::json;
    use std::sync::atomic::AtomicU64;

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
        project: LocalProject,
        asset: ProjectAsset,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "flow-export-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let source = root.join("source.png");
            image::RgbaImage::from_pixel(3, 2, image::Rgba([12, 98, 201, 127]))
                .save(&source)
                .unwrap();
            let mut project = ProjectStore::new(root.join("projects"))
                .unwrap()
                .create_project("Exports", &FlowDocument::default(), &standard_registry())
                .unwrap();
            let asset = project.import_image(&source).unwrap();
            Self {
                root,
                project,
                asset,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn vector_export_writes_new_svg_preserves_project_and_refuses_unsafe_destinations() {
        use crate::shapes::{ShapeData, ShapeKind, create_shape_node};
        let fixture = Fixture::new();
        let before = fs::read(fixture.project.directory().join("project.json")).unwrap();
        let mut graph = Graph::new();
        graph
            .add_node(
                create_shape_node(
                    "shape",
                    "Square",
                    Point::new(10., 20.),
                    gpui_flow_core::Size::new(100., 80.),
                    ShapeData::new(ShapeKind::Rectangle),
                )
                .unwrap(),
            )
            .unwrap();
        let selection = gpui_flow_core::Selection {
            nodes: ["shape".into()].into_iter().collect(),
            ..Default::default()
        };
        let vector = crate::vector_export::prepare_vector_export(&graph, &selection).unwrap();
        let destination = fixture.root.join("vector.svg");
        let result = export_vector(
            &fixture.project,
            &vector,
            &destination,
            &AtomicBool::new(false),
        )
        .unwrap();
        let bytes = fs::read(&destination).unwrap();
        assert_eq!(bytes, vector.to_svg().unwrap());
        assert_eq!(result.byte_len, bytes.len() as u64);
        assert!(
            export_vector(
                &fixture.project,
                &vector,
                &destination,
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert_eq!(fs::read(&destination).unwrap(), bytes);
        for invalid in [
            fixture.project.directory().join("blocked.svg"),
            fixture.root.join("wrong.png"),
            PathBuf::from("relative.svg"),
        ] {
            assert!(
                export_vector(&fixture.project, &vector, &invalid, &AtomicBool::new(false))
                    .is_err()
            );
            assert!(!invalid.exists());
        }
        let cancelled = fixture.root.join("cancelled.svg");
        assert!(
            export_vector(
                &fixture.project,
                &vector,
                &cancelled,
                &AtomicBool::new(true)
            )
            .is_err()
        );
        assert!(!cancelled.exists());
        assert_eq!(
            fs::read(fixture.project.directory().join("project.json")).unwrap(),
            before
        );
        assert_eq!(fixture.project.assets().len(), 1);
    }

    #[test]
    fn export_copies_original_bytes_without_mutating_source_or_project() {
        let fixture = Fixture::new();
        let project_json = fs::read(fixture.project.directory().join("project.json")).unwrap();
        let source = fixture
            .project
            .resolve_asset_path(&fixture.asset.key)
            .unwrap();
        let bytes = fs::read(&source).unwrap();
        let destination = fixture.root.join("result.png");
        let output = export_asset(
            &fixture.project,
            &fixture.asset.key,
            &destination,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(output.byte_len, bytes.len() as u64);
        assert_eq!(fs::read(destination).unwrap(), bytes);
        assert_eq!(fs::read(source).unwrap(), bytes);
        assert_eq!(
            fs::read(fixture.project.directory().join("project.json")).unwrap(),
            project_json
        );
    }

    #[test]
    fn export_refuses_overwrite_project_paths_wrong_format_and_cancellation() {
        let fixture = Fixture::new();
        let existing = fixture.root.join("existing.png");
        fs::write(&existing, b"keep this file").unwrap();
        let cancel = AtomicBool::new(false);
        assert!(export_asset(&fixture.project, &fixture.asset.key, &existing, &cancel).is_err());
        assert_eq!(fs::read(existing).unwrap(), b"keep this file");
        for path in [
            fixture.project.directory().join("copy.png"),
            fixture.root.join("wrong.jpg"),
            PathBuf::from("relative.png"),
        ] {
            assert!(export_asset(&fixture.project, &fixture.asset.key, &path, &cancel).is_err());
            assert!(!path.exists());
        }
        let cancelled_path = fixture.root.join("cancelled.png");
        cancel.store(true, Ordering::Release);
        assert!(
            export_asset(
                &fixture.project,
                &fixture.asset.key,
                &cancelled_path,
                &cancel
            )
            .is_err()
        );
        assert!(!cancelled_path.exists());
    }

    #[test]
    fn export_uses_real_typed_outputs_and_keeps_the_asset_extension() {
        let fixture = Fixture::new();
        let mut graph = Graph::new();
        let registry = standard_registry();
        let mut image = registry
            .make_node(
                "image",
                "image",
                Point::ZERO,
                Some(json!({"asset": fixture.asset.key, "caption":""})),
            )
            .unwrap();
        image.title = "Cropped photo.jpg".into();
        graph.add_node(image).unwrap();
        graph
            .add_node(
                registry
                    .make_node("generate_image", "pending", Point::ZERO, None)
                    .unwrap(),
            )
            .unwrap();
        let outputs = export_assets(&graph, &"image".into(), fixture.project.assets()).unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].suggested_name, "Cropped photo.png");
        assert_eq!(outputs[0].asset, fixture.asset);
        assert!(export_assets(&graph, &"pending".into(), fixture.project.assets()).is_err());
    }
    #[test]
    fn exporting_a_processor_rejects_stale_source_outputs() {
        let mut fixture = Fixture::new();
        let output = fixture
            .project
            .import_image(fixture.root.join("source.png"))
            .unwrap();
        let registry = standard_registry();
        let mut graph = Graph::new();
        graph
            .add_node(
                registry
                    .make_node(
                        "image",
                        "source",
                        Point::ZERO,
                        Some(json!({"asset":fixture.asset.key,"caption":""})),
                    )
                    .unwrap(),
            )
            .unwrap();
        let settings = crate::processing::ProcessingKind::Image.default_settings();
        let data = crate::processing_node::ProcessingNodeData {
            settings: settings.clone(),
            asset: output.key,
            source_asset: Some(fixture.asset.key.clone()),
            output_settings: Some(settings),
            error: None,
        };
        graph
            .add_node(
                registry
                    .make_node(
                        "process_image",
                        "processed",
                        Point::ZERO,
                        Some(serde_json::to_value(data).unwrap()),
                    )
                    .unwrap(),
            )
            .unwrap();
        graph
            .connect(
                "input",
                PortRef::new("source", "image"),
                PortRef::new("processed", "input"),
            )
            .unwrap();
        assert_eq!(
            export_assets(&graph, &"processed".into(), fixture.project.assets())
                .unwrap()
                .len(),
            1
        );
        graph.nodes.get_mut(&"source".into()).unwrap().data["asset"] = json!("assets/changed.png");
        assert!(export_assets(&graph, &"processed".into(), fixture.project.assets()).is_err());
    }
}

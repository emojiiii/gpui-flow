//! Local-only projects. `project.json` is a versioned envelope around an unchanged
//! `FlowDocument`; its asset manifest and graph commit together in one rename.
//!
//! Only an explicit host-selected path enters `import_image` or `import_model`.
//! JSON contains asset keys, never paths to open. Project IDs and keys are single,
//! validated names; symlink components are rejected before filesystem access. This protects against
//! untrusted project contents, not another process concurrently replacing trusted
//! parent directories. Hosts should serialize saves and keep the returned handle:
//! stale cloned handles fail with `Conflict` rather than overwriting newer work.

use crate::model_preview::{MAX_GLB_BYTES, PreviewError, parse_glb};
use crate::{CreativeError, CreativeRegistry, MAX_CARDS, MAX_JSON_BYTES, MAX_LINKS, tr};
use gpui_flow_core::FlowDocument;
use serde::{Deserialize, Serialize};
#[cfg(any(unix, test))]
use std::fs::File;
use std::{
    collections::BTreeSet,
    fmt,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub const PROJECT_SCHEMA_VERSION: u32 = 2;
pub const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_PROJECT_ASSET_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_PROJECT_ASSETS: usize = 512;
pub const MAX_IMAGE_PIXELS: u64 = 64 * 1024 * 1024;
const MAX_PROJECT_BYTES: u64 = MAX_JSON_BYTES as u64 + 2 * 1024 * 1024;
const PROJECT_FILE: &str = "project.json";
static NEXT_NAME: AtomicU64 = AtomicU64::new(0);
static TRANSACTION: Mutex<()> = Mutex::new(());
#[cfg(feature = "desktop")]
mod clipboard_assets;
mod image_metadata;
mod mask_assets;
mod media_assets;
mod orientation_migration;
pub use media_assets::{MAX_MEDIA_BYTES, MAX_MEDIA_DURATION_SECONDS, creative_media_backend};

#[derive(Debug)]
pub enum ProjectError {
    Io(io::Error),
    Json(serde_json::Error),
    Invalid(String),
    TooLarge(String),
    Conflict,
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Json(error) => error.fmt(f),
            Self::Invalid(message) | Self::TooLarge(message) => message.fmt(f),
            Self::Conflict => f.write_str(tr(
                "磁盘上的项目已更改，请重新打开后再保存",
                "project changed on disk; reopen before saving",
            )),
        }
    }
}
impl std::error::Error for ProjectError {}
impl From<io::Error> for ProjectError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<serde_json::Error> for ProjectError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
impl From<CreativeError> for ProjectError {
    fn from(error: CreativeError) -> Self {
        Self::Invalid(error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectMetadata {
    pub id: String,
    pub name: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectAsset {
    /// A stable project-relative key such as `assets/a-012345.png`.
    pub key: String,
    /// Display name only. Never interpreted as a filesystem path.
    pub name: String,
    pub media_type: String,
    pub byte_len: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSummary {
    pub metadata: ProjectMetadata,
    pub card_count: usize,
    pub asset_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectListWarning {
    pub id: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectListing {
    /// Newest saved project first. Invalid projects are reported in `skipped`.
    pub projects: Vec<ProjectSummary>,
    pub skipped: Vec<ProjectListWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectFile {
    schema_version: u32,
    revision: u64,
    metadata: ProjectMetadata,
    document: FlowDocument,
    #[serde(default)]
    assets: Vec<ProjectAsset>,
}

#[derive(Debug, Clone)]
pub struct ProjectStore {
    root: PathBuf,
}

impl ProjectStore {
    /// Create the local project home if absent. Every component must be a real
    /// directory, with no symlink or `..` indirection.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let root = absolute_path(root.as_ref())?;
        checked_directory(&root, true)?;
        Ok(Self { root })
    }

    /// No filesystem side effects. A host may pass a different explicit location
    /// to `new`, including its own application data directory.
    pub fn default_root() -> Result<PathBuf, ProjectError> {
        #[cfg(target_os = "windows")]
        let root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("Flow Creative").join("Projects"));
        #[cfg(target_os = "macos")]
        let root = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|path| path.join("Library/Application Support/Flow Creative/Projects"));
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let root = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .map(|path| path.join("gpui-flow/projects"));
        root.ok_or_else(|| {
            ProjectError::Invalid(
                tr(
                    "无法定位本地项目目录",
                    "could not locate the local project home",
                )
                .into(),
            )
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Enumerate bounded, validated envelopes. One damaged project does not hide
    /// the others. Unknown card kinds are checked when opening with the registry.
    pub fn list_projects(&self) -> Result<ProjectListing, ProjectError> {
        checked_directory(&self.root, false)?;
        let mut listing = ProjectListing::default();
        for (index, entry) in fs::read_dir(&self.root)?.enumerate() {
            if index >= 4096 {
                listing.skipped.push(ProjectListWarning {
                    id: String::new(),
                    message: "project home listing stopped after 4096 entries".into(),
                });
                break;
            }
            let entry = entry?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if id.starts_with('.') {
                continue;
            }
            let result = (|| {
                validate_id(&id)?;
                let directory = self.root.join(&id);
                checked_directory(&directory, false)?;
                let project = read_project(&directory, &id)?;
                Ok::<_, ProjectError>(ProjectSummary {
                    card_count: project.document.graph.nodes.len(),
                    asset_count: project.assets.len(),
                    metadata: project.metadata,
                })
            })();
            match result {
                Ok(project) => listing.projects.push(project),
                Err(error) => listing.skipped.push(ProjectListWarning {
                    id,
                    message: error.to_string(),
                }),
            }
        }
        listing.projects.sort_by(|a, b| {
            b.metadata
                .updated_at_ms
                .cmp(&a.metadata.updated_at_ms)
                .then_with(|| a.metadata.name.cmp(&b.metadata.name))
                .then_with(|| a.metadata.id.cmp(&b.metadata.id))
        });
        Ok(listing)
    }

    /// A project-owned image for its home card. The UI decodes it asynchronously.
    /// This never follows an external document path or a symbolic built-in key.
    pub fn thumbnail_path(&self, id: &str) -> Result<Option<PathBuf>, ProjectError> {
        validate_id(id)?;
        let directory = self.root.join(id);
        let project = read_project(&directory, id)?;
        let hidden = gpui_flow_core::hidden_node_ids(&project.document.graph);
        for node in project
            .document
            .graph
            .nodes
            .values()
            .filter(|node| !hidden.contains(&node.id))
        {
            for key in [
                node.data["asset"].as_str(),
                crate::references::reference_asset(node),
            ]
            .into_iter()
            .flatten()
            {
                if let Some(asset) = project
                    .assets
                    .iter()
                    .find(|a| a.key == key && a.media_type.starts_with("image/"))
                {
                    let path = directory.join(&asset.key);
                    check_asset_file(&path, asset)?;
                    return Ok(Some(path));
                }
            }
        }
        Ok(None)
    }

    /// The first complete document is persisted before this returns. Names are
    /// display-only; folder names are generated and cannot escape the home.
    pub fn create_project(
        &self,
        name: impl AsRef<str>,
        document: &FlowDocument,
        registry: &CreativeRegistry,
    ) -> Result<LocalProject, ProjectError> {
        let _transaction = transaction()?;
        let name = name.as_ref().trim();
        validate_name(name)?;
        registry.validate_document(document)?;
        validate_document_assets(document, &[])?;
        checked_directory(&self.root, false)?;
        let id = unique_name("p");
        let directory = self.root.join(&id);
        fs::create_dir(&directory)?;
        let now = now_ms();
        let project = ProjectFile {
            schema_version: PROJECT_SCHEMA_VERSION,
            revision: 0,
            metadata: ProjectMetadata {
                id,
                name: name.to_owned(),
                created_at_ms: now,
                updated_at_ms: now,
            },
            document: document.clone(),
            assets: Vec::new(),
        };
        let result = (|| {
            fs::create_dir(directory.join("assets"))?;
            write_project(&directory, &project)?;
            #[cfg(unix)]
            if let Ok(home) = File::open(&self.root) {
                let _ = home.sync_all();
            }
            Ok(LocalProject {
                directory: directory.clone(),
                project,
                migrated_reference_count: 0,
                migrated_orientation_count: 0,
            })
        })();
        if result.is_err() {
            // Only our just-created, empty directories; never recursively delete.
            let _ = fs::remove_dir(directory.join("assets"));
            let _ = fs::remove_dir(&directory);
        }
        result
    }

    pub fn open_project(
        &self,
        id: &str,
        registry: &CreativeRegistry,
    ) -> Result<LocalProject, ProjectError> {
        let _transaction = transaction()?;
        validate_id(id)?;
        checked_directory(&self.root, false)?;
        let directory = self.root.join(id);
        let mut project = read_project(&directory, id)?;
        check_assets_on_disk(&directory, &project.assets)?;
        let changed = image_metadata::reconcile_assets(&directory, &mut project, || Ok(()))?;
        let mut local = LocalProject {
            directory,
            project,
            migrated_reference_count: 0,
            migrated_orientation_count: changed.len(),
        };
        let has_references = local
            .project
            .document
            .graph
            .nodes
            .values()
            .any(crate::references::has_legacy_reference);
        let mut candidate = local.project.clone();
        if !changed.is_empty() {
            orientation_migration::guard_legacy_orientation(
                &mut candidate.document.graph,
                &changed,
            )?;
        }
        if has_references {
            let (document, converted) = crate::references::migrate_legacy_references(
                registry,
                &candidate.document,
                &candidate.assets,
            )?;
            candidate.document = document;
            local.migrated_reference_count = converted;
        }
        registry.validate_document(&candidate.document)?;
        validate_document_assets(&candidate.document, &candidate.assets)?;
        if !changed.is_empty() || has_references {
            let revision = local.next_revision()?;
            candidate.revision = revision.revision;
            candidate.metadata = revision.metadata;
            candidate.schema_version = PROJECT_SCHEMA_VERSION;
            local.check_revision()?;
            write_project(&local.directory, &candidate)?;
        }
        // A v1 project without oriented images upgrades on its next actual save.
        candidate.schema_version = PROJECT_SCHEMA_VERSION;
        local.project = candidate;
        Ok(local)
    }

    /// Read an immutable preview source without asset conversion or migration.
    /// The preview renderer admits only the assets it actually needs. Legacy
    /// projects requiring migration must be explicitly opened by the user.
    #[cfg(all(feature = "desktop", test))]
    pub(crate) fn read_preview_project(
        &self,
        id: &str,
        registry: &CreativeRegistry,
    ) -> Result<LocalProject, ProjectError> {
        self.read_preview_project_cancellable(
            id,
            registry,
            &std::sync::atomic::AtomicBool::new(false),
        )
    }

    #[cfg(feature = "desktop")]
    pub(crate) fn read_preview_project_cancellable(
        &self,
        id: &str,
        registry: &CreativeRegistry,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<LocalProject, ProjectError> {
        validate_id(id)?;
        checked_directory(&self.root, false)?;
        let directory = self.root.join(id);
        let mut project = read_project(&directory, id)?;
        registry.validate_document(&project.document)?;
        image_metadata::reconcile_preview(&directory, &mut project, cancelled)?;
        Ok(LocalProject {
            directory,
            project,
            migrated_reference_count: 0,
            migrated_orientation_count: 0,
        })
    }
}

#[derive(Debug, Clone)]
pub struct LocalProject {
    directory: PathBuf,
    project: ProjectFile,
    migrated_reference_count: usize,
    migrated_orientation_count: usize,
}

impl LocalProject {
    pub fn metadata(&self) -> &ProjectMetadata {
        &self.project.metadata
    }
    pub fn document(&self) -> &FlowDocument {
        &self.project.document
    }
    pub fn assets(&self) -> &[ProjectAsset] {
        &self.project.assets
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn revision(&self) -> u64 {
        self.project.revision
    }

    /// Legacy attachment bindings converted to visible image inputs by this
    /// open. Reopening an already converted project returns zero.
    pub fn migrated_reference_count(&self) -> usize {
        self.migrated_reference_count
    }

    /// Images whose legacy encoded orientation was reconciled by this open.
    pub fn migrated_orientation_count(&self) -> usize {
        self.migrated_orientation_count
    }

    /// Validate first, then fsync a unique sibling and atomically replace
    /// `project.json`. Any returned error leaves the previous document and this
    /// handle untouched. No delete-then-rename fallback is used.
    pub fn save_document(
        &mut self,
        document: &FlowDocument,
        registry: &CreativeRegistry,
    ) -> Result<(), ProjectError> {
        self.save_named_document(document, registry, None)
    }

    /// Commit a rename and the latest canvas in the same project revision.
    pub fn save_named_document(
        &mut self,
        document: &FlowDocument,
        registry: &CreativeRegistry,
        name: Option<&str>,
    ) -> Result<(), ProjectError> {
        let _transaction = transaction()?;
        if let Some(name) = name {
            validate_name(name)?;
        }
        registry.validate_document(document)?;
        validate_document_assets(document, &self.project.assets)?;
        self.check_revision()?;
        check_assets_on_disk(&self.directory, &self.project.assets)?;
        let mut candidate = self.next_revision()?;
        candidate.document = document.clone();
        if let Some(name) = name {
            candidate.metadata.name = name.into();
        }
        write_project(&self.directory, &candidate)?;
        self.project = candidate;
        Ok(())
    }

    /// Validate a candidate before replacing the live canvas. This has no writes.
    pub fn validate_document(
        &self,
        document: &FlowDocument,
        registry: &CreativeRegistry,
    ) -> Result<(), ProjectError> {
        registry.validate_document(document)?;
        validate_document_assets(document, &self.project.assets)
    }

    /// Copy a user-selected local raster image into this project's assets. The
    /// original path is never persisted. A failed import never adds a manifest
    /// entry; successful imports can be resolved immediately and survive restart.
    /// Supported containers: PNG, JPEG, GIF, WebP. The renderer still decodes the
    /// image and must report malformed compressed pixel data normally.
    pub fn import_image(&mut self, source: impl AsRef<Path>) -> Result<ProjectAsset, ProjectError> {
        self.import_asset(source.as_ref(), AssetKind::Image)
    }

    /// Copy a user-selected, self-contained GLB 2.0 into this project's assets.
    /// The bounded CPU-preview parser validates geometry before anything is
    /// written, rejects external/data URIs, and never reads referenced files.
    /// Models have media type `model/gltf-binary` and zero width/height.
    pub fn import_model(&mut self, source: impl AsRef<Path>) -> Result<ProjectAsset, ProjectError> {
        self.import_asset(source.as_ref(), AssetKind::Model)
    }

    fn import_asset(
        &mut self,
        source: &Path,
        kind: AssetKind,
    ) -> Result<ProjectAsset, ProjectError> {
        let _transaction = transaction()?;
        self.check_revision()?;
        if self.project.assets.len() >= MAX_PROJECT_ASSETS {
            return Err(ProjectError::TooLarge(
                tr(
                    "项目素材数量已达上限（512）",
                    "project has reached its 512 asset limit",
                )
                .into(),
            ));
        }
        let source = absolute_path(source)?;
        let bytes = read_regular(&source, kind.max_bytes())?;
        let format = kind.inspect(&bytes)?;
        let total = self
            .project
            .assets
            .iter()
            .map(|asset| asset.byte_len)
            .sum::<u64>();
        if total.saturating_add(bytes.len() as u64) > MAX_PROJECT_ASSET_BYTES {
            return Err(ProjectError::TooLarge(
                tr(
                    "项目素材总大小超过 512 MiB",
                    "project assets exceed 512 MiB",
                )
                .into(),
            ));
        }
        let key = format!("assets/{}.{}", unique_name("a"), format.extension);
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(kind.default_name())
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect::<String>();
        let asset = ProjectAsset {
            key,
            name: if name.is_empty() {
                kind.default_name().into()
            } else {
                name
            },
            media_type: format.media_type.into(),
            byte_len: bytes.len() as u64,
            width: format.width,
            height: format.height,
        };
        checked_directory(&self.directory.join("assets"), false)?;
        let path = self.directory.join(&asset.key);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            // Persist the new directory entry before a committed manifest may
            // reference it. Failure here is still safely before the commit.
            #[cfg(unix)]
            File::open(self.directory.join("assets"))?.sync_all()?;
            let mut candidate = self.next_revision()?;
            candidate.assets.push(asset.clone());
            write_project(&self.directory, &candidate)?;
            self.project = candidate;
            Ok(asset)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&path);
        }
        result
    }

    /// Resolve only manifest-owned relative keys. Built-in symbolic resources
    /// belong to the host renderer and are deliberately not filesystem paths.
    /// Audio/video resolution probes the copied file; use a background worker.
    /// UI code can use `resolve_asset_path` and let its media player decode later.
    pub fn resolve_asset(&self, key: &str) -> Result<PathBuf, ProjectError> {
        let (path, asset) = self.asset_path(key)?;
        verify_asset(&path, asset)?;
        Ok(path)
    }

    /// Resolve a manifest-owned asset without parsing its contents. This checks
    /// path components, symlinks, regular-file status and the saved byte length.
    /// Hosts may use this on a UI thread and validate/decode the file later on a
    /// worker; success alone does not establish that the bytes are valid media.
    pub fn resolve_asset_path(&self, key: &str) -> Result<PathBuf, ProjectError> {
        self.asset_path(key).map(|(path, _)| path)
    }

    fn asset_path(&self, key: &str) -> Result<(PathBuf, &ProjectAsset), ProjectError> {
        validate_asset_key(key)?;
        let asset = self
            .project
            .assets
            .iter()
            .find(|asset| asset.key == key)
            .ok_or_else(|| {
                ProjectError::Invalid(
                    tr(
                        "当前项目的素材清单中没有此素材",
                        "asset is not in this project's manifest",
                    )
                    .into(),
                )
            })?;
        let path = self.directory.join(key);
        check_asset_file(&path, asset)?;
        Ok((path, asset))
    }

    fn check_revision(&self) -> Result<(), ProjectError> {
        let current = read_project(&self.directory, &self.project.metadata.id)?;
        if current.revision != self.project.revision
            || current.metadata.created_at_ms != self.project.metadata.created_at_ms
        {
            return Err(ProjectError::Conflict);
        }
        Ok(())
    }

    fn next_revision(&self) -> Result<ProjectFile, ProjectError> {
        let mut candidate = self.project.clone();
        candidate.revision = candidate.revision.checked_add(1).ok_or_else(|| {
            ProjectError::Invalid(tr("项目修订号超出范围", "project revision overflow").into())
        })?;
        candidate.schema_version = PROJECT_SCHEMA_VERSION;
        candidate.metadata.updated_at_ms = now_ms().max(candidate.metadata.updated_at_ms);
        Ok(candidate)
    }
}

fn transaction() -> Result<std::sync::MutexGuard<'static, ()>, ProjectError> {
    TRANSACTION.lock().map_err(|_| {
        ProjectError::Invalid(tr("项目存储锁状态异常", "project storage lock was poisoned").into())
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub(crate) fn unique_name(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{prefix}-{nanos:x}-{:x}-{:x}",
        std::process::id(),
        NEXT_NAME.fetch_add(1, Ordering::Relaxed)
    )
}

fn validate_id(id: &str) -> Result<(), ProjectError> {
    if id.is_empty()
        || id.len() > 100
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(ProjectError::Invalid(
            tr("项目 ID 无效", "invalid project ID").into(),
        ));
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), ProjectError> {
    if name.trim().is_empty() || name.len() > 512 || name.chars().any(char::is_control) {
        return Err(ProjectError::Invalid(
            tr(
                "项目名称必须包含 1–512 字节的可打印文本",
                "project name must contain 1–512 bytes of printable text",
            )
            .into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_asset_key(key: &str) -> Result<(), ProjectError> {
    let valid = key.strip_prefix("assets/").is_some_and(|name| {
        name.len() <= 128
            && name.rsplit_once('.').is_some_and(|(stem, ext)| {
                !stem.is_empty()
                    && stem
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                    && matches!(
                        ext,
                        "png"
                            | "jpg"
                            | "gif"
                            | "webp"
                            | "glb"
                            | "mp4"
                            | "webm"
                            | "wav"
                            | "mp3"
                            | "flac"
                            | "ogg"
                            | "m4a"
                    )
            })
    });
    if !valid {
        return Err(ProjectError::Invalid(
            tr(
                "相对于项目目录的素材键无效",
                "invalid project-relative asset key",
            )
            .into(),
        ));
    }
    Ok(())
}

fn validate_document_assets(
    document: &FlowDocument,
    assets: &[ProjectAsset],
) -> Result<(), ProjectError> {
    for node in document.graph.nodes.values() {
        crate::custom_host::validate_custom_node_assets(node, assets)?;
        if node.kind == "image" {
            for version in crate::image_versions::entries(node).map_err(ProjectError::Invalid)? {
                if assets
                    .iter()
                    .find(|asset| asset.key == version.asset_key)
                    .is_none_or(|asset| {
                        !matches!(asset.media_type.as_str(), "image/png" | "image/jpeg")
                    })
                {
                    return Err(ProjectError::Invalid(
                        tr(
                            "图片版本素材不属于本项目静态图片",
                            "The image version asset is not a still image in this project",
                        )
                        .into(),
                    ));
                }
            }
        }
        if node.kind == "mask" {
            let data = crate::MaskNodeData::from_value(&node.data)?;
            if let Some(source) = &data.state.source {
                let original = assets
                    .iter()
                    .find(|asset| asset.key == source.asset_key)
                    .ok_or_else(|| {
                        ProjectError::Invalid(
                            tr(
                                "Mask 原图不属于当前项目",
                                "The mask source image does not belong to this project",
                            )
                            .into(),
                        )
                    })?;
                if !matches!(original.media_type.as_str(), "image/png" | "image/jpeg")
                    || (!crate::mask_node::orientation_review_required(&node.data)?
                        && (original.width != source.width || original.height != source.height))
                {
                    return Err(ProjectError::Invalid(
                        tr(
                            "Mask 原图尺寸或类型不匹配",
                            "The mask source image dimensions or type do not match",
                        )
                        .into(),
                    ));
                }
                if !data.asset.is_empty() {
                    let output = assets
                        .iter()
                        .find(|asset| asset.key == data.asset)
                        .ok_or_else(|| {
                            ProjectError::Invalid(
                                tr("Mask 输出素材不存在", "The mask output asset is missing")
                                    .into(),
                            )
                        })?;
                    if output.media_type != crate::MASK_MEDIA_TYPE
                        || output.width != source.width
                        || output.height != source.height
                    {
                        return Err(ProjectError::Invalid(
                            tr(
                                "Mask 输出与原图尺寸不匹配",
                                "The mask output dimensions do not match the source image",
                            )
                            .into(),
                        ));
                    }
                }
            }
        }
        if let Some(kind) = crate::processing::ProcessingKind::from_node_kind(&node.kind) {
            let data = crate::processing_node::ProcessingNodeData::from_value(&node.data)
                .map_err(|e| ProjectError::Invalid(e.to_string()))?;
            if let Some(key) = &data.source_asset {
                let source = assets
                    .iter()
                    .find(|asset| &asset.key == key)
                    .ok_or_else(|| {
                        ProjectError::Invalid(
                            tr(
                                "处理输出缺少原素材",
                                "The processing output is missing its source asset",
                            )
                            .into(),
                        )
                    })?;
                let matches = match kind {
                    crate::processing::ProcessingKind::Image => {
                        matches!(source.media_type.as_str(), "image/png" | "image/jpeg")
                    }
                    crate::processing::ProcessingKind::Video => {
                        source.media_type.starts_with("video/")
                    }
                    crate::processing::ProcessingKind::Audio => {
                        source.media_type.starts_with("audio/")
                    }
                };
                if !matches {
                    return Err(ProjectError::Invalid(
                        tr(
                            "处理输出的原素材类型不匹配",
                            "The processing output's source asset type does not match",
                        )
                        .into(),
                    ));
                }
            }
            if !data.asset.is_empty()
                && assets
                    .iter()
                    .find(|asset| asset.key == data.asset)
                    .is_none_or(|asset| asset.media_type != data.settings.media_type())
            {
                return Err(ProjectError::Invalid(
                    tr(
                        "处理输出格式与参数不匹配",
                        "The processing output format does not match its settings",
                    )
                    .into(),
                ));
            }
        }
        // Only the legacy envelope reader admits this key. Public document
        // validation and every write reject it until migration has removed it.
        if crate::references::has_legacy_reference(node)
            && let Some(value) = node
                .data
                .get("settings")
                .and_then(|settings| settings.get("reference_asset"))
        {
            let key = value.as_str().ok_or_else(|| {
                ProjectError::Invalid(
                    tr(
                        "旧版参考素材必须是字符串",
                        "legacy reference asset must be a string",
                    )
                    .into(),
                )
            })?;
            if !key.is_empty() {
                if !matches!(
                    crate::GenerationKind::from_node_kind(&node.kind),
                    Some(
                        crate::GenerationKind::Image
                            | crate::GenerationKind::Video
                            | crate::GenerationKind::Model
                    )
                ) || !crate::references::accepts_image_reference(node)
                {
                    return Err(ProjectError::Invalid(
                        tr(
                            "此节点不接受旧版参考图片",
                            "this node does not accept a legacy image reference",
                        )
                        .into(),
                    ));
                }
                validate_asset_key(key)?;
                let asset = assets
                    .iter()
                    .find(|asset| asset.key == key)
                    .ok_or_else(|| {
                        ProjectError::Invalid(tr_format!(
                            "缺少旧版参考素材：{key}",
                            "missing legacy reference asset: {key}"
                        ))
                    })?;
                if !asset.media_type.starts_with("image/") {
                    return Err(ProjectError::Invalid(
                        tr(
                            "旧版参考图片必须属于当前项目",
                            "legacy reference attachments must be project-owned images",
                        )
                        .into(),
                    ));
                }
            }
        }
        let Some(value) = node.data.get("asset") else {
            continue;
        };
        let key = value.as_str().ok_or_else(|| {
            ProjectError::Invalid(tr("素材键必须是字符串", "asset key must be a string").into())
        })?;
        // Empty and simple symbolic keys are host-owned built-ins/placeholders.
        // They can never be passed to filesystem resolution by this module.
        if key.len() <= 128
            && key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            continue;
        }
        validate_asset_key(key)?;
        let asset = assets
            .iter()
            .find(|asset| asset.key == key)
            .ok_or_else(|| {
                ProjectError::Invalid(tr_format!(
                    "缺少项目素材：{key}",
                    "missing project asset: {key}"
                ))
            })?;
        // Model output has a strict contract. Existing image/compose cards and
        // host-defined image kinds retain their compatibility; do not invent an
        // allowlist for unknown host kinds. Raster posters for existing video
        // or other host cards remain valid; only model cards require real GLB.
        let compatible = match AssetKind::from_media_type(&asset.media_type)? {
            AssetKind::Mask => node.kind == "mask",
            AssetKind::Model => matches!(node.kind.as_str(), "model" | "generate_model"),
            AssetKind::Video => matches!(
                node.kind.as_str(),
                "video" | "generate_video" | "process_video"
            ),
            AssetKind::Audio => matches!(
                node.kind.as_str(),
                "audio" | "generate_audio" | "process_audio"
            ),
            AssetKind::Image => !matches!(node.kind.as_str(), "model" | "generate_model" | "mask"),
        };
        if !compatible {
            return Err(ProjectError::Invalid(tr_format!(
                "素材类型 {} 与节点类型 {} 不兼容",
                "asset type {} is incompatible with node kind {}",
                asset.media_type,
                node.kind
            )));
        }
    }
    Ok(())
}

fn validate_project(project: &ProjectFile, id: &str) -> Result<(), ProjectError> {
    if !matches!(project.schema_version, 1 | PROJECT_SCHEMA_VERSION) {
        return Err(ProjectError::Invalid(tr_format!(
            "不支持的项目格式版本：{}",
            "unsupported project schema version: {}",
            project.schema_version
        )));
    }
    validate_id(&project.metadata.id)?;
    if project.metadata.id != id {
        return Err(ProjectError::Invalid(
            tr(
                "项目 ID 与其文件夹不匹配",
                "project ID does not match its directory",
            )
            .into(),
        ));
    }
    validate_name(&project.metadata.name)?;
    if project.metadata.updated_at_ms < project.metadata.created_at_ms {
        return Err(ProjectError::Invalid(
            tr("项目时间戳无效", "invalid project timestamps").into(),
        ));
    }
    if project.document.graph.nodes.len() > MAX_CARDS
        || project.document.graph.edges.len() > MAX_LINKS
        || project.document.graph.groups.len() > MAX_CARDS
    {
        return Err(ProjectError::TooLarge(
            tr("画布对象数量超过上限", "creative board size limit exceeded").into(),
        ));
    }
    project
        .document
        .validate()
        .map_err(|error| ProjectError::Invalid(error.to_string()))?;
    if serde_json::to_vec(&project.document)?.len() > MAX_JSON_BYTES {
        return Err(ProjectError::TooLarge(
            tr("画布文档超过 32 MiB", "creative document exceeds 32 MiB").into(),
        ));
    }
    if project.assets.len() > MAX_PROJECT_ASSETS {
        return Err(ProjectError::TooLarge(
            tr("项目素材数量超过 512", "project has more than 512 assets").into(),
        ));
    }
    let mut keys = BTreeSet::new();
    let mut total = 0u64;
    for asset in &project.assets {
        validate_asset_key(&asset.key)?;
        if !keys.insert(&asset.key) {
            return Err(ProjectError::Invalid(
                tr("项目素材键重复", "duplicate project asset key").into(),
            ));
        }
        if asset.name.is_empty()
            || asset.name.len() > 1024
            || asset.name.chars().any(char::is_control)
        {
            return Err(ProjectError::Invalid(
                tr("素材显示名称无效", "invalid asset display name").into(),
            ));
        }
        let expected_type = match asset.key.rsplit('.').next() {
            Some("png") if asset.media_type == crate::MASK_MEDIA_TYPE => crate::MASK_MEDIA_TYPE,
            Some("png") => "image/png",
            Some("jpg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            Some("glb") => "model/gltf-binary",
            Some("mp4") => "video/mp4",
            Some("webm") => "video/webm",
            Some("wav") => "audio/wav",
            Some("mp3") => "audio/mpeg",
            Some("flac") => "audio/flac",
            Some("ogg") => "audio/ogg",
            Some("m4a") => "audio/mp4",
            _ => {
                return Err(ProjectError::Invalid(
                    tr("不支持的素材文件扩展名", "unsupported asset extension").into(),
                ));
            }
        };
        let kind = AssetKind::from_media_type(&asset.media_type)?;
        if asset.media_type != expected_type
            || asset.byte_len == 0
            || asset.byte_len > kind.max_bytes()
        {
            return Err(ProjectError::Invalid(
                tr("素材类型或大小无效", "invalid asset type or size").into(),
            ));
        }
        match kind {
            AssetKind::Image | AssetKind::Mask | AssetKind::Video => {
                validate_dimensions(asset.width, asset.height)?
            }
            AssetKind::Audio if asset.width != 0 || asset.height != 0 => {
                return Err(ProjectError::Invalid(
                    tr(
                        "音频素材的宽高必须均为零",
                        "audio assets must have zero dimensions",
                    )
                    .into(),
                ));
            }
            AssetKind::Audio => {}
            AssetKind::Model if asset.width != 0 || asset.height != 0 => {
                return Err(ProjectError::Invalid(
                    tr(
                        "模型素材的宽高必须均为零",
                        "model assets must have zero width and height",
                    )
                    .into(),
                ));
            }
            AssetKind::Model => {}
        }
        total = total.saturating_add(asset.byte_len);
    }
    if total > MAX_PROJECT_ASSET_BYTES {
        return Err(ProjectError::TooLarge(
            tr(
                "项目素材总大小超过 512 MiB",
                "project assets exceed 512 MiB",
            )
            .into(),
        ));
    }
    validate_document_assets(&project.document, &project.assets)
}

fn read_project(directory: &Path, id: &str) -> Result<ProjectFile, ProjectError> {
    checked_directory(directory, false)?;
    let bytes = read_regular(&directory.join(PROJECT_FILE), MAX_PROJECT_BYTES)?;
    let project = serde_json::from_slice(&bytes)?;
    validate_project(&project, id)?;
    Ok(project)
}

fn write_project(directory: &Path, project: &ProjectFile) -> Result<(), ProjectError> {
    for node in project.document.graph.nodes.values() {
        crate::references::reject_legacy_reference(node)?;
    }
    validate_project(project, &project.metadata.id)?;
    let bytes = serde_json::to_vec_pretty(project)?;
    if bytes.len() as u64 > MAX_PROJECT_BYTES {
        return Err(ProjectError::TooLarge(
            tr("项目 JSON 超过 34 MiB", "project JSON exceeds 34 MiB").into(),
        ));
    }
    checked_directory(directory, false)?;
    let target = directory.join(PROJECT_FILE);
    match fs::symlink_metadata(&target) {
        Ok(metadata) if !metadata.is_file() || is_link(&metadata) => {
            return Err(ProjectError::Invalid(
                tr(
                    "project.json 必须是常规文件",
                    "project.json must be a regular file",
                )
                .into(),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let temp = directory.join(format!(".{}.tmp", unique_name("save")));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        checked_directory(directory, false)?;
        fs::rename(&temp, &target)?;
        // The commit already happened. A directory-sync failure must not be
        // reported as a failed save: that would incorrectly imply old contents.
        #[cfg(unix)]
        if let Ok(directory_file) = File::open(directory) {
            let _ = directory_file.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn absolute_path(path: &Path) -> Result<PathBuf, ProjectError> {
    if path.as_os_str().is_empty() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(ProjectError::Invalid(
            tr(
                "路径不能为空，也不能指向父目录",
                "empty paths and parent traversal are not allowed",
            )
            .into(),
        ));
    }
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    // Remove harmless `.` while preserving drive/root components.
    Ok(path
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect())
}

fn checked_directory(path: &Path, create: bool) -> Result<(), ProjectError> {
    let mut current = PathBuf::new();
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        if matches!(component, Component::ParentDir) {
            return Err(ProjectError::Invalid(
                tr("路径不能指向父目录", "parent traversal is not allowed").into(),
            ));
        }
        current.push(component.as_os_str());
        // A volume prefix on its own is not a directory. Check it once the
        // root separator is present, which is required for Windows verbatim paths.
        if matches!(component, Component::Prefix(_))
            && matches!(components.peek(), Some(Component::RootDir))
        {
            continue;
        }
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
                fs::symlink_metadata(&current)?
            }
            Err(error) => return Err(error.into()),
        };
        if is_link(&metadata) || !metadata.is_dir() {
            return Err(ProjectError::Invalid(
                tr(
                    "项目路径只能由常规目录组成，不能包含符号链接",
                    "project paths cannot contain symlinks or non-directories",
                )
                .into(),
            ));
        }
    }
    Ok(())
}

fn regular_metadata(path: &Path) -> Result<fs::Metadata, ProjectError> {
    let parent = path.parent().ok_or_else(|| {
        ProjectError::Invalid(tr("文件没有所在目录", "file has no parent").into())
    })?;
    checked_directory(parent, false)?;
    let metadata = fs::symlink_metadata(path)?;
    if is_link(&metadata) || !metadata.is_file() {
        return Err(ProjectError::Invalid(
            tr(
                "请选择常规文件，不能使用符号链接",
                "expected a regular file, not a symlink",
            )
            .into(),
        ));
    }
    Ok(metadata)
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Reject junctions and other reparse points as well as ordinary symlinks.
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>, ProjectError> {
    let before = regular_metadata(path)?;
    if before.len() > limit {
        return Err(ProjectError::TooLarge(
            tr("文件超过允许大小", "file exceeds its allowed size").into(),
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    // O_NOFOLLOW closes the final-component symlink race on supported Unix hosts.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x20000);
    }
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x100);
    }
    let file = options.open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() > limit {
        return Err(ProjectError::Invalid(
            tr(
                "文件已更改或超过允许大小",
                "file changed or exceeds its allowed size",
            )
            .into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != opened.dev() || before.ino() != opened.ino() {
            return Err(ProjectError::Invalid(
                tr("文件在打开过程中已更改", "file changed while being opened").into(),
            ));
        }
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(ProjectError::TooLarge(
            tr("文件超过允许大小", "file exceeds its allowed size").into(),
        ));
    }
    regular_metadata(path)?;
    Ok(bytes)
}

fn check_asset_file(path: &Path, asset: &ProjectAsset) -> Result<(), ProjectError> {
    let metadata = regular_metadata(path)?;
    if metadata.len() != asset.byte_len {
        return Err(ProjectError::Invalid(tr_format!(
            "素材大小已更改：{}",
            "asset size changed: {}",
            asset.key
        )));
    }
    Ok(())
}

fn verify_asset(path: &Path, asset: &ProjectAsset) -> Result<(), ProjectError> {
    let kind = AssetKind::from_media_type(&asset.media_type)?;
    let bytes = read_regular(path, kind.max_bytes())?;
    let mut info = if matches!(kind, AssetKind::Model) {
        // Existing GLBs were admitted by the original geometry-only contract.
        // New texture preview limits must not prevent opening their projects.
        crate::model_preview::validate_stored_glb(&bytes).map_err(|error| match error {
            PreviewError::BudgetExceeded(_) => ProjectError::TooLarge(error.to_string()),
            _ => ProjectError::Invalid(error.to_string()),
        })?;
        AssetInfo {
            extension: "glb",
            media_type: "model/gltf-binary",
            width: 0,
            height: 0,
        }
    } else {
        kind.inspect(&bytes)?
    };
    if matches!(kind, AssetKind::Video | AssetKind::Audio) {
        media_assets::verify_media(path, &mut info)?;
    }
    if bytes.len() as u64 != asset.byte_len
        || info.width != asset.width
        || info.height != asset.height
        || info.media_type != asset.media_type
    {
        return Err(ProjectError::Invalid(tr_format!(
            "素材与项目清单不匹配：{}",
            "asset does not match its manifest: {}",
            asset.key
        )));
    }
    Ok(())
}

fn check_assets_on_disk(directory: &Path, assets: &[ProjectAsset]) -> Result<(), ProjectError> {
    checked_directory(&directory.join("assets"), false)?;
    for asset in assets {
        check_asset_file(&directory.join(&asset.key), asset)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum AssetKind {
    Image,
    Mask,
    Model,
    Video,
    Audio,
}

impl AssetKind {
    fn from_media_type(media_type: &str) -> Result<Self, ProjectError> {
        match media_type {
            crate::MASK_MEDIA_TYPE => Ok(Self::Mask),
            "image/png" | "image/jpeg" | "image/gif" | "image/webp" => Ok(Self::Image),
            "model/gltf-binary" => Ok(Self::Model),
            "video/mp4" | "video/webm" => Ok(Self::Video),
            "audio/wav" | "audio/mpeg" | "audio/flac" | "audio/ogg" | "audio/mp4" => {
                Ok(Self::Audio)
            }
            _ => Err(ProjectError::Invalid(
                tr("不支持的素材媒体类型", "unsupported asset media type").into(),
            )),
        }
    }

    fn max_bytes(self) -> u64 {
        match self {
            Self::Image | Self::Mask => MAX_IMAGE_BYTES,
            Self::Model => MAX_GLB_BYTES as u64,
            Self::Video | Self::Audio => MAX_MEDIA_BYTES,
        }
    }

    fn default_name(self) -> &'static str {
        match self {
            Self::Image => "Imported image",
            Self::Mask => "Mask",
            Self::Model => "Imported model",
            Self::Video => "Imported video",
            Self::Audio => "Imported audio",
        }
    }

    fn inspect(self, bytes: &[u8]) -> Result<AssetInfo, ProjectError> {
        match self {
            Self::Image => image_info(bytes),
            Self::Mask => mask_assets::mask_info(bytes),
            Self::Video | Self::Audio => media_assets::media_info(bytes, Some(self)),
            Self::Model => {
                parse_glb(bytes).map_err(|error| match error {
                    PreviewError::BudgetExceeded(_) => ProjectError::TooLarge(error.to_string()),
                    _ => ProjectError::Invalid(error.to_string()),
                })?;
                Ok(AssetInfo {
                    extension: "glb",
                    media_type: "model/gltf-binary",
                    width: 0,
                    height: 0,
                })
            }
        }
    }
}

struct AssetInfo {
    extension: &'static str,
    media_type: &'static str,
    width: u32,
    height: u32,
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), ProjectError> {
    if width == 0
        || height == 0
        || width > 32768
        || height > 32768
        || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
    {
        return Err(ProjectError::TooLarge(
            tr(
                "图片宽高必须大于零，每边最多 32768 像素，总计不超过 6400 万像素",
                "image dimensions must be nonzero, at most 32768 per side and 64 megapixels",
            )
            .into(),
        ));
    }
    Ok(())
}

fn image_info(bytes: &[u8]) -> Result<AssetInfo, ProjectError> {
    let mut info = encoded_image_info(bytes)?;
    let orientation = image_metadata::inspect(bytes, &info)?;
    (info.width, info.height) = orientation.oriented_dimensions;
    Ok(info)
}

fn encoded_image_info(bytes: &[u8]) -> Result<AssetInfo, ProjectError> {
    let invalid = || {
        ProjectError::Invalid(
            tr(
                "请选择有效的 PNG、JPEG、GIF 或 WebP 图片",
                "choose a valid PNG, JPEG, GIF, or WebP image",
            )
            .into(),
        )
    };
    let be32 = |slice: &[u8]| u32::from_be_bytes(slice.try_into().expect("four checked bytes"));
    let le24 = |slice: &[u8]| {
        u32::from(slice[0]) | (u32::from(slice[1]) << 8) | (u32::from(slice[2]) << 16)
    };
    let (extension, media_type, width, height) = if bytes.len() >= 33
        && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
    {
        if bytes[8..12] != [0, 0, 0, 13] || &bytes[12..16] != b"IHDR" {
            return Err(invalid());
        }
        (
            "png",
            "image/png",
            be32(&bytes[16..20]),
            be32(&bytes[20..24]),
        )
    } else if bytes.len() >= 13 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        (
            "gif",
            "image/gif",
            u32::from(u16::from_le_bytes([bytes[6], bytes[7]])),
            u32::from(u16::from_le_bytes([bytes[8], bytes[9]])),
        )
    } else if bytes.len() >= 4 && bytes.starts_with(&[0xff, 0xd8]) {
        let (width, height) = jpeg_dimensions(bytes).ok_or_else(invalid)?;
        ("jpg", "image/jpeg", width, height)
    } else if bytes.len() >= 25 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        let declared =
            u32::from_le_bytes(bytes[4..8].try_into().expect("four checked bytes")) as u64 + 8;
        if declared != bytes.len() as u64 {
            return Err(invalid());
        }
        let (width, height) = match &bytes[12..16] {
            b"VP8X" if bytes.len() >= 30 => (le24(&bytes[24..27]) + 1, le24(&bytes[27..30]) + 1),
            b"VP8 " if bytes.len() >= 30 && bytes[23..26] == [0x9d, 0x01, 0x2a] => (
                u32::from(u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3fff),
                u32::from(u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3fff),
            ),
            b"VP8L" if bytes[20] == 0x2f => (
                (u32::from(bytes[21]) | (u32::from(bytes[22] & 0x3f) << 8)) + 1,
                (u32::from(bytes[22] >> 6)
                    | (u32::from(bytes[23]) << 2)
                    | (u32::from(bytes[24] & 0xf) << 10))
                    + 1,
            ),
            _ => return Err(invalid()),
        };
        ("webp", "image/webp", width, height)
    } else {
        return Err(invalid());
    };
    validate_dimensions(width, height)?;
    Ok(AssetInfo {
        extension,
        media_type,
        width,
        height,
    })
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut cursor = 2;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] != 0xff {
            return None;
        }
        while bytes.get(cursor) == Some(&0xff) {
            cursor += 1;
        }
        let marker = *bytes.get(cursor)?;
        cursor += 1;
        if marker == 0xd9 || marker == 0xda {
            return None;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let length = usize::from(u16::from_be_bytes([
            *bytes.get(cursor)?,
            *bytes.get(cursor + 1)?,
        ]));
        if length < 2 || cursor.checked_add(length)? > bytes.len() {
            return None;
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if length < 8 {
                return None;
            }
            let height = u32::from(u16::from_be_bytes([bytes[cursor + 3], bytes[cursor + 4]]));
            let width = u32::from(u16::from_be_bytes([bytes[cursor + 5], bytes[cursor + 6]]));
            return Some((width, height));
        }
        cursor += length;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standard_registry;
    use gpui_flow_core::Point;
    use serde_json::json;

    struct TestHome(PathBuf);
    impl TestHome {
        fn new() -> Self {
            // Canonicalize only the test fixture's trusted temp base (e.g. macOS
            // /var -> /private/var). Project paths themselves never resolve links.
            let base = fs::canonicalize(std::env::temp_dir()).unwrap();
            let path = base.join(unique_name("flow-creative-test"));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn store(&self) -> ProjectStore {
            ProjectStore::new(self.0.join("projects")).unwrap()
        }
    }
    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(windows)]
    #[test]
    fn canonical_windows_project_home_checks_root_and_directory_components() {
        let home = TestHome::new();
        let store = home.store();
        assert_eq!(store.root(), home.0.join("projects"));
        assert!(store.root().is_dir());

        let file = home.0.join("file");
        fs::write(&file, b"not a directory").unwrap();
        assert!(matches!(
            ProjectStore::new(file.join("projects")),
            Err(ProjectError::Invalid(_))
        ));
    }

    fn png() -> Vec<u8> {
        // Complete 1x1 PNG fixture, not just a filename with an image extension.
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 11, 73, 68, 65, 84, 120, 156, 99, 96, 0, 2,
            0, 0, 5, 0, 1, 122, 94, 171, 63, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ]
    }

    fn model_json() -> serde_json::Value {
        json!({
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "mode": 4}]}],
            "buffers": [{"byteLength": 36}],
            "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}],
            "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3,
                "type": "VEC3", "min": [-1, -1, 0], "max": [1, 1, 0]}]
        })
    }

    fn glb_with_json(value: &serde_json::Value) -> Vec<u8> {
        // A complete GLB 2.0 with real FLOAT positions for one triangle. Tests
        // exercise persisted model bytes, not image headers or placeholder PNGs.
        let mut json = serde_json::to_vec(value).unwrap();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let positions: [f32; 9] = [-1., -1., 0., 1., -1., 0., 0., 1., 0.];
        let bin: Vec<u8> = positions.into_iter().flat_map(f32::to_le_bytes).collect();
        let mut bytes = b"glTF".to_vec();
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&((28 + json.len() + bin.len()) as u32).to_le_bytes());
        bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
        bytes.extend_from_slice(b"JSON");
        bytes.extend_from_slice(&json);
        bytes.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        bytes.extend_from_slice(b"BIN\0");
        bytes.extend_from_slice(&bin);
        bytes
    }

    fn glb() -> Vec<u8> {
        glb_with_json(&model_json())
    }

    #[test]
    fn create_save_open_and_list_preserve_flow_document() {
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let mut project = store
            .create_project("  我的项目  ", &FlowDocument::default(), &registry)
            .unwrap();
        assert_eq!(project.metadata().name, "我的项目");
        assert!(project.directory().join(PROJECT_FILE).is_file());
        assert!(project.directory().join("assets").is_dir());
        let mut document = project.document().clone();
        document.viewport.pan = Point::new(123., -45.);
        document
            .graph
            .add_node(
                registry
                    .make_node("text", "brief", Point::new(20., 30.), None)
                    .unwrap(),
            )
            .unwrap();
        project.save_document(&document, &registry).unwrap();
        let reopened = store
            .open_project(&project.metadata().id, &registry)
            .unwrap();
        assert_eq!(reopened.document(), &document);
        assert_eq!(reopened.revision(), 1);
        let listing = store.list_projects().unwrap();
        assert_eq!(listing.projects.len(), 1);
        assert_eq!(listing.projects[0].card_count, 1);
        assert!(listing.skipped.is_empty());
    }

    #[test]
    fn copied_asset_survives_source_deletion_and_reopen() {
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let source = home.0.join("reference.png");
        fs::write(&source, png()).unwrap();
        let mut project = store
            .create_project("Images", &FlowDocument::default(), &registry)
            .unwrap();
        let asset = project.import_image(&source).unwrap();
        assert_eq!((asset.width, asset.height), (1, 1));
        assert_eq!(
            fs::read(project.resolve_asset(&asset.key).unwrap()).unwrap(),
            png()
        );
        let mut document = project.document().clone();
        document
            .graph
            .add_node(
                registry
                    .make_node(
                        "image",
                        "reference",
                        Point::ZERO,
                        Some(json!({"asset": asset.key, "caption": "Local image"})),
                    )
                    .unwrap(),
            )
            .unwrap();
        project.save_document(&document, &registry).unwrap();
        fs::remove_file(source).unwrap();
        let reopened = store
            .open_project(&project.metadata().id, &registry)
            .unwrap();
        assert_eq!(
            fs::read(reopened.resolve_asset(&asset.key).unwrap()).unwrap(),
            png()
        );
        assert_eq!(reopened.document(), &document);
        assert_eq!(reopened.assets(), &[asset]);
    }

    /// Use the current importer for a real owned copy, then write the older
    /// envelope directly: production writes must no longer accept attachments.
    fn legacy_reference_project(home: &TestHome, kind: &str) -> (LocalProject, ProjectAsset) {
        let registry = standard_registry();
        let mut project = home
            .store()
            .create_project("Legacy references", &FlowDocument::default(), &registry)
            .unwrap();
        let source = home.0.join("reference.png");
        fs::write(&source, png()).unwrap();
        let asset = project.import_image(&source).unwrap();
        fs::remove_file(source).unwrap();
        let mut node = registry
            .make_node(kind, "gen", Point::new(800., 40.), None)
            .unwrap();
        node.data["prompt"] = json!("Create a variation");
        node.data["settings"]["reference_asset"] = json!(asset.key);
        project.project.document.graph.add_node(node).unwrap();
        write_legacy_fixture(&project);
        (project, asset)
    }

    fn write_legacy_fixture(project: &LocalProject) {
        fs::write(
            project.directory().join(PROJECT_FILE),
            serde_json::to_vec_pretty(&project.project).unwrap(),
        )
        .unwrap();
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn project_preview_read_is_read_only_and_never_migrates_legacy_data() {
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let project = store
            .create_project("Preview", &FlowDocument::default(), &registry)
            .unwrap();
        let path = project.directory().join(PROJECT_FILE);
        let bytes = fs::read(&path).unwrap();
        let snapshot = store
            .read_preview_project(&project.metadata().id, &registry)
            .unwrap();
        assert_eq!(snapshot.document(), project.document());
        assert_eq!(snapshot.revision(), project.revision());
        assert_eq!(fs::read(path).unwrap(), bytes);
        let (legacy, _) = legacy_reference_project(&home, "generate_image");
        let path = legacy.directory().join(PROJECT_FILE);
        let bytes = fs::read(&path).unwrap();
        assert!(
            store
                .read_preview_project(&legacy.metadata().id, &registry)
                .is_err()
        );
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn old_owned_reference_becomes_visible_image_input_and_reopen_is_idempotent() {
        let home = TestHome::new();
        let registry = standard_registry();
        let (project, asset) = legacy_reference_project(&home, "generate_image");
        let original_asset = fs::read(project.directory().join(&asset.key)).unwrap();
        let reopened = home
            .store()
            .open_project(&project.metadata().id, &registry)
            .unwrap();
        let document = reopened.document();
        assert_eq!(reopened.migrated_reference_count(), 1);
        assert_eq!(reopened.revision(), project.revision() + 1);
        assert_eq!(document.graph.nodes.len(), 2);
        assert_eq!(document.graph.edges.len(), 1);
        let generator = document.graph.node(&"gen".into()).unwrap();
        let source = crate::references::linked_reference(&document.graph, &generator.id).unwrap();
        assert_eq!(source.kind, "image");
        assert_eq!(source.data["asset"], asset.key);
        assert!(source.bounds().max_x() < generator.position.x);
        assert!(!crate::references::has_legacy_reference(generator));
        let request = crate::GenerationRequest::from_node(generator, &document.graph).unwrap();
        assert_eq!(request.inputs.len(), 1);
        assert_eq!(request.inputs[0].port, "reference");
        assert_eq!(request.inputs[0].asset.as_deref(), Some(asset.key.as_str()));
        assert!(request.settings.get("reference_asset").is_none());
        assert_eq!(reopened.assets(), std::slice::from_ref(&asset));
        assert_eq!(
            fs::read(reopened.resolve_asset(&asset.key).unwrap()).unwrap(),
            original_asset
        );
        assert_eq!(
            home.store().thumbnail_path(&project.metadata().id).unwrap(),
            Some(project.directory().join(&asset.key))
        );
        let bytes = fs::read(project.directory().join(PROJECT_FILE)).unwrap();
        let again = home
            .store()
            .open_project(&project.metadata().id, &registry)
            .unwrap();
        assert_eq!(again.document(), reopened.document());
        assert_eq!(again.revision(), reopened.revision());
        assert_eq!(again.migrated_reference_count(), 0);
        assert_eq!(
            fs::read(project.directory().join(PROJECT_FILE)).unwrap(),
            bytes
        );
    }

    #[test]
    fn old_references_reuse_one_existing_image_for_distinct_generator_kinds() {
        let home = TestHome::new();
        let registry = standard_registry();
        let (mut project, asset) = legacy_reference_project(&home, "generate_video");
        let image = registry
            .make_node(
                "image",
                "existing",
                Point::new(12., 30.),
                Some(json!({"asset":asset.key,"caption":"Keep my image"})),
            )
            .unwrap();
        project
            .project
            .document
            .graph
            .add_node(image.clone())
            .unwrap();
        let mut model = registry
            .make_node("generate_model", "model-gen", Point::new(800., 500.), None)
            .unwrap();
        model.data["settings"]["reference_asset"] = json!(asset.key);
        project.project.document.graph.add_node(model).unwrap();
        write_legacy_fixture(&project);
        let reopened = home
            .store()
            .open_project(&project.metadata().id, &registry)
            .unwrap();
        assert_eq!(reopened.migrated_reference_count(), 2);
        assert_eq!(reopened.document().graph.nodes.len(), 3);
        assert_eq!(reopened.document().graph.node(&image.id), Some(&image));
        for target in ["gen", "model-gen"] {
            assert_eq!(
                crate::references::linked_reference(&reopened.document().graph, &target.into())
                    .unwrap()
                    .id,
                image.id
            );
        }
        assert_eq!(reopened.assets().len(), 1);
    }

    #[test]
    fn reference_conflict_preserves_old_project_and_owned_image_bytes() {
        let home = TestHome::new();
        let registry = standard_registry();
        let (mut project, asset) = legacy_reference_project(&home, "generate_image");
        project
            .project
            .document
            .graph
            .add_node(
                registry
                    .make_node(
                        "image",
                        "existing",
                        Point::ZERO,
                        Some(json!({"asset":asset.key,"caption":"Existing input"})),
                    )
                    .unwrap(),
            )
            .unwrap();
        project
            .project
            .document
            .graph
            .connect(
                "existing-link",
                gpui_flow_core::PortRef::new("existing", "image"),
                gpui_flow_core::PortRef::new("gen", "reference"),
            )
            .unwrap();
        write_legacy_fixture(&project);
        let bytes = fs::read(project.directory().join(PROJECT_FILE)).unwrap();
        let owned = fs::read(project.directory().join(&asset.key)).unwrap();
        let error = home
            .store()
            .open_project(&project.metadata().id, &registry)
            .unwrap_err();
        assert!(error.to_string().contains("冲突"));
        assert_eq!(
            fs::read(project.directory().join(PROJECT_FILE)).unwrap(),
            bytes
        );
        assert_eq!(
            fs::read(project.directory().join(&asset.key)).unwrap(),
            owned
        );
        assert_eq!(fs::read_dir(project.directory()).unwrap().count(), 2);
    }

    #[test]
    fn missing_reference_asset_or_manifest_entry_preserves_old_project_bytes() {
        for missing_file in [true, false] {
            let home = TestHome::new();
            let registry = standard_registry();
            let (mut project, asset) = legacy_reference_project(&home, "generate_image");
            if missing_file {
                fs::remove_file(project.directory().join(&asset.key)).unwrap();
            } else {
                project
                    .project
                    .document
                    .graph
                    .nodes
                    .get_mut(&"gen".into())
                    .unwrap()
                    .data["settings"]["reference_asset"] = json!("assets/missing.png");
                write_legacy_fixture(&project);
            }
            let bytes = fs::read(project.directory().join(PROJECT_FILE)).unwrap();
            let file_count = fs::read_dir(project.directory().join("assets"))
                .unwrap()
                .count();
            assert!(
                home.store()
                    .open_project(&project.metadata().id, &registry)
                    .is_err()
            );
            assert_eq!(
                fs::read(project.directory().join(PROJECT_FILE)).unwrap(),
                bytes
            );
            assert_eq!(
                fs::read_dir(project.directory().join("assets"))
                    .unwrap()
                    .count(),
                file_count
            );
            assert_eq!(fs::read_dir(project.directory()).unwrap().count(), 2);
        }
    }

    #[test]
    fn migration_validation_failure_keeps_a_full_board_and_its_assets_unchanged() {
        let home = TestHome::new();
        let registry = standard_registry();
        let (mut project, asset) = legacy_reference_project(&home, "generate_image");
        for index in 1..MAX_CARDS {
            project
                .project
                .document
                .graph
                .add_node(
                    registry
                        .make_node("text", format!("note-{index}"), Point::ZERO, None)
                        .unwrap(),
                )
                .unwrap();
        }
        write_legacy_fixture(&project);
        let bytes = fs::read(project.directory().join(PROJECT_FILE)).unwrap();
        let error = home
            .store()
            .open_project(&project.metadata().id, &registry)
            .unwrap_err();
        assert!(error.to_string().contains("size limit"));
        assert_eq!(
            fs::read(project.directory().join(PROJECT_FILE)).unwrap(),
            bytes
        );
        assert_eq!(
            fs::read(project.directory().join(&asset.key)).unwrap(),
            png()
        );
        assert_eq!(fs::read_dir(project.directory()).unwrap().count(), 2);
        assert_eq!(
            fs::read_dir(project.directory().join("assets"))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn a_manifest_owned_model_cannot_be_migrated_as_an_image_input() {
        let home = TestHome::new();
        let registry = standard_registry();
        let mut project = home
            .store()
            .create_project("Wrong reference type", &FlowDocument::default(), &registry)
            .unwrap();
        let source = home.0.join("model.glb");
        fs::write(&source, glb()).unwrap();
        let asset = project.import_model(&source).unwrap();
        let mut generator = registry
            .make_node("generate_image", "gen", Point::ZERO, None)
            .unwrap();
        generator.data["settings"]["reference_asset"] = json!(asset.key);
        project.project.document.graph.add_node(generator).unwrap();
        write_legacy_fixture(&project);
        let bytes = fs::read(project.directory().join(PROJECT_FILE)).unwrap();
        assert!(
            home.store()
                .open_project(&project.metadata().id, &registry)
                .is_err()
        );
        assert_eq!(
            fs::read(project.directory().join(PROJECT_FILE)).unwrap(),
            bytes
        );
        assert_eq!(
            fs::read(project.directory().join(&asset.key)).unwrap(),
            glb()
        );
    }

    #[test]
    fn unsupported_audio_reference_and_new_attachment_saves_are_rejected() {
        let home = TestHome::new();
        let registry = standard_registry();
        let (mut project, _) = legacy_reference_project(&home, "generate_audio");
        let bytes = fs::read(project.directory().join(PROJECT_FILE)).unwrap();
        assert!(
            home.store()
                .open_project(&project.metadata().id, &registry)
                .is_err()
        );
        assert_eq!(
            fs::read(project.directory().join(PROJECT_FILE)).unwrap(),
            bytes
        );
        let document = project.document().clone();
        assert!(project.save_document(&document, &registry).is_err());
        assert_eq!(
            fs::read(project.directory().join(PROJECT_FILE)).unwrap(),
            bytes
        );

        let other_home = TestHome::new();
        let (mut other, _) = legacy_reference_project(&other_home, "generate_image");
        let document = other.document().clone();
        let bytes = fs::read(other.directory().join(PROJECT_FILE)).unwrap();
        assert!(other.save_document(&document, &registry).is_err());
        assert!(other.validate_document(&document, &registry).is_err());
        assert_eq!(
            fs::read(other.directory().join(PROJECT_FILE)).unwrap(),
            bytes
        );
    }

    #[test]
    #[ignore = "requires host-installed ffprobe; validates real copied WAV and rejected import cleanup"]
    fn local_audio_is_copied_probed_and_reopened_without_original_path() {
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let mut project = store
            .create_project("Audio", &FlowDocument::default(), &registry)
            .unwrap();
        let source = home.0.join("voice.wav");
        let samples = 8000u32;
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend((36 + samples * 2).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(8000u32.to_le_bytes());
        wav.extend(16000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((samples * 2).to_le_bytes());
        wav.resize(44 + samples as usize * 2, 0);
        fs::write(&source, &wav).unwrap();
        let asset = project.import_media(&source).unwrap();
        assert_eq!(asset.media_type, "audio/wav");
        let mut document = project.document().clone();
        document
            .graph
            .add_node(
                registry
                    .make_node(
                        "audio",
                        "voice",
                        Point::ZERO,
                        Some(json!({"asset":asset.key,"caption":"Local audio"})),
                    )
                    .unwrap(),
            )
            .unwrap();
        project.save_document(&document, &registry).unwrap();
        fs::remove_file(source).unwrap();
        let reopened = store
            .open_project(&project.metadata().id, &registry)
            .unwrap();
        assert_eq!(
            fs::read(reopened.resolve_asset(&asset.key).unwrap()).unwrap(),
            wav
        );
        let previous = fs::read(project.directory().join(PROJECT_FILE)).unwrap();
        let asset_count = fs::read_dir(project.directory().join("assets"))
            .unwrap()
            .count();
        let corrupt = home.0.join("broken.wav");
        fs::write(&corrupt, b"RIFF\0\0\0\0WAVEbroken").unwrap();
        assert!(project.import_media(&corrupt).is_err());
        assert_eq!(
            fs::read(project.directory().join(PROJECT_FILE)).unwrap(),
            previous
        );
        assert_eq!(
            fs::read_dir(project.directory().join("assets"))
                .unwrap()
                .count(),
            asset_count
        );
        document.graph.nodes.insert(
            "voice".into(),
            registry
                .make_node(
                    "video",
                    "voice",
                    Point::ZERO,
                    Some(json!({"asset":asset.key,"caption":"wrong type"})),
                )
                .unwrap(),
        );
        assert!(registry.validate_document(&document).is_ok());
        assert!(project.validate_document(&document, &registry).is_err());
    }

    #[test]
    fn styled_text_roundtrips_and_invalid_style_preserves_saved_project() {
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let mut project = store
            .create_project("Typography", &FlowDocument::default(), &registry)
            .unwrap();
        let mut document = project.document().clone();
        let data = crate::edit_text_style(
            &json!({"text":"山野之间\nFind your quiet", "host":{"keep":true}}),
            |style| {
                style.font_size = 42.;
                style.bold = true;
                style.alignment = crate::TextAlignment::Center;
            },
        )
        .unwrap();
        document
            .graph
            .add_node(
                registry
                    .make_node("text", "title", Point::new(42., 64.), Some(data.clone()))
                    .unwrap(),
            )
            .unwrap();
        project.save_document(&document, &registry).unwrap();
        let reopened = store
            .open_project(&project.metadata().id, &registry)
            .unwrap();
        assert_eq!(
            reopened
                .document()
                .graph
                .node(&"title".into())
                .unwrap()
                .data,
            data
        );
        let saved = fs::read(project.directory().join(PROJECT_FILE)).unwrap();
        document.graph.nodes.get_mut(&"title".into()).unwrap().data["style"]["font_size"] =
            json!(1000);
        assert!(project.save_document(&document, &registry).is_err());
        assert_eq!(
            fs::read(project.directory().join(PROJECT_FILE)).unwrap(),
            saved
        );
    }

    #[test]
    fn rename_commits_latest_canvas_and_invalid_name_keeps_previous_revision() {
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let mut project = store
            .create_project("Before", &FlowDocument::default(), &registry)
            .unwrap();
        let mut doc = project.document().clone();
        doc.viewport.pan = Point::new(93., -17.);
        project
            .save_named_document(&doc, &registry, Some("After"))
            .unwrap();
        let reopened = store
            .open_project(&project.metadata().id, &registry)
            .unwrap();
        assert_eq!(reopened.metadata().name, "After");
        assert_eq!(reopened.document(), &doc);
        let bytes = fs::read(project.directory().join(PROJECT_FILE)).unwrap();
        let revision = project.revision();
        assert!(
            project
                .save_named_document(&doc, &registry, Some(""))
                .is_err()
        );
        assert_eq!(project.revision(), revision);
        assert_eq!(
            fs::read(project.directory().join(PROJECT_FILE)).unwrap(),
            bytes
        );
    }

    #[test]
    fn project_thumbnail_uses_referenced_copied_image_and_rejects_missing_asset() {
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let mut project = store
            .create_project("Cover", &FlowDocument::default(), &registry)
            .unwrap();
        let id = project.metadata().id.clone();
        assert_eq!(store.thumbnail_path(&id).unwrap(), None);
        let source = home.0.join("cover.png");
        fs::write(&source, png()).unwrap();
        let asset = project.import_image(&source).unwrap();
        assert_eq!(store.thumbnail_path(&id).unwrap(), None);
        let mut doc = project.document().clone();
        doc.graph
            .add_node(
                registry
                    .make_node(
                        "image",
                        "cover",
                        Point::ZERO,
                        Some(json!({"asset":asset.key,"caption":""})),
                    )
                    .unwrap(),
            )
            .unwrap();
        project.save_document(&doc, &registry).unwrap();
        fs::remove_file(source).unwrap();
        let copied = project.directory().join(&asset.key);
        assert_eq!(store.thumbnail_path(&id).unwrap(), Some(copied.clone()));
        gpui_flow_core::set_node_visible(&mut doc.graph, &"cover".into(), false).unwrap();
        project.save_document(&doc, &registry).unwrap();
        assert_eq!(store.thumbnail_path(&id).unwrap(), None);
        assert!(copied.exists());
        gpui_flow_core::set_node_visible(&mut doc.graph, &"cover".into(), true).unwrap();
        project.save_document(&doc, &registry).unwrap();
        assert_eq!(store.thumbnail_path(&id).unwrap(), Some(copied.clone()));
        fs::remove_file(copied).unwrap();
        assert!(store.thumbnail_path(&id).is_err());
    }

    #[test]
    fn image_versions_reopen_with_owned_assets_and_reject_foreign_history() {
        use crate::image_versions::{ImageVersionOperation, entries, record_edit, restore_version};
        use gpui_flow_core::Size;
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let mut project = store
            .create_project("Versions", &FlowDocument::default(), &registry)
            .unwrap();
        let source = home.0.join("source.png");
        fs::write(&source, png()).unwrap();
        let first = project.import_image(&source).unwrap();
        let next = project.import_image(&source).unwrap();
        let before = registry
            .make_node(
                "image",
                "image",
                Point::new(100., 100.),
                Some(json!({"asset":first.key,"caption":"kept"})),
            )
            .unwrap();
        let mut after = before.clone();
        after.data["asset"] = json!(next.key);
        after.size = Size::new(160., 120.);
        let edited = record_edit(&before, &after, ImageVersionOperation::Adjust).unwrap();
        let mut document = project.document().clone();
        document.graph.add_node(edited.clone()).unwrap();
        project.save_document(&document, &registry).unwrap();
        let mut reopened = store
            .open_project(&project.metadata().id, &registry)
            .unwrap();
        assert_eq!(reopened.document(), &document);
        for asset in [&first, &next] {
            assert_eq!(
                fs::read(reopened.resolve_asset(&asset.key).unwrap()).unwrap(),
                png()
            );
        }
        let history = entries(&edited).unwrap();
        assert_eq!(history.len(), 2);
        let restored = restore_version(&edited, &history[0]).unwrap();
        document.graph.nodes.insert(restored.id.clone(), restored);
        reopened.save_document(&document, &registry).unwrap();
        let saved = fs::read(reopened.directory().join(PROJECT_FILE)).unwrap();
        let node = document.graph.nodes.get_mut(&"image".into()).unwrap();
        node.data[crate::image_versions::IMAGE_VERSIONS_KEY]["entries"][1]["asset_key"] =
            json!("assets/foreign.png");
        assert!(reopened.save_document(&document, &registry).is_err());
        assert_eq!(
            fs::read(reopened.directory().join(PROJECT_FILE)).unwrap(),
            saved
        );
        assert_eq!(fs::read(source).unwrap(), png());
    }

    #[test]
    fn copied_glb_survives_source_deletion_and_reopen_as_real_geometry() {
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let source = home.0.join("triangle.glb");
        fs::write(&source, glb()).unwrap();
        let mut project = store
            .create_project("Models", &FlowDocument::default(), &registry)
            .unwrap();
        let asset = project.import_model(&source).unwrap();
        assert_eq!(asset.media_type, "model/gltf-binary");
        assert!(asset.key.ends_with(".glb"));
        assert_eq!((asset.width, asset.height), (0, 0));
        assert_eq!(project.revision(), 1);
        let mut document = project.document().clone();
        let mut node = registry
            .make_node("generate_model", "model", Point::ZERO, None)
            .unwrap();
        node.data["asset"] = json!(asset.key);
        document.graph.add_node(node).unwrap();
        project.save_document(&document, &registry).unwrap();
        fs::remove_file(source).unwrap();
        let reopened = store
            .open_project(&project.metadata().id, &registry)
            .unwrap();
        let bytes = fs::read(reopened.resolve_asset(&asset.key).unwrap()).unwrap();
        assert_eq!(bytes, glb());
        let mesh = parse_glb(&bytes).unwrap();
        assert_eq!((mesh.vertex_count(), mesh.triangle_count()), (3, 1));
        assert_eq!(reopened.revision(), 2);
        assert_eq!(reopened.document(), &document);
        assert_eq!(reopened.assets(), &[asset]);
    }

    #[test]
    fn image_and_model_assets_keep_node_contracts_and_host_image_compatibility() {
        let home = TestHome::new();
        let registry = standard_registry();
        let mut project = home
            .store()
            .create_project("Typed assets", &FlowDocument::default(), &registry)
            .unwrap();
        let image_path = home.0.join("image.png");
        let model_path = home.0.join("model.glb");
        fs::write(&image_path, png()).unwrap();
        fs::write(&model_path, glb()).unwrap();
        let image = project.import_image(&image_path).unwrap();
        let model = project.import_model(&model_path).unwrap();
        for kind in [
            "image",
            "compose",
            "generate_image",
            "generate_video",
            "generate_audio",
            "host_custom_image",
            "model",
            "generate_model",
        ] {
            for asset in [&image, &model] {
                let mut document = FlowDocument::default();
                let mut node = registry
                    .make_node("image", "asset", Point::ZERO, None)
                    .unwrap();
                node.kind = kind.into();
                node.data["asset"] = json!(asset.key);
                document.graph.add_node(node).unwrap();
                let model_kind = matches!(kind, "model" | "generate_model");
                assert_eq!(
                    validate_document_assets(&document, project.assets()).is_ok(),
                    model_kind == (asset.media_type == "model/gltf-binary"),
                    "kind {kind}, media type {}",
                    asset.media_type
                );
            }
        }
        let before = fs::read(project.directory().join(PROJECT_FILE)).unwrap();
        let mut document = FlowDocument::default();
        let mut node = registry
            .make_node("image", "wrong-type", Point::ZERO, None)
            .unwrap();
        node.data["asset"] = json!(model.key);
        document.graph.add_node(node).unwrap();
        assert!(project.save_document(&document, &registry).is_err());
        assert_eq!(
            before,
            fs::read(project.directory().join(PROJECT_FILE)).unwrap()
        );
    }

    #[test]
    fn model_import_rejects_external_uris_and_hostile_sizes_before_commit() {
        let home = TestHome::new();
        let registry = standard_registry();
        let mut project = home
            .store()
            .create_project("Bounded models", &FlowDocument::default(), &registry)
            .unwrap();
        let source = home.0.join("model.glb");
        let before = fs::read(project.directory().join(PROJECT_FILE)).unwrap();
        for uri in [
            "https://example.invalid/positions.bin",
            "file:///tmp/positions.bin",
            "../positions.bin",
            "data:application/octet-stream;base64,AAAA",
        ] {
            let mut value = model_json();
            value["buffers"][0]["uri"] = json!(uri);
            fs::write(&source, glb_with_json(&value)).unwrap();
            assert!(project.import_model(&source).is_err(), "URI {uri}");
        }
        let mut oversized_count = model_json();
        oversized_count["accessors"][0]["count"] = json!(u64::MAX);
        let mut oversized_view = model_json();
        oversized_view["bufferViews"][0]["byteLength"] = json!(u64::MAX);
        let mut bad_header = glb();
        bad_header[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        for bytes in [
            png(),
            glb_with_json(&oversized_count),
            glb_with_json(&oversized_view),
            bad_header,
        ] {
            fs::write(&source, bytes).unwrap();
            assert!(project.import_model(&source).is_err());
        }
        File::create(&source)
            .unwrap()
            .set_len(MAX_GLB_BYTES as u64 + 1)
            .unwrap();
        assert!(matches!(
            project.import_model(&source),
            Err(ProjectError::TooLarge(_))
        ));
        assert!(project.assets().is_empty());
        assert_eq!(project.revision(), 0);
        assert_eq!(
            before,
            fs::read(project.directory().join(PROJECT_FILE)).unwrap()
        );
        assert_eq!(
            fs::read_dir(project.directory().join("assets"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn stale_model_import_and_image_model_confusion_leave_manifest_unchanged() {
        let home = TestHome::new();
        let registry = standard_registry();
        let mut project = home
            .store()
            .create_project("Shared models", &FlowDocument::default(), &registry)
            .unwrap();
        let mut stale = project.clone();
        let source = home.0.join("model.glb");
        fs::write(&source, glb()).unwrap();
        assert!(project.import_image(&source).is_err());
        project.import_model(&source).unwrap();
        let before = fs::read(project.directory().join(PROJECT_FILE)).unwrap();
        assert!(matches!(
            stale.import_model(&source),
            Err(ProjectError::Conflict)
        ));
        assert_eq!(
            before,
            fs::read(project.directory().join(PROJECT_FILE)).unwrap()
        );
        assert_eq!(project.assets().len(), 1);
        assert_eq!(
            fs::read_dir(project.directory().join("assets"))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn model_content_and_manifest_tampering_are_rejected_on_open_and_resolve() {
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let mut project = store
            .create_project("Checked models", &FlowDocument::default(), &registry)
            .unwrap();
        let source = home.0.join("model.glb");
        fs::write(&source, glb()).unwrap();
        let asset = project.import_model(&source).unwrap();
        let path = project.resolve_asset_path(&asset.key).unwrap();
        let mut changed = glb();
        changed[0] = 0;
        fs::write(&path, changed).unwrap();
        // A UI path lookup deliberately leaves expensive content validation to
        // the worker. Full resolve and reopen must still reject invalid bytes.
        assert_eq!(project.resolve_asset_path(&asset.key).unwrap(), path);
        assert!(project.resolve_asset(&asset.key).is_err());
        assert!(
            store
                .open_project(&project.metadata().id, &registry)
                .is_err()
        );
        fs::write(&path, glb()).unwrap();
        assert!(
            store
                .open_project(&project.metadata().id, &registry)
                .is_ok()
        );
        for (field, value) in [
            ("width", json!(1)),
            ("height", json!(u32::MAX)),
            ("media_type", json!("image/png")),
            ("byte_len", json!(MAX_GLB_BYTES as u64 + 1)),
        ] {
            let mut manifest = serde_json::to_value(&project.project).unwrap();
            manifest["assets"][0][field] = value;
            fs::write(
                project.directory().join(PROJECT_FILE),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            assert!(
                store
                    .open_project(&project.metadata().id, &registry)
                    .is_err(),
                "field {field}"
            );
        }
    }

    #[test]
    fn mixed_media_share_project_manifest_count_and_total_byte_budgets() {
        let home = TestHome::new();
        let registry = standard_registry();
        let project = home
            .store()
            .create_project("Shared budget", &FlowDocument::default(), &registry)
            .unwrap();
        // Validate manifest accounting directly: the filesystem does not need
        // half a gigabyte of fixtures to exercise the mixed-media total limit.
        let mut candidate = project.project.clone();
        candidate.assets = (0..16)
            .map(|index| {
                let model = index % 2 == 0;
                ProjectAsset {
                    key: format!("assets/asset-{index}.{}", if model { "glb" } else { "png" }),
                    name: format!("Asset {index}"),
                    media_type: if model {
                        "model/gltf-binary"
                    } else {
                        "image/png"
                    }
                    .into(),
                    byte_len: MAX_IMAGE_BYTES,
                    width: u32::from(!model),
                    height: u32::from(!model),
                }
            })
            .collect();
        assert!(validate_project(&candidate, &candidate.metadata.id).is_ok());
        let mut extra = candidate.assets[0].clone();
        extra.key = "assets/extra.glb".into();
        extra.byte_len = glb().len() as u64;
        candidate.assets.push(extra.clone());
        assert!(matches!(
            validate_project(&candidate, &candidate.metadata.id),
            Err(ProjectError::TooLarge(_))
        ));
        candidate.assets = (0..=MAX_PROJECT_ASSETS)
            .map(|index| ProjectAsset {
                key: format!("assets/model-{index}.glb"),
                ..extra.clone()
            })
            .collect();
        assert!(matches!(
            validate_project(&candidate, &candidate.metadata.id),
            Err(ProjectError::TooLarge(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn model_import_and_both_resolvers_reject_symlink_files() {
        use std::os::unix::fs::symlink;
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let mut project = store
            .create_project("Model links", &FlowDocument::default(), &registry)
            .unwrap();
        let source = home.0.join("model.glb");
        fs::write(&source, glb()).unwrap();
        let linked_source = home.0.join("linked.glb");
        symlink(&source, &linked_source).unwrap();
        assert!(project.import_model(linked_source).is_err());
        assert!(project.assets().is_empty());
        let asset = project.import_model(&source).unwrap();
        let asset_path = project.resolve_asset_path(&asset.key).unwrap();
        fs::remove_file(&asset_path).unwrap();
        symlink(&source, &asset_path).unwrap();
        assert!(project.resolve_asset_path(&asset.key).is_err());
        assert!(project.resolve_asset(&asset.key).is_err());
        assert!(
            store
                .open_project(&project.metadata().id, &registry)
                .is_err()
        );
        assert!(project.resolve_asset_path("assets/missing.glb").is_err());
        assert!(project.resolve_asset_path("assets/../model.glb").is_err());
    }

    #[test]
    fn invalid_save_keeps_previous_bytes_and_handle() {
        let home = TestHome::new();
        let registry = standard_registry();
        let mut project = home
            .store()
            .create_project("Safe", &FlowDocument::default(), &registry)
            .unwrap();
        let before = fs::read(project.directory().join(PROJECT_FILE)).unwrap();
        let mut document = project.document().clone();
        document.viewport.zoom = f32::NAN;
        assert!(project.save_document(&document, &registry).is_err());
        assert_eq!(
            before,
            fs::read(project.directory().join(PROJECT_FILE)).unwrap()
        );
        assert_eq!(project.revision(), 0);
        assert_eq!(project.document(), &FlowDocument::default());
    }

    #[test]
    fn stale_clone_cannot_overwrite_a_newer_save_or_asset_manifest() {
        let home = TestHome::new();
        let registry = standard_registry();
        let mut project = home
            .store()
            .create_project("Shared", &FlowDocument::default(), &registry)
            .unwrap();
        let mut stale = project.clone();
        let source = home.0.join("image.png");
        fs::write(&source, png()).unwrap();
        project.import_image(&source).unwrap();
        let before = fs::read(project.directory().join(PROJECT_FILE)).unwrap();
        assert!(matches!(
            stale.save_document(&FlowDocument::default(), &registry),
            Err(ProjectError::Conflict)
        ));
        assert_eq!(
            before,
            fs::read(project.directory().join(PROJECT_FILE)).unwrap()
        );
    }

    #[test]
    fn traversal_urls_unknown_assets_and_external_json_paths_are_rejected() {
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        for id in ["..", "../outside", "/tmp", "a/b", "a\\b", "file:outside"] {
            assert!(store.open_project(id, &registry).is_err());
        }
        let mut project = store
            .create_project("Paths", &FlowDocument::default(), &registry)
            .unwrap();
        for key in [
            "../outside.png",
            "assets/../outside.png",
            "assets/a/b.png",
            "assets/a\\b.png",
            "assets/a%2fb.png",
            "https://example.test/a.png",
            "/tmp/a.png",
            "assets/missing.png",
        ] {
            assert!(project.resolve_asset(key).is_err());
            let mut document = FlowDocument::default();
            document
                .graph
                .add_node(
                    registry
                        .make_node(
                            "image",
                            "image",
                            Point::ZERO,
                            Some(json!({"asset": key, "caption": ""})),
                        )
                        .unwrap(),
                )
                .unwrap();
            assert!(project.save_document(&document, &registry).is_err());
        }
    }

    #[test]
    fn malformed_or_future_projects_do_not_hide_good_home_entries() {
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let good = store
            .create_project("Good", &FlowDocument::default(), &registry)
            .unwrap();
        fs::create_dir(store.root().join("broken")).unwrap();
        fs::write(store.root().join("broken/project.json"), b"not json").unwrap();
        let future = store
            .create_project("Future", &FlowDocument::default(), &registry)
            .unwrap();
        let path = future.directory().join(PROJECT_FILE);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["schema_version"] = json!(99);
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            store
                .open_project(&future.metadata().id, &registry)
                .is_err()
        );
        let listing = store.list_projects().unwrap();
        assert_eq!(listing.projects.len(), 1);
        assert_eq!(listing.projects[0].metadata.id, good.metadata().id);
        assert_eq!(listing.skipped.len(), 2);
    }

    #[test]
    fn image_import_is_bounded_and_rejects_svg_and_fake_extensions() {
        let home = TestHome::new();
        let registry = standard_registry();
        let mut project = home
            .store()
            .create_project("Bounded", &FlowDocument::default(), &registry)
            .unwrap();
        let source = home.0.join("image.png");
        for bytes in [
            b"not a PNG".as_slice(),
            b"<svg><image href='file:///secret'/></svg>",
        ] {
            fs::write(&source, bytes).unwrap();
            assert!(project.import_image(&source).is_err());
        }
        let mut bomb = png();
        bomb[16..20].copy_from_slice(&32768u32.to_be_bytes());
        bomb[20..24].copy_from_slice(&32768u32.to_be_bytes());
        fs::write(&source, bomb).unwrap();
        assert!(project.import_image(&source).is_err());
        File::create(&source)
            .unwrap()
            .set_len(MAX_IMAGE_BYTES + 1)
            .unwrap();
        assert!(matches!(
            project.import_image(&source),
            Err(ProjectError::TooLarge(_))
        ));
        assert!(project.assets().is_empty());
        assert_eq!(project.revision(), 0);
        assert_eq!(
            fs::read_dir(project.directory().join("assets"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn tampered_manifest_and_missing_asset_are_rejected_on_open() {
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let mut project = store
            .create_project("Manifest", &FlowDocument::default(), &registry)
            .unwrap();
        let source = home.0.join("image.png");
        fs::write(&source, png()).unwrap();
        let asset = project.import_image(&source).unwrap();
        fs::remove_file(project.resolve_asset(&asset.key).unwrap()).unwrap();
        assert!(
            store
                .open_project(&project.metadata().id, &registry)
                .is_err()
        );
        let path = project.directory().join(PROJECT_FILE);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["assets"][0]["key"] = json!("../outside.png");
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            store
                .open_project(&project.metadata().id, &registry)
                .is_err()
        );
    }

    #[test]
    fn forged_manifest_cannot_hide_changed_image_dimensions() {
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let mut project = store
            .create_project("Changed image", &FlowDocument::default(), &registry)
            .unwrap();
        let source = home.0.join("image.png");
        fs::write(&source, png()).unwrap();
        let asset = project.import_image(&source).unwrap();
        let path = project.resolve_asset(&asset.key).unwrap();
        let mut bytes = png();
        bytes[16..20].copy_from_slice(&32768u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&32768u32.to_be_bytes());
        fs::write(path, bytes).unwrap();
        assert!(project.resolve_asset(&asset.key).is_err());
        assert!(
            store
                .open_project(&project.metadata().id, &registry)
                .is_err()
        );
    }

    #[test]
    fn oversized_project_is_rejected_before_json_parsing() {
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let project = store
            .create_project("Oversized", &FlowDocument::default(), &registry)
            .unwrap();
        File::create(project.directory().join(PROJECT_FILE))
            .unwrap()
            .set_len(MAX_PROJECT_BYTES + 1)
            .unwrap();
        assert!(matches!(
            store.open_project(&project.metadata().id, &registry),
            Err(ProjectError::TooLarge(_))
        ));
        let listing = store.list_projects().unwrap();
        assert!(listing.projects.is_empty());
        assert_eq!(listing.skipped.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_sources_project_files_asset_files_and_directories_are_rejected() {
        use std::os::unix::fs::symlink;
        let home = TestHome::new();
        let store = home.store();
        let registry = standard_registry();
        let mut project = store
            .create_project("Links", &FlowDocument::default(), &registry)
            .unwrap();
        let source = home.0.join("image.png");
        fs::write(&source, png()).unwrap();
        let linked_source = home.0.join("link.png");
        symlink(&source, &linked_source).unwrap();
        assert!(project.import_image(linked_source).is_err());
        let asset = project.import_image(&source).unwrap();
        let asset_path = project.resolve_asset(&asset.key).unwrap();
        fs::remove_file(&asset_path).unwrap();
        symlink(&source, &asset_path).unwrap();
        assert!(project.resolve_asset(&asset.key).is_err());
        assert!(
            store
                .open_project(&project.metadata().id, &registry)
                .is_err()
        );
        fs::remove_file(&asset_path).unwrap();
        fs::write(&asset_path, png()).unwrap();
        let json_path = project.directory().join(PROJECT_FILE);
        let elsewhere = home.0.join("external.json");
        fs::rename(&json_path, &elsewhere).unwrap();
        let before = fs::read(&elsewhere).unwrap();
        symlink(&elsewhere, &json_path).unwrap();
        assert!(
            project
                .save_document(&FlowDocument::default(), &registry)
                .is_err()
        );
        assert_eq!(before, fs::read(&elsewhere).unwrap());
        symlink(store.root(), home.0.join("linked-home")).unwrap();
        assert!(ProjectStore::new(home.0.join("linked-home")).is_err());
        symlink(project.directory(), store.root().join("linked-project")).unwrap();
        assert!(store.open_project("linked-project", &registry).is_err());
    }

    #[test]
    fn encoded_jpeg_gif_and_webp_headers_are_bounded_before_decoder_construction() {
        let jpeg = [
            0xff, 0xd8, 0xff, 0xc0, 0, 11, 8, 0, 12, 0, 34, 1, 1, 0x11, 0, 0xff, 0xd9,
        ];
        let info = encoded_image_info(&jpeg).unwrap();
        assert_eq!((info.width, info.height, info.extension), (34, 12, "jpg"));
        let gif = b"GIF89a\x02\x00\x03\x00\x00\x00\x00";
        let info = encoded_image_info(gif).unwrap();
        assert_eq!((info.width, info.height, info.extension), (2, 3, "gif"));
        let mut webp =
            b"RIFF\x16\x00\x00\x00WEBPVP8X\x0a\x00\x00\x00\x00\x00\x00\x00\x04\x00\x00\x06\x00\x00"
                .to_vec();
        let info = encoded_image_info(&webp).unwrap();
        assert_eq!((info.width, info.height, info.extension), (5, 7, "webp"));
        webp[4] = 0;
        assert!(encoded_image_info(&webp).is_err());
    }
}

//! Local-only projects. `project.json` is a versioned envelope around an unchanged
//! `FlowDocument`; its asset manifest and graph commit together in one rename.
//!
//! Only an explicit host-selected path enters `import_image`. JSON contains asset
//! keys, never paths to open. Project IDs and keys are single, validated names;
//! symlink components are rejected before filesystem access. This protects against
//! untrusted project contents, not another process concurrently replacing trusted
//! parent directories. Hosts should serialize saves and keep the returned handle:
//! stale cloned handles fail with `Conflict` rather than overwriting newer work.

use crate::{CreativeError, CreativeRegistry, MAX_CARDS, MAX_JSON_BYTES, MAX_LINKS};
use gpui_flow_core::FlowDocument;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub const PROJECT_SCHEMA_VERSION: u32 = 1;
pub const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_PROJECT_ASSET_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_PROJECT_ASSETS: usize = 512;
pub const MAX_IMAGE_PIXELS: u64 = 64 * 1024 * 1024;
const MAX_PROJECT_BYTES: u64 = MAX_JSON_BYTES as u64 + 2 * 1024 * 1024;
const PROJECT_FILE: &str = "project.json";
static NEXT_NAME: AtomicU64 = AtomicU64::new(0);
static TRANSACTION: Mutex<()> = Mutex::new(());

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
            Self::Conflict => f.write_str("project changed on disk; reopen before saving"),
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
        root.ok_or_else(|| ProjectError::Invalid("could not locate the local project home".into()))
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
        validate_id(id)?;
        checked_directory(&self.root, false)?;
        let directory = self.root.join(id);
        let project = read_project(&directory, id)?;
        registry.validate_document(&project.document)?;
        check_assets_on_disk(&directory, &project.assets)?;
        for asset in &project.assets {
            verify_asset_image(&directory.join(&asset.key), asset)?;
        }
        Ok(LocalProject { directory, project })
    }
}

#[derive(Debug, Clone)]
pub struct LocalProject {
    directory: PathBuf,
    project: ProjectFile,
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

    /// Validate first, then fsync a unique sibling and atomically replace
    /// `project.json`. Any returned error leaves the previous document and this
    /// handle untouched. No delete-then-rename fallback is used.
    pub fn save_document(
        &mut self,
        document: &FlowDocument,
        registry: &CreativeRegistry,
    ) -> Result<(), ProjectError> {
        let _transaction = transaction()?;
        registry.validate_document(document)?;
        validate_document_assets(document, &self.project.assets)?;
        self.check_revision()?;
        check_assets_on_disk(&self.directory, &self.project.assets)?;
        let mut candidate = self.next_revision()?;
        candidate.document = document.clone();
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
        let _transaction = transaction()?;
        self.check_revision()?;
        if self.project.assets.len() >= MAX_PROJECT_ASSETS {
            return Err(ProjectError::TooLarge(
                "project has reached its 512 asset limit".into(),
            ));
        }
        let source = absolute_path(source.as_ref())?;
        let bytes = read_regular(&source, MAX_IMAGE_BYTES)?;
        let format = image_info(&bytes)?;
        let total = self
            .project
            .assets
            .iter()
            .map(|asset| asset.byte_len)
            .sum::<u64>();
        if total.saturating_add(bytes.len() as u64) > MAX_PROJECT_ASSET_BYTES {
            return Err(ProjectError::TooLarge(
                "project assets exceed 512 MiB".into(),
            ));
        }
        let key = format!("assets/{}.{}", unique_name("a"), format.extension);
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Imported image")
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect::<String>();
        let asset = ProjectAsset {
            key,
            name: if name.is_empty() {
                "Imported image".into()
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
    pub fn resolve_asset(&self, key: &str) -> Result<PathBuf, ProjectError> {
        validate_asset_key(key)?;
        let asset = self
            .project
            .assets
            .iter()
            .find(|asset| asset.key == key)
            .ok_or_else(|| {
                ProjectError::Invalid("asset is not in this project's manifest".into())
            })?;
        let path = self.directory.join(key);
        check_asset_file(&path, asset)?;
        verify_asset_image(&path, asset)?;
        Ok(path)
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
        candidate.revision = candidate
            .revision
            .checked_add(1)
            .ok_or_else(|| ProjectError::Invalid("project revision overflow".into()))?;
        candidate.metadata.updated_at_ms = now_ms().max(candidate.metadata.updated_at_ms);
        Ok(candidate)
    }
}

fn transaction() -> Result<std::sync::MutexGuard<'static, ()>, ProjectError> {
    TRANSACTION
        .lock()
        .map_err(|_| ProjectError::Invalid("project storage lock was poisoned".into()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn unique_name(prefix: &str) -> String {
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
        return Err(ProjectError::Invalid("invalid project ID".into()));
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), ProjectError> {
    if name.trim().is_empty() || name.len() > 512 || name.chars().any(char::is_control) {
        return Err(ProjectError::Invalid(
            "project name must contain 1–512 bytes of printable text".into(),
        ));
    }
    Ok(())
}

fn validate_asset_key(key: &str) -> Result<(), ProjectError> {
    let valid = key.strip_prefix("assets/").is_some_and(|name| {
        name.len() <= 128
            && name.rsplit_once('.').is_some_and(|(stem, ext)| {
                !stem.is_empty()
                    && stem
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                    && matches!(ext, "png" | "jpg" | "gif" | "webp")
            })
    });
    if !valid {
        return Err(ProjectError::Invalid(
            "invalid project-relative asset key".into(),
        ));
    }
    Ok(())
}

fn validate_document_assets(
    document: &FlowDocument,
    assets: &[ProjectAsset],
) -> Result<(), ProjectError> {
    for node in document.graph.nodes.values() {
        let Some(value) = node.data.get("asset") else {
            continue;
        };
        let key = value
            .as_str()
            .ok_or_else(|| ProjectError::Invalid("asset key must be a string".into()))?;
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
        if !assets.iter().any(|asset| asset.key == key) {
            return Err(ProjectError::Invalid(format!(
                "missing project asset: {key}"
            )));
        }
    }
    Ok(())
}

fn validate_project(project: &ProjectFile, id: &str) -> Result<(), ProjectError> {
    if project.schema_version != PROJECT_SCHEMA_VERSION {
        return Err(ProjectError::Invalid(format!(
            "unsupported project schema version: {}",
            project.schema_version
        )));
    }
    validate_id(&project.metadata.id)?;
    if project.metadata.id != id {
        return Err(ProjectError::Invalid(
            "project ID does not match its directory".into(),
        ));
    }
    validate_name(&project.metadata.name)?;
    if project.metadata.updated_at_ms < project.metadata.created_at_ms {
        return Err(ProjectError::Invalid("invalid project timestamps".into()));
    }
    if project.document.graph.nodes.len() > MAX_CARDS
        || project.document.graph.edges.len() > MAX_LINKS
        || project.document.graph.groups.len() > MAX_CARDS
    {
        return Err(ProjectError::TooLarge(
            "creative board size limit exceeded".into(),
        ));
    }
    project
        .document
        .validate()
        .map_err(|error| ProjectError::Invalid(error.to_string()))?;
    if serde_json::to_vec(&project.document)?.len() > MAX_JSON_BYTES {
        return Err(ProjectError::TooLarge(
            "creative document exceeds 32 MiB".into(),
        ));
    }
    if project.assets.len() > MAX_PROJECT_ASSETS {
        return Err(ProjectError::TooLarge(
            "project has more than 512 assets".into(),
        ));
    }
    let mut keys = BTreeSet::new();
    let mut total = 0u64;
    for asset in &project.assets {
        validate_asset_key(&asset.key)?;
        if !keys.insert(&asset.key) {
            return Err(ProjectError::Invalid("duplicate project asset key".into()));
        }
        if asset.name.is_empty()
            || asset.name.len() > 1024
            || asset.name.chars().any(char::is_control)
        {
            return Err(ProjectError::Invalid("invalid asset display name".into()));
        }
        let expected_type = match asset.key.rsplit('.').next() {
            Some("png") => "image/png",
            Some("jpg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            _ => return Err(ProjectError::Invalid("unsupported image extension".into())),
        };
        if asset.media_type != expected_type
            || asset.byte_len == 0
            || asset.byte_len > MAX_IMAGE_BYTES
        {
            return Err(ProjectError::Invalid("invalid asset type or size".into()));
        }
        validate_dimensions(asset.width, asset.height)?;
        total = total.saturating_add(asset.byte_len);
    }
    if total > MAX_PROJECT_ASSET_BYTES {
        return Err(ProjectError::TooLarge(
            "project assets exceed 512 MiB".into(),
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
    validate_project(project, &project.metadata.id)?;
    let bytes = serde_json::to_vec_pretty(project)?;
    if bytes.len() as u64 > MAX_PROJECT_BYTES {
        return Err(ProjectError::TooLarge("project JSON exceeds 34 MiB".into()));
    }
    checked_directory(directory, false)?;
    let target = directory.join(PROJECT_FILE);
    match fs::symlink_metadata(&target) {
        Ok(metadata) if !metadata.is_file() || is_link(&metadata) => {
            return Err(ProjectError::Invalid(
                "project.json must be a regular file".into(),
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
            "empty paths and parent traversal are not allowed".into(),
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
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err(ProjectError::Invalid(
                "parent traversal is not allowed".into(),
            ));
        }
        current.push(component.as_os_str());
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
                "project paths cannot contain symlinks or non-directories".into(),
            ));
        }
    }
    Ok(())
}

fn regular_metadata(path: &Path) -> Result<fs::Metadata, ProjectError> {
    let parent = path
        .parent()
        .ok_or_else(|| ProjectError::Invalid("file has no parent".into()))?;
    checked_directory(parent, false)?;
    let metadata = fs::symlink_metadata(path)?;
    if is_link(&metadata) || !metadata.is_file() {
        return Err(ProjectError::Invalid(
            "expected a regular file, not a symlink".into(),
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
            "file exceeds its allowed size".into(),
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
            "file changed or exceeds its allowed size".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != opened.dev() || before.ino() != opened.ino() {
            return Err(ProjectError::Invalid(
                "file changed while being opened".into(),
            ));
        }
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(ProjectError::TooLarge(
            "file exceeds its allowed size".into(),
        ));
    }
    regular_metadata(path)?;
    Ok(bytes)
}

fn check_asset_file(path: &Path, asset: &ProjectAsset) -> Result<(), ProjectError> {
    let metadata = regular_metadata(path)?;
    if metadata.len() != asset.byte_len {
        return Err(ProjectError::Invalid(format!(
            "asset size changed: {}",
            asset.key
        )));
    }
    Ok(())
}

fn verify_asset_image(path: &Path, asset: &ProjectAsset) -> Result<(), ProjectError> {
    let bytes = read_regular(path, MAX_IMAGE_BYTES)?;
    let info = image_info(&bytes)?;
    if bytes.len() as u64 != asset.byte_len
        || info.width != asset.width
        || info.height != asset.height
        || info.media_type != asset.media_type
    {
        return Err(ProjectError::Invalid(format!(
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

struct ImageInfo {
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
            "image dimensions must be nonzero, at most 32768 per side and 64 megapixels".into(),
        ));
    }
    Ok(())
}

fn image_info(bytes: &[u8]) -> Result<ImageInfo, ProjectError> {
    let invalid = || ProjectError::Invalid("choose a valid PNG, JPEG, GIF, or WebP image".into());
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
    Ok(ImageInfo {
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
    fn png() -> Vec<u8> {
        // Complete 1x1 PNG fixture, not just a filename with an image extension.
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 11, 73, 68, 65, 84, 120, 156, 99, 96, 0, 2,
            0, 0, 5, 0, 1, 122, 94, 171, 63, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ]
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
    fn jpeg_gif_and_webp_dimensions_are_read_without_an_image_dependency() {
        let jpeg = [
            0xff, 0xd8, 0xff, 0xc0, 0, 11, 8, 0, 12, 0, 34, 1, 1, 0x11, 0, 0xff, 0xd9,
        ];
        let info = image_info(&jpeg).unwrap();
        assert_eq!((info.width, info.height, info.extension), (34, 12, "jpg"));
        let gif = b"GIF89a\x02\x00\x03\x00\x00\x00\x00";
        let info = image_info(gif).unwrap();
        assert_eq!((info.width, info.height, info.extension), (2, 3, "gif"));
        let mut webp =
            b"RIFF\x16\x00\x00\x00WEBPVP8X\x0a\x00\x00\x00\x00\x00\x00\x00\x04\x00\x00\x06\x00\x00"
                .to_vec();
        let info = image_info(&webp).unwrap();
        assert_eq!((info.width, info.height, info.extension), (5, 7, "webp"));
        webp[4] = 0;
        assert!(image_info(&webp).is_err());
    }
}

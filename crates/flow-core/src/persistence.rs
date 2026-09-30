//! Bounded, validated file persistence. Local paths and provider secrets remain host concerns.
use crate::{FlowDocument, GraphError};
use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
const MAX_DOCUMENT_BYTES: u64 = 32 * 1024 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
#[derive(Debug)]
pub enum PersistenceError {
    Io(io::Error),
    Invalid(GraphError),
    TooLarge,
}
impl fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Invalid(e) => e.fmt(f),
            Self::TooLarge => write!(f, "document exceeds 32 MiB limit"),
        }
    }
}
impl std::error::Error for PersistenceError {}
impl From<io::Error> for PersistenceError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<GraphError> for PersistenceError {
    fn from(e: GraphError) -> Self {
        Self::Invalid(e)
    }
}
/// Validate, write a unique sibling temporary file, sync, then rename over the destination.
/// No auto-created directories; errors preserve the existing destination. Filesystem rename
/// durability and overwrite guarantees ultimately depend on the host OS/filesystem.
pub fn save_document(
    path: impl AsRef<Path>,
    document: &FlowDocument,
) -> Result<(), PersistenceError> {
    let path = path.as_ref();
    let json = document.to_json()?;
    if json.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(PersistenceError::TooLarge);
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temp = parent.join(format!(
        ".gpui-flow-{}-{}.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| {
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
/// Read at most 32 MiB plus a sentinel byte, then validate schema, topology and viewport.
pub fn load_document(path: impl AsRef<Path>) -> Result<FlowDocument, PersistenceError> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(PersistenceError::TooLarge);
    }
    let json =
        String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(FlowDocument::from_json(&json)?)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Editor, Point};
    fn path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "gpui-flow-test-{}-{}.json",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ))
    }
    #[test]
    fn roundtrip_and_overwrite() {
        let p = path();
        let mut d = Editor::default().document();
        save_document(&p, &d).unwrap();
        assert_eq!(load_document(&p).unwrap(), d);
        d.viewport.pan = Point::new(4., 8.);
        save_document(&p, &d).unwrap();
        assert_eq!(load_document(&p).unwrap(), d);
        fs::remove_file(p).unwrap();
    }
    #[test]
    fn invalid_save_preserves_file() {
        let p = path();
        let mut d = Editor::default().document();
        save_document(&p, &d).unwrap();
        let before = fs::read(&p).unwrap();
        d.viewport.zoom = f32::NAN;
        assert!(save_document(&p, &d).is_err());
        assert_eq!(before, fs::read(&p).unwrap());
        fs::remove_file(p).unwrap();
    }
    #[test]
    fn invalid_load() {
        let p = path();
        fs::write(&p, b"not json").unwrap();
        assert!(load_document(&p).is_err());
        fs::remove_file(p).unwrap();
    }
    #[test]
    fn bounded_load() {
        let p = path();
        let f = fs::File::create(&p).unwrap();
        f.set_len(MAX_DOCUMENT_BYTES + 1).unwrap();
        assert!(matches!(load_document(&p), Err(PersistenceError::TooLarge)));
        fs::remove_file(p).unwrap();
    }
}

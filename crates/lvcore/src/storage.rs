use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::error::Error;
use crate::error::Result;

pub trait StorageBackend: Send + Sync {
    fn root(&self) -> &Path;
    fn read(&self, relative: &Path) -> Result<Vec<u8>>;
    fn exists(&self, relative: &Path) -> Result<bool>;
    fn resolve_casefolded(&self, relative: &Path) -> Result<Option<PathBuf>>;
    fn list_dir(&self, relative: &Path) -> Result<Vec<PathBuf>>;
}

#[derive(Debug, Clone)]
pub struct DirectoryStorage {
    resolver: CaseFoldedDirectory,
}

impl DirectoryStorage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            resolver: CaseFoldedDirectory::new(root),
        }
    }
}

impl StorageBackend for DirectoryStorage {
    fn root(&self) -> &Path {
        self.resolver.root()
    }

    fn read(&self, relative: &Path) -> Result<Vec<u8>> {
        let Some(path) = self.resolve_casefolded(relative)? else {
            return Err(Error::Driver(format!(
                "storage path is missing or outside the package: {}",
                relative.display()
            )));
        };
        if !path_stays_inside_root(self.root(), &path)? {
            return Err(Error::Driver(format!(
                "storage path is outside the package: {}",
                relative.display()
            )));
        }
        Ok(fs::read(path)?)
    }

    fn exists(&self, relative: &Path) -> Result<bool> {
        let Some(path) = self.resolve_casefolded(relative)? else {
            return Ok(false);
        };
        path_stays_inside_root(self.root(), &path)
    }

    fn resolve_casefolded(&self, relative: &Path) -> Result<Option<PathBuf>> {
        self.resolver.find(relative)
    }

    fn list_dir(&self, relative: &Path) -> Result<Vec<PathBuf>> {
        let Some(base) = self.resolve_casefolded(relative)? else {
            return Ok(Vec::new());
        };
        if !base.is_dir() {
            return Ok(Vec::new());
        }
        if !path_stays_inside_root(self.root(), &base)? {
            return Ok(Vec::new());
        }
        let mut rows = Vec::new();
        for entry in fs::read_dir(base)? {
            rows.push(entry?.path());
        }
        rows.sort_by_cached_key(|path| {
            (
                path.file_name()
                    .map(|v| v.to_string_lossy().casefold())
                    .unwrap_or_default(),
                path.clone(),
            )
        });
        Ok(rows)
    }
}

#[derive(Debug, Clone)]
pub struct CaseFoldedDirectory {
    root: PathBuf,
    directory_cache: Arc<Mutex<BTreeMap<PathBuf, BTreeMap<String, PathBuf>>>>,
}

impl CaseFoldedDirectory {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            directory_cache: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn find(&self, relative: &Path) -> Result<Option<PathBuf>> {
        if relative.as_os_str().is_empty() {
            return Ok(Some(self.root.clone()));
        }
        let mut current = self.root.clone();
        for component in relative.components() {
            let wanted = component.as_os_str().to_string_lossy().casefold();
            if wanted == "." {
                continue;
            }
            if wanted == ".." {
                return Ok(None);
            }
            let children = self.children_by_casefold(&current)?;
            let Some(next) = children.get(&wanted) else {
                return Ok(None);
            };
            current = next.clone();
        }
        Ok(Some(current))
    }

    pub fn find_child_named(&self, directory: &Path, name: &str) -> Result<Option<PathBuf>> {
        let children = self.children_by_casefold(directory)?;
        Ok(children.get(&name.casefold()).cloned())
    }

    fn children_by_casefold(&self, directory: &Path) -> Result<BTreeMap<String, PathBuf>> {
        {
            let cache = self
                .directory_cache
                .lock()
                .map_err(|_| Error::Driver("casefold directory cache is poisoned".to_owned()))?;
            if let Some(children) = cache.get(directory) {
                return Ok(children.clone());
            }
        }

        let children = directory_children_by_casefold(directory)?;
        let mut cache = self
            .directory_cache
            .lock()
            .map_err(|_| Error::Driver("casefold directory cache is poisoned".to_owned()))?;
        Ok(cache
            .entry(directory.to_path_buf())
            .or_insert(children)
            .clone())
    }
}

fn directory_children_by_casefold(directory: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let mut children = BTreeMap::new();
    if !directory.is_dir() {
        return Ok(children);
    }
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let Some(name) = path.file_name() else {
            continue;
        };
        children
            .entry(name.to_string_lossy().casefold())
            .or_insert(path);
    }
    Ok(children)
}

fn path_stays_inside_root(root: &Path, path: &Path) -> Result<bool> {
    let root = fs::canonicalize(root)?;
    let path = fs::canonicalize(path)?;
    Ok(path.starts_with(root))
}

trait Casefold {
    fn casefold(&self) -> String;
}

impl Casefold for str {
    fn casefold(&self) -> String {
        self.to_lowercase()
    }
}

impl Casefold for std::borrow::Cow<'_, str> {
    fn casefold(&self) -> String {
        self.as_ref().to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn casefold_lookup_preserves_on_disk_casing() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("HONMON.DIN"), b"data").unwrap();
        let storage = DirectoryStorage::new(dir.path());

        let resolved = storage
            .resolve_casefolded(Path::new("honmon.din"))
            .unwrap()
            .unwrap();
        assert_eq!(resolved.file_name().unwrap(), "HONMON.DIN");
        assert_eq!(storage.read(Path::new("honmon.din")).unwrap(), b"data");
    }

    #[test]
    fn read_does_not_escape_package_root() {
        let dir = tempdir().unwrap();
        let outside = dir.path().with_file_name("outside-lvcore-storage-test.txt");
        fs::write(&outside, b"outside").unwrap();
        let storage = DirectoryStorage::new(dir.path());

        let error = storage.read(Path::new("../outside-lvcore-storage-test.txt"));

        fs::remove_file(outside).unwrap();
        assert!(error.is_err());
    }

    #[test]
    fn list_dir_missing_or_parent_paths_stay_inside_package() {
        let dir = tempdir().unwrap();
        let storage = DirectoryStorage::new(dir.path());

        assert!(storage.list_dir(Path::new("missing")).unwrap().is_empty());
        assert!(storage.list_dir(Path::new("..")).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_not_readable() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let outside = dir
            .path()
            .with_file_name("outside-lvcore-storage-symlink.txt");
        fs::write(&outside, b"outside").unwrap();
        symlink(&outside, dir.path().join("outside-link")).unwrap();
        let storage = DirectoryStorage::new(dir.path());

        assert!(!storage.exists(Path::new("outside-link")).unwrap());
        assert!(storage.read(Path::new("outside-link")).is_err());

        fs::remove_file(outside).unwrap();
    }
}

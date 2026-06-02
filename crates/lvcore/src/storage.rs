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

pub(crate) fn private_cache_dir(namespace: &str) -> Result<PathBuf> {
    let dir = user_cache_base_dir().join("lvcore-rs").join(namespace);
    create_private_dir_all(&dir)?;
    Ok(dir)
}

fn user_cache_base_dir() -> PathBuf {
    if let Some(value) = std::env::var_os("XDG_CACHE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }
    if let Some(value) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(value).join(".cache");
    }
    if let Some(value) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }
    if let Some(value) = std::env::var_os("APPDATA").filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }
    std::env::temp_dir().join(format!("lvcore-rs-cache-{}", std::process::id()))
}

#[cfg(unix)]
fn create_private_dir_all(path: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn create_private_dir_all(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    Ok(())
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
        if !self.resolver.path_stays_inside_root(&path)? {
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
        self.resolver.path_stays_inside_root(&path)
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
        if !self.resolver.path_stays_inside_root(&base)? {
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
    canonical_root: Arc<Mutex<Option<PathBuf>>>,
}

impl CaseFoldedDirectory {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            directory_cache: Arc::new(Mutex::new(BTreeMap::new())),
            canonical_root: Arc::new(Mutex::new(None)),
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
            let Some(next) = self.find_child_by_casefold(&current, &wanted)? else {
                return Ok(None);
            };
            current = next;
        }
        Ok(Some(current))
    }

    pub fn find_child_named(&self, directory: &Path, name: &str) -> Result<Option<PathBuf>> {
        self.find_child_by_casefold(directory, &name.casefold())
    }

    fn find_child_by_casefold(&self, directory: &Path, wanted: &str) -> Result<Option<PathBuf>> {
        {
            let cache = self
                .directory_cache
                .lock()
                .map_err(|_| Error::Driver("casefold directory cache is poisoned".to_owned()))?;
            if let Some(children) = cache.get(directory) {
                return Ok(children.get(wanted).cloned());
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
            .get(wanted)
            .cloned())
    }

    fn path_stays_inside_root(&self, path: &Path) -> Result<bool> {
        path_stays_inside_root_canonical(&self.canonical_root()?, path)
    }

    fn canonical_root(&self) -> Result<PathBuf> {
        {
            let cache = self
                .canonical_root
                .lock()
                .map_err(|_| Error::Driver("canonical root cache is poisoned".to_owned()))?;
            if let Some(root) = cache.as_ref() {
                return Ok(root.clone());
            }
        }
        let root = fs::canonicalize(&self.root)?;
        let mut cache = self
            .canonical_root
            .lock()
            .map_err(|_| Error::Driver("canonical root cache is poisoned".to_owned()))?;
        Ok(cache.get_or_insert(root).clone())
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

pub(crate) fn path_stays_inside_root(root: &Path, path: &Path) -> Result<bool> {
    path_stays_inside_root_canonical(&fs::canonicalize(root)?, path)
}

pub(crate) fn regular_file_inside_root(root: &Path, path: &Path) -> Result<bool> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(false);
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(false);
    }
    if path_has_symlink_component(root, path)? {
        return Ok(false);
    }
    path_stays_inside_root(root, path)
}

pub(crate) fn regular_directory_inside_root(root: &Path, path: &Path) -> Result<bool> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(false);
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(false);
    }
    if path_has_symlink_component(root, path)? {
        return Ok(false);
    }
    path_stays_inside_root(root, path)
}

fn path_has_symlink_component(root: &Path, path: &Path) -> Result<bool> {
    let Ok(relative) = path.strip_prefix(root) else {
        return Ok(false);
    };
    let mut current = root.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(name) => current.push(name),
            std::path::Component::CurDir => continue,
            _ => return Ok(true),
        }
        let Ok(metadata) = fs::symlink_metadata(&current) else {
            return Ok(false);
        };
        if metadata.file_type().is_symlink() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn path_stays_inside_root_canonical(canonical_root: &Path, path: &Path) -> Result<bool> {
    let path = fs::canonicalize(path)?;
    Ok(path.starts_with(canonical_root))
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

    #[cfg(unix)]
    #[test]
    fn regular_file_inside_root_rejects_symlink_parent() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        fs::write(real.join("page.html"), b"inside").unwrap();
        symlink(&real, dir.path().join("linked")).unwrap();

        assert!(
            !regular_file_inside_root(dir.path(), &dir.path().join("linked/page.html")).unwrap()
        );
    }
}

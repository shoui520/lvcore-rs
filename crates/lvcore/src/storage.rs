use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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
        let path = self
            .resolve_casefolded(relative)?
            .unwrap_or_else(|| self.root().join(relative));
        Ok(fs::read(path)?)
    }

    fn exists(&self, relative: &Path) -> Result<bool> {
        Ok(self.resolve_casefolded(relative)?.is_some())
    }

    fn resolve_casefolded(&self, relative: &Path) -> Result<Option<PathBuf>> {
        self.resolver.find(relative)
    }

    fn list_dir(&self, relative: &Path) -> Result<Vec<PathBuf>> {
        let base = self
            .resolve_casefolded(relative)?
            .unwrap_or_else(|| self.root().join(relative));
        if !base.is_dir() {
            return Ok(Vec::new());
        }
        let mut rows = Vec::new();
        for entry in fs::read_dir(base)? {
            rows.push(entry?.path());
        }
        rows.sort_by(|a, b| {
            a.file_name()
                .map(|v| v.to_string_lossy().casefold())
                .cmp(&b.file_name().map(|v| v.to_string_lossy().casefold()))
                .then_with(|| a.cmp(b))
        });
        Ok(rows)
    }
}

#[derive(Debug, Clone)]
pub struct CaseFoldedDirectory {
    root: PathBuf,
}

impl CaseFoldedDirectory {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
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
            let children = directory_children_by_casefold(&current)?;
            let Some(next) = children.get(&wanted) else {
                return Ok(None);
            };
            current = next.clone();
        }
        Ok(Some(current))
    }

    pub fn find_child_named(&self, directory: &Path, name: &str) -> Result<Option<PathBuf>> {
        let children = directory_children_by_casefold(directory)?;
        Ok(children.get(&name.casefold()).cloned())
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
}

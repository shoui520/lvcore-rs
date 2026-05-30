use super::*;

impl ReaderBookPackage {
    pub(super) fn resolve_package_file_path(&self, path: &str) -> Result<Option<PathBuf>> {
        let normalized = path.replace('\\', "/");
        let relative = Path::new(&normalized);
        if self.storage.exists(relative)?
            && let Some(path) = self.storage.resolve_casefolded(relative)?
        {
            return Ok(Some(path));
        }
        self.resolve_adjacent_templates_file_path(&normalized)
    }

    pub(super) fn read_package_file_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let normalized = path.replace('\\', "/");
        let relative = Path::new(&normalized);
        if self.storage.exists(relative)? {
            return self.storage.read(relative);
        }
        let Some((templates_root, stripped)) = self.adjacent_templates_root_and_path(relative)
        else {
            return Err(Error::Driver(format!("resource not found: {path}")));
        };
        DirectoryStorage::new(templates_root).read(stripped)
    }

    fn resolve_adjacent_templates_file_path(&self, path: &str) -> Result<Option<PathBuf>> {
        let relative = Path::new(path);
        let Some((templates_root, stripped)) = self.adjacent_templates_root_and_path(relative)
        else {
            return Ok(None);
        };
        let storage = DirectoryStorage::new(templates_root);
        if storage.exists(stripped)? {
            return storage.resolve_casefolded(stripped);
        }
        Ok(None)
    }

    fn adjacent_templates_root_and_path<'a>(
        &self,
        relative: &'a Path,
    ) -> Option<(PathBuf, &'a Path)> {
        let mut components = relative.components();
        let first = components.next()?;
        if !first
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("Templates")
        {
            return None;
        }
        let stripped = components.as_path();
        if stripped.as_os_str().is_empty() {
            return None;
        }
        let package_name = self.root.file_name().and_then(|name| name.to_str())?;
        let sibling_templates_root = self
            .root
            .with_file_name(format!("{package_name}_Templates"));
        if !sibling_templates_root.is_dir() {
            return None;
        }
        Some((sibling_templates_root, stripped))
    }
}

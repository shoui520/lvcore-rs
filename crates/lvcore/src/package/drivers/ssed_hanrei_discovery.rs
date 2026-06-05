use super::*;

impl ReaderBookPackage {
    pub(super) fn has_ssed_hanrei_surface(&self) -> Result<bool> {
        for candidate in [
            "hanrei.html",
            "HANREI.html",
            "HANREI/index.html",
            "HANREI/index.htm",
            "HANREI/hanrei.html",
            "HANREI/hanrei.htm",
            "HANREI.chm",
        ] {
            if self.ssed_hanrei_regular_file_exists(candidate)? {
                return Ok(true);
            }
        }
        if self.ssed_hanrei_folder_has_html("HANREI", 0)? {
            return Ok(true);
        }
        for path in self.storage.list_dir(Path::new(""))? {
            if !regular_directory_inside_root(&self.root, &path)? {
                continue;
            }
            let Some(name) = path.file_name().map(|value| value.to_string_lossy()) else {
                continue;
            };
            if name.starts_with("._") || !name.to_ascii_lowercase().ends_with("_help.localized") {
                continue;
            }
            let root = name.replace('\\', "/");
            for candidate in [
                format!("{root}/index.html"),
                format!("{root}/index.htm"),
                format!("{root}/menu.html"),
                format!("{root}/top.html"),
                format!("{root}/contents/hanrei.html"),
                format!("{root}/contents/hanrei.htm"),
                format!("{root}/contents/copyright.html"),
                format!("{root}/contents/copyright.htm"),
            ] {
                if self.ssed_hanrei_regular_file_exists(&candidate)? {
                    return Ok(true);
                }
            }
            if self.ssed_hanrei_folder_has_html(&format!("{root}/contents"), 0)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn discover_ssed_hanrei_pages(&self) -> Result<Vec<SsedHanreiPage>> {
        let mut pages = Vec::new();
        let mut seen = BTreeSet::new();

        for candidate in [
            "hanrei.html",
            "HANREI.html",
            "HANREI/index.html",
            "HANREI/index.htm",
            "HANREI/hanrei.html",
            "HANREI/hanrei.htm",
        ] {
            self.push_ssed_hanrei_page(candidate, &mut pages, &mut seen)?;
        }

        self.push_ssed_hanrei_folder_pages("HANREI", &mut pages, &mut seen, 0)?;
        self.push_ssed_hanrei_chm_pages("HANREI.chm", &mut pages, &mut seen)?;

        for path in self.storage.list_dir(Path::new(""))? {
            if !regular_directory_inside_root(&self.root, &path)? {
                continue;
            }
            let Some(name) = path.file_name().map(|value| value.to_string_lossy()) else {
                continue;
            };
            if name.starts_with("._") || !name.to_ascii_lowercase().ends_with("_help.localized") {
                continue;
            }
            let root = name.replace('\\', "/");
            for candidate in [
                format!("{root}/index.html"),
                format!("{root}/index.htm"),
                format!("{root}/menu.html"),
                format!("{root}/top.html"),
                format!("{root}/contents/hanrei.html"),
                format!("{root}/contents/hanrei.htm"),
                format!("{root}/contents/copyright.html"),
                format!("{root}/contents/copyright.htm"),
            ] {
                self.push_ssed_hanrei_page(&candidate, &mut pages, &mut seen)?;
            }

            let contents_dir = format!("{root}/contents");
            for child in self.storage.list_dir(Path::new(&contents_dir))? {
                if !regular_file_inside_root(&self.root, &child)? {
                    continue;
                }
                let Some(file_name) = child.file_name().map(|value| value.to_string_lossy()) else {
                    continue;
                };
                if file_name.starts_with("._") {
                    continue;
                }
                if !path_has_extension(&file_name, &["html", "htm"]) {
                    continue;
                }
                let candidate = format!("{contents_dir}/{file_name}");
                self.push_ssed_hanrei_page(&candidate, &mut pages, &mut seen)?;
            }
        }

        Ok(pages)
    }

    fn ssed_hanrei_regular_file_exists(&self, relative: &str) -> Result<bool> {
        let normalized = relative.replace('\\', "/");
        if normalized
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        {
            return Ok(false);
        }
        if !self.storage.exists(Path::new(&normalized))? {
            return Ok(false);
        }
        let Some(path) = self.storage.resolve_casefolded(Path::new(&normalized))? else {
            return Ok(false);
        };
        regular_file_inside_root(&self.root, &path)
    }

    fn ssed_hanrei_folder_has_html(&self, relative_dir: &str, depth: usize) -> Result<bool> {
        if depth > 8 || !self.storage.exists(Path::new(relative_dir))? {
            return Ok(false);
        }
        for child in self.storage.list_dir(Path::new(relative_dir))? {
            let Some(file_name) = child.file_name().map(|value| value.to_string_lossy()) else {
                continue;
            };
            if file_name.starts_with("._") {
                continue;
            }
            let candidate = format!("{relative_dir}/{file_name}");
            if regular_directory_inside_root(&self.root, &child)? {
                if self.ssed_hanrei_folder_has_html(&candidate, depth + 1)? {
                    return Ok(true);
                }
            } else if regular_file_inside_root(&self.root, &child)?
                && path_has_extension(&file_name, &["html", "htm"])
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn push_ssed_hanrei_folder_pages(
        &self,
        relative_dir: &str,
        pages: &mut Vec<SsedHanreiPage>,
        seen: &mut BTreeSet<String>,
        depth: usize,
    ) -> Result<()> {
        if depth > 8 || !self.storage.exists(Path::new(relative_dir))? {
            return Ok(());
        }
        for child in self.storage.list_dir(Path::new(relative_dir))? {
            let Some(file_name) = child.file_name().map(|value| value.to_string_lossy()) else {
                continue;
            };
            if file_name.starts_with("._") {
                continue;
            }
            let candidate = format!("{relative_dir}/{file_name}");
            if regular_directory_inside_root(&self.root, &child)? {
                self.push_ssed_hanrei_folder_pages(&candidate, pages, seen, depth + 1)?;
            } else if regular_file_inside_root(&self.root, &child)?
                && path_has_extension(&file_name, &["html", "htm"])
            {
                self.push_ssed_hanrei_page(&candidate, pages, seen)?;
            }
        }
        Ok(())
    }

    pub(super) fn push_ssed_hanrei_page(
        &self,
        candidate: &str,
        pages: &mut Vec<SsedHanreiPage>,
        seen: &mut BTreeSet<String>,
    ) -> Result<()> {
        let normalized = candidate.replace('\\', "/");
        if normalized
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        {
            return Ok(());
        }
        if !self.storage.exists(Path::new(&normalized))? {
            return Ok(());
        }
        let Some(path) = self.storage.resolve_casefolded(Path::new(&normalized))? else {
            return Ok(());
        };
        if !regular_file_inside_root(&self.root, &path)? {
            return Ok(());
        }
        if !seen.insert(normalized.to_ascii_lowercase()) {
            return Ok(());
        }
        let resource_kind = resource_kind_from_path(&normalized);
        pages.push(SsedHanreiPage {
            item_id: normalized.clone(),
            label: self.ssed_hanrei_package_page_label(&normalized),
            resource: InternalResource::PackageFile {
                path: normalized,
                resource_kind,
            },
            anchor: None,
            diagnostics: Vec::new(),
        });
        Ok(())
    }

    pub(super) fn ssed_hanrei_package_page_label(&self, normalized: &str) -> String {
        if path_has_extension(normalized, &["html", "htm"])
            && let Ok(data) = self.storage.read(Path::new(normalized))
        {
            let html = decode_package_html_text(&data);
            if let Some(label) = html_document_label(&html) {
                return label;
            }
        }
        ssed_hanrei_page_label(normalized)
    }

    pub(super) fn push_ssed_hanrei_chm_pages(
        &self,
        chm_path: &str,
        pages: &mut Vec<SsedHanreiPage>,
        seen: &mut BTreeSet<String>,
    ) -> Result<()> {
        if !self.storage.exists(Path::new(chm_path))? {
            return Ok(());
        }
        let Some(resolved) = self.storage.resolve_casefolded(Path::new(chm_path))? else {
            return Ok(());
        };
        if !regular_file_inside_root(&self.root, &resolved)? {
            return Ok(());
        }
        let mut entries = match list_chm_entries(&resolved) {
            Ok(entries) => entries,
            Err(err) => {
                let item_id = chm_path.replace('\\', "/");
                if seen.insert(item_id.to_ascii_lowercase()) {
                    pages.push(SsedHanreiPage {
                        item_id: item_id.clone(),
                        label: ssed_hanrei_page_label(&item_id),
                        resource: InternalResource::PackageFile {
                            path: item_id,
                            resource_kind: ResourceKind::Other,
                        },
                        anchor: None,
                        diagnostics: vec![Diagnostic::info(
                            "ssed_hanrei_chm_deferred",
                            format!("HANREI.chm was found, but CHM decoding failed: {err}"),
                        )],
                    });
                }
                return Ok(());
            }
        };
        entries.sort_by_key(|entry| chm_hanrei_entry_sort_key(&entry.path));
        let mut hhc_items = Vec::new();
        for entry in &entries {
            if !path_has_extension(&entry.path, &["hhc"]) {
                continue;
            }
            if let Ok(bytes) = read_chm_entry(&resolved, &entry.path) {
                let html = decode_package_html_text(&bytes);
                hhc_items.extend(parse_chm_hhc_toc(&html));
            }
        }
        let mut html_count = 0usize;
        for entry in entries.iter().filter(|entry| {
            path_has_extension(&entry.path, &["html", "htm"])
                && chm_hanrei_entry_sort_key(&entry.path).0 == 0
        }) {
            if self.push_ssed_hanrei_chm_entry_page(
                chm_path,
                &entry.path,
                None,
                None,
                pages,
                seen,
            )? {
                html_count += 1;
            }
        }
        for item in hhc_items {
            let Some(local) = item.local.as_deref() else {
                continue;
            };
            let Some(reference) = chm_local_reference(local) else {
                continue;
            };
            if !path_has_extension(&reference.path, &["html", "htm"]) {
                continue;
            }
            if self.push_ssed_hanrei_chm_entry_page(
                chm_path,
                &reference.path,
                reference.anchor,
                Some(item.name),
                pages,
                seen,
            )? {
                html_count += 1;
            }
        }
        for entry in entries {
            if !path_has_extension(&entry.path, &["html", "htm"]) {
                continue;
            }
            if self.push_ssed_hanrei_chm_entry_page(
                chm_path,
                &entry.path,
                None,
                None,
                pages,
                seen,
            )? {
                html_count += 1;
            }
        }
        if html_count == 0 {
            let item_id = chm_path.replace('\\', "/");
            if seen.insert(item_id.to_ascii_lowercase()) {
                pages.push(SsedHanreiPage {
                    item_id: item_id.clone(),
                    label: ssed_hanrei_page_label(&item_id),
                    resource: InternalResource::PackageFile {
                        path: item_id,
                        resource_kind: ResourceKind::Other,
                    },
                    anchor: None,
                    diagnostics: vec![Diagnostic::info(
                        "ssed_hanrei_chm_deferred",
                        "HANREI.chm was found, but no HTML entries were discovered",
                    )],
                });
            }
        }
        Ok(())
    }

    pub(super) fn push_ssed_hanrei_chm_entry_page(
        &self,
        chm_path: &str,
        entry_path: &str,
        anchor: Option<String>,
        label: Option<String>,
        pages: &mut Vec<SsedHanreiPage>,
        seen: &mut BTreeSet<String>,
    ) -> Result<bool> {
        let item_id = if let Some(anchor) = &anchor {
            format!("{chm_path}!/{entry_path}#{anchor}")
        } else {
            format!("{chm_path}!/{entry_path}")
        };
        if !seen.insert(item_id.to_ascii_lowercase()) {
            return Ok(false);
        }
        pages.push(SsedHanreiPage {
            item_id: item_id.clone(),
            label: label.unwrap_or_else(|| ssed_hanrei_page_label(&item_id)),
            resource: InternalResource::ChmFile {
                chm_path: chm_path.to_owned(),
                entry_path: entry_path.to_owned(),
                resource_kind: ResourceKind::Html,
            },
            anchor,
            diagnostics: Vec::new(),
        });
        Ok(true)
    }
}

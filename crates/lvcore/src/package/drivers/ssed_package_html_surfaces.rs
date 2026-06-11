use super::*;

pub(super) const SSED_EXINFO_INDEX_URL_SURFACE_ID: &str = "ssed-exinfo-index-url";

#[derive(Debug, Clone)]
pub(super) struct SsedPackageHtmlSource {
    pub(super) path: String,
    pub(super) title: String,
}

impl ReaderBookPackage {
    pub(super) fn ssed_exinfo_index_url_source(&self) -> Result<Option<SsedPackageHtmlSource>> {
        let relative = Path::new("EXINFO.INI");
        if !self.storage.exists(relative)? {
            return Ok(None);
        }
        let exinfo = self.storage.read(relative)?;
        let Some(index_url) = crate::ssed_panel::exinfo_general_value(&exinfo, "INDEXURL") else {
            return Ok(None);
        };
        let Some(path) = self.resolve_exinfo_index_url_path(&index_url)? else {
            return Ok(None);
        };
        let bytes = self.read_package_file_bytes(&path)?;
        let html = decode_package_html_text(&bytes);
        if !ssed_exinfo_index_url_html_is_standalone(&html) {
            return Ok(None);
        }
        let title = html_document_label(&html).unwrap_or_else(|| "Package HTML".to_owned());
        Ok(Some(SsedPackageHtmlSource { path, title }))
    }

    pub(super) fn open_ssed_exinfo_index_url_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        if limit == 0 {
            return Ok(NavigationSurface::InfoPages {
                surface_id: surface_id.to_owned(),
                pages: Vec::new(),
                next_cursor: None,
            });
        }
        let Some(source) = self.ssed_exinfo_index_url_source()? else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_exinfo_index_url_missing",
                    "EXINFO.INI INDEXURL did not expose a renderable package HTML page",
                )],
            });
        };
        let offset = decode_offset_cursor(cursor);
        if offset > 0 {
            return Ok(NavigationSurface::InfoPages {
                surface_id: surface_id.to_owned(),
                pages: Vec::new(),
                next_cursor: None,
            });
        }
        Ok(NavigationSurface::InfoPages {
            surface_id: surface_id.to_owned(),
            pages: vec![self.ssed_package_html_navigation_item(&source)?],
            next_cursor: None,
        })
    }

    fn resolve_exinfo_index_url_path(&self, index_url: &str) -> Result<Option<String>> {
        let Some(reference) = package_relative_html_reference("", index_url) else {
            return Ok(None);
        };
        if !path_has_extension(&reference.path, &["html", "htm"]) {
            return Ok(None);
        }
        let mut candidates = vec![reference.path.clone()];
        if !reference.path.contains('/') {
            candidates.push(format!("Templates/{}", reference.path));
        }
        for path in candidates {
            if self.resolve_package_file_path(&path)?.is_some() {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    fn ssed_package_html_navigation_item(
        &self,
        source: &SsedPackageHtmlSource,
    ) -> Result<NavigationItem> {
        let resource = ResourceToken::new(&InternalResource::PackageFile {
            path: source.path.clone(),
            resource_kind: ResourceKind::Html,
        })?;
        let target = TargetToken::new(&InternalTarget::Resource {
            resource,
            anchor: None,
        })?;
        Ok(NavigationItem {
            href: String::new(),
            item_id: source.path.clone(),
            label_html: escape_plain_label_html(&source.title),
            label_text: source.title.clone(),
            target,
            diagnostics: vec![
                Diagnostic::info(
                    "ssed_exinfo_index_url",
                    "EXINFO.INI INDEXURL exposes a package HTML start page",
                )
                .with_context("path", &source.path),
            ],
        })
    }
}

pub(super) fn is_ssed_exinfo_index_url_surface_id(surface_id: &str) -> bool {
    surface_id == SSED_EXINFO_INDEX_URL_SURFACE_ID
}

fn ssed_exinfo_index_url_html_is_standalone(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    ![
        "<!--index-->",
        "<!--css-->",
        "<!--honbun-->",
        "<!--body-->",
        "<!--mp3-->",
    ]
    .iter()
    .any(|placeholder| lower.contains(placeholder))
}

use super::*;

pub(super) const SSED_EXINFO_INDEX_URL_SURFACE_ID: &str = "ssed-exinfo-index-url";
const SSED_EXINFO_AUX_HTML_SURFACE_PREFIX: &str = "aux-html:";

#[derive(Debug, Clone)]
pub(super) struct SsedPackageHtmlSource {
    pub(super) path: String,
    pub(super) title: String,
}

#[derive(Debug, Clone)]
pub(super) struct SsedAuxPackageHtmlSource {
    pub(super) index: usize,
    pub(super) surface_id: String,
    pub(super) source: SsedPackageHtmlSource,
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
        let Some(path) = self.resolve_exinfo_package_html_path(&index_url)? else {
            return Ok(None);
        };
        self.ssed_package_html_source_from_path(&path, None)
    }

    pub(super) fn ssed_exinfo_aux_html_sources(&self) -> Result<Vec<SsedAuxPackageHtmlSource>> {
        let mut sources = Vec::new();
        for spec in self.ssed_aux_index_specs()? {
            if !path_has_extension(&spec.info, &["html", "htm"]) {
                continue;
            }
            let Some(path) = self.resolve_exinfo_package_html_path(&spec.info)? else {
                continue;
            };
            let fallback_title = (!spec.name.trim().is_empty()).then_some(spec.name.as_str());
            let Some(source) = self.ssed_package_html_source_from_path(&path, fallback_title)?
            else {
                continue;
            };
            sources.push(SsedAuxPackageHtmlSource {
                index: spec.index,
                surface_id: ssed_exinfo_aux_html_surface_id(spec.index),
                source,
            });
        }
        Ok(sources)
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
            pages: vec![self.ssed_package_html_navigation_item(
                &source,
                "ssed_exinfo_index_url",
                "EXINFO.INI INDEXURL exposes a package HTML start page",
            )?],
            next_cursor: None,
        })
    }

    pub(super) fn open_ssed_exinfo_aux_html_surface(
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
        let Some(index) = ssed_exinfo_aux_html_index_from_surface_id(surface_id) else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "ssed_auxiliary_html_invalid_surface",
                    "EXINFO auxiliary HTML surface id is malformed",
                )],
            });
        };
        let Some(source) = self
            .ssed_exinfo_aux_html_sources()?
            .into_iter()
            .find(|source| source.index == index)
        else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_auxiliary_html_missing",
                    "EXINFO auxiliary HTML page was not found or is not standalone",
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
            pages: vec![self.ssed_package_html_navigation_item(
                &source.source,
                "ssed_auxiliary_html",
                "EXINFO auxiliary IDXINFO exposes a package HTML page",
            )?],
            next_cursor: None,
        })
    }

    fn resolve_exinfo_package_html_path(&self, raw_path: &str) -> Result<Option<String>> {
        let Some(reference) = package_relative_html_reference("", raw_path) else {
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

    fn ssed_package_html_source_from_path(
        &self,
        path: &str,
        fallback_title: Option<&str>,
    ) -> Result<Option<SsedPackageHtmlSource>> {
        let bytes = self.read_package_file_bytes(path)?;
        let html = decode_package_html_text(&bytes);
        if !ssed_exinfo_package_html_is_standalone(&html) {
            return Ok(None);
        }
        let title = html_document_label(&html)
            .or_else(|| {
                fallback_title
                    .map(str::trim)
                    .filter(|title| !title.is_empty())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| "Package HTML".to_owned());
        Ok(Some(SsedPackageHtmlSource {
            path: path.to_owned(),
            title,
        }))
    }

    fn ssed_package_html_navigation_item(
        &self,
        source: &SsedPackageHtmlSource,
        diagnostic_code: &'static str,
        diagnostic_message: &'static str,
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
                Diagnostic::info(diagnostic_code, diagnostic_message)
                    .with_context("path", &source.path),
            ],
        })
    }
}

pub(super) fn is_ssed_exinfo_index_url_surface_id(surface_id: &str) -> bool {
    surface_id == SSED_EXINFO_INDEX_URL_SURFACE_ID
}

pub(super) fn is_ssed_exinfo_aux_html_surface_id(surface_id: &str) -> bool {
    ssed_exinfo_aux_html_index_from_surface_id(surface_id).is_some()
}

fn ssed_exinfo_aux_html_surface_id(index: usize) -> String {
    format!("{SSED_EXINFO_AUX_HTML_SURFACE_PREFIX}{index}")
}

fn ssed_exinfo_aux_html_index_from_surface_id(surface_id: &str) -> Option<usize> {
    surface_id
        .strip_prefix(SSED_EXINFO_AUX_HTML_SURFACE_PREFIX)?
        .parse::<usize>()
        .ok()
}

fn ssed_exinfo_package_html_is_standalone(html: &str) -> bool {
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

use super::*;

impl ReaderBookPackage {
    pub(super) fn open_ssed_panel_surface(
        &self,
        surface_id: &str,
        options: &LabelOptions,
    ) -> Result<NavigationSurface> {
        let Some(metadata) = self.read_ssed_panel_metadata()? else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_panels_missing",
                    "Panels.xml, Panels.plist, or mobile menu.plist was not found",
                )],
            });
        };
        let parsed = match metadata.parse() {
            Ok(parsed) => parsed,
            Err(error) => {
                return Ok(NavigationSurface::Deferred {
                    surface_id: surface_id.to_owned(),
                    diagnostics: vec![Diagnostic::warning(
                        "ssed_panels_metadata_parse_failed",
                        format!("{} could not be parsed: {error}", metadata.label),
                    )],
                });
            }
        };
        let requested_panel_id = surface_id
            .strip_prefix("panels:")
            .filter(|id| !id.is_empty());
        let root_panel_id = requested_panel_id.or_else(|| {
            parsed
                .inline_cells
                .first()
                .map(|cell| cell.panel_id.as_str())
        });
        let inline_cells = parsed
            .inline_cells
            .iter()
            .filter(|cell| root_panel_id.is_none_or(|panel_id| cell.panel_id == panel_id))
            .cloned()
            .collect::<Vec<_>>();
        let include_external_bins = requested_panel_id.is_some() || inline_cells.is_empty();
        let mut diagnostics = Vec::new();
        let mut cells = Vec::new();
        for cell in inline_cells {
            cells.push(ssed_panel_inline_cell_to_navigation_cell(
                self,
                &cell,
                &options.gaiji_policy,
            )?);
        }
        for data_ref in parsed.data_refs.into_iter().filter(|data_ref| {
            include_external_bins
                && requested_panel_id.is_none_or(|panel_id| data_ref.panel_id == panel_id)
        }) {
            let Some(data) = self.read_ssed_panel_bin_bytes(&data_ref.filename)? else {
                diagnostics.push(Diagnostic::warning(
                    "ssed_panel_bin_missing",
                    format!("Panel BIN {} was not found", data_ref.filename),
                ));
                continue;
            };
            let panel = match parse_panel_bin(&data) {
                Ok(panel) => panel,
                Err(error) => {
                    diagnostics.push(Diagnostic::warning(
                        "ssed_panel_bin_parse_failed",
                        format!(
                            "Panel BIN {} could not be parsed: {error}",
                            data_ref.filename
                        ),
                    ));
                    continue;
                }
            };
            for record in panel.records {
                cells.push(ssed_panel_bin_record_to_navigation_cell(
                    self,
                    &data_ref,
                    &record,
                    &mut diagnostics,
                    &options.gaiji_policy,
                )?);
            }
        }
        if cells.is_empty() {
            if diagnostics.is_empty() {
                diagnostics.push(Diagnostic::info(
                    "ssed_panels_empty",
                    "Panels.xml did not expose inline cells or decoded BIN rows",
                ));
            }
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics,
            });
        }
        Ok(NavigationSurface::Panel {
            surface_id: surface_id.to_owned(),
            cells,
        })
    }

    fn read_ssed_panel_bin_bytes(&self, filename: &str) -> Result<Option<Vec<u8>>> {
        let relative = filename.replace('\\', "/");
        let relative_path = Path::new(&relative);
        if self.storage.exists(relative_path)? {
            return self.storage.read(relative_path).map(Some);
        }
        if let Some(stripped) = relative.strip_prefix("Panel/")
            && let Some(package_name) = self.root.file_name().and_then(|name| name.to_str())
        {
            let sibling_panel_root = self.root.with_file_name(format!("{package_name}_Panel"));
            if sibling_panel_root.is_dir() {
                let sibling_storage = DirectoryStorage::new(sibling_panel_root);
                let stripped_path = Path::new(stripped);
                if sibling_storage.exists(stripped_path)? {
                    return sibling_storage.read(stripped_path).map(Some);
                }
            }
        }
        if let Some(stripped) = relative.strip_prefix("bin/")
            && let Some(parent) = self.root.parent()
        {
            let candidate = parent.join("bin").join(stripped);
            if regular_file_inside_root(parent, &candidate).unwrap_or(false) && candidate.is_file()
            {
                return fs::read(candidate).map(Some).map_err(Error::from);
            }
        }
        Ok(None)
    }

    pub(super) fn has_ssed_panel_metadata(&self) -> Result<bool> {
        self.read_ssed_panel_metadata()
            .map(|metadata| metadata.is_some())
    }

    fn read_ssed_panel_metadata(&self) -> Result<Option<SsedPanelMetadata>> {
        for path in [
            "Panels.xml",
            "Panels.plist",
            "menu.plist",
            "menu_.plist",
            "menu_iPad.plist",
        ] {
            let relative = Path::new(path);
            if self.storage.exists(relative)? {
                return Ok(Some(SsedPanelMetadata {
                    label: path.to_owned(),
                    bytes: self.storage.read(relative)?,
                    format: panel_metadata_format(path),
                }));
            }
        }
        let Some(parent) = self.root.parent() else {
            return Ok(None);
        };
        for path in [
            "Panels.plist",
            "menu.plist",
            "menu_.plist",
            "menu_iPad.plist",
        ] {
            let candidate = parent.join(path);
            if regular_file_inside_root(parent, &candidate).unwrap_or(false) && candidate.is_file()
            {
                return Ok(Some(SsedPanelMetadata {
                    label: path.to_owned(),
                    bytes: fs::read(candidate)?,
                    format: panel_metadata_format(path),
                }));
            }
        }
        Ok(None)
    }
}

struct SsedPanelMetadata {
    label: String,
    bytes: Vec<u8>,
    format: SsedPanelMetadataFormat,
}

enum SsedPanelMetadataFormat {
    Xml,
    Plist,
}

impl SsedPanelMetadata {
    fn parse(&self) -> Result<crate::ssed_panel::SsedPanelXml> {
        match self.format {
            SsedPanelMetadataFormat::Xml => parse_panel_xml_bytes(&self.bytes),
            SsedPanelMetadataFormat::Plist => parse_panel_plist_bytes(&self.bytes, &self.label),
        }
    }
}

fn panel_metadata_format(path: &str) -> SsedPanelMetadataFormat {
    if path.to_ascii_lowercase().ends_with(".plist") {
        SsedPanelMetadataFormat::Plist
    } else {
        SsedPanelMetadataFormat::Xml
    }
}

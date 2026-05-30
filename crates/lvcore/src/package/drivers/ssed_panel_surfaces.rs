use super::*;

impl ReaderBookPackage {
    pub(super) fn open_ssed_panel_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        if !self.storage.exists(Path::new("Panels.xml"))? {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_panels_missing",
                    "Panels.xml was not found",
                )],
            });
        }
        let parsed = match parse_panel_xml_bytes(&self.storage.read(Path::new("Panels.xml"))?) {
            Ok(parsed) => parsed,
            Err(error) => {
                return Ok(NavigationSurface::Deferred {
                    surface_id: surface_id.to_owned(),
                    diagnostics: vec![Diagnostic::warning(
                        "ssed_panels_xml_parse_failed",
                        format!("Panels.xml could not be parsed: {error}"),
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
            cells.push(ssed_panel_inline_cell_to_navigation_cell(self, &cell)?);
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
        let Some(stripped) = relative.strip_prefix("Panel/") else {
            return Ok(None);
        };
        let Some(package_name) = self.root.file_name().and_then(|name| name.to_str()) else {
            return Ok(None);
        };
        let sibling_panel_root = self.root.with_file_name(format!("{package_name}_Panel"));
        if !sibling_panel_root.is_dir() {
            return Ok(None);
        }
        let sibling_storage = DirectoryStorage::new(sibling_panel_root);
        let stripped_path = Path::new(stripped);
        if sibling_storage.exists(stripped_path)? {
            return sibling_storage.read(stripped_path).map(Some);
        }
        Ok(None)
    }
}

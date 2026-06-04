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
        let requested_panel_id = surface_id
            .strip_prefix("panels:")
            .filter(|id| !id.is_empty());
        let parsed = match metadata.parse(self, requested_panel_id) {
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
        let selected_data_refs = parsed
            .data_refs
            .iter()
            .filter(|data_ref| {
                include_external_bins
                    && requested_panel_id.is_none_or(|panel_id| data_ref.panel_id == panel_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut missing_data_refs = Vec::new();
        for data_ref in &selected_data_refs {
            if matches!(
                self.append_ssed_panel_bin_cells(
                    data_ref,
                    &mut cells,
                    &mut diagnostics,
                    &options.gaiji_policy,
                )?,
                SsedPanelBinLoadStatus::Missing
            ) {
                missing_data_refs.push(data_ref.clone());
            }
        }
        if cells.is_empty()
            && requested_panel_id.is_some()
            && selected_data_refs
                .iter()
                .any(ssed_panel_data_ref_is_aggregate)
        {
            let mut aggregate_source_count = 0usize;
            for data_ref in parsed.data_refs.iter().filter(|data_ref| {
                !ssed_panel_data_ref_is_aggregate(data_ref)
                    && !selected_data_refs.iter().any(|selected| {
                        selected.panel_id == data_ref.panel_id
                            && selected.filename == data_ref.filename
                    })
            }) {
                if matches!(
                    self.append_ssed_panel_bin_cells(
                        data_ref,
                        &mut cells,
                        &mut diagnostics,
                        &options.gaiji_policy,
                    )?,
                    SsedPanelBinLoadStatus::Decoded
                ) {
                    aggregate_source_count += 1;
                }
            }
            if aggregate_source_count > 0 {
                missing_data_refs.clear();
                diagnostics.push(
                    Diagnostic::info(
                        "ssed_panel_aggregate_synthesized",
                        "missing aggregate Panel BIN was synthesized from available content BIN panels",
                    )
                    .with_context("source_count", aggregate_source_count.to_string()),
                );
            }
        }
        for data_ref in missing_data_refs {
            diagnostics.push(Diagnostic::warning(
                "ssed_panel_bin_missing",
                format!("Panel BIN {} was not found", data_ref.filename),
            ));
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

    fn append_ssed_panel_bin_cells(
        &self,
        data_ref: &SsedPanelDataRef,
        cells: &mut Vec<PanelCell>,
        diagnostics: &mut Vec<Diagnostic>,
        gaiji_policy: &GaijiPolicy,
    ) -> Result<SsedPanelBinLoadStatus> {
        let Some(data) = self.read_ssed_panel_bin_bytes(&data_ref.filename)? else {
            return Ok(SsedPanelBinLoadStatus::Missing);
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
                return Ok(SsedPanelBinLoadStatus::ParseFailed);
            }
        };
        for record in &panel.records {
            let next_record = nearest_higher_panel_record(&panel.records, record);
            cells.push(ssed_panel_bin_record_to_navigation_cell(
                self,
                data_ref,
                record,
                next_record,
                diagnostics,
                gaiji_policy,
            )?);
        }
        Ok(SsedPanelBinLoadStatus::Decoded)
    }

    fn read_ssed_panel_bin_bytes(&self, filename: &str) -> Result<Option<Vec<u8>>> {
        let relative = filename.replace('\\', "/");
        let relative_path = Path::new(&relative);
        if self.storage.exists(relative_path)? {
            return self.storage.read(relative_path).map(Some);
        }
        if let Some(stripped) = relative.strip_prefix("Panel/")
            && let Some(package_name) = self.root.file_name().and_then(|name| name.to_str())
            && let Some(parent) = self.root.parent()
        {
            let sibling_panel_root = parent.join(format!("{package_name}_Panel"));
            if regular_directory_inside_root(parent, &sibling_panel_root).unwrap_or(false) {
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
            if regular_file_inside_root(parent, &candidate).unwrap_or(false) {
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
            if regular_file_inside_root(parent, &candidate).unwrap_or(false) {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SsedPanelBinLoadStatus {
    Decoded,
    Missing,
    ParseFailed,
}

fn ssed_panel_data_ref_is_aggregate(data_ref: &SsedPanelDataRef) -> bool {
    let title = data_ref.title.trim();
    if title == "すべて" || title.eq_ignore_ascii_case("all") {
        return true;
    }
    let filename = data_ref.filename.replace('\\', "/");
    let Some(stem) = Path::new(&filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
    else {
        return false;
    };
    let stem = stem.to_ascii_lowercase();
    stem == "all" || stem.ends_with("_all") || stem.ends_with("-all")
}

fn nearest_higher_panel_record<'a>(
    records: &'a [SsedPanelBinRecord],
    record: &SsedPanelBinRecord,
) -> Option<&'a SsedPanelBinRecord> {
    records
        .iter()
        .filter(|candidate| candidate.block != 0 || candidate.offset != 0)
        .filter(|candidate| (candidate.block, candidate.offset) > (record.block, record.offset))
        .min_by_key(|candidate| (candidate.block, candidate.offset))
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
    fn parse(
        &self,
        package: &ReaderBookPackage,
        requested_panel_id: Option<&str>,
    ) -> Result<crate::ssed_panel::SsedPanelXml> {
        match self.format {
            SsedPanelMetadataFormat::Xml => parse_panel_xml_bytes(&self.bytes),
            SsedPanelMetadataFormat::Plist => package
                .cached_ssed_panel_plist(&self.bytes, &self.label)
                .and_then(|value| parse_panel_plist_value_for_panel(value, requested_panel_id)),
        }
    }
}

impl ReaderBookPackage {
    fn cached_ssed_panel_plist(&self, bytes: &[u8], label: &str) -> Result<&PlistValue> {
        let cached = self.ssed_panel_plist.get_or_init(|| {
            parse_xml_plist(bytes, label)
                .map(Some)
                .map_err(|error| error.to_string())
        });
        match cached {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(Error::Driver("cached panel plist is missing".to_owned())),
            Err(error) => Err(Error::Driver(error.clone())),
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

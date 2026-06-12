use super::*;

impl ReaderBookPackage {
    pub(super) fn open_ssed_panel_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
        options: &LabelOptions,
    ) -> Result<NavigationSurface> {
        if limit == 0 {
            return Ok(NavigationSurface::Panel {
                surface_id: surface_id.to_owned(),
                cells: Vec::new(),
                next_cursor: None,
            });
        }
        let Some(request) = ssed_panel_surface_request(surface_id) else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_panels_surface_id_unrecognized",
                    "panel surface id is not recognized",
                )],
            });
        };
        let Some(metadata) = self.read_ssed_panel_metadata_for_surface(&request.base_surface_id)?
        else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_panels_missing",
                    "Panels.xml, Panels.plist, or mobile menu.plist was not found",
                )],
            });
        };
        let requested_panel_id = request.requested_panel_id.as_deref();
        let mut parsed = match metadata.parse(self, requested_panel_id) {
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
        if request
            .base_surface_id
            .starts_with(super::ssed_ios_plist_surfaces::IOS_PLIST_PANEL_PREFIX)
        {
            self.attach_implicit_ios_panel_bin_refs(&mut parsed, &metadata, requested_panel_id)?;
        }
        let root_panel_id = requested_panel_id.or_else(|| {
            parsed
                .inline_cells
                .first()
                .map(|cell| cell.panel_id.as_str())
        });
        let known_panel_ids = ssed_panel_known_panel_ids(&parsed);
        let inline_cells = parsed
            .inline_cells
            .iter()
            .filter(|cell| root_panel_id.is_none_or(|panel_id| cell.panel_id == panel_id))
            .cloned()
            .collect::<Vec<_>>();
        let include_external_bins = requested_panel_id.is_some() || inline_cells.is_empty();
        let mut diagnostics = Vec::new();
        let mut builder = PanelCellPageBuilder::new(decode_offset_cursor(cursor), limit);
        for cell in inline_cells {
            builder.push_cell(|| {
                ssed_panel_inline_cell_to_navigation_cell(
                    self,
                    &cell,
                    &known_panel_ids,
                    &request.base_surface_id,
                    &options.gaiji_policy,
                )
            })?;
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
            if !ssed_panel_data_ref_is_bin(data_ref) {
                if ssed_panel_data_ref_is_plist(data_ref) {
                    if matches!(
                        self.append_ssed_panel_plist_cells(
                            data_ref,
                            &mut builder,
                            &mut diagnostics,
                            &options.gaiji_policy,
                            &request.base_surface_id,
                        )?,
                        SsedPanelPlistLoadStatus::Missing
                    ) {
                        diagnostics.push(Diagnostic::warning(
                            "ssed_panel_plist_missing",
                            format!("Panel plist {} was not found", data_ref.filename),
                        ));
                    }
                } else if ssed_panel_data_ref_is_html(data_ref) {
                    builder.push_cell(|| {
                        self.ssed_panel_external_html_data_cell(
                            data_ref,
                            &options.gaiji_policy,
                            &mut diagnostics,
                        )
                    })?;
                } else {
                    diagnostics.push(
                        Diagnostic::info(
                            "ssed_panel_external_data_deferred",
                            format!(
                                "Panel external data {} has type {}; only BIN and HTML rows are decoded",
                                data_ref.filename,
                                display_panel_data_type(&data_ref.data_type)
                            ),
                        )
                        .with_context("type", display_panel_data_type(&data_ref.data_type))
                        .with_context("filename", &data_ref.filename),
                    );
                }
                continue;
            }
            if matches!(
                self.append_ssed_panel_bin_cells(
                    data_ref,
                    &mut builder,
                    &mut diagnostics,
                    &options.gaiji_policy,
                )?,
                SsedPanelBinLoadStatus::Missing
            ) {
                missing_data_refs.push(data_ref.clone());
            }
            if builder.is_page_full() {
                break;
            }
        }
        if builder.total_seen() == 0
            && requested_panel_id.is_some()
            && selected_data_refs
                .iter()
                .any(ssed_panel_data_ref_is_aggregate)
        {
            let mut aggregate_source_count = 0usize;
            for data_ref in parsed.data_refs.iter().filter(|data_ref| {
                ssed_panel_data_ref_is_bin(data_ref)
                    && !ssed_panel_data_ref_is_aggregate(data_ref)
                    && !selected_data_refs.iter().any(|selected| {
                        selected.panel_id == data_ref.panel_id
                            && selected.filename == data_ref.filename
                    })
            }) {
                if matches!(
                    self.append_ssed_panel_bin_cells(
                        data_ref,
                        &mut builder,
                        &mut diagnostics,
                        &options.gaiji_policy,
                    )?,
                    SsedPanelBinLoadStatus::Decoded
                ) {
                    aggregate_source_count += 1;
                }
                if builder.is_page_full() {
                    break;
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
        if builder.total_seen() == 0 {
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
        let (cells, next_cursor) = builder.finish();
        Ok(NavigationSurface::Panel {
            surface_id: surface_id.to_owned(),
            cells,
            next_cursor,
        })
    }

    fn attach_implicit_ios_panel_bin_refs(
        &self,
        parsed: &mut crate::ssed_panel::SsedPanelXml,
        metadata: &SsedPanelMetadata,
        requested_panel_id: Option<&str>,
    ) -> Result<()> {
        if parsed.inline_cells.is_empty()
            && parsed.data_refs.is_empty()
            && let Some(panel_id) = requested_panel_id
        {
            let root = metadata.parse(self, None)?;
            if let Some(data_ref) = root.inline_cells.iter().find_map(|cell| {
                let child_panel_id = implicit_ios_panel_child_id(cell);
                (child_panel_id == panel_id)
                    .then(|| self.implicit_ios_panel_data_ref(panel_id, &cell.label, &cell.label))
            }) && self.read_ssed_panel_bin_bytes(&data_ref)?.is_some()
            {
                parsed.data_refs.push(data_ref);
            }
            return Ok(());
        }

        let mut existing_refs = parsed
            .data_refs
            .iter()
            .map(|data_ref| (data_ref.panel_id.clone(), data_ref.filename.clone()))
            .collect::<BTreeSet<_>>();
        let mut additions = Vec::new();
        for cell in &mut parsed.inline_cells {
            if !cell.ref_id.trim().is_empty()
                || !cell.action_verb.trim().is_empty()
                || cell.target_block.is_some()
                || cell.label.trim().is_empty()
            {
                continue;
            }
            let child_panel_id = implicit_ios_panel_child_id(cell);
            let data_ref =
                self.implicit_ios_panel_data_ref(&child_panel_id, &cell.label, &cell.label);
            if !existing_refs.insert((data_ref.panel_id.clone(), data_ref.filename.clone())) {
                continue;
            }
            if self.read_ssed_panel_bin_bytes(&data_ref)?.is_some() {
                cell.ref_id = child_panel_id;
                additions.push(data_ref);
            }
        }
        parsed.data_refs.extend(additions);
        Ok(())
    }

    fn implicit_ios_panel_data_ref(
        &self,
        panel_id: &str,
        title: &str,
        label: &str,
    ) -> SsedPanelDataRef {
        let stem = root_level_product_idx_code(&self.root)
            .map(|code| format!("{code}_{label}"))
            .unwrap_or_else(|| label.to_owned());
        SsedPanelDataRef {
            panel_id: panel_id.to_owned(),
            panel_type: "contents".to_owned(),
            title: title.to_owned(),
            filename: format!("bin/{stem}.bin"),
            data_type: "bin".to_owned(),
        }
    }

    fn append_ssed_panel_bin_cells(
        &self,
        data_ref: &SsedPanelDataRef,
        builder: &mut PanelCellPageBuilder,
        diagnostics: &mut Vec<Diagnostic>,
        gaiji_policy: &GaijiPolicy,
    ) -> Result<SsedPanelBinLoadStatus> {
        let Some(data) = self.read_ssed_panel_bin_bytes(data_ref)? else {
            return Ok(SsedPanelBinLoadStatus::Missing);
        };
        if let Some(page) = parse_panel_bin_page(
            &data,
            builder.local_offset_for_collection(),
            builder.remaining_capacity_with_lookahead(),
        )? {
            let total_count = page.actual_record_count as usize;
            let local_start = builder.local_offset_for_collection().min(total_count);
            if local_start >= total_count {
                builder.skip_unbuilt(total_count);
                return Ok(SsedPanelBinLoadStatus::Decoded);
            }
            builder.skip_unbuilt(local_start);
            let decoded_count = page.records.len();
            for page_record in &page.records {
                builder.push_cell(|| {
                    ssed_panel_bin_record_to_navigation_cell(
                        self,
                        data_ref,
                        &page_record.record,
                        page_record.next_record.as_ref(),
                        diagnostics,
                        gaiji_policy,
                    )
                })?;
            }
            if local_start.saturating_add(decoded_count) < total_count {
                builder.mark_has_more();
            }
            return Ok(SsedPanelBinLoadStatus::Decoded);
        }
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
        let sorted_targets = sorted_panel_record_targets(&panel.records);
        for record in &panel.records {
            builder.push_cell(|| {
                let next_record =
                    nearest_higher_panel_record(&panel.records, &sorted_targets, record);
                ssed_panel_bin_record_to_navigation_cell(
                    self,
                    data_ref,
                    record,
                    next_record,
                    diagnostics,
                    gaiji_policy,
                )
            })?;
        }
        Ok(SsedPanelBinLoadStatus::Decoded)
    }

    fn append_ssed_panel_plist_cells(
        &self,
        data_ref: &SsedPanelDataRef,
        builder: &mut PanelCellPageBuilder,
        diagnostics: &mut Vec<Diagnostic>,
        gaiji_policy: &GaijiPolicy,
        base_surface_id: &str,
    ) -> Result<SsedPanelPlistLoadStatus> {
        let Some(data) = self.read_ssed_panel_plist_bytes(data_ref)? else {
            return Ok(SsedPanelPlistLoadStatus::Missing);
        };
        let label = format!("Panel plist {}", data_ref.filename);
        let parsed = self.cached_ssed_panel_plist_panel(&data, &label, None)?;
        let root_panel_id = parsed
            .inline_cells
            .first()
            .map(|cell| cell.panel_id.as_str());
        let known_panel_ids = ssed_panel_known_panel_ids(&parsed);
        for cell in parsed
            .inline_cells
            .iter()
            .filter(|cell| root_panel_id.is_none_or(|panel_id| cell.panel_id == panel_id))
        {
            builder.push_cell(|| {
                ssed_panel_inline_cell_to_navigation_cell(
                    self,
                    cell,
                    &known_panel_ids,
                    base_surface_id,
                    gaiji_policy,
                )
            })?;
            if builder.is_page_full() {
                break;
            }
        }
        diagnostics.push(
            Diagnostic::info(
                "ssed_panel_plist_child",
                "iOS mobile menu file reference was resolved from a child plist",
            )
            .with_context("filename", &data_ref.filename),
        );
        Ok(SsedPanelPlistLoadStatus::Decoded)
    }

    fn read_ssed_panel_bin_bytes(&self, data_ref: &SsedPanelDataRef) -> Result<Option<Vec<u8>>> {
        let names = panel_bin_candidate_names(&data_ref.filename, &data_ref.data_type);
        for name in &names {
            let relative_path = Path::new(name.as_str());
            if self.storage.exists(relative_path)? {
                return self.storage.read(relative_path).map(Some);
            }
        }
        for base in ["Panel", "bin"] {
            for name in &names {
                let relative = Path::new(base).join(name.as_str());
                if self.storage.exists(&relative)? {
                    return self.storage.read(&relative).map(Some);
                }
            }
        }
        let Some(parent) = self.root.parent() else {
            return Ok(None);
        };
        if let Some(package_name) = self.root.file_name().and_then(|name| name.to_str()) {
            let sibling_panel_root = parent.join(format!("{package_name}_Panel"));
            if regular_directory_inside_root(parent, &sibling_panel_root).unwrap_or(false) {
                let sibling_storage = DirectoryStorage::new(sibling_panel_root);
                for name in &names {
                    let relative_path = Path::new(name.as_str());
                    if sibling_storage.exists(relative_path)? {
                        return sibling_storage.read(relative_path).map(Some);
                    }
                }
            }
        }
        let parent_bin_root = parent.join("bin");
        if regular_directory_inside_root(parent, &parent_bin_root).unwrap_or(false) {
            let parent_bin_storage = DirectoryStorage::new(parent_bin_root);
            for name in &names {
                let relative_path = Path::new(name.as_str());
                if parent_bin_storage.exists(relative_path)? {
                    return parent_bin_storage.read(relative_path).map(Some);
                }
            }
        }
        Ok(None)
    }

    fn read_ssed_panel_plist_bytes(&self, data_ref: &SsedPanelDataRef) -> Result<Option<Vec<u8>>> {
        let names = panel_plist_candidate_names(&data_ref.filename);
        for name in &names {
            let relative_path = Path::new(name.as_str());
            if self.storage.exists(relative_path)? {
                return self.storage.read(relative_path).map(Some);
            }
        }
        let Some(parent) = self.root.parent() else {
            return Ok(None);
        };
        let parent_storage = DirectoryStorage::new(parent.to_path_buf());
        for name in &names {
            let relative_path = Path::new(name.as_str());
            if parent_storage.exists(relative_path)? {
                return parent_storage.read(relative_path).map(Some);
            }
        }
        Ok(None)
    }

    pub(super) fn has_ssed_panel_metadata(&self) -> Result<bool> {
        self.read_ssed_panel_metadata_for_surface("panels")
            .map(|metadata| metadata.is_some())
    }

    pub(super) fn ssed_panel_home_title(&self) -> Result<Option<String>> {
        self.ssed_panel_surface_title("panels")
    }

    pub(super) fn ssed_panel_surface_title(&self, surface_id: &str) -> Result<Option<String>> {
        let Some(request) = ssed_panel_surface_request(surface_id) else {
            return Ok(None);
        };
        let Some(metadata) = self.read_ssed_panel_metadata_for_surface(&request.base_surface_id)?
        else {
            return Ok(None);
        };
        let Ok(parsed) = metadata.parse(self, request.requested_panel_id.as_deref()) else {
            return Ok(None);
        };
        Ok(first_ssed_panel_title(
            &parsed,
            request.requested_panel_id.as_deref(),
        ))
    }

    fn read_ssed_panel_metadata_for_surface(
        &self,
        base_surface_id: &str,
    ) -> Result<Option<SsedPanelMetadata>> {
        if let Some(source_id) =
            base_surface_id.strip_prefix(super::ssed_ios_plist_surfaces::IOS_PLIST_PANEL_PREFIX)
        {
            if super::ssed_ios_plist_surfaces::is_ssed_ios_panel_plist_candidate(source_id)
                && let Some(source) = self.ssed_ios_plist_file_by_source_id(source_id)?
            {
                return Ok(Some(SsedPanelMetadata {
                    label: source.label,
                    bytes: source.bytes,
                    format: SsedPanelMetadataFormat::Plist,
                }));
            }
            return Ok(None);
        }
        let mut candidates = Vec::new();
        if let Some(declared_panel) = self.read_exinfo_panel_metadata_name()? {
            push_unique_panel_metadata_candidate(&mut candidates, declared_panel);
        }
        for path in [
            "Panels.xml",
            "Panels.plist",
            "menu.plist",
            "menu_.plist",
            "menu_iPad.plist",
            "Panel/Panels.xml",
            "Panel/Panels.plist",
        ] {
            push_unique_panel_metadata_candidate(&mut candidates, path.to_owned());
        }
        for path in &candidates {
            let relative = Path::new(path);
            if self.storage.exists(relative)? {
                return Ok(Some(SsedPanelMetadata {
                    label: path.clone(),
                    bytes: self.storage.read(relative)?,
                    format: panel_metadata_format(path),
                }));
            }
        }
        let Some(parent) = self.root.parent() else {
            return Ok(None);
        };
        let parent_storage = DirectoryStorage::new(parent.to_path_buf());
        for path in &candidates {
            let relative = Path::new(path);
            if parent_storage.exists(relative)? {
                return Ok(Some(SsedPanelMetadata {
                    label: path.clone(),
                    bytes: parent_storage.read(relative)?,
                    format: panel_metadata_format(path),
                }));
            }
        }
        Ok(None)
    }

    fn read_exinfo_panel_metadata_name(&self) -> Result<Option<String>> {
        let relative = Path::new("EXINFO.INI");
        if !self.storage.exists(relative)? {
            return Ok(None);
        }
        let bytes = self.storage.read(relative)?;
        Ok(crate::ssed_panel::exinfo_panel_metadata_name(&bytes)
            .map(normalize_panel_metadata_candidate))
    }

    fn ssed_panel_external_html_data_cell(
        &self,
        data_ref: &SsedPanelDataRef,
        gaiji_policy: &GaijiPolicy,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<PanelCell> {
        let rich_label = self.ssed_rich_label_with_policy(
            if data_ref.title.trim().is_empty() {
                &data_ref.filename
            } else {
                &data_ref.title
            },
            gaiji_policy,
        );
        let path = self.ssed_panel_external_html_resource_path(data_ref)?;
        if self.resolve_package_file_path(&path)?.is_none() {
            diagnostics.push(
                Diagnostic::warning(
                    "ssed_panel_external_html_missing",
                    format!("Panel HTML resource {} was not found", data_ref.filename),
                )
                .with_context("filename", &data_ref.filename),
            );
        }
        let resource = ResourceToken::new(&InternalResource::PackageFile {
            path,
            resource_kind: ResourceKind::Html,
        })?;
        let target = TargetToken::new(&InternalTarget::Resource {
            resource,
            anchor: None,
        })?;
        Ok(PanelCell {
            href: None,
            panel_id: data_ref.panel_id.clone(),
            row: 0,
            column: 0,
            label_html: rich_label.html,
            label_text: rich_label.text,
            target: Some(target),
            diagnostics: rich_label.diagnostics,
        })
    }

    fn ssed_panel_external_html_resource_path(
        &self,
        data_ref: &SsedPanelDataRef,
    ) -> Result<String> {
        let normalized = data_ref.filename.replace('\\', "/");
        for candidate in panel_html_candidate_names(&normalized) {
            if self.resolve_package_file_path(&candidate)?.is_some() {
                return Ok(candidate);
            }
        }
        Ok(normalized)
    }
}

struct SsedPanelSurfaceRequest {
    base_surface_id: String,
    requested_panel_id: Option<String>,
}

fn ssed_panel_surface_request(surface_id: &str) -> Option<SsedPanelSurfaceRequest> {
    if surface_id == "panels" {
        return Some(SsedPanelSurfaceRequest {
            base_surface_id: "panels".to_owned(),
            requested_panel_id: None,
        });
    }
    if let Some(panel_id) = surface_id.strip_prefix("panels:") {
        if panel_id.is_empty() {
            return None;
        }
        return Some(SsedPanelSurfaceRequest {
            base_surface_id: "panels".to_owned(),
            requested_panel_id: Some(panel_id.to_owned()),
        });
    }
    if let Some(rest) =
        surface_id.strip_prefix(super::ssed_ios_plist_surfaces::IOS_PLIST_PANEL_PREFIX)
    {
        if rest.is_empty() {
            return None;
        }
        let (source_id, requested_panel_id) = rest
            .split_once(':')
            .map(|(source, panel)| (source, Some(panel.to_owned())))
            .unwrap_or((rest, None));
        if source_id.is_empty() {
            return None;
        }
        return Some(SsedPanelSurfaceRequest {
            base_surface_id: format!(
                "{}{}",
                super::ssed_ios_plist_surfaces::IOS_PLIST_PANEL_PREFIX,
                source_id
            ),
            requested_panel_id,
        });
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SsedPanelBinLoadStatus {
    Decoded,
    Missing,
    ParseFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SsedPanelPlistLoadStatus {
    Decoded,
    Missing,
}

struct PanelCellPageBuilder {
    offset: usize,
    limit: usize,
    seen: usize,
    cells: Vec<PanelCell>,
    has_more: bool,
}

impl PanelCellPageBuilder {
    fn new(offset: usize, limit: usize) -> Self {
        Self {
            offset,
            limit,
            seen: 0,
            cells: Vec::with_capacity(limit.min(256)),
            has_more: false,
        }
    }

    fn push_cell(&mut self, build: impl FnOnce() -> Result<PanelCell>) -> Result<()> {
        let index = self.seen;
        self.seen = self.seen.saturating_add(1);
        if self.limit == 0 || index < self.offset {
            return Ok(());
        }
        if self.cells.len() < self.limit {
            self.cells.push(build()?);
        } else {
            self.has_more = true;
        }
        Ok(())
    }

    fn local_offset_for_collection(&self) -> usize {
        self.offset.saturating_sub(self.seen)
    }

    fn remaining_capacity_with_lookahead(&self) -> usize {
        self.limit
            .saturating_sub(self.cells.len())
            .saturating_add(1)
    }

    fn skip_unbuilt(&mut self, count: usize) {
        self.seen = self.seen.saturating_add(count);
    }

    fn mark_has_more(&mut self) {
        self.has_more = true;
    }

    fn is_page_full(&self) -> bool {
        self.has_more
    }

    fn total_seen(&self) -> usize {
        self.seen
    }

    fn finish(self) -> (Vec<PanelCell>, Option<String>) {
        let next_cursor = self
            .has_more
            .then(|| self.offset.saturating_add(self.limit).to_string());
        (self.cells, next_cursor)
    }
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

fn implicit_ios_panel_child_id(cell: &SsedPanelInlineCell) -> String {
    format!("{}.{:04}", cell.panel_id, cell.cell_index)
}

fn ssed_panel_known_panel_ids(parsed: &crate::ssed_panel::SsedPanelXml) -> BTreeSet<String> {
    parsed
        .inline_cells
        .iter()
        .map(|cell| cell.panel_id.clone())
        .chain(
            parsed
                .data_refs
                .iter()
                .map(|data_ref| data_ref.panel_id.clone()),
        )
        .collect()
}

fn ssed_panel_data_ref_is_bin(data_ref: &SsedPanelDataRef) -> bool {
    if data_ref.data_type.trim().eq_ignore_ascii_case("bin") {
        return true;
    }
    data_ref.data_type.trim().is_empty() && path_has_extension(&data_ref.filename, &["bin"])
}

fn ssed_panel_data_ref_is_html(data_ref: &SsedPanelDataRef) -> bool {
    if data_ref.data_type.trim().eq_ignore_ascii_case("html") {
        return true;
    }
    path_has_extension(&data_ref.filename, &["html", "htm"])
}

fn ssed_panel_data_ref_is_plist(data_ref: &SsedPanelDataRef) -> bool {
    if data_ref.data_type.trim().eq_ignore_ascii_case("plist") {
        return true;
    }
    path_has_extension(&data_ref.filename, &["plist"])
}

fn display_panel_data_type(data_type: &str) -> String {
    let data_type = data_type.trim();
    if data_type.is_empty() {
        "(unspecified)".to_owned()
    } else {
        data_type.to_owned()
    }
}

fn panel_html_candidate_names(filename: &str) -> Vec<String> {
    let normalized = filename.trim_start_matches('/').to_owned();
    let mut names = Vec::new();
    push_unique_panel_bin_name(&mut names, normalized.clone());
    if !normalized
        .get(.."Templates/".len())
        .is_some_and(|head| head.eq_ignore_ascii_case("Templates/"))
    {
        push_unique_panel_bin_name(&mut names, format!("Templates/{normalized}"));
    }
    if !normalized
        .get(.."HTMLs/".len())
        .is_some_and(|head| head.eq_ignore_ascii_case("HTMLs/"))
    {
        push_unique_panel_bin_name(&mut names, format!("HTMLs/{normalized}"));
    }
    names
}

fn sorted_panel_record_targets(records: &[SsedPanelBinRecord]) -> Vec<(u32, u32, usize)> {
    let mut targets = records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.block != 0 || record.offset != 0)
        .map(|(index, record)| (record.block, record.offset, index))
        .collect::<Vec<_>>();
    targets.sort_unstable();
    targets
}

fn nearest_higher_panel_record<'a>(
    records: &'a [SsedPanelBinRecord],
    sorted_targets: &[(u32, u32, usize)],
    record: &SsedPanelBinRecord,
) -> Option<&'a SsedPanelBinRecord> {
    let index = sorted_targets
        .partition_point(|(block, offset, _)| (*block, *offset) <= (record.block, record.offset));
    sorted_targets
        .get(index)
        .and_then(|(_, _, record_index)| records.get(*record_index))
}

fn first_ssed_panel_title(
    parsed: &crate::ssed_panel::SsedPanelXml,
    requested_panel_id: Option<&str>,
) -> Option<String> {
    parsed
        .inline_cells
        .iter()
        .filter(|cell| requested_panel_id.is_none_or(|panel_id| cell.panel_id == panel_id))
        .map(|cell| cell.title.trim())
        .chain(
            parsed
                .data_refs
                .iter()
                .filter(|data_ref| {
                    requested_panel_id.is_none_or(|panel_id| data_ref.panel_id == panel_id)
                })
                .map(|data_ref| data_ref.title.trim()),
        )
        .find(|title| !title.is_empty())
        .map(ToOwned::to_owned)
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
            SsedPanelMetadataFormat::Xml => package
                .cached_ssed_panel_xml(&self.bytes, &self.label)
                .cloned(),
            SsedPanelMetadataFormat::Plist => package
                .cached_ssed_panel_plist_panel(&self.bytes, &self.label, requested_panel_id)
                .map(|value| value.as_ref().clone()),
        }
    }
}

impl ReaderBookPackage {
    fn cached_ssed_panel_xml(
        &self,
        bytes: &[u8],
        label: &str,
    ) -> Result<&crate::ssed_panel::SsedPanelXml> {
        let cached = self.ssed_panel_xml.get_or_init(|| {
            parse_panel_xml_bytes(bytes)
                .map(Some)
                .map_err(|error| format!("{label}: {error}"))
        });
        match cached {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(Error::Driver("cached panel XML is missing".to_owned())),
            Err(error) => Err(Error::Driver(error.clone())),
        }
    }

    pub(super) fn cached_ssed_panel_plist(
        &self,
        bytes: &[u8],
        label: &str,
    ) -> Result<Arc<PlistValue>> {
        let cache_key = label.to_ascii_lowercase();
        let mut cache = self
            .ssed_panel_plists
            .lock()
            .map_err(|_| Error::Driver("panel plist cache lock was poisoned".to_owned()))?;
        if let Some(cached) = cache.get(&cache_key) {
            return cached
                .as_ref()
                .map(Arc::clone)
                .map_err(|error| Error::Driver(error.clone()));
        }
        let parsed = parse_xml_plist(bytes, label)
            .map(Arc::new)
            .map_err(|error| error.to_string());
        cache.insert(cache_key, parsed.clone());
        parsed.map_err(Error::Driver)
    }

    fn cached_ssed_panel_plist_panel(
        &self,
        bytes: &[u8],
        label: &str,
        requested_panel_id: Option<&str>,
    ) -> Result<Arc<crate::ssed_panel::SsedPanelXml>> {
        let cache_key = ssed_panel_parsed_cache_key(label, requested_panel_id);
        {
            let cache = self.ssed_panel_parsed.lock().map_err(|_| {
                Error::Driver("parsed panel plist cache lock was poisoned".to_owned())
            })?;
            if let Some(cached) = cache.get(&cache_key) {
                return cached
                    .as_ref()
                    .map(Arc::clone)
                    .map_err(|error| Error::Driver(error.clone()));
            }
        }

        let parsed = self
            .cached_ssed_panel_plist(bytes, label)
            .and_then(|value| parse_panel_plist_value_for_panel(&value, requested_panel_id))
            .map(Arc::new)
            .map_err(|error| error.to_string());
        let mut cache = self
            .ssed_panel_parsed
            .lock()
            .map_err(|_| Error::Driver("parsed panel plist cache lock was poisoned".to_owned()))?;
        let cached = cache.entry(cache_key).or_insert_with(|| parsed.clone());
        cached
            .as_ref()
            .map(Arc::clone)
            .map_err(|error| Error::Driver(error.clone()))
    }
}

fn ssed_panel_parsed_cache_key(label: &str, requested_panel_id: Option<&str>) -> String {
    let panel_id = requested_panel_id.unwrap_or("<root>");
    format!("{}|{panel_id}", label.to_ascii_lowercase())
}

fn panel_metadata_format(path: &str) -> SsedPanelMetadataFormat {
    if path.to_ascii_lowercase().ends_with(".plist") {
        SsedPanelMetadataFormat::Plist
    } else {
        SsedPanelMetadataFormat::Xml
    }
}

fn normalize_panel_metadata_candidate(path: String) -> String {
    path.replace('\\', "/")
}

fn push_unique_panel_metadata_candidate(candidates: &mut Vec<String>, path: String) {
    let normalized = normalize_panel_metadata_candidate(path);
    if !normalized.is_empty()
        && !candidates
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(&normalized))
    {
        candidates.push(normalized);
    }
}

fn panel_bin_candidate_names(filename: &str, data_type: &str) -> Vec<String> {
    let normalized = filename.replace('\\', "/");
    let mut names = Vec::new();
    push_unique_panel_bin_candidate(&mut names, normalized.clone(), data_type);
    if let Some(stripped) = normalized.strip_prefix("Panel/") {
        push_unique_panel_bin_candidate(&mut names, stripped.to_owned(), data_type);
    }
    if let Some(stripped) = normalized.strip_prefix("bin/") {
        push_unique_panel_bin_candidate(&mut names, stripped.to_owned(), data_type);
    }
    names
}

fn panel_plist_candidate_names(filename: &str) -> Vec<String> {
    let normalized = filename.replace('\\', "/");
    let mut names = Vec::new();
    push_unique_panel_plist_candidate(&mut names, normalized.clone());
    if let Some(stripped) = normalized.strip_prefix("list/") {
        push_unique_panel_plist_candidate(&mut names, stripped.to_owned());
    } else {
        push_unique_panel_plist_candidate(&mut names, format!("list/{normalized}"));
    }
    names
}

fn push_unique_panel_plist_candidate(names: &mut Vec<String>, name: String) {
    let name = name.trim_start_matches('/').to_owned();
    if name.is_empty() {
        return;
    }
    push_unique_panel_bin_name(names, name.clone());
    if Path::new(&name).extension().is_none() {
        push_unique_panel_bin_name(names, format!("{name}.plist"));
    }
}

fn push_unique_panel_bin_candidate(names: &mut Vec<String>, name: String, data_type: &str) {
    let name = name.trim_start_matches('/').to_owned();
    if name.is_empty() {
        return;
    }
    push_unique_panel_bin_name(names, name.clone());
    if data_type.eq_ignore_ascii_case("bin") && Path::new(&name).extension().is_none() {
        push_unique_panel_bin_name(names, format!("{name}.bin"));
    }
}

fn push_unique_panel_bin_name(names: &mut Vec<String>, name: String) {
    if !names
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(&name))
    {
        names.push(name);
    }
}

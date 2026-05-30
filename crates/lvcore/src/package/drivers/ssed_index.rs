use super::*;

impl ReaderBookPackage {
    pub(super) fn scan_ssed_simple_leaf_index_rows_near_key(
        &self,
        mode: &SearchMode,
        needle: &str,
        mut on_row: impl FnMut(SsedIndexRow) -> Result<bool>,
    ) -> Result<SsedNearKeyScanResult> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(SsedNearKeyScanResult {
                scanned_components: 0,
                needs_linear_fallback: true,
                diagnostics: vec![Diagnostic::error(
                    "ssed_catalog_missing",
                    "SSED index scanning requires a parsed SSEDINFO catalog",
                )],
            });
        };
        let mut diagnostics = Vec::new();
        let mut scanned_components = 0usize;
        let mut needs_linear_fallback = false;
        let probe = if *mode == SearchMode::Backward {
            reverse_search_match_text(needle)
        } else {
            needle.to_owned()
        };
        let needle_keys = ssed_index_search_key_candidates(&probe);
        if needle_keys.is_empty() && !probe.is_empty() {
            return Ok(SsedNearKeyScanResult {
                scanned_components: 0,
                needs_linear_fallback: true,
                diagnostics,
            });
        }
        'candidates: for needle_key in needle_keys {
            for component in catalog.components_by_role(SsedComponentRole::Index) {
                if !is_simple_leaf_index_type(component.component_type) {
                    continue;
                }
                let is_backward_index = ssed_index_component_name_is_backward(&component.filename);
                match mode {
                    SearchMode::Exact | SearchMode::Forward if is_backward_index => continue,
                    SearchMode::Backward if !is_backward_index => continue,
                    _ => {}
                }
                let path = match self.resolve_readable_ssed_component_path(component) {
                    Ok(Some(path)) => path,
                    Ok(None) => continue,
                    Err(error) => {
                        diagnostics.push(
                            Diagnostic::warning(
                                "ssed_index_component_decode_failed",
                                format!(
                                    "{} is not readable as SSEDDATA: {error}",
                                    component.filename
                                ),
                            )
                            .with_context("component", &component.filename),
                        );
                        continue;
                    }
                };
                let mut reader = SsedDataFile::open(&path)?;
                let page_count = component.block_count() as usize;
                let start_page = match self.ssed_simple_index_candidate_leaf_page(
                    component,
                    &mut reader,
                    &needle_key,
                )? {
                    Some(page_index) => page_index,
                    None => continue,
                };
                scanned_components = scanned_components.saturating_add(1);
                let mut last_key = None::<Vec<u8>>;
                'pages: for page_index in start_page..page_count {
                    let page = reader.read_range(page_index * INDEX_PAGE_SIZE, INDEX_PAGE_SIZE)?;
                    if page.len() < 4 {
                        break;
                    }
                    let word = u16::from_be_bytes([page[0], page[1]]);
                    if !is_leaf_page(word) {
                        continue;
                    }
                    let logical_block = component.start_block + page_index as u32;
                    let (rows, unknown) = parse_simple_leaf_page(
                        &component.filename,
                        &page,
                        page_index as u32,
                        logical_block,
                    );
                    if rows.windows(2).any(|pair| {
                        ssed_index_row_order_key(&pair[1]) < ssed_index_row_order_key(&pair[0])
                    }) {
                        needs_linear_fallback = true;
                    }
                    if unknown > 0 {
                        diagnostics.push(
                            Diagnostic::warning(
                                "ssed_index_unknown_leaf_bytes",
                                format!(
                                    "{} had {unknown} unknown simple leaf row(s)",
                                    component.filename
                                ),
                            )
                            .with_context("component", &component.filename),
                        );
                    }
                    for row in rows {
                        let key = ssed_index_row_match_text(&row);
                        let key_bytes = ssed_index_row_order_key(&row);
                        let key_has_needle_prefix =
                            !needle_key.is_empty() && key_bytes.starts_with(&needle_key);
                        if last_key
                            .as_ref()
                            .is_some_and(|last_key| key_bytes.as_slice() < last_key.as_slice())
                        {
                            needs_linear_fallback = true;
                        }
                        last_key = Some(key_bytes.clone());
                        let row_matches = match mode {
                            SearchMode::Exact => key == needle,
                            SearchMode::Forward => key.starts_with(needle),
                            SearchMode::Backward => key.ends_with(needle),
                            _ => false,
                        };
                        let passed_match_region = match mode {
                            SearchMode::Exact => {
                                !needs_linear_fallback
                                    && key_bytes.as_slice() > needle_key.as_slice()
                            }
                            SearchMode::Forward => {
                                !needs_linear_fallback
                                    && !key_has_needle_prefix
                                    && key_bytes.as_slice() > needle_key.as_slice()
                            }
                            SearchMode::Backward => {
                                !needs_linear_fallback
                                    && !key_has_needle_prefix
                                    && key_bytes.as_slice() > needle_key.as_slice()
                            }
                            _ => false,
                        };
                        if row_matches {
                            if !on_row(row)? {
                                break 'candidates;
                            }
                        } else if passed_match_region {
                            break 'pages;
                        }
                    }
                }
            }
        }
        Ok(SsedNearKeyScanResult {
            scanned_components,
            needs_linear_fallback,
            diagnostics,
        })
    }

    fn ssed_simple_index_candidate_leaf_page(
        &self,
        component: &SsedComponent,
        reader: &mut SsedDataFile,
        needle_key: &[u8],
    ) -> Result<Option<usize>> {
        let page_count = component.block_count() as usize;
        if page_count == 0 {
            return Ok(None);
        }
        let mut page_index = 0usize;
        let mut guard = 0usize;
        while page_index < page_count && guard <= page_count {
            guard = guard.saturating_add(1);
            let page = reader.read_range(page_index * INDEX_PAGE_SIZE, INDEX_PAGE_SIZE)?;
            if page.len() < 4 {
                return Ok(None);
            }
            let word = u16::from_be_bytes([page[0], page[1]]);
            if is_leaf_page(word) {
                return Ok(Some(page_index));
            }
            let rows = parse_internal_page(
                &component.filename,
                &page,
                page_index as u32,
                component.start_block + page_index as u32,
            );
            let Some(child_block) = rows
                .iter()
                .find(|row| {
                    row.raw_key.iter().all(|value| *value == 0xff)
                        || row.raw_key.as_slice() >= needle_key
                })
                .or_else(|| rows.last())
                .map(|row| row.child_block)
            else {
                return Ok(None);
            };
            if child_block < component.start_block {
                return Ok(None);
            }
            page_index = (child_block - component.start_block) as usize;
        }
        Ok(None)
    }

    pub(super) fn scan_ssed_simple_index_rows(
        &self,
        row_limit: Option<usize>,
        mut on_row: impl FnMut(SsedIndexRow) -> Result<bool>,
    ) -> Result<Vec<Diagnostic>> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(vec![Diagnostic::error(
                "ssed_catalog_missing",
                "SSED index scanning requires a parsed SSEDINFO catalog",
            )]);
        };
        let mut diagnostics = Vec::new();
        let mut row_count = 0usize;
        'components: for component in catalog.components_by_role(SsedComponentRole::Index) {
            if row_limit.is_some_and(|limit| row_count >= limit) {
                break;
            }
            if !is_supported_index_type(component.component_type) {
                diagnostics.push(
                    Diagnostic::info(
                        "ssed_index_variant_deferred",
                        format!("{} is not a supported index component", component.filename),
                    )
                    .with_context("component", &component.filename),
                );
                continue;
            }
            let path = match self.resolve_readable_ssed_component_path(component) {
                Ok(Some(path)) => path,
                Ok(None) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_index_component_missing",
                            format!("{} is declared but not present on disk", component.filename),
                        )
                        .with_context("component", &component.filename),
                    );
                    continue;
                }
                Err(error) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_index_component_decode_failed",
                            format!(
                                "{} is not readable as SSEDDATA: {error}",
                                component.filename
                            ),
                        )
                        .with_context("component", &component.filename),
                    );
                    continue;
                }
            };
            let mut reader = SsedDataFile::open(&path)?;
            let page_count = component.block_count() as usize;
            let mut scan_state = SsedIndexScanState::default();
            for page_index in 0..page_count {
                if row_limit.is_some_and(|limit| row_count >= limit) {
                    break;
                }
                let page = reader.read_range(page_index * INDEX_PAGE_SIZE, INDEX_PAGE_SIZE)?;
                if page.len() < 4 {
                    break;
                }
                let word = u16::from_be_bytes([page[0], page[1]]);
                if !is_leaf_page(word) {
                    continue;
                }
                let logical_block = component.start_block + page_index as u32;
                let (page_rows, unknown) = parse_supported_leaf_page(
                    &component.filename,
                    component.component_type,
                    &page,
                    page_index as u32,
                    logical_block,
                    &mut scan_state,
                );
                if unknown > 0 {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_index_unknown_leaf_bytes",
                            format!(
                                "{} had {unknown} unknown simple leaf row(s)",
                                component.filename
                            ),
                        )
                        .with_context("component", &component.filename),
                    );
                }
                for row in page_rows {
                    if row_limit.is_some_and(|limit| row_count >= limit) {
                        break 'components;
                    }
                    row_count = row_count.saturating_add(1);
                    if !on_row(row)? {
                        break 'components;
                    }
                }
            }
        }
        Ok(diagnostics)
    }

    pub(super) fn scan_ssed_index_component_rows(
        &self,
        component: &SsedComponent,
        row_limit: Option<usize>,
        mut on_row: impl FnMut(SsedIndexRow) -> Result<bool>,
    ) -> Result<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();
        if !is_supported_index_type(component.component_type) {
            diagnostics.push(
                Diagnostic::info(
                    "ssed_index_variant_deferred",
                    format!("{} is not a supported index component", component.filename),
                )
                .with_context("component", &component.filename),
            );
            return Ok(diagnostics);
        }
        let path = match self.resolve_readable_ssed_component_path(component) {
            Ok(Some(path)) => path,
            Ok(None) => {
                diagnostics.push(
                    Diagnostic::warning(
                        "ssed_index_component_missing",
                        format!("{} is declared but not present on disk", component.filename),
                    )
                    .with_context("component", &component.filename),
                );
                return Ok(diagnostics);
            }
            Err(error) => {
                diagnostics.push(
                    Diagnostic::warning(
                        "ssed_index_component_decode_failed",
                        format!(
                            "{} is not readable as SSEDDATA: {error}",
                            component.filename
                        ),
                    )
                    .with_context("component", &component.filename),
                );
                return Ok(diagnostics);
            }
        };
        let mut reader = SsedDataFile::open(&path)?;
        let page_count = component.block_count() as usize;
        let mut scan_state = SsedIndexScanState::default();
        let mut row_count = 0usize;
        'pages: for page_index in 0..page_count {
            if row_limit.is_some_and(|limit| row_count >= limit) {
                break;
            }
            let page = reader.read_range(page_index * INDEX_PAGE_SIZE, INDEX_PAGE_SIZE)?;
            if page.len() < 4 {
                break;
            }
            let word = u16::from_be_bytes([page[0], page[1]]);
            if !is_leaf_page(word) {
                continue;
            }
            let logical_block = component.start_block + page_index as u32;
            let (page_rows, unknown) = parse_supported_leaf_page(
                &component.filename,
                component.component_type,
                &page,
                page_index as u32,
                logical_block,
                &mut scan_state,
            );
            if unknown > 0 {
                diagnostics.push(
                    Diagnostic::warning(
                        "ssed_index_unknown_leaf_bytes",
                        format!(
                            "{} had {unknown} unknown simple leaf row(s)",
                            component.filename
                        ),
                    )
                    .with_context("component", &component.filename),
                );
            }
            for row in page_rows {
                if row_limit.is_some_and(|limit| row_count >= limit) {
                    break 'pages;
                }
                row_count = row_count.saturating_add(1);
                if !on_row(row)? {
                    break 'pages;
                }
            }
        }
        Ok(diagnostics)
    }

    pub(super) fn ssed_title_text(&self, pointer: SsedIndexPointer) -> Option<String> {
        let catalog = self.ssed_catalog.as_ref()?;
        let component = catalog.component_for_address(pointer.block)?;
        if component.role != SsedComponentRole::Title {
            return None;
        }
        let component_offset = component.relative_offset(pointer.block, pointer.offset)?;
        let path = self
            .resolve_readable_ssed_component_path(component)
            .ok()
            .flatten()?;
        let mut reader = SsedDataFile::open(path).ok()?;
        let data = reader
            .read_range(usize::try_from(component_offset).ok()?, 512)
            .ok()?;
        let title = decode_title_text(&data);
        (!title.is_empty()).then_some(title)
    }

    pub(in crate::package) fn ssed_rich_label(&self, value: &str) -> RichLabel {
        resolve_rich_label(self, value, &GaijiPolicy::default())
    }

    pub(in crate::package) fn ssed_target_for_index_pointer(
        &self,
        pointer: SsedIndexPointer,
    ) -> Result<std::result::Result<TargetToken, Diagnostic>> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(Err(Diagnostic::error(
                "ssed_catalog_missing",
                "SSED index body pointers require a parsed SSEDINFO catalog",
            )));
        };
        let Some(component) = catalog.component_for_address(pointer.block) else {
            return Ok(Err(Diagnostic::warning(
                "ssed_index_body_component_missing",
                format!(
                    "no component contains index body pointer block {} offset {}",
                    pointer.block, pointer.offset
                ),
            )));
        };
        if component
            .relative_offset(pointer.block, pointer.offset)
            .is_none()
        {
            return Ok(Err(Diagnostic::warning(
                "ssed_index_body_pointer_invalid",
                format!(
                    "{} does not contain index body pointer block {} offset {}",
                    component.filename, pointer.block, pointer.offset
                ),
            )
            .with_context("component", &component.filename)));
        }
        Ok(Ok(TargetToken::new(&InternalTarget::SsedAddress {
            component: component.filename.clone(),
            block: pointer.block,
            offset: pointer.offset,
        })?))
    }

    pub(super) fn ssed_target_for_loose_address(
        &self,
        block: u32,
        offset: u32,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Option<TargetToken>> {
        let Some(catalog) = &self.ssed_catalog else {
            diagnostics.push(Diagnostic::error(
                "ssed_catalog_missing",
                "loose SSED address links require a parsed SSEDINFO catalog",
            ));
            return Ok(None);
        };
        let Some(component) = catalog.component_for_address(block) else {
            diagnostics.push(Diagnostic::warning(
                "ssed_loose_address_unresolved",
                format!(
                    "loose SSED address {:08x}:{:04x} is outside declared components",
                    block, offset
                ),
            ));
            return Ok(None);
        };
        if component.relative_offset(block, offset).is_none() {
            diagnostics.push(
                Diagnostic::warning(
                    "ssed_loose_address_invalid",
                    format!(
                        "{} does not contain loose address {:08x}:{:04x}",
                        component.filename, block, offset
                    ),
                )
                .with_context("component", &component.filename),
            );
            return Ok(None);
        }
        Ok(Some(TargetToken::new(&InternalTarget::SsedAddress {
            component: component.filename.clone(),
            block,
            offset,
        })?))
    }

    pub(super) fn ssed_index_row_label(&self, row: &SsedIndexRow) -> RichLabel {
        let label = self.ssed_display_text_for_index_row(row);
        self.ssed_rich_label(&label)
    }

    pub(in crate::package) fn ssed_display_text_for_index_row(&self, row: &SsedIndexRow) -> String {
        let title = self.ssed_title_text(row.title);
        match title {
            Some(title) if !looks_like_raw_anchor_label(&title) => title,
            _ => row.key.clone(),
        }
    }

    pub(super) fn ssed_component_for_index_pointer(
        &self,
        pointer: SsedIndexPointer,
    ) -> Option<&str> {
        self.ssed_catalog
            .as_ref()
            .and_then(|catalog| catalog.component_for_address(pointer.block))
            .map(|component| component.filename.as_str())
    }
}

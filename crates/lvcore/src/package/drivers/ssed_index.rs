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
                let component_read_base = ssed_component_read_base(component, &reader);
                let page_count = component.block_count() as usize;
                let start_page = match self.ssed_simple_index_candidate_leaf_page(
                    component,
                    &mut reader,
                    component_read_base,
                    &needle_key,
                )? {
                    Some(page_index) => page_index,
                    None => continue,
                };
                scanned_components = scanned_components.saturating_add(1);
                let mut last_key = None::<Vec<u8>>;
                'pages: for page_index in start_page..page_count {
                    let page = reader.read_range(
                        component_page_offset(component_read_base, page_index),
                        INDEX_PAGE_SIZE,
                    )?;
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
                        if !on_row(row)? {
                            break 'candidates;
                        }
                        if !row_matches && passed_match_region {
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
        component_read_base: usize,
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
            let page = reader.read_range(
                component_page_offset(component_read_base, page_index),
                INDEX_PAGE_SIZE,
            )?;
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
        on_row: impl FnMut(SsedIndexRow) -> Result<bool>,
    ) -> Result<Vec<Diagnostic>> {
        self.scan_ssed_simple_index_rows_with_filters(row_limit, |_| true, |_, _| true, on_row)
    }

    pub(super) fn scan_ssed_simple_index_rows_with_page_filter(
        &self,
        row_limit: Option<usize>,
        page_may_match: impl FnMut(&SsedComponent, &[u8]) -> bool,
        on_row: impl FnMut(SsedIndexRow) -> Result<bool>,
    ) -> Result<Vec<Diagnostic>> {
        self.scan_ssed_simple_index_rows_with_filters(row_limit, |_| true, page_may_match, on_row)
    }

    pub(super) fn scan_ssed_simple_index_rows_with_filters(
        &self,
        row_limit: Option<usize>,
        mut component_may_match: impl FnMut(&SsedComponent) -> bool,
        mut page_may_match: impl FnMut(&SsedComponent, &[u8]) -> bool,
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
            if !component_may_match(component) {
                continue;
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
                        Diagnostic::info(
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
            let component_read_base = ssed_component_read_base(component, &reader);
            let page_count = component.block_count() as usize;
            let mut scan_state = SsedIndexScanState::default();
            for page_index in 0..page_count {
                if row_limit.is_some_and(|limit| row_count >= limit) {
                    break;
                }
                let page = reader.read_range(
                    component_page_offset(component_read_base, page_index),
                    INDEX_PAGE_SIZE,
                )?;
                if page.len() < 4 {
                    break;
                }
                let word = u16::from_be_bytes([page[0], page[1]]);
                if !is_leaf_page(word) {
                    continue;
                }
                if !page_may_match(component, &page) {
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
                    Diagnostic::info(
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
        let component_read_base = ssed_component_read_base(component, &reader);
        let page_count = component.block_count() as usize;
        let mut scan_state = SsedIndexScanState::default();
        let mut row_count = 0usize;
        'pages: for page_index in 0..page_count {
            if row_limit.is_some_and(|limit| row_count >= limit) {
                break;
            }
            let page = reader.read_range(
                component_page_offset(component_read_base, page_index),
                INDEX_PAGE_SIZE,
            )?;
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

    pub(in crate::package) fn ssed_rich_label_with_policy(
        &self,
        value: &str,
        policy: &GaijiPolicy,
    ) -> RichLabel {
        resolve_rich_label(self, value, policy)
    }

    pub(in crate::package) fn ssed_target_for_index_pointer(
        &self,
        pointer: SsedIndexPointer,
    ) -> Result<std::result::Result<TargetToken, Diagnostic>> {
        self.ssed_target_for_index_pointer_with_bound(pointer, None)
    }

    pub(in crate::package) fn ssed_target_for_index_row(
        &self,
        row: &SsedIndexRow,
        next_row: Option<&SsedIndexRow>,
    ) -> Result<std::result::Result<TargetToken, Diagnostic>> {
        let end = next_row
            .filter(|next| next.body != row.body)
            .filter(|next| (next.body.block, next.body.offset) > (row.body.block, row.body.offset))
            .map(|next| next.body);
        self.ssed_target_for_index_pointer_with_bound(row.body, end)
    }

    pub(in crate::package) fn ssed_target_for_search_index_row(
        &self,
        row: &SsedIndexRow,
    ) -> Result<std::result::Result<TargetToken, Diagnostic>> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(Err(Diagnostic::error(
                "ssed_catalog_missing",
                "SSED index body pointers require a parsed SSEDINFO catalog",
            )));
        };
        let Some(component) = catalog.component_for_address(row.body.block) else {
            return Ok(Err(Diagnostic::warning(
                "ssed_index_body_component_missing",
                format!(
                    "no component contains index body pointer block {} offset {}",
                    row.body.block, row.body.offset
                ),
            )));
        };
        if component
            .relative_offset(row.body.block, row.body.offset)
            .is_none()
        {
            return Ok(Err(Diagnostic::warning(
                "ssed_index_body_pointer_invalid",
                format!(
                    "{} does not contain index body pointer block {} offset {}",
                    component.filename, row.body.block, row.body.offset
                ),
            )
            .with_context("component", &component.filename)));
        }
        Ok(Ok(TargetToken::new(&InternalTarget::SsedIndexAddress {
            component: component.filename.clone(),
            block: row.body.block,
            offset: row.body.offset,
            index_component: row.component.clone(),
        })?))
    }

    pub(in crate::package) fn ssed_index_row_body_pointer_is_outside_catalog_range(
        &self,
        row: &SsedIndexRow,
    ) -> bool {
        let Some(catalog) = &self.ssed_catalog else {
            return false;
        };
        if catalog.component_for_address(row.body.block).is_some() {
            return false;
        }
        let mut ranged_components = catalog
            .components
            .iter()
            .filter(|component| component.has_positive_range());
        let Some(first) = ranged_components.next() else {
            return false;
        };
        let mut min_start = first.start_block;
        let mut max_end = first.end_block;
        for component in ranged_components {
            min_start = min_start.min(component.start_block);
            max_end = max_end.max(component.end_block);
        }
        row.body.block < min_start || row.body.block > max_end
    }

    pub(in crate::package) fn ssed_next_index_body_pointer_after(
        &self,
        pointer: SsedIndexPointer,
    ) -> Result<Option<SsedIndexPointer>> {
        let Some(component_name) = self.ssed_component_for_index_pointer(pointer) else {
            return Ok(None);
        };
        let boundaries = self.ssed_index_body_boundaries()?;
        let Some(pointers) = boundaries.get(component_name) else {
            return Ok(None);
        };
        Ok(pointers
            .iter()
            .find(|candidate| (candidate.block, candidate.offset) > (pointer.block, pointer.offset))
            .copied())
    }

    fn ssed_index_body_boundaries(&self) -> Result<&BTreeMap<String, Vec<SsedIndexPointer>>> {
        let boundaries = self.ssed_index_body_boundaries.get_or_init(|| {
            self.build_ssed_index_body_boundaries()
                .map_err(|error| error.to_string())
        });
        match boundaries {
            Ok(boundaries) => Ok(boundaries),
            Err(error) => Err(Error::Driver(error.clone())),
        }
    }

    fn build_ssed_index_body_boundaries(&self) -> Result<BTreeMap<String, Vec<SsedIndexPointer>>> {
        let mut by_component = BTreeMap::<String, Vec<SsedIndexPointer>>::new();
        let mut diagnostics = self.scan_ssed_simple_index_rows_with_filters(
            None,
            |component| !ssed_index_component_name_is_backward(&component.filename),
            |_, _| true,
            |row| {
                if let Some(component) = self.ssed_component_for_index_pointer(row.body) {
                    by_component
                        .entry(component.to_owned())
                        .or_default()
                        .push(row.body);
                }
                Ok(true)
            },
        )?;
        diagnostics.retain(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error);
        if let Some(diagnostic) = diagnostics.into_iter().next() {
            return Err(Error::Driver(diagnostic.message));
        }
        for pointers in by_component.values_mut() {
            pointers.sort_by_key(|pointer| (pointer.block, pointer.offset));
            pointers.dedup_by_key(|pointer| (pointer.block, pointer.offset));
        }
        Ok(by_component)
    }

    pub(in crate::package) fn ssed_target_for_index_pointer_with_bound(
        &self,
        pointer: SsedIndexPointer,
        end: Option<SsedIndexPointer>,
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
        let target = if let Some(end) = end {
            InternalTarget::SsedBoundedAddress {
                component: component.filename.clone(),
                block: pointer.block,
                offset: pointer.offset,
                end_block: end.block,
                end_offset: end.offset,
            }
        } else {
            InternalTarget::SsedAddress {
                component: component.filename.clone(),
                block: pointer.block,
                offset: pointer.offset,
            }
        };
        Ok(Ok(TargetToken::new(&target)?))
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

    pub(super) fn ssed_index_row_label_with_policy(
        &self,
        row: &SsedIndexRow,
        policy: &GaijiPolicy,
    ) -> RichLabel {
        let label = self.ssed_display_text_for_index_row(row);
        self.ssed_rich_label_with_policy(&label, policy)
    }

    pub(in crate::package) fn ssed_display_text_for_index_row(&self, row: &SsedIndexRow) -> String {
        self.ssed_display_text_for_index_title_or_key(row.title, &row.key)
    }

    pub(in crate::package) fn ssed_display_text_for_index_title_or_key(
        &self,
        title_pointer: SsedIndexPointer,
        fallback_key: &str,
    ) -> String {
        let title = self.ssed_title_text(title_pointer);
        match title {
            Some(title) if !looks_like_raw_anchor_label(&title) => title,
            _ => fallback_key.to_owned(),
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

fn ssed_component_read_base(component: &SsedComponent, reader: &SsedDataFile) -> usize {
    if component.start_block >= reader.header().start_block
        && component.end_block <= reader.header().end_block
    {
        return usize::try_from(component.start_block - reader.header().start_block).unwrap_or(0);
    }
    0
}

fn component_page_offset(component_read_base: usize, page_index: usize) -> usize {
    component_read_base
        .saturating_add(page_index)
        .saturating_mul(INDEX_PAGE_SIZE)
}

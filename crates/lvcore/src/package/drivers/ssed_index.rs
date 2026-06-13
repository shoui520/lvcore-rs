use super::*;

const SSED_ADJACENT_INDEX_BODY_BOUND_MAX_BYTES: u64 = 256 * 1024;
const SSED_NEAR_KEY_MAX_LEAF_PAGES_PER_COMPONENT: usize = 32;
const SSED_INDEX_PAGE_PREFILTER_IN_MEMORY_MAX_EXPANDED_BYTES: usize = 32 * 1024 * 1024;
const SSED_INDEX_PAGE_PREFILTER_ANCHOR_MIN_LEN: usize = 2;

impl ReaderBookPackage {
    pub(super) fn scan_ssed_simple_leaf_index_rows_near_key(
        &self,
        mode: &SearchMode,
        needle: &str,
        mut on_row: impl FnMut(SsedIndexRow) -> Result<bool>,
        mut candidate_satisfied: impl FnMut() -> bool,
    ) -> Result<SsedNearKeyScanResult> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(SsedNearKeyScanResult {
                scanned_components: 0,
                needs_prefilter_fallback: false,
                diagnostics: vec![Diagnostic::error(
                    "ssed_catalog_missing",
                    "SSED index scanning requires a parsed SSEDINFO catalog",
                )],
            });
        };
        let mut diagnostics = Vec::new();
        let mut scanned_components = 0usize;
        let mut needs_linear_fallback = false;
        let mut needs_prefilter_fallback = false;
        let probe = if *mode == SearchMode::Backward {
            reverse_search_match_text(needle)
        } else {
            needle.to_owned()
        };
        let needle_keys = ssed_index_search_key_candidates(&probe);
        if needle_keys.is_empty() && !probe.is_empty() {
            return Ok(SsedNearKeyScanResult {
                scanned_components: 0,
                needs_prefilter_fallback: false,
                diagnostics,
            });
        }
        'candidates: for needle_key in needle_keys {
            for component in catalog.components_by_role(SsedComponentRole::Index) {
                if component.multi == 0xff {
                    continue;
                }
                if !is_supported_index_type(component.component_type) {
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
                let start_pages = self.ssed_simple_index_candidate_leaf_pages(
                    mode,
                    component,
                    &mut reader,
                    component_read_base,
                    &needle_key,
                )?;
                if start_pages.is_empty() {
                    needs_prefilter_fallback = true;
                    continue;
                }
                scanned_components = scanned_components.saturating_add(1);
                for start_page in start_pages {
                    let mut last_key = None::<Vec<u8>>;
                    let mut scan_state = SsedIndexScanState::default();
                    let mut scanned_leaf_pages = 0usize;
                    'pages: for page_index in start_page..page_count {
                        let page = read_index_page(&mut reader, component_read_base, page_index)?;
                        if page.len() < 4 {
                            break;
                        }
                        let word = u16::from_be_bytes([page[0], page[1]]);
                        if !is_leaf_page(word) {
                            continue;
                        }
                        scanned_leaf_pages = scanned_leaf_pages.saturating_add(1);
                        let logical_block = component.start_block + page_index as u32;
                        let (rows, unknown) = parse_supported_leaf_page(
                            &component.filename,
                            component.component_type,
                            page,
                            page_index as u32,
                            logical_block,
                            &mut scan_state,
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
                                        "{} had {unknown} unknown index leaf row(s)",
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
                        if scanned_leaf_pages >= SSED_NEAR_KEY_MAX_LEAF_PAGES_PER_COMPONENT {
                            break 'pages;
                        }
                    }
                    if candidate_satisfied() {
                        break 'candidates;
                    }
                }
            }
        }
        Ok(SsedNearKeyScanResult {
            scanned_components,
            needs_prefilter_fallback,
            diagnostics,
        })
    }

    pub(super) fn scan_ssed_simple_leaf_index_component_rows_near_exact_key(
        &self,
        component: &SsedComponent,
        needle: &str,
        mut on_row: impl FnMut(SsedIndexRow) -> Result<bool>,
        mut candidate_satisfied: impl FnMut() -> bool,
    ) -> Result<(Vec<Diagnostic>, bool)> {
        let mut diagnostics = Vec::new();
        if !is_simple_leaf_index_type(component.component_type) {
            return Ok((diagnostics, true));
        }
        let needle_keys = ssed_index_search_key_candidates(needle);
        if needle_keys.is_empty() && !needle.is_empty() {
            return Ok((diagnostics, true));
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
                return Ok((diagnostics, false));
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
                return Ok((diagnostics, false));
            }
        };
        let mut reader = SsedDataFile::open(&path)?;
        let component_read_base = ssed_component_read_base(component, &reader);
        let page_count = component.block_count() as usize;
        let mut needs_linear_fallback = false;
        let mut seen_rows = BTreeSet::<(String, u32, u32)>::new();
        for needle_key in needle_keys {
            let start_pages = self.ssed_simple_index_candidate_leaf_pages(
                &SearchMode::Exact,
                component,
                &mut reader,
                component_read_base,
                &needle_key,
            )?;
            if start_pages.is_empty() {
                needs_linear_fallback = true;
                continue;
            }
            for start_page in start_pages {
                let mut last_key = None::<Vec<u8>>;
                let mut scan_state = SsedIndexScanState::default();
                'pages: for page_index in start_page..page_count {
                    let page = read_index_page(&mut reader, component_read_base, page_index)?;
                    if page.len() < 4 {
                        break;
                    }
                    let word = u16::from_be_bytes([page[0], page[1]]);
                    if !is_leaf_page(word) {
                        continue;
                    }
                    let logical_block = component.start_block + page_index as u32;
                    let (rows, unknown) = parse_supported_leaf_page(
                        &component.filename,
                        component.component_type,
                        page,
                        page_index as u32,
                        logical_block,
                        &mut scan_state,
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
                                    "{} had {unknown} unknown index leaf row(s)",
                                    component.filename
                                ),
                            )
                            .with_context("component", &component.filename),
                        );
                    }
                    for row in rows {
                        let key = ssed_index_row_match_text(&row);
                        let key_bytes = ssed_index_row_order_key(&row);
                        if last_key
                            .as_ref()
                            .is_some_and(|last_key| key_bytes.as_slice() < last_key.as_slice())
                        {
                            needs_linear_fallback = true;
                        }
                        last_key = Some(key_bytes.clone());
                        let row_matches = key == needle;
                        let passed_match_region =
                            !needs_linear_fallback && key_bytes.as_slice() > needle_key.as_slice();
                        if row_matches
                            && seen_rows.insert((
                                row.component.clone(),
                                row.page_index,
                                row.row_index,
                            ))
                            && !on_row(row)?
                        {
                            return Ok((diagnostics, needs_linear_fallback));
                        }
                        if !row_matches && passed_match_region {
                            break 'pages;
                        }
                    }
                    if candidate_satisfied() {
                        return Ok((diagnostics, needs_linear_fallback));
                    }
                }
                if candidate_satisfied() {
                    return Ok((diagnostics, needs_linear_fallback));
                }
            }
        }
        Ok((diagnostics, needs_linear_fallback))
    }

    fn ssed_simple_index_candidate_leaf_pages(
        &self,
        mode: &SearchMode,
        component: &SsedComponent,
        reader: &mut SsedDataFile,
        component_read_base: usize,
        needle_key: &[u8],
    ) -> Result<Vec<usize>> {
        let mut pages = Vec::new();
        let mut push_page = |page: Option<usize>| {
            if let Some(page) = page
                && !pages.contains(&page)
            {
                pages.push(page);
            }
        };
        if *mode == SearchMode::Exact {
            push_page(self.ssed_simple_index_candidate_leaf_page_lower_bound(
                component,
                reader,
                component_read_base,
                needle_key,
            )?);
            push_page(self.ssed_simple_index_candidate_leaf_page_upper_bound(
                component,
                reader,
                component_read_base,
                needle_key,
            )?);
        } else if *mode == SearchMode::Forward || *mode == SearchMode::Backward {
            push_page(self.ssed_simple_index_candidate_leaf_page_upper_bound(
                component,
                reader,
                component_read_base,
                needle_key,
            )?);
        }
        Ok(pages)
    }

    fn ssed_simple_index_candidate_leaf_page_upper_bound(
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
            let page = read_index_page(reader, component_read_base, page_index)?;
            if page.len() < 4 {
                return Ok(None);
            }
            let word = u16::from_be_bytes([page[0], page[1]]);
            if is_leaf_page(word) {
                return Ok(Some(page_index));
            }
            let rows = parse_internal_page(
                &component.filename,
                page,
                page_index as u32,
                component.start_block + page_index as u32,
            );
            let Some(child_block) = rows
                .iter()
                .find(|row| {
                    ssed_internal_key_is_ff_sentinel(&row.raw_key)
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

    fn ssed_simple_index_candidate_leaf_page_lower_bound(
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
            let page = read_index_page(reader, component_read_base, page_index)?;
            if page.len() < 4 {
                return Ok(None);
            }
            let word = u16::from_be_bytes([page[0], page[1]]);
            if is_leaf_page(word) {
                return Ok(Some(page_index));
            }
            let rows = parse_internal_page(
                &component.filename,
                page,
                page_index as u32,
                component.start_block + page_index as u32,
            );
            let mut chosen = rows.first();
            for row in &rows {
                if row.raw_key.is_empty()
                    || (!ssed_internal_key_is_ff_sentinel(&row.raw_key)
                        && row.raw_key.as_slice() <= needle_key)
                {
                    chosen = Some(row);
                    continue;
                }
                break;
            }
            let Some(child_block) = chosen.map(|row| row.child_block) else {
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
                let page = read_index_page(&mut reader, component_read_base, page_index)?;
                if page.len() < 4 {
                    break;
                }
                let word = u16::from_be_bytes([page[0], page[1]]);
                if !is_leaf_page(word) {
                    continue;
                }
                if !page_may_match(component, page) {
                    continue;
                }
                let logical_block = component.start_block + page_index as u32;
                let (page_rows, unknown) = parse_supported_leaf_page(
                    &component.filename,
                    component.component_type,
                    page,
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

    pub(super) fn scan_ssed_ordered_index_rows_with_filters(
        &self,
        row_limit: Option<usize>,
        mut component_may_match: impl FnMut(&SsedComponent) -> bool,
        mut on_row: impl FnMut(SsedIndexRow) -> Result<bool>,
    ) -> Result<Vec<Diagnostic>> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(vec![Diagnostic::error(
                "ssed_catalog_missing",
                "SSED ordered index scanning requires a parsed SSEDINFO catalog",
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
            if page_count == 0 {
                continue;
            }
            let mut stack = vec![0usize];
            let mut visited = HashSet::new();
            let mut scan_state = SsedIndexScanState::default();
            while let Some(page_index) = stack.pop() {
                if row_limit.is_some_and(|limit| row_count >= limit) {
                    break 'components;
                }
                if page_index >= page_count || !visited.insert(page_index) {
                    continue;
                }
                let page = read_index_page(&mut reader, component_read_base, page_index)?;
                if page.len() < 4 {
                    continue;
                }
                let word = u16::from_be_bytes([page[0], page[1]]);
                if is_leaf_page(word) {
                    let logical_block = component.start_block + page_index as u32;
                    let (page_rows, unknown) = parse_supported_leaf_page(
                        &component.filename,
                        component.component_type,
                        page,
                        page_index as u32,
                        logical_block,
                        &mut scan_state,
                    );
                    if unknown > 0 {
                        diagnostics.push(
                            Diagnostic::warning(
                                "ssed_index_unknown_leaf_bytes",
                                format!(
                                    "{} had {unknown} unknown ordered index leaf row(s)",
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
                    continue;
                }
                let child_rows = parse_internal_page(
                    &component.filename,
                    page,
                    page_index as u32,
                    component.start_block + page_index as u32,
                );
                for child in child_rows.into_iter().rev() {
                    if child.child_block < component.start_block {
                        continue;
                    }
                    let child_page = (child.child_block - component.start_block) as usize;
                    if child_page < page_count {
                        stack.push(child_page);
                    }
                }
            }
        }
        Ok(diagnostics)
    }

    pub(super) fn scan_ssed_partial_index_rows_paged(
        &self,
        needle: &str,
        cursor: Option<SsedPartialIndexScanCursor>,
        mut on_row: impl FnMut(SsedIndexRow) -> Result<bool>,
    ) -> Result<SsedPartialIndexScanResult> {
        self.scan_ssed_partial_index_rows_paged_with_leaf_budget(
            needle,
            cursor,
            SSED_PARTIAL_INDEX_SCAN_LEAF_PAGE_BUDGET,
            &mut on_row,
        )
    }

    pub(super) fn scan_ssed_partial_index_rows_paged_with_leaf_budget(
        &self,
        needle: &str,
        cursor: Option<SsedPartialIndexScanCursor>,
        leaf_page_budget: usize,
        mut on_row: impl FnMut(SsedIndexRow) -> Result<bool>,
    ) -> Result<SsedPartialIndexScanResult> {
        self.scan_ssed_partial_index_rows_paged_with_leaf_budget_and_cursor(
            needle,
            cursor,
            leaf_page_budget,
            SSED_PARTIAL_INDEX_PREFILTERED_LEAF_PAGE_BUDGET,
            false,
            |_, row| on_row(row),
        )
    }

    pub(super) fn scan_ssed_partial_index_rows_paged_with_leaf_budget_and_cursor(
        &self,
        needle: &str,
        cursor: Option<SsedPartialIndexScanCursor>,
        leaf_page_budget: usize,
        prefiltered_leaf_page_budget: usize,
        allow_nonprefix_page_prefilter_extensions: bool,
        mut on_row: impl FnMut(SsedPartialIndexScanCursor, SsedIndexRow) -> Result<bool>,
    ) -> Result<SsedPartialIndexScanResult> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(SsedPartialIndexScanResult {
                diagnostics: vec![Diagnostic::error(
                    "ssed_catalog_missing",
                    "SSED index scanning requires a parsed SSEDINFO catalog",
                )],
                next_cursor: None,
            });
        };
        let forward_candidates = ssed_index_page_prefilter_candidates(needle);
        let use_page_prefilter = !forward_candidates.is_empty();
        let reversed_needle = reverse_search_match_text(needle);
        let reverse_candidates = ssed_index_page_prefilter_candidates(&reversed_needle);
        let skip_backward_rows = self.ssed_has_forward_browse_index();
        let start_component_index = cursor.map(|cursor| cursor.component_index).unwrap_or(0);
        let mut diagnostics = Vec::new();
        let mut decoded_leaf_pages = 0usize;
        let mut prefiltered_leaf_pages = 0usize;

        'components: for component in catalog.components_by_role(SsedComponentRole::Index) {
            if component.index < start_component_index {
                continue;
            }
            if skip_backward_rows && ssed_index_component_name_is_backward(&component.filename) {
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
            let page_count = component.block_count() as usize;
            let start_page =
                if cursor.is_some_and(|cursor| cursor.component_index == component.index) {
                    cursor.map(|cursor| cursor.page_index).unwrap_or(0)
                } else {
                    0
                };
            let page_candidates = if ssed_index_component_name_is_backward(&component.filename) {
                &reverse_candidates
            } else {
                &forward_candidates
            };
            let page_prefilter_is_safe =
                ssed_index_page_prefilter_is_safe(component.component_type);
            let page_prefilter_anchors = if use_page_prefilter && page_prefilter_is_safe {
                ssed_page_prefilter_anchor_candidates(page_candidates)
                    .into_iter()
                    .filter(|anchor| anchor.len() >= SSED_INDEX_PAGE_PREFILTER_ANCHOR_MIN_LEN)
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            if allow_nonprefix_page_prefilter_extensions
                && !page_prefilter_anchors.is_empty()
                && SsedDataHeader::parse_file(&path).is_ok_and(|header| {
                    header.expanded_size() <= SSED_INDEX_PAGE_PREFILTER_IN_MEMORY_MAX_EXPANDED_BYTES
                })
                && let Ok(reader) = SsedDataReader::parse_file(&path)
            {
                let component_read_base =
                    ssed_component_read_base_for_header(component, reader.header());
                let mut scan_state = SsedIndexScanState::default();
                let candidate_pages = ssed_candidate_pages_for_expanded_index(
                    reader.expanded(),
                    component_read_base,
                    start_page,
                    page_count,
                    &page_prefilter_anchors,
                );
                for page_index in candidate_pages {
                    let page = read_index_page_from_expanded(
                        reader.expanded(),
                        component_read_base,
                        page_index,
                    );
                    if page.len() < 4 {
                        break;
                    }
                    let word = u16::from_be_bytes([page[0], page[1]]);
                    if !is_leaf_page(word)
                        || !ssed_body_window_may_contain_query(page, page_candidates)
                    {
                        continue;
                    }
                    decoded_leaf_pages = decoded_leaf_pages.saturating_add(1);
                    let logical_block = component.start_block + page_index as u32;
                    let (page_rows, unknown) = parse_supported_leaf_page(
                        &component.filename,
                        component.component_type,
                        page,
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
                    let row_cursor = SsedPartialIndexScanCursor {
                        component_index: component.index,
                        page_index,
                    };
                    for row in page_rows {
                        if !on_row(row_cursor, row)? {
                            break 'components;
                        }
                    }
                    if decoded_leaf_pages >= leaf_page_budget {
                        let next_cursor = next_ssed_partial_index_scan_cursor(
                            catalog,
                            component.index,
                            page_index.saturating_add(1),
                        );
                        return Ok(SsedPartialIndexScanResult {
                            diagnostics,
                            next_cursor: next_cursor.map(encode_ssed_partial_index_scan_cursor),
                        });
                    }
                }
                continue;
            }

            let mut reader = SsedDataFile::open(&path)?;
            let component_read_base = ssed_component_read_base(component, &reader);
            let mut scan_state = SsedIndexScanState::default();
            let mut tagged_prefilter_state = SsedTaggedLeafPagePrefilterState::default();
            for page_index in start_page..page_count {
                let page = read_index_page(&mut reader, component_read_base, page_index)?;
                if page.len() < 4 {
                    break;
                }
                let word = u16::from_be_bytes([page[0], page[1]]);
                if !is_leaf_page(word) {
                    continue;
                }
                let page_may_match = if !use_page_prefilter {
                    true
                } else if page_prefilter_is_safe {
                    ssed_body_window_may_contain_query(page, page_candidates)
                } else if allow_nonprefix_page_prefilter_extensions {
                    ssed_tagged_leaf_page_may_contain_query(
                        component.component_type,
                        page,
                        page_candidates,
                        &mut tagged_prefilter_state,
                    )
                    .unwrap_or(true)
                } else {
                    true
                };
                if page_may_match {
                    decoded_leaf_pages = decoded_leaf_pages.saturating_add(1);
                    let logical_block = component.start_block + page_index as u32;
                    let (page_rows, unknown) = parse_supported_leaf_page(
                        &component.filename,
                        component.component_type,
                        page,
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
                    let row_cursor = SsedPartialIndexScanCursor {
                        component_index: component.index,
                        page_index,
                    };
                    for row in page_rows {
                        if !on_row(row_cursor, row)? {
                            break 'components;
                        }
                    }
                    if decoded_leaf_pages >= leaf_page_budget {
                        let next_cursor = next_ssed_partial_index_scan_cursor(
                            catalog,
                            component.index,
                            page_index.saturating_add(1),
                        );
                        return Ok(SsedPartialIndexScanResult {
                            diagnostics,
                            next_cursor: next_cursor.map(encode_ssed_partial_index_scan_cursor),
                        });
                    }
                } else {
                    prefiltered_leaf_pages = prefiltered_leaf_pages.saturating_add(1);
                    if prefiltered_leaf_pages >= prefiltered_leaf_page_budget {
                        let next_cursor = next_ssed_partial_index_scan_cursor(
                            catalog,
                            component.index,
                            page_index.saturating_add(1),
                        );
                        return Ok(SsedPartialIndexScanResult {
                            diagnostics,
                            next_cursor: next_cursor.map(encode_ssed_partial_index_scan_cursor),
                        });
                    }
                }
            }
        }
        Ok(SsedPartialIndexScanResult {
            diagnostics,
            next_cursor: None,
        })
    }

    pub(super) fn scan_ssed_prefiltered_index_rows_paged(
        &self,
        mode: &SearchMode,
        needle: &str,
        include_simple_indexes: bool,
        cursor: Option<SsedPrefilteredIndexScanCursor>,
        mut on_row: impl FnMut(SsedIndexRow) -> Result<bool>,
    ) -> Result<SsedPrefilteredIndexScanResult> {
        self.scan_ssed_prefiltered_index_rows_paged_with_leaf_budget(
            mode,
            needle,
            include_simple_indexes,
            cursor,
            SSED_PARTIAL_INDEX_SCAN_LEAF_PAGE_BUDGET,
            &mut on_row,
        )
    }

    pub(super) fn scan_ssed_prefiltered_index_rows_paged_with_leaf_budget(
        &self,
        mode: &SearchMode,
        needle: &str,
        include_simple_indexes: bool,
        cursor: Option<SsedPrefilteredIndexScanCursor>,
        leaf_page_budget: usize,
        mut on_row: impl FnMut(SsedIndexRow) -> Result<bool>,
    ) -> Result<SsedPrefilteredIndexScanResult> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(SsedPrefilteredIndexScanResult {
                diagnostics: vec![Diagnostic::error(
                    "ssed_catalog_missing",
                    "SSED index scanning requires a parsed SSEDINFO catalog",
                )],
                next_cursor: None,
            });
        };
        let probe = if *mode == SearchMode::Backward {
            reverse_search_match_text(needle)
        } else {
            needle.to_owned()
        };
        let candidates = ssed_index_page_prefilter_candidates(&probe);
        let use_page_prefilter = !candidates.is_empty();
        let start_component_index = cursor.map(|cursor| cursor.component_index).unwrap_or(0);
        let mut diagnostics = Vec::new();
        let mut scanned_leaf_pages = 0usize;

        'components: for component in catalog.components_by_role(SsedComponentRole::Index) {
            if component.index < start_component_index {
                continue;
            }
            if !ssed_prefiltered_index_component_may_match(mode, include_simple_indexes, component)
            {
                continue;
            }
            if !is_supported_index_type(component.component_type) {
                continue;
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
            let start_page =
                if cursor.is_some_and(|cursor| cursor.component_index == component.index) {
                    cursor.map(|cursor| cursor.page_index).unwrap_or(0)
                } else {
                    0
                };
            let mut scan_state = SsedIndexScanState::default();
            for page_index in start_page..page_count {
                let page = read_index_page(&mut reader, component_read_base, page_index)?;
                if page.len() < 4 {
                    break;
                }
                let word = u16::from_be_bytes([page[0], page[1]]);
                if !is_leaf_page(word) {
                    continue;
                }
                scanned_leaf_pages = scanned_leaf_pages.saturating_add(1);
                if !use_page_prefilter
                    || !ssed_index_page_prefilter_is_safe(component.component_type)
                    || ssed_body_window_may_contain_query(page, &candidates)
                {
                    let logical_block = component.start_block + page_index as u32;
                    let (page_rows, unknown) = parse_supported_leaf_page(
                        &component.filename,
                        component.component_type,
                        page,
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
                        if !on_row(row)? {
                            break 'components;
                        }
                    }
                }
                if scanned_leaf_pages >= leaf_page_budget {
                    let next_cursor = next_ssed_prefiltered_index_scan_cursor(
                        catalog,
                        mode,
                        include_simple_indexes,
                        component.index,
                        page_index.saturating_add(1),
                    );
                    return Ok(SsedPrefilteredIndexScanResult {
                        diagnostics,
                        next_cursor: next_cursor.map(encode_ssed_prefiltered_index_scan_cursor),
                    });
                }
            }
        }
        Ok(SsedPrefilteredIndexScanResult {
            diagnostics,
            next_cursor: None,
        })
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
            let page = read_index_page(&mut reader, component_read_base, page_index)?;
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
                page,
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

    fn scan_ssed_index_component_body_pointers(
        &self,
        component: &SsedComponent,
        mut on_pointer: impl FnMut(SsedIndexPointer) -> Result<bool>,
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
        'pages: for page_index in 0..page_count {
            let page = read_index_page(&mut reader, component_read_base, page_index)?;
            if page.len() < 4 {
                break;
            }
            let word = u16::from_be_bytes([page[0], page[1]]);
            if !is_leaf_page(word) {
                continue;
            }
            if let Some((pointers, unknown)) =
                parse_supported_leaf_page_body_pointers(component.component_type, page)
            {
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
                for pointer in pointers {
                    if !on_pointer(pointer)? {
                        break 'pages;
                    }
                }
                continue;
            }
            let logical_block = component.start_block + page_index as u32;
            let (page_rows, unknown) = parse_supported_leaf_page(
                &component.filename,
                component.component_type,
                page,
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
                if !on_pointer(row.body)? {
                    break 'pages;
                }
            }
        }
        Ok(diagnostics)
    }

    pub(super) fn ssed_title_text(&self, pointer: SsedIndexPointer) -> Option<String> {
        let cache_key = (pointer.block, pointer.offset);
        if let Ok(cache) = self.ssed_title_text_cache.lock()
            && let Some(cached) = cache.get(&cache_key)
        {
            return cached.as_ref().map(|value| value.to_string());
        }
        let decoded = self.decode_ssed_title_text_uncached(pointer);
        if let Ok(mut cache) = self.ssed_title_text_cache.lock() {
            cache.insert(
                cache_key,
                decoded
                    .as_ref()
                    .map(|value| Arc::<str>::from(value.as_str())),
            );
        }
        decoded
    }

    fn decode_ssed_title_text_uncached(&self, pointer: SsedIndexPointer) -> Option<String> {
        let catalog = self.ssed_catalog.as_ref()?;
        let component = catalog.component_for_address(pointer.block)?;
        if component.role != SsedComponentRole::Title {
            return None;
        }
        let component_offset = component.relative_offset(pointer.block, pointer.offset)?;
        let mut readers = self.ssed_title_reader_cache.lock().ok()?;
        if !readers.contains_key(&component.filename) {
            let reader = self
                .resolve_readable_ssed_component_path(component)
                .map_err(|error| error.to_string())
                .and_then(|path| {
                    path.ok_or_else(|| format!("{} is not present on disk", component.filename))
                })
                .and_then(|path| SsedDataFile::open(path).map_err(|error| error.to_string()));
            readers.insert(component.filename.clone(), reader);
        }
        let reader = match readers.get_mut(&component.filename)? {
            Ok(reader) => reader,
            Err(_) => return None,
        };
        let data = reader
            .read_range(usize::try_from(component_offset).ok()?, 512)
            .ok()?;
        let title = decode_title_text_with_gaiji_filter(&data, |identity| {
            self.gaiji_unicode_map.contains_key(identity)
        });
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

    pub(in crate::package) fn ssed_browse_target_for_index_row(
        &self,
        row: &SsedIndexRow,
        next_row: Option<&SsedIndexRow>,
    ) -> Result<std::result::Result<TargetToken, Diagnostic>> {
        if let Some(end) = ssed_plausible_adjacent_index_bound(row, next_row) {
            return self.ssed_target_for_index_pointer_with_bound(row.body, Some(end));
        }
        self.ssed_target_for_search_index_row(row)
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

    pub(in crate::package) fn ssed_index_row_points_to_dense_sidecar_anchor(
        &self,
        row: &SsedIndexRow,
    ) -> Result<bool> {
        if self.ssed_sidecar_body_resolvers()?.is_empty() {
            return Ok(false);
        }
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(false);
        };
        let Some(component) = catalog.component_for_address(row.body.block) else {
            return Ok(false);
        };
        if component.role != SsedComponentRole::Honmon {
            return Ok(false);
        }
        let Some(component_offset) = component.relative_offset(row.body.block, row.body.offset)
        else {
            return Ok(false);
        };
        Ok(self
            .ssed_dense_anchor_at_component_offset(
                component,
                usize::try_from(component_offset).unwrap_or(usize::MAX),
            )?
            .is_some())
    }

    pub(in crate::package) fn ssed_next_index_body_pointer_after_in_index_component(
        &self,
        pointer: SsedIndexPointer,
        index_component: &str,
    ) -> Result<Option<SsedIndexPointer>> {
        let Some(component_name) = self.ssed_component_for_index_pointer(pointer) else {
            return Ok(None);
        };
        let boundaries = self.ssed_index_body_boundaries_for_index_component(index_component)?;
        let Some(pointers) = boundaries.get(component_name) else {
            return Ok(None);
        };
        Ok(pointers
            .iter()
            .find(|candidate| (candidate.block, candidate.offset) > (pointer.block, pointer.offset))
            .copied())
    }

    fn ssed_index_body_boundaries_for_index_component(
        &self,
        index_component_name: &str,
    ) -> Result<Arc<SsedIndexBodyBoundaryMap>> {
        let key = index_component_name.to_ascii_lowercase();
        {
            let cache = self
                .ssed_index_component_body_boundaries
                .lock()
                .map_err(|_| Error::Driver("SSED index boundary cache was poisoned".to_owned()))?;
            if let Some(cached) = cache.get(&key) {
                return cached
                    .as_ref()
                    .map(Arc::clone)
                    .map_err(|error| Error::Driver(error.clone()));
            }
        }

        let built = self
            .build_ssed_index_body_boundaries_for_index_component(index_component_name)
            .map(Arc::new)
            .map_err(|error| error.to_string());
        let mut cache = self
            .ssed_index_component_body_boundaries
            .lock()
            .map_err(|_| Error::Driver("SSED index boundary cache was poisoned".to_owned()))?;
        let cached = cache.entry(key).or_insert_with(|| built);
        cached
            .as_ref()
            .map(Arc::clone)
            .map_err(|error| Error::Driver(error.clone()))
    }

    fn build_ssed_index_body_boundaries_for_index_component(
        &self,
        index_component_name: &str,
    ) -> Result<BTreeMap<String, Vec<SsedIndexPointer>>> {
        let Some(catalog) = &self.ssed_catalog else {
            return Err(Error::Driver(
                "SSED index boundaries require a parsed SSEDINFO catalog".to_owned(),
            ));
        };
        let Some(component) =
            catalog
                .components_by_role(SsedComponentRole::Index)
                .find(|component| {
                    component
                        .filename
                        .eq_ignore_ascii_case(index_component_name)
                })
        else {
            return Ok(BTreeMap::new());
        };
        let mut by_component = BTreeMap::<String, Vec<SsedIndexPointer>>::new();
        let mut diagnostics =
            self.scan_ssed_index_component_body_pointers(component, |pointer| {
                if let Some(component) = self.ssed_component_for_index_pointer(pointer) {
                    by_component
                        .entry(component.to_owned())
                        .or_default()
                        .push(pointer);
                }
                Ok(true)
            })?;
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
            if self
                .visual_body_for_ssed_sidecar_address(block, offset)?
                .is_some()
                && let Some(honmon_component) = catalog
                    .components
                    .iter()
                    .find(|component| component.role == SsedComponentRole::Honmon)
            {
                return Ok(Some(TargetToken::new(&InternalTarget::SsedAddress {
                    component: honmon_component.filename.clone(),
                    block,
                    offset,
                })?));
            }
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
            if self
                .visual_body_for_ssed_sidecar_address(block, offset)?
                .is_some()
            {
                return Ok(Some(TargetToken::new(&InternalTarget::SsedAddress {
                    component: component.filename.clone(),
                    block,
                    offset,
                })?));
            }
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
        self.ssed_display_text_for_index_title_or_key(row.title, &ssed_visible_index_key(row))
    }

    pub(in crate::package) fn ssed_browse_display_text_for_index_row(
        &self,
        row: &SsedIndexRow,
        target: &TargetToken,
    ) -> Result<String> {
        let label = self.ssed_display_text_for_index_row(row);
        if !ssed_index_display_label_needs_body_fallback(&label) {
            return Ok(label);
        }
        let Some(title) = self.title_for_body_target(target)? else {
            return Ok(label);
        };
        let title = clean_ssed_index_display_label(&title);
        if ssed_index_display_label_needs_body_fallback(&title) {
            return Ok(label);
        }
        Ok(title)
    }

    pub(in crate::package) fn ssed_display_text_for_index_title_or_key(
        &self,
        title_pointer: SsedIndexPointer,
        fallback_key: &str,
    ) -> String {
        let title = self.ssed_title_text(title_pointer);
        let display = match title {
            Some(title) if !looks_like_raw_anchor_label(&title) => title,
            _ => fallback_key.to_owned(),
        };
        clean_ssed_index_display_label(&display)
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

fn next_ssed_partial_index_scan_cursor(
    catalog: &SsedCatalog,
    component_index: u8,
    page_index: usize,
) -> Option<SsedPartialIndexScanCursor> {
    let mut components = catalog
        .components_by_role(SsedComponentRole::Index)
        .filter(|component| {
            component.index >= component_index
                && component.has_positive_range()
                && is_supported_index_type(component.component_type)
        });
    let current = components.find(|component| component.index == component_index)?;
    if page_index < current.block_count() as usize {
        return Some(SsedPartialIndexScanCursor {
            component_index,
            page_index,
        });
    }
    catalog
        .components_by_role(SsedComponentRole::Index)
        .find(|component| {
            component.index > component_index
                && component.has_positive_range()
                && is_supported_index_type(component.component_type)
        })
        .map(|component| SsedPartialIndexScanCursor {
            component_index: component.index,
            page_index: 0,
        })
}

pub(super) fn ssed_index_page_prefilter_is_safe(component_type: u8) -> bool {
    is_simple_leaf_index_type(component_type) || is_body_only_simple_leaf_index_type(component_type)
}

fn next_ssed_prefiltered_index_scan_cursor(
    catalog: &SsedCatalog,
    mode: &SearchMode,
    include_simple_indexes: bool,
    component_index: u8,
    page_index: usize,
) -> Option<SsedPrefilteredIndexScanCursor> {
    let mut components = catalog
        .components_by_role(SsedComponentRole::Index)
        .filter(|component| {
            component.index >= component_index
                && component.has_positive_range()
                && is_supported_index_type(component.component_type)
                && ssed_prefiltered_index_component_may_match(
                    mode,
                    include_simple_indexes,
                    component,
                )
        });
    let current = components.find(|component| component.index == component_index)?;
    if page_index < current.block_count() as usize {
        return Some(SsedPrefilteredIndexScanCursor {
            component_index,
            page_index,
        });
    }
    catalog
        .components_by_role(SsedComponentRole::Index)
        .find(|component| {
            component.index > component_index
                && component.has_positive_range()
                && is_supported_index_type(component.component_type)
                && ssed_prefiltered_index_component_may_match(
                    mode,
                    include_simple_indexes,
                    component,
                )
        })
        .map(|component| SsedPrefilteredIndexScanCursor {
            component_index: component.index,
            page_index: 0,
        })
}

fn ssed_visible_index_key(row: &SsedIndexRow) -> String {
    let stripped = strip_ssed_index_disambiguation_marker(&row.key);
    if stripped == row.key {
        row.key.clone()
    } else {
        stripped.to_owned()
    }
}

pub(in crate::package::drivers) fn clean_ssed_index_display_label(value: &str) -> String {
    let mut value = value.trim();
    while let Some(rest) = value.strip_prefix('¶') {
        let rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        value = rest;
    }
    for marker in ['■', '§'] {
        if let Some((visible, _metadata)) = value.split_once(marker) {
            let visible = visible.trim();
            if !visible.is_empty() {
                value = visible;
            }
        }
    }
    value.trim().to_owned()
}

fn ssed_index_display_label_needs_body_fallback(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || looks_like_raw_anchor_label(value) {
        return true;
    }
    value.chars().all(is_unusable_index_display_char)
}

fn is_unusable_index_display_char(value: char) -> bool {
    matches!(value, '?' | '？' | '�' | '□' | '〓' | 'Δ')
}

fn strip_ssed_index_disambiguation_marker(value: &str) -> &str {
    let value = value.trim();
    if value.chars().count() < 2 {
        return value;
    }
    if let Some(rest) = value.strip_prefix(is_ssed_index_disambiguation_marker)
        && contains_japanese_label_char(rest)
    {
        return rest.trim();
    }
    if let Some(rest) = value.strip_suffix(is_ssed_index_disambiguation_marker)
        && contains_japanese_label_char(rest)
    {
        return rest.trim();
    }
    value
}

fn is_ssed_index_disambiguation_marker(value: char) -> bool {
    matches!(value, '?' | '？' | 'Δ')
}

fn contains_japanese_label_char(value: &str) -> bool {
    value.chars().any(|ch| {
        matches!(
            ch,
            '\u{3040}'..='\u{30ff}'
                | '\u{3400}'..='\u{9fff}'
                | '\u{f900}'..='\u{faff}'
                | '\u{ff66}'..='\u{ff9f}'
        )
    })
}

fn ssed_prefiltered_index_component_may_match(
    mode: &SearchMode,
    include_simple_indexes: bool,
    component: &SsedComponent,
) -> bool {
    if component.multi == 0xff {
        return false;
    }
    if !include_simple_indexes && is_simple_leaf_index_type(component.component_type) {
        return false;
    }
    let is_backward_index = ssed_index_component_name_is_backward(&component.filename);
    match mode {
        SearchMode::Exact | SearchMode::Forward => !is_backward_index,
        SearchMode::Backward => is_backward_index,
        _ => true,
    }
}

fn ssed_internal_key_is_ff_sentinel(raw_key: &[u8]) -> bool {
    !raw_key.is_empty() && raw_key.iter().all(|value| *value == 0xff)
}

pub(in crate::package::drivers) fn ssed_index_bound_is_plausible(
    start: SsedIndexPointer,
    end: SsedIndexPointer,
) -> bool {
    ssed_index_pointer_distance(start, end)
        .is_some_and(|distance| distance <= SSED_ADJACENT_INDEX_BODY_BOUND_MAX_BYTES)
}

fn ssed_plausible_adjacent_index_bound(
    row: &SsedIndexRow,
    next_row: Option<&SsedIndexRow>,
) -> Option<SsedIndexPointer> {
    next_row
        .filter(|next| next.body != row.body)
        .filter(|next| (next.body.block, next.body.offset) > (row.body.block, row.body.offset))
        .filter(|next| ssed_index_bound_is_plausible(row.body, next.body))
        .map(|next| next.body)
}

fn ssed_index_pointer_distance(start: SsedIndexPointer, end: SsedIndexPointer) -> Option<u64> {
    let start_abs = u64::from(start.block)
        .checked_mul(u64::from(BLOCK_SIZE))?
        .checked_add(u64::from(start.offset))?;
    let end_abs = u64::from(end.block)
        .checked_mul(u64::from(BLOCK_SIZE))?
        .checked_add(u64::from(end.offset))?;
    end_abs.checked_sub(start_abs)
}

fn ssed_component_read_base_for_header(
    component: &SsedComponent,
    header: &SsedDataHeader,
) -> usize {
    if component.start_block >= header.start_block && component.end_block <= header.end_block {
        return usize::try_from(component.start_block - header.start_block).unwrap_or(0);
    }
    0
}

pub(super) fn ssed_component_read_base(component: &SsedComponent, reader: &SsedDataFile) -> usize {
    ssed_component_read_base_for_header(component, reader.header())
}

#[derive(Default)]
struct SsedTaggedLeafPagePrefilterState {
    current_key_may_match: bool,
}

fn ssed_tagged_leaf_page_may_contain_query(
    component_type: u8,
    page: &[u8],
    candidates: &[Vec<u8>],
    state: &mut SsedTaggedLeafPagePrefilterState,
) -> Option<bool> {
    let pointer_len = if is_body_only_tagged_leaf_index_type(component_type) {
        6
    } else if is_tagged_leaf_index_type(component_type) {
        12
    } else {
        return None;
    };
    if page.len() < 4 {
        return Some(true);
    }

    let count = u16::from_be_bytes([page[2], page[3]]);
    let mut pos = 4usize;
    let mut subrecord = 0u16;
    let mut page_may_match = false;
    while subrecord < count && pos + 2 <= page.len() {
        let tag = page[pos];
        let key_len = page[pos + 1] as usize;
        if tag == 0 && key_len == 0 {
            break;
        }
        pos += 2;

        match tag {
            0x00 => {
                if pos + key_len + pointer_len > page.len() {
                    return Some(true);
                }
                if ssed_body_window_may_contain_query(&page[pos..pos + key_len], candidates) {
                    page_may_match = true;
                }
                pos += key_len + pointer_len;
            }
            0x80 => {
                if pos + 2 + key_len > page.len() {
                    return Some(true);
                }
                pos += 2;
                state.current_key_may_match =
                    ssed_body_window_may_contain_query(&page[pos..pos + key_len], candidates);
                pos += key_len;
            }
            0xc0 => {
                if pos + key_len + pointer_len > page.len() {
                    return Some(true);
                }
                if state.current_key_may_match
                    || ssed_body_window_may_contain_query(&page[pos..pos + key_len], candidates)
                {
                    page_may_match = true;
                }
                pos += key_len + pointer_len;
            }
            _ => return Some(true),
        }
        subrecord = subrecord.saturating_add(1);
    }
    Some(page_may_match)
}

fn ssed_candidate_pages_for_expanded_index(
    expanded: &[u8],
    component_read_base: usize,
    start_page: usize,
    page_count: usize,
    anchors: &[Vec<u8>],
) -> BTreeSet<usize> {
    let start_offset = component_page_offset(component_read_base, start_page);
    let end_offset = component_page_offset(component_read_base, page_count).min(expanded.len());
    if start_offset >= end_offset {
        return BTreeSet::new();
    }

    let mut pages = BTreeSet::new();
    let search_data = &expanded[start_offset..end_offset];
    for anchor in anchors {
        if anchor.is_empty() {
            continue;
        }
        for relative_offset in memchr::memmem::find_iter(search_data, anchor) {
            let absolute_offset = start_offset + relative_offset;
            let physical_page = absolute_offset / INDEX_PAGE_SIZE;
            if physical_page < component_read_base {
                continue;
            }
            let page_index = physical_page - component_read_base;
            if (start_page..page_count).contains(&page_index) {
                pages.insert(page_index);
            }
        }
    }
    pages
}

fn read_index_page_from_expanded(
    expanded: &[u8],
    component_read_base: usize,
    page_index: usize,
) -> &[u8] {
    let offset = component_page_offset(component_read_base, page_index);
    if offset >= expanded.len() {
        return &[];
    }
    let end = offset.saturating_add(INDEX_PAGE_SIZE).min(expanded.len());
    &expanded[offset..end]
}

fn component_page_offset(component_read_base: usize, page_index: usize) -> usize {
    component_read_base
        .saturating_add(page_index)
        .saturating_mul(INDEX_PAGE_SIZE)
}

pub(super) fn read_index_page(
    reader: &mut SsedDataFile,
    component_read_base: usize,
    page_index: usize,
) -> Result<&[u8]> {
    let offset = component_page_offset(component_read_base, page_index);
    if offset >= reader.header().expanded_size() {
        return Ok(&[]);
    }
    let chunk_index = offset / CHUNK_SIZE;
    let start = offset % CHUNK_SIZE;
    let chunk = reader.read_expanded_chunk(chunk_index)?;
    if start >= chunk.len() {
        return Ok(&[]);
    }
    let end = start.saturating_add(INDEX_PAGE_SIZE).min(chunk.len());
    Ok(&chunk[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_ff_key_is_sentinel_not_lower_bound() {
        let sentinel = vec![0xff, 0xff];
        assert!(ssed_internal_key_is_ff_sentinel(&sentinel));
        assert!(!ssed_internal_key_is_ff_sentinel(&[0x23, b'z']));
    }

    #[test]
    fn tagged_leaf_prefilter_carries_matching_group_key_to_continuation_page() {
        let candidates = ssed_index_page_prefilter_candidates("needle");
        let mut group_page = vec![0u8; INDEX_PAGE_SIZE];
        group_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        group_page[2..4].copy_from_slice(&1u16.to_be_bytes());
        group_page[4] = 0x80;
        group_page[5] = 11;
        group_page[6..8].copy_from_slice(&1u16.to_be_bytes());
        group_page[8..19].copy_from_slice(b"groupneedle");

        let mut continuation_page = vec![0u8; INDEX_PAGE_SIZE];
        continuation_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        continuation_page[2..4].copy_from_slice(&1u16.to_be_bytes());
        continuation_page[4] = 0xc0;
        continuation_page[5] = 0;

        let mut state = SsedTaggedLeafPagePrefilterState::default();
        assert_eq!(
            ssed_tagged_leaf_page_may_contain_query(0x90, &group_page, &candidates, &mut state),
            Some(false)
        );
        assert_eq!(
            ssed_tagged_leaf_page_may_contain_query(
                0x90,
                &continuation_page,
                &candidates,
                &mut state
            ),
            Some(true)
        );
    }
}

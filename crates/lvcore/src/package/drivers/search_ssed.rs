use std::{cell::Cell, collections::HashSet};

use super::*;
use crate::package::drivers::ssed_navigation::{
    ssed_aux_bound_target_row, ssed_aux_index_row_target,
};

const SSED_TITLE_LABEL_SEARCH_FALLBACK_MAX_ROWS: usize = 256;
const SSED_TITLE_LABEL_SEARCH_FALLBACK_EMPTY_PAGE_MAX_ROWS: usize = 20_480;
const SSED_INDEX_EMPTY_PHYSICAL_CURSOR_ADVANCE_LIMIT: usize = 2;
const SSED_PARTIAL_NONPREFIX_EMPTY_PHYSICAL_CURSOR_ADVANCE_LIMIT: usize = 8;
const SSED_PARTIAL_NONPREFIX_PREFILTERED_LEAF_PAGE_BUDGET: usize = 128;
const SSED_INDEX_EMPTY_PHYSICAL_SCAN_LEAF_PAGE_BUDGET: usize = 16;
const SSED_FULLTEXT_UNBOUNDED_TITLE_PREPASS_MAX_INDEX_BLOCKS: u32 = 2048;
const SSED_FULLTEXT_BODY_CURSOR_MAX_ROWS: usize = 4096;
const SSED_PARTIAL_EAGER_NONPREFIX_MAX_INDEX_BLOCKS: u32 = 256;
const SSED_SIDECAR_TITLE_CURSOR_PREFIX: &str = "sidecar-title:";
const SSED_TITLE_LABEL_CURSOR_PREFIX: &str = "ssed-title-label:";
const SSED_TITLE_LABEL_UNVERIFIED_CURSOR_PREFIX: &str = "ssed-title-label-unverified:";
const SSED_AUX_LABEL_CURSOR_PREFIX: &str = "ssed-aux-label:";
const SSED_UNVERIFIED_OFFSET_CURSOR_PREFIX: &str = "ssed-offset-unverified:";

impl ReaderBookPackage {
    pub(super) fn search_ssed_simple_indexes(&self, query: &SearchQuery) -> Result<SearchPage> {
        if query.limit == 0 {
            return Ok(SearchPage {
                hits: Vec::new(),
                next_cursor: None,
                result_sequence: None,
                diagnostics: Vec::new(),
            });
        }
        if query.mode == SearchMode::FullText {
            return self.search_ssed_fulltext_body_windows(query);
        }
        if matches!(query.mode, SearchMode::Advanced(_))
            && (!self.retained_ios_search_payloads.is_empty()
                || !self.retained_ios_full_db_payloads.is_empty())
        {
            return self.search_ssed_ios_search_dbs(query);
        }
        if !matches!(
            query.mode,
            SearchMode::Exact | SearchMode::Forward | SearchMode::Backward | SearchMode::Partial
        ) {
            return Ok(SearchPage::deferred(
                "SSED search mode is not implemented for simple title/index scanning yet",
            ));
        }
        let needle = normalize_search_match_text(&query.query);
        if let Some(aux_label_row_offset) = decode_ssed_aux_label_cursor(query.cursor.as_deref()) {
            return self.search_ssed_aux_index_labels(query, &needle, aux_label_row_offset);
        }
        if let Some(sidecar_cursor) = decode_ssed_sidecar_title_cursor(query.cursor.as_deref()) {
            return self.search_ssed_sidecar_title_page(query, sidecar_cursor, Vec::new());
        }
        if let Some(title_label_row_offset) =
            decode_ssed_title_label_cursor(query.cursor.as_deref())
        {
            return self.search_ssed_title_label_fallback_page(
                query,
                &needle,
                title_label_row_offset,
            );
        }
        let has_readable_ssed_indexes = self
            .ssed_catalog
            .as_ref()
            .is_some_and(|catalog| has_readable_ssed_index_payload(catalog, &self.storage));
        if !has_readable_ssed_indexes && self.has_searchable_ssed_aux_index()? {
            return self.search_ssed_aux_index_labels(query, &needle, 0);
        }
        if !has_readable_ssed_indexes {
            return if matches!(
                query.mode,
                SearchMode::Exact | SearchMode::Forward | SearchMode::Backward
            ) {
                self.search_ssed_sidecar_title_page(
                    query,
                    SsedSidecarTitleCursor::Offset(0),
                    Vec::new(),
                )
            } else {
                Ok(SearchPage {
                    hits: Vec::new(),
                    next_cursor: None,
                    result_sequence: None,
                    diagnostics: Vec::new(),
                })
            };
        }
        let mut dense_sidecar_titles_preferred = None;
        let mut sidecar_title_prepass_exhausted_empty = false;
        if query.cursor.is_none() && ssed_sidecar_title_auto_append_is_bounded(&query.query) {
            let mut diagnostics = Vec::new();
            let prefers_dense_sidecar_titles =
                self.ssed_simple_search_should_prefer_dense_sidecar_titles(&mut diagnostics)?;
            dense_sidecar_titles_preferred = Some(prefers_dense_sidecar_titles);
            if prefers_dense_sidecar_titles {
                let page = self.search_ssed_sidecar_title_page(
                    query,
                    SsedSidecarTitleCursor::Offset(0),
                    diagnostics,
                )?;
                if !page.hits.is_empty() || page.next_cursor.is_some() {
                    return Ok(page);
                }
                sidecar_title_prepass_exhausted_empty = true;
            }
        }

        if query.mode == SearchMode::Partial {
            if let Some(prefix_cursor) = decode_ssed_partial_prefix_cursor(query.cursor.as_deref())
            {
                if let Some(page) =
                    self.search_ssed_partial_prefix_page(query, &needle, Some(prefix_cursor))?
                {
                    return Ok(page);
                }
            } else if let Some(nonprefix_cursor) =
                decode_ssed_partial_nonprefix_cursor(query.cursor.as_deref())
            {
                return self.search_ssed_partial_nonprefix_page(
                    query,
                    &needle,
                    Some(nonprefix_cursor),
                    true,
                );
            } else if query.cursor.is_none() && ssed_partial_prefix_prepass_is_bounded(&query.query)
            {
                if let Some(page) = self.search_ssed_partial_prefix_page(query, &needle, None)? {
                    return Ok(page);
                }
                return self.search_ssed_partial_nonprefix_page(query, &needle, None, false);
            } else if query.cursor.is_none() {
                return self.search_ssed_partial_nonprefix_page(query, &needle, None, false);
            }
        }

        let partial_scan_cursor = if query.mode == SearchMode::Partial {
            decode_ssed_partial_index_scan_cursor(query.cursor.as_deref())
        } else {
            None
        };
        let prefiltered_scan_cursor = if matches!(
            query.mode,
            SearchMode::Exact | SearchMode::Forward | SearchMode::Backward
        ) {
            decode_ssed_prefiltered_index_scan_cursor(query.cursor.as_deref())
        } else {
            None
        };
        let uses_native_offset_cursor =
            partial_scan_cursor.is_none() && prefiltered_scan_cursor.is_none();
        let offset = if uses_native_offset_cursor {
            decode_ssed_unverified_offset_cursor(query.cursor.as_deref())
                .unwrap_or_else(|| decode_offset_cursor(query.cursor.as_deref()))
        } else {
            0
        };
        let defer_native_offset_overfetch = uses_native_offset_cursor
            && query.cursor.is_some()
            && matches!(
                query.mode,
                SearchMode::Exact | SearchMode::Forward | SearchMode::Backward
            );
        let page_limit = if defer_native_offset_overfetch {
            query.limit
        } else {
            query.limit.saturating_add(1)
        };
        let gaiji_policy = query.label_gaiji_policy();
        let mut pending_empty_title_label_fallback_diagnostics = Vec::new();
        let mut collector = SsedIndexSearchCollector::new(
            self,
            &query.mode,
            &needle,
            offset,
            page_limit,
            gaiji_policy,
        )
        .with_display_label_matching();
        let mut optimized_scan_components = 0usize;
        let mut scan_needs_prefilter_fallback = false;
        let mut optimized_diagnostics = Vec::new();
        let mut physical_next_cursor = None;
        if prefiltered_scan_cursor.is_some() {
            physical_next_cursor = self.scan_ssed_prefiltered_index_rows_paged_until_visible(
                &query.mode,
                &needle,
                true,
                prefiltered_scan_cursor,
                &mut collector,
            )?;
        } else if matches!(
            query.mode,
            SearchMode::Exact | SearchMode::Forward | SearchMode::Backward
        ) {
            let candidate_has_hits = Cell::new(false);
            let scan_result = self.scan_ssed_simple_leaf_index_rows_near_key(
                &query.mode,
                &needle,
                |row| {
                    let keep_scanning = collector.push_row(row)?;
                    if collector.has_hits() {
                        candidate_has_hits.set(true);
                    }
                    Ok(keep_scanning)
                },
                || candidate_has_hits.get(),
            )?;
            optimized_scan_components = scan_result.scanned_components;
            scan_needs_prefilter_fallback = scan_result.needs_prefilter_fallback;
            optimized_diagnostics.extend(scan_result.diagnostics);
            collector.extend_diagnostics(optimized_diagnostics.clone());
        }
        if !collector.has_hits() && optimized_scan_components == 0 {
            let scan_diagnostics = if query.mode == SearchMode::Partial {
                physical_next_cursor = self.scan_ssed_partial_index_rows_paged_until_visible(
                    &needle,
                    partial_scan_cursor,
                    &mut collector,
                )?;
                Vec::new()
            } else if matches!(
                query.mode,
                SearchMode::Exact | SearchMode::Forward | SearchMode::Backward
            ) && scan_needs_prefilter_fallback
            {
                physical_next_cursor = self.scan_ssed_prefiltered_index_rows_paged_until_visible(
                    &query.mode,
                    &needle,
                    true,
                    None,
                    &mut collector,
                )?;
                Vec::new()
            } else {
                self.scan_ssed_simple_index_rows(None, |row| collector.push_row(row))?
            };
            collector.extend_diagnostics(scan_diagnostics);
        } else if matches!(
            query.mode,
            SearchMode::Exact | SearchMode::Forward | SearchMode::Backward
        ) && collector.needs_more_hits()
            && scan_needs_prefilter_fallback
        {
            let mut fallback_collector = SsedIndexSearchCollector::new(
                self,
                &query.mode,
                &needle,
                offset,
                page_limit,
                query.label_gaiji_policy(),
            )
            .with_display_label_matching();
            fallback_collector.extend_diagnostics(optimized_diagnostics);
            physical_next_cursor = self.scan_ssed_prefiltered_index_rows_paged_until_visible(
                &query.mode,
                &needle,
                true,
                None,
                &mut fallback_collector,
            )?;
            collector = fallback_collector;
        }
        if !collector.has_hits()
            && query.cursor.is_none()
            && matches!(
                query.mode,
                SearchMode::Exact | SearchMode::Forward | SearchMode::Backward
            )
        {
            if ssed_title_label_fallback_is_reasonable(&query.mode, &needle) {
                let mut fallback_page =
                    self.search_ssed_title_label_fallback_page(query, &needle, 0)?;
                if fallback_page.hits.is_empty() && fallback_page.next_cursor.is_none() {
                    let mut immediate_diagnostics = Vec::new();
                    for diagnostic in fallback_page.diagnostics {
                        if diagnostic.code == "ssed_title_label_search_fallback_no_hit_limited" {
                            pending_empty_title_label_fallback_diagnostics.push(diagnostic);
                        } else {
                            immediate_diagnostics.push(diagnostic);
                        }
                    }
                    collector.extend_diagnostics(immediate_diagnostics);
                } else {
                    if fallback_page.next_cursor.is_none()
                        && fallback_page.hits.len() < query.limit
                        && ssed_sidecar_title_auto_append_is_bounded(&query.query)
                        && !sidecar_title_prepass_exhausted_empty
                        && self.ssed_should_append_sidecar_titles_after_native_page(
                            &fallback_page,
                            dense_sidecar_titles_preferred,
                        )?
                    {
                        self.append_ssed_sidecar_title_hits(
                            query,
                            &mut fallback_page,
                            SsedSidecarTitleCursor::Offset(0),
                        )?;
                    }
                    return Ok(fallback_page);
                }
            } else {
                collector.extend_diagnostics(vec![Diagnostic::info(
                    "ssed_title_label_search_fallback_skipped_short_query",
                    "SSED title-label fallback search was skipped for a short exact/backward query after native index search found no hits",
                )]);
            }
        }
        let mut page = collector.into_search_page(query.limit);
        if defer_native_offset_overfetch
            && page.next_cursor.is_none()
            && page.hits.len() == query.limit
        {
            page.next_cursor = Some(encode_ssed_unverified_offset_cursor(
                offset.saturating_add(query.limit),
            ));
        }
        if page.next_cursor.is_none() {
            page.next_cursor = physical_next_cursor;
        }
        if page.next_cursor.is_none()
            && query.cursor.is_none()
            && page.hits.len() < query.limit
            && ssed_sidecar_title_auto_append_is_bounded(&query.query)
            && !sidecar_title_prepass_exhausted_empty
            && self.ssed_should_append_sidecar_titles_after_native_page(
                &page,
                dense_sidecar_titles_preferred,
            )?
        {
            self.append_ssed_sidecar_title_hits(
                query,
                &mut page,
                SsedSidecarTitleCursor::Offset(0),
            )?;
        }
        if page.hits.is_empty() {
            page.diagnostics
                .extend(pending_empty_title_label_fallback_diagnostics);
        }
        Ok(page)
    }

    fn has_searchable_ssed_aux_index(&self) -> Result<bool> {
        for spec in self.ssed_aux_index_specs()? {
            if !path_has_extension(&spec.info, &["idx"]) {
                continue;
            }
            let path = Path::new(&spec.info);
            if !self.storage.exists(path)? {
                continue;
            }
            let rows = parse_aux_index_text_bytes(&self.storage.read(path)?)?;
            if rows.iter().any(|row| row.has_target()) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn search_ssed_aux_index_labels(
        &self,
        query: &SearchQuery,
        needle: &str,
        row_offset: usize,
    ) -> Result<SearchPage> {
        let label_policy = query.label_gaiji_policy();
        let mut checked_rows = 0usize;
        let mut hits = Vec::new();
        let mut diagnostics = Vec::new();
        let mut seen_targets = HashSet::new();
        let mut stopped = SsedAuxLabelSearchStop::Exhausted;

        'specs: for spec in self.ssed_aux_index_specs()? {
            if !path_has_extension(&spec.info, &["idx"]) {
                continue;
            }
            let path = Path::new(&spec.info);
            if !self.storage.exists(path)? {
                continue;
            }
            let rows = parse_aux_index_text_bytes(&self.storage.read(path)?)?;
            for row in &rows {
                if checked_rows < row_offset {
                    checked_rows = checked_rows.saturating_add(1);
                    continue;
                }
                checked_rows = checked_rows.saturating_add(1);
                if !row.has_target()
                    || !ssed_aux_label_search_row_matches(&query.mode, needle, &row.label)
                {
                    continue;
                }
                let mut target_diagnostics = Vec::new();
                let target = ssed_aux_index_row_target(
                    self,
                    row,
                    ssed_aux_bound_target_row(&rows, row),
                    &mut target_diagnostics,
                )?;
                let Some(target) = target else {
                    diagnostics.extend(target_diagnostics);
                    continue;
                };
                if !seen_targets.insert(target.as_str().to_owned()) {
                    continue;
                }
                let label = self.ssed_rich_label_with_policy(&row.label, &label_policy);
                let href = target.href();
                let mut hit_diagnostics = target_diagnostics;
                hit_diagnostics.extend(label.diagnostics);
                hits.push(SearchHit {
                    href,
                    book_id: self.metadata.book_id.clone(),
                    target,
                    title_html: label.html,
                    title_text: label.text,
                    snippet_html: None,
                    sequence_hint: None,
                    diagnostics: hit_diagnostics,
                });
                if hits.len() >= query.limit {
                    stopped = SsedAuxLabelSearchStop::PageFull;
                    break 'specs;
                }
            }
        }

        if !hits.is_empty() {
            diagnostics.insert(
                0,
                Diagnostic::info(
                    "ssed_auxiliary_index_label_search",
                    "SSED search used EXINFO auxiliary text index labels",
                ),
            );
        }
        let next_cursor = match stopped {
            SsedAuxLabelSearchStop::Exhausted => None,
            SsedAuxLabelSearchStop::PageFull => Some(encode_ssed_aux_label_cursor(checked_rows)),
        };
        Ok(SearchPage {
            hits,
            next_cursor,
            result_sequence: None,
            diagnostics,
        })
    }

    fn search_ssed_partial_prefix_page(
        &self,
        query: &SearchQuery,
        needle: &str,
        cursor: Option<String>,
    ) -> Result<Option<SearchPage>> {
        let mut prefix_query = query.clone();
        prefix_query.mode = SearchMode::Forward;
        prefix_query.cursor = cursor;
        let mut page = self.search_ssed_simple_indexes(&prefix_query)?;
        if page.hits.is_empty() && page.next_cursor.is_none() {
            return Ok(None);
        }
        page.next_cursor = page
            .next_cursor
            .take()
            .map(encode_ssed_partial_prefix_cursor);
        if page.next_cursor.is_none() {
            let remaining = query.limit.saturating_sub(page.hits.len());
            if !page.hits.is_empty() && self.ssed_partial_nonprefix_fill_should_defer() {
                page.next_cursor = Some(encode_ssed_partial_unverified_nonprefix_cursor(None));
            } else if remaining > 0 {
                if !page.hits.is_empty() {
                    page.next_cursor = self
                        .ssed_deferred_partial_nonprefix_cursor_if_visible(query, needle, true)?;
                } else {
                    let mut nonprefix_query = query.clone();
                    nonprefix_query.limit = remaining;
                    let mut nonprefix_page = self.search_ssed_partial_nonprefix_page(
                        &nonprefix_query,
                        needle,
                        None,
                        true,
                    )?;
                    page.hits.append(&mut nonprefix_page.hits);
                    page.diagnostics.extend(nonprefix_page.diagnostics);
                    page.next_cursor = nonprefix_page.next_cursor;
                }
            } else {
                page.next_cursor =
                    self.ssed_deferred_partial_nonprefix_cursor_if_visible(query, needle, true)?;
            }
        }
        page.diagnostics.insert(
            0,
            Diagnostic::info(
                "ssed_partial_prefix_prepass",
                "SSED partial search returned native prefix/title-index matches before scanning non-prefix contains matches",
            ),
        );
        Ok(Some(page))
    }

    fn ssed_deferred_partial_nonprefix_cursor_if_visible(
        &self,
        query: &SearchQuery,
        needle: &str,
        skip_prefix_rows: bool,
    ) -> Result<Option<String>> {
        let mut probe_query = query.clone();
        probe_query.limit = 1;
        let probe_page = self.search_ssed_partial_nonprefix_page_inner(
            &probe_query,
            needle,
            None,
            skip_prefix_rows,
        )?;
        if probe_page.page.hits.is_empty() {
            return Ok(None);
        }
        Ok(probe_page
            .visible_start_cursor
            .map(|cursor| encode_ssed_partial_nonprefix_cursor(Some(cursor), skip_prefix_rows))
            .or_else(|| Some(encode_ssed_partial_nonprefix_cursor(None, skip_prefix_rows))))
    }

    fn ssed_partial_nonprefix_fill_should_defer(&self) -> bool {
        let Some(catalog) = &self.ssed_catalog else {
            return false;
        };
        let skip_backward_rows = self.ssed_has_forward_browse_index();
        let mut block_count = 0u32;
        for component in catalog.components_by_role(SsedComponentRole::Index) {
            if skip_backward_rows && ssed_index_component_name_is_backward(&component.filename) {
                continue;
            }
            if !is_supported_index_type(component.component_type) {
                continue;
            }
            block_count = block_count.saturating_add(component.block_count());
            if block_count > SSED_PARTIAL_EAGER_NONPREFIX_MAX_INDEX_BLOCKS {
                return true;
            }
        }
        false
    }

    fn search_ssed_partial_nonprefix_page(
        &self,
        query: &SearchQuery,
        needle: &str,
        cursor: Option<SsedPartialNonprefixCursor>,
        skip_prefix_rows: bool,
    ) -> Result<SearchPage> {
        Ok(self
            .search_ssed_partial_nonprefix_page_inner(query, needle, cursor, skip_prefix_rows)?
            .page)
    }

    fn search_ssed_partial_nonprefix_page_inner(
        &self,
        query: &SearchQuery,
        needle: &str,
        cursor: Option<SsedPartialNonprefixCursor>,
        skip_prefix_rows: bool,
    ) -> Result<SsedPartialNonprefixSearchPage> {
        let page_limit = query.limit.saturating_add(1);
        let skip_prefix_rows = cursor
            .as_ref()
            .map(SsedPartialNonprefixCursor::skip_prefix_rows)
            .unwrap_or(skip_prefix_rows);
        let offset = match cursor {
            Some(
                SsedPartialNonprefixCursor::MatchedOffset { offset, .. }
                | SsedPartialNonprefixCursor::MatchedPhysicalOffset { offset, .. },
            ) => offset,
            Some(
                SsedPartialNonprefixCursor::Physical { .. }
                | SsedPartialNonprefixCursor::UnverifiedPhysical { .. },
            )
            | None => 0,
        };
        let physical_cursor = match cursor {
            Some(
                SsedPartialNonprefixCursor::Physical { cursor, .. }
                | SsedPartialNonprefixCursor::UnverifiedPhysical { cursor, .. }
                | SsedPartialNonprefixCursor::MatchedPhysicalOffset { cursor, .. },
            ) => Some(cursor),
            Some(SsedPartialNonprefixCursor::MatchedOffset { .. }) | None => None,
        };
        let offset_cursor_physical_start = match cursor {
            Some(
                SsedPartialNonprefixCursor::Physical { .. }
                | SsedPartialNonprefixCursor::MatchedPhysicalOffset { .. },
            ) => physical_cursor,
            Some(
                SsedPartialNonprefixCursor::MatchedOffset { .. }
                | SsedPartialNonprefixCursor::UnverifiedPhysical { .. },
            )
            | None => None,
        };
        let mut collector = SsedIndexSearchCollector::new(
            self,
            &SearchMode::Partial,
            needle,
            offset,
            page_limit,
            query.label_gaiji_policy(),
        )
        .with_display_label_matching();
        let physical_scan = self.scan_ssed_partial_nonprefix_index_rows_paged_until_visible(
            needle,
            physical_cursor,
            &mut collector,
            skip_prefix_rows,
        )?;
        let visible_start_cursor = physical_scan.visible_start_cursor;
        let matched_offset_physical_start = offset_cursor_physical_start.or(visible_start_cursor);
        let mut page = collector.into_search_page(query.limit);
        page.next_cursor = page
            .next_cursor
            .take()
            .map(|cursor| {
                if let Some(physical_start) = matched_offset_physical_start {
                    encode_ssed_partial_nonprefix_physical_offset_cursor(
                        physical_start,
                        cursor,
                        skip_prefix_rows,
                    )
                } else {
                    encode_ssed_partial_nonprefix_offset_cursor(cursor, skip_prefix_rows)
                }
            })
            .or(physical_scan.next_cursor);
        Ok(SsedPartialNonprefixSearchPage {
            page,
            visible_start_cursor,
        })
    }

    fn scan_ssed_partial_index_rows_paged_until_visible(
        &self,
        needle: &str,
        cursor: Option<SsedPartialIndexScanCursor>,
        collector: &mut SsedIndexSearchCollector<'_>,
    ) -> Result<Option<String>> {
        self.scan_ssed_partial_index_rows_paged_until_visible_with_prefilter_budget(
            needle,
            cursor,
            collector,
            SSED_PARTIAL_INDEX_PREFILTERED_LEAF_PAGE_BUDGET,
        )
    }

    fn scan_ssed_partial_index_rows_paged_until_visible_with_prefilter_budget(
        &self,
        needle: &str,
        cursor: Option<SsedPartialIndexScanCursor>,
        collector: &mut SsedIndexSearchCollector<'_>,
        prefiltered_leaf_page_budget: usize,
    ) -> Result<Option<String>> {
        let mut current_cursor = cursor;
        let mut advanced_empty_pages = 0usize;
        let mut use_empty_scan_budget = false;
        loop {
            let scan_result = if prefiltered_leaf_page_budget
                == SSED_PARTIAL_INDEX_PREFILTERED_LEAF_PAGE_BUDGET
            {
                if use_empty_scan_budget {
                    self.scan_ssed_partial_index_rows_paged_with_leaf_budget(
                        needle,
                        current_cursor,
                        SSED_INDEX_EMPTY_PHYSICAL_SCAN_LEAF_PAGE_BUDGET,
                        |row| collector.push_row(row),
                    )?
                } else {
                    self.scan_ssed_partial_index_rows_paged(needle, current_cursor, |row| {
                        collector.push_row(row)
                    })?
                }
            } else if use_empty_scan_budget {
                self.scan_ssed_partial_index_rows_paged_with_leaf_budget_and_cursor(
                    needle,
                    current_cursor,
                    SSED_INDEX_EMPTY_PHYSICAL_SCAN_LEAF_PAGE_BUDGET,
                    prefiltered_leaf_page_budget,
                    |_, row| collector.push_row(row),
                )?
            } else {
                self.scan_ssed_partial_index_rows_paged_with_leaf_budget_and_cursor(
                    needle,
                    current_cursor,
                    SSED_PARTIAL_INDEX_SCAN_LEAF_PAGE_BUDGET,
                    prefiltered_leaf_page_budget,
                    |_, row| collector.push_row(row),
                )?
            };
            let next_cursor = scan_result.next_cursor;
            collector.extend_diagnostics(scan_result.diagnostics);
            if collector.has_hits() || next_cursor.is_none() {
                self.record_ssed_empty_physical_scan_advances(
                    collector,
                    advanced_empty_pages,
                    next_cursor.as_deref(),
                    false,
                );
                return Ok(next_cursor);
            }
            if advanced_empty_pages >= SSED_INDEX_EMPTY_PHYSICAL_CURSOR_ADVANCE_LIMIT {
                self.record_ssed_empty_physical_scan_advances(
                    collector,
                    advanced_empty_pages,
                    next_cursor.as_deref(),
                    true,
                );
                return Ok(next_cursor);
            }
            advanced_empty_pages = advanced_empty_pages.saturating_add(1);
            use_empty_scan_budget = true;
            let decoded = decode_ssed_partial_index_scan_cursor(next_cursor.as_deref());
            if decoded.is_none() {
                collector.extend_diagnostics(vec![
                    Diagnostic::warning(
                        "ssed_index_physical_cursor_decode_failed",
                        "SSED partial index scan produced an unreadable continuation cursor",
                    )
                    .with_context("next_cursor", next_cursor.clone().unwrap_or_default()),
                ]);
                return Ok(next_cursor);
            }
            current_cursor = decoded;
        }
    }

    fn scan_ssed_partial_nonprefix_index_rows_paged_until_visible(
        &self,
        needle: &str,
        cursor: Option<SsedPartialIndexScanCursor>,
        collector: &mut SsedIndexSearchCollector<'_>,
        skip_prefix_rows: bool,
    ) -> Result<SsedPartialNonprefixPhysicalScan> {
        let mut current_cursor = cursor;
        let mut advanced_empty_pages = 0usize;
        let mut use_empty_scan_budget = false;
        let mut first_visible_row_cursor = None;
        let has_page_prefilter_candidates =
            !ssed_index_page_prefilter_candidates(needle).is_empty();
        let prefiltered_leaf_page_budget = has_page_prefilter_candidates
            .then_some(SSED_PARTIAL_NONPREFIX_PREFILTERED_LEAF_PAGE_BUDGET);
        loop {
            let scan_start_cursor = current_cursor;
            let fallback_leaf_page_budget = if use_empty_scan_budget {
                SSED_INDEX_EMPTY_PHYSICAL_SCAN_LEAF_PAGE_BUDGET
            } else {
                SSED_PARTIAL_INDEX_SCAN_LEAF_PAGE_BUDGET
            };
            let leaf_page_budget =
                prefiltered_leaf_page_budget.unwrap_or(fallback_leaf_page_budget);
            let scan_result = self.scan_ssed_partial_index_rows_paged_with_leaf_budget_and_cursor(
                needle,
                current_cursor,
                leaf_page_budget,
                SSED_PARTIAL_INDEX_PREFILTERED_LEAF_PAGE_BUDGET,
                |row_cursor, row| {
                    if skip_prefix_rows
                        && ssed_title_label_fallback_row_matches(
                            self,
                            &SearchMode::Forward,
                            needle,
                            &row,
                        )
                    {
                        return Ok(true);
                    }
                    let had_hits = collector.has_hits();
                    let keep_scanning = collector.push_row(row)?;
                    if !had_hits && collector.has_hits() && first_visible_row_cursor.is_none() {
                        first_visible_row_cursor = Some(row_cursor);
                    }
                    Ok(keep_scanning)
                },
            )?;
            let next_cursor = scan_result.next_cursor;
            collector.extend_diagnostics(scan_result.diagnostics);
            if collector.has_hits() || next_cursor.is_none() {
                let next_cursor = next_cursor
                    .as_deref()
                    .and_then(|cursor| decode_ssed_partial_index_scan_cursor(Some(cursor)))
                    .map(|cursor| {
                        encode_ssed_partial_nonprefix_cursor(Some(cursor), skip_prefix_rows)
                    });
                self.record_ssed_empty_physical_scan_advances(
                    collector,
                    advanced_empty_pages,
                    next_cursor.as_deref(),
                    false,
                );
                let visible_start_cursor = if collector.has_hits() {
                    first_visible_row_cursor.or(scan_start_cursor)
                } else {
                    None
                };
                return Ok(SsedPartialNonprefixPhysicalScan {
                    next_cursor,
                    visible_start_cursor,
                });
            }
            if advanced_empty_pages >= SSED_PARTIAL_NONPREFIX_EMPTY_PHYSICAL_CURSOR_ADVANCE_LIMIT {
                let next_cursor = next_cursor
                    .as_deref()
                    .and_then(|cursor| decode_ssed_partial_index_scan_cursor(Some(cursor)))
                    .map(|cursor| {
                        encode_ssed_partial_nonprefix_cursor(Some(cursor), skip_prefix_rows)
                    });
                self.record_ssed_empty_physical_scan_advances(
                    collector,
                    advanced_empty_pages,
                    next_cursor.as_deref(),
                    true,
                );
                return Ok(SsedPartialNonprefixPhysicalScan {
                    next_cursor,
                    visible_start_cursor: None,
                });
            }
            advanced_empty_pages = advanced_empty_pages.saturating_add(1);
            use_empty_scan_budget = true;
            let decoded = decode_ssed_partial_index_scan_cursor(next_cursor.as_deref());
            if decoded.is_none() {
                collector.extend_diagnostics(vec![
                    Diagnostic::warning(
                        "ssed_index_physical_cursor_decode_failed",
                        "SSED partial non-prefix index scan produced an unreadable continuation cursor",
                    )
                    .with_context("next_cursor", next_cursor.clone().unwrap_or_default()),
                ]);
                return Ok(SsedPartialNonprefixPhysicalScan {
                    next_cursor,
                    visible_start_cursor: None,
                });
            }
            current_cursor = decoded;
        }
    }

    fn scan_ssed_prefiltered_index_rows_paged_until_visible(
        &self,
        mode: &SearchMode,
        needle: &str,
        include_simple_indexes: bool,
        cursor: Option<SsedPrefilteredIndexScanCursor>,
        collector: &mut SsedIndexSearchCollector<'_>,
    ) -> Result<Option<String>> {
        let mut current_cursor = cursor;
        let mut advanced_empty_pages = 0usize;
        let mut use_empty_scan_budget = false;
        loop {
            let scan_result = if use_empty_scan_budget {
                self.scan_ssed_prefiltered_index_rows_paged_with_leaf_budget(
                    mode,
                    needle,
                    include_simple_indexes,
                    current_cursor,
                    SSED_INDEX_EMPTY_PHYSICAL_SCAN_LEAF_PAGE_BUDGET,
                    |row| collector.push_row(row),
                )?
            } else {
                self.scan_ssed_prefiltered_index_rows_paged(
                    mode,
                    needle,
                    include_simple_indexes,
                    current_cursor,
                    |row| collector.push_row(row),
                )?
            };
            let next_cursor = scan_result.next_cursor;
            collector.extend_diagnostics(scan_result.diagnostics);
            if collector.has_hits() || next_cursor.is_none() {
                self.record_ssed_empty_physical_scan_advances(
                    collector,
                    advanced_empty_pages,
                    next_cursor.as_deref(),
                    false,
                );
                return Ok(next_cursor);
            }
            if advanced_empty_pages >= SSED_INDEX_EMPTY_PHYSICAL_CURSOR_ADVANCE_LIMIT {
                self.record_ssed_empty_physical_scan_advances(
                    collector,
                    advanced_empty_pages,
                    next_cursor.as_deref(),
                    true,
                );
                return Ok(next_cursor);
            }
            advanced_empty_pages = advanced_empty_pages.saturating_add(1);
            use_empty_scan_budget = true;
            let decoded = decode_ssed_prefiltered_index_scan_cursor(next_cursor.as_deref());
            if decoded.is_none() {
                collector.extend_diagnostics(vec![
                    Diagnostic::warning(
                        "ssed_index_physical_cursor_decode_failed",
                        "SSED prefiltered index scan produced an unreadable continuation cursor",
                    )
                    .with_context("next_cursor", next_cursor.clone().unwrap_or_default()),
                ]);
                return Ok(next_cursor);
            }
            current_cursor = decoded;
        }
    }

    fn record_ssed_empty_physical_scan_advances(
        &self,
        collector: &mut SsedIndexSearchCollector<'_>,
        advanced_empty_pages: usize,
        next_cursor: Option<&str>,
        limited: bool,
    ) {
        if advanced_empty_pages == 0 {
            return;
        }
        let mut diagnostic = if limited {
            Diagnostic::info(
                "ssed_index_empty_physical_scan_limited",
                "SSED native index search advanced empty physical scan pages and stopped at the bounded cursor budget",
            )
        } else {
            Diagnostic::info(
                "ssed_index_empty_physical_pages_skipped",
                "SSED native index search advanced empty physical scan pages before returning a user-visible page",
            )
        }
        .with_context("advanced_empty_pages", advanced_empty_pages.to_string());
        if let Some(next_cursor) = next_cursor {
            diagnostic = diagnostic.with_context("next_cursor", next_cursor.to_owned());
        }
        collector.extend_diagnostics(vec![diagnostic]);
    }

    fn search_ssed_title_label_fallback_page(
        &self,
        query: &SearchQuery,
        needle: &str,
        row_offset: usize,
    ) -> Result<SearchPage> {
        self.search_ssed_title_label_fallback_page_inner(query, needle, row_offset)
    }

    fn search_ssed_title_label_fallback_page_inner(
        &self,
        query: &SearchQuery,
        needle: &str,
        row_offset: usize,
    ) -> Result<SearchPage> {
        let label_policy = query.label_gaiji_policy();
        let skip_backward_rows = self.ssed_has_forward_browse_index();
        let mut checked_rows = 0usize;
        let mut scanned_rows = 0usize;
        let mut hits = Vec::new();
        let mut diagnostics = Vec::new();
        let mut seen_targets = HashSet::new();
        let mut stopped = SsedTitleLabelFallbackStop::Exhausted;
        let fallback_diagnostics = self.scan_ssed_ordered_index_rows_with_filters(
            None,
            |component| {
                if skip_backward_rows && ssed_index_component_name_is_backward(&component.filename)
                {
                    return false;
                }
                self.resolve_readable_ssed_component_path(component)
                    .ok()
                    .flatten()
                    .is_some()
            },
            |row| {
                if checked_rows < row_offset {
                    checked_rows = checked_rows.saturating_add(1);
                    return Ok(true);
                }
                let row_budget = if hits.is_empty() {
                    SSED_TITLE_LABEL_SEARCH_FALLBACK_EMPTY_PAGE_MAX_ROWS
                } else {
                    SSED_TITLE_LABEL_SEARCH_FALLBACK_MAX_ROWS
                };
                if scanned_rows >= row_budget {
                    stopped = SsedTitleLabelFallbackStop::Budget;
                    return Ok(false);
                }
                checked_rows = checked_rows.saturating_add(1);
                scanned_rows = scanned_rows.saturating_add(1);
                if !ssed_title_label_fallback_row_matches(self, &query.mode, needle, &row) {
                    return Ok(true);
                }
                if self.ssed_index_row_body_pointer_is_outside_catalog_range(&row) {
                    return Ok(true);
                }
                let body_key = format!("{:08x}:{:04x}", row.body.block, row.body.offset);
                if !seen_targets.insert(body_key) {
                    return Ok(true);
                }
                let target = match self.ssed_target_for_search_index_row(&row)? {
                    Ok(target) => target,
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                        return Ok(true);
                    }
                };
                let title = self.ssed_display_text_for_index_row(&row);
                let label = self.ssed_rich_label_with_policy(&title, &label_policy);
                let href = target.href();
                hits.push(SearchHit {
                    href,
                    book_id: self.book_id_for_hit(),
                    target,
                    title_html: label.html,
                    title_text: label.text,
                    snippet_html: None,
                    sequence_hint: None,
                    diagnostics: label.diagnostics,
                });
                if hits.len() >= query.limit {
                    stopped = SsedTitleLabelFallbackStop::PageFull;
                    return Ok(false);
                }
                Ok(true)
            },
        )?;
        diagnostics.extend(fallback_diagnostics);
        let next_cursor = match stopped {
            SsedTitleLabelFallbackStop::Exhausted => None,
            SsedTitleLabelFallbackStop::Budget if hits.is_empty() => None,
            SsedTitleLabelFallbackStop::Budget => {
                Some(encode_ssed_title_label_cursor(checked_rows))
            }
            SsedTitleLabelFallbackStop::PageFull => {
                Some(encode_ssed_unverified_title_label_cursor(checked_rows))
            }
        };
        if matches!(stopped, SsedTitleLabelFallbackStop::Budget) {
            let mut diagnostic = if hits.is_empty() {
                Diagnostic::info(
                    "ssed_title_label_search_fallback_no_hit_limited",
                    "SSED title-label fallback search reached its bounded no-hit row budget",
                )
            } else {
                Diagnostic::info(
                    "ssed_title_label_search_fallback_limited",
                    "SSED title-label fallback search reached its bounded row budget before exhausting all title/index rows",
                )
            }
            .with_context("checked_rows", checked_rows.to_string())
            .with_context("scanned_rows", scanned_rows.to_string());
            if let Some(next_cursor) = &next_cursor {
                diagnostic = diagnostic.with_context("next_cursor", next_cursor.clone());
            }
            diagnostics.push(diagnostic);
        }
        Ok(SearchPage {
            hits,
            next_cursor,
            result_sequence: None,
            diagnostics,
        })
    }

    fn search_ssed_sidecar_title_page(
        &self,
        query: &SearchQuery,
        sidecar_cursor: SsedSidecarTitleCursor,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<SearchPage> {
        let mut page = SearchPage {
            hits: Vec::new(),
            next_cursor: None,
            result_sequence: None,
            diagnostics,
        };
        self.append_ssed_sidecar_title_hits(query, &mut page, sidecar_cursor)?;
        Ok(page)
    }

    fn append_ssed_sidecar_title_hits(
        &self,
        query: &SearchQuery,
        page: &mut SearchPage,
        sidecar_cursor: SsedSidecarTitleCursor,
    ) -> Result<()> {
        let Some(mode) = ssed_sidecar_title_search_mode(&query.mode) else {
            return Ok(());
        };
        let remaining = query.limit.saturating_sub(page.hits.len());
        if remaining == 0 {
            return Ok(());
        }
        let (sidecar_offset, physical_cursor) = match sidecar_cursor {
            SsedSidecarTitleCursor::Offset(offset) => (offset, None),
            SsedSidecarTitleCursor::Physical(cursor) => (0, Some(cursor)),
        };
        let sidecar_page = search_ssed_dense_sidecar_titles_with_resolvers(
            self.ssed_sidecar_body_resolvers()?,
            mode,
            &query.query,
            sidecar_offset,
            physical_cursor.as_ref(),
            remaining.saturating_add(1),
        )?;
        if !sidecar_page.hits.is_empty() || sidecar_page.matched_count > sidecar_offset {
            page.diagnostics.push(Diagnostic::info(
                "ssed_sidecar_title_search",
                "SSED search included renderable dense HONMON sidecar titles",
            ));
        }
        let mut existing_titles = page
            .hits
            .iter()
            .map(|hit| normalize_search_match_text(&hit.title_text))
            .collect::<HashSet<_>>();
        let mut next_sidecar_offset = sidecar_offset;
        let mut next_physical_cursor = None::<SsedSidecarBodyCursor>;
        for hit in sidecar_page.hits {
            next_sidecar_offset = next_sidecar_offset.saturating_add(1);
            next_physical_cursor = hit.cursor.clone();
            let title = if hit.body.title.trim().is_empty() {
                hit.body.text.chars().take(80).collect::<String>()
            } else {
                hit.body.title.clone()
            };
            let label = self.ssed_rich_label_with_policy(&title, &query.label_gaiji_policy());
            if !existing_titles.insert(normalize_search_match_text(&label.text)) {
                continue;
            }
            let resolver_hint = hit
                .body
                .resolver
                .path
                .file_name()
                .map(|name| name.to_string_lossy().to_string());
            let mut hit_diagnostics = label.diagnostics;
            hit_diagnostics.extend(hit.body.diagnostics);
            let target = TargetToken::new(&InternalTarget::SsedDenseAnchor {
                anchor: hit.anchor_id,
                resolver_hint,
            })?;
            let href = target.href();
            page.hits.push(SearchHit {
                href,
                book_id: self.metadata.book_id.clone(),
                target,
                title_html: label.html,
                title_text: label.text,
                snippet_html: None,
                sequence_hint: None,
                diagnostics: hit_diagnostics,
            });
            if page.hits.len() >= query.limit {
                break;
            }
        }
        if !sidecar_page.exhausted {
            page.next_cursor = next_physical_cursor
                .as_ref()
                .map(encode_ssed_sidecar_title_physical_cursor)
                .or_else(|| Some(encode_ssed_sidecar_title_cursor(next_sidecar_offset)));
        }
        Ok(())
    }

    fn ssed_should_append_sidecar_titles_after_native_page(
        &self,
        page: &SearchPage,
        dense_sidecar_titles_preferred: Option<bool>,
    ) -> Result<bool> {
        if page.hits.is_empty() {
            return Ok(true);
        }
        if let Some(preferred) = dense_sidecar_titles_preferred {
            return Ok(preferred);
        }
        let mut diagnostics = Vec::new();
        self.ssed_simple_search_should_prefer_dense_sidecar_titles(&mut diagnostics)
    }

    fn scan_ssed_partial_index_rows(
        &self,
        needle: &str,
        on_row: impl FnMut(SsedIndexRow) -> Result<bool>,
    ) -> Result<Vec<Diagnostic>> {
        let forward_candidates = ssed_index_page_prefilter_candidates(needle);
        if forward_candidates.is_empty() {
            return self.scan_ssed_simple_index_rows(None, on_row);
        }
        let reversed_needle = reverse_search_match_text(needle);
        let reverse_candidates = ssed_index_page_prefilter_candidates(&reversed_needle);
        let skip_backward_rows = self.ssed_has_forward_browse_index();
        self.scan_ssed_simple_index_rows_with_filters(
            None,
            |component| {
                !(skip_backward_rows && ssed_index_component_name_is_backward(&component.filename))
            },
            |component, page| {
                if !ssed_index_page_prefilter_is_safe(component.component_type) {
                    return true;
                }
                if ssed_index_component_name_is_backward(&component.filename) {
                    ssed_body_window_may_contain_query(page, &reverse_candidates)
                } else {
                    ssed_body_window_may_contain_query(page, &forward_candidates)
                }
            },
            on_row,
        )
    }

    fn ssed_fulltext_can_scan_all_title_indexes(&self) -> bool {
        let Some(catalog) = &self.ssed_catalog else {
            return false;
        };
        let skip_backward_rows = self.ssed_has_forward_browse_index();
        let mut block_count = 0u32;
        for component in catalog.components_by_role(SsedComponentRole::Index) {
            if skip_backward_rows && ssed_index_component_name_is_backward(&component.filename) {
                continue;
            }
            if !is_supported_index_type(component.component_type) {
                continue;
            }
            block_count = block_count.saturating_add(component.block_count());
            if block_count > SSED_FULLTEXT_UNBOUNDED_TITLE_PREPASS_MAX_INDEX_BLOCKS {
                return false;
            }
        }
        block_count > 0
    }

    fn search_ssed_fulltext_body_windows(&self, query: &SearchQuery) -> Result<SearchPage> {
        let needle = normalize_search_match_text(&query.query);
        if needle.is_empty() {
            return Ok(SearchPage {
                hits: Vec::new(),
                next_cursor: None,
                result_sequence: None,
                diagnostics: Vec::new(),
            });
        }
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(SearchPage {
                hits: Vec::new(),
                next_cursor: None,
                result_sequence: None,
                diagnostics: vec![Diagnostic::error(
                    "ssed_catalog_missing",
                    "SSED full-text search requires a parsed SSEDINFO catalog",
                )],
            });
        };

        let page_limit = query.limit.saturating_add(1);
        let label_policy = query.label_gaiji_policy();
        let title_cursor = decode_ssed_fulltext_title_cursor(query.cursor.as_deref());
        let sidecar_body_cursor = decode_ssed_fulltext_sidecar_body_cursor(query.cursor.as_deref());
        let sidecar_body_physical_cursor =
            decode_ssed_fulltext_sidecar_body_physical_cursor(query.cursor.as_deref());
        let chronology_cursor = decode_ssed_fulltext_chronology_cursor(query.cursor.as_deref());
        let row_cursor = decode_ssed_fulltext_row_cursor(query.cursor.as_deref());
        let body_physical_cursor =
            decode_ssed_fulltext_body_physical_cursor(query.cursor.as_deref());
        let body_offset = decode_ssed_fulltext_body_cursor(query.cursor.as_deref());
        let mut diagnostics = Vec::new();
        let has_readable_ssed_indexes = has_readable_ssed_index_payload(catalog, &self.storage);
        if query.cursor.is_none()
            && ssed_fulltext_sidecar_title_prepass_is_bounded(&query.query)
            && let Some(page) = self.ssed_fulltext_sidecar_title_prepass(query)?
        {
            return Ok(page);
        }
        let honmon_body_window_scan_needed =
            self.ssed_honmon_body_window_scan_is_needed(catalog, &mut diagnostics)?;
        if honmon_body_window_scan_needed
            && has_readable_ssed_indexes
            && query.cursor.is_none()
            && let Some(page) =
                self.ssed_fulltext_initial_title_index_prepass(query, &needle, page_limit)?
        {
            return Ok(page);
        }
        if honmon_body_window_scan_needed
            && has_readable_ssed_indexes
            && query.cursor.is_none()
            && let Some(page) =
                self.ssed_fulltext_initial_partial_title_index_prepass(query, &needle, page_limit)?
        {
            return Ok(page);
        }
        if honmon_body_window_scan_needed
            && has_readable_ssed_indexes
            && let Some(title_cursor) = title_cursor
            && let Some(page) =
                self.ssed_fulltext_title_index_prepass(query, &needle, title_cursor, page_limit)?
        {
            return Ok(page);
        }
        if let Some(body_cursor) = body_physical_cursor {
            if !honmon_body_window_scan_needed || !has_readable_ssed_indexes {
                return Ok(SearchPage {
                    hits: Vec::new(),
                    next_cursor: None,
                    result_sequence: None,
                    diagnostics,
                });
            }
            return self.ssed_fulltext_body_physical_cursor_page(
                catalog,
                query,
                &needle,
                &body_cursor,
                diagnostics,
            );
        }

        let mut hits = Vec::new();
        let sidecar_limit = query.limit;
        let body_cursor_explicit = query.cursor.as_deref().is_some_and(|cursor| {
            cursor.starts_with("body:") || cursor.starts_with(SSED_FULLTEXT_BODY_CURSOR_PREFIX)
        });
        let row_cursor_explicit = row_cursor.is_some();
        let run_sidecar =
            chronology_cursor.is_none() && !body_cursor_explicit && !row_cursor_explicit;
        let sidecar_offset = sidecar_body_cursor.unwrap_or(0);
        let sidecar_page = if run_sidecar {
            search_ssed_dense_sidecar_bodies_with_resolvers(
                self.ssed_sidecar_body_resolvers()?,
                &query.query,
                sidecar_offset,
                sidecar_body_physical_cursor.as_ref(),
                sidecar_limit,
            )?
        } else {
            SsedSidecarSearchPage {
                hits: Vec::new(),
                matched_count: 0,
                exhausted: true,
            }
        };
        if !sidecar_page.hits.is_empty() || sidecar_page.matched_count > 0 {
            diagnostics.push(Diagnostic::info(
                "ssed_fulltext_sidecar_scan",
                "SSED full-text search included renderable dense HONMON sidecar bodies",
            ));
        }
        let mut next_sidecar_body_cursor = None::<SsedSidecarBodyCursor>;
        for hit in sidecar_page.hits {
            let title = if hit.body.title.trim().is_empty() {
                hit.body.text.chars().take(80).collect::<String>()
            } else {
                hit.body.title.clone()
            };
            next_sidecar_body_cursor = hit.cursor.clone();
            let label = self.ssed_rich_label_with_policy(&title, &label_policy);
            let resolver_hint = hit
                .body
                .resolver
                .path
                .file_name()
                .map(|name| name.to_string_lossy().to_string());
            let mut hit_diagnostics = label.diagnostics;
            hit_diagnostics.extend(hit.body.diagnostics);
            let target = TargetToken::new(&InternalTarget::SsedDenseAnchor {
                anchor: hit.anchor_id,
                resolver_hint,
            })?;
            let href = target.href();
            hits.push(SearchHit {
                href,
                book_id: self.metadata.book_id.clone(),
                target,
                title_html: label.html,
                title_text: label.text,
                snippet_html: ssed_fulltext_snippet_html(&hit.body.text, &query.query),
                sequence_hint: None,
                diagnostics: hit_diagnostics,
            });
            if hits.len() >= page_limit {
                break;
            }
        }
        let next_sidecar_cursor = || {
            next_sidecar_body_cursor
                .as_ref()
                .map(encode_ssed_fulltext_sidecar_body_physical_cursor)
                .unwrap_or_else(|| {
                    encode_ssed_fulltext_sidecar_body_cursor(
                        sidecar_offset.saturating_add(query.limit),
                    )
                })
        };
        if query.cursor.is_none() && hits.len() >= query.limit && !sidecar_page.exhausted {
            hits.truncate(query.limit);
            return Ok(SearchPage {
                hits,
                next_cursor: Some(next_sidecar_cursor()),
                result_sequence: None,
                diagnostics,
            });
        }
        if query.cursor.is_some() && hits.len() >= query.limit && !sidecar_page.exhausted {
            hits.truncate(query.limit);
            return Ok(SearchPage {
                hits,
                next_cursor: Some(next_sidecar_cursor()),
                result_sequence: None,
                diagnostics,
            });
        }
        if hits.len() >= page_limit && !sidecar_page.exhausted {
            hits.truncate(query.limit);
            return Ok(SearchPage {
                hits,
                next_cursor: Some(next_sidecar_cursor()),
                result_sequence: None,
                diagnostics,
            });
        }
        if (query.cursor.is_none() || title_cursor.is_some() || chronology_cursor.is_some())
            && hits.len() < page_limit
        {
            let chronology_offset = chronology_cursor.unwrap_or(0);
            let remaining = page_limit.saturating_sub(hits.len());
            let hits_before_chronology = hits.len();
            let records = search_britannica_chronology_records(
                &self.root,
                &query.query,
                chronology_offset,
                remaining,
            )?;
            let chronology_exhausted = records.len() < remaining;
            if !records.is_empty() {
                diagnostics.push(Diagnostic::info(
                    "ssed_fulltext_britannica_chronology_scan",
                    "SSED full-text search included the Britannica chronology SQLite helper database",
                ));
            }
            let mut chronology_hits = 0usize;
            for record in records {
                let title = record.title();
                let label = self.ssed_rich_label_with_policy(&title, &label_policy);
                let target = TargetToken::new(&InternalTarget::SsedAuxRecord {
                    source: BRITANNICA_CHRONOLOGY_SOURCE_ID.to_owned(),
                    key: record.inc_code.clone(),
                    anchor: None,
                })?;
                let href = target.href();
                hits.push(SearchHit {
                    href,
                    book_id: self.metadata.book_id.clone(),
                    target,
                    title_html: label.html,
                    title_text: label.text,
                    snippet_html: ssed_fulltext_snippet_html(&record.text, &query.query),
                    sequence_hint: None,
                    diagnostics: label.diagnostics,
                });
                chronology_hits = chronology_hits.saturating_add(1);
                if hits.len() >= page_limit {
                    break;
                }
            }
            if hits.len() >= page_limit {
                let returned_chronology_hits = query.limit.saturating_sub(hits_before_chronology);
                hits.truncate(query.limit);
                return Ok(SearchPage {
                    hits,
                    next_cursor: Some(format!(
                        "chronology:{}",
                        chronology_offset.saturating_add(returned_chronology_hits)
                    )),
                    result_sequence: None,
                    diagnostics,
                });
            }
            if hits.len() >= query.limit {
                hits.truncate(query.limit);
                let exhausted_next_cursor =
                    honmon_body_window_scan_needed.then(|| "body:0".to_owned());
                return Ok(SearchPage {
                    hits,
                    next_cursor: if chronology_exhausted {
                        exhausted_next_cursor
                    } else {
                        Some(format!(
                            "chronology:{}",
                            chronology_offset.saturating_add(chronology_hits)
                        ))
                    },
                    result_sequence: None,
                    diagnostics,
                });
            }
        }
        let byte_candidates = ssed_body_search_byte_candidates(&query.query);
        let sidecar_body_phase_cursor =
            sidecar_body_cursor.is_some() || sidecar_body_physical_cursor.is_some();
        let row_driven_search_allowed =
            query.cursor.is_none() || row_cursor.is_some() || sidecar_body_phase_cursor;
        if honmon_body_window_scan_needed
            && has_readable_ssed_indexes
            && row_driven_search_allowed
            && hits.len() < page_limit
        {
            let previous_hits = hits.len();
            let row_offset = row_cursor.unwrap_or(0);
            let remaining_limit = page_limit.saturating_sub(previous_hits);
            let row_page =
                self.ssed_fulltext_row_driven_body_page(SsedRowDrivenFulltextRequest {
                    catalog,
                    raw_query: &query.query,
                    needle: &needle,
                    byte_candidates: &byte_candidates,
                    offset: row_offset,
                    page_limit: remaining_limit,
                    max_checked_rows: Some(ssed_fulltext_row_prefetch_max_rows(
                        query,
                        &byte_candidates,
                    )),
                    gaiji_policy: &label_policy,
                })?;
            let row_page_has_hits = !row_page.hits.is_empty();
            if row_page.exhausted || row_page_has_hits || row_cursor.is_some() {
                diagnostics.extend(row_page.diagnostics);
                hits.extend(row_page.hits);
                let next_cursor = if row_page.exhausted {
                    None
                } else {
                    Some(encode_ssed_fulltext_row_cursor(row_page.next_row_offset))
                };
                hits.truncate(query.limit);
                return Ok(SearchPage {
                    hits,
                    next_cursor,
                    result_sequence: None,
                    diagnostics,
                });
            }
            diagnostics.extend(row_page.diagnostics);
        }
        if !honmon_body_window_scan_needed || !has_readable_ssed_indexes {
            hits.truncate(query.limit);
            return Ok(SearchPage {
                hits,
                next_cursor: None,
                result_sequence: None,
                diagnostics,
            });
        }
        if let Some(page) = self.ssed_fulltext_direct_body_scan_page(SsedDirectFulltextRequest {
            catalog,
            query,
            needle: &needle,
            byte_candidates: &byte_candidates,
            gaiji_policy: &label_policy,
            matched_offset: body_offset,
            base_diagnostics: &diagnostics,
        })? {
            return Ok(page);
        }
        diagnostics.push(Diagnostic::info(
            "ssed_fulltext_body_window_scan",
            format!(
                "SSED full-text search is scanning bounded HONMON windows behind native index targets ({} bytes per target)",
                SSED_FULLTEXT_BODY_WINDOW_BYTES
            ),
        ));
        let mut matched_count = 0usize;
        let mut next_physical_body_cursor = None::<SsedFulltextBodyCursor>;
        let body_hit_ranges_by_component = self.ssed_fulltext_body_hit_ranges(
            catalog,
            &needle,
            &byte_candidates,
            &mut diagnostics,
        )?;
        if body_hit_ranges_by_component.is_empty() {
            return Ok(SearchPage {
                hits,
                next_cursor: None,
                result_sequence: None,
                diagnostics,
            });
        }
        let body_hit_blocks = ssed_fulltext_body_hit_blocks(catalog, &body_hit_ranges_by_component);
        let has_forward_index_component =
            catalog
                .components_by_role(SsedComponentRole::Index)
                .any(|component| {
                    is_supported_index_type(component.component_type)
                        && !ssed_index_component_name_is_backward(&component.filename)
                });

        let mut rows_by_component: BTreeMap<String, Vec<SsedFulltextRow>> = BTreeMap::new();
        let scan_diagnostics = self.scan_ssed_simple_index_rows_with_filters(
            None,
            |component| {
                !has_forward_index_component
                    || !ssed_index_component_name_is_backward(&component.filename)
            },
            |_, page| ssed_index_page_may_point_to_body_blocks(page, &body_hit_blocks),
            |row| {
                if looks_like_raw_anchor_label(&row.key) {
                    return Ok(true);
                }
                let Some(component) = catalog.component_for_address(row.body.block) else {
                    if self.ssed_index_row_body_pointer_is_outside_catalog_range(&row) {
                        return Ok(true);
                    }
                    diagnostics.push(
                        Diagnostic::info(
                            "ssed_fulltext_body_component_missing",
                            format!(
                                "no component contains body pointer block {} offset {}",
                                row.body.block, row.body.offset
                            ),
                        )
                        .with_context("index_component", &row.component),
                    );
                    return Ok(true);
                };
                if component.role != SsedComponentRole::Honmon {
                    return Ok(true);
                }
                let Some(component_offset) =
                    component.relative_offset(row.body.block, row.body.offset)
                else {
                    diagnostics.push(
                        Diagnostic::info(
                            "ssed_fulltext_body_pointer_invalid",
                            format!(
                                "{} does not contain body pointer block {} offset {}",
                                component.filename, row.body.block, row.body.offset
                            ),
                        )
                        .with_context("component", &component.filename),
                    );
                    return Ok(true);
                };
                let Some(ranges) = body_hit_ranges_by_component.get(&component.filename) else {
                    return Ok(true);
                };
                if !ssed_fulltext_offset_in_ranges(ranges, component_offset) {
                    return Ok(true);
                }
                rows_by_component
                    .entry(component.filename.clone())
                    .or_default()
                    .push(SsedFulltextRow {
                        offset: component_offset,
                        body: row.body,
                        title: row.title,
                        key: row.key,
                    });
                Ok(true)
            },
        )?;
        diagnostics.extend(scan_diagnostics);
        for rows in rows_by_component.values_mut() {
            rows.sort_by_key(|row| row.offset);
        }

        'components: for (component_name, rows) in rows_by_component {
            let Some(component) = catalog.component_named(&component_name) else {
                continue;
            };
            let path = match self.resolve_readable_ssed_component_path(component) {
                Ok(Some(path)) => path,
                Ok(None) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_fulltext_body_component_missing",
                            format!("{} is declared but not present on disk", component.filename),
                        )
                        .with_context("component", &component.filename),
                    );
                    continue;
                }
                Err(error) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_fulltext_body_component_decode_failed",
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
            let mut reader = match SsedDataFile::open(&path) {
                Ok(reader) => reader,
                Err(error) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_fulltext_body_component_decode_failed",
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
            let mut verified_offsets = BTreeSet::new();
            for (candidate_index, candidate) in rows.iter().enumerate() {
                if !verified_offsets.insert(candidate.offset) {
                    continue;
                }
                let body_window_len = ssed_fulltext_body_window_len(&rows, candidate_index);
                let body_data = reader.read_range(
                    usize::try_from(candidate.offset).map_err(|_| {
                        Error::Driver("SSED body offset does not fit in usize".to_owned())
                    })?,
                    body_window_len,
                )?;
                if !ssed_body_window_may_contain_query(&body_data, &byte_candidates) {
                    continue;
                }
                let body_text = decode_ssed_body_search_text(&body_data);
                if !normalize_search_match_text(&body_text).contains(&needle) {
                    continue;
                }
                if matched_count < body_offset {
                    matched_count = matched_count.saturating_add(1);
                    continue;
                }
                let target = match self.ssed_target_for_index_pointer(candidate.body)? {
                    Ok(target) => target,
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                        continue;
                    }
                };
                let title =
                    self.ssed_display_text_for_index_title_or_key(candidate.title, &candidate.key);
                if looks_like_raw_anchor_label(&title) {
                    continue;
                }
                let label = self.ssed_rich_label_with_policy(&title, &label_policy);
                let href = target.href();
                hits.push(SearchHit {
                    href,
                    book_id: self.metadata.book_id.clone(),
                    target,
                    title_html: label.html,
                    title_text: label.text,
                    snippet_html: ssed_fulltext_snippet_html(&body_text, &query.query),
                    sequence_hint: None,
                    diagnostics: label.diagnostics,
                });
                if hits.len() <= query.limit {
                    next_physical_body_cursor = Some(SsedFulltextBodyCursor {
                        component: component_name.clone(),
                        offset: candidate.offset,
                    });
                }
                matched_count = matched_count.saturating_add(1);
                if hits.len() >= page_limit {
                    break 'components;
                }
            }
        }
        let next_cursor = (hits.len() > query.limit).then(|| {
            next_physical_body_cursor
                .as_ref()
                .map(encode_ssed_fulltext_body_physical_cursor)
                .unwrap_or_else(|| format!("body:{}", body_offset + query.limit))
        });
        hits.truncate(query.limit);

        Ok(SearchPage {
            hits,
            next_cursor,
            result_sequence: None,
            diagnostics,
        })
    }

    fn ssed_fulltext_body_physical_cursor_page(
        &self,
        catalog: &SsedCatalog,
        query: &SearchQuery,
        needle: &str,
        cursor: &SsedFulltextBodyCursor,
        mut diagnostics: Vec<Diagnostic>,
    ) -> Result<SearchPage> {
        let page_limit = query.limit.saturating_add(1);
        let byte_candidates = ssed_body_search_byte_candidates(&query.query);
        let label_policy = query.label_gaiji_policy();
        let has_forward_index_component =
            catalog
                .components_by_role(SsedComponentRole::Index)
                .any(|component| {
                    is_supported_index_type(component.component_type)
                        && !ssed_index_component_name_is_backward(&component.filename)
                });
        let mut rows_by_component: BTreeMap<String, Vec<SsedFulltextRow>> = BTreeMap::new();
        let mut seen_offsets: BTreeSet<(String, u64)> = BTreeSet::new();
        let scan_diagnostics = self.scan_ssed_simple_index_rows_with_filters(
            None,
            |component| {
                !has_forward_index_component
                    || !ssed_index_component_name_is_backward(&component.filename)
            },
            |_, _| true,
            |row| {
                if looks_like_raw_anchor_label(&row.key) {
                    return Ok(true);
                }
                let Some(component) = catalog.component_for_address(row.body.block) else {
                    return Ok(true);
                };
                if component.role != SsedComponentRole::Honmon {
                    return Ok(true);
                }
                let Some(component_offset) =
                    component.relative_offset(row.body.block, row.body.offset)
                else {
                    return Ok(true);
                };
                if !ssed_fulltext_body_cursor_precedes(
                    cursor,
                    &component.filename,
                    component_offset,
                ) {
                    return Ok(true);
                }
                if !seen_offsets.insert((component.filename.clone(), component_offset)) {
                    return Ok(true);
                }
                rows_by_component
                    .entry(component.filename.clone())
                    .or_default()
                    .push(SsedFulltextRow {
                        offset: component_offset,
                        body: row.body,
                        title: row.title,
                        key: row.key,
                    });
                Ok(true)
            },
        )?;
        diagnostics.extend(scan_diagnostics);
        for rows in rows_by_component.values_mut() {
            rows.sort_by_key(|row| row.offset);
        }

        let mut hits = Vec::new();
        let mut checked_rows = 0usize;
        let mut byte_candidate_rows = 0usize;
        let mut decoded_candidate_rows = 0usize;
        let mut stopped_for_budget = false;
        let mut next_cursor_after_returned = None::<SsedFulltextBodyCursor>;
        let mut next_cursor_after_checked = None::<SsedFulltextBodyCursor>;

        'components: for (component_name, rows) in rows_by_component {
            let Some(component) = catalog.component_named(&component_name) else {
                continue;
            };
            let path = match self.resolve_readable_ssed_component_path(component) {
                Ok(Some(path)) => path,
                Ok(None) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_fulltext_body_component_missing",
                            format!("{} is declared but not present on disk", component.filename),
                        )
                        .with_context("component", &component.filename),
                    );
                    continue;
                }
                Err(error) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_fulltext_body_component_decode_failed",
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
            let mut reader = match SsedDataFile::open(&path) {
                Ok(reader) => reader,
                Err(error) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_fulltext_body_component_decode_failed",
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
            for (candidate_index, candidate) in rows.iter().enumerate() {
                if checked_rows >= SSED_FULLTEXT_BODY_CURSOR_MAX_ROWS {
                    stopped_for_budget = true;
                    break 'components;
                }
                checked_rows = checked_rows.saturating_add(1);
                next_cursor_after_checked = Some(SsedFulltextBodyCursor {
                    component: component_name.clone(),
                    offset: candidate.offset,
                });
                let body_window_len = ssed_fulltext_body_window_len(&rows, candidate_index);
                let body_data = reader.read_range(
                    usize::try_from(candidate.offset).map_err(|_| {
                        Error::Driver("SSED body offset does not fit in usize".to_owned())
                    })?,
                    body_window_len,
                )?;
                if !ssed_body_window_may_contain_query(&body_data, &byte_candidates) {
                    continue;
                }
                byte_candidate_rows = byte_candidate_rows.saturating_add(1);
                let body_text = decode_ssed_body_search_text(&body_data);
                if !normalize_search_match_text(&body_text).contains(needle) {
                    continue;
                }
                decoded_candidate_rows = decoded_candidate_rows.saturating_add(1);
                let target = match self.ssed_target_for_index_pointer(candidate.body)? {
                    Ok(target) => target,
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                        continue;
                    }
                };
                let title =
                    self.ssed_display_text_for_index_title_or_key(candidate.title, &candidate.key);
                if looks_like_raw_anchor_label(&title) {
                    continue;
                }
                let label = self.ssed_rich_label_with_policy(&title, &label_policy);
                let href = target.href();
                hits.push(SearchHit {
                    href,
                    book_id: self.metadata.book_id.clone(),
                    target,
                    title_html: label.html,
                    title_text: label.text,
                    snippet_html: ssed_fulltext_snippet_html(&body_text, &query.query),
                    sequence_hint: None,
                    diagnostics: label.diagnostics,
                });
                if hits.len() <= query.limit {
                    next_cursor_after_returned = Some(SsedFulltextBodyCursor {
                        component: component_name.clone(),
                        offset: candidate.offset,
                    });
                }
                if hits.len() >= page_limit {
                    break 'components;
                }
            }
        }

        diagnostics.push(
            Diagnostic::info(
                "ssed_fulltext_body_cursor_scan",
                "SSED full-text search resumed native HONMON body scanning from a physical cursor",
            )
            .with_context("checked_rows", checked_rows.to_string())
            .with_context("byte_candidate_rows", byte_candidate_rows.to_string())
            .with_context("decoded_candidate_rows", decoded_candidate_rows.to_string()),
        );
        let next_cursor = if hits.len() > query.limit {
            next_cursor_after_returned
                .as_ref()
                .map(encode_ssed_fulltext_body_physical_cursor)
        } else if stopped_for_budget {
            next_cursor_after_checked
                .as_ref()
                .map(encode_ssed_fulltext_body_physical_cursor)
        } else {
            None
        };
        hits.truncate(query.limit);

        Ok(SearchPage {
            hits,
            next_cursor,
            result_sequence: None,
            diagnostics,
        })
    }

    fn ssed_honmon_body_window_scan_is_needed(
        &self,
        catalog: &SsedCatalog,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<bool> {
        self.ssed_honmon_body_window_scan_is_needed_with_diagnostics(
            catalog,
            diagnostics,
            "ssed_fulltext_honmon_scan_skipped_sidecar_backed",
            "SSED full-text search skipped raw HONMON scanning because ordered HONBUN renderer rows are the visual body source",
            "SSED full-text search skipped raw HONMON scanning because sampled native index targets dereference to dense sidecar bodies",
        )
    }

    fn ssed_simple_search_should_prefer_dense_sidecar_titles(
        &self,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<bool> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(false);
        };
        let native_scan_needed = self.ssed_honmon_body_window_scan_is_needed_with_diagnostics(
            catalog,
            diagnostics,
            "ssed_native_index_search_skipped_sidecar_backed",
            "SSED native title/index search skipped raw HONMON targets because ordered HONBUN renderer rows are the visual body source",
            "SSED native title/index search skipped raw HONMON targets because sampled native index targets dereference to dense sidecar bodies",
        )?;
        Ok(!native_scan_needed)
    }

    fn ssed_honmon_body_window_scan_is_needed_with_diagnostics(
        &self,
        catalog: &SsedCatalog,
        diagnostics: &mut Vec<Diagnostic>,
        skipped_code: &'static str,
        ordered_message: &'static str,
        sampled_message: &'static str,
    ) -> Result<bool> {
        let plan = self.ssed_honmon_body_scan_plan(catalog)?;
        diagnostics.extend(plan.diagnostics);
        match plan.decision {
            SsedHonmonBodyScanDecision::ScanNativeHonmon => Ok(true),
            SsedHonmonBodyScanDecision::OrderedHonbunRendererBody => {
                diagnostics.push(Diagnostic::info(skipped_code, ordered_message));
                Ok(false)
            }
            SsedHonmonBodyScanDecision::SampledDenseSidecarBacked { checked_targets } => {
                diagnostics.push(
                    Diagnostic::info(skipped_code, sampled_message)
                        .with_context("checked_targets", checked_targets.to_string()),
                );
                Ok(false)
            }
        }
    }

    fn ssed_honmon_body_scan_plan(&self, catalog: &SsedCatalog) -> Result<SsedHonmonBodyScanPlan> {
        let cached = self.ssed_honmon_body_scan_plan.get_or_init(|| {
            self.compute_ssed_honmon_body_scan_plan(catalog)
                .map_err(|error| error.to_string())
        });
        match cached {
            Ok(plan) => Ok(plan.clone()),
            Err(error) => Err(Error::Driver(error.clone())),
        }
    }

    fn compute_ssed_honmon_body_scan_plan(
        &self,
        catalog: &SsedCatalog,
    ) -> Result<SsedHonmonBodyScanPlan> {
        let resolvers = self.ssed_sidecar_body_resolvers()?;
        if resolvers.is_empty() {
            return Ok(SsedHonmonBodyScanPlan {
                decision: SsedHonmonBodyScanDecision::ScanNativeHonmon,
                diagnostics: Vec::new(),
            });
        }
        if !has_readable_ssed_index_payload(catalog, &self.storage) {
            return Ok(SsedHonmonBodyScanPlan {
                decision: SsedHonmonBodyScanDecision::ScanNativeHonmon,
                diagnostics: Vec::new(),
            });
        }
        if resolvers
            .iter()
            .any(SsedSidecarBodyResolver::is_ordered_honbun_renderer_body)
        {
            return Ok(SsedHonmonBodyScanPlan {
                decision: SsedHonmonBodyScanDecision::OrderedHonbunRendererBody,
                diagnostics: Vec::new(),
            });
        }

        const SIDECAR_BACKED_SAMPLE_TARGETS: usize = 16;
        let mut checked_targets = 0usize;
        let mut sidecar_backed_targets = 0usize;
        let diagnostics = self.scan_ssed_simple_index_rows_with_filters(
            None,
            |component| !ssed_index_component_name_is_backward(&component.filename),
            |_, _| true,
            |row| {
                if checked_targets >= SIDECAR_BACKED_SAMPLE_TARGETS {
                    return Ok(false);
                }
                if looks_like_raw_anchor_label(&row.key) {
                    return Ok(true);
                }
                let Some(component) = catalog.component_for_address(row.body.block) else {
                    return Ok(true);
                };
                if component.role != SsedComponentRole::Honmon {
                    return Ok(true);
                }
                let Some(component_offset) =
                    component.relative_offset(row.body.block, row.body.offset)
                else {
                    return Ok(true);
                };
                checked_targets = checked_targets.saturating_add(1);
                if self
                    .ssed_dense_anchor_at_component_offset(
                        component,
                        usize::try_from(component_offset).unwrap_or(usize::MAX),
                    )?
                    .is_some()
                {
                    sidecar_backed_targets = sidecar_backed_targets.saturating_add(1);
                }
                Ok(true)
            },
        )?;
        if checked_targets > 0 && checked_targets == sidecar_backed_targets {
            return Ok(SsedHonmonBodyScanPlan {
                decision: SsedHonmonBodyScanDecision::SampledDenseSidecarBacked { checked_targets },
                diagnostics,
            });
        }
        Ok(SsedHonmonBodyScanPlan {
            decision: SsedHonmonBodyScanDecision::ScanNativeHonmon,
            diagnostics,
        })
    }

    fn ssed_fulltext_row_driven_body_page(
        &self,
        request: SsedRowDrivenFulltextRequest<'_>,
    ) -> Result<SsedRowDrivenFulltextPage> {
        let SsedRowDrivenFulltextRequest {
            catalog,
            raw_query,
            needle,
            byte_candidates,
            offset,
            page_limit,
            max_checked_rows,
            gaiji_policy,
        } = request;
        if page_limit == 0 {
            return Ok(SsedRowDrivenFulltextPage {
                hits: Vec::new(),
                exhausted: true,
                next_row_offset: offset,
                diagnostics: Vec::new(),
            });
        }
        let mut hits = Vec::new();
        let mut diagnostics = Vec::new();
        let mut readers: BTreeMap<String, SsedDataFile> = BTreeMap::new();
        let mut seen_offsets: BTreeSet<(String, u64)> = BTreeSet::new();
        let mut skipped_rows = 0usize;
        let mut checked_rows = 0usize;
        let mut byte_candidate_rows = 0usize;
        let mut decoded_candidate_rows = 0usize;
        let mut stopped_early = false;
        let mut stopped_for_page_limit = false;
        let scan_diagnostics = self.scan_ssed_simple_index_rows(None, |row| {
            if max_checked_rows.is_some_and(|limit| checked_rows >= limit) {
                stopped_early = true;
                return Ok(false);
            }
            if looks_like_raw_anchor_label(&row.key) {
                return Ok(true);
            }
            let Some(component) = catalog.component_for_address(row.body.block) else {
                if self.ssed_index_row_body_pointer_is_outside_catalog_range(&row) {
                    return Ok(true);
                }
                diagnostics.push(
                    Diagnostic::info(
                        "ssed_fulltext_body_component_missing",
                        format!(
                            "no component contains body pointer block {} offset {}",
                            row.body.block, row.body.offset
                        ),
                    )
                    .with_context("index_component", &row.component),
                );
                return Ok(true);
            };
            if component.role != SsedComponentRole::Honmon {
                return Ok(true);
            }
            let Some(component_offset) = component.relative_offset(row.body.block, row.body.offset)
            else {
                diagnostics.push(
                    Diagnostic::info(
                        "ssed_fulltext_body_pointer_invalid",
                        format!(
                            "{} does not contain body pointer block {} offset {}",
                            component.filename, row.body.block, row.body.offset
                        ),
                    )
                    .with_context("component", &component.filename),
                );
                return Ok(true);
            };
            if !seen_offsets.insert((component.filename.clone(), component_offset)) {
                return Ok(true);
            }
            if skipped_rows < offset {
                skipped_rows = skipped_rows.saturating_add(1);
                return Ok(true);
            }
            checked_rows = checked_rows.saturating_add(1);
            let reader = match readers.get_mut(&component.filename) {
                Some(reader) => reader,
                None => {
                    let path = match self.resolve_readable_ssed_component_path(component) {
                        Ok(Some(path)) => path,
                        Ok(None) => {
                            diagnostics.push(
                                Diagnostic::warning(
                                    "ssed_fulltext_body_component_missing",
                                    format!(
                                        "{} is declared but not present on disk",
                                        component.filename
                                    ),
                                )
                                .with_context("component", &component.filename),
                            );
                            return Ok(true);
                        }
                        Err(error) => {
                            diagnostics.push(
                                Diagnostic::warning(
                                    "ssed_fulltext_body_component_decode_failed",
                                    format!(
                                        "{} is not readable as SSEDDATA: {error}",
                                        component.filename
                                    ),
                                )
                                .with_context("component", &component.filename),
                            );
                            return Ok(true);
                        }
                    };
                    let reader = match SsedDataFile::open(&path) {
                        Ok(reader) => reader,
                        Err(error) => {
                            diagnostics.push(
                                Diagnostic::warning(
                                    "ssed_fulltext_body_component_decode_failed",
                                    format!(
                                        "{} is not readable as SSEDDATA: {error}",
                                        component.filename
                                    ),
                                )
                                .with_context("component", &component.filename),
                            );
                            return Ok(true);
                        }
                    };
                    readers.insert(component.filename.clone(), reader);
                    match readers.get_mut(&component.filename) {
                        Some(reader) => reader,
                        None => {
                            return Err(Error::Driver(format!(
                                "{} reader cache insert did not persist",
                                component.filename
                            )));
                        }
                    }
                }
            };
            let body_data = reader.read_range(
                usize::try_from(component_offset).map_err(|_| {
                    Error::Driver("SSED body offset does not fit in usize".to_owned())
                })?,
                SSED_FULLTEXT_BODY_WINDOW_BYTES,
            )?;
            if !ssed_body_window_may_contain_query(&body_data, byte_candidates) {
                return Ok(true);
            }
            byte_candidate_rows = byte_candidate_rows.saturating_add(1);
            let body_text = decode_ssed_body_search_text(&body_data);
            if !normalize_search_match_text(&body_text).contains(needle) {
                return Ok(true);
            }
            decoded_candidate_rows = decoded_candidate_rows.saturating_add(1);
            let target = match self.ssed_target_for_index_pointer(row.body)? {
                Ok(target) => target,
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    return Ok(true);
                }
            };
            let title = self.ssed_display_text_for_index_title_or_key(row.title, &row.key);
            if looks_like_raw_anchor_label(&title) {
                return Ok(true);
            }
            let label = self.ssed_rich_label_with_policy(&title, gaiji_policy);
            let href = target.href();
            hits.push(SearchHit {
                href,
                book_id: self.metadata.book_id.clone(),
                target,
                title_html: label.html,
                title_text: label.text,
                snippet_html: ssed_fulltext_snippet_html(&body_text, raw_query),
                sequence_hint: None,
                diagnostics: label.diagnostics,
            });
            if hits.len() >= page_limit {
                stopped_early = true;
                stopped_for_page_limit = true;
                return Ok(false);
            }
            Ok(true)
        })?;
        diagnostics.extend(scan_diagnostics);
        diagnostics.push(
            Diagnostic::info(
                "ssed_fulltext_row_driven_body_prefetch",
                "SSED full-text search checked a bounded page of native index targets",
            )
            .with_context("checked_rows", checked_rows.to_string())
            .with_context("byte_candidate_rows", byte_candidate_rows.to_string())
            .with_context("decoded_candidate_rows", decoded_candidate_rows.to_string()),
        );
        Ok(SsedRowDrivenFulltextPage {
            hits,
            exhausted: !stopped_early,
            next_row_offset: if stopped_for_page_limit {
                offset.saturating_add(checked_rows.saturating_sub(1))
            } else {
                offset.saturating_add(checked_rows)
            },
            diagnostics,
        })
    }

    fn ssed_fulltext_direct_body_scan_page(
        &self,
        request: SsedDirectFulltextRequest<'_>,
    ) -> Result<Option<SearchPage>> {
        let SsedDirectFulltextRequest {
            catalog,
            query,
            needle,
            byte_candidates,
            gaiji_policy,
            matched_offset,
            base_diagnostics,
        } = request;
        if byte_candidates.is_empty() {
            return Ok(None);
        }

        let page_limit = query.limit.saturating_add(1);
        let mut hits = Vec::new();
        let mut diagnostics = base_diagnostics.to_vec();
        let mut seen_entries = BTreeSet::new();
        let mut scanned_windows = 0usize;
        let mut byte_candidate_windows = 0usize;
        let mut decoded_candidate_windows = 0usize;
        let mut matched_count = 0usize;
        let mut next_cursor_after_returned = None::<SsedFulltextBodyCursor>;
        let mut needs_index_fallback = false;

        'components: for component in catalog.components_by_role(SsedComponentRole::Honmon) {
            if !component.has_positive_range() {
                continue;
            }
            let path = match self.resolve_readable_ssed_component_path(component) {
                Ok(Some(path)) => path,
                Ok(None) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_fulltext_body_component_missing",
                            format!("{} is declared but not present on disk", component.filename),
                        )
                        .with_context("component", &component.filename),
                    );
                    continue;
                }
                Err(error) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_fulltext_body_component_decode_failed",
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
            let mut reader = match SsedDataFile::open(&path) {
                Ok(reader) => reader,
                Err(error) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_fulltext_body_component_decode_failed",
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
            let expanded_size = reader.header().expanded_size();
            let mut scan_offset = 0usize;
            while scan_offset < expanded_size {
                let read_start = scan_offset.saturating_sub(SSED_FULLTEXT_BODY_WINDOW_BYTES);
                let read_end = expanded_size.min(
                    scan_offset
                        .saturating_add(SSED_FULLTEXT_SCAN_WINDOW_BYTES)
                        .saturating_add(SSED_FULLTEXT_SCAN_OVERLAP_BYTES),
                );
                let read_size = read_end.saturating_sub(read_start);
                let data = reader.read_range(read_start, read_size)?;
                if data.is_empty() {
                    break;
                }
                scanned_windows = scanned_windows.saturating_add(1);
                let search_start = scan_offset.saturating_sub(read_start);
                let Some(candidate_offset) =
                    ssed_fulltext_first_byte_candidate_offset(&data, byte_candidates, search_start)
                else {
                    if scan_offset + SSED_FULLTEXT_SCAN_WINDOW_BYTES >= expanded_size {
                        break;
                    }
                    scan_offset += SSED_FULLTEXT_SCAN_WINDOW_BYTES;
                    continue;
                };
                byte_candidate_windows = byte_candidate_windows.saturating_add(1);

                let Some(entry_start) =
                    ssed_fulltext_entry_marker_before_offset(&data, candidate_offset)
                else {
                    needs_index_fallback = true;
                    break 'components;
                };
                let absolute_entry_offset = read_start.saturating_add(entry_start);
                if !seen_entries.insert((component.filename.clone(), absolute_entry_offset)) {
                    if scan_offset + SSED_FULLTEXT_SCAN_WINDOW_BYTES >= expanded_size {
                        break;
                    }
                    scan_offset += SSED_FULLTEXT_SCAN_WINDOW_BYTES;
                    continue;
                }

                let entry_end = ssed_fulltext_next_entry_marker_offset(
                    &data,
                    entry_start.saturating_add(SSED_ENTRY_MARKER.len()),
                )
                .unwrap_or(data.len());
                let body_data = &data[entry_start..entry_end];
                let body_text = decode_ssed_body_search_text(body_data);
                if !normalize_search_match_text(&body_text).contains(needle) {
                    if scan_offset + SSED_FULLTEXT_SCAN_WINDOW_BYTES >= expanded_size {
                        break;
                    }
                    scan_offset += SSED_FULLTEXT_SCAN_WINDOW_BYTES;
                    continue;
                }
                decoded_candidate_windows = decoded_candidate_windows.saturating_add(1);
                if matched_count < matched_offset {
                    matched_count = matched_count.saturating_add(1);
                    if scan_offset + SSED_FULLTEXT_SCAN_WINDOW_BYTES >= expanded_size {
                        break;
                    }
                    scan_offset += SSED_FULLTEXT_SCAN_WINDOW_BYTES;
                    continue;
                }

                let Some(pointer) = ssed_component_pointer_for_relative_offset(
                    component,
                    u64::try_from(absolute_entry_offset).unwrap_or(u64::MAX),
                ) else {
                    needs_index_fallback = true;
                    break 'components;
                };
                let target = TargetToken::new(&InternalTarget::SsedAddress {
                    component: component.filename.clone(),
                    block: pointer.block,
                    offset: pointer.offset,
                })?;
                let title = ssed_fulltext_direct_body_title(&body_text);
                let label = self.ssed_rich_label_with_policy(&title, gaiji_policy);
                let href = target.href();
                hits.push(SearchHit {
                    href,
                    book_id: self.metadata.book_id.clone(),
                    target,
                    title_html: label.html,
                    title_text: label.text,
                    snippet_html: ssed_fulltext_snippet_html(&body_text, &query.query),
                    sequence_hint: None,
                    diagnostics: label.diagnostics,
                });
                if hits.len() <= query.limit {
                    next_cursor_after_returned = Some(SsedFulltextBodyCursor {
                        component: component.filename.clone(),
                        offset: u64::try_from(absolute_entry_offset).unwrap_or(u64::MAX),
                    });
                }
                matched_count = matched_count.saturating_add(1);
                if hits.len() >= page_limit {
                    break 'components;
                }

                if scan_offset + SSED_FULLTEXT_SCAN_WINDOW_BYTES >= expanded_size {
                    break;
                }
                scan_offset += SSED_FULLTEXT_SCAN_WINDOW_BYTES;
            }
        }

        if needs_index_fallback {
            return Ok(None);
        }

        diagnostics.push(
            Diagnostic::info(
                "ssed_fulltext_body_direct_scan",
                "SSED full-text search scanned HONMON byte windows for direct body-address hits",
            )
            .with_context("scanned_windows", scanned_windows.to_string())
            .with_context("byte_candidate_windows", byte_candidate_windows.to_string())
            .with_context(
                "decoded_candidate_windows",
                decoded_candidate_windows.to_string(),
            ),
        );
        let next_cursor = (hits.len() > query.limit).then(|| {
            next_cursor_after_returned
                .as_ref()
                .map(encode_ssed_fulltext_body_physical_cursor)
                .unwrap_or_else(|| format!("body:{}", matched_offset.saturating_add(query.limit)))
        });
        hits.truncate(query.limit);
        Ok(Some(SearchPage {
            hits,
            next_cursor,
            result_sequence: None,
            diagnostics,
        }))
    }

    fn ssed_fulltext_body_hit_ranges(
        &self,
        catalog: &SsedCatalog,
        needle: &str,
        byte_candidates: &[Vec<u8>],
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<BTreeMap<String, Vec<(u64, u64)>>> {
        let mut ranges_by_component = BTreeMap::new();
        let mut scanned_windows = 0usize;
        let mut byte_candidate_windows = 0usize;
        let mut matched_windows = 0usize;
        for component in catalog.components_by_role(SsedComponentRole::Honmon) {
            if !component.has_positive_range() {
                continue;
            }
            let path = match self.resolve_readable_ssed_component_path(component) {
                Ok(Some(path)) => path,
                Ok(None) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_fulltext_body_component_missing",
                            format!("{} is declared but not present on disk", component.filename),
                        )
                        .with_context("component", &component.filename),
                    );
                    continue;
                }
                Err(error) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_fulltext_body_component_decode_failed",
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
            let mut reader = match SsedDataFile::open(&path) {
                Ok(reader) => reader,
                Err(error) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_fulltext_body_component_decode_failed",
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
            let expanded_size = reader.header().expanded_size();
            let mut scan_offset = 0usize;
            let mut ranges = Vec::new();
            while scan_offset < expanded_size {
                let read_size = expanded_size
                    .saturating_sub(scan_offset)
                    .min(SSED_FULLTEXT_SCAN_WINDOW_BYTES + SSED_FULLTEXT_SCAN_OVERLAP_BYTES);
                let data = reader.read_range(scan_offset, read_size)?;
                if data.is_empty() {
                    break;
                }
                scanned_windows = scanned_windows.saturating_add(1);
                if !ssed_body_window_may_contain_query(&data, byte_candidates) {
                    if scan_offset + SSED_FULLTEXT_SCAN_WINDOW_BYTES >= expanded_size {
                        break;
                    }
                    scan_offset += SSED_FULLTEXT_SCAN_WINDOW_BYTES;
                    continue;
                }
                byte_candidate_windows = byte_candidate_windows.saturating_add(1);
                let window_text = decode_ssed_body_search_text(&data);
                if normalize_search_match_text(&window_text).contains(needle) {
                    matched_windows = matched_windows.saturating_add(1);
                    let lower = scan_offset.saturating_sub(SSED_FULLTEXT_BODY_WINDOW_BYTES) as u64;
                    let upper = scan_offset.saturating_add(read_size) as u64;
                    push_ssed_fulltext_range(&mut ranges, lower, upper);
                }
                if scan_offset + SSED_FULLTEXT_SCAN_WINDOW_BYTES >= expanded_size {
                    break;
                }
                scan_offset += SSED_FULLTEXT_SCAN_WINDOW_BYTES;
            }
            if !ranges.is_empty() {
                ranges_by_component.insert(component.filename.clone(), ranges);
            }
        }
        if !byte_candidates.is_empty() {
            diagnostics.push(
                Diagnostic::info(
                    "ssed_fulltext_byte_prefilter",
                    "SSED full-text search used raw body byte candidates before decoding HONMON windows",
                )
                .with_context("scanned_windows", scanned_windows.to_string())
                .with_context("byte_candidate_windows", byte_candidate_windows.to_string())
                .with_context("matched_windows", matched_windows.to_string()),
            );
        }
        Ok(ranges_by_component)
    }

    fn ssed_fulltext_initial_title_index_prepass(
        &self,
        query: &SearchQuery,
        needle: &str,
        page_limit: usize,
    ) -> Result<Option<SearchPage>> {
        let mut collector = SsedIndexSearchCollector::new(
            self,
            &SearchMode::Forward,
            needle,
            0,
            page_limit,
            query.label_gaiji_policy(),
        )
        .with_display_label_matching();
        let candidate_has_hits = Cell::new(false);
        let scan_result = self.scan_ssed_simple_leaf_index_rows_near_key(
            &SearchMode::Forward,
            needle,
            |row| {
                let keep_scanning = collector.push_row(row)?;
                if collector.has_hits() {
                    candidate_has_hits.set(true);
                }
                Ok(keep_scanning)
            },
            || candidate_has_hits.get(),
        )?;
        collector.extend_diagnostics(scan_result.diagnostics);
        if !collector.has_hits() {
            return Ok(None);
        }
        let mut page = collector.into_search_page(query.limit);
        page.next_cursor = Some(self.ssed_fulltext_post_title_prepass_cursor(query)?);
        page.diagnostics.insert(
            0,
            Diagnostic::info(
                "ssed_fulltext_title_index_prepass",
                "SSED full-text search returned native title/index matches before scanning HONMON bodies",
            ),
        );
        Ok(Some(page))
    }

    fn ssed_fulltext_initial_partial_title_index_prepass(
        &self,
        query: &SearchQuery,
        needle: &str,
        page_limit: usize,
    ) -> Result<Option<SearchPage>> {
        let mut collector = SsedIndexSearchCollector::new(
            self,
            &SearchMode::Partial,
            needle,
            0,
            page_limit,
            query.label_gaiji_policy(),
        )
        .with_display_label_matching();
        let physical_next_cursor = if self.ssed_fulltext_can_scan_all_title_indexes() {
            let scan_diagnostics =
                self.scan_ssed_partial_index_rows(needle, |row| collector.push_row(row))?;
            collector.extend_diagnostics(scan_diagnostics);
            None
        } else {
            self.scan_ssed_partial_index_rows_paged_until_visible(needle, None, &mut collector)?
        };
        if !collector.has_hits() {
            return Ok(None);
        }
        let mut page = collector.into_search_page(query.limit);
        let post_title_prepass_cursor = self.ssed_fulltext_post_title_prepass_cursor(query)?;
        page.next_cursor = page
            .next_cursor
            .as_deref()
            .map(|cursor| format!("title:{cursor}"))
            .or_else(|| physical_next_cursor.map(|cursor| format!("title:{cursor}")))
            .or(Some(post_title_prepass_cursor));
        page.diagnostics.insert(
            0,
            Diagnostic::info(
                "ssed_fulltext_title_index_prepass",
                "SSED full-text search returned native title/index matches before scanning HONMON bodies",
            ),
        );
        Ok(Some(page))
    }

    fn ssed_fulltext_sidecar_title_prepass(
        &self,
        query: &SearchQuery,
    ) -> Result<Option<SearchPage>> {
        let mut title_query = query.clone();
        title_query.mode = SearchMode::Forward;
        let mut page = SearchPage {
            hits: Vec::new(),
            next_cursor: None,
            result_sequence: None,
            diagnostics: Vec::new(),
        };
        self.append_ssed_sidecar_title_hits(
            &title_query,
            &mut page,
            SsedSidecarTitleCursor::Offset(0),
        )?;
        if page.hits.is_empty() {
            return Ok(None);
        }
        page.next_cursor = Some(self.ssed_fulltext_post_sidecar_title_prepass_cursor(query)?);
        page.diagnostics.insert(
            0,
            Diagnostic::info(
                "ssed_fulltext_sidecar_title_prepass",
                "SSED full-text search returned dense sidecar title matches before scanning sidecar bodies",
            ),
        );
        Ok(Some(page))
    }

    fn ssed_fulltext_title_index_prepass(
        &self,
        query: &SearchQuery,
        needle: &str,
        title_cursor: SsedFulltextTitleCursor,
        page_limit: usize,
    ) -> Result<Option<SearchPage>> {
        let mut collector = SsedIndexSearchCollector::new(
            self,
            &SearchMode::Partial,
            needle,
            match title_cursor {
                SsedFulltextTitleCursor::MatchedOffset(offset) => offset,
                SsedFulltextTitleCursor::Physical(_) => 0,
            },
            page_limit,
            query.label_gaiji_policy(),
        )
        .with_display_label_matching();
        let physical_next_cursor = match title_cursor {
            SsedFulltextTitleCursor::MatchedOffset(_) => {
                let scan_diagnostics =
                    self.scan_ssed_partial_index_rows(needle, |row| collector.push_row(row))?;
                collector.extend_diagnostics(scan_diagnostics);
                None
            }
            SsedFulltextTitleCursor::Physical(cursor) => self
                .scan_ssed_partial_index_rows_paged_until_visible_with_prefilter_budget(
                    needle,
                    Some(cursor),
                    &mut collector,
                    SSED_FULLTEXT_TITLE_CURSOR_PREFILTERED_LEAF_PAGE_BUDGET,
                )?,
        };
        let mut page = collector.into_search_page(query.limit);
        if page.hits.is_empty() {
            return Ok(None);
        }
        let post_title_prepass_cursor = self.ssed_fulltext_post_title_prepass_cursor(query)?;
        page.next_cursor = page
            .next_cursor
            .as_deref()
            .map(|cursor| format!("title:{cursor}"))
            .or_else(|| physical_next_cursor.map(|cursor| format!("title:{cursor}")))
            .or(Some(post_title_prepass_cursor));
        page.diagnostics.insert(
            0,
            Diagnostic::info(
                "ssed_fulltext_title_index_prepass",
                "SSED full-text search satisfied the first page from native title/index labels before scanning HONMON bodies",
            ),
        );
        Ok(Some(page))
    }

    fn ssed_fulltext_post_title_prepass_cursor(&self, query: &SearchQuery) -> Result<String> {
        let sidecar_resolvers = self.ssed_sidecar_body_resolvers()?;
        if sidecar_resolvers.is_empty() {
            return Ok(encode_ssed_fulltext_body_cursor(0));
        }
        if sidecar_sql_prefilter_is_authoritative(&query.query) {
            let sidecar_page = search_ssed_dense_sidecar_bodies_with_resolvers(
                sidecar_resolvers,
                &query.query,
                0,
                None,
                1,
            )?;
            if sidecar_page.hits.is_empty() && sidecar_page.exhausted {
                return Ok(encode_ssed_fulltext_body_cursor(0));
            }
            return Ok(encode_ssed_fulltext_sidecar_body_start_cursor());
        }
        if self.ssed_fulltext_prefiltered_sidecar_body_has_initial_hit(query, sidecar_resolvers)? {
            return Ok(encode_ssed_fulltext_sidecar_body_start_cursor());
        }
        Ok(encode_ssed_fulltext_body_cursor(0))
    }

    fn ssed_fulltext_post_sidecar_title_prepass_cursor(
        &self,
        query: &SearchQuery,
    ) -> Result<String> {
        let sidecar_resolvers = self.ssed_sidecar_body_resolvers()?;
        if self.ssed_fulltext_prefiltered_sidecar_body_has_initial_hit(query, sidecar_resolvers)? {
            return Ok(encode_ssed_fulltext_sidecar_body_start_cursor());
        }
        Ok(encode_ssed_fulltext_sidecar_body_cursor(0))
    }

    fn ssed_fulltext_prefiltered_sidecar_body_has_initial_hit(
        &self,
        query: &SearchQuery,
        sidecar_resolvers: &[SsedSidecarBodyResolver],
    ) -> Result<bool> {
        let sidecar_page = search_ssed_dense_sidecar_bodies_prefiltered_with_resolvers(
            sidecar_resolvers,
            &query.query,
            0,
            None,
            1,
        )?;
        Ok(!sidecar_page.hits.is_empty())
    }
}

const SSED_FULLTEXT_ROW_PREFETCH_MAX_ROWS: usize = 512;
const SSED_FULLTEXT_INITIAL_ROW_PREFETCH_MAX_ROWS: usize = 64;

fn ssed_fulltext_row_prefetch_max_rows(query: &SearchQuery, byte_candidates: &[Vec<u8>]) -> usize {
    if query.cursor.is_none() && !byte_candidates.is_empty() {
        SSED_FULLTEXT_INITIAL_ROW_PREFETCH_MAX_ROWS
    } else {
        SSED_FULLTEXT_ROW_PREFETCH_MAX_ROWS
    }
}

struct SsedRowDrivenFulltextRequest<'a> {
    catalog: &'a SsedCatalog,
    raw_query: &'a str,
    needle: &'a str,
    byte_candidates: &'a [Vec<u8>],
    offset: usize,
    page_limit: usize,
    max_checked_rows: Option<usize>,
    gaiji_policy: &'a GaijiPolicy,
}

struct SsedRowDrivenFulltextPage {
    hits: Vec<SearchHit>,
    exhausted: bool,
    next_row_offset: usize,
    diagnostics: Vec<Diagnostic>,
}

struct SsedDirectFulltextRequest<'a> {
    catalog: &'a SsedCatalog,
    query: &'a SearchQuery,
    needle: &'a str,
    byte_candidates: &'a [Vec<u8>],
    gaiji_policy: &'a GaijiPolicy,
    matched_offset: usize,
    base_diagnostics: &'a [Diagnostic],
}

fn push_ssed_fulltext_range(ranges: &mut Vec<(u64, u64)>, lower: u64, upper: u64) {
    if let Some(last) = ranges.last_mut()
        && lower <= last.1
    {
        last.1 = last.1.max(upper);
        return;
    }
    ranges.push((lower, upper));
}

fn ssed_fulltext_offset_in_ranges(ranges: &[(u64, u64)], offset: u64) -> bool {
    ranges
        .binary_search_by(|(lower, upper)| {
            if offset < *lower {
                std::cmp::Ordering::Greater
            } else if offset > *upper {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

fn ssed_fulltext_first_byte_candidate_offset(
    data: &[u8],
    byte_candidates: &[Vec<u8>],
    start: usize,
) -> Option<usize> {
    let start = start.min(data.len());
    byte_candidates
        .iter()
        .filter(|candidate| !candidate.is_empty())
        .filter_map(|candidate| {
            memchr::memmem::find(&data[start..], candidate).map(|relative| start + relative)
        })
        .min()
}

fn ssed_fulltext_entry_marker_before_offset(data: &[u8], offset: usize) -> Option<usize> {
    let end = offset.min(data.len());
    data[..end]
        .windows(SSED_ENTRY_MARKER.len())
        .rposition(|window| window == SSED_ENTRY_MARKER)
        .map(|marker| {
            if marker >= 2 && data[marker - 2..marker] == [0x1f, 0x02] {
                marker - 2
            } else {
                marker
            }
        })
}

fn ssed_fulltext_next_entry_marker_offset(data: &[u8], start: usize) -> Option<usize> {
    let start = start.min(data.len());
    data[start..]
        .windows(SSED_ENTRY_MARKER.len())
        .position(|window| window == SSED_ENTRY_MARKER)
        .map(|relative| start + relative)
}

fn ssed_component_pointer_for_relative_offset(
    component: &SsedComponent,
    relative_offset: u64,
) -> Option<SsedIndexPointer> {
    let block_delta = u32::try_from(relative_offset / u64::from(BLOCK_SIZE)).ok()?;
    let block = component.start_block.checked_add(block_delta)?;
    let offset = u32::try_from(relative_offset % u64::from(BLOCK_SIZE)).ok()?;
    component
        .relative_offset(block, offset)
        .is_some()
        .then_some(SsedIndexPointer { block, offset })
}

fn ssed_fulltext_direct_body_title(body_text: &str) -> String {
    let title = body_text
        .split_whitespace()
        .take(12)
        .collect::<Vec<_>>()
        .join(" ");
    if title.is_empty() {
        "HONMON body match".to_owned()
    } else {
        title.chars().take(80).collect()
    }
}

fn ssed_fulltext_body_hit_blocks(
    catalog: &SsedCatalog,
    ranges_by_component: &BTreeMap<String, Vec<(u64, u64)>>,
) -> HashSet<u32> {
    let mut blocks = HashSet::new();
    for (component_name, ranges) in ranges_by_component {
        let Some(component) = catalog.component_named(component_name) else {
            continue;
        };
        for (lower, upper) in ranges {
            let start_block = u64::from(component.start_block) + lower / u64::from(BLOCK_SIZE);
            let end_block = u64::from(component.start_block) + upper / u64::from(BLOCK_SIZE);
            for block in start_block..=end_block {
                if let Ok(block) = u32::try_from(block) {
                    blocks.insert(block);
                }
            }
        }
    }
    blocks
}

fn ssed_index_page_may_point_to_body_blocks(page: &[u8], body_blocks: &HashSet<u32>) -> bool {
    if body_blocks.is_empty() {
        return true;
    }
    page.windows(4).any(|window| {
        body_blocks.contains(&u32::from_be_bytes([
            window[0], window[1], window[2], window[3],
        ]))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SsedFulltextTitleCursor {
    MatchedOffset(usize),
    Physical(SsedPartialIndexScanCursor),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SsedFulltextBodyCursor {
    component: String,
    offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SsedSidecarTitleCursor {
    Offset(usize),
    Physical(SsedSidecarBodyCursor),
}

#[derive(Debug)]
struct SsedPartialNonprefixSearchPage {
    page: SearchPage,
    visible_start_cursor: Option<SsedPartialIndexScanCursor>,
}

#[derive(Debug, Default)]
struct SsedPartialNonprefixPhysicalScan {
    next_cursor: Option<String>,
    visible_start_cursor: Option<SsedPartialIndexScanCursor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SsedPartialNonprefixCursor {
    MatchedOffset {
        offset: usize,
        skip_prefix_rows: bool,
    },
    MatchedPhysicalOffset {
        cursor: SsedPartialIndexScanCursor,
        offset: usize,
        skip_prefix_rows: bool,
    },
    Physical {
        cursor: SsedPartialIndexScanCursor,
        skip_prefix_rows: bool,
    },
    UnverifiedPhysical {
        cursor: SsedPartialIndexScanCursor,
        skip_prefix_rows: bool,
    },
}

impl SsedPartialNonprefixCursor {
    fn skip_prefix_rows(&self) -> bool {
        match self {
            Self::MatchedOffset {
                skip_prefix_rows, ..
            }
            | Self::MatchedPhysicalOffset {
                skip_prefix_rows, ..
            }
            | Self::Physical {
                skip_prefix_rows, ..
            }
            | Self::UnverifiedPhysical {
                skip_prefix_rows, ..
            } => *skip_prefix_rows,
        }
    }
}

const SSED_PARTIAL_PREFIX_CURSOR_PREFIX: &str = "ssed-partial-prefix:";
const SSED_PARTIAL_NONPREFIX_UNVERIFIED_INDEX_CURSOR_PREFIX: &str =
    "ssed-partial-nonprefix-unverified-index:";
const SSED_PARTIAL_NONPREFIX_INDEX_CURSOR_PREFIX: &str = "ssed-partial-nonprefix-index:";
const SSED_PARTIAL_NONPREFIX_OFFSET_CURSOR_PREFIX: &str = "ssed-partial-nonprefix-offset:";
const SSED_PARTIAL_NONPREFIX_PHYSICAL_OFFSET_CURSOR_PREFIX: &str =
    "ssed-partial-nonprefix-physical-offset:";
const SSED_PARTIAL_NONPREFIX_NOSKIP_INDEX_CURSOR_PREFIX: &str =
    "ssed-partial-nonprefix-noskip-index:";
const SSED_PARTIAL_NONPREFIX_NOSKIP_OFFSET_CURSOR_PREFIX: &str =
    "ssed-partial-nonprefix-noskip-offset:";
const SSED_PARTIAL_NONPREFIX_NOSKIP_PHYSICAL_OFFSET_CURSOR_PREFIX: &str =
    "ssed-partial-nonprefix-noskip-physical-offset:";

fn decode_ssed_partial_prefix_cursor(cursor: Option<&str>) -> Option<String> {
    cursor?
        .strip_prefix(SSED_PARTIAL_PREFIX_CURSOR_PREFIX)
        .map(ToOwned::to_owned)
}

fn encode_ssed_partial_prefix_cursor(cursor: String) -> String {
    format!("{SSED_PARTIAL_PREFIX_CURSOR_PREFIX}{cursor}")
}

fn decode_ssed_partial_nonprefix_cursor(
    cursor: Option<&str>,
) -> Option<SsedPartialNonprefixCursor> {
    let cursor = cursor?;
    if let Some(value) = cursor.strip_prefix(SSED_PARTIAL_NONPREFIX_OFFSET_CURSOR_PREFIX) {
        return Some(SsedPartialNonprefixCursor::MatchedOffset {
            offset: value.parse().ok()?,
            skip_prefix_rows: true,
        });
    }
    if let Some(value) = cursor.strip_prefix(SSED_PARTIAL_NONPREFIX_NOSKIP_OFFSET_CURSOR_PREFIX) {
        return Some(SsedPartialNonprefixCursor::MatchedOffset {
            offset: value.parse().ok()?,
            skip_prefix_rows: false,
        });
    }
    if let Some(value) = cursor.strip_prefix(SSED_PARTIAL_NONPREFIX_PHYSICAL_OFFSET_CURSOR_PREFIX) {
        return decode_ssed_partial_nonprefix_physical_offset_cursor(value, true);
    }
    if let Some(value) =
        cursor.strip_prefix(SSED_PARTIAL_NONPREFIX_NOSKIP_PHYSICAL_OFFSET_CURSOR_PREFIX)
    {
        return decode_ssed_partial_nonprefix_physical_offset_cursor(value, false);
    }
    if let Some(value) = cursor.strip_prefix(SSED_PARTIAL_NONPREFIX_UNVERIFIED_INDEX_CURSOR_PREFIX)
    {
        let (component_index, page_index) = value.split_once(':')?;
        return Some(SsedPartialNonprefixCursor::UnverifiedPhysical {
            cursor: SsedPartialIndexScanCursor {
                component_index: component_index.parse().ok()?,
                page_index: page_index.parse().ok()?,
            },
            skip_prefix_rows: true,
        });
    }
    if let Some(value) = cursor.strip_prefix(SSED_PARTIAL_NONPREFIX_INDEX_CURSOR_PREFIX) {
        let (component_index, page_index) = value.split_once(':')?;
        return Some(SsedPartialNonprefixCursor::Physical {
            cursor: SsedPartialIndexScanCursor {
                component_index: component_index.parse().ok()?,
                page_index: page_index.parse().ok()?,
            },
            skip_prefix_rows: true,
        });
    }
    let value = cursor.strip_prefix(SSED_PARTIAL_NONPREFIX_NOSKIP_INDEX_CURSOR_PREFIX)?;
    let (component_index, page_index) = value.split_once(':')?;
    Some(SsedPartialNonprefixCursor::Physical {
        cursor: SsedPartialIndexScanCursor {
            component_index: component_index.parse().ok()?,
            page_index: page_index.parse().ok()?,
        },
        skip_prefix_rows: false,
    })
}

fn decode_ssed_partial_nonprefix_physical_offset_cursor(
    value: &str,
    skip_prefix_rows: bool,
) -> Option<SsedPartialNonprefixCursor> {
    let mut parts = value.split(':');
    let component_index = parts.next()?.parse().ok()?;
    let page_index = parts.next()?.parse().ok()?;
    let offset = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(SsedPartialNonprefixCursor::MatchedPhysicalOffset {
        cursor: SsedPartialIndexScanCursor {
            component_index,
            page_index,
        },
        offset,
        skip_prefix_rows,
    })
}

fn encode_ssed_partial_nonprefix_cursor(
    cursor: Option<SsedPartialIndexScanCursor>,
    skip_prefix_rows: bool,
) -> String {
    let cursor = cursor.unwrap_or(SsedPartialIndexScanCursor {
        component_index: 0,
        page_index: 0,
    });
    let prefix = if skip_prefix_rows {
        SSED_PARTIAL_NONPREFIX_INDEX_CURSOR_PREFIX
    } else {
        SSED_PARTIAL_NONPREFIX_NOSKIP_INDEX_CURSOR_PREFIX
    };
    format!("{prefix}{}:{}", cursor.component_index, cursor.page_index)
}

fn encode_ssed_partial_unverified_nonprefix_cursor(
    cursor: Option<SsedPartialIndexScanCursor>,
) -> String {
    let cursor = cursor.unwrap_or(SsedPartialIndexScanCursor {
        component_index: 0,
        page_index: 0,
    });
    format!(
        "{SSED_PARTIAL_NONPREFIX_UNVERIFIED_INDEX_CURSOR_PREFIX}{}:{}",
        cursor.component_index, cursor.page_index
    )
}

fn encode_ssed_partial_nonprefix_offset_cursor(offset: String, skip_prefix_rows: bool) -> String {
    let prefix = if skip_prefix_rows {
        SSED_PARTIAL_NONPREFIX_OFFSET_CURSOR_PREFIX
    } else {
        SSED_PARTIAL_NONPREFIX_NOSKIP_OFFSET_CURSOR_PREFIX
    };
    format!("{prefix}{offset}")
}

fn encode_ssed_partial_nonprefix_physical_offset_cursor(
    cursor: SsedPartialIndexScanCursor,
    offset: String,
    skip_prefix_rows: bool,
) -> String {
    let prefix = if skip_prefix_rows {
        SSED_PARTIAL_NONPREFIX_PHYSICAL_OFFSET_CURSOR_PREFIX
    } else {
        SSED_PARTIAL_NONPREFIX_NOSKIP_PHYSICAL_OFFSET_CURSOR_PREFIX
    };
    format!(
        "{prefix}{}:{}:{offset}",
        cursor.component_index, cursor.page_index
    )
}

fn decode_ssed_fulltext_title_cursor(cursor: Option<&str>) -> Option<SsedFulltextTitleCursor> {
    let value = cursor?.strip_prefix("title:")?;
    if let Some(cursor) = decode_ssed_partial_index_scan_cursor(Some(value)) {
        return Some(SsedFulltextTitleCursor::Physical(cursor));
    }
    value
        .parse::<usize>()
        .ok()
        .map(SsedFulltextTitleCursor::MatchedOffset)
}

fn decode_ssed_fulltext_body_cursor(cursor: Option<&str>) -> usize {
    match cursor {
        Some(value) if value.starts_with("title:") => 0,
        Some(value) if value.starts_with(SSED_FULLTEXT_SIDECAR_BODY_CURSOR_PREFIX) => 0,
        Some(value) if value.starts_with("chronology:") => 0,
        Some(value) if let Some(body) = value.strip_prefix("body:") => {
            body.parse::<usize>().unwrap_or(0)
        }
        Some(_) | None => 0,
    }
}

const SSED_FULLTEXT_SIDECAR_BODY_CURSOR_PREFIX: &str = "sidecar-body:";
const SSED_FULLTEXT_SIDECAR_BODY_START_CURSOR: &str = "sidecar-body-start";
const SSED_FULLTEXT_SIDECAR_BODY_PHYSICAL_CURSOR_PREFIX: &str = "sidecar-body-row:";
const SSED_FULLTEXT_BODY_CURSOR_PREFIX: &str = "body-offset:";

fn decode_ssed_fulltext_sidecar_body_cursor(cursor: Option<&str>) -> Option<usize> {
    let cursor = cursor?;
    if cursor == SSED_FULLTEXT_SIDECAR_BODY_START_CURSOR {
        return Some(0);
    }
    if let Some(value) = cursor.strip_prefix(SSED_FULLTEXT_SIDECAR_BODY_CURSOR_PREFIX) {
        return value.parse::<usize>().ok();
    }
    if cursor.starts_with(SSED_FULLTEXT_SIDECAR_BODY_PHYSICAL_CURSOR_PREFIX) {
        return None;
    }
    if cursor.starts_with("title:")
        || cursor.starts_with("body:")
        || cursor.starts_with("chronology:")
        || cursor.starts_with("row:")
    {
        return None;
    }
    Some(decode_offset_cursor(Some(cursor)))
}

fn encode_ssed_fulltext_sidecar_body_cursor(offset: usize) -> String {
    format!("{SSED_FULLTEXT_SIDECAR_BODY_CURSOR_PREFIX}{offset}")
}

fn encode_ssed_fulltext_body_cursor(offset: usize) -> String {
    format!("body:{offset}")
}

fn encode_ssed_fulltext_sidecar_body_start_cursor() -> String {
    SSED_FULLTEXT_SIDECAR_BODY_START_CURSOR.to_owned()
}

fn decode_ssed_fulltext_sidecar_body_physical_cursor(
    cursor: Option<&str>,
) -> Option<SsedSidecarBodyCursor> {
    let value = cursor?.strip_prefix(SSED_FULLTEXT_SIDECAR_BODY_PHYSICAL_CURSOR_PREFIX)?;
    decode_ssed_sidecar_row_cursor(value)
}

fn decode_ssed_sidecar_row_cursor(value: &str) -> Option<SsedSidecarBodyCursor> {
    let mut parts = value.split(':');
    let sidecar_name = decode_hex_string(parts.next()?)?;
    let table = decode_hex_string(parts.next()?)?;
    let id_column = decode_hex_string(parts.next()?)?;
    let id_rule = match parts.next()? {
        "direct" => SsedSidecarIdRule::DirectColumn,
        "rowid-times-five" => SsedSidecarIdRule::RowIdTimesFive,
        _ => return None,
    };
    let order_value = decode_hex_string(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some(SsedSidecarBodyCursor {
        sidecar_name,
        table,
        id_column,
        id_rule,
        order_value,
    })
}

fn encode_ssed_fulltext_sidecar_body_physical_cursor(cursor: &SsedSidecarBodyCursor) -> String {
    format!(
        "{}{}",
        SSED_FULLTEXT_SIDECAR_BODY_PHYSICAL_CURSOR_PREFIX,
        encode_ssed_sidecar_row_cursor(cursor)
    )
}

fn encode_ssed_sidecar_row_cursor(cursor: &SsedSidecarBodyCursor) -> String {
    let id_rule = match cursor.id_rule {
        SsedSidecarIdRule::DirectColumn => "direct",
        SsedSidecarIdRule::RowIdTimesFive => "rowid-times-five",
    };
    format!(
        "{}:{}:{}:{}:{}",
        hex::encode(cursor.sidecar_name.as_bytes()),
        hex::encode(cursor.table.as_bytes()),
        hex::encode(cursor.id_column.as_bytes()),
        id_rule,
        hex::encode(cursor.order_value.as_bytes())
    )
}

fn decode_hex_string(value: &str) -> Option<String> {
    String::from_utf8(hex::decode(value).ok()?).ok()
}

fn decode_ssed_fulltext_body_physical_cursor(
    cursor: Option<&str>,
) -> Option<SsedFulltextBodyCursor> {
    let value = cursor?.strip_prefix(SSED_FULLTEXT_BODY_CURSOR_PREFIX)?;
    let (component_hex, offset_hex) = value.split_once(':')?;
    let component = String::from_utf8(hex::decode(component_hex).ok()?).ok()?;
    let offset = u64::from_str_radix(offset_hex, 16).ok()?;
    Some(SsedFulltextBodyCursor { component, offset })
}

fn encode_ssed_fulltext_body_physical_cursor(cursor: &SsedFulltextBodyCursor) -> String {
    format!(
        "{}{}:{:x}",
        SSED_FULLTEXT_BODY_CURSOR_PREFIX,
        hex::encode(cursor.component.as_bytes()),
        cursor.offset
    )
}

fn ssed_fulltext_body_cursor_precedes(
    cursor: &SsedFulltextBodyCursor,
    component: &str,
    offset: u64,
) -> bool {
    component > cursor.component.as_str()
        || (component == cursor.component && offset > cursor.offset)
}

fn decode_ssed_fulltext_chronology_cursor(cursor: Option<&str>) -> Option<usize> {
    cursor?
        .strip_prefix("chronology:")
        .and_then(|value| value.parse::<usize>().ok())
}

fn decode_ssed_fulltext_row_cursor(cursor: Option<&str>) -> Option<usize> {
    cursor?
        .strip_prefix("row:")
        .and_then(|value| value.parse::<usize>().ok())
}

fn encode_ssed_fulltext_row_cursor(offset: usize) -> String {
    format!("row:{offset}")
}

const SSED_SIDECAR_TITLE_PHYSICAL_CURSOR_PREFIX: &str = "sidecar-title-row:";

fn decode_ssed_sidecar_title_cursor(cursor: Option<&str>) -> Option<SsedSidecarTitleCursor> {
    let cursor = cursor?;
    if let Some(value) = cursor.strip_prefix(SSED_SIDECAR_TITLE_CURSOR_PREFIX) {
        return value.parse().ok().map(SsedSidecarTitleCursor::Offset);
    }
    decode_ssed_sidecar_row_cursor(cursor.strip_prefix(SSED_SIDECAR_TITLE_PHYSICAL_CURSOR_PREFIX)?)
        .map(SsedSidecarTitleCursor::Physical)
}

fn encode_ssed_sidecar_title_cursor(offset: usize) -> String {
    format!("{SSED_SIDECAR_TITLE_CURSOR_PREFIX}{offset}")
}

fn encode_ssed_sidecar_title_physical_cursor(cursor: &SsedSidecarBodyCursor) -> String {
    format!(
        "{}{}",
        SSED_SIDECAR_TITLE_PHYSICAL_CURSOR_PREFIX,
        encode_ssed_sidecar_row_cursor(cursor)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SsedTitleLabelFallbackStop {
    Exhausted,
    Budget,
    PageFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SsedAuxLabelSearchStop {
    Exhausted,
    PageFull,
}

fn decode_ssed_title_label_cursor(cursor: Option<&str>) -> Option<usize> {
    let cursor = cursor?;
    cursor
        .strip_prefix(SSED_TITLE_LABEL_UNVERIFIED_CURSOR_PREFIX)
        .or_else(|| cursor.strip_prefix(SSED_TITLE_LABEL_CURSOR_PREFIX))?
        .parse()
        .ok()
}

fn encode_ssed_title_label_cursor(offset: usize) -> String {
    format!("{SSED_TITLE_LABEL_CURSOR_PREFIX}{offset}")
}

fn encode_ssed_unverified_title_label_cursor(offset: usize) -> String {
    format!("{SSED_TITLE_LABEL_UNVERIFIED_CURSOR_PREFIX}{offset}")
}

fn decode_ssed_aux_label_cursor(cursor: Option<&str>) -> Option<usize> {
    cursor?
        .strip_prefix(SSED_AUX_LABEL_CURSOR_PREFIX)?
        .parse()
        .ok()
}

fn encode_ssed_aux_label_cursor(offset: usize) -> String {
    format!("{SSED_AUX_LABEL_CURSOR_PREFIX}{offset}")
}

fn decode_ssed_unverified_offset_cursor(cursor: Option<&str>) -> Option<usize> {
    cursor?
        .strip_prefix(SSED_UNVERIFIED_OFFSET_CURSOR_PREFIX)?
        .parse()
        .ok()
}

fn encode_ssed_unverified_offset_cursor(offset: usize) -> String {
    format!("{SSED_UNVERIFIED_OFFSET_CURSOR_PREFIX}{offset}")
}

fn ssed_title_label_fallback_is_reasonable(mode: &SearchMode, needle: &str) -> bool {
    match mode {
        SearchMode::Exact | SearchMode::Backward => needle.chars().count() >= 2,
        SearchMode::Forward => true,
        SearchMode::Partial | SearchMode::FullText | SearchMode::Advanced(_) => false,
    }
}

fn ssed_title_label_fallback_row_matches(
    package: &ReaderBookPackage,
    mode: &SearchMode,
    needle: &str,
    row: &SsedIndexRow,
) -> bool {
    let key = ssed_index_row_match_text(row);
    if ssed_search_mode_matches(mode, &key, needle) {
        if mode == &SearchMode::Exact
            && ssed_index_component_name_is_cross_reference(&row.component)
        {
            if matches!(
                package.ssed_index_row_points_to_dense_sidecar_anchor(row),
                Ok(true)
            ) {
                return true;
            }
            let display = package.ssed_display_text_for_index_row(row);
            if display == row.key {
                return true;
            }
            let display_keys = ssed_display_label_match_keys_for_row(row, &display);
            return !display_keys.is_empty()
                && display_keys
                    .iter()
                    .any(|display_key| ssed_search_mode_matches(mode, display_key, needle));
        }
        return true;
    }
    let display = package.ssed_display_text_for_index_row(row);
    let display_keys = ssed_display_label_match_keys_for_row(row, &display);
    if display_keys.is_empty() {
        return false;
    }
    display_keys
        .iter()
        .any(|display_key| ssed_search_mode_matches(mode, display_key, needle))
}

fn ssed_display_label_match_keys_for_row(row: &SsedIndexRow, display: &str) -> Vec<String> {
    if display == row.key {
        return Vec::new();
    }
    let mut display_keys = ssed_display_label_match_texts(display);
    if ssed_index_component_name_is_backward(&row.component) {
        for display_key in &mut display_keys {
            *display_key = reverse_search_match_text(display_key);
        }
    }
    display_keys
}

fn ssed_search_mode_matches(mode: &SearchMode, key: &str, needle: &str) -> bool {
    match mode {
        SearchMode::Exact => key == needle,
        SearchMode::Forward => key.starts_with(needle),
        SearchMode::Backward => key.ends_with(needle),
        SearchMode::Partial => key.contains(needle),
        SearchMode::FullText | SearchMode::Advanced(_) => false,
    }
}

fn ssed_aux_label_search_row_matches(mode: &SearchMode, needle: &str, label: &str) -> bool {
    ssed_aux_label_search_match_texts(label)
        .iter()
        .any(|candidate| ssed_search_mode_matches(mode, candidate, needle))
}

fn ssed_aux_label_search_match_texts(label: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    push_unique_aux_label_match_text(&mut candidates, label);
    let stripped = label.trim_start_matches(is_ssed_aux_label_decoration);
    if stripped != label {
        push_unique_aux_label_match_text(&mut candidates, stripped);
    }
    candidates
}

fn push_unique_aux_label_match_text(candidates: &mut Vec<String>, label: &str) {
    let normalized = normalize_search_match_text(label.trim());
    if !normalized.is_empty() && !candidates.contains(&normalized) {
        candidates.push(normalized);
    }
}

fn is_ssed_aux_label_decoration(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '■' | '□'
                | '▲'
                | '△'
                | '▼'
                | '▽'
                | '◆'
                | '◇'
                | '●'
                | '○'
                | '◎'
                | '§'
                | '・'
                | '-'
                | '－'
                | '–'
                | '—'
                | '▶'
                | '▷'
        )
}

fn ssed_sidecar_title_search_mode(mode: &SearchMode) -> Option<SsedSidecarTitleSearchMode> {
    match mode {
        SearchMode::Exact => Some(SsedSidecarTitleSearchMode::Exact),
        SearchMode::Forward => Some(SsedSidecarTitleSearchMode::Forward),
        SearchMode::Backward => Some(SsedSidecarTitleSearchMode::Backward),
        SearchMode::Partial => Some(SsedSidecarTitleSearchMode::Partial),
        SearchMode::FullText | SearchMode::Advanced(_) => None,
    }
}

fn ssed_sidecar_title_auto_append_is_bounded(query: &str) -> bool {
    let query = query.trim();
    !query.is_empty() && !query.chars().any(char::is_whitespace)
}

fn ssed_partial_prefix_prepass_is_bounded(query: &str) -> bool {
    let query = query.trim();
    !query.is_empty() && !query.chars().any(char::is_whitespace)
}

fn ssed_fulltext_sidecar_title_prepass_is_bounded(query: &str) -> bool {
    let query = query.trim();
    query.len() >= 2
        && query.is_ascii()
        && !query.chars().any(char::is_whitespace)
        && query.bytes().any(|byte| byte.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::{
        SsedPartialIndexScanCursor, SsedPartialNonprefixCursor,
        decode_ssed_partial_nonprefix_cursor, decode_ssed_title_label_cursor,
        encode_ssed_partial_nonprefix_cursor, encode_ssed_partial_nonprefix_offset_cursor,
        encode_ssed_partial_nonprefix_physical_offset_cursor,
        encode_ssed_partial_unverified_nonprefix_cursor, encode_ssed_unverified_title_label_cursor,
        ssed_fulltext_first_byte_candidate_offset, ssed_fulltext_sidecar_title_prepass_is_bounded,
        ssed_partial_prefix_prepass_is_bounded, ssed_sidecar_title_auto_append_is_bounded,
    };

    #[test]
    fn sidecar_title_auto_append_accepts_bounded_single_token_queries() {
        assert!(ssed_sidecar_title_auto_append_is_bounded("et"));
        assert!(ssed_sidecar_title_auto_append_is_bounded(" abaisser "));
        assert!(ssed_sidecar_title_auto_append_is_bounded("白水"));
        assert!(ssed_sidecar_title_auto_append_is_bounded("丂"));
        assert!(ssed_sidecar_title_auto_append_is_bounded("◯に"));
        assert!(ssed_sidecar_title_auto_append_is_bounded("ａｂｃ"));
        assert!(!ssed_sidecar_title_auto_append_is_bounded(""));
        assert!(!ssed_sidecar_title_auto_append_is_bounded("two words"));
    }

    #[test]
    fn fulltext_sidecar_title_prepass_is_limited_to_bounded_ascii_word_queries() {
        assert!(ssed_fulltext_sidecar_title_prepass_is_bounded("In"));
        assert!(ssed_fulltext_sidecar_title_prepass_is_bounded("read"));
        assert!(!ssed_fulltext_sidecar_title_prepass_is_bounded("a"));
        assert!(!ssed_fulltext_sidecar_title_prepass_is_bounded("two words"));
        assert!(!ssed_fulltext_sidecar_title_prepass_is_bounded("犬"));
        assert!(!ssed_fulltext_sidecar_title_prepass_is_bounded("ｉｎ"));
    }

    #[test]
    fn fulltext_byte_candidate_offset_returns_earliest_candidate_after_start() {
        let candidates = vec![b"target".to_vec(), b"needle".to_vec()];
        let data = b"xx needle xx target xx needle";

        assert_eq!(
            ssed_fulltext_first_byte_candidate_offset(data, &candidates, 0),
            Some(3)
        );
        assert_eq!(
            ssed_fulltext_first_byte_candidate_offset(data, &candidates, 4),
            Some(13)
        );
        assert_eq!(
            ssed_fulltext_first_byte_candidate_offset(data, &candidates, data.len()),
            None
        );
    }

    #[test]
    fn partial_prefix_prepass_is_limited_to_bounded_single_token_queries() {
        assert!(ssed_partial_prefix_prepass_is_bounded("read"));
        assert!(ssed_partial_prefix_prepass_is_bounded("白水"));
        assert!(!ssed_partial_prefix_prepass_is_bounded(""));
        assert!(!ssed_partial_prefix_prepass_is_bounded("United States"));
    }

    #[test]
    fn partial_nonprefix_cursors_preserve_prefix_skip_state() {
        let old_index = "ssed-partial-nonprefix-index:2:56";
        assert_eq!(
            decode_ssed_partial_nonprefix_cursor(Some(old_index)),
            Some(SsedPartialNonprefixCursor::Physical {
                cursor: SsedPartialIndexScanCursor {
                    component_index: 2,
                    page_index: 56,
                },
                skip_prefix_rows: true,
            })
        );

        let unverified_index =
            encode_ssed_partial_unverified_nonprefix_cursor(Some(SsedPartialIndexScanCursor {
                component_index: 2,
                page_index: 56,
            }));
        assert_eq!(
            unverified_index,
            "ssed-partial-nonprefix-unverified-index:2:56"
        );
        assert_eq!(
            decode_ssed_partial_nonprefix_cursor(Some(&unverified_index)),
            Some(SsedPartialNonprefixCursor::UnverifiedPhysical {
                cursor: SsedPartialIndexScanCursor {
                    component_index: 2,
                    page_index: 56,
                },
                skip_prefix_rows: true,
            })
        );

        let noskip_index = encode_ssed_partial_nonprefix_cursor(
            Some(SsedPartialIndexScanCursor {
                component_index: 2,
                page_index: 56,
            }),
            false,
        );
        assert_eq!(noskip_index, "ssed-partial-nonprefix-noskip-index:2:56");
        assert_eq!(
            decode_ssed_partial_nonprefix_cursor(Some(&noskip_index)),
            Some(SsedPartialNonprefixCursor::Physical {
                cursor: SsedPartialIndexScanCursor {
                    component_index: 2,
                    page_index: 56,
                },
                skip_prefix_rows: false,
            })
        );

        let noskip_offset = encode_ssed_partial_nonprefix_offset_cursor("17".to_owned(), false);
        assert_eq!(
            decode_ssed_partial_nonprefix_cursor(Some(&noskip_offset)),
            Some(SsedPartialNonprefixCursor::MatchedOffset {
                offset: 17,
                skip_prefix_rows: false,
            })
        );

        let physical_offset = encode_ssed_partial_nonprefix_physical_offset_cursor(
            SsedPartialIndexScanCursor {
                component_index: 2,
                page_index: 56,
            },
            "17".to_owned(),
            true,
        );
        assert_eq!(
            physical_offset,
            "ssed-partial-nonprefix-physical-offset:2:56:17"
        );
        assert_eq!(
            decode_ssed_partial_nonprefix_cursor(Some(&physical_offset)),
            Some(SsedPartialNonprefixCursor::MatchedPhysicalOffset {
                cursor: SsedPartialIndexScanCursor {
                    component_index: 2,
                    page_index: 56,
                },
                offset: 17,
                skip_prefix_rows: true,
            })
        );
    }

    #[test]
    fn title_label_unverified_cursor_decodes_as_title_label_offset() {
        let cursor = encode_ssed_unverified_title_label_cursor(42);
        assert_eq!(cursor, "ssed-title-label-unverified:42");
        assert_eq!(decode_ssed_title_label_cursor(Some(&cursor)), Some(42));
        assert_eq!(
            decode_ssed_title_label_cursor(Some("ssed-title-label:42")),
            Some(42)
        );
    }
}

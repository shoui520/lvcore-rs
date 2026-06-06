use std::{cell::Cell, collections::HashSet};

use super::*;

const SSED_TITLE_LABEL_SEARCH_FALLBACK_MAX_ROWS: usize = 256;
const SSED_TITLE_LABEL_SEARCH_FALLBACK_EMPTY_PAGE_MAX_ROWS: usize = 20_480;
const SSED_INDEX_EMPTY_PHYSICAL_CURSOR_ADVANCE_LIMIT: usize = 2;
const SSED_INDEX_EMPTY_PHYSICAL_SCAN_LEAF_PAGE_BUDGET: usize = 128;
const SSED_FULLTEXT_UNBOUNDED_TITLE_PREPASS_MAX_INDEX_BLOCKS: u32 = 2048;
const SSED_SIDECAR_TITLE_CURSOR_PREFIX: &str = "sidecar-title:";
const SSED_TITLE_LABEL_CURSOR_PREFIX: &str = "ssed-title-label:";

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
        if let Some(sidecar_offset) = decode_ssed_sidecar_title_cursor(query.cursor.as_deref()) {
            return self.search_ssed_sidecar_title_page(query, sidecar_offset, Vec::new());
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
        let offset = if partial_scan_cursor.is_some() || prefiltered_scan_cursor.is_some() {
            0
        } else {
            decode_offset_cursor(query.cursor.as_deref())
        };
        let page_limit = query.limit.saturating_add(1);
        let gaiji_policy = query.label_gaiji_policy();
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
                    collector.extend_diagnostics(fallback_page.diagnostics);
                } else {
                    if fallback_page.next_cursor.is_none()
                        && fallback_page.hits.len() < query.limit
                        && ssed_sidecar_title_auto_append_is_bounded(&query.query)
                    {
                        self.append_ssed_sidecar_title_hits(query, &mut fallback_page, 0)?;
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
        if page.next_cursor.is_none() {
            page.next_cursor = physical_next_cursor;
        }
        if page.next_cursor.is_none()
            && query.cursor.is_none()
            && page.hits.len() < query.limit
            && ssed_sidecar_title_auto_append_is_bounded(&query.query)
        {
            self.append_ssed_sidecar_title_hits(query, &mut page, 0)?;
        }
        Ok(page)
    }

    fn scan_ssed_partial_index_rows_paged_until_visible(
        &self,
        needle: &str,
        cursor: Option<SsedPartialIndexScanCursor>,
        collector: &mut SsedIndexSearchCollector<'_>,
    ) -> Result<Option<String>> {
        let mut current_cursor = cursor;
        let mut advanced_empty_pages = 0usize;
        let mut use_empty_scan_budget = false;
        loop {
            let scan_result = if use_empty_scan_budget {
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
            SsedTitleLabelFallbackStop::Budget | SsedTitleLabelFallbackStop::PageFull => {
                Some(encode_ssed_title_label_cursor(checked_rows))
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
        sidecar_offset: usize,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<SearchPage> {
        let mut page = SearchPage {
            hits: Vec::new(),
            next_cursor: None,
            result_sequence: None,
            diagnostics,
        };
        self.append_ssed_sidecar_title_hits(query, &mut page, sidecar_offset)?;
        Ok(page)
    }

    fn append_ssed_sidecar_title_hits(
        &self,
        query: &SearchQuery,
        page: &mut SearchPage,
        sidecar_offset: usize,
    ) -> Result<()> {
        let Some(mode) = ssed_sidecar_title_search_mode(&query.mode) else {
            return Ok(());
        };
        let remaining = query.limit.saturating_sub(page.hits.len());
        if remaining == 0 {
            return Ok(());
        }
        let sidecar_page = search_ssed_dense_sidecar_titles_with_resolvers(
            self.ssed_sidecar_body_resolvers()?,
            mode,
            &query.query,
            sidecar_offset,
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
        for hit in sidecar_page.hits {
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
            page.next_cursor = Some(encode_ssed_sidecar_title_cursor(sidecar_page.matched_count));
        }
        Ok(())
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
        let chronology_cursor = decode_ssed_fulltext_chronology_cursor(query.cursor.as_deref());
        let row_cursor = decode_ssed_fulltext_row_cursor(query.cursor.as_deref());
        let offset = decode_ssed_fulltext_body_cursor(query.cursor.as_deref());
        let mut diagnostics = Vec::new();
        if query.cursor.is_none()
            && ssed_fulltext_sidecar_title_prepass_is_bounded(&query.query)
            && let Some(page) = self.ssed_fulltext_sidecar_title_prepass(query)?
        {
            return Ok(page);
        }
        let honmon_body_window_scan_needed =
            self.ssed_honmon_body_window_scan_is_needed(catalog, &mut diagnostics)?;
        if honmon_body_window_scan_needed
            && query.cursor.is_none()
            && let Some(page) =
                self.ssed_fulltext_initial_title_index_prepass(query, &needle, page_limit)?
        {
            return Ok(page);
        }
        if honmon_body_window_scan_needed
            && query.cursor.is_none()
            && let Some(page) =
                self.ssed_fulltext_initial_partial_title_index_prepass(query, &needle, page_limit)?
        {
            return Ok(page);
        }
        if honmon_body_window_scan_needed
            && let Some(title_cursor) = title_cursor
            && let Some(page) =
                self.ssed_fulltext_title_index_prepass(query, &needle, title_cursor, page_limit)?
        {
            return Ok(page);
        }

        let mut hits = Vec::new();
        let sidecar_limit = if query.cursor.is_none() {
            query.limit
        } else {
            page_limit
        };
        let body_cursor_explicit = query
            .cursor
            .as_deref()
            .is_some_and(|cursor| cursor.starts_with("body:"));
        let row_cursor_explicit = row_cursor.is_some();
        let run_sidecar =
            chronology_cursor.is_none() && !body_cursor_explicit && !row_cursor_explicit;
        let sidecar_page = if run_sidecar {
            search_ssed_dense_sidecar_bodies_with_resolvers(
                self.ssed_sidecar_body_resolvers()?,
                &query.query,
                offset,
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
        for hit in sidecar_page.hits {
            let title = if hit.body.title.trim().is_empty() {
                hit.body.text.chars().take(80).collect::<String>()
            } else {
                hit.body.title.clone()
            };
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
        if query.cursor.is_none() && hits.len() >= query.limit && !sidecar_page.exhausted {
            hits.truncate(query.limit);
            return Ok(SearchPage {
                hits,
                next_cursor: Some(query.limit.to_string()),
                result_sequence: None,
                diagnostics,
            });
        }
        if hits.len() >= page_limit && !sidecar_page.exhausted {
            hits.truncate(query.limit);
            return Ok(SearchPage {
                hits,
                next_cursor: Some((offset + query.limit).to_string()),
                result_sequence: None,
                diagnostics,
            });
        }
        if (query.cursor.is_none() || title_cursor.is_some() || chronology_cursor.is_some())
            && hits.len() < page_limit
        {
            let chronology_offset = chronology_cursor.unwrap_or(0);
            let remaining = page_limit.saturating_sub(hits.len());
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
                hits.truncate(query.limit);
                return Ok(SearchPage {
                    hits,
                    next_cursor: Some(format!(
                        "chronology:{}",
                        chronology_offset.saturating_add(query.limit)
                    )),
                    result_sequence: None,
                    diagnostics,
                });
            }
            if hits.len() >= query.limit {
                hits.truncate(query.limit);
                return Ok(SearchPage {
                    hits,
                    next_cursor: chronology_exhausted.then_some("body:0".to_owned()).or_else(
                        || {
                            Some(format!(
                                "chronology:{}",
                                chronology_offset.saturating_add(chronology_hits)
                            ))
                        },
                    ),
                    result_sequence: None,
                    diagnostics,
                });
            }
        }
        let byte_candidates = ssed_body_search_byte_candidates(&query.query);
        let row_driven_search_allowed = query.cursor.is_none() || row_cursor.is_some();
        if honmon_body_window_scan_needed
            && row_driven_search_allowed
            && (query.cursor.is_none() || row_cursor.is_some())
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
                    max_checked_rows: Some(SSED_FULLTEXT_ROW_PREFETCH_MAX_ROWS),
                    gaiji_policy: &label_policy,
                })?;
            let row_page_has_hits = !row_page.hits.is_empty();
            if row_page.exhausted || row_page_has_hits || row_cursor.is_some() {
                diagnostics.extend(row_page.diagnostics);
                hits.extend(row_page.hits);
                let next_cursor = if row_page.exhausted {
                    None
                } else {
                    Some(format!("row:{}", row_page.next_row_offset))
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
        if !honmon_body_window_scan_needed {
            hits.truncate(query.limit);
            return Ok(SearchPage {
                hits,
                next_cursor: None,
                result_sequence: None,
                diagnostics,
            });
        }
        diagnostics.push(Diagnostic::info(
            "ssed_fulltext_body_window_scan",
            format!(
                "SSED full-text search is scanning bounded HONMON windows behind native index targets ({} bytes per target)",
                SSED_FULLTEXT_BODY_WINDOW_BYTES
            ),
        ));
        let mut matched_count = 0usize;
        let body_offset = if sidecar_page.exhausted {
            offset.saturating_sub(sidecar_page.matched_count)
        } else {
            offset
        };
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
                matched_count = matched_count.saturating_add(1);
                if hits.len() >= page_limit {
                    break 'components;
                }
            }
        }
        let next_cursor = (hits.len() > query.limit).then(|| (offset + query.limit).to_string());
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
        let resolvers = self.ssed_sidecar_body_resolvers()?;
        if resolvers.is_empty() {
            return Ok(true);
        }
        if resolvers
            .iter()
            .any(SsedSidecarBodyResolver::is_ordered_honbun_renderer_body)
        {
            diagnostics.push(Diagnostic::info(
                "ssed_fulltext_honmon_scan_skipped_sidecar_backed",
                "SSED full-text search skipped raw HONMON scanning because ordered HONBUN renderer rows are the visual body source",
            ));
            return Ok(false);
        }

        const SIDECAR_BACKED_SAMPLE_TARGETS: usize = 16;
        let mut checked_targets = 0usize;
        let mut sidecar_backed_targets = 0usize;
        let mut sample_diagnostics = self.scan_ssed_simple_index_rows_with_filters(
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
        diagnostics.append(&mut sample_diagnostics);
        if checked_targets > 0 && checked_targets == sidecar_backed_targets {
            diagnostics.push(
                Diagnostic::info(
                    "ssed_fulltext_honmon_scan_skipped_sidecar_backed",
                    "SSED full-text search skipped raw HONMON scanning because sampled native index targets dereference to dense sidecar bodies",
                )
                .with_context("checked_targets", checked_targets.to_string()),
            );
            return Ok(false);
        }
        Ok(true)
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
            next_row_offset: offset.saturating_add(checked_rows),
            diagnostics,
        })
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
        page.next_cursor = Some("body:0".to_owned());
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
        page.next_cursor = page
            .next_cursor
            .as_deref()
            .map(|cursor| format!("title:{cursor}"))
            .or_else(|| physical_next_cursor.map(|cursor| format!("title:{cursor}")))
            .or_else(|| Some("body:0".to_owned()));
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
        self.append_ssed_sidecar_title_hits(&title_query, &mut page, 0)?;
        if page.hits.is_empty() {
            return Ok(None);
        }
        page.next_cursor = Some("body:0".to_owned());
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
                .scan_ssed_partial_index_rows_paged_until_visible(
                    needle,
                    Some(cursor),
                    &mut collector,
                )?,
        };
        let mut page = collector.into_search_page(query.limit);
        if page.hits.is_empty() {
            return Ok(None);
        }
        page.next_cursor = page
            .next_cursor
            .as_deref()
            .map(|cursor| format!("title:{cursor}"))
            .or_else(|| physical_next_cursor.map(|cursor| format!("title:{cursor}")))
            .or_else(|| Some("body:0".to_owned()));
        page.diagnostics.insert(
            0,
            Diagnostic::info(
                "ssed_fulltext_title_index_prepass",
                "SSED full-text search satisfied the first page from native title/index labels before scanning HONMON bodies",
            ),
        );
        Ok(Some(page))
    }
}

const SSED_FULLTEXT_ROW_PREFETCH_MAX_ROWS: usize = 512;

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
        Some(value) if value.starts_with("chronology:") => 0,
        Some(value) if let Some(body) = value.strip_prefix("body:") => {
            body.parse::<usize>().unwrap_or(0)
        }
        value => decode_offset_cursor(value),
    }
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

fn decode_ssed_sidecar_title_cursor(cursor: Option<&str>) -> Option<usize> {
    cursor?
        .strip_prefix(SSED_SIDECAR_TITLE_CURSOR_PREFIX)?
        .parse()
        .ok()
}

fn encode_ssed_sidecar_title_cursor(offset: usize) -> String {
    format!("{SSED_SIDECAR_TITLE_CURSOR_PREFIX}{offset}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SsedTitleLabelFallbackStop {
    Exhausted,
    Budget,
    PageFull,
}

fn decode_ssed_title_label_cursor(cursor: Option<&str>) -> Option<usize> {
    cursor?
        .strip_prefix(SSED_TITLE_LABEL_CURSOR_PREFIX)?
        .parse()
        .ok()
}

fn encode_ssed_title_label_cursor(offset: usize) -> String {
    format!("{SSED_TITLE_LABEL_CURSOR_PREFIX}{offset}")
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
        return true;
    }
    let display = package.ssed_display_text_for_index_row(row);
    if display == row.key {
        return false;
    }
    let mut display_keys = ssed_title_label_fallback_display_match_texts(&display);
    if display_keys.is_empty() {
        return false;
    }
    if ssed_index_component_name_is_backward(&row.component) {
        for display_key in &mut display_keys {
            *display_key = reverse_search_match_text(display_key);
        }
    }
    display_keys
        .iter()
        .any(|display_key| ssed_search_mode_matches(mode, display_key, needle))
}

fn ssed_title_label_fallback_display_match_texts(display: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    push_unique_ssed_title_label_match_text(&mut candidates, normalize_search_match_text(display));
    if let Some(headword) = ssed_visible_title_headword_segment(display) {
        push_unique_ssed_title_label_match_text(
            &mut candidates,
            normalize_search_match_text(headword),
        );
    }
    candidates
}

fn push_unique_ssed_title_label_match_text(candidates: &mut Vec<String>, candidate: String) {
    if !candidate.is_empty() && !candidates.iter().any(|existing| existing == &candidate) {
        candidates.push(candidate);
    }
}

fn ssed_visible_title_headword_segment(display: &str) -> Option<&str> {
    let display = display.trim();
    if display.is_empty() {
        return None;
    }
    let end = display
        .char_indices()
        .find_map(|(index, ch)| {
            (ch.is_whitespace() || ssed_visible_title_metadata_boundary(ch)).then_some(index)
        })
        .unwrap_or(display.len());
    let headword = display[..end].trim();
    (!headword.is_empty() && headword != display).then_some(headword)
}

fn ssed_visible_title_metadata_boundary(ch: char) -> bool {
    matches!(
        ch,
        '【' | '［'
            | '['
            | '〖'
            | '〘'
            | '《'
            | '〈'
            | '('
            | '（'
            | '〔'
            | '<'
            | '＜'
            | ':'
            | '：'
            | ','
            | '，'
            | '、'
            | ';'
            | '；'
            | '/'
            | '／'
            | '|'
            | '｜'
    )
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
    !query.is_empty() && query.is_ascii()
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
        ssed_fulltext_sidecar_title_prepass_is_bounded, ssed_sidecar_title_auto_append_is_bounded,
    };

    #[test]
    fn sidecar_title_auto_append_is_limited_to_bounded_ascii_queries() {
        assert!(ssed_sidecar_title_auto_append_is_bounded("et"));
        assert!(ssed_sidecar_title_auto_append_is_bounded(" abaisser "));
        assert!(!ssed_sidecar_title_auto_append_is_bounded(""));
        assert!(!ssed_sidecar_title_auto_append_is_bounded("◯に"));
        assert!(!ssed_sidecar_title_auto_append_is_bounded("白水"));
        assert!(!ssed_sidecar_title_auto_append_is_bounded("ａｂｃ"));
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
}

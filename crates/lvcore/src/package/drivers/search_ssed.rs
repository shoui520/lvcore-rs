use super::*;

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
        if !matches!(
            query.mode,
            SearchMode::Exact | SearchMode::Forward | SearchMode::Backward | SearchMode::Partial
        ) {
            return Ok(SearchPage::deferred(
                "SSED search mode is not implemented for simple title/index scanning yet",
            ));
        }

        let offset = decode_offset_cursor(query.cursor.as_deref());
        let page_limit = query.limit.saturating_add(1);
        let needle = normalize_search_match_text(&query.query);
        let gaiji_policy = query.label_gaiji_policy();
        let mut collector = SsedIndexSearchCollector::new(
            self,
            &query.mode,
            &needle,
            offset,
            page_limit,
            gaiji_policy,
        );
        let mut optimized_scan_components = 0usize;
        let mut scan_needs_linear_fallback = false;
        if matches!(
            query.mode,
            SearchMode::Exact | SearchMode::Forward | SearchMode::Backward
        ) {
            let scan_result =
                self.scan_ssed_simple_leaf_index_rows_near_key(&query.mode, &needle, |row| {
                    collector.push_row(row)
                })?;
            optimized_scan_components = scan_result.scanned_components;
            scan_needs_linear_fallback = scan_result.needs_linear_fallback;
            collector.extend_diagnostics(scan_result.diagnostics);
        }
        if !collector.has_hits() && (optimized_scan_components == 0 || scan_needs_linear_fallback) {
            let scan_diagnostics = if query.mode == SearchMode::Partial {
                self.scan_ssed_partial_index_rows(&needle, |row| collector.push_row(row))?
            } else {
                self.scan_ssed_simple_index_rows(None, |row| collector.push_row(row))?
            };
            collector.extend_diagnostics(scan_diagnostics);
        }
        Ok(collector.into_search_page(query.limit))
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
        let honmon_body_window_scan_needed =
            self.ssed_honmon_body_window_scan_is_needed(catalog, &mut diagnostics)?;
        if honmon_body_window_scan_needed
            && title_cursor.is_some()
            && let Some(page) = self.ssed_fulltext_title_index_prepass(
                query,
                &needle,
                title_cursor.unwrap_or(0),
                page_limit,
            )?
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
            if row_page.exhausted || query.cursor.is_none() || row_cursor.is_some() {
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

    fn ssed_fulltext_title_index_prepass(
        &self,
        query: &SearchQuery,
        needle: &str,
        title_offset: usize,
        page_limit: usize,
    ) -> Result<Option<SearchPage>> {
        let mut collector = SsedIndexSearchCollector::new(
            self,
            &SearchMode::Partial,
            needle,
            title_offset,
            page_limit,
            query.label_gaiji_policy(),
        );
        let scan_diagnostics =
            self.scan_ssed_partial_index_rows(needle, |row| collector.push_row(row))?;
        collector.extend_diagnostics(scan_diagnostics);
        let mut page = collector.into_search_page(query.limit);
        if page.hits.len() < query.limit {
            return Ok(None);
        }
        page.next_cursor = page
            .next_cursor
            .as_deref()
            .map(|cursor| format!("title:{cursor}"))
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

fn decode_ssed_fulltext_title_cursor(cursor: Option<&str>) -> Option<usize> {
    cursor?
        .strip_prefix("title:")
        .and_then(|value| value.parse::<usize>().ok())
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

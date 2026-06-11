use super::ssed_index::{read_index_page, ssed_component_read_base};
use super::*;

const LVED_RETAINED_INDEX_CURSOR_PREFIX: &str = "lved-retained-ssed-index:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LvedRetainedIndexCursor {
    matched_offset: usize,
}

impl ReaderBookPackage {
    pub(super) fn search_lved_retained_ssed_indexes(
        &self,
        query: &SearchQuery,
        _existing_hits: &[SearchHit],
        page_limit: usize,
        matched_offset: usize,
    ) -> Result<SearchPage> {
        let hits = Vec::new();
        let mut diagnostics = Vec::new();
        let mut deferred_match_count = 0usize;
        let mut deferred_match_diagnostic = None::<Diagnostic>;
        let mut retained_matches_seen = 0usize;
        let needle = normalize_search_match_text(&query.query);
        if needle.is_empty() || page_limit == 0 {
            return Ok(SearchPage {
                hits,
                next_cursor: None,
                result_sequence: None,
                diagnostics,
            });
        }

        for (component_ordinal, retained) in self.retained_ssed_components.iter().enumerate() {
            let Some(component) = retained_lved_index_component(component_ordinal, retained) else {
                continue;
            };
            if !retained_lved_component_matches_search_mode(&component, &query.mode) {
                continue;
            }
            let path = match self.resolve_readable_ssed_component_path(&component) {
                Ok(Some(path)) => path,
                Ok(None) => {
                    diagnostics.push(
                        Diagnostic::info(
                            "lved_retained_ssed_index_missing",
                            format!(
                                "{} was discovered as a retained LVED index but is not present on disk",
                                component.filename
                            ),
                        )
                        .with_context("component", &component.filename),
                    );
                    continue;
                }
                Err(error) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "lved_retained_ssed_index_decode_failed",
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
            let component_read_base = ssed_component_read_base(&component, &reader);
            let page_count = component.block_count() as usize;
            let mut scan_state = SsedIndexScanState::default();
            for page_index in 0..page_count {
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
                if unknown > 0 {
                    diagnostics.push(
                        Diagnostic::warning(
                            "lved_retained_ssed_index_unknown_leaf_bytes",
                            format!(
                                "{} had {unknown} unknown retained index leaf row(s)",
                                component.filename
                            ),
                        )
                        .with_context("component", &component.filename),
                    );
                }
                for row in rows {
                    let key = ssed_index_row_match_text(&row);
                    if !retained_lved_row_matches(&query.mode, &key, &needle) {
                        continue;
                    }
                    if retained_matches_seen < matched_offset {
                        retained_matches_seen = retained_matches_seen.saturating_add(1);
                        continue;
                    }
                    retained_matches_seen = retained_matches_seen.saturating_add(1);
                    deferred_match_count = deferred_match_count.saturating_add(1);
                    if deferred_match_diagnostic.is_none() {
                        deferred_match_diagnostic =
                            Some(retained_lved_index_match_deferred_diagnostic(&row));
                    }
                }
            }
        }
        if let Some(diagnostic) = deferred_match_diagnostic {
            diagnostics.push(
                diagnostic.with_context("deferred_match_count", deferred_match_count.to_string()),
            );
        }

        Ok(SearchPage {
            hits,
            next_cursor: None,
            result_sequence: None,
            diagnostics,
        })
    }

    pub(super) fn decode_lved_retained_index_cursor(&self, cursor: Option<&str>) -> Option<usize> {
        decode_lved_retained_index_cursor(cursor).map(|cursor| cursor.matched_offset)
    }
}

fn retained_lved_index_match_deferred_diagnostic(row: &SsedIndexRow) -> Diagnostic {
    Diagnostic::info(
        "lved_retained_ssed_index_match_deferred",
        "retained LVED SSED index match was not emitted because its block/offset target has no proven SQLite bridge",
    )
    .with_context("component", &row.component)
    .with_context("key", &row.key)
    .with_context("body_block", row.body.block.to_string())
    .with_context("body_offset", row.body.offset.to_string())
    .with_context(
        "body_address",
        format!("{:08x}:{:04x}", row.body.block, row.body.offset),
    )
    .with_context("title_block", row.title.block.to_string())
    .with_context("title_offset", row.title.offset.to_string())
    .with_context(
        "title_address",
        format!("{:08x}:{:04x}", row.title.block, row.title.offset),
    )
    .with_context("page_index", row.page_index.to_string())
}

fn retained_lved_index_component(
    component_ordinal: usize,
    retained: &RetainedSsedComponent,
) -> Option<SsedComponent> {
    if retained.role != SsedComponentRole::Index {
        return None;
    }
    let component_type = retained.component_type?;
    if !is_supported_index_type(component_type) || retained.start_block > retained.end_block {
        return None;
    }
    Some(SsedComponent {
        index: u8::try_from(component_ordinal).unwrap_or(u8::MAX),
        multi: 0,
        component_type,
        start_block: retained.start_block,
        end_block: retained.end_block,
        data: [0; 4],
        filename: retained.filename.clone(),
        role: SsedComponentRole::Index,
    })
}

fn retained_lved_component_matches_search_mode(
    component: &SsedComponent,
    mode: &SearchMode,
) -> bool {
    let is_backward = ssed_index_component_name_is_backward(&component.filename);
    match mode {
        SearchMode::Exact | SearchMode::Forward => !is_backward,
        SearchMode::Backward => is_backward,
        SearchMode::Partial => true,
        SearchMode::FullText | SearchMode::Advanced(_) => false,
    }
}

fn retained_lved_row_matches(mode: &SearchMode, key: &str, needle: &str) -> bool {
    match mode {
        SearchMode::Exact => key == needle,
        SearchMode::Forward => key.starts_with(needle),
        SearchMode::Backward => key.ends_with(needle),
        SearchMode::Partial => key.contains(needle),
        SearchMode::FullText | SearchMode::Advanced(_) => false,
    }
}

fn decode_lved_retained_index_cursor(cursor: Option<&str>) -> Option<LvedRetainedIndexCursor> {
    let cursor = cursor?.strip_prefix(LVED_RETAINED_INDEX_CURSOR_PREFIX)?;
    Some(LvedRetainedIndexCursor {
        matched_offset: cursor.parse().ok()?,
    })
}

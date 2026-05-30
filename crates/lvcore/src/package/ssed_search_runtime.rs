use std::collections::HashSet;

use super::drivers::ReaderBookPackage;
use super::ssed_search::{normalize_search_match_text, reverse_search_match_text};
use crate::diagnostics::Diagnostic;
use crate::error::Result;
use crate::search::{SearchHit, SearchMode, SearchPage};
use crate::ssed_index::SsedIndexRow;

pub(super) const SSED_FULLTEXT_BODY_WINDOW_BYTES: usize = 16 * 1024;
pub(super) const SSED_FULLTEXT_SCAN_WINDOW_BYTES: usize = 256 * 1024;
pub(super) const SSED_FULLTEXT_SCAN_OVERLAP_BYTES: usize = 512;

#[derive(Debug, Clone)]
pub(super) struct SsedFulltextRow {
    pub(super) offset: u64,
    pub(super) row: SsedIndexRow,
}

#[derive(Debug, Default)]
pub(super) struct SsedNearKeyScanResult {
    pub(super) scanned_components: usize,
    pub(super) needs_linear_fallback: bool,
    pub(super) diagnostics: Vec<Diagnostic>,
}

pub(super) struct SsedIndexSearchCollector<'a> {
    package: &'a ReaderBookPackage,
    mode: &'a SearchMode,
    needle: &'a str,
    offset: usize,
    page_limit: usize,
    matched_count: usize,
    hits: Vec<SearchHit>,
    diagnostics: Vec<Diagnostic>,
    seen_targets: HashSet<String>,
}

impl<'a> SsedIndexSearchCollector<'a> {
    pub(super) fn new(
        package: &'a ReaderBookPackage,
        mode: &'a SearchMode,
        needle: &'a str,
        offset: usize,
        page_limit: usize,
    ) -> Self {
        Self {
            package,
            mode,
            needle,
            offset,
            page_limit,
            matched_count: 0,
            hits: Vec::new(),
            diagnostics: Vec::new(),
            seen_targets: HashSet::new(),
        }
    }

    pub(super) fn push_row(&mut self, row: SsedIndexRow) -> Result<bool> {
        let key = ssed_index_row_match_text(&row);
        let row_matches = match self.mode {
            SearchMode::Exact => key == self.needle,
            SearchMode::Forward => key.starts_with(self.needle),
            SearchMode::Backward => key.ends_with(self.needle),
            SearchMode::Partial => key.contains(self.needle),
            SearchMode::FullText | SearchMode::Advanced(_) => false,
        };
        if !row_matches {
            return Ok(true);
        }
        let target = match self.package.ssed_target_for_index_pointer(row.body)? {
            Ok(target) => target,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return Ok(true);
            }
        };
        if !self.seen_targets.insert(target.as_str().to_owned()) {
            return Ok(true);
        }
        if self.matched_count < self.offset {
            self.matched_count = self.matched_count.saturating_add(1);
            return Ok(true);
        }
        let title = self.package.ssed_display_text_for_index_row(&row);
        let label = self.package.ssed_rich_label(&title);
        self.hits.push(SearchHit {
            book_id: self.package.book_id_for_hit(),
            target,
            title_html: label.html,
            title_text: label.text,
            snippet_html: None,
            diagnostics: label.diagnostics,
        });
        self.matched_count = self.matched_count.saturating_add(1);
        Ok(self.hits.len() < self.page_limit)
    }

    pub(super) fn has_hits(&self) -> bool {
        !self.hits.is_empty()
    }

    pub(super) fn extend_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) {
        self.diagnostics.extend(diagnostics);
    }

    pub(super) fn into_search_page(mut self, limit: usize) -> SearchPage {
        let next_cursor = (self.hits.len() > limit).then(|| (self.offset + limit).to_string());
        self.hits.truncate(limit);
        SearchPage {
            hits: self.hits,
            next_cursor,
            diagnostics: self.diagnostics,
        }
    }
}

pub(super) fn ssed_index_component_name_is_backward(component: &str) -> bool {
    component.to_ascii_uppercase().starts_with('B')
}

pub(super) fn ssed_index_row_match_text(row: &SsedIndexRow) -> String {
    let key = normalize_search_match_text(&row.key);
    if ssed_index_component_name_is_backward(&row.component) {
        reverse_search_match_text(&key)
    } else {
        key
    }
}

pub(super) fn ssed_fulltext_body_window_len(rows: &[SsedFulltextRow], index: usize) -> usize {
    let Some(row) = rows.get(index) else {
        return SSED_FULLTEXT_BODY_WINDOW_BYTES;
    };
    rows[index + 1..]
        .iter()
        .find_map(|next| {
            next.offset
                .checked_sub(row.offset)
                .filter(|length| *length > 0)
        })
        .and_then(|length| usize::try_from(length).ok())
        .map(|length| length.min(SSED_FULLTEXT_BODY_WINDOW_BYTES))
        .unwrap_or(SSED_FULLTEXT_BODY_WINDOW_BYTES)
}

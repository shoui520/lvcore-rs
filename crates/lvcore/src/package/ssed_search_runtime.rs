use std::collections::HashSet;

use super::drivers::ReaderBookPackage;
use super::ssed_search::{normalize_search_match_text, reverse_search_match_text};
use crate::diagnostics::Diagnostic;
use crate::error::Result;
use crate::gaiji::GaijiPolicy;
use crate::search::{SearchHit, SearchMode, SearchPage};
use crate::ssed_index::{SsedIndexPointer, SsedIndexRow};

pub(super) const SSED_FULLTEXT_BODY_WINDOW_BYTES: usize = 16 * 1024;
pub(super) const SSED_FULLTEXT_SCAN_WINDOW_BYTES: usize = 256 * 1024;
pub(super) const SSED_FULLTEXT_SCAN_OVERLAP_BYTES: usize = 512;

#[derive(Debug, Clone)]
pub(super) struct SsedFulltextRow {
    pub(super) offset: u64,
    pub(super) body: SsedIndexPointer,
    pub(super) title: SsedIndexPointer,
    pub(super) key: String,
}

#[derive(Debug, Default)]
pub(super) struct SsedNearKeyScanResult {
    pub(super) scanned_components: usize,
    pub(super) needs_prefilter_fallback: bool,
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
    pending_row: Option<SsedIndexRow>,
    gaiji_policy: GaijiPolicy,
}

impl<'a> SsedIndexSearchCollector<'a> {
    pub(super) fn new(
        package: &'a ReaderBookPackage,
        mode: &'a SearchMode,
        needle: &'a str,
        offset: usize,
        page_limit: usize,
        gaiji_policy: GaijiPolicy,
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
            pending_row: None,
            gaiji_policy,
        }
    }

    pub(super) fn push_row(&mut self, row: SsedIndexRow) -> Result<bool> {
        if let Some(pending) = self.pending_row.take() {
            if pending.body == row.body {
                self.pending_row = Some(pending);
                return Ok(true);
            }
            self.emit_hit(pending)?;
            if self.hits.len() >= self.page_limit {
                return Ok(false);
            }
        }
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
        if self
            .package
            .ssed_index_row_body_pointer_is_outside_catalog_range(&row)
        {
            return Ok(true);
        }
        let body_key = ssed_index_body_key(row.body);
        if !self.seen_targets.insert(body_key) {
            return Ok(true);
        }
        if self.matched_count < self.offset {
            self.matched_count = self.matched_count.saturating_add(1);
            return Ok(true);
        }
        self.pending_row = Some(row);
        self.matched_count = self.matched_count.saturating_add(1);
        Ok(true)
    }

    fn emit_hit(&mut self, row: SsedIndexRow) -> Result<()> {
        let target = match self.package.ssed_target_for_search_index_row(&row)? {
            Ok(target) => target,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return Ok(());
            }
        };
        let title = self.package.ssed_display_text_for_index_row(&row);
        let label = self
            .package
            .ssed_rich_label_with_policy(&title, &self.gaiji_policy);
        let href = target.href();
        self.hits.push(SearchHit {
            href,
            book_id: self.package.book_id_for_hit(),
            target,
            title_html: label.html,
            title_text: label.text,
            snippet_html: None,
            sequence_hint: None,
            diagnostics: label.diagnostics,
        });
        Ok(())
    }

    pub(super) fn has_hits(&self) -> bool {
        !self.hits.is_empty() || self.pending_row.is_some()
    }

    pub(super) fn needs_more_hits(&self) -> bool {
        self.hits.len() + usize::from(self.pending_row.is_some()) < self.page_limit
    }

    pub(super) fn extend_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) {
        self.diagnostics.extend(diagnostics);
    }

    pub(super) fn into_search_page(mut self, limit: usize) -> SearchPage {
        if let Some(row) = self.pending_row.take() {
            match self.package.ssed_target_for_search_index_row(&row) {
                Ok(Ok(target)) => {
                    let title = self.package.ssed_display_text_for_index_row(&row);
                    let label = self
                        .package
                        .ssed_rich_label_with_policy(&title, &self.gaiji_policy);
                    let href = target.href();
                    self.hits.push(SearchHit {
                        href,
                        book_id: self.package.book_id_for_hit(),
                        target,
                        title_html: label.html,
                        title_text: label.text,
                        snippet_html: None,
                        sequence_hint: None,
                        diagnostics: label.diagnostics,
                    });
                }
                Ok(Err(diagnostic)) => self.diagnostics.push(diagnostic),
                Err(error) => self.diagnostics.push(Diagnostic::warning(
                    "ssed_search_target_encode_failed",
                    format!("failed to encode SSED search target: {error}"),
                )),
            }
        }
        let next_cursor = (self.hits.len() > limit).then(|| (self.offset + limit).to_string());
        self.hits.truncate(limit);
        SearchPage {
            hits: self.hits,
            next_cursor,
            result_sequence: None,
            diagnostics: self.diagnostics,
        }
    }
}

fn ssed_index_body_key(pointer: SsedIndexPointer) -> String {
    format!("{:08x}:{:04x}", pointer.block, pointer.offset)
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

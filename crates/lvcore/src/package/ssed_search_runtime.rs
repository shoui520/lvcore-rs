use std::collections::HashSet;

use super::drivers::ReaderBookPackage;
use super::ssed_search::{
    normalize_search_match_text, reverse_search_match_text, ssed_display_label_match_texts,
};
use crate::diagnostics::Diagnostic;
use crate::error::Result;
use crate::gaiji::GaijiPolicy;
use crate::search::{SearchHit, SearchMode, SearchPage};
use crate::ssed_index::{SsedIndexPointer, SsedIndexRow};

pub(super) const SSED_FULLTEXT_BODY_WINDOW_BYTES: usize = 16 * 1024;
pub(super) const SSED_FULLTEXT_SCAN_WINDOW_BYTES: usize = 1024 * 1024;
pub(super) const SSED_FULLTEXT_SCAN_OVERLAP_BYTES: usize = 512;
pub(super) const SSED_PARTIAL_INDEX_SCAN_LEAF_PAGE_BUDGET: usize = 8;
pub(super) const SSED_PARTIAL_INDEX_PREFILTERED_LEAF_PAGE_BUDGET: usize = 1024;
pub(super) const SSED_FULLTEXT_TITLE_CURSOR_PREFILTERED_LEAF_PAGE_BUDGET: usize = 128;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SsedPartialIndexScanCursor {
    pub(super) component_index: u8,
    pub(super) page_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SsedPrefilteredIndexScanCursor {
    pub(super) component_index: u8,
    pub(super) page_index: usize,
}

#[derive(Debug, Default)]
pub(super) struct SsedPartialIndexScanResult {
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) next_cursor: Option<String>,
}

#[derive(Debug, Default)]
pub(super) struct SsedPrefilteredIndexScanResult {
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) next_cursor: Option<String>,
}

const SSED_PARTIAL_INDEX_SCAN_CURSOR_PREFIX: &str = "ssed-partial-index:";
const SSED_PREFILTERED_INDEX_SCAN_CURSOR_PREFIX: &str = "ssed-prefiltered-index:";

pub(super) fn decode_ssed_partial_index_scan_cursor(
    cursor: Option<&str>,
) -> Option<SsedPartialIndexScanCursor> {
    let cursor = cursor?.strip_prefix(SSED_PARTIAL_INDEX_SCAN_CURSOR_PREFIX)?;
    let (component_index, page_index) = cursor.split_once(':')?;
    Some(SsedPartialIndexScanCursor {
        component_index: component_index.parse().ok()?,
        page_index: page_index.parse().ok()?,
    })
}

pub(super) fn encode_ssed_partial_index_scan_cursor(cursor: SsedPartialIndexScanCursor) -> String {
    format!(
        "{SSED_PARTIAL_INDEX_SCAN_CURSOR_PREFIX}{}:{}",
        cursor.component_index, cursor.page_index
    )
}

pub(super) fn decode_ssed_prefiltered_index_scan_cursor(
    cursor: Option<&str>,
) -> Option<SsedPrefilteredIndexScanCursor> {
    let cursor = cursor?.strip_prefix(SSED_PREFILTERED_INDEX_SCAN_CURSOR_PREFIX)?;
    let (component_index, page_index) = cursor.split_once(':')?;
    Some(SsedPrefilteredIndexScanCursor {
        component_index: component_index.parse().ok()?,
        page_index: page_index.parse().ok()?,
    })
}

pub(super) fn encode_ssed_prefiltered_index_scan_cursor(
    cursor: SsedPrefilteredIndexScanCursor,
) -> String {
    format!(
        "{SSED_PREFILTERED_INDEX_SCAN_CURSOR_PREFIX}{}:{}",
        cursor.component_index, cursor.page_index
    )
}

pub(super) struct SsedIndexSearchCollector<'a> {
    package: &'a ReaderBookPackage,
    mode: &'a SearchMode,
    needle: &'a str,
    offset: usize,
    page_limit: usize,
    matched_count: usize,
    hits: Vec<SearchHit>,
    hit_target_keys: Vec<String>,
    diagnostics: Vec<Diagnostic>,
    seen_targets: HashSet<String>,
    offset_seen_targets: HashSet<String>,
    pending_row: Option<SsedIndexRow>,
    gaiji_policy: GaijiPolicy,
    match_display_label: bool,
    skip_forward_prefix_matches: bool,
    stop_on_pending_page_limit: bool,
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
            hit_target_keys: Vec::new(),
            diagnostics: Vec::new(),
            seen_targets: HashSet::new(),
            offset_seen_targets: HashSet::new(),
            pending_row: None,
            gaiji_policy,
            match_display_label: false,
            skip_forward_prefix_matches: false,
            stop_on_pending_page_limit: false,
        }
    }

    pub(super) fn with_seen_targets(mut self, seen_targets: HashSet<String>) -> Self {
        self.offset_seen_targets = seen_targets.clone();
        self.seen_targets = seen_targets;
        self
    }

    pub(super) fn with_display_label_matching(mut self) -> Self {
        self.match_display_label = true;
        self
    }

    pub(super) fn with_forward_prefix_skip(mut self) -> Self {
        self.skip_forward_prefix_matches = true;
        self
    }

    pub(super) fn with_pending_page_limit_stop(mut self) -> Self {
        self.stop_on_pending_page_limit = true;
        self
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
        let body_key = ssed_index_body_key(row.body);
        if self.seen_targets.contains(&body_key) && !self.offset_seen_targets.contains(&body_key) {
            return Ok(true);
        }
        let row_matches = self.row_matches(&row);
        if !row_matches {
            return Ok(true);
        }
        if self
            .package
            .ssed_index_row_body_pointer_is_outside_catalog_range(&row)
        {
            return Ok(true);
        }
        let already_seen = !self.seen_targets.insert(body_key.clone());
        let count_for_offset = !already_seen || self.offset_seen_targets.remove(&body_key);
        if self.matched_count < self.offset {
            if count_for_offset {
                self.matched_count = self.matched_count.saturating_add(1);
            }
            return Ok(true);
        }
        if already_seen {
            return Ok(true);
        }
        self.pending_row = Some(row);
        self.matched_count = self.matched_count.saturating_add(1);
        if self.stop_on_pending_page_limit && !self.needs_more_hits() {
            return Ok(false);
        }
        Ok(true)
    }

    fn row_matches(&self, row: &SsedIndexRow) -> bool {
        let key = ssed_index_row_match_text(row);
        if self.skip_forward_prefix_matches
            && search_match_satisfied(&SearchMode::Forward, &key, self.needle)
        {
            return false;
        }
        if search_match_satisfied(self.mode, &key, self.needle) {
            if self.mode == &SearchMode::Exact
                && self.match_display_label
                && ssed_index_component_name_is_cross_reference(&row.component)
            {
                return self.exact_display_label_matches(row);
            }
            return true;
        }
        if !self.match_display_label {
            return false;
        }
        let display = self.package.ssed_display_text_for_index_row(row);
        let display_keys = self.display_label_match_texts(row, &display);
        if display_keys.is_empty() {
            return false;
        }
        if self.skip_forward_prefix_matches
            && display_keys.iter().any(|display_key| {
                search_match_satisfied(&SearchMode::Forward, display_key, self.needle)
            })
        {
            return false;
        }
        display_keys
            .iter()
            .any(|display_key| search_match_satisfied(self.mode, display_key, self.needle))
    }

    fn exact_display_label_matches(&self, row: &SsedIndexRow) -> bool {
        if matches!(
            self.package
                .ssed_index_row_points_to_dense_sidecar_anchor(row),
            Ok(true)
        ) {
            return true;
        }
        let display = self.package.ssed_display_text_for_index_row(row);
        if display == row.key {
            return true;
        }
        let display_keys = self.display_label_match_texts(row, &display);
        !display_keys.is_empty()
            && display_keys
                .iter()
                .any(|display_key| search_match_satisfied(self.mode, display_key, self.needle))
    }

    fn display_label_match_texts(&self, row: &SsedIndexRow, display: &str) -> Vec<String> {
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

    fn emit_hit(&mut self, row: SsedIndexRow) -> Result<()> {
        let body_key = ssed_index_body_key(row.body);
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
        self.hit_target_keys.push(body_key);
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

    pub(super) fn into_search_page(self, limit: usize) -> SearchPage {
        self.into_search_page_with_hit_target_keys(limit).0
    }

    pub(super) fn into_search_page_with_hit_target_keys(
        mut self,
        limit: usize,
    ) -> (SearchPage, Vec<String>) {
        if let Some(row) = self.pending_row.take() {
            let body_key = ssed_index_body_key(row.body);
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
                    self.hit_target_keys.push(body_key);
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
        self.hit_target_keys.truncate(limit);
        (
            SearchPage {
                hits: self.hits,
                next_cursor,
                result_sequence: None,
                diagnostics: self.diagnostics,
            },
            self.hit_target_keys,
        )
    }
}

fn search_match_satisfied(mode: &SearchMode, key: &str, needle: &str) -> bool {
    match mode {
        SearchMode::Exact => key == needle,
        SearchMode::Forward => key.starts_with(needle),
        SearchMode::Backward => key.ends_with(needle),
        SearchMode::Partial => key.contains(needle),
        SearchMode::FullText | SearchMode::Advanced(_) => false,
    }
}

fn ssed_index_body_key(pointer: SsedIndexPointer) -> String {
    format!("{:08x}:{:04x}", pointer.block, pointer.offset)
}

pub(super) fn ssed_index_component_name_is_backward(component: &str) -> bool {
    component.to_ascii_uppercase().starts_with('B')
}

pub(super) fn ssed_index_component_name_is_cross_reference(component: &str) -> bool {
    component.to_ascii_uppercase().starts_with("CR")
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

use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::error::Result;
use crate::gaiji::GaijiPolicy;
use crate::package::BookId;
use crate::sequence::{SEARCH_RESULT_SEQUENCE_MAX_TARGETS, SearchResultSequence, SequenceHint};
use crate::target::TargetToken;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    Exact,
    Forward,
    Backward,
    Partial,
    FullText,
    Advanced(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchScope {
    CurrentBook { book_id: BookId },
    SelectedBooks { book_ids: Vec<BookId> },
    AllBooks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    pub scope: SearchScope,
    pub mode: SearchMode,
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gaiji_policy: Option<GaijiPolicy>,
}

impl SearchQuery {
    pub fn label_gaiji_policy(&self) -> GaijiPolicy {
        self.gaiji_policy.clone().unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    pub book_id: BookId,
    pub target: TargetToken,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub href: String,
    pub title_html: String,
    pub title_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet_html: Option<String>,
    /// Page-owned continuous-view sequence hint for this hit.
    ///
    /// This lets frontend search result lists preserve per-page continuous-view
    /// context after pagination without reconstructing search result sequences
    /// from target internals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence_hint: Option<SequenceHint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPage {
    pub hits: Vec<SearchHit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Opaque search-result order value for continuous view.
    ///
    /// Frontends pass this back as `SequenceHint::SearchResults { value }`
    /// when the user opens a hit and wants surrounding search results shown in
    /// the same entry body view. Individual package providers may leave this
    /// empty; the library boundary populates it after book-scoped hrefs are
    /// applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_sequence: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

impl SearchPage {
    pub fn deferred(message: impl Into<String>) -> Self {
        Self {
            hits: Vec::new(),
            next_cursor: None,
            result_sequence: None,
            diagnostics: vec![Diagnostic::info("search_deferred", message)],
        }
    }

    pub fn attach_result_sequence(&mut self) -> Result<()> {
        self.diagnostics
            .retain(|diagnostic| diagnostic.code != "search_result_sequence_omitted");
        if self.hits.is_empty() {
            self.result_sequence = None;
            return Ok(());
        }
        if self.hits.len() > SEARCH_RESULT_SEQUENCE_MAX_TARGETS {
            self.result_sequence = None;
            for hit in &mut self.hits {
                hit.sequence_hint = None;
            }
            self.diagnostics.push(
                Diagnostic::info(
                    "search_result_sequence_omitted",
                    "search result page is larger than the maximum continuous-view sequence payload",
                )
                .with_context("hit_count", self.hits.len().to_string())
                .with_context(
                    "max_targets",
                    SEARCH_RESULT_SEQUENCE_MAX_TARGETS.to_string(),
                ),
            );
            return Ok(());
        }

        let value = SearchResultSequence::from_search_page(self)?.encode()?;
        let hint = SequenceHint::SearchResults {
            value: value.clone(),
        };
        for hit in &mut self.hits {
            hit.sequence_hint = Some(hint.clone());
        }
        self.result_sequence = Some(value);
        Ok(())
    }
}

pub trait SearchProvider: Send + Sync {
    fn search(&self, query: &SearchQuery) -> Result<SearchPage>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advanced_search_mode_has_stable_public_json_shape() {
        let mode = SearchMode::Advanced("advanced1".to_owned());
        let json = serde_json::to_value(&mode).unwrap();
        assert_eq!(json, serde_json::json!({ "advanced": "advanced1" }));
        assert_eq!(serde_json::from_value::<SearchMode>(json).unwrap(), mode);
    }

    #[test]
    fn search_scope_has_frontend_safe_tagged_json_shape() {
        let current = SearchScope::CurrentBook {
            book_id: BookId("SSED:KOJIEN7".to_owned()),
        };
        let selected = SearchScope::SelectedBooks {
            book_ids: vec![
                BookId("SSED:KOJIEN7".to_owned()),
                BookId("LVED_SQLITE3:DAIJIRN4".to_owned()),
            ],
        };

        assert_eq!(
            serde_json::to_value(&current).unwrap(),
            serde_json::json!({ "kind": "current_book", "book_id": "SSED:KOJIEN7" })
        );
        assert_eq!(
            serde_json::to_value(&selected).unwrap(),
            serde_json::json!({
                "kind": "selected_books",
                "book_ids": ["SSED:KOJIEN7", "LVED_SQLITE3:DAIJIRN4"]
            })
        );
        assert_eq!(
            serde_json::to_value(SearchScope::AllBooks).unwrap(),
            serde_json::json!({ "kind": "all_books" })
        );
        assert_eq!(
            serde_json::from_value::<SearchScope>(serde_json::json!({
                "kind": "current_book",
                "book_id": "SSED:KOJIEN7"
            }))
            .unwrap(),
            current
        );
    }
}

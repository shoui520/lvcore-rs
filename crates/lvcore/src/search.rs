use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::error::Result;
use crate::package::BookId;
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
#[serde(rename_all = "snake_case")]
pub enum SearchScope {
    CurrentBook(BookId),
    SelectedBooks(Vec<BookId>),
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    pub book_id: BookId,
    pub target: TargetToken,
    pub title_html: String,
    pub title_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet_html: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPage {
    pub hits: Vec<SearchHit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

impl SearchPage {
    pub fn deferred(message: impl Into<String>) -> Self {
        Self {
            hits: Vec::new(),
            next_cursor: None,
            diagnostics: vec![Diagnostic::info("search_deferred", message)],
        }
    }
}

pub trait SearchProvider: Send + Sync {
    fn search(&self, query: &SearchQuery) -> Result<SearchPage>;
}

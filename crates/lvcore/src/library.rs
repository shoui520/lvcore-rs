use std::collections::BTreeMap;
use std::path::Path;

use crate::diagnostics::Diagnostic;
use crate::error::{Error, Result};
use crate::package::{BookId, BookMetadata, BookPackage, DriverRegistry};
use crate::search::{SearchPage, SearchQuery, SearchScope};

#[derive(Default)]
pub struct BookLibrary {
    books: BTreeMap<BookId, Box<dyn BookPackage>>,
}

impl BookLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_path(
        &mut self,
        path: impl AsRef<Path>,
        registry: &DriverRegistry,
    ) -> Result<BookId> {
        let package = registry.open_best(path.as_ref())?;
        let book_id = package.metadata().book_id.clone();
        self.insert(package);
        Ok(book_id)
    }

    pub fn insert(&mut self, package: Box<dyn BookPackage>) {
        let book_id = package.metadata().book_id.clone();
        self.books.insert(book_id, package);
    }

    pub fn is_empty(&self) -> bool {
        self.books.is_empty()
    }

    pub fn len(&self) -> usize {
        self.books.len()
    }

    pub fn metadata(&self) -> Vec<&BookMetadata> {
        self.books.values().map(|book| book.metadata()).collect()
    }

    pub fn book(&self, book_id: &BookId) -> Option<&dyn BookPackage> {
        self.books.get(book_id).map(Box::as_ref)
    }

    pub fn search(&self, query: &SearchQuery) -> Result<SearchPage> {
        match &query.scope {
            SearchScope::CurrentBook(book_id) => {
                let book = self
                    .book(book_id)
                    .ok_or_else(|| Error::BookNotFound(book_id.0.clone()))?;
                book.search(query)
            }
            SearchScope::SelectedBooks(book_ids) => self.search_many(book_ids.iter(), query),
            SearchScope::AllBooks => self.search_many(self.books.keys(), query),
        }
    }

    fn search_many<'a>(
        &self,
        book_ids: impl Iterator<Item = &'a BookId>,
        query: &SearchQuery,
    ) -> Result<SearchPage> {
        let mut page = SearchPage {
            hits: Vec::new(),
            next_cursor: None,
            diagnostics: Vec::new(),
        };

        for book_id in book_ids {
            let Some(book) = self.book(book_id) else {
                page.diagnostics.push(
                    Diagnostic::warning("book_missing", format!("{} is not open", book_id.0))
                        .with_context("book_id", &book_id.0),
                );
                continue;
            };
            if page.hits.len() >= query.limit {
                break;
            }
            let mut book_query = query.clone();
            book_query.scope = SearchScope::CurrentBook(book_id.clone());
            book_query.limit = query.limit.saturating_sub(page.hits.len());
            let mut book_page = book.search(&book_query)?;
            for diagnostic in &mut book_page.diagnostics {
                diagnostic
                    .context
                    .entry("book_id".to_owned())
                    .or_insert_with(|| book_id.0.clone());
            }
            page.hits.extend(book_page.hits);
            page.diagnostics.extend(book_page.diagnostics);
            if book_page.next_cursor.is_some() {
                page.diagnostics.push(
                    Diagnostic::info(
                        "search_cursor_deferred",
                        "cross-book search cursor merging is not implemented yet",
                    )
                    .with_context("book_id", &book_id.0),
                );
            }
        }

        if page.hits.len() > query.limit {
            page.hits.truncate(query.limit);
        }
        Ok(page)
    }
}

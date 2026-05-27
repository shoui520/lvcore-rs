use std::collections::BTreeMap;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::error::{Error, Result};
use crate::navigation::{HomeSurface, NavigationSurface};
use crate::package::{BookId, BookMetadata, BookPackage, DriverRegistry};
use crate::render::{RenderOptions, RendererInput, ResolvedTargetView};
use crate::resources::{ResourceRef, ResourceToken};
use crate::search::{SearchPage, SearchQuery, SearchScope};
use crate::sequence::{SequenceHint, TargetWindow};
use crate::target::TargetToken;

#[derive(Default)]
pub struct BookLibrary {
    books: BTreeMap<BookId, Box<dyn BookPackage>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LibrarySearchCursor {
    version: u8,
    book_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    book_cursor: Option<String>,
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

    pub fn metadata_snapshot(&self) -> Vec<BookMetadata> {
        self.books
            .values()
            .map(|book| book.metadata().clone())
            .collect()
    }

    pub fn book(&self, book_id: &BookId) -> Option<&dyn BookPackage> {
        self.books.get(book_id).map(Box::as_ref)
    }

    pub fn home_surfaces(&self, book_id: &BookId) -> Result<Vec<HomeSurface>> {
        self.required_book(book_id)?.home_surfaces()
    }

    pub fn open_surface(&self, book_id: &BookId, surface_id: &str) -> Result<NavigationSurface> {
        self.required_book(book_id)?.open_surface(surface_id)
    }

    pub fn open_surface_page(
        &self,
        book_id: &BookId,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        self.required_book(book_id)?
            .open_surface_page(surface_id, cursor, limit)
    }

    pub fn render_target(
        &self,
        book_id: &BookId,
        target: &TargetToken,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        self.required_book(book_id)?.render_target(target, options)
    }

    pub fn renderer_input_for_target(
        &self,
        book_id: &BookId,
        target: &TargetToken,
    ) -> Result<RendererInput> {
        self.required_book(book_id)?
            .renderer_input_for_target(target)
    }

    pub fn resolve_target_window(
        &self,
        book_id: &BookId,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<TargetWindow> {
        self.required_book(book_id)?.resolve_target_window(
            target,
            sequence_hint,
            before,
            after,
            options,
        )
    }

    pub fn resolve_resource(
        &self,
        book_id: &BookId,
        resource: &ResourceToken,
    ) -> Result<ResourceRef> {
        self.required_book(book_id)?.resolve_resource(resource)
    }

    pub fn read_resource(&self, book_id: &BookId, resource: &ResourceToken) -> Result<Vec<u8>> {
        self.required_book(book_id)?.read_resource(resource)
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

    fn required_book(&self, book_id: &BookId) -> Result<&dyn BookPackage> {
        self.book(book_id)
            .ok_or_else(|| Error::BookNotFound(book_id.0.clone()))
    }

    fn search_many<'a>(
        &self,
        book_ids: impl Iterator<Item = &'a BookId>,
        query: &SearchQuery,
    ) -> Result<SearchPage> {
        if query.limit == 0 {
            return Ok(SearchPage {
                hits: Vec::new(),
                next_cursor: None,
                diagnostics: Vec::new(),
            });
        }
        let ordered_book_ids = book_ids.collect::<Vec<_>>();
        let (start_index, mut inner_cursor, cursor_diagnostic) =
            decode_library_search_cursor(query.cursor.as_deref());
        let mut page = SearchPage {
            hits: Vec::new(),
            next_cursor: None,
            diagnostics: Vec::new(),
        };
        if let Some(diagnostic) = cursor_diagnostic {
            page.diagnostics.push(diagnostic);
        }

        for (book_index, book_id) in ordered_book_ids.iter().enumerate().skip(start_index) {
            if page.hits.len() >= query.limit {
                page.next_cursor = Some(encode_library_search_cursor(book_index, None));
                break;
            }
            let Some(book) = self.book(book_id) else {
                page.diagnostics.push(
                    Diagnostic::warning("book_missing", format!("{} is not open", book_id.0))
                        .with_context("book_id", &book_id.0),
                );
                continue;
            };
            let mut book_query = query.clone();
            book_query.scope = SearchScope::CurrentBook((*book_id).clone());
            book_query.cursor = if book_index == start_index {
                inner_cursor.take()
            } else {
                None
            };
            book_query.limit = query.limit.saturating_sub(page.hits.len());
            let mut book_page = book.search(&book_query)?;
            for diagnostic in &mut book_page.diagnostics {
                diagnostic
                    .context
                    .entry("book_id".to_owned())
                    .or_insert_with(|| book_id.0.clone());
            }
            let next_book_cursor = book_page.next_cursor.take();
            page.hits.extend(book_page.hits);
            page.diagnostics.extend(book_page.diagnostics);
            if let Some(book_cursor) = next_book_cursor {
                page.next_cursor =
                    Some(encode_library_search_cursor(book_index, Some(book_cursor)));
                break;
            }
        }

        if page.hits.len() > query.limit {
            page.hits.truncate(query.limit);
        }
        Ok(page)
    }
}

fn encode_library_search_cursor(book_index: usize, book_cursor: Option<String>) -> String {
    let cursor = LibrarySearchCursor {
        version: 1,
        book_index,
        book_cursor,
    };
    let bytes = serde_json::to_vec(&cursor).unwrap_or_default();
    URL_SAFE_NO_PAD.encode(bytes)
}

fn decode_library_search_cursor(
    cursor: Option<&str>,
) -> (usize, Option<String>, Option<Diagnostic>) {
    let Some(cursor) = cursor else {
        return (0, None, None);
    };
    let decoded = URL_SAFE_NO_PAD
        .decode(cursor)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LibrarySearchCursor>(&bytes).ok());
    match decoded {
        Some(cursor) if cursor.version == 1 => (cursor.book_index, cursor.book_cursor, None),
        _ => (
            0,
            None,
            Some(Diagnostic::warning(
                "invalid_search_cursor",
                "library search cursor could not be decoded; search restarted from the first book",
            )),
        ),
    }
}

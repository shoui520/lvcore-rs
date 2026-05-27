use std::collections::BTreeMap;
use std::path::Path;

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

    pub fn home_surfaces(&self, book_id: &BookId) -> Result<Vec<HomeSurface>> {
        self.required_book(book_id)?.home_surfaces()
    }

    pub fn open_surface(&self, book_id: &BookId, surface_id: &str) -> Result<NavigationSurface> {
        self.required_book(book_id)?.open_surface(surface_id)
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

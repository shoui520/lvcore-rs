use std::collections::BTreeMap;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::error::{Error, Result};
use crate::navigation::{HomeSurface, NavigationSurface};
use crate::package::{BookAliasKind, BookId, BookMetadata, BookPackage, DriverRegistry};
use crate::render::{RenderOptions, RendererInput, ResolvedTargetKind, ResolvedTargetView};
use crate::resources::{ResourceRef, ResourceToken};
use crate::search::{SearchPage, SearchQuery, SearchScope};
use crate::sequence::{SequenceHint, TargetWindow};
use crate::target::{InternalTarget, TargetToken};

#[derive(Default)]
pub struct BookLibrary {
    books: BTreeMap<BookId, Box<dyn BookPackage>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutedTargetView {
    pub book_id: BookId,
    pub view: ResolvedTargetView,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutedTargetWindow {
    pub book_id: BookId,
    pub window: TargetWindow,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LibrarySearchCursor {
    version: u8,
    book_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    book_cursor: Option<String>,
}

struct LvedCrossBookRequest<'a> {
    source_book_id: &'a BookId,
    source_book: &'a dyn BookPackage,
    original_target: &'a TargetToken,
    dict_code: &'a str,
    content_id: &'a str,
    anchor: Option<String>,
    options: &'a RenderOptions,
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

    /// Resolve a target that may leave the source book, such as LVED
    /// cross-dictionary links. Local targets are delegated to the source book.
    pub fn render_target_routed(
        &self,
        book_id: &BookId,
        target: &TargetToken,
        options: &RenderOptions,
    ) -> Result<RoutedTargetView> {
        let source_book = self.required_book(book_id)?;
        match target.decode()? {
            InternalTarget::LvedCrossBook {
                dict_code,
                content_id,
                anchor,
                ..
            } => self.render_lved_cross_book_target(LvedCrossBookRequest {
                source_book_id: book_id,
                source_book,
                original_target: target,
                dict_code: &dict_code,
                content_id: &content_id,
                anchor,
                options,
            }),
            _ => {
                let mut view = source_book.render_target(target, options)?;
                scope_view_resource_hrefs(book_id, &mut view);
                Ok(RoutedTargetView {
                    book_id: book_id.clone(),
                    view,
                    diagnostics: Vec::new(),
                })
            }
        }
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

    /// Resolve a continuous-view window for a target that may route to another
    /// loaded book before sequencing.
    pub fn resolve_target_window_routed(
        &self,
        book_id: &BookId,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<RoutedTargetWindow> {
        let routed = self.render_target_routed(book_id, target, options)?;
        if &routed.book_id == book_id {
            if routed.view.kind == ResolvedTargetKind::Unsupported {
                return Ok(RoutedTargetWindow {
                    book_id: routed.book_id,
                    window: TargetWindow {
                        center: routed.view,
                        before: Vec::new(),
                        after: Vec::new(),
                        diagnostics: routed.diagnostics.clone(),
                    },
                    diagnostics: routed.diagnostics,
                });
            }
            return Ok(RoutedTargetWindow {
                book_id: book_id.clone(),
                window: scope_target_window_resource_hrefs(
                    book_id,
                    self.required_book(book_id)?.resolve_target_window(
                        target,
                        sequence_hint,
                        before,
                        after,
                        options,
                    )?,
                ),
                diagnostics: routed.diagnostics,
            });
        }

        let destination_target = routed.view.target.clone();
        let mut window = self.required_book(&routed.book_id)?.resolve_target_window(
            &destination_target,
            sequence_hint,
            before,
            after,
            options,
        )?;
        window.diagnostics.extend(routed.diagnostics.clone());
        window = scope_target_window_resource_hrefs(&routed.book_id, window);
        Ok(RoutedTargetWindow {
            book_id: routed.book_id,
            window,
            diagnostics: routed.diagnostics,
        })
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

    pub fn read_scoped_resource_href(&self, href: &str) -> Result<Vec<u8>> {
        let (book_id, resource) = parse_scoped_resource_href(href)?;
        self.read_resource(&book_id, &resource)
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

    fn render_lved_cross_book_target(
        &self,
        request: LvedCrossBookRequest<'_>,
    ) -> Result<RoutedTargetView> {
        let row_id = match request.content_id.parse::<i64>() {
            Ok(value) => value,
            Err(_) => {
                let diagnostic = Diagnostic::warning(
                    "lved_cross_book_content_id_invalid",
                    format!(
                        "LVED cross-book content id {:?} is not numeric",
                        request.content_id
                    ),
                )
                .with_context("dict_code", request.dict_code)
                .with_context("content_id", request.content_id);
                let mut view = request
                    .source_book
                    .render_target(request.original_target, request.options)?;
                view.diagnostics.push(diagnostic.clone());
                scope_view_resource_hrefs(request.source_book_id, &mut view);
                return Ok(RoutedTargetView {
                    book_id: request.source_book_id.clone(),
                    view,
                    diagnostics: vec![diagnostic],
                });
            }
        };

        let Some((destination_book_id, destination_book)) =
            self.find_lved_dict_code_alias(request.dict_code)
        else {
            let diagnostic = Diagnostic::info(
                "lved_cross_book_destination_missing",
                format!(
                    "LVED cross-book destination dictionary {} is not open in the library",
                    request.dict_code
                ),
            )
            .with_context("dict_code", request.dict_code)
            .with_context("source_book_id", &request.source_book_id.0);
            let mut view = request
                .source_book
                .render_target(request.original_target, request.options)?;
            view.diagnostics.push(diagnostic.clone());
            scope_view_resource_hrefs(request.source_book_id, &mut view);
            return Ok(RoutedTargetView {
                book_id: request.source_book_id.clone(),
                view,
                diagnostics: vec![diagnostic],
            });
        };

        let destination_target = TargetToken::new(&InternalTarget::LvedRow {
            table: "content".to_owned(),
            row_id,
            anchor: request.anchor,
        })?;
        let mut view = destination_book.render_target(&destination_target, request.options)?;
        scope_view_resource_hrefs(destination_book_id, &mut view);
        Ok(RoutedTargetView {
            book_id: destination_book_id.clone(),
            view,
            diagnostics: vec![
                Diagnostic::info(
                    "lved_cross_book_routed",
                    format!(
                        "LVED cross-book target routed to dictionary {}",
                        request.dict_code
                    ),
                )
                .with_context("dict_code", request.dict_code)
                .with_context("source_book_id", &request.source_book_id.0)
                .with_context("destination_book_id", &destination_book_id.0),
            ],
        })
    }

    fn find_lved_dict_code_alias(&self, dict_code: &str) -> Option<(&BookId, &dyn BookPackage)> {
        self.books.iter().find_map(|(book_id, book)| {
            book.routing_aliases()
                .iter()
                .any(|alias| {
                    alias.kind == BookAliasKind::LvedDictCode
                        && alias.value.eq_ignore_ascii_case(dict_code)
                })
                .then_some((book_id, book.as_ref()))
        })
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

fn scope_target_window_resource_hrefs(book_id: &BookId, mut window: TargetWindow) -> TargetWindow {
    scope_view_resource_hrefs(book_id, &mut window.center);
    for view in &mut window.before {
        scope_view_resource_hrefs(book_id, view);
    }
    for view in &mut window.after {
        scope_view_resource_hrefs(book_id, view);
    }
    window
}

fn scope_view_resource_hrefs(book_id: &BookId, view: &mut ResolvedTargetView) {
    let Some(display_html) = &mut view.display_html else {
        for resource in &mut view.resources {
            scope_resource_ref_href(book_id, resource);
        }
        return;
    };
    for resource in &mut view.resources {
        let old_href = resource.href.clone();
        scope_resource_ref_href(book_id, resource);
        if let (Some(old_href), Some(new_href)) = (old_href, resource.href.as_ref()) {
            *display_html = display_html.replace(&old_href, new_href);
        }
    }
}

fn scope_resource_ref_href(book_id: &BookId, resource: &mut ResourceRef) {
    if resource.href.is_some() {
        resource.href = Some(scoped_resource_href(book_id, &resource.token));
    }
}

fn scoped_resource_href(book_id: &BookId, token: &ResourceToken) -> String {
    format!(
        "lvcore://resource/{}/{}",
        URL_SAFE_NO_PAD.encode(book_id.0.as_bytes()),
        token.as_str()
    )
}

fn parse_scoped_resource_href(href: &str) -> Result<(BookId, ResourceToken)> {
    let Some(rest) = href.strip_prefix("lvcore://resource/") else {
        return Err(Error::InvalidResourceHref);
    };
    let mut parts = rest.split('/');
    let Some(book_scope) = parts.next().filter(|value| !value.is_empty()) else {
        return Err(Error::InvalidResourceHref);
    };
    let Some(resource_token) = parts.next().filter(|value| !value.is_empty()) else {
        return Err(Error::InvalidResourceHref);
    };
    if parts.next().is_some() {
        return Err(Error::InvalidResourceHref);
    }
    let book_id_bytes = URL_SAFE_NO_PAD
        .decode(book_scope)
        .map_err(|_| Error::InvalidResourceHref)?;
    let book_id = String::from_utf8(book_id_bytes).map_err(|_| Error::InvalidResourceHref)?;
    Ok((BookId(book_id), ResourceToken::from_opaque(resource_token)))
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

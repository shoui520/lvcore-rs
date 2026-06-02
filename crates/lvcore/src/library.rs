use std::collections::BTreeMap;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::diagnostics::Diagnostic;
use crate::error::{Error, Result};
use crate::navigation::{HomeSurface, LabelOptions, NavigationSurface};
use crate::package::{
    BookAliasKind, BookId, BookMetadata, BookPackage, DetectedPackage, DriverRegistry,
    PackageDiscoveryOptions,
};
use crate::render::{RenderOptions, RendererInput, ResolvedTargetKind, ResolvedTargetView};
use crate::resources::{ResourceRef, ResourceToken};
use crate::search::{SearchPage, SearchQuery, SearchScope};
use crate::sequence::{
    SearchResultSequence, SearchResultSequenceTarget, SequenceHint, TargetWindow,
};
use crate::target::{InternalTarget, TargetToken};

mod scope;

use scope::{
    parse_scoped_resource_href, parse_target_href, scope_home_surfaces_resource_hrefs,
    scope_navigation_surface_resource_hrefs, scope_renderer_input_resource_hrefs,
    scope_resource_ref_href, scope_search_page_resource_hrefs, scope_target_window_resource_hrefs,
    scope_view_resource_hrefs,
};

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
pub struct LibraryTargetWindow {
    pub center: RoutedTargetView,
    pub before: Vec<RoutedTargetView>,
    pub after: Vec<RoutedTargetView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryImportReport {
    pub opened: Vec<BookId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibrarySnapshot {
    pub books: Vec<BookMetadata>,
    pub book_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryImportResult {
    pub books: Vec<BookMetadata>,
    pub book_count: usize,
    pub opened_book_ids: Vec<BookId>,
    #[serde(default)]
    pub import_diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LibrarySearchCursor {
    version: u8,
    book_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    book_id: Option<BookId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    query_fingerprint: Option<String>,
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

    pub fn open_detected_package(
        &mut self,
        detected: DetectedPackage,
        registry: &DriverRegistry,
    ) -> Result<BookId> {
        let (book_id, _) = self.open_detected_package_with_status(detected, registry)?;
        Ok(book_id)
    }

    fn open_detected_package_with_status(
        &mut self,
        detected: DetectedPackage,
        registry: &DriverRegistry,
    ) -> Result<(BookId, bool)> {
        let package = registry.open_detected_package(detected)?;
        let book_id = package.metadata().book_id.clone();
        let inserted = self.insert(package);
        Ok((book_id, inserted))
    }

    pub fn open_discovered_paths(
        &mut self,
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
        registry: &DriverRegistry,
        options: PackageDiscoveryOptions,
    ) -> Result<Vec<BookId>> {
        let mut opened = Vec::new();
        for path in paths {
            let remaining = options.max.map(|max| max.saturating_sub(opened.len()));
            if remaining == Some(0) {
                break;
            }
            let packages = registry.discover_best_packages(
                path.as_ref(),
                PackageDiscoveryOptions { max: remaining },
            )?;
            for detected in packages {
                if options.max.is_some_and(|max| opened.len() >= max) {
                    break;
                }
                let (book_id, inserted) =
                    self.open_detected_package_with_status(detected, registry)?;
                if inserted {
                    opened.push(book_id);
                }
            }
        }
        Ok(opened)
    }

    pub fn try_open_discovered_paths(
        &mut self,
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
        registry: &DriverRegistry,
        options: PackageDiscoveryOptions,
    ) -> LibraryImportReport {
        let mut report = LibraryImportReport::default();
        for path in paths {
            let path = path.as_ref();
            let remaining = options
                .max
                .map(|max| max.saturating_sub(report.opened.len()));
            if remaining == Some(0) {
                break;
            }
            let packages = match registry
                .discover_best_packages(path, PackageDiscoveryOptions { max: remaining })
            {
                Ok(roots) => roots,
                Err(error) => {
                    report.diagnostics.push(
                        Diagnostic::warning(
                            "library_discovery_failed",
                            format!("package discovery failed for {}: {error}", path.display()),
                        )
                        .with_context("path", path.display().to_string()),
                    );
                    continue;
                }
            };
            for detected in packages {
                if options.max.is_some_and(|max| report.opened.len() >= max) {
                    break;
                }
                let root = detected.root.clone();
                match self.open_detected_package_with_status(detected, registry) {
                    Ok((book_id, true)) => report.opened.push(book_id),
                    Ok((book_id, false)) => {
                        let book_id_text = book_id.0.clone();
                        report.diagnostics.push(
                            Diagnostic::info(
                                "library_duplicate_book_skipped",
                                format!("duplicate book {book_id_text} was already opened"),
                            )
                            .with_context("path", root.display().to_string())
                            .with_context("book_id", book_id_text),
                        );
                    }
                    Err(error) => report.diagnostics.push(
                        Diagnostic::warning(
                            "book_open_failed",
                            format!("package open failed for {}: {error}", root.display()),
                        )
                        .with_context("path", root.display().to_string()),
                    ),
                }
            }
        }
        report
    }

    pub fn insert(&mut self, package: Box<dyn BookPackage>) -> bool {
        let book_id = package.metadata().book_id.clone();
        if self.books.contains_key(&book_id) {
            return false;
        }
        self.books.insert(book_id, package);
        true
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

    pub fn snapshot(&self) -> LibrarySnapshot {
        LibrarySnapshot {
            books: self.metadata_snapshot(),
            book_count: self.len(),
        }
    }

    pub fn import_result(&self, report: LibraryImportReport) -> LibraryImportResult {
        let snapshot = self.snapshot();
        LibraryImportResult {
            books: snapshot.books,
            book_count: snapshot.book_count,
            opened_book_ids: report.opened,
            import_diagnostics: report.diagnostics,
        }
    }

    pub fn book(&self, book_id: &BookId) -> Option<&dyn BookPackage> {
        self.books.get(book_id).map(Box::as_ref)
    }

    pub fn home_surfaces(&self, book_id: &BookId) -> Result<Vec<HomeSurface>> {
        let mut surfaces = self.required_book(book_id)?.home_surfaces()?;
        scope_home_surfaces_resource_hrefs(book_id, &mut surfaces);
        Ok(surfaces)
    }

    pub fn open_surface(&self, book_id: &BookId, surface_id: &str) -> Result<NavigationSurface> {
        self.open_surface_with_options(book_id, surface_id, &LabelOptions::default())
    }

    pub fn open_surface_with_options(
        &self,
        book_id: &BookId,
        surface_id: &str,
        options: &LabelOptions,
    ) -> Result<NavigationSurface> {
        let mut surface = self
            .required_book(book_id)?
            .open_surface_with_options(surface_id, options)?;
        scope_navigation_surface_resource_hrefs(book_id, &mut surface);
        Ok(surface)
    }

    pub fn open_surface_page(
        &self,
        book_id: &BookId,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        self.open_surface_page_with_options(
            book_id,
            surface_id,
            cursor,
            limit,
            &LabelOptions::default(),
        )
    }

    pub fn open_surface_page_with_options(
        &self,
        book_id: &BookId,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
        options: &LabelOptions,
    ) -> Result<NavigationSurface> {
        let mut surface = self
            .required_book(book_id)?
            .open_surface_page_with_options(surface_id, cursor, limit, options)?;
        scope_navigation_surface_resource_hrefs(book_id, &mut surface);
        Ok(surface)
    }

    pub fn render_target(
        &self,
        book_id: &BookId,
        target: &TargetToken,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let mut view = self
            .required_book(book_id)?
            .render_target(target, options)?;
        scope_view_resource_hrefs(book_id, &mut view);
        Ok(view)
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

    /// Parse and route a reader-owned target URL emitted in `display_html`.
    ///
    /// The source `book_id` is still required because ordinary target tokens are
    /// intentionally scoped by the current view; cross-book links then route
    /// through loaded book aliases when the target itself declares that.
    pub fn render_target_href_routed(
        &self,
        book_id: &BookId,
        href: &str,
        options: &RenderOptions,
    ) -> Result<RoutedTargetView> {
        let target = parse_target_href(href)?;
        self.render_target_routed(book_id, &target, options)
    }

    pub fn renderer_input_for_target(
        &self,
        book_id: &BookId,
        target: &TargetToken,
    ) -> Result<RendererInput> {
        let mut input = self
            .required_book(book_id)?
            .renderer_input_for_target(target)?;
        scope_renderer_input_resource_hrefs(book_id, &mut input);
        Ok(input)
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
        let window = self.required_book(book_id)?.resolve_target_window(
            target,
            sequence_hint,
            before,
            after,
            options,
        )?;
        Ok(scope_target_window_resource_hrefs(book_id, window))
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

    /// Parse and route a reader-owned target URL emitted in `display_html`, then
    /// expand the resolved target into a continuous-view window.
    pub fn resolve_target_window_href_routed(
        &self,
        book_id: &BookId,
        href: &str,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<RoutedTargetWindow> {
        let target = parse_target_href(href)?;
        self.resolve_target_window_routed(book_id, &target, sequence_hint, before, after, options)
    }

    /// Resolve a continuous-view window in search-result order across a loaded
    /// library. This is the reader-core path for 串刺し検索 result pages, where
    /// neighboring hits may belong to different books and therefore need
    /// per-view resource/link scoping.
    pub fn resolve_search_result_window_routed(
        &self,
        source_book_id: &BookId,
        target: &TargetToken,
        sequence_value: &str,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<LibraryTargetWindow> {
        let sequence = match SearchResultSequence::decode(sequence_value) {
            Ok(sequence) => sequence,
            Err(error) => {
                let diagnostic =
                    Diagnostic::warning("search_results_sequence_invalid", error.to_string());
                let mut center = self.render_target_routed(source_book_id, target, options)?;
                center.view.diagnostics.push(diagnostic.clone());
                center.diagnostics.push(diagnostic.clone());
                return Ok(LibraryTargetWindow {
                    center,
                    before: Vec::new(),
                    after: Vec::new(),
                    diagnostics: vec![diagnostic],
                });
            }
        };
        let Some(center_index) = sequence.targets.iter().position(|candidate| {
            library_search_sequence_target_matches(candidate, source_book_id, target)
        }) else {
            let diagnostic = Diagnostic::info(
                "sequence_target_not_in_search_results",
                "target is not present in the provided library search-result order",
            );
            let mut center = self.render_target_routed(source_book_id, target, options)?;
            center.view.diagnostics.push(diagnostic.clone());
            center.diagnostics.push(diagnostic.clone());
            return Ok(LibraryTargetWindow {
                center,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![diagnostic],
            });
        };

        let before_start = center_index.saturating_sub(before);
        let before_views = sequence.targets[before_start..center_index]
            .iter()
            .map(|item| self.render_search_sequence_target_routed(source_book_id, item, options))
            .collect::<Result<Vec<_>>>()?;
        let mut center = self.render_search_sequence_target_routed(
            source_book_id,
            &sequence.targets[center_index],
            options,
        )?;
        if let Some(title) = &sequence.targets[center_index].title {
            center.view.title = Some(title.clone());
        }
        let after_end = (center_index + 1 + after).min(sequence.targets.len());
        let after_views = sequence.targets[center_index + 1..after_end]
            .iter()
            .map(|item| self.render_search_sequence_target_routed(source_book_id, item, options))
            .collect::<Result<Vec<_>>>()?;

        Ok(LibraryTargetWindow {
            center,
            before: before_views,
            after: after_views,
            diagnostics: Vec::new(),
        })
    }

    pub fn resolve_resource(
        &self,
        book_id: &BookId,
        resource: &ResourceToken,
    ) -> Result<ResourceRef> {
        let mut resource = self.required_book(book_id)?.resolve_resource(resource)?;
        scope_resource_ref_href(book_id, &mut resource);
        Ok(resource)
    }

    pub fn read_resource(&self, book_id: &BookId, resource: &ResourceToken) -> Result<Vec<u8>> {
        self.required_book(book_id)?.read_resource(resource)
    }

    pub fn resolve_scoped_resource_href(&self, href: &str) -> Result<ResourceRef> {
        let (book_id, resource) = parse_scoped_resource_href(href)?;
        self.resolve_resource(&book_id, &resource)
    }

    pub fn read_scoped_resource_href(&self, href: &str) -> Result<Vec<u8>> {
        let (book_id, resource) = parse_scoped_resource_href(href)?;
        self.read_resource(&book_id, &resource)
    }

    pub fn search(&self, query: &SearchQuery) -> Result<SearchPage> {
        match &query.scope {
            SearchScope::CurrentBook { book_id } => {
                let book = self
                    .book(book_id)
                    .ok_or_else(|| Error::BookNotFound(book_id.0.clone()))?;
                let mut page = book.search(query)?;
                scope_search_page_resource_hrefs(book_id, &mut page);
                populate_search_result_sequence(&mut page)?;
                Ok(page)
            }
            SearchScope::SelectedBooks { book_ids } => self.search_many(book_ids.iter(), query),
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
            query: None,
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

    fn render_search_sequence_target_routed(
        &self,
        source_book_id: &BookId,
        item: &SearchResultSequenceTarget,
        options: &RenderOptions,
    ) -> Result<RoutedTargetView> {
        let item_book_id = item.book_id.as_ref().unwrap_or(source_book_id);
        if self.book(item_book_id).is_none() {
            let diagnostic = Diagnostic::warning(
                "search_result_sequence_book_missing",
                format!(
                    "search-result sequence referenced {}, which is not open in the library",
                    item_book_id.0
                ),
            )
            .with_context("book_id", &item_book_id.0);
            let mut view = ResolvedTargetView::unsupported(
                item.target.clone(),
                item.title
                    .clone()
                    .unwrap_or_else(|| "Missing book".to_owned()),
                diagnostic.clone(),
            );
            view.href = item.target.href();
            return Ok(RoutedTargetView {
                book_id: item_book_id.clone(),
                view,
                diagnostics: vec![diagnostic],
            });
        }
        let mut routed = self.render_target_routed(item_book_id, &item.target, options)?;
        if let Some(title) = &item.title {
            routed.view.title = Some(title.clone());
        }
        Ok(routed)
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
                result_sequence: None,
                diagnostics: Vec::new(),
            });
        }
        let ordered_book_ids = book_ids.collect::<Vec<_>>();
        let query_fingerprint = library_search_cursor_fingerprint(&ordered_book_ids, query);
        let (
            raw_start_index,
            mut inner_cursor,
            cursor_book_id,
            cursor_query_fingerprint,
            cursor_diagnostic,
        ) = decode_library_search_cursor(query.cursor.as_deref());
        let mut start_index = raw_start_index;
        let mut cursor_scope_matches = true;
        let mut page = SearchPage {
            hits: Vec::new(),
            next_cursor: None,
            result_sequence: None,
            diagnostics: Vec::new(),
        };
        if let Some(diagnostic) = cursor_diagnostic {
            page.diagnostics.push(diagnostic);
        }
        if let Some(cursor_query_fingerprint) = cursor_query_fingerprint
            && cursor_query_fingerprint != query_fingerprint
        {
            start_index = 0;
            inner_cursor = None;
            cursor_scope_matches = false;
            page.diagnostics.push(Diagnostic::warning(
                "stale_search_cursor_scope_changed",
                "library search cursor was created for a different query, mode, or book scope; search restarted",
            ));
        }
        if cursor_scope_matches && let Some(cursor_book_id) = cursor_book_id {
            if let Some(cursor_book_index) = ordered_book_ids
                .iter()
                .position(|book_id| **book_id == cursor_book_id)
            {
                start_index = cursor_book_index;
            } else {
                inner_cursor = None;
                page.diagnostics.push(
                    Diagnostic::warning(
                        "stale_search_cursor_book_missing",
                        format!(
                            "library search cursor referenced {}, which is not in the current search scope",
                            cursor_book_id.0
                        ),
                    )
                    .with_context("book_id", &cursor_book_id.0),
                );
            }
        }

        for (book_index, book_id) in ordered_book_ids.iter().enumerate().skip(start_index) {
            if page.hits.len() >= query.limit {
                page.next_cursor = Some(encode_library_search_cursor(
                    book_index,
                    Some(book_id),
                    Some(&query_fingerprint),
                    None,
                ));
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
            book_query.scope = SearchScope::CurrentBook {
                book_id: (*book_id).clone(),
            };
            book_query.cursor = if book_index == start_index {
                inner_cursor.take()
            } else {
                None
            };
            book_query.limit = query.limit.saturating_sub(page.hits.len());
            let mut book_page = book.search(&book_query)?;
            scope_search_page_resource_hrefs(book_id, &mut book_page);
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
                page.next_cursor = Some(encode_library_search_cursor(
                    book_index,
                    Some(book_id),
                    Some(&query_fingerprint),
                    Some(book_cursor),
                ));
                break;
            }
        }

        if page.hits.len() > query.limit {
            page.hits.truncate(query.limit);
        }
        populate_search_result_sequence(&mut page)?;
        Ok(page)
    }
}

fn populate_search_result_sequence(page: &mut SearchPage) -> Result<()> {
    if page.hits.is_empty() {
        page.result_sequence = None;
        return Ok(());
    }
    page.result_sequence = Some(SearchResultSequence::from_search_page(page)?.encode()?);
    Ok(())
}

fn library_search_sequence_target_matches(
    candidate: &SearchResultSequenceTarget,
    source_book_id: &BookId,
    target: &TargetToken,
) -> bool {
    candidate.target == *target
        && candidate
            .book_id
            .as_ref()
            .is_none_or(|book_id| book_id == source_book_id)
}

fn encode_library_search_cursor(
    book_index: usize,
    book_id: Option<&BookId>,
    query_fingerprint: Option<&str>,
    book_cursor: Option<String>,
) -> String {
    let cursor = LibrarySearchCursor {
        version: 1,
        book_index,
        book_id: book_id.cloned(),
        query_fingerprint: query_fingerprint.map(str::to_owned),
        book_cursor,
    };
    let bytes = serde_json::to_vec(&cursor).unwrap_or_default();
    URL_SAFE_NO_PAD.encode(bytes)
}

fn decode_library_search_cursor(
    cursor: Option<&str>,
) -> (
    usize,
    Option<String>,
    Option<BookId>,
    Option<String>,
    Option<Diagnostic>,
) {
    let Some(cursor) = cursor else {
        return (0, None, None, None, None);
    };
    let decoded = URL_SAFE_NO_PAD
        .decode(cursor)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LibrarySearchCursor>(&bytes).ok());
    match decoded {
        Some(cursor) if cursor.version == 1 => (
            cursor.book_index,
            cursor.book_cursor,
            cursor.book_id,
            cursor.query_fingerprint,
            None,
        ),
        _ => (
            0,
            None,
            None,
            None,
            Some(Diagnostic::warning(
                "invalid_search_cursor",
                "library search cursor could not be decoded; search restarted from the first book",
            )),
        ),
    }
}

fn library_search_cursor_fingerprint(book_ids: &[&BookId], query: &SearchQuery) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"lvcore-library-search-cursor-v1\0");
    hasher.update(serde_json::to_vec(&query.mode).unwrap_or_default());
    hasher.update(b"\0");
    hasher.update(query.query.as_bytes());
    hasher.update(b"\0");
    for book_id in book_ids {
        hasher.update(book_id.0.as_bytes());
        hasher.update(b"\0");
    }
    hex::encode(hasher.finalize())[..16].to_owned()
}

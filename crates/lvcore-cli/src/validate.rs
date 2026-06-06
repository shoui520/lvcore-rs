use std::collections::BTreeMap;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use lvcore::{
    BookId, BookLibrary, BookMetadata, Capability, DetectedPackage, Diagnostic, DiagnosticSeverity,
    DriverRegistry, FormatFamily, HomeSurface, NavigationStatus, NavigationSurface,
    NavigationSurfaceKind, NavigationTarget, RenderMode, RenderOptions, ResolvedTargetKind,
    ResolvedTargetView, ResourceKind, SearchHit, SearchMode, SearchQuery, SearchResultSequence,
    SearchScope, SequenceHint,
};
use serde_json::json;

use super::metadata_for;
#[cfg(test)]
use super::open_single_book_library;

const VALIDATE_RESOURCE_TARGET_SCAN_LIMIT: usize = 8;
const VALIDATE_GENERIC_HTML_NATIVE_HTML_LIMIT: usize = 128 * 1024;
const VALIDATE_GENERIC_HTML_RESOURCE_LIMIT: usize = 64;
const VALIDATE_DIAGNOSTIC_SAMPLE_LIMIT: usize = 8;
const VALIDATE_SURFACE_TARGET_PAGE_LIMIT: usize = 16;
const VALIDATE_SURFACE_PROBE_PAGE_LIMIT: usize = 16;
const VALIDATE_EMPTY_SEARCH_CURSOR_FOLLOW_LIMIT: usize = 4;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ValidateOptions {
    pub(crate) deep: bool,
    pub(crate) include_expensive_search: bool,
}

struct SurfaceRenderedProbeContext<'a> {
    surface_id: &'a str,
    surface_kind: &'a NavigationSurfaceKind,
    opened_kind: &'a str,
    label: String,
    resource_scan: serde_json::Value,
}

#[cfg(test)]
pub(crate) fn validate_package_json(
    registry: &DriverRegistry,
    path: &Path,
    options: ValidateOptions,
) -> serde_json::Value {
    match open_single_book_library(registry, path) {
        Ok((library, book_id)) => {
            validate_opened_package_json(path.to_path_buf(), library, book_id, options)
        }
        Err(error) => json!({
            "path": path,
            "status": "open_error",
            "error": error.to_string(),
        }),
    }
}

pub(crate) fn validate_detected_package_json(
    registry: &DriverRegistry,
    detected: DetectedPackage,
    options: ValidateOptions,
) -> serde_json::Value {
    let path = detected.root.clone();
    let mut library = BookLibrary::new();
    match library.open_detected_package(detected, registry) {
        Ok(book_id) => validate_opened_package_json(path, library, book_id, options),
        Err(error) => json!({
            "path": path,
            "status": "open_error",
            "error": error.to_string(),
        }),
    }
}

fn validate_opened_package_json(
    path: PathBuf,
    library: BookLibrary,
    book_id: BookId,
    options: ValidateOptions,
) -> serde_json::Value {
    let metadata = match metadata_for(&library, &book_id) {
        Ok(metadata) => metadata,
        Err(error) => {
            return json!({
                "path": path,
                "status": "metadata_error",
                "book_id": book_id,
                "error": error.to_string(),
            });
        }
    };
    match library.home_surfaces(&book_id) {
        Ok(surfaces) => {
            let exercises = if options.deep {
                Some(exercise_reader_paths(
                    &library,
                    &book_id,
                    &surfaces,
                    metadata.format_family,
                    options.include_expensive_search,
                ))
            } else {
                None
            };
            json!({
                "path": path,
                "status": "ok",
                "book_id": metadata.book_id,
                "format_family": metadata.format_family,
                "format_label": metadata.format_label,
                "title": metadata.title,
                "capabilities": metadata.capabilities,
                "search_modes": metadata.search_modes,
                "diagnostics": metadata.diagnostics,
                "surface_count": surfaces.len(),
                "surfaces": surfaces,
                "exercises": exercises,
            })
        }
        Err(error) => json!({
            "path": path,
            "status": "surface_error",
            "book_id": metadata.book_id,
            "format_family": metadata.format_family,
            "format_label": metadata.format_label,
            "title": metadata.title,
            "capabilities": metadata.capabilities,
            "search_modes": metadata.search_modes,
            "diagnostics": metadata.diagnostics,
            "error": error.to_string(),
        }),
    }
}

pub(crate) fn validate_row_has_failure(row: &serde_json::Value) -> bool {
    let Some(status) = row.get("status").and_then(serde_json::Value::as_str) else {
        return true;
    };
    if status != "ok" {
        return true;
    }
    row.get("exercises")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|exercises| exercises.iter().any(validate_exercise_has_failure))
}

fn validate_exercise_has_failure(row: &serde_json::Value) -> bool {
    match row {
        serde_json::Value::Object(object) => {
            object
                .get("status")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|status| status.ends_with("_error"))
                || object.values().any(validate_exercise_has_failure)
        }
        serde_json::Value::Array(values) => values.iter().any(validate_exercise_has_failure),
        _ => false,
    }
}

fn exercise_reader_paths(
    library: &BookLibrary,
    book_id: &BookId,
    surfaces: &[HomeSurface],
    format_family: FormatFamily,
    include_expensive_search: bool,
) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    let resource_scan_limit = resource_scan_limit_for(format_family);
    for surface in surfaces {
        if surface.status != NavigationStatus::Available || surface.surface_id == "search" {
            continue;
        }
        let started = Instant::now();
        let mut row = match open_surface_probe_page(library, book_id, &surface.surface_id, None) {
            Ok(opened) => {
                if let NavigationSurface::Deferred { diagnostics, .. } = &opened {
                    let mut row = json!({
                        "kind": "surface_first_target",
                        "surface_id": surface.surface_id,
                        "surface_kind": surface.kind,
                        "opened_kind": navigation_surface_kind_name(&opened),
                        "status": "deferred",
                    });
                    insert_diagnostic_fields(&mut row, diagnostics);
                    row
                } else {
                    match surface_probe_targets(library, book_id, &surface.surface_id, &opened) {
                        Ok(probe) => match probe.first_target.as_ref() {
                            Some(target) => match library.render_target(
                                book_id,
                                &target.target,
                                &RenderOptions::default(),
                            ) {
                                Ok(view) => {
                                    let resource_scan = rendered_resource_scan_with_first_view(
                                        library,
                                        book_id,
                                        &probe.resource_targets,
                                        resource_scan_limit,
                                        &view,
                                    );
                                    let window_probe = target.sequence_hint.clone().map(|hint| {
                                        continuous_window_probe(
                                            library,
                                            book_id,
                                            &target.target,
                                            hint,
                                        )
                                    });
                                    let mut row = surface_rendered_view_probe(
                                        library,
                                        book_id,
                                        &view,
                                        SurfaceRenderedProbeContext {
                                            surface_id: &surface.surface_id,
                                            surface_kind: &surface.kind,
                                            opened_kind: navigation_surface_kind_name(&opened),
                                            label: target.label_text.clone(),
                                            resource_scan,
                                        },
                                    );
                                    insert_surface_page_probe_fields(&mut row, &probe);
                                    if let Some(window_probe) = window_probe {
                                        insert_named_value(&mut row, "window", window_probe);
                                    }
                                    row
                                }
                                Err(error) => json!({
                                    "kind": "surface_first_target",
                                    "surface_id": surface.surface_id,
                                    "surface_kind": surface.kind,
                                    "status": "render_error",
                                    "label": target.label_text,
                                    "resource_scan": rendered_resource_scan_first_render_error(
                                        &probe.resource_targets,
                                        &target.source_id,
                                        &target.label_text,
                                        error.to_string(),
                                    ),
                                    "pages_scanned": probe.pages_scanned,
                                    "remaining_cursor": probe.remaining_cursor,
                                    "error": error.to_string(),
                                }),
                            },
                            None => {
                                let resource_scan = rendered_resource_scan(
                                    library,
                                    book_id,
                                    &probe.resource_targets,
                                    resource_scan_limit,
                                );
                                targetless_surface_probe_row(
                                    &opened,
                                    &probe,
                                    resource_scan,
                                    &surface.surface_id,
                                    &surface.kind,
                                )
                            }
                        },
                        Err(error) => json!({
                            "kind": "surface_first_target",
                            "surface_id": surface.surface_id,
                            "surface_kind": surface.kind,
                            "opened_kind": navigation_surface_kind_name(&opened),
                            "status": "surface_page_error",
                            "error": error.to_string(),
                        }),
                    }
                }
            }
            Err(error) => json!({
                "kind": "surface_first_target",
                "surface_id": surface.surface_id,
                "surface_kind": surface.kind,
                "status": "surface_error",
                "error": error.to_string(),
            }),
        };
        insert_elapsed_ms(&mut row, started);
        rows.push(row);
    }

    let metadata = match metadata_for(library, book_id) {
        Ok(metadata) => metadata,
        Err(error) => {
            rows.push(json!({
                "kind": "search_modes",
                "status": "metadata_error",
                "error": error.to_string(),
            }));
            return rows;
        }
    };
    rows.extend(search_mode_exercises(
        library,
        book_id,
        &metadata,
        surfaces,
        resource_scan_limit,
        include_expensive_search,
    ));
    rows
}

#[derive(Debug)]
struct SurfaceTargetProbe {
    first_target: Option<NavigationTarget>,
    resource_targets: Vec<NavigationTarget>,
    pages_scanned: usize,
    remaining_cursor: Option<String>,
}

#[derive(Debug)]
pub(crate) struct TargetlessSurfaceProbe {
    pub(crate) status: &'static str,
    pub(crate) visible_item_count: usize,
    pub(crate) diagnostic_count: usize,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

pub(crate) fn targetless_surface_probe(surface: &NavigationSurface) -> TargetlessSurfaceProbe {
    let visible_item_count = surface_visible_item_count(surface);
    let diagnostics = surface_diagnostic_sample(surface);
    let diagnostic_count = surface_diagnostic_count(surface);
    let status = if visible_item_count > 0 || diagnostic_count > 0 {
        "ok"
    } else {
        "no_target"
    };
    TargetlessSurfaceProbe {
        status,
        visible_item_count,
        diagnostic_count,
        diagnostics,
    }
}

fn targetless_surface_probe_row(
    opened: &NavigationSurface,
    probe: &SurfaceTargetProbe,
    resource_scan: serde_json::Value,
    surface_id: &str,
    surface_kind: &NavigationSurfaceKind,
) -> serde_json::Value {
    let targetless = targetless_surface_probe(opened);
    let mut row = json!({
        "kind": "surface_first_target",
        "surface_id": surface_id,
        "surface_kind": surface_kind,
        "opened_kind": navigation_surface_kind_name(opened),
        "status": targetless.status,
        "target_status": "none",
        "visible_item_count": targetless.visible_item_count,
        "targetless_diagnostic_count": targetless.diagnostic_count,
        "resource_scan": resource_scan,
        "pages_scanned": probe.pages_scanned,
        "remaining_cursor": probe.remaining_cursor,
    });
    if targetless.status == "ok" {
        insert_named_value(
            &mut row,
            "note",
            json!("surface opened but has no actionable entry/resource target to render"),
        );
    }
    insert_diagnostic_fields(&mut row, &targetless.diagnostics);
    row
}

fn surface_probe_targets(
    library: &BookLibrary,
    book_id: &BookId,
    surface_id: &str,
    first_surface: &NavigationSurface,
) -> lvcore::Result<SurfaceTargetProbe> {
    let mut pages_scanned = 1usize;
    let mut resource_targets = first_surface.actionable_targets();
    let mut remaining_cursor = navigation_surface_next_cursor(first_surface).map(str::to_owned);

    while resource_targets.is_empty()
        && remaining_cursor.is_some()
        && pages_scanned < VALIDATE_SURFACE_TARGET_PAGE_LIMIT
    {
        let cursor = remaining_cursor.clone();
        let page = library.open_surface_page(
            book_id,
            surface_id,
            cursor.as_deref(),
            VALIDATE_SURFACE_PROBE_PAGE_LIMIT,
        )?;
        pages_scanned += 1;
        resource_targets = page.actionable_targets();
        remaining_cursor = navigation_surface_next_cursor(&page).map(str::to_owned);
    }

    Ok(SurfaceTargetProbe {
        first_target: resource_targets.first().cloned(),
        resource_targets,
        pages_scanned,
        remaining_cursor,
    })
}

fn navigation_surface_next_cursor(surface: &NavigationSurface) -> Option<&str> {
    match surface {
        NavigationSurface::SimpleMenu { next_cursor, .. }
        | NavigationSurface::TitleIndexBrowse { next_cursor, .. }
        | NavigationSurface::HierarchicalTree { next_cursor, .. }
        | NavigationSurface::InfoPages { next_cursor, .. } => next_cursor.as_deref(),
        NavigationSurface::ScreenMenu { .. }
        | NavigationSurface::Panel { .. }
        | NavigationSurface::FallbackSearch { .. }
        | NavigationSurface::Deferred { .. } => None,
    }
}

fn surface_visible_item_count(surface: &NavigationSurface) -> usize {
    match surface {
        NavigationSurface::SimpleMenu { nodes, .. }
        | NavigationSurface::HierarchicalTree { nodes, .. } => node_visible_item_count(nodes),
        NavigationSurface::ScreenMenu { screens, .. } => screens
            .iter()
            .map(|screen| 1usize + screen.hotspots.len())
            .sum(),
        NavigationSurface::TitleIndexBrowse { items, .. } => items.len(),
        NavigationSurface::Panel { cells, .. } => cells.len(),
        NavigationSurface::InfoPages { pages, .. } => pages.len(),
        NavigationSurface::FallbackSearch { .. } | NavigationSurface::Deferred { .. } => 0,
    }
}

fn node_visible_item_count(nodes: &[lvcore::navigation::NavigationNode]) -> usize {
    nodes
        .iter()
        .map(|node| {
            let label_count = usize::from(!node.label_text.trim().is_empty());
            label_count + node_visible_item_count(&node.children)
        })
        .sum()
}

fn surface_diagnostic_count(surface: &NavigationSurface) -> usize {
    match surface {
        NavigationSurface::SimpleMenu { nodes, .. }
        | NavigationSurface::HierarchicalTree { nodes, .. } => node_diagnostic_count(nodes),
        NavigationSurface::ScreenMenu {
            screens,
            diagnostics,
            ..
        } => {
            diagnostics.len()
                + screens
                    .iter()
                    .map(|screen| {
                        screen.diagnostics.len()
                            + screen
                                .hotspots
                                .iter()
                                .map(|hotspot| hotspot.diagnostics.len())
                                .sum::<usize>()
                    })
                    .sum::<usize>()
        }
        NavigationSurface::TitleIndexBrowse { items, .. }
        | NavigationSurface::InfoPages { pages: items, .. } => {
            items.iter().map(|item| item.diagnostics.len()).sum()
        }
        NavigationSurface::Panel { cells, .. } => {
            cells.iter().map(|cell| cell.diagnostics.len()).sum()
        }
        NavigationSurface::Deferred { diagnostics, .. } => diagnostics.len(),
        NavigationSurface::FallbackSearch { .. } => 0,
    }
}

fn node_diagnostic_count(nodes: &[lvcore::navigation::NavigationNode]) -> usize {
    nodes
        .iter()
        .map(|node| node.diagnostics.len() + node_diagnostic_count(&node.children))
        .sum()
}

fn surface_diagnostic_sample(surface: &NavigationSurface) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    collect_surface_diagnostic_sample(surface, &mut diagnostics);
    diagnostics.truncate(VALIDATE_DIAGNOSTIC_SAMPLE_LIMIT);
    diagnostics
}

fn collect_surface_diagnostic_sample(surface: &NavigationSurface, out: &mut Vec<Diagnostic>) {
    match surface {
        NavigationSurface::SimpleMenu { nodes, .. }
        | NavigationSurface::HierarchicalTree { nodes, .. } => {
            collect_node_diagnostic_sample(nodes, out);
        }
        NavigationSurface::ScreenMenu {
            screens,
            diagnostics,
            ..
        } => {
            out.extend(diagnostics.iter().cloned());
            for screen in screens {
                out.extend(screen.diagnostics.iter().cloned());
                for hotspot in &screen.hotspots {
                    out.extend(hotspot.diagnostics.iter().cloned());
                }
            }
        }
        NavigationSurface::TitleIndexBrowse { items, .. }
        | NavigationSurface::InfoPages { pages: items, .. } => {
            for item in items {
                out.extend(item.diagnostics.iter().cloned());
            }
        }
        NavigationSurface::Panel { cells, .. } => {
            for cell in cells {
                out.extend(cell.diagnostics.iter().cloned());
            }
        }
        NavigationSurface::Deferred { diagnostics, .. } => out.extend(diagnostics.iter().cloned()),
        NavigationSurface::FallbackSearch { .. } => {}
    }
}

fn collect_node_diagnostic_sample(
    nodes: &[lvcore::navigation::NavigationNode],
    out: &mut Vec<Diagnostic>,
) {
    for node in nodes {
        out.extend(node.diagnostics.iter().cloned());
        collect_node_diagnostic_sample(&node.children, out);
    }
}

fn insert_surface_page_probe_fields(row: &mut serde_json::Value, probe: &SurfaceTargetProbe) {
    if let Some(object) = row.as_object_mut() {
        object.insert("pages_scanned".to_owned(), json!(probe.pages_scanned));
        object.insert(
            "remaining_cursor".to_owned(),
            json!(probe.remaining_cursor.clone()),
        );
    }
}

fn search_mode_exercises(
    library: &BookLibrary,
    book_id: &BookId,
    metadata: &BookMetadata,
    surfaces: &[HomeSurface],
    resource_scan_limit: usize,
    include_expensive_search: bool,
) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    let probe_labels = search_probe_labels(library, book_id, metadata, surfaces);
    for mode in validate_search_modes_to_probe(metadata) {
        if should_skip_search_mode_probe(metadata, &mode, include_expensive_search) {
            rows.push(skipped_search_mode_exercise(mode));
            continue;
        }
        let query = select_search_probe_query(library, book_id, &mode, &probe_labels);
        let render_hits = mode == SearchMode::Forward;
        rows.push(search_mode_exercise(
            library,
            book_id,
            mode,
            query,
            resource_scan_limit,
            render_hits,
        ));
    }
    rows
}

fn validate_search_modes_to_probe(metadata: &BookMetadata) -> Vec<SearchMode> {
    metadata.search_modes.to_vec()
}

fn should_skip_search_mode_probe(
    metadata: &BookMetadata,
    mode: &SearchMode,
    include_expensive_search: bool,
) -> bool {
    if include_expensive_search || metadata.format_family != FormatFamily::Ssed {
        return false;
    }
    match mode {
        SearchMode::Partial => true,
        SearchMode::FullText => !metadata.capabilities.contains(&Capability::PreservedHtml),
        _ => false,
    }
}

fn skipped_search_mode_exercise(mode: SearchMode) -> serde_json::Value {
    json!({
        "kind": format!("search_{}", search_mode_key(&mode)),
        "status": "skipped_expensive",
        "mode": mode,
        "reason": "ssed_linear_index_or_raw_honmon_search_requires_explicit_include_expensive_search",
    })
}

fn search_mode_exercise(
    library: &BookLibrary,
    book_id: &BookId,
    mode: SearchMode,
    query: String,
    resource_scan_limit: usize,
    render_hits: bool,
) -> serde_json::Value {
    let kind = format!("search_{}", search_mode_key(&mode));
    let limit = if render_hits { 3 } else { 1 };
    let started = Instant::now();
    let mut row = match search_with_empty_cursor_follow(library, book_id, &mode, &query, limit) {
        Ok((page, cursor_pages_followed)) => {
            let mut row = json!({
                "kind": kind,
                "status": "ok",
                "mode": mode,
                "query": query,
                "hit_count": page.hits.len(),
                "cursor_pages_followed": cursor_pages_followed,
                "remaining_cursor": page.next_cursor,
            });
            insert_diagnostic_fields(&mut row, &page.diagnostics);
            if render_hits {
                let first_rendered_view = first_search_hit_render(library, book_id, &page.hits);
                let rendered_first = rendered_first_search_hit_probe(
                    library,
                    book_id,
                    first_rendered_view.as_ref().map(|row| row.as_ref()),
                );
                let resource_scan = rendered_search_resource_scan(
                    library,
                    book_id,
                    &page.hits,
                    resource_scan_limit,
                    first_rendered_view
                        .as_ref()
                        .and_then(|row| row.as_ref().ok()),
                );
                let window_probe = search_result_window_probe(library, book_id, &page.hits);
                if let Some(object) = row.as_object_mut() {
                    let rendered_hit_count = usize::from(rendered_first.is_some());
                    object.insert("rendered_first".to_owned(), json!(rendered_first));
                    object.insert("rendered_hit_count".to_owned(), json!(rendered_hit_count));
                    object.insert("resource_scan".to_owned(), resource_scan);
                    object.insert("window".to_owned(), window_probe);
                }
            }
            row
        }
        Err(error) => json!({
            "kind": kind,
            "status": "search_error",
            "mode": mode,
            "query": query,
            "error": error.to_string(),
        }),
    };
    insert_elapsed_ms(&mut row, started);
    row
}

pub(crate) fn search_with_empty_cursor_follow(
    library: &BookLibrary,
    book_id: &BookId,
    mode: &SearchMode,
    query: &str,
    limit: usize,
) -> lvcore::Result<(lvcore::SearchPage, usize)> {
    let mut page = library.search(&SearchQuery {
        scope: SearchScope::CurrentBook {
            book_id: book_id.clone(),
        },
        mode: mode.clone(),
        query: query.to_owned(),
        cursor: None,
        limit,
        gaiji_policy: None,
    })?;
    let mut cursor_pages_followed = 0usize;
    while page.hits.is_empty()
        && page.next_cursor.is_some()
        && cursor_pages_followed < VALIDATE_EMPTY_SEARCH_CURSOR_FOLLOW_LIMIT
    {
        let cursor = page.next_cursor.clone();
        let mut next_page = library.search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: book_id.clone(),
            },
            mode: mode.clone(),
            query: query.to_owned(),
            cursor,
            limit,
            gaiji_policy: None,
        })?;
        let mut diagnostics = page.diagnostics;
        diagnostics.extend(next_page.diagnostics);
        next_page.diagnostics = diagnostics;
        page = next_page;
        cursor_pages_followed = cursor_pages_followed.saturating_add(1);
    }
    Ok((page, cursor_pages_followed))
}

fn insert_elapsed_ms(row: &mut serde_json::Value, started: Instant) {
    if let Some(object) = row.as_object_mut() {
        object.insert(
            "elapsed_ms".to_owned(),
            json!(started.elapsed().as_millis()),
        );
    }
}

trait JsonDiagnosticFields {
    fn with_diagnostics(self, diagnostics: &[Diagnostic]) -> serde_json::Value;
}

impl JsonDiagnosticFields for serde_json::Value {
    fn with_diagnostics(mut self, diagnostics: &[Diagnostic]) -> serde_json::Value {
        insert_diagnostic_fields(&mut self, diagnostics);
        self
    }
}

fn insert_diagnostic_fields(row: &mut serde_json::Value, diagnostics: &[Diagnostic]) {
    let Some(object) = row.as_object_mut() else {
        return;
    };
    object.insert("diagnostic_count".to_owned(), json!(diagnostics.len()));
    if diagnostics.is_empty() {
        return;
    }
    object.insert(
        "diagnostic_codes".to_owned(),
        diagnostic_code_counts(diagnostics),
    );
    object.insert(
        "diagnostics".to_owned(),
        json!(
            diagnostics
                .iter()
                .take(VALIDATE_DIAGNOSTIC_SAMPLE_LIMIT)
                .cloned()
                .collect::<Vec<_>>()
        ),
    );
}

fn diagnostic_code_counts(diagnostics: &[Diagnostic]) -> serde_json::Value {
    let mut counts = BTreeMap::<(String, String), usize>::new();
    for diagnostic in diagnostics {
        *counts
            .entry((
                diagnostic_severity_key(diagnostic.severity).to_owned(),
                diagnostic.code.clone(),
            ))
            .or_insert(0) += 1;
    }
    serde_json::Value::Array(
        counts
            .into_iter()
            .map(|((severity, code), count)| {
                json!({
                    "severity": severity,
                    "code": code,
                    "count": count,
                })
            })
            .collect(),
    )
}

fn diagnostic_severity_key(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Info => "info",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Error => "error",
    }
}

fn rendered_view_probe(
    library: &BookLibrary,
    book_id: &BookId,
    view: &ResolvedTargetView,
) -> serde_json::Value {
    let resource_probe = first_readable_resource_probe(library, book_id, view);
    let status = if resource_probe
        .as_ref()
        .and_then(|probe| probe.get("status"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|status| status.ends_with("_error"))
    {
        "resource_error"
    } else {
        "ok"
    };
    json!({
        "status": status,
        "view_kind": view.kind,
        "display_html_len": view.display_html.as_ref().map(|value| value.len()).unwrap_or(0),
        "resource_count": view.resources.len(),
        "first_resource": resource_probe,
        "render_modes": render_mode_contract_probe(library, book_id, view),
    })
    .with_diagnostics(&view.diagnostics)
}

fn render_mode_contract_probe(
    library: &BookLibrary,
    book_id: &BookId,
    native_view: &ResolvedTargetView,
) -> serde_json::Value {
    let generic_html = if let Some(reason) = generic_html_probe_skip_reason(
        native_view
            .display_html
            .as_ref()
            .map(|value| value.len())
            .unwrap_or(0),
        native_view.resources.len(),
    ) {
        skipped_render_mode_probe(native_view, RenderMode::GenericHtml, reason)
    } else {
        render_mode_probe(library, book_id, native_view, RenderMode::GenericHtml)
    };
    json!({
        "generic_html": generic_html,
        "basic_text": render_mode_probe(library, book_id, native_view, RenderMode::BasicText),
    })
}

fn generic_html_probe_skip_reason(
    native_display_html_len: usize,
    native_resource_count: usize,
) -> Option<&'static str> {
    if native_display_html_len > VALIDATE_GENERIC_HTML_NATIVE_HTML_LIMIT {
        return Some("native_display_html_too_large");
    }
    if native_resource_count > VALIDATE_GENERIC_HTML_RESOURCE_LIMIT {
        return Some("resource_count_too_large");
    }
    None
}

fn skipped_render_mode_probe(
    native_view: &ResolvedTargetView,
    mode: RenderMode,
    reason: &'static str,
) -> serde_json::Value {
    json!({
        "status": "skipped_large_view",
        "view_kind": native_view.kind,
        "mode": mode,
        "reason": reason,
        "native_display_html_len": native_view.display_html.as_ref().map(|value| value.len()).unwrap_or(0),
        "native_resource_count": native_view.resources.len(),
    })
}

fn render_mode_probe(
    library: &BookLibrary,
    book_id: &BookId,
    native_view: &ResolvedTargetView,
    mode: RenderMode,
) -> serde_json::Value {
    match library.render_target(
        book_id,
        &native_view.target,
        &RenderOptions {
            mode,
            ..RenderOptions::default()
        },
    ) {
        Ok(view) => {
            let has_router_refs = view.display_html.as_deref().is_some_and(|html| {
                html.contains("lvcore://target/") || html.contains("lvcore://resource/")
            });
            let status = match mode {
                RenderMode::GenericHtml if has_router_refs => "router_reference_remaining",
                RenderMode::BasicText if view.display_html.is_some() => "basic_text_html_error",
                RenderMode::BasicText
                    if basic_text_expected(view.kind) && view.basic_text.is_none() =>
                {
                    "basic_text_empty_error"
                }
                _ => "ok",
            };
            let mut row = json!({
                "status": status,
                "view_kind": view.kind,
                "display_html_len": view.display_html.as_ref().map(|value| value.len()).unwrap_or(0),
                "basic_text_len": view.basic_text.as_ref().map(|value| value.len()).unwrap_or(0),
                "resource_count": view.resources.len(),
                "link_count": view.links.len(),
                "has_router_refs": has_router_refs,
            });
            insert_diagnostic_fields(&mut row, &view.diagnostics);
            row
        }
        Err(error) => json!({
            "status": format!("{}_error", render_mode_probe_key(mode)),
            "error": error.to_string(),
        }),
    }
}

fn basic_text_expected(kind: ResolvedTargetKind) -> bool {
    matches!(
        kind,
        ResolvedTargetKind::EntryBody
            | ResolvedTargetKind::HanreiPage
            | ResolvedTargetKind::InfoPage
            | ResolvedTargetKind::LawArticle
            | ResolvedTargetKind::SearchResults
    )
}

fn render_mode_probe_key(mode: RenderMode) -> &'static str {
    match mode {
        RenderMode::Native => "native",
        RenderMode::GenericHtml => "generic_html",
        RenderMode::BasicText => "basic_text",
        RenderMode::Debug => "debug",
    }
}

fn insert_named_value(row: &mut serde_json::Value, name: &str, value: serde_json::Value) {
    if let Some(object) = row.as_object_mut() {
        object.insert(name.to_owned(), value);
    }
}

fn search_result_window_probe(
    library: &BookLibrary,
    book_id: &BookId,
    hits: &[SearchHit],
) -> serde_json::Value {
    let Some(first) = hits.first() else {
        return json!({
            "status": "no_target",
        });
    };
    match SearchResultSequence::from_hits(hits)
        .and_then(|sequence| sequence.encode())
        .map(|value| SequenceHint::SearchResults { value })
        .and_then(|hint| {
            library.resolve_target_window(
                book_id,
                &first.target,
                Some(&hint),
                1,
                1,
                &RenderOptions::default(),
            )
        }) {
        Ok(window) => continuous_window_result_json(window),
        Err(error) => json!({
            "status": "window_error",
            "error": error.to_string(),
        }),
    }
}

fn continuous_window_probe(
    library: &BookLibrary,
    book_id: &BookId,
    target: &lvcore::TargetToken,
    sequence_hint: SequenceHint,
) -> serde_json::Value {
    match library.resolve_target_window(
        book_id,
        target,
        Some(&sequence_hint),
        1,
        1,
        &RenderOptions::default(),
    ) {
        Ok(window) => continuous_window_result_json(window),
        Err(error) => json!({
            "status": "window_error",
            "sequence_hint": sequence_hint,
            "error": error.to_string(),
        }),
    }
}

fn continuous_window_result_json(window: lvcore::TargetWindow) -> serde_json::Value {
    let mut row = json!({
        "status": "ok",
        "center_kind": window.center.kind,
        "before_count": window.before.len(),
        "after_count": window.after.len(),
        "center_display_html_len": window.center.display_html.as_ref().map(|value| value.len()).unwrap_or(0),
    });
    insert_diagnostic_fields(&mut row, &window.diagnostics);
    if let Some(object) = row.as_object_mut() {
        object.insert(
            "before_kinds".to_owned(),
            json!(
                window
                    .before
                    .iter()
                    .map(|view| view.kind)
                    .collect::<Vec<_>>()
            ),
        );
        object.insert(
            "after_kinds".to_owned(),
            json!(
                window
                    .after
                    .iter()
                    .map(|view| view.kind)
                    .collect::<Vec<_>>()
            ),
        );
    }
    row
}

fn surface_rendered_view_probe(
    library: &BookLibrary,
    book_id: &BookId,
    view: &ResolvedTargetView,
    context: SurfaceRenderedProbeContext<'_>,
) -> serde_json::Value {
    let mut row = rendered_view_probe(library, book_id, view);
    if let Some(object) = row.as_object_mut() {
        object.insert("kind".to_owned(), json!("surface_first_target"));
        object.insert("surface_id".to_owned(), json!(context.surface_id));
        object.insert("surface_kind".to_owned(), json!(context.surface_kind));
        object.insert("opened_kind".to_owned(), json!(context.opened_kind));
        object.insert("label".to_owned(), json!(context.label));
        object.insert("resource_scan".to_owned(), context.resource_scan);
    }
    row
}

fn first_search_hit_render(
    library: &BookLibrary,
    book_id: &BookId,
    hits: &[SearchHit],
) -> Option<std::result::Result<ResolvedTargetView, String>> {
    let hit = hits.first()?;
    Some(
        library
            .render_target(book_id, &hit.target, &RenderOptions::default())
            .map_err(|error| error.to_string()),
    )
}

fn rendered_first_search_hit_probe(
    library: &BookLibrary,
    book_id: &BookId,
    first_rendered_view: Option<std::result::Result<&ResolvedTargetView, &String>>,
) -> Option<serde_json::Value> {
    first_rendered_view.map(|row| match row {
        Ok(view) => rendered_view_probe(library, book_id, view),
        Err(error) => json!({
            "status": "render_error",
            "error": error,
        }),
    })
}

fn rendered_search_resource_scan(
    library: &BookLibrary,
    book_id: &BookId,
    hits: &[SearchHit],
    limit: usize,
    first_rendered_view: Option<&ResolvedTargetView>,
) -> serde_json::Value {
    let targets = hits
        .iter()
        .map(|hit| NavigationTarget {
            surface_id: "search".to_owned(),
            source_id: hit.title_text.clone(),
            label_html: hit.title_html.clone(),
            label_text: hit.title_text.clone(),
            target: hit.target.clone(),
            href: hit.href.clone(),
            sequence_hint: hit.sequence_hint.clone(),
            diagnostics: hit.diagnostics.clone(),
        })
        .collect::<Vec<_>>();
    if let Some(first_rendered_view) = first_rendered_view {
        rendered_resource_scan_with_first_view(
            library,
            book_id,
            &targets,
            limit,
            first_rendered_view,
        )
    } else {
        rendered_resource_scan(library, book_id, &targets, limit)
    }
}

fn rendered_resource_scan(
    library: &BookLibrary,
    book_id: &BookId,
    targets: &[NavigationTarget],
    limit: usize,
) -> serde_json::Value {
    rendered_resource_scan_inner(library, book_id, targets, limit, None)
}

fn rendered_resource_scan_with_first_view(
    library: &BookLibrary,
    book_id: &BookId,
    targets: &[NavigationTarget],
    limit: usize,
    first_view: &ResolvedTargetView,
) -> serde_json::Value {
    rendered_resource_scan_inner(library, book_id, targets, limit, Some(first_view))
}

fn rendered_resource_scan_inner(
    library: &BookLibrary,
    book_id: &BookId,
    targets: &[NavigationTarget],
    limit: usize,
    first_view: Option<&ResolvedTargetView>,
) -> serde_json::Value {
    let started = Instant::now();
    let mut checked_target_count = 0usize;
    let mut slowest_target_ms = 0u128;
    let mut slowest_target_index = None;
    for (index, target) in targets.iter().take(limit).enumerate() {
        checked_target_count += 1;
        let target_started = Instant::now();
        let cached_view = (index == 0).then_some(first_view).flatten();
        let owned_view;
        let view = if let Some(view) = cached_view {
            view
        } else {
            owned_view =
                match library.render_target(book_id, &target.target, &RenderOptions::default()) {
                    Ok(view) => view,
                    Err(error) => {
                        return json!({
                            "status": "render_error",
                            "target_count": targets.len(),
                            "checked_target_count": checked_target_count,
                            "target_index": index,
                            "source_id": target.source_id,
                            "label": target.label_text,
                            "error": error.to_string(),
                        });
                    }
                };
            &owned_view
        };
        let target_elapsed_ms = if cached_view.is_some() {
            0
        } else {
            target_started.elapsed().as_millis()
        };
        if target_elapsed_ms > slowest_target_ms {
            slowest_target_ms = target_elapsed_ms;
            slowest_target_index = Some(index);
        }
        let Some(first_resource) = first_readable_resource_probe(library, book_id, view) else {
            continue;
        };
        let status = if first_resource
            .get("status")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|status| status.ends_with("_error"))
        {
            "resource_error"
        } else {
            "ok"
        };
        return json!({
            "status": status,
            "target_count": targets.len(),
            "checked_target_count": checked_target_count,
            "target_index": index,
            "elapsed_ms": started.elapsed().as_millis(),
            "slowest_target_ms": slowest_target_ms,
            "slowest_target_index": slowest_target_index,
            "source_id": target.source_id,
            "label": target.label_text,
            "view_kind": view.kind,
            "display_html_len": view.display_html.as_ref().map(|value| value.len()).unwrap_or(0),
            "resource_count": view.resources.len(),
            "first_resource": first_resource,
        })
        .with_diagnostics(&view.diagnostics);
    }

    json!({
        "status": "no_resource",
        "target_count": targets.len(),
        "checked_target_count": checked_target_count,
        "elapsed_ms": started.elapsed().as_millis(),
        "slowest_target_ms": slowest_target_ms,
        "slowest_target_index": slowest_target_index,
    })
}

fn rendered_resource_scan_first_render_error(
    targets: &[NavigationTarget],
    source_id: &str,
    label: &str,
    error: String,
) -> serde_json::Value {
    json!({
        "status": "render_error",
        "target_count": targets.len(),
        "checked_target_count": usize::from(!targets.is_empty()),
        "target_index": 0,
        "source_id": source_id,
        "label": label,
        "error": error,
    })
}

fn resource_scan_limit_for(format_family: FormatFamily) -> usize {
    match format_family {
        FormatFamily::Ssed => 1,
        FormatFamily::LvedSqlite3 | FormatFamily::LvlMultiView | FormatFamily::Hourei => {
            VALIDATE_RESOURCE_TARGET_SCAN_LIMIT
        }
        FormatFamily::Unknown => 1,
    }
}

fn first_readable_resource_probe(
    library: &BookLibrary,
    book_id: &BookId,
    view: &ResolvedTargetView,
) -> Option<serde_json::Value> {
    let resource = view.resources.iter().find(|resource| {
        resource.href.is_some()
            && !matches!(resource.kind, ResourceKind::Css | ResourceKind::Javascript)
    })?;
    match library.read_resource(book_id, &resource.token) {
        Ok(bytes) => Some(json!({
            "status": "ok",
            "kind": resource.kind,
            "mime_type": resource.mime_type,
            "byte_len": bytes.len(),
        })),
        Err(error) => Some(json!({
            "status": "resource_read_error",
            "kind": resource.kind,
            "mime_type": resource.mime_type,
            "error": error.to_string(),
        })),
    }
}

fn navigation_surface_kind_name(surface: &NavigationSurface) -> &'static str {
    match surface {
        NavigationSurface::SimpleMenu { .. } => "simple_menu",
        NavigationSurface::ScreenMenu { .. } => "screen_menu",
        NavigationSurface::TitleIndexBrowse { .. } => "title_index_browse",
        NavigationSurface::Panel { .. } => "panel",
        NavigationSurface::HierarchicalTree { .. } => "hierarchical_tree",
        NavigationSurface::InfoPages { .. } => "info_pages",
        NavigationSurface::FallbackSearch { .. } => "fallback_search",
        NavigationSurface::Deferred { .. } => "deferred",
    }
}

fn search_probe_labels(
    library: &BookLibrary,
    book_id: &BookId,
    metadata: &BookMetadata,
    surfaces: &[HomeSurface],
) -> Vec<String> {
    let mut labels = Vec::new();
    if metadata.format_family == FormatFamily::Ssed {
        for label in default_search_probe_labels(metadata) {
            push_probe_label(&mut labels, label);
        }
    }
    let preferred = [
        NavigationSurfaceKind::TitleIndexBrowse,
        NavigationSurfaceKind::Menu,
        NavigationSurfaceKind::MultiSelector,
        NavigationSurfaceKind::LvedTree,
        NavigationSurfaceKind::LawTree,
        NavigationSurfaceKind::MultiviewTree,
        NavigationSurfaceKind::Panel,
    ];
    for kind in preferred {
        for surface in surfaces {
            if surface.status != NavigationStatus::Available
                || surface.surface_id == "search"
                || surface.kind != kind
            {
                continue;
            }
            let Ok(opened) = open_surface_probe_page(library, book_id, &surface.surface_id, None)
            else {
                continue;
            };
            collect_actionable_probe_labels(&opened, &mut labels);
        }
    }
    if let Some(title) = &metadata.title {
        push_probe_label(&mut labels, title);
    }
    if metadata.format_family != FormatFamily::Ssed {
        for label in default_search_probe_labels(metadata) {
            push_probe_label(&mut labels, label);
        }
    }
    if labels.is_empty() {
        labels.push("a".to_owned());
    }
    labels
}

fn open_surface_probe_page(
    library: &BookLibrary,
    book_id: &BookId,
    surface_id: &str,
    cursor: Option<&str>,
) -> lvcore::Result<NavigationSurface> {
    library.open_surface_page(
        book_id,
        surface_id,
        cursor,
        VALIDATE_SURFACE_PROBE_PAGE_LIMIT,
    )
}

fn collect_actionable_probe_labels(surface: &NavigationSurface, out: &mut Vec<String>) {
    const SEARCH_PROBE_LABEL_LIMIT: usize = 12;
    for target in surface.actionable_targets() {
        push_probe_label(out, &target.label_text);
        if out.len() >= SEARCH_PROBE_LABEL_LIMIT {
            break;
        }
    }
}

fn push_probe_label(out: &mut Vec<String>, label: &str) {
    let trimmed = label.trim();
    if trimmed.is_empty() || search_probe_lookup_text(trimmed).is_none() {
        return;
    }
    if !out.iter().any(|seen| seen == trimmed) {
        out.push(trimmed.to_owned());
    }
}

fn default_search_probe_labels(metadata: &BookMetadata) -> &'static [&'static str] {
    match metadata.format_family {
        FormatFamily::LvlMultiView
            if metadata.capabilities.contains(&Capability::LawNavigation) =>
        {
            &["民法", "憲法", "刑法", "a", "あ"]
        }
        FormatFamily::LvlMultiView => &["a", "la", "あ"],
        _ => &["a", "あ"],
    }
}

fn select_search_probe_query(
    library: &BookLibrary,
    book_id: &BookId,
    mode: &SearchMode,
    labels: &[String],
) -> String {
    let mut fallback = None;
    let mut prioritized_labels = labels
        .iter()
        .filter(|label| !is_default_search_probe_label(label))
        .chain(
            labels
                .iter()
                .filter(|label| is_default_search_probe_label(label)),
        );
    let mut normal_labels = labels.iter();
    let label_iter: &mut dyn Iterator<Item = &String> = if search_probe_prefers_real_labels(mode) {
        &mut prioritized_labels
    } else {
        &mut normal_labels
    };
    for label in label_iter {
        let query = search_probe_query(label, mode);
        if !search_probe_query_is_useful(&query) {
            continue;
        }
        if fallback.is_none() {
            fallback = Some(query.clone());
        }
        if query == "a" && search_probe_lookup_text(label).is_none() {
            continue;
        }
        let Ok(page) = library.search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: book_id.clone(),
            },
            mode: mode.clone(),
            query: query.clone(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        }) else {
            continue;
        };
        if !page.hits.is_empty() {
            return query;
        }
    }
    fallback.unwrap_or_else(|| "a".to_owned())
}

fn search_probe_prefers_real_labels(mode: &SearchMode) -> bool {
    matches!(
        mode,
        SearchMode::Exact
            | SearchMode::Forward
            | SearchMode::Backward
            | SearchMode::Partial
            | SearchMode::FullText
            | SearchMode::Advanced(_)
    )
}

fn is_default_search_probe_label(label: &str) -> bool {
    matches!(label, "a" | "あ")
}

fn search_probe_query_is_useful(query: &str) -> bool {
    query.chars().any(char::is_alphanumeric)
}

fn search_probe_prefix(title: &str) -> Option<String> {
    let normalized = search_probe_lookup_text(title)?;
    let trimmed = normalized.as_str();
    if trimmed.is_empty() {
        return None;
    }
    let mut end = 0usize;
    let mut chars = 0usize;
    for (index, ch) in trimmed.char_indices() {
        if ch.is_whitespace() {
            if chars == 0 {
                continue;
            }
            break;
        }
        end = index + ch.len_utf8();
        chars += 1;
        if chars >= 2 {
            break;
        }
    }
    (end > 0).then(|| trimmed[..end].to_owned())
}

fn search_probe_suffix(title: &str) -> Option<String> {
    let normalized = search_probe_lookup_text(title)?;
    let trimmed = normalized.as_str();
    if trimmed.is_empty() {
        return None;
    }
    let chars = trimmed.chars().rev().take(2).collect::<Vec<_>>();
    if chars.is_empty() {
        None
    } else {
        Some(chars.into_iter().rev().collect())
    }
}

fn search_probe_partial_text(title: &str) -> Option<String> {
    let normalized = search_probe_lookup_text(title)?;
    let mut current = String::new();
    for ch in normalized.chars().chain(std::iter::once(' ')) {
        if ch.is_alphanumeric() {
            current.push(ch);
            continue;
        }
        if search_probe_run_is_useful(&current) {
            return Some(current.chars().take(2).collect());
        }
        current.clear();
    }
    search_probe_prefix(title)
}

fn search_probe_run_is_useful(value: &str) -> bool {
    value.chars().count() >= 2 && !value.chars().all(|ch| ch.is_numeric())
}

fn search_probe_lookup_text(title: &str) -> Option<String> {
    let title = title.trim();
    if let Some((inside, after)) = split_leading_search_probe_bracket(title) {
        if let Some(after_lookup) = search_probe_lookup_text(after) {
            return Some(after_lookup);
        }
        return search_probe_lookup_text(inside);
    }
    let mut started = false;
    let mut out = String::new();
    for ch in title.chars() {
        if !started {
            if is_search_probe_leading_decoration(ch) || ch.is_whitespace() {
                continue;
            }
            started = true;
        }
        if ch.is_whitespace() || is_search_probe_label_boundary(ch) {
            break;
        }
        let first = out.chars().next();
        if ch.is_ascii_digit() && first.is_some_and(|first| !first.is_ascii_digit()) {
            break;
        }
        out.push(ch);
    }
    let out = out.trim().to_owned();
    (!out.is_empty()).then_some(out)
}

fn split_leading_search_probe_bracket(title: &str) -> Option<(&str, &str)> {
    let (open, close) = match title.chars().next()? {
        '【' => ('【', '】'),
        '［' => ('［', '］'),
        '[' => ('[', ']'),
        '〖' => ('〖', '〗'),
        '〘' => ('〘', '〙'),
        '《' => ('《', '》'),
        '〈' => ('〈', '〉'),
        '(' => ('(', ')'),
        '（' => ('（', '）'),
        _ => return None,
    };
    let open_len = open.len_utf8();
    let close_index = title[open_len..].find(close)? + open_len;
    let close_len = close.len_utf8();
    Some((
        &title[open_len..close_index],
        &title[close_index + close_len..],
    ))
}

fn is_search_probe_leading_decoration(ch: char) -> bool {
    matches!(
        ch,
        '◎' | '○'
            | '●'
            | '■'
            | '□'
            | '◆'
            | '◇'
            | '・'
            | '▶'
            | '▷'
            | '▸'
            | '▹'
            | '→'
            | '⇒'
            | '※'
            | '*'
            | '＊'
            | '★'
            | '☆'
            | '【'
            | '［'
            | '['
            | '〖'
            | '〘'
            | '《'
            | '〈'
            | '('
            | '（'
    )
}

fn is_search_probe_label_boundary(ch: char) -> bool {
    matches!(
        ch,
        '【' | '（'
            | '】'
            | '('
            | '）'
            | ')'
            | '［'
            | '］'
            | '['
            | ']'
            | '〖'
            | '〗'
            | '〘'
            | '〙'
            | '《'
            | '》'
            | '〈'
            | '〉'
            | '<'
            | '＜'
            | ':'
            | '：'
            | ','
            | '，'
            | '、'
            | ';'
            | '；'
            | '/'
            | '／'
    )
}

fn search_probe_query(title: &str, mode: &SearchMode) -> String {
    match mode {
        SearchMode::Exact => {
            if let Some(trimmed) = search_probe_lookup_text(title) {
                trimmed
            } else {
                "a".to_owned()
            }
        }
        SearchMode::Backward => search_probe_suffix(title).unwrap_or_else(|| "a".to_owned()),
        SearchMode::Forward => search_probe_prefix(title).unwrap_or_else(|| "a".to_owned()),
        SearchMode::Partial | SearchMode::FullText | SearchMode::Advanced(_) => {
            search_probe_partial_text(title).unwrap_or_else(|| "a".to_owned())
        }
    }
}

fn search_mode_key(mode: &SearchMode) -> String {
    match mode {
        SearchMode::Exact => "exact".to_owned(),
        SearchMode::Forward => "forward".to_owned(),
        SearchMode::Backward => "backward".to_owned(),
        SearchMode::Partial => "partial".to_owned(),
        SearchMode::FullText => "full_text".to_owned(),
        SearchMode::Advanced(column) => format!(
            "advanced_{}",
            column
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
                .collect::<String>()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use rusqlite::Connection;
    use tempfile::tempdir;

    #[test]
    fn diagnostic_fields_include_counts_and_bounded_samples() {
        let diagnostics = (0..12)
            .map(|index| {
                Diagnostic::warning(
                    if index % 2 == 0 {
                        "even_code"
                    } else {
                        "odd_code"
                    },
                    format!("diagnostic {index}"),
                )
            })
            .collect::<Vec<_>>();
        let mut row = json!({ "status": "ok" });

        insert_diagnostic_fields(&mut row, &diagnostics);

        assert_eq!(row["diagnostic_count"], 12);
        assert_eq!(row["diagnostics"].as_array().unwrap().len(), 8);
        let codes = row["diagnostic_codes"].as_array().unwrap();
        assert!(codes.iter().any(|entry| {
            entry["severity"] == "warning" && entry["code"] == "even_code" && entry["count"] == 6
        }));
        assert!(codes.iter().any(|entry| {
            entry["severity"] == "warning" && entry["code"] == "odd_code" && entry["count"] == 6
        }));
    }

    #[test]
    fn search_probe_query_uses_lookup_term_not_display_decoration() {
        assert_eq!(
            search_probe_query("◎日本国憲法", &SearchMode::Exact),
            "日本国憲法"
        );
        assert_eq!(search_probe_query("あ【あ・ア】", &SearchMode::Exact), "あ");
        assert_eq!(
            search_probe_query("ああ1（aa）", &SearchMode::Exact),
            "ああ"
        );
        assert_eq!(search_probe_query("read1小", &SearchMode::Exact), "read");
        assert_eq!(search_probe_query("0＜sze zro＞", &SearchMode::Exact), "0");
        assert_eq!(search_probe_query("3D", &SearchMode::Exact), "3D");
        assert_eq!(search_probe_query("a, A", &SearchMode::Exact), "a");
        assert_eq!(search_probe_query("А, а1", &SearchMode::Exact), "А");
        assert_eq!(search_probe_query("***a", &SearchMode::Exact), "a");
        assert_eq!(search_probe_query("★重要", &SearchMode::Exact), "重要");
        assert_eq!(search_probe_lookup_text("【】"), None);
        assert_eq!(search_probe_query("【角】", &SearchMode::Exact), "角");
        assert_eq!(search_probe_query("《凡例》", &SearchMode::Exact), "凡例");
        assert_eq!(
            search_probe_query("【巻頭キーパーソン】大谷翔平", &SearchMode::Exact),
            "大谷翔平"
        );
    }

    #[test]
    fn search_probe_prefix_and_suffix_use_lookup_term() {
        assert_eq!(
            search_probe_query("◎日本国憲法", &SearchMode::Forward),
            "日本"
        );
        assert_eq!(search_probe_query("【角】", &SearchMode::Forward), "角");
        assert_eq!(
            search_probe_query("◎日本国憲法", &SearchMode::Backward),
            "憲法"
        );
        assert_eq!(
            search_probe_query("関係 関係がある 〖0001.01〗", &SearchMode::Partial),
            "関係"
        );
        assert_eq!(
            search_probe_query("０°人工歯(zero degree teeth)", &SearchMode::Partial),
            "人工"
        );
        assert_eq!(
            search_probe_query("０°人工歯(zero degree teeth)", &SearchMode::FullText),
            "人工"
        );
    }

    #[test]
    fn search_probe_query_usefulness_rejects_symbol_only_terms() {
        assert!(!search_probe_query_is_useful("〜\u{301}"));
        assert!(!search_probe_query_is_useful("・—"));
        assert!(search_probe_query_is_useful("007"));
        assert!(search_probe_query_is_useful("ａｌｐｈａ"));
        assert!(search_probe_query_is_useful("重要"));
    }

    #[test]
    fn validation_probe_prefers_real_labels_before_defaults() {
        assert!(search_probe_prefers_real_labels(&SearchMode::Exact));
        assert!(search_probe_prefers_real_labels(&SearchMode::Forward));
        assert!(search_probe_prefers_real_labels(&SearchMode::Backward));
        assert!(search_probe_prefers_real_labels(&SearchMode::Partial));
        assert!(search_probe_prefers_real_labels(&SearchMode::FullText));
        assert!(search_probe_prefers_real_labels(&SearchMode::Advanced(
            "advanced1".to_owned()
        )));
    }

    #[test]
    fn validate_deep_surface_probe_uses_bounded_pages() {
        let dir = tempdir().unwrap();
        write_many_row_lved_fixture(dir.path(), 20);

        let row = validate_package_json(
            &DriverRegistry::default(),
            dir.path(),
            ValidateOptions {
                deep: true,
                include_expensive_search: false,
            },
        );
        let lved_list = row["exercises"]
            .as_array()
            .unwrap()
            .iter()
            .find(|exercise| exercise["surface_id"] == "lved-list")
            .expect("expected LVED list surface validation row");

        assert_eq!(lved_list["status"], "ok");
        assert_eq!(lved_list["pages_scanned"], 1);
        assert_eq!(lved_list["remaining_cursor"], "16");
    }

    #[test]
    fn validate_resource_scans_are_bounded_by_family() {
        assert_eq!(resource_scan_limit_for(FormatFamily::Ssed), 1);
        assert_eq!(
            resource_scan_limit_for(FormatFamily::LvedSqlite3),
            VALIDATE_RESOURCE_TARGET_SCAN_LIMIT
        );
        assert_eq!(
            resource_scan_limit_for(FormatFamily::LvlMultiView),
            VALIDATE_RESOURCE_TARGET_SCAN_LIMIT
        );
        assert_eq!(
            resource_scan_limit_for(FormatFamily::Hourei),
            VALIDATE_RESOURCE_TARGET_SCAN_LIMIT
        );
        assert_eq!(VALIDATE_RESOURCE_TARGET_SCAN_LIMIT, 8);
    }

    #[test]
    fn validate_generic_html_probe_skips_large_native_views_only() {
        assert_eq!(generic_html_probe_skip_reason(4096, 4), None);
        assert_eq!(
            generic_html_probe_skip_reason(VALIDATE_GENERIC_HTML_NATIVE_HTML_LIMIT + 1, 4),
            Some("native_display_html_too_large")
        );
        assert_eq!(
            generic_html_probe_skip_reason(4096, VALIDATE_GENERIC_HTML_RESOURCE_LIMIT + 1),
            Some("resource_count_too_large")
        );
    }

    #[test]
    fn validate_reports_package_metadata_diagnostics() {
        let dir = tempdir().unwrap();
        write_many_row_lved_fixture(dir.path(), 2);
        fs::write(
            dir.path().join("BHINDEX.DIC"),
            sseddata_literal_fixture(b"retained"),
        )
        .unwrap();

        let row = validate_package_json(
            &DriverRegistry::default(),
            dir.path(),
            ValidateOptions {
                deep: false,
                include_expensive_search: false,
            },
        );

        assert_eq!(row["status"], "ok");
        assert_eq!(row["diagnostics"].as_array().unwrap().len(), 1);
        assert_eq!(
            row["diagnostics"][0]["code"],
            "retained_ssed_component_deferred"
        );
        assert_eq!(row["diagnostics"][0]["context"]["filename"], "BHINDEX.DIC");
    }

    fn write_many_row_lved_fixture(root: &Path, row_count: usize) {
        let key = "test-key";
        let payload = root.join("main.data");
        let connection = Connection::open(&payload).unwrap();
        connection.pragma_update(None, "key", key).unwrap();
        connection
            .pragma_update(None, "cipher_compatibility", 4)
            .unwrap();
        connection
            .execute_batch(
                "
                create table info (id integer, type integer, name text primary key, body text, media text);
                insert into info values (1, 1, 'about.html', '<h1>Validator Fixture</h1>', '');
                create table content (id integer primary key, type integer, body text, media text);
                create table list (id integer primary key, refid integer, type integer, anchor text, title text, titlesub text);
                create virtual table search using fts4(forward, back, part, fts, advanced1, advanced2, filter);
                ",
            )
            .unwrap();
        for index in 0..row_count {
            let id = index as i64 + 1;
            let content_id = 1000 + index as i64;
            let title = format!("alpha{index:02}");
            let body = format!("<article><h1>{title}</h1></article>");
            connection
                .execute(
                    "insert into content values (?1, 1, ?2, '')",
                    (content_id, body.as_str()),
                )
                .unwrap();
            connection
                .execute(
                    "insert into list values (?1, ?2, 1, '', ?3, '')",
                    (id, content_id, title.as_str()),
                )
                .unwrap();
            connection
                .execute(
                    "insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                     values (?1, ?2, ?3, ?4, ?5, '', '', ?6)",
                    (
                        id,
                        title.as_str(),
                        title.chars().rev().collect::<String>(),
                        title.as_str(),
                        body.as_str(),
                        format!("∥{title}∥"),
                    ),
                )
                .unwrap();
        }
        drop(connection);
        fs::write(root.join("main.key"), key).unwrap();
    }

    fn sseddata_literal_fixture(literals: &[u8]) -> Vec<u8> {
        let chunk_offset = 0x44usize;
        let mut data = vec![0u8; chunk_offset];
        data[..8].copy_from_slice(lvcore::SSEDDATA_MAGIC);
        data[0x0f] = 1;
        data[0x16..0x18].copy_from_slice(&1u16.to_be_bytes());
        data[0x18..0x1c].copy_from_slice(&1u32.to_be_bytes());
        data[0x1c..0x20].copy_from_slice(&1u32.to_be_bytes());
        data[0x40..0x44].copy_from_slice(&(chunk_offset as u32).to_be_bytes());
        data.extend_from_slice(&[0, 0]);
        data.extend_from_slice(&(literals.len() as u16).to_be_bytes());
        data.push(0);
        for literal in literals {
            data.extend_from_slice(&[0, 0, *literal]);
        }
        data
    }

    #[test]
    fn ssed_preserved_html_fulltext_is_not_skipped_by_default() {
        let mut metadata = BookMetadata {
            book_id: BookId("SSED:TEST".to_owned()),
            format_family: FormatFamily::Ssed,
            format_label: "SSED".to_owned(),
            package_root: PathBuf::from("test"),
            title: Some("test".to_owned()),
            root_fingerprint: "test".to_owned(),
            capabilities: vec![Capability::PreservedHtml],
            search_modes: vec![SearchMode::Exact, SearchMode::Partial, SearchMode::FullText],
            diagnostics: Vec::new(),
        };

        assert!(should_skip_search_mode_probe(
            &metadata,
            &SearchMode::Partial,
            false
        ));
        assert!(!should_skip_search_mode_probe(
            &metadata,
            &SearchMode::FullText,
            false
        ));

        metadata.capabilities.clear();
        assert!(should_skip_search_mode_probe(
            &metadata,
            &SearchMode::FullText,
            false
        ));
        assert!(!should_skip_search_mode_probe(
            &metadata,
            &SearchMode::FullText,
            true
        ));
    }
}

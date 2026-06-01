use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use lvcore::{
    BookId, BookLibrary, BookMetadata, Diagnostic, DiagnosticSeverity, DriverRegistry,
    FormatFamily, HomeSurface, NavigationStatus, NavigationSurface, NavigationSurfaceKind,
    NavigationTarget, RenderOptions, ResolvedTargetView, ResourceKind, SearchHit, SearchMode,
    SearchQuery, SearchScope,
};
use serde_json::json;

use super::{metadata_for, open_single_book_library};

const VALIDATE_RESOURCE_TARGET_SCAN_LIMIT: usize = 32;
const VALIDATE_SEARCH_HIT_RENDER_LIMIT: usize = 3;
const VALIDATE_DIAGNOSTIC_SAMPLE_LIMIT: usize = 8;

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

pub(crate) fn validate_package_json(
    registry: &DriverRegistry,
    path: &Path,
    options: ValidateOptions,
) -> serde_json::Value {
    match open_single_book_library(registry, path) {
        Ok((library, book_id)) => {
            let metadata = metadata_for(&library, &book_id);
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
                    "error": error.to_string(),
                }),
            }
        }
        Err(error) => json!({
            "path": path,
            "status": "open_error",
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
        let mut row = match library.open_surface(book_id, &surface.surface_id) {
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
                    let targets = opened.actionable_targets();
                    let resource_scan =
                        rendered_resource_scan(library, book_id, &targets, resource_scan_limit);
                    let (target, label) = targets
                        .first()
                        .map(|target| (Some(target.target.clone()), target.label_text.clone()))
                        .unwrap_or((None, String::new()));
                    match target {
                        Some(target) => {
                            match library.render_target(book_id, &target, &RenderOptions::default())
                            {
                                Ok(view) => surface_rendered_view_probe(
                                    library,
                                    book_id,
                                    &view,
                                    SurfaceRenderedProbeContext {
                                        surface_id: &surface.surface_id,
                                        surface_kind: &surface.kind,
                                        opened_kind: navigation_surface_kind_name(&opened),
                                        label,
                                        resource_scan,
                                    },
                                ),
                                Err(error) => json!({
                                    "kind": "surface_first_target",
                                    "surface_id": surface.surface_id,
                                    "surface_kind": surface.kind,
                                    "status": "render_error",
                                    "label": label,
                                    "resource_scan": resource_scan,
                                    "error": error.to_string(),
                                }),
                            }
                        }
                        None => json!({
                            "kind": "surface_first_target",
                            "surface_id": surface.surface_id,
                            "surface_kind": surface.kind,
                            "opened_kind": navigation_surface_kind_name(&opened),
                            "status": "no_target",
                            "resource_scan": resource_scan,
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

    let metadata = metadata_for(library, book_id);
    rows.extend(search_mode_exercises(
        library,
        book_id,
        &metadata,
        resource_scan_limit,
        include_expensive_search,
    ));
    rows
}

fn search_mode_exercises(
    library: &BookLibrary,
    book_id: &BookId,
    metadata: &BookMetadata,
    resource_scan_limit: usize,
    include_expensive_search: bool,
) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for mode in validate_search_modes_to_probe(metadata) {
        if should_skip_search_mode_probe(metadata, &mode, include_expensive_search) {
            rows.push(skipped_search_mode_exercise(mode));
            continue;
        }
        let query = search_probe_query(metadata.title.as_deref(), &mode);
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
    !include_expensive_search
        && metadata.format_family == FormatFamily::Ssed
        && matches!(mode, SearchMode::Partial | SearchMode::FullText)
}

fn skipped_search_mode_exercise(mode: SearchMode) -> serde_json::Value {
    json!({
        "kind": format!("search_{}", search_mode_key(&mode)),
        "status": "skipped_expensive",
        "mode": mode,
        "reason": "ssed_linear_or_fulltext_validation_requires_explicit_include_expensive_search",
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
    let mut row = match library.search(&SearchQuery {
        scope: SearchScope::CurrentBook {
            book_id: book_id.clone(),
        },
        mode: mode.clone(),
        query: query.clone(),
        cursor: None,
        limit,
    }) {
        Ok(page) => {
            let mut row = json!({
                "kind": kind,
                "status": "ok",
                "mode": mode,
                "query": query,
                "hit_count": page.hits.len(),
            });
            insert_diagnostic_fields(&mut row, &page.diagnostics);
            if render_hits {
                let rendered_hits = rendered_search_hit_probes(library, book_id, &page.hits);
                let rendered_first = rendered_hits.first().cloned();
                let resource_scan = rendered_search_resource_scan(
                    library,
                    book_id,
                    &page.hits,
                    resource_scan_limit,
                );
                if let Some(object) = row.as_object_mut() {
                    object.insert("rendered_first".to_owned(), json!(rendered_first));
                    object.insert("rendered_hit_count".to_owned(), json!(rendered_hits.len()));
                    object.insert("resource_scan".to_owned(), resource_scan);
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
    })
    .with_diagnostics(&view.diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

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

fn rendered_search_hit_probes(
    library: &BookLibrary,
    book_id: &BookId,
    hits: &[SearchHit],
) -> Vec<serde_json::Value> {
    hits.iter()
        .take(VALIDATE_SEARCH_HIT_RENDER_LIMIT)
        .map(|hit| {
            library
                .render_target(book_id, &hit.target, &RenderOptions::default())
                .map(|view| rendered_view_probe(library, book_id, &view))
                .unwrap_or_else(|error| {
                    json!({
                        "status": "render_error",
                        "error": error.to_string(),
                    })
                })
        })
        .collect()
}

fn rendered_search_resource_scan(
    library: &BookLibrary,
    book_id: &BookId,
    hits: &[SearchHit],
    limit: usize,
) -> serde_json::Value {
    let targets = hits
        .iter()
        .map(|hit| NavigationTarget {
            surface_id: "search".to_owned(),
            source_id: hit.title_text.clone(),
            label_html: hit.title_html.clone(),
            label_text: hit.title_text.clone(),
            target: hit.target.clone(),
            diagnostics: hit.diagnostics.clone(),
        })
        .collect::<Vec<_>>();
    rendered_resource_scan(library, book_id, &targets, limit)
}

fn rendered_resource_scan(
    library: &BookLibrary,
    book_id: &BookId,
    targets: &[NavigationTarget],
    limit: usize,
) -> serde_json::Value {
    let started = Instant::now();
    let mut checked_target_count = 0usize;
    let mut slowest_target_ms = 0u128;
    let mut slowest_target_index = None;
    for (index, target) in targets.iter().take(limit).enumerate() {
        checked_target_count += 1;
        let target_started = Instant::now();
        let view = match library.render_target(book_id, &target.target, &RenderOptions::default()) {
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
        let target_elapsed_ms = target_started.elapsed().as_millis();
        if target_elapsed_ms > slowest_target_ms {
            slowest_target_ms = target_elapsed_ms;
            slowest_target_index = Some(index);
        }
        let Some(first_resource) = first_readable_resource_probe(library, book_id, &view) else {
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

fn search_probe_prefix(title: &str) -> Option<&str> {
    let trimmed = title.trim();
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
    (end > 0).then_some(&trimmed[..end])
}

fn search_probe_suffix(title: &str) -> Option<String> {
    let trimmed = title.trim();
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

fn search_probe_query(title: Option<&str>, mode: &SearchMode) -> String {
    let title = title.unwrap_or_default();
    match mode {
        SearchMode::Exact => {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                "a".to_owned()
            } else {
                trimmed.to_owned()
            }
        }
        SearchMode::Backward => search_probe_suffix(title).unwrap_or_else(|| "a".to_owned()),
        SearchMode::Forward
        | SearchMode::Partial
        | SearchMode::FullText
        | SearchMode::Advanced(_) => search_probe_prefix(title).unwrap_or("a").to_owned(),
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

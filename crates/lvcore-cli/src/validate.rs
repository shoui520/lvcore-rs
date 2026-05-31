use std::path::Path;

use lvcore::{
    BookId, BookLibrary, DriverRegistry, FormatFamily, HomeSurface, NavigationStatus,
    NavigationSurface, NavigationSurfaceKind, NavigationTarget, RenderOptions, ResolvedTargetView,
    ResourceKind, SearchHit, SearchMode, SearchQuery, SearchScope,
};
use serde_json::json;

use super::{metadata_for, open_single_book_library};

const VALIDATE_RESOURCE_TARGET_SCAN_LIMIT: usize = 32;
const VALIDATE_SEARCH_HIT_RENDER_LIMIT: usize = 3;

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
    deep: bool,
) -> serde_json::Value {
    match open_single_book_library(registry, path) {
        Ok((library, book_id)) => {
            let metadata = metadata_for(&library, &book_id);
            match library.home_surfaces(&book_id) {
                Ok(surfaces) => {
                    let exercises = if deep {
                        Some(exercise_reader_paths(
                            &library,
                            &book_id,
                            &surfaces,
                            metadata.format_family,
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
) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    let resource_scan_limit = resource_scan_limit_for(format_family);
    for surface in surfaces {
        if surface.status != NavigationStatus::Available || surface.surface_id == "search" {
            continue;
        }
        let row = match library.open_surface(book_id, &surface.surface_id) {
            Ok(opened) => {
                if let NavigationSurface::Deferred { diagnostics, .. } = &opened {
                    json!({
                        "kind": "surface_first_target",
                        "surface_id": surface.surface_id,
                        "surface_kind": surface.kind,
                        "opened_kind": navigation_surface_kind_name(&opened),
                        "status": "deferred",
                        "diagnostic_count": diagnostics.len(),
                    })
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
        rows.push(row);
    }

    let metadata = metadata_for(library, book_id);
    let query = metadata
        .title
        .as_deref()
        .and_then(search_probe_prefix)
        .unwrap_or("a")
        .to_owned();
    let search_row = match library.search(&SearchQuery {
        scope: SearchScope::CurrentBook {
            book_id: book_id.clone(),
        },
        mode: SearchMode::Forward,
        query: query.clone(),
        cursor: None,
        limit: 3,
    }) {
        Ok(page) => {
            let rendered_hits = rendered_search_hit_probes(library, book_id, &page.hits);
            let rendered_first = rendered_hits.first().cloned();
            let resource_scan =
                rendered_search_resource_scan(library, book_id, &page.hits, resource_scan_limit);
            json!({
                "kind": "search_forward",
                "status": "ok",
                "query": query,
                "hit_count": page.hits.len(),
                "diagnostic_count": page.diagnostics.len(),
                "rendered_first": rendered_first,
                "rendered_hit_count": rendered_hits.len(),
                "resource_scan": resource_scan,
            })
        }
        Err(error) => json!({
            "kind": "search_forward",
            "status": "search_error",
            "query": query,
            "error": error.to_string(),
        }),
    };
    rows.push(search_row);
    rows
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
        "diagnostic_count": view.diagnostics.len(),
        "display_html_len": view.display_html.as_ref().map(|value| value.len()).unwrap_or(0),
        "resource_count": view.resources.len(),
        "first_resource": resource_probe,
    })
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
    let mut checked_target_count = 0usize;
    for (index, target) in targets.iter().take(limit).enumerate() {
        checked_target_count += 1;
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
            "source_id": target.source_id,
            "label": target.label_text,
            "view_kind": view.kind,
            "diagnostic_count": view.diagnostics.len(),
            "display_html_len": view.display_html.as_ref().map(|value| value.len()).unwrap_or(0),
            "resource_count": view.resources.len(),
            "first_resource": first_resource,
        });
    }

    json!({
        "status": "no_resource",
        "target_count": targets.len(),
        "checked_target_count": checked_target_count,
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

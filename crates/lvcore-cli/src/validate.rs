use std::path::Path;

use lvcore::{
    BookId, BookLibrary, DriverRegistry, HomeSurface, NavigationStatus, NavigationSurface,
    RenderOptions, SearchMode, SearchQuery, SearchScope, TargetToken,
};
use serde_json::json;

use super::{metadata_for, open_single_book_library};

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
                        Some(exercise_reader_paths(&library, &book_id, &surfaces))
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
    row.get("status")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|status| status.ends_with("_error"))
}

fn exercise_reader_paths(
    library: &BookLibrary,
    book_id: &BookId,
    surfaces: &[HomeSurface],
) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
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
                    let (target, label) = first_surface_target(&opened)
                        .map(|(target, label)| (Some(target), label))
                        .unwrap_or((None, String::new()));
                    match target {
                        Some(target) => {
                            match library.render_target(book_id, &target, &RenderOptions::default())
                            {
                                Ok(view) => json!({
                                    "kind": "surface_first_target",
                                    "surface_id": surface.surface_id,
                                    "surface_kind": surface.kind,
                                    "opened_kind": navigation_surface_kind_name(&opened),
                                    "status": "ok",
                                    "label": label,
                                    "view_kind": view.kind,
                                    "diagnostic_count": view.diagnostics.len(),
                                    "display_html_len": view.display_html.as_ref().map(|value| value.len()).unwrap_or(0),
                                }),
                                Err(error) => json!({
                                    "kind": "surface_first_target",
                                    "surface_id": surface.surface_id,
                                    "surface_kind": surface.kind,
                                    "status": "render_error",
                                    "label": label,
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
            let rendered_first = page.hits.first().map(|hit| {
                library
                    .render_target(book_id, &hit.target, &RenderOptions::default())
                    .map(|view| {
                        json!({
                            "status": "ok",
                            "view_kind": view.kind,
                            "diagnostic_count": view.diagnostics.len(),
                            "display_html_len": view.display_html.as_ref().map(|value| value.len()).unwrap_or(0),
                        })
                    })
                    .unwrap_or_else(|error| {
                        json!({
                            "status": "render_error",
                            "error": error.to_string(),
                        })
                    })
            });
            json!({
                "kind": "search_forward",
                "status": "ok",
                "query": query,
                "hit_count": page.hits.len(),
                "diagnostic_count": page.diagnostics.len(),
                "rendered_first": rendered_first,
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

fn first_surface_target(surface: &NavigationSurface) -> Option<(TargetToken, String)> {
    surface
        .actionable_targets()
        .into_iter()
        .next()
        .map(|target| (target.target, target.label_text))
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

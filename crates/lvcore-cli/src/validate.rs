use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use lvcore::{
    BookId, BookLibrary, BookMetadata, Capability, DetectedPackage, Diagnostic, DiagnosticSeverity,
    DriverRegistry, FormatFamily, HomeSurface, InternalTarget, NavigationStatus, NavigationSurface,
    NavigationSurfaceKind, NavigationTarget, PackageDiscoveryOptions, RenderMode, RenderOptions,
    ResolvedTargetKind, ResolvedTargetView, ResourceKind, SearchHit, SearchMode, SearchPage,
    SearchQuery, SearchResultSequence, SearchScope, SequenceHint, TargetKind,
};
use serde_json::json;

use super::metadata_for;
#[cfg(test)]
use super::open_single_book_library;

const VALIDATE_RESOURCE_TARGET_SCAN_LIMIT: usize = 8;
const VALIDATE_LINK_TARGET_SCAN_LIMIT: usize = 8;
const VALIDATE_GENERIC_HTML_NATIVE_HTML_LIMIT: usize = 128 * 1024;
const VALIDATE_GENERIC_HTML_RESOURCE_LIMIT: usize = 64;
const VALIDATE_GENERIC_HTML_RESOURCE_BYTES_LIMIT: u64 = 4 * 1024 * 1024;
const VALIDATE_DIAGNOSTIC_SAMPLE_LIMIT: usize = 8;
const VALIDATE_SURFACE_TARGET_PAGE_LIMIT: usize = 16;
const VALIDATE_SURFACE_PROBE_PAGE_LIMIT: usize = 16;
const VALIDATE_EMPTY_SEARCH_CURSOR_FOLLOW_LIMIT: usize = 4;
const VALIDATE_SEARCH_CURSOR_PROBE_LIMIT: usize = 1;
const VALIDATE_SEARCH_PROBE_LABEL_LIMIT: usize = 48;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ValidateOptions {
    pub(crate) deep: bool,
}

struct SurfaceRenderedProbeContext<'a> {
    surface_id: &'a str,
    surface_kind: &'a NavigationSurfaceKind,
    opened_kind: &'a str,
    label: String,
    resource_scan: serde_json::Value,
    link_scan_limit: usize,
}

#[cfg(test)]
pub(crate) fn validate_package_json(
    registry: &DriverRegistry,
    path: &Path,
    options: ValidateOptions,
) -> serde_json::Value {
    match open_single_book_library(registry, path) {
        Ok((mut library, book_id)) => {
            if options.deep {
                open_validation_cross_book_destinations(registry, &mut library, &book_id, path);
            }
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
        Ok(book_id) => {
            if options.deep {
                open_validation_cross_book_destinations(registry, &mut library, &book_id, &path);
            }
            validate_opened_package_json(path, library, book_id, options)
        }
        Err(error) => json!({
            "path": path,
            "status": "open_error",
            "error": error.to_string(),
        }),
    }
}

fn open_validation_cross_book_destinations(
    registry: &DriverRegistry,
    library: &mut BookLibrary,
    source_book_id: &BookId,
    source_root: &Path,
) {
    let Ok(surfaces) = library.home_surfaces(source_book_id) else {
        return;
    };
    let mut dict_codes = BTreeSet::new();
    collect_validation_ssed_cross_book_dict_codes(
        library,
        source_book_id,
        &surfaces,
        &mut dict_codes,
    );
    if metadata_for(library, source_book_id)
        .is_ok_and(|metadata| metadata.format_family == FormatFamily::LvedSqlite3)
    {
        collect_validation_lved_cross_book_dict_codes(
            library,
            source_book_id,
            &surfaces,
            &mut dict_codes,
        );
    }
    for dict_code in dict_codes {
        let Some(root) = validation_sibling_package_root(source_root, &dict_code) else {
            continue;
        };
        let Ok(mut detected) =
            registry.discover_best_packages(&root, PackageDiscoveryOptions::with_max(1))
        else {
            continue;
        };
        if let Some(detected) = detected.pop() {
            let _ = library.open_detected_package(detected, registry);
        }
    }
}

fn collect_validation_ssed_cross_book_dict_codes(
    library: &BookLibrary,
    source_book_id: &BookId,
    surfaces: &[HomeSurface],
    dict_codes: &mut BTreeSet<String>,
) {
    let mut probed_surface_ids = BTreeSet::new();
    for surface in surfaces {
        if !should_probe_home_surface(&mut probed_surface_ids, surface) {
            continue;
        }
        let Ok(opened) =
            open_surface_probe_page(library, source_book_id, &surface.surface_id, None)
        else {
            continue;
        };
        let Ok(probe) =
            surface_probe_targets(library, source_book_id, &surface.surface_id, &opened)
        else {
            continue;
        };
        for target in &probe.resource_targets {
            let Ok(InternalTarget::SsedCrossBookAddress { dict_code, .. }) = target.target.decode()
            else {
                continue;
            };
            dict_codes.insert(dict_code);
        }
    }
}

fn collect_validation_lved_cross_book_dict_codes(
    library: &BookLibrary,
    source_book_id: &BookId,
    surfaces: &[HomeSurface],
    dict_codes: &mut BTreeSet<String>,
) {
    let mut probed_surface_ids = BTreeSet::new();
    for surface in surfaces {
        if !should_probe_home_surface(&mut probed_surface_ids, surface) {
            continue;
        }
        let Ok(opened) =
            open_surface_probe_page(library, source_book_id, &surface.surface_id, None)
        else {
            continue;
        };
        let Ok(probe) =
            surface_probe_targets(library, source_book_id, &surface.surface_id, &opened)
        else {
            continue;
        };
        let Some(target) = probe.first_target else {
            continue;
        };
        let Ok(view) =
            library.render_target(source_book_id, &target.target, &RenderOptions::default())
        else {
            continue;
        };
        for link in view
            .links
            .iter()
            .filter(|link| link.kind == TargetKind::LvedCrossBook)
            .take(VALIDATE_LINK_TARGET_SCAN_LIMIT)
        {
            if let Some(dict_code) = lved_cross_book_dict_code_from_ref(&link.label) {
                dict_codes.insert(dict_code.to_owned());
            }
        }
    }
}

fn lved_cross_book_dict_code_from_ref(raw_ref: &str) -> Option<&str> {
    if let Some(value) = raw_ref.strip_prefix("lved.dataid.dict.") {
        return value
            .split_once(':')
            .map(|(dict_code, _)| dict_code)
            .filter(|dict_code| !dict_code.is_empty());
    }
    if let Some(value) = raw_ref.strip_prefix("lved.contentlink:") {
        return value
            .split_once('.')
            .map(|(dict_code, _)| dict_code)
            .filter(|dict_code| !dict_code.is_empty());
    }
    None
}

fn validation_sibling_package_root(source_root: &Path, dict_code: &str) -> Option<PathBuf> {
    let mut collection_roots = Vec::new();
    if let Some(parent) = source_root.parent() {
        collection_roots.push(parent.to_path_buf());
        if let Some(grandparent) = parent.parent() {
            collection_roots.push(grandparent.to_path_buf());
        }
    }
    let wanted = validation_dict_code_key(dict_code);
    for collection_root in collection_roots {
        for entry in fs::read_dir(collection_root).ok()? {
            let path = entry.ok()?.path();
            if !path.is_dir() || path == source_root {
                continue;
            }
            let name = path.file_name()?.to_string_lossy();
            if validation_dict_code_key(&name) != wanted {
                continue;
            }
            let nested = path.join(name.as_ref());
            if nested.is_dir() {
                return Some(nested);
            }
            return Some(path);
        }
    }
    None
}

fn validation_dict_code_key(value: &str) -> String {
    let trimmed = value.trim();
    trimmed
        .strip_prefix("_DCT_")
        .unwrap_or(trimmed)
        .to_ascii_uppercase()
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
) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    let resource_scan_limit = resource_scan_limit_for(format_family);
    let link_scan_limit = link_scan_limit_for(format_family);
    let mut probed_surface_ids = BTreeSet::new();
    for surface in surfaces {
        if !should_probe_home_surface(&mut probed_surface_ids, surface) {
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
                            Some(target) => match library.render_target_routed(
                                book_id,
                                &target.target,
                                &RenderOptions::default(),
                            ) {
                                Ok(routed) => {
                                    let routed_book_id = routed.book_id.clone();
                                    let routing_diagnostics = routed.diagnostics;
                                    let view = routed.view;
                                    let resource_scan = rendered_resource_scan_with_first_view(
                                        library,
                                        book_id,
                                        &probe.resource_targets,
                                        resource_scan_limit,
                                        &view,
                                        &routed_book_id,
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
                                        &routed_book_id,
                                        &view,
                                        SurfaceRenderedProbeContext {
                                            surface_id: &surface.surface_id,
                                            surface_kind: &surface.kind,
                                            opened_kind: navigation_surface_kind_name(&opened),
                                            label: target.label_text.clone(),
                                            resource_scan,
                                            link_scan_limit,
                                        },
                                    );
                                    insert_routing_probe_fields(
                                        &mut row,
                                        &routed_book_id,
                                        &routing_diagnostics,
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
        link_scan_limit,
    ));
    rows
}

fn should_probe_home_surface(
    probed_surface_ids: &mut BTreeSet<String>,
    surface: &HomeSurface,
) -> bool {
    if surface.status != NavigationStatus::Available || surface.surface_id == "search" {
        return false;
    }
    probed_surface_ids.insert(surface.surface_id.clone())
}

#[derive(Debug)]
struct SurfaceTargetProbe {
    first_target: Option<NavigationTarget>,
    resource_targets: Vec<NavigationTarget>,
    pages_scanned: usize,
    remaining_cursor: Option<String>,
    cursor_probe: Option<serde_json::Value>,
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
    if let Some(cursor_probe) = &probe.cursor_probe {
        insert_named_value(&mut row, "cursor_probe", cursor_probe.clone());
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
    let mut child_cursor = navigation_surface_first_child_cursor(first_surface).map(str::to_owned);
    let cursor_probe =
        navigation_surface_probe_cursor(first_surface).map(|(cursor_kind, cursor)| {
            surface_cursor_probe(library, book_id, surface_id, cursor_kind, &cursor)
        });

    while resource_targets.is_empty()
        && (remaining_cursor.is_some() || child_cursor.is_some())
        && pages_scanned < VALIDATE_SURFACE_TARGET_PAGE_LIMIT
    {
        let cursor = child_cursor.take().or_else(|| remaining_cursor.clone());
        let page = library.open_surface_page(
            book_id,
            surface_id,
            cursor.as_deref(),
            VALIDATE_SURFACE_PROBE_PAGE_LIMIT,
        )?;
        pages_scanned += 1;
        resource_targets = page.actionable_targets();
        remaining_cursor = navigation_surface_next_cursor(&page).map(str::to_owned);
        child_cursor = navigation_surface_first_child_cursor(&page).map(str::to_owned);
    }

    Ok(SurfaceTargetProbe {
        first_target: resource_targets.first().cloned(),
        resource_targets,
        pages_scanned,
        remaining_cursor,
        cursor_probe,
    })
}

fn navigation_surface_probe_cursor(surface: &NavigationSurface) -> Option<(&'static str, String)> {
    if let Some(cursor) = navigation_surface_first_child_cursor(surface) {
        return Some(("child", cursor.to_owned()));
    }
    navigation_surface_next_cursor(surface).map(|cursor| ("page", cursor.to_owned()))
}

fn surface_cursor_probe(
    library: &BookLibrary,
    book_id: &BookId,
    surface_id: &str,
    cursor_kind: &'static str,
    cursor: &str,
) -> serde_json::Value {
    let started = Instant::now();
    let mut row = match library.open_surface_page(
        book_id,
        surface_id,
        Some(cursor),
        VALIDATE_SURFACE_PROBE_PAGE_LIMIT,
    ) {
        Ok(page) => {
            let mut row = json!({
                "status": "ok",
                "cursor": cursor,
                "cursor_kind": cursor_kind,
                "opened_kind": navigation_surface_kind_name(&page),
                "visible_item_count": surface_visible_item_count(&page),
                "actionable_target_count": page.actionable_targets().len(),
                "remaining_cursor": navigation_surface_next_cursor(&page),
            });
            insert_diagnostic_fields(&mut row, &surface_diagnostic_sample(&page));
            row
        }
        Err(error) => json!({
            "status": "surface_cursor_error",
            "cursor": cursor,
            "cursor_kind": cursor_kind,
            "error": error.to_string(),
        }),
    };
    insert_elapsed_ms(&mut row, started);
    row
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

fn navigation_surface_first_child_cursor(surface: &NavigationSurface) -> Option<&str> {
    match surface {
        NavigationSurface::SimpleMenu { nodes, .. }
        | NavigationSurface::HierarchicalTree { nodes, .. } => first_node_child_cursor(nodes),
        NavigationSurface::ScreenMenu { .. }
        | NavigationSurface::TitleIndexBrowse { .. }
        | NavigationSurface::Panel { .. }
        | NavigationSurface::InfoPages { .. }
        | NavigationSurface::FallbackSearch { .. }
        | NavigationSurface::Deferred { .. } => None,
    }
}

fn first_node_child_cursor(nodes: &[lvcore::navigation::NavigationNode]) -> Option<&str> {
    for node in nodes {
        if let Some(cursor) = node.child_cursor.as_deref() {
            return Some(cursor);
        }
        if let Some(cursor) = first_node_child_cursor(&node.children) {
            return Some(cursor);
        }
    }
    None
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
        if let Some(cursor_probe) = &probe.cursor_probe {
            object.insert("cursor_probe".to_owned(), cursor_probe.clone());
        }
    }
}

fn search_mode_exercises(
    library: &BookLibrary,
    book_id: &BookId,
    metadata: &BookMetadata,
    surfaces: &[HomeSurface],
    resource_scan_limit: usize,
    link_scan_limit: usize,
) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    let probe_labels = search_probe_labels(library, book_id, metadata, surfaces);
    for mode in validate_search_modes_to_probe(metadata) {
        let query =
            select_validation_search_probe_query(library, book_id, metadata, &mode, &probe_labels);
        let render_hits = mode == SearchMode::Forward;
        rows.push(search_mode_exercise(
            library,
            book_id,
            mode,
            query,
            resource_scan_limit,
            link_scan_limit,
            render_hits,
        ));
    }
    rows
}

fn validate_search_modes_to_probe(metadata: &BookMetadata) -> Vec<SearchMode> {
    metadata.search_modes.to_vec()
}

fn search_mode_exercise(
    library: &BookLibrary,
    book_id: &BookId,
    mode: SearchMode,
    query: String,
    resource_scan_limit: usize,
    link_scan_limit: usize,
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
                    link_scan_limit,
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
            if let Some(cursor) = page.next_cursor.as_deref() {
                let cursor_probe = if should_probe_search_cursor(&mode, cursor) {
                    search_cursor_probe(
                        library,
                        book_id,
                        &mode,
                        &query,
                        cursor,
                        VALIDATE_SEARCH_CURSOR_PROBE_LIMIT,
                    )
                } else {
                    json!({
                        "status": "not_probed",
                        "cursor": cursor,
                        "reason": skipped_search_cursor_probe_reason(&mode, cursor),
                    })
                };
                if let Some(object) = row.as_object_mut() {
                    if cursor_probe
                        .get("status")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|status| status.ends_with("_error"))
                    {
                        object.insert("status".to_owned(), json!("search_cursor_error"));
                    }
                    object.insert("cursor_probe".to_owned(), cursor_probe);
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

fn should_probe_search_cursor(mode: &SearchMode, cursor: &str) -> bool {
    if is_unverified_partial_nonprefix_cursor(mode, cursor) {
        return false;
    }
    if is_unverified_native_offset_cursor(mode, cursor) {
        return false;
    }
    if is_unverified_title_label_cursor(mode, cursor) {
        return false;
    }
    if is_unverified_sidecar_title_cursor(mode, cursor) {
        return false;
    }
    if is_unverified_fulltext_nonprefix_title_cursor(mode, cursor) {
        return false;
    }
    if cursor.starts_with("lved-offset-unverified:") {
        return false;
    }
    !(matches!(mode, SearchMode::FullText)
        && (cursor.starts_with("body:")
            || cursor.starts_with("body-offset:")
            || cursor.starts_with("sidecar-body:")))
}

fn skipped_search_cursor_probe_reason(mode: &SearchMode, cursor: &str) -> &'static str {
    if is_unverified_partial_nonprefix_cursor(mode, cursor) {
        return "unverified partial non-prefix continuation may scan large SSED indexes";
    }
    if is_unverified_native_offset_cursor(mode, cursor) {
        return "unverified native offset continuation may scan large SSED indexes";
    }
    if is_unverified_title_label_cursor(mode, cursor) {
        return "unverified title-label fallback continuation may scan large SSED indexes";
    }
    if is_unverified_sidecar_title_cursor(mode, cursor) {
        return "unverified sidecar title continuation may scan large SSED sidecars";
    }
    if is_unverified_fulltext_nonprefix_title_cursor(mode, cursor) {
        return "unverified full-text non-prefix title continuation may scan large SSED indexes";
    }
    if cursor.starts_with("lved-offset-unverified:") {
        return "unverified LVED offset continuation may repeat broad SQLite searches";
    }
    "body full-text continuation cursors may rescan large SSED body windows"
}

fn is_unverified_partial_nonprefix_cursor(mode: &SearchMode, cursor: &str) -> bool {
    matches!(mode, SearchMode::Partial)
        && (cursor.starts_with("ssed-partial-nonprefix-unverified-index:")
            || cursor.starts_with("ssed-partial-nonprefix-unverified-physical-offset:")
            || cursor.starts_with("ssed-partial-nonprefix-noskip-unverified-physical-offset:"))
}

fn is_unverified_sidecar_title_cursor(mode: &SearchMode, cursor: &str) -> bool {
    (matches!(
        mode,
        SearchMode::Exact | SearchMode::Forward | SearchMode::Backward
    ) && cursor.starts_with("sidecar-title-unverified-row:"))
        || (matches!(mode, SearchMode::Partial)
            && cursor.starts_with("ssed-partial-prefix:sidecar-title-unverified-row:"))
}

fn is_unverified_fulltext_nonprefix_title_cursor(mode: &SearchMode, cursor: &str) -> bool {
    matches!(mode, SearchMode::FullText) && cursor.starts_with("title-nonprefix-unverified:")
}

fn is_unverified_title_label_cursor(mode: &SearchMode, cursor: &str) -> bool {
    (matches!(
        mode,
        SearchMode::Exact | SearchMode::Forward | SearchMode::Backward
    ) && cursor.starts_with("ssed-title-label-unverified:"))
        || (matches!(mode, SearchMode::Partial)
            && cursor.starts_with("ssed-partial-prefix:ssed-title-label-unverified:"))
}

fn is_unverified_native_offset_cursor(mode: &SearchMode, cursor: &str) -> bool {
    (matches!(
        mode,
        SearchMode::Exact | SearchMode::Forward | SearchMode::Backward
    ) && cursor.starts_with("ssed-offset-unverified:"))
        || (matches!(mode, SearchMode::Partial)
            && cursor.starts_with("ssed-partial-prefix:ssed-offset-unverified:"))
}

fn search_cursor_probe(
    library: &BookLibrary,
    book_id: &BookId,
    mode: &SearchMode,
    query: &str,
    cursor: &str,
    limit: usize,
) -> serde_json::Value {
    let started = Instant::now();
    let mut row = match library.search(&SearchQuery {
        scope: SearchScope::CurrentBook {
            book_id: book_id.clone(),
        },
        mode: mode.clone(),
        query: query.to_owned(),
        cursor: Some(cursor.to_owned()),
        limit,
        gaiji_policy: None,
    }) {
        Ok(page) => {
            let mut row = json!({
                "status": "ok",
                "cursor": cursor,
                "hit_count": page.hits.len(),
                "remaining_cursor": page.next_cursor,
            });
            insert_diagnostic_fields(&mut row, &page.diagnostics);
            row
        }
        Err(error) => json!({
            "status": "search_error",
            "cursor": cursor,
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

fn insert_routing_probe_fields(
    row: &mut serde_json::Value,
    routed_book_id: &BookId,
    diagnostics: &[Diagnostic],
) {
    let Some(object) = row.as_object_mut() else {
        return;
    };
    object.insert("routed_book_id".to_owned(), json!(routed_book_id));
    object.insert(
        "routing_diagnostic_count".to_owned(),
        json!(diagnostics.len()),
    );
    if diagnostics.is_empty() {
        return;
    }
    object.insert(
        "routing_diagnostic_codes".to_owned(),
        diagnostic_code_counts(diagnostics),
    );
    object.insert(
        "routing_diagnostics".to_owned(),
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
    link_scan_limit: usize,
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
    let mut row = json!({
        "status": status,
        "view_kind": view.kind,
        "display_html_len": view.display_html.as_ref().map(|value| value.len()).unwrap_or(0),
        "resource_count": view.resources.len(),
        "first_resource": resource_probe,
        "render_modes": render_mode_contract_probe(library, book_id, view),
    })
    .with_diagnostics(&view.diagnostics);
    let link_scan_limit = rendered_link_scan_limit_for_view(view, link_scan_limit);
    if link_scan_limit > 0 {
        insert_named_value(
            &mut row,
            "link_scan",
            rendered_link_scan(library, book_id, view, link_scan_limit),
        );
    }
    row
}

fn rendered_link_scan_limit_for_view(view: &ResolvedTargetView, link_scan_limit: usize) -> usize {
    if link_scan_limit == 0 || rendered_view_has_hc_diagnostics(view) {
        return 0;
    }
    link_scan_limit
}

fn rendered_view_has_hc_diagnostics(view: &ResolvedTargetView) -> bool {
    view.diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code.starts_with("hc_"))
}

fn render_mode_contract_probe(
    library: &BookLibrary,
    book_id: &BookId,
    native_view: &ResolvedTargetView,
) -> serde_json::Value {
    if let Some(reason) = render_mode_contract_skip_reason(native_view.kind) {
        return json!({
            "generic_html": skipped_render_mode_probe(native_view, RenderMode::GenericHtml, reason),
            "basic_text": skipped_render_mode_probe(native_view, RenderMode::BasicText, reason),
        });
    }

    let generic_html = if let Some(reason) = generic_html_probe_skip_reason(
        native_view
            .display_html
            .as_ref()
            .map(|value| value.len())
            .unwrap_or(0),
        native_view.resources.len(),
        native_view_known_resource_bytes(native_view),
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

fn render_mode_contract_skip_reason(kind: ResolvedTargetKind) -> Option<&'static str> {
    match kind {
        ResolvedTargetKind::NavigationSurface
        | ResolvedTargetKind::PanelSurface
        | ResolvedTargetKind::Deferred => Some("mode_invariant_surface"),
        _ => None,
    }
}

fn generic_html_probe_skip_reason(
    native_display_html_len: usize,
    native_resource_count: usize,
    native_resource_bytes: u64,
) -> Option<&'static str> {
    if native_display_html_len > VALIDATE_GENERIC_HTML_NATIVE_HTML_LIMIT {
        return Some("native_display_html_too_large");
    }
    if native_resource_count > VALIDATE_GENERIC_HTML_RESOURCE_LIMIT {
        return Some("resource_count_too_large");
    }
    if native_resource_bytes > VALIDATE_GENERIC_HTML_RESOURCE_BYTES_LIMIT {
        return Some("resource_bytes_too_large");
    }
    None
}

fn native_view_known_resource_bytes(native_view: &ResolvedTargetView) -> u64 {
    native_view
        .resources
        .iter()
        .filter_map(|resource| resource.byte_len)
        .sum()
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
        "native_resource_bytes": native_view_known_resource_bytes(native_view),
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
    match library.resolve_target_window_routed(
        book_id,
        target,
        Some(&sequence_hint),
        1,
        1,
        &RenderOptions::default(),
    ) {
        Ok(window) => continuous_routed_window_result_json(window),
        Err(error) => json!({
            "status": "window_error",
            "sequence_hint": sequence_hint,
            "error": error.to_string(),
        }),
    }
}

fn continuous_routed_window_result_json(window: lvcore::RoutedTargetWindow) -> serde_json::Value {
    let routed_book_id = window.book_id.clone();
    let mut row = continuous_window_result_json(window.window);
    if let Some(object) = row.as_object_mut() {
        object.insert("routed_book_id".to_owned(), json!(routed_book_id));
    }
    row
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
    let mut row = rendered_view_probe(library, book_id, view, context.link_scan_limit);
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
    link_scan_limit: usize,
) -> Option<serde_json::Value> {
    first_rendered_view.map(|row| match row {
        Ok(view) => rendered_view_probe(library, book_id, view, link_scan_limit),
        Err(error) => json!({
            "status": "render_error",
            "error": error,
        }),
    })
}

fn rendered_link_scan(
    library: &BookLibrary,
    book_id: &BookId,
    view: &ResolvedTargetView,
    limit: usize,
) -> serde_json::Value {
    let started = Instant::now();
    let mut checked_link_count = 0usize;
    let mut ok_count = 0usize;
    let mut unsupported_count = 0usize;
    let mut deferred_count = 0usize;
    let mut skipped_link_count = 0usize;
    let mut diagnostics = Vec::new();
    let mut samples = Vec::new();

    for (index, link) in view.links.iter().enumerate() {
        if checked_link_count >= limit {
            break;
        }
        if !rendered_link_target_is_validation_safe(link.kind) {
            skipped_link_count += 1;
            if samples.len() < VALIDATE_DIAGNOSTIC_SAMPLE_LIMIT {
                samples.push(json!({
                    "link_index": index,
                    "status": "skipped",
                    "label": link.label,
                    "target_kind": link.kind,
                    "skip_reason": "ssed_target_may_use_hc_rendering",
                    "link_diagnostic_count": link.diagnostics.len(),
                }));
            }
            continue;
        }
        checked_link_count += 1;
        let routed_target =
            match library.render_target_routed(book_id, &link.token, &RenderOptions::default()) {
                Ok(routed_target) => routed_target,
                Err(error) => {
                    diagnostics.extend(link.diagnostics.clone());
                    let mut row = json!({
                        "status": "render_error",
                        "link_count": view.links.len(),
                        "checked_link_count": checked_link_count,
                        "link_index": index,
                        "label": link.label,
                        "target_kind": link.kind,
                        "error": error.to_string(),
                        "elapsed_ms": started.elapsed().as_millis(),
                    });
                    insert_diagnostic_fields(&mut row, &diagnostics);
                    return row;
                }
            };
        let routed_cross_book = routed_target
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "lved_cross_book_routed");
        diagnostics.extend(
            link.diagnostics
                .iter()
                .filter(|diagnostic| {
                    !(routed_cross_book && diagnostic.code == "lved_cross_book_deferred")
                })
                .cloned(),
        );
        diagnostics.extend(routed_target.diagnostics);
        let target_view = routed_target.view;
        diagnostics.extend(target_view.diagnostics.clone());
        let link_status = rendered_link_target_status(&target_view);
        match link_status {
            "unsupported" => unsupported_count += 1,
            "deferred" => deferred_count += 1,
            _ => ok_count += 1,
        }
        if samples.len() < VALIDATE_DIAGNOSTIC_SAMPLE_LIMIT {
            samples.push(json!({
                "link_index": index,
                "status": link_status,
                "label": link.label,
                "target_kind": link.kind,
                "view_kind": target_view.kind,
                "display_html_len": target_view.display_html.as_ref().map(|value| value.len()).unwrap_or(0),
                "resource_count": target_view.resources.len(),
                "link_diagnostic_count": link.diagnostics.len(),
                "target_diagnostic_count": target_view.diagnostics.len(),
            }));
        }
    }

    let status = if checked_link_count == 0 && skipped_link_count > 0 {
        "skipped"
    } else if checked_link_count == 0 {
        "no_link"
    } else if unsupported_count > 0 {
        "unsupported"
    } else if deferred_count > 0 {
        "deferred"
    } else {
        "ok"
    };
    let mut row = json!({
        "status": status,
        "link_count": view.links.len(),
        "checked_link_count": checked_link_count,
        "ok_count": ok_count,
        "unsupported_count": unsupported_count,
        "deferred_count": deferred_count,
        "skipped_link_count": skipped_link_count,
        "elapsed_ms": started.elapsed().as_millis(),
        "samples": samples,
    });
    insert_diagnostic_fields(&mut row, &diagnostics);
    row
}

fn rendered_link_target_is_validation_safe(kind: TargetKind) -> bool {
    !matches!(
        kind,
        TargetKind::SsedAddress
            | TargetKind::SsedCrossBookAddress
            | TargetKind::SsedDenseAnchor
            | TargetKind::SsedAuxRecord
            | TargetKind::SsedIosHtmlPage
    )
}

fn rendered_link_target_status(view: &ResolvedTargetView) -> &'static str {
    match view.kind {
        ResolvedTargetKind::Unsupported
            if view
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.contains("_deferred")) =>
        {
            "deferred"
        }
        ResolvedTargetKind::Unsupported => "unsupported",
        ResolvedTargetKind::Deferred => "deferred",
        _ => "ok",
    }
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
            book_id,
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
    first_view_book_id: &BookId,
) -> serde_json::Value {
    rendered_resource_scan_inner(
        library,
        book_id,
        targets,
        limit,
        Some((first_view, first_view_book_id)),
    )
}

fn rendered_resource_scan_inner(
    library: &BookLibrary,
    book_id: &BookId,
    targets: &[NavigationTarget],
    limit: usize,
    first_view: Option<(&ResolvedTargetView, &BookId)>,
) -> serde_json::Value {
    let started = Instant::now();
    let mut checked_target_count = 0usize;
    let mut slowest_target_ms = 0u128;
    let mut slowest_target_index = None;
    for (index, target) in targets.iter().take(limit).enumerate() {
        checked_target_count += 1;
        let target_started = Instant::now();
        let cached_view = (index == 0).then_some(first_view).flatten();
        let owned_routed;
        let (view, view_book_id) = if let Some((view, book_id)) = cached_view {
            (view, book_id)
        } else {
            owned_routed = match library.render_target_routed(
                book_id,
                &target.target,
                &RenderOptions::default(),
            ) {
                Ok(routed) => routed,
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
            (&owned_routed.view, &owned_routed.book_id)
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
        let Some(first_resource) = first_readable_resource_probe(library, view_book_id, view)
        else {
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

fn link_scan_limit_for(format_family: FormatFamily) -> usize {
    match format_family {
        FormatFamily::Ssed
        | FormatFamily::LvedSqlite3
        | FormatFamily::LvlMultiView
        | FormatFamily::Hourei => VALIDATE_LINK_TARGET_SCAN_LIMIT,
        FormatFamily::Unknown => 0,
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
    for kind in search_probe_surface_kinds(metadata.format_family) {
        for surface in surfaces {
            if surface.status != NavigationStatus::Available
                || surface.surface_id == "search"
                || surface.kind != *kind
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

fn search_probe_surface_kinds(format_family: FormatFamily) -> &'static [NavigationSurfaceKind] {
    match format_family {
        FormatFamily::Ssed => &[NavigationSurfaceKind::TitleIndexBrowse],
        FormatFamily::LvedSqlite3 => &[
            NavigationSurfaceKind::TitleIndexBrowse,
            NavigationSurfaceKind::LvedTree,
        ],
        FormatFamily::LvlMultiView => &[
            NavigationSurfaceKind::TitleIndexBrowse,
            NavigationSurfaceKind::LawTree,
            NavigationSurfaceKind::MultiviewTree,
        ],
        FormatFamily::Hourei => &[
            NavigationSurfaceKind::LawTree,
            NavigationSurfaceKind::TitleIndexBrowse,
        ],
        FormatFamily::Unknown => &[NavigationSurfaceKind::TitleIndexBrowse],
    }
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
    for target in surface.actionable_targets() {
        if out.len() >= VALIDATE_SEARCH_PROBE_LABEL_LIMIT {
            break;
        }
        push_probe_label(out, &target.label_text);
    }
}

fn push_probe_label(out: &mut Vec<String>, label: &str) {
    if out.len() >= VALIDATE_SEARCH_PROBE_LABEL_LIMIT {
        return;
    }
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
        FormatFamily::Ssed if metadata_title_contains_cjk(metadata) => &["a", "あ", "新"],
        FormatFamily::LvlMultiView
            if metadata.capabilities.contains(&Capability::LawNavigation) =>
        {
            &["民法", "憲法", "刑法", "a", "あ"]
        }
        FormatFamily::LvlMultiView => &["a", "la", "あ"],
        _ => &["a", "あ"],
    }
}

fn metadata_title_contains_cjk(metadata: &BookMetadata) -> bool {
    metadata.title.as_deref().is_some_and(|title| {
        title.chars().any(|ch| {
            matches!(
                ch as u32,
                0x3040..=0x30ff
                    | 0x3400..=0x4dbf
                    | 0x4e00..=0x9fff
                    | 0xf900..=0xfaff
            )
        })
    })
}

fn search_probe_candidate_queries(
    metadata: &BookMetadata,
    mode: &SearchMode,
    labels: &[String],
) -> Vec<String> {
    let mut candidates = Vec::new();
    if search_probe_prefers_real_labels(metadata.format_family, mode) {
        for preference in 0..=2 {
            for label in labels.iter().filter(|label| {
                !is_default_search_probe_label(label)
                    && search_probe_label_preference_for_mode(label, mode) == preference
            }) {
                push_search_probe_candidate(&mut candidates, label, mode);
            }
        }
        for label in labels
            .iter()
            .filter(|label| is_default_search_probe_label(label))
        {
            push_search_probe_candidate(&mut candidates, label, mode);
        }
    } else {
        for label in labels {
            push_search_probe_candidate(&mut candidates, label, mode);
        }
    }
    candidates
}

fn search_probe_label_preference_for_mode(label: &str, mode: &SearchMode) -> u8 {
    match mode {
        SearchMode::Partial => {
            let query = search_probe_query(label, mode);
            if search_probe_query_looks_like_pronunciation(&query) {
                return 2;
            }
            let original_starts_numeric = search_probe_first_alphanumeric(label)
                .is_some_and(is_search_probe_numeric_like_start);
            let lookup_starts_well = search_probe_lookup_text(label)
                .and_then(|lookup| lookup.chars().next())
                .is_some_and(is_search_probe_preferred_partial_start);
            let query_starts_well = query
                .chars()
                .next()
                .is_some_and(is_search_probe_preferred_partial_start);
            match (
                lookup_starts_well && !original_starts_numeric,
                query_starts_well,
            ) {
                (true, true) => 0,
                (_, true) => 1,
                _ => 2,
            }
        }
        _ => 0,
    }
}

fn search_probe_first_alphanumeric(label: &str) -> Option<char> {
    label.chars().find(|ch| ch.is_alphanumeric())
}

fn search_probe_query_looks_like_pronunciation(query: &str) -> bool {
    query.chars().any(|ch| matches!(ch as u32, 0x0250..=0x02af))
}

fn is_search_probe_preferred_partial_start(ch: char) -> bool {
    ch.is_alphabetic() && !is_search_probe_numeric_like_start(ch)
}

fn is_search_probe_numeric_like_start(ch: char) -> bool {
    ch.is_numeric()
        || matches!(
            ch,
            '〇' | '零'
                | '一'
                | '二'
                | '三'
                | '四'
                | '五'
                | '六'
                | '七'
                | '八'
                | '九'
                | '十'
                | '百'
                | '千'
                | '万'
                | '億'
                | '兆'
        )
}

fn push_search_probe_candidate(candidates: &mut Vec<String>, label: &str, mode: &SearchMode) {
    let query = search_probe_query(label, mode);
    if !search_probe_query_is_useful(&query) || candidates.iter().any(|seen| seen == &query) {
        return;
    }
    candidates.push(query);
}

fn select_validation_search_probe_query(
    library: &BookLibrary,
    book_id: &BookId,
    metadata: &BookMetadata,
    mode: &SearchMode,
    labels: &[String],
) -> String {
    let mut candidates = search_probe_candidate_queries(metadata, mode, labels);
    if candidates.is_empty() {
        candidates.push("a".to_owned());
    }
    let query = candidates[0].clone();
    if !validation_search_probe_should_hit_check(mode) {
        return query;
    }
    for label in validation_search_probe_fallback_labels(metadata, mode) {
        push_search_probe_candidate(&mut candidates, label, mode);
    }
    let mut first_noisy_hit = None;
    for (index, candidate) in candidates.iter().enumerate() {
        match search_probe_query_hit_quality(library, book_id, mode, candidate) {
            Ok(ValidationSearchProbeHitQuality::CleanHit) => return candidate.clone(),
            Ok(ValidationSearchProbeHitQuality::NoisyHit) => {
                first_noisy_hit.get_or_insert_with(|| candidate.clone());
            }
            Ok(ValidationSearchProbeHitQuality::Miss) => {}
            Err(_) if index == 0 => return query,
            Err(_) => {}
        }
    }
    first_noisy_hit.unwrap_or(query)
}

fn validation_search_probe_should_hit_check(mode: &SearchMode) -> bool {
    matches!(
        mode,
        SearchMode::Exact | SearchMode::Forward | SearchMode::Backward
    )
}

fn validation_search_probe_fallback_labels(
    metadata: &BookMetadata,
    mode: &SearchMode,
) -> &'static [&'static str] {
    match (metadata.format_family, mode) {
        (FormatFamily::LvedSqlite3, SearchMode::Forward) => &["a", "あ", "お", "結", "祝"],
        _ => default_search_probe_labels(metadata),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValidationSearchProbeHitQuality {
    Miss,
    NoisyHit,
    CleanHit,
}

fn search_probe_query_hit_quality(
    library: &BookLibrary,
    book_id: &BookId,
    mode: &SearchMode,
    query: &str,
) -> lvcore::Result<ValidationSearchProbeHitQuality> {
    let (page, _) = search_with_empty_cursor_follow(
        library,
        book_id,
        mode,
        query,
        validation_search_probe_hit_check_limit(mode),
    )?;
    Ok(validation_search_probe_hit_quality(&page))
}

fn validation_search_probe_hit_check_limit(mode: &SearchMode) -> usize {
    match mode {
        SearchMode::Forward => 3,
        _ => 1,
    }
}

fn validation_search_probe_hit_quality(page: &SearchPage) -> ValidationSearchProbeHitQuality {
    if page.hits.is_empty() {
        return ValidationSearchProbeHitQuality::Miss;
    }
    if page.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code.as_str(),
            "ssed_title_label_search_fallback_skipped_short_query"
                | "ssed_title_label_search_fallback_limited"
        )
    }) {
        return ValidationSearchProbeHitQuality::NoisyHit;
    }
    ValidationSearchProbeHitQuality::CleanHit
}

fn search_probe_prefers_real_labels(_format_family: FormatFamily, mode: &SearchMode) -> bool {
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
    matches!(label, "a" | "あ" | "新")
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
    if search_probe_lookup_is_nonword_prefix(trimmed)
        && let Some(run) = search_probe_first_useful_run(title)
        && run != trimmed
    {
        return Some(run);
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
    if search_probe_lookup_is_nonword_prefix(trimmed)
        && let Some(run) = search_probe_first_useful_run(title)
        && run != trimmed
    {
        return Some(run);
    }
    let chars = trimmed.chars().rev().take(2).collect::<Vec<_>>();
    if chars.is_empty() {
        None
    } else {
        let suffix = chars.into_iter().rev().collect::<String>();
        (!suffix.chars().all(char::is_numeric)).then_some(suffix)
    }
}

fn search_probe_partial_text(title: &str) -> Option<String> {
    search_probe_first_useful_run(title)
        .map(|run| {
            search_probe_ascii_alnum_slice(&run).unwrap_or_else(|| run.chars().take(2).collect())
        })
        .or_else(|| search_probe_prefix(title))
}

fn search_probe_first_useful_run(title: &str) -> Option<String> {
    let normalized = search_probe_partial_source_text(title)?;
    let mut current = String::new();
    for ch in normalized.chars().chain(std::iter::once(' ')) {
        if ch.is_alphanumeric() {
            current.push(ch);
            continue;
        }
        if search_probe_run_is_useful(&current) {
            return Some(current);
        }
        current.clear();
    }
    None
}

fn search_probe_partial_source_text(title: &str) -> Option<String> {
    let title = title.trim();
    if let Some((inside, after)) = split_leading_search_probe_bracket(title) {
        if let Some(after_lookup) = search_probe_partial_source_text(after) {
            return Some(after_lookup);
        }
        return search_probe_partial_source_text(inside);
    }
    let trimmed = title
        .trim_start_matches(|ch| is_search_probe_leading_decoration(ch) || ch.is_whitespace())
        .trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn search_probe_run_is_useful(value: &str) -> bool {
    value.chars().count() >= 2 && value.chars().any(char::is_alphabetic)
}

fn search_probe_ascii_alnum_slice(value: &str) -> Option<String> {
    let mut current = String::new();
    for ch in value.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_alphanumeric() {
            current.push(ch);
            continue;
        }
        if current.chars().count() >= 2 {
            return Some(current.chars().take(2).collect());
        }
        current.clear();
    }
    None
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
            | 'º'
            | 'ª'
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
            | '|'
            | '｜'
    )
}

fn search_probe_query(title: &str, mode: &SearchMode) -> String {
    match mode {
        SearchMode::Exact => search_probe_exact_text(title).unwrap_or_else(|| "a".to_owned()),
        SearchMode::Backward => search_probe_suffix(title).unwrap_or_else(|| "a".to_owned()),
        SearchMode::Forward => search_probe_prefix(title).unwrap_or_else(|| "a".to_owned()),
        SearchMode::Partial | SearchMode::FullText | SearchMode::Advanced(_) => {
            search_probe_partial_text(title).unwrap_or_else(|| "a".to_owned())
        }
    }
}

fn search_probe_exact_text(title: &str) -> Option<String> {
    let lookup = search_probe_lookup_text(title)?;
    if search_probe_lookup_is_nonword_prefix(&lookup)
        && let Some(run) = search_probe_first_useful_run(title)
        && run != lookup
    {
        return Some(run);
    }
    if let Some(full_label) = search_probe_full_label_text(title)
        && full_label != lookup
        && search_probe_exact_should_use_full_label(&full_label, &lookup)
    {
        return Some(full_label);
    }
    Some(lookup)
}

fn search_probe_lookup_is_nonword_prefix(value: &str) -> bool {
    value.chars().count() > 1 && !value.chars().any(char::is_alphabetic)
}

fn search_probe_full_label_text(title: &str) -> Option<String> {
    let text = search_probe_partial_source_text(title)?;
    let text = text
        .split(['|', '｜'])
        .next()
        .unwrap_or("")
        .trim()
        .to_owned();
    (!text.is_empty()).then_some(text)
}

fn search_probe_exact_should_use_full_label(value: &str, lookup: &str) -> bool {
    if value
        .strip_prefix(lookup)
        .map(str::trim_start)
        .is_some_and(|suffix| suffix.starts_with('<') || suffix.starts_with('＜'))
    {
        return false;
    }
    value.chars().next().is_some_and(|ch| !ch.is_alphabetic())
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

    use lvcore::TargetToken;
    use rusqlite::Connection;
    use tempfile::tempdir;

    #[test]
    fn validate_deep_surface_probe_samples_unique_surface_ids() {
        let mut probed = BTreeSet::new();
        let first_aux = HomeSurface {
            surface_id: "aux-index:0".to_owned(),
            kind: NavigationSurfaceKind::AuxiliaryIndex,
            status: NavigationStatus::Available,
            title_html: "1991".to_owned(),
            title_text: "1991".to_owned(),
            target: None,
            href: None,
            diagnostics: Vec::new(),
        };
        let second_aux = HomeSurface {
            surface_id: "aux-index:1".to_owned(),
            kind: NavigationSurfaceKind::AuxiliaryIndex,
            status: NavigationStatus::Available,
            title_html: "1992".to_owned(),
            title_text: "1992".to_owned(),
            target: None,
            href: None,
            diagnostics: Vec::new(),
        };
        let duplicate_aux = HomeSurface {
            surface_id: "aux-index:1".to_owned(),
            kind: NavigationSurfaceKind::AuxiliaryIndex,
            status: NavigationStatus::Available,
            title_html: "1992 duplicate".to_owned(),
            title_text: "1992 duplicate".to_owned(),
            target: None,
            href: None,
            diagnostics: Vec::new(),
        };
        let hanrei = HomeSurface {
            surface_id: "hanrei".to_owned(),
            kind: NavigationSurfaceKind::Hanrei,
            status: NavigationStatus::Available,
            title_html: "凡例".to_owned(),
            title_text: "凡例".to_owned(),
            target: None,
            href: None,
            diagnostics: Vec::new(),
        };

        assert!(should_probe_home_surface(&mut probed, &first_aux));
        assert!(should_probe_home_surface(&mut probed, &second_aux));
        assert!(!should_probe_home_surface(&mut probed, &duplicate_aux));
        assert!(should_probe_home_surface(&mut probed, &hanrei));
    }

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
        assert_eq!(search_probe_query("ºO1, ºo", &SearchMode::Exact), "O");
        assert_eq!(search_probe_query("0＜sze zro＞", &SearchMode::Exact), "0");
        assert_eq!(search_probe_query("3D", &SearchMode::Exact), "3D");
        assert_eq!(search_probe_query("a, A", &SearchMode::Exact), "a");
        assert_eq!(search_probe_query("А, а1", &SearchMode::Exact), "А");
        assert_eq!(search_probe_query("***a", &SearchMode::Exact), "a");
        assert_eq!(search_probe_query("★重要", &SearchMode::Exact), "重要");
        assert_eq!(
            search_probe_query(
                "0歳平均余命｜ゼロサイヘイキンヨメイ｜0歳平均余命",
                &SearchMode::Exact
            ),
            "0歳平均余命"
        );
        assert_eq!(
            search_probe_query("0゜ 人工歯 ＜zero degree teeth＞", &SearchMode::Exact),
            "人工歯"
        );
        assert_eq!(
            search_probe_query(".com company", &SearchMode::Exact),
            ".com company"
        );
        assert_eq!(
            search_probe_query("０°人工歯(zero degree teeth)", &SearchMode::Exact),
            "０°人工歯(zero degree teeth)"
        );
        assert_eq!(
            search_probe_query("0030.05 ～", &SearchMode::Exact),
            "0030.05 ～"
        );
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
        assert_eq!(search_probe_query("ºO1, ºo", &SearchMode::Forward), "O");
        assert_eq!(
            search_probe_query("0゜ 人工歯 ＜zero degree teeth＞", &SearchMode::Forward),
            "人工歯"
        );
        assert_eq!(search_probe_query("【角】", &SearchMode::Forward), "角");
        assert_eq!(
            search_probe_query("◎日本国憲法", &SearchMode::Backward),
            "憲法"
        );
        assert_eq!(
            search_probe_query("0゜ 人工歯 ＜zero degree teeth＞", &SearchMode::Backward),
            "人工歯"
        );
        assert_eq!(
            search_probe_suffix("0030.05 ～"),
            None,
            "numeric classification suffixes are poor backward-search probes"
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
            search_probe_query("0゜ 人工歯 ＜zero degree teeth＞", &SearchMode::Partial),
            "人工"
        );
        assert_eq!(
            search_probe_query("０°人工歯(zero degree teeth)", &SearchMode::FullText),
            "人工"
        );
        assert_eq!(search_probe_query("ºO1, ºo", &SearchMode::FullText), "O1");
        assert_eq!(
            search_probe_query("0＜síze zéro＞", &SearchMode::FullText),
            "ze"
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
        assert!(search_probe_prefers_real_labels(
            FormatFamily::Ssed,
            &SearchMode::Exact
        ));
        assert!(search_probe_prefers_real_labels(
            FormatFamily::Ssed,
            &SearchMode::Forward
        ));
        assert!(search_probe_prefers_real_labels(
            FormatFamily::Ssed,
            &SearchMode::Backward
        ));
        assert!(search_probe_prefers_real_labels(
            FormatFamily::Ssed,
            &SearchMode::Partial
        ));
        assert!(search_probe_prefers_real_labels(
            FormatFamily::Ssed,
            &SearchMode::FullText
        ));
        assert!(search_probe_prefers_real_labels(
            FormatFamily::LvedSqlite3,
            &SearchMode::Exact
        ));
        assert!(search_probe_prefers_real_labels(
            FormatFamily::LvedSqlite3,
            &SearchMode::Backward
        ));
        assert!(search_probe_prefers_real_labels(
            FormatFamily::Ssed,
            &SearchMode::Advanced("advanced1".to_owned())
        ));
        assert!(is_default_search_probe_label("新"));
    }

    #[test]
    fn validation_search_probe_hit_quality_marks_fallback_diagnostics_noisy() {
        assert_eq!(
            validation_search_probe_hit_check_limit(&SearchMode::Forward),
            3
        );
        assert_eq!(
            validation_search_probe_hit_check_limit(&SearchMode::Exact),
            1
        );

        let clean = validation_search_probe_page(vec![validation_search_probe_hit()], Vec::new());
        assert_eq!(
            validation_search_probe_hit_quality(&clean),
            ValidationSearchProbeHitQuality::CleanHit
        );

        let noisy = validation_search_probe_page(
            vec![validation_search_probe_hit()],
            vec![Diagnostic::info(
                "ssed_title_label_search_fallback_skipped_short_query",
                "short query",
            )],
        );
        assert_eq!(
            validation_search_probe_hit_quality(&noisy),
            ValidationSearchProbeHitQuality::NoisyHit
        );

        let limited = validation_search_probe_page(
            vec![validation_search_probe_hit()],
            vec![Diagnostic::info(
                "ssed_title_label_search_fallback_limited",
                "bounded fallback page",
            )],
        );
        assert_eq!(
            validation_search_probe_hit_quality(&limited),
            ValidationSearchProbeHitQuality::NoisyHit
        );

        let miss = validation_search_probe_page(Vec::new(), Vec::new());
        assert_eq!(
            validation_search_probe_hit_quality(&miss),
            ValidationSearchProbeHitQuality::Miss
        );
    }

    fn validation_search_probe_page(
        hits: Vec<SearchHit>,
        diagnostics: Vec<Diagnostic>,
    ) -> SearchPage {
        SearchPage {
            hits,
            next_cursor: None,
            result_sequence: None,
            diagnostics,
        }
    }

    fn validation_search_probe_hit() -> SearchHit {
        SearchHit {
            book_id: BookId("TEST".to_owned()),
            target: TargetToken::from_opaque("test"),
            href: String::new(),
            title_html: "hit".to_owned(),
            title_text: "hit".to_owned(),
            snippet_html: None,
            sequence_hint: None,
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn validation_partial_probe_prefers_headword_labels_over_leading_punctuation() {
        let metadata = BookMetadata {
            book_id: BookId("SSED:TEST".to_owned()),
            format_family: FormatFamily::Ssed,
            format_label: "SSED".to_owned(),
            package_root: PathBuf::from("test"),
            title: Some("現代用語".to_owned()),
            root_fingerprint: "test".to_owned(),
            capabilities: Vec::new(),
            search_modes: Vec::new(),
            diagnostics: Vec::new(),
        };
        let labels = vec![
            "a".to_owned(),
            "あ".to_owned(),
            "――曙光が".to_owned(),
            "キャッシュバランス型企業年金".to_owned(),
            "「007 は殺しの番号」".to_owned(),
            "ひゃくとおばん【110 番】".to_owned(),
            "100% Pure Java".to_owned(),
            "A(１), a(１) /eɪ/".to_owned(),
            "alpha".to_owned(),
            "１型糖尿病［がたとうにょうびょう］の治療［ちりょう］".to_owned(),
            "Abdominal injury".to_owned(),
        ];

        let partial = search_probe_candidate_queries(&metadata, &SearchMode::Partial, &labels);
        assert_eq!(partial[0], "キャ");
        assert_eq!(partial[1], "ひゃ");
        assert_eq!(partial[2], "al");
        assert_eq!(partial[3], "Ab");
        let pronunciation = partial.iter().position(|query| query == "eɪ").unwrap();
        assert!(pronunciation > 3);
        assert!(partial[..4].iter().all(|query| query != "eɪ"));

        let exact = search_probe_candidate_queries(&metadata, &SearchMode::Exact, &labels);
        assert_eq!(exact[0], "――曙光が");
    }

    #[test]
    fn validation_search_probe_surfaces_skip_navigation_chrome_for_ssed() {
        let ssed_kinds = search_probe_surface_kinds(FormatFamily::Ssed);
        assert_eq!(ssed_kinds, &[NavigationSurfaceKind::TitleIndexBrowse]);
        assert!(!ssed_kinds.contains(&NavigationSurfaceKind::Panel));
        assert!(!ssed_kinds.contains(&NavigationSurfaceKind::Menu));
        assert!(!ssed_kinds.contains(&NavigationSurfaceKind::MultiSelector));

        let lved_kinds = search_probe_surface_kinds(FormatFamily::LvedSqlite3);
        assert!(lved_kinds.contains(&NavigationSurfaceKind::TitleIndexBrowse));
        assert!(lved_kinds.contains(&NavigationSurfaceKind::LvedTree));

        let law_kinds = search_probe_surface_kinds(FormatFamily::Hourei);
        assert!(law_kinds.contains(&NavigationSurfaceKind::LawTree));
        assert!(!law_kinds.contains(&NavigationSurfaceKind::Panel));
    }

    #[test]
    fn validation_search_probe_labels_are_hard_bounded() {
        let mut labels = Vec::new();
        for index in 0..(VALIDATE_SEARCH_PROBE_LABEL_LIMIT + 4) {
            push_probe_label(&mut labels, &format!("probe{index}"));
        }
        assert_eq!(labels.len(), VALIDATE_SEARCH_PROBE_LABEL_LIMIT);
        assert_eq!(labels.last().map(String::as_str), Some("probe47"));
    }

    #[test]
    fn ssed_default_search_probes_are_script_aware() {
        let mut metadata = BookMetadata {
            book_id: BookId("SSED:TEST".to_owned()),
            format_family: FormatFamily::Ssed,
            format_label: "SSED".to_owned(),
            package_root: PathBuf::from("test"),
            title: Some("角川新字源".to_owned()),
            root_fingerprint: "test".to_owned(),
            capabilities: Vec::new(),
            search_modes: Vec::new(),
            diagnostics: Vec::new(),
        };

        assert_eq!(default_search_probe_labels(&metadata), &["a", "あ", "新"]);

        metadata.title = Some("Readers Special".to_owned());
        assert_eq!(default_search_probe_labels(&metadata), &["a", "あ"]);
    }

    #[test]
    fn validate_deep_surface_probe_uses_bounded_pages() {
        let dir = tempdir().unwrap();
        write_many_row_lved_fixture(dir.path(), 20);

        let row = validate_package_json(
            &DriverRegistry::default(),
            dir.path(),
            ValidateOptions { deep: true },
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
        assert_eq!(lved_list["cursor_probe"]["status"], "ok");
        assert_eq!(lved_list["cursor_probe"]["cursor_kind"], "page");
        assert_eq!(lved_list["cursor_probe"]["cursor"], "16");
        assert_eq!(lved_list["cursor_probe"]["visible_item_count"], 4);
    }

    #[test]
    fn validate_deep_search_probe_checks_remaining_cursor() {
        let dir = tempdir().unwrap();
        write_many_row_lved_fixture(dir.path(), 20);

        let row = validate_package_json(
            &DriverRegistry::default(),
            dir.path(),
            ValidateOptions { deep: true },
        );
        let search_forward = row["exercises"]
            .as_array()
            .unwrap()
            .iter()
            .find(|exercise| exercise["kind"] == "search_forward")
            .expect("expected forward search validation row");

        assert_eq!(search_forward["status"], "ok");
        assert_eq!(search_forward["remaining_cursor"], "3");
        assert_eq!(search_forward["cursor_probe"]["status"], "ok");
        assert_eq!(search_forward["cursor_probe"]["cursor"], "3");
        assert_eq!(search_forward["cursor_probe"]["hit_count"], 1);
    }

    #[test]
    fn validate_search_cursor_probe_skips_expensive_fulltext_body_cursors() {
        assert!(!should_probe_search_cursor(&SearchMode::FullText, "body:0"));
        assert!(!should_probe_search_cursor(
            &SearchMode::FullText,
            "sidecar-body:0"
        ));
        assert!(should_probe_search_cursor(
            &SearchMode::FullText,
            "sidecar-body-row:626f64792e6462:745f636f6e74656e7473:665f446174614964:direct:32"
        ));
        assert!(should_probe_search_cursor(
            &SearchMode::FullText,
            "title:ssed-partial-index:2:126"
        ));
        assert!(!should_probe_search_cursor(
            &SearchMode::FullText,
            "body-offset:484f4e4d4f4e2e444943:1000"
        ));
        assert!(should_probe_search_cursor(
            &SearchMode::Partial,
            "ssed-partial-nonprefix-index:0:0"
        ));
        assert!(!should_probe_search_cursor(
            &SearchMode::Partial,
            "ssed-partial-nonprefix-unverified-index:0:0"
        ));
        assert!(!should_probe_search_cursor(
            &SearchMode::Partial,
            "ssed-partial-nonprefix-noskip-unverified-physical-offset:8:121:1"
        ));
        assert_eq!(
            skipped_search_cursor_probe_reason(
                &SearchMode::Partial,
                "ssed-partial-nonprefix-noskip-unverified-physical-offset:8:121:1"
            ),
            "unverified partial non-prefix continuation may scan large SSED indexes"
        );
        assert!(!should_probe_search_cursor(
            &SearchMode::Backward,
            "ssed-offset-unverified:2"
        ));
        assert_eq!(
            skipped_search_cursor_probe_reason(&SearchMode::Backward, "ssed-offset-unverified:2"),
            "unverified native offset continuation may scan large SSED indexes"
        );
        assert!(!should_probe_search_cursor(
            &SearchMode::Partial,
            "ssed-partial-prefix:ssed-offset-unverified:2"
        ));
        assert_eq!(
            skipped_search_cursor_probe_reason(
                &SearchMode::Partial,
                "ssed-partial-prefix:ssed-offset-unverified:2"
            ),
            "unverified native offset continuation may scan large SSED indexes"
        );
        assert!(!should_probe_search_cursor(
            &SearchMode::Backward,
            "ssed-title-label-unverified:2"
        ));
        assert_eq!(
            skipped_search_cursor_probe_reason(
                &SearchMode::Backward,
                "ssed-title-label-unverified:2"
            ),
            "unverified title-label fallback continuation may scan large SSED indexes"
        );
        assert!(!should_probe_search_cursor(
            &SearchMode::Partial,
            "ssed-partial-prefix:ssed-title-label-unverified:2"
        ));
        assert_eq!(
            skipped_search_cursor_probe_reason(
                &SearchMode::Partial,
                "ssed-partial-prefix:ssed-title-label-unverified:2"
            ),
            "unverified title-label fallback continuation may scan large SSED indexes"
        );
        assert!(!should_probe_search_cursor(
            &SearchMode::Exact,
            "sidecar-title-unverified-row:626f64792e6462:745f636f6e7473:665f446174614964:direct:32"
        ));
        assert_eq!(
            skipped_search_cursor_probe_reason(
                &SearchMode::Exact,
                "sidecar-title-unverified-row:626f64792e6462:745f636f6e7473:665f446174614964:direct:32"
            ),
            "unverified sidecar title continuation may scan large SSED sidecars"
        );
        assert!(!should_probe_search_cursor(
            &SearchMode::Partial,
            "ssed-partial-prefix:sidecar-title-unverified-row:626f64792e6462:745f636f6e7473:665f446174614964:direct:32"
        ));
        assert!(!should_probe_search_cursor(
            &SearchMode::FullText,
            "title-nonprefix-unverified:c3NlZC1wYXJ0aWFsLW5vbnByZWZpeC1pbmRleDo2Ojk0Ngo="
        ));
        assert_eq!(
            skipped_search_cursor_probe_reason(
                &SearchMode::FullText,
                "title-nonprefix-unverified:c3NlZC1wYXJ0aWFsLW5vbnByZWZpeC1pbmRleDo2Ojk0Ngo="
            ),
            "unverified full-text non-prefix title continuation may scan large SSED indexes"
        );
        assert!(!should_probe_search_cursor(
            &SearchMode::FullText,
            "lved-offset-unverified:2"
        ));
        assert_eq!(
            skipped_search_cursor_probe_reason(&SearchMode::FullText, "lved-offset-unverified:2"),
            "unverified LVED offset continuation may repeat broad SQLite searches"
        );
    }

    #[test]
    fn validation_lved_cross_book_refs_expose_dict_codes() {
        assert_eq!(
            lved_cross_book_dict_code_from_ref("lved.contentlink:MEIKYOU3.00083000"),
            Some("MEIKYOU3")
        );
        assert_eq!(
            lved_cross_book_dict_code_from_ref("lved.dataid.dict.STEDMAN6:0001443054#0001443054"),
            Some("STEDMAN6")
        );
        assert_eq!(
            lved_cross_book_dict_code_from_ref("lved.bookmark:1#2"),
            None
        );
    }

    #[test]
    fn validation_sibling_package_root_checks_direct_collection_parent() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("_DCT_SOURCE");
        let destination = dir.path().join("_DCT_BUREI");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();

        assert_eq!(
            validation_sibling_package_root(&source, "BUREI").as_deref(),
            Some(destination.as_path())
        );
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
    fn validate_link_scans_include_ssed_but_skip_hc_views() {
        assert_eq!(
            link_scan_limit_for(FormatFamily::Ssed),
            VALIDATE_LINK_TARGET_SCAN_LIMIT
        );
        assert_eq!(
            link_scan_limit_for(FormatFamily::LvedSqlite3),
            VALIDATE_LINK_TARGET_SCAN_LIMIT
        );
        assert_eq!(
            link_scan_limit_for(FormatFamily::LvlMultiView),
            VALIDATE_LINK_TARGET_SCAN_LIMIT
        );
        assert_eq!(
            link_scan_limit_for(FormatFamily::Hourei),
            VALIDATE_LINK_TARGET_SCAN_LIMIT
        );
        assert_eq!(link_scan_limit_for(FormatFamily::Unknown), 0);

        let target = lvcore::TargetToken::new(&lvcore::InternalTarget::Unsupported {
            reason: "fixture".to_owned(),
        })
        .unwrap();
        let mut view = ResolvedTargetView {
            kind: ResolvedTargetKind::EntryBody,
            target,
            href: String::new(),
            title: None,
            display_html: None,
            basic_text: None,
            scroll_anchor: None,
            surface: None,
            resources: Vec::new(),
            links: Vec::new(),
            capabilities: Vec::new(),
            diagnostics: Vec::new(),
            debug_trace: None,
        };
        assert_eq!(
            rendered_link_scan_limit_for_view(&view, VALIDATE_LINK_TARGET_SCAN_LIMIT),
            VALIDATE_LINK_TARGET_SCAN_LIMIT
        );

        view.diagnostics.push(lvcore::Diagnostic::info(
            "hc_renderer_input_ready",
            "fixture HC diagnostic",
        ));
        assert_eq!(
            rendered_link_scan_limit_for_view(&view, VALIDATE_LINK_TARGET_SCAN_LIMIT),
            0
        );

        assert!(!rendered_link_target_is_validation_safe(
            TargetKind::SsedAddress
        ));
        assert!(!rendered_link_target_is_validation_safe(
            TargetKind::SsedCrossBookAddress
        ));
        assert!(!rendered_link_target_is_validation_safe(
            TargetKind::SsedDenseAnchor
        ));
        assert!(!rendered_link_target_is_validation_safe(
            TargetKind::SsedAuxRecord
        ));
        assert!(!rendered_link_target_is_validation_safe(
            TargetKind::SsedIosHtmlPage
        ));
        assert!(rendered_link_target_is_validation_safe(TargetKind::LvedRow));
        assert!(rendered_link_target_is_validation_safe(
            TargetKind::HoureiLaw
        ));
    }

    #[test]
    fn validate_generic_html_probe_skips_large_native_views_only() {
        assert_eq!(generic_html_probe_skip_reason(4096, 4, 4096), None);
        assert_eq!(
            generic_html_probe_skip_reason(VALIDATE_GENERIC_HTML_NATIVE_HTML_LIMIT + 1, 4, 4096),
            Some("native_display_html_too_large")
        );
        assert_eq!(
            generic_html_probe_skip_reason(4096, VALIDATE_GENERIC_HTML_RESOURCE_LIMIT + 1, 4096),
            Some("resource_count_too_large")
        );
        assert_eq!(
            generic_html_probe_skip_reason(4096, 4, VALIDATE_GENERIC_HTML_RESOURCE_BYTES_LIMIT + 1),
            Some("resource_bytes_too_large")
        );
    }

    #[test]
    fn validate_render_mode_probe_skips_mode_invariant_surface_views() {
        for kind in [
            ResolvedTargetKind::NavigationSurface,
            ResolvedTargetKind::PanelSurface,
            ResolvedTargetKind::Deferred,
        ] {
            assert_eq!(
                render_mode_contract_skip_reason(kind),
                Some("mode_invariant_surface")
            );
        }

        assert_eq!(
            render_mode_contract_skip_reason(ResolvedTargetKind::EntryBody),
            None
        );
        assert_eq!(
            render_mode_contract_skip_reason(ResolvedTargetKind::InfoPage),
            None
        );
    }

    #[test]
    fn validate_reports_package_metadata_diagnostics() {
        let dir = tempdir().unwrap();
        write_many_row_lved_fixture(dir.path(), 2);
        fs::write(
            dir.path().join("COLSCR.DIC"),
            sseddata_literal_fixture(b"retained"),
        )
        .unwrap();

        let row = validate_package_json(
            &DriverRegistry::default(),
            dir.path(),
            ValidateOptions { deep: false },
        );

        assert_eq!(row["status"], "ok");
        assert_eq!(row["diagnostics"].as_array().unwrap().len(), 1);
        assert_eq!(
            row["diagnostics"][0]["code"],
            "retained_ssed_component_deferred"
        );
        assert_eq!(row["diagnostics"][0]["context"]["filename"], "COLSCR.DIC");
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
}

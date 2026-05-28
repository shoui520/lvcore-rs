use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Parser, Subcommand, ValueEnum};
use lvcore::{
    BookId, BookLibrary, BookMetadata, DriverRegistry, HomeSurface, NavigationStatus,
    NavigationSurface, RenderMode, RenderOptions, ResourceToken, Result, SearchMode, SearchQuery,
    SearchScope, TargetToken, lved_sqlite::is_lved_payload_name,
};
use serde_json::json;

#[derive(Debug, Parser)]
#[command(name = "lvcore")]
#[command(about = "Developer CLI for the lvcore reader library")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Detect package families and print stable metadata as JSON.
    Detect {
        /// Package root or payload path to inspect.
        path: PathBuf,
    },
    /// Recursively open packages and exercise reader-facing metadata/surfaces.
    Validate {
        /// Package roots or corpus roots to inspect.
        paths: Vec<PathBuf>,
        /// Stop after this many discovered packages.
        #[arg(long)]
        max: Option<usize>,
        /// Also open available surfaces, render their first target, and run a small search.
        #[arg(long)]
        deep: bool,
        /// Stream one JSON object per package as soon as it is validated.
        #[arg(long)]
        jsonl: bool,
    },
    /// Open one package, run native search, and optionally render the first hit.
    Search {
        /// Package root or payload path to inspect.
        path: PathBuf,
        /// Query text.
        query: String,
        /// Search mode to run.
        #[arg(long, default_value = "forward")]
        mode: CliSearchMode,
        /// LVED advanced FTS column to search, for example `advanced1` or `advanced2`.
        #[arg(long)]
        advanced_column: Option<String>,
        /// Maximum hits to return.
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Opaque cursor from a previous search page.
        #[arg(long)]
        cursor: Option<String>,
        /// Resolve and render the first hit.
        #[arg(long)]
        render_first: bool,
        /// Render mode to use with --render-first or continuous view output.
        #[arg(long, default_value = "native")]
        render_mode: CliRenderMode,
        /// Include backend debug trace in rendered output.
        #[arg(long)]
        debug_trace: bool,
        /// Resolve a continuous-view window before the first hit.
        #[arg(long, default_value_t = 0)]
        window_before: usize,
        /// Resolve a continuous-view window after the first hit.
        #[arg(long, default_value_t = 0)]
        window_after: usize,
    },
    /// Open a reader navigation surface for one package.
    Surface {
        /// Package root or payload path to inspect.
        path: PathBuf,
        /// Surface identifier, for example `lved-list`, `info`, or `title-index`.
        surface_id: String,
        /// Opaque cursor from a previous paged surface.
        #[arg(long)]
        cursor: Option<String>,
        /// Maximum surface items to return.
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Resolve and render one opaque target token for one package.
    Render {
        /// Package root or payload path to inspect.
        path: PathBuf,
        /// Target token previously returned by search, navigation, or links.
        token: String,
        /// Render mode to request.
        #[arg(long, default_value = "native")]
        mode: CliRenderMode,
        /// Include backend debug trace in rendered output.
        #[arg(long)]
        debug_trace: bool,
    },
    /// Resolve one opaque target token into backend-owned renderer input.
    RendererInput {
        /// Package root or payload path to inspect.
        path: PathBuf,
        /// Target token previously returned by search, navigation, or links.
        token: String,
    },
    /// Resolve and read one opaque resource token for one package.
    Resource {
        /// Package root or payload path to inspect.
        path: PathBuf,
        /// Resource token previously returned by rendered views or navigation labels.
        token: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliSearchMode {
    Exact,
    Forward,
    Backward,
    Partial,
    Fulltext,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliRenderMode {
    Native,
    GenericHtml,
    BasicText,
    Debug,
}

impl From<CliSearchMode> for SearchMode {
    fn from(value: CliSearchMode) -> Self {
        match value {
            CliSearchMode::Exact => Self::Exact,
            CliSearchMode::Forward => Self::Forward,
            CliSearchMode::Backward => Self::Backward,
            CliSearchMode::Partial => Self::Partial,
            CliSearchMode::Fulltext => Self::FullText,
        }
    }
}

fn cli_search_mode(mode: CliSearchMode, advanced_column: Option<String>) -> SearchMode {
    match advanced_column {
        Some(column) if !column.trim().is_empty() => SearchMode::Advanced(column.trim().to_owned()),
        _ => mode.into(),
    }
}

impl From<CliRenderMode> for RenderMode {
    fn from(value: CliRenderMode) -> Self {
        match value {
            CliRenderMode::Native => Self::Native,
            CliRenderMode::GenericHtml => Self::GenericHtml,
            CliRenderMode::BasicText => Self::BasicText,
            CliRenderMode::Debug => Self::Debug,
        }
    }
}

fn cli_render_options(mode: CliRenderMode, debug_trace: bool) -> RenderOptions {
    RenderOptions {
        mode: mode.into(),
        include_debug_trace: debug_trace,
        ..RenderOptions::default()
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Detect { path } => {
            let registry = DriverRegistry::default();
            let detected = registry.detect(&path)?;
            println!("{}", serde_json::to_string_pretty(&detected)?);
        }
        Command::Validate {
            paths,
            max,
            deep,
            jsonl,
        } => {
            let registry = DriverRegistry::default();
            let mut package_paths = Vec::new();
            for path in paths {
                discover_packages(&registry, &path, max, &mut package_paths)?;
                if max.is_some_and(|max| package_paths.len() >= max) {
                    break;
                }
            }

            let mut rows = Vec::new();
            for path in package_paths {
                eprintln!("lvcore: validating {}", path.display());
                let started = Instant::now();
                let mut row = validate_package_json(&registry, &path, deep);
                if let Some(object) = row.as_object_mut() {
                    object.insert(
                        "elapsed_ms".to_owned(),
                        json!(started.elapsed().as_millis()),
                    );
                }
                if jsonl {
                    println!("{}", serde_json::to_string(&row)?);
                    io::stdout().flush()?;
                } else {
                    rows.push(row);
                }
            }
            if !jsonl {
                println!("{}", serde_json::to_string_pretty(&rows)?);
            }
        }
        Command::Search {
            path,
            query,
            mode,
            advanced_column,
            limit,
            cursor,
            render_first,
            render_mode,
            debug_trace,
            window_before,
            window_after,
        } => {
            let registry = DriverRegistry::default();
            let output = search_command_json(
                &registry,
                &path,
                query,
                cli_search_mode(mode, advanced_column),
                limit,
                cursor,
                cli_render_options(render_mode, debug_trace),
                render_first,
                window_before,
                window_after,
            )?;
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        Command::Surface {
            path,
            surface_id,
            cursor,
            limit,
        } => {
            let registry = DriverRegistry::default();
            let (library, book_id) = open_single_book_library(&registry, &path)?;
            let metadata = metadata_for(&library, &book_id);
            let surface =
                library.open_surface_page(&book_id, &surface_id, cursor.as_deref(), limit)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "metadata": metadata,
                    "cursor": cursor,
                    "limit": limit,
                    "surface": surface,
                }))?
            );
        }
        Command::Render {
            path,
            token,
            mode,
            debug_trace,
        } => {
            let registry = DriverRegistry::default();
            let (library, book_id) = open_single_book_library(&registry, &path)?;
            let metadata = metadata_for(&library, &book_id);
            let target = TargetToken::from_opaque(token);
            let render_options = cli_render_options(mode, debug_trace);
            let view = library.render_target(&book_id, &target, &render_options)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "metadata": metadata,
                    "render_options": render_options,
                    "view": view,
                }))?
            );
        }
        Command::RendererInput { path, token } => {
            let registry = DriverRegistry::default();
            let (library, book_id) = open_single_book_library(&registry, &path)?;
            let metadata = metadata_for(&library, &book_id);
            let target = TargetToken::from_opaque(token);
            let input = library.renderer_input_for_target(&book_id, &target)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "metadata": metadata,
                    "renderer_input": input,
                }))?
            );
        }
        Command::Resource { path, token } => {
            let registry = DriverRegistry::default();
            let output = resource_command_json(&registry, &path, token)?;
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }
    Ok(())
}

fn open_single_book_library(
    registry: &DriverRegistry,
    path: &Path,
) -> Result<(BookLibrary, BookId)> {
    let mut library = BookLibrary::new();
    let book_id = library.open_path(path, registry)?;
    Ok((library, book_id))
}

fn metadata_for(library: &BookLibrary, book_id: &BookId) -> BookMetadata {
    library
        .book(book_id)
        .expect("book id returned by open_path must exist")
        .metadata()
        .clone()
}

fn validate_package_json(registry: &DriverRegistry, path: &Path, deep: bool) -> serde_json::Value {
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

#[allow(clippy::too_many_arguments)]
fn search_command_json(
    registry: &DriverRegistry,
    path: &Path,
    query: String,
    mode: SearchMode,
    limit: usize,
    cursor: Option<String>,
    render_options: RenderOptions,
    render_first: bool,
    window_before: usize,
    window_after: usize,
) -> Result<serde_json::Value> {
    let (library, book_id) = open_single_book_library(registry, path)?;
    let metadata = metadata_for(&library, &book_id);
    let page = library.search(&SearchQuery {
        scope: SearchScope::CurrentBook(book_id.clone()),
        mode,
        query,
        cursor,
        limit,
    })?;
    let first_target = page.hits.first().map(|hit| hit.target.clone());
    let rendered_first = if render_first {
        first_target
            .as_ref()
            .map(|target| library.render_target(&book_id, target, &render_options))
            .transpose()?
    } else {
        None
    };
    let target_window = if window_before > 0 || window_after > 0 {
        first_target
            .as_ref()
            .map(|target| {
                library.resolve_target_window(
                    &book_id,
                    target,
                    None,
                    window_before,
                    window_after,
                    &render_options,
                )
            })
            .transpose()?
    } else {
        None
    };
    Ok(json!({
        "metadata": metadata,
        "hits": page.hits,
        "next_cursor": page.next_cursor,
        "diagnostics": page.diagnostics,
        "rendered_first": rendered_first,
        "target_window": target_window,
    }))
}

fn resource_command_json(
    registry: &DriverRegistry,
    path: &Path,
    token: String,
) -> Result<serde_json::Value> {
    let (library, book_id) = open_single_book_library(registry, path)?;
    let metadata = metadata_for(&library, &book_id);
    let resource = ResourceToken::from_opaque(token);
    let resource_ref = library.resolve_resource(&book_id, &resource)?;
    let bytes = library.read_resource(&book_id, &resource)?;
    Ok(json!({
        "metadata": metadata,
        "resource": resource_ref,
        "byte_len": bytes.len(),
    }))
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
        scope: SearchScope::CurrentBook(book_id.clone()),
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

fn discover_packages(
    registry: &DriverRegistry,
    path: &Path,
    max: Option<usize>,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    if max.is_some_and(|max| out.len() >= max) {
        return Ok(());
    }
    if !path.exists() {
        return Ok(());
    }
    if path.is_file() && !is_package_file_candidate(path) {
        return Ok(());
    }
    if path.is_dir() && is_obvious_resource_only_dir(path) {
        return Ok(());
    }
    if is_obvious_package_candidate(path)? && !registry.detect(path)?.is_empty() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    if !path.is_dir() {
        return Ok(());
    }

    let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        discover_packages(registry, &entry.path(), max, out)?;
        if max.is_some_and(|max| out.len() >= max) {
            break;
        }
    }
    Ok(())
}

fn is_package_file_candidate(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    name == "main.data" || name.ends_with(".dbc") || name.ends_with(".idx") || name.ends_with(".db")
}

fn is_obvious_package_candidate(path: &Path) -> Result<bool> {
    if path.is_file() {
        return Ok(is_package_file_candidate(path));
    }
    if !path.is_dir() {
        return Ok(false);
    }
    if path.join("main.data").is_file()
        || directory_has_file_suffix(path, ".dbc")?
        || directory_has_lved_payload(path)?
    {
        return Ok(true);
    }
    if directory_has_file_suffix(path, ".idx")? {
        return Ok(true);
    }
    if path.join("menuData.xml").is_file() && directory_has_multiview_payload(path)? {
        return Ok(true);
    }
    let hourei_required = [
        "_DataBase/hore_base.db",
        "_DataBase/hore_search_a.db",
        "_DataBase/horejo_base.db",
    ];
    Ok(hourei_required
        .iter()
        .all(|relative| path.join(relative).is_file()))
}

fn directory_has_lved_payload(path: &Path) -> Result<bool> {
    if !path.is_dir() {
        return Ok(false);
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path.is_file() && is_lved_payload_name(&entry_path) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_obvious_resource_only_dir(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    name.ends_with("_media")
        || name.ends_with("_sound_files")
        || name.ends_with("_mathjax")
        || name.ends_with("_templates")
        || name == "templates"
        || name == "template"
        || name == "img"
        || name == "images"
        || name == "sound"
        || name == "sounds"
        || name == "mathjax"
}

fn directory_has_file_suffix(path: &Path, suffix: &str) -> Result<bool> {
    if !path.is_dir() {
        return Ok(false);
    }
    let suffix = suffix.to_lowercase();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.path().is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .to_lowercase()
                .ends_with(&suffix)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn directory_has_multiview_payload(path: &Path) -> Result<bool> {
    if !path.is_dir() {
        return Ok(false);
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if !entry.path().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.len() == 6
            && name.as_bytes()[1] == b'l'
            && name.as_bytes()[2] == b'v'
            && (name.ends_with("bat") || name.ends_with("dat"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lvcore::lved_sqlite::apply_sqlcipher_key;
    use rusqlite::Connection;

    #[test]
    fn discovery_ignores_resource_directories_with_non_package_idx_files() {
        let dir = tempfile::tempdir().unwrap();
        let resources = dir.path().join("Viewer.app/Contents/Resources");
        fs::create_dir_all(&resources).unwrap();
        fs::write(resources.join("Localizable.idx"), b"not an SSED catalog").unwrap();

        let mut discovered = Vec::new();
        discover_packages(
            &DriverRegistry::default(),
            dir.path(),
            None,
            &mut discovered,
        )
        .unwrap();

        assert!(discovered.is_empty());
    }

    #[test]
    fn advanced_column_overrides_unit_search_mode() {
        assert_eq!(
            cli_search_mode(CliSearchMode::Forward, Some(" advanced1 ".to_owned())),
            SearchMode::Advanced("advanced1".to_owned())
        );
        assert_eq!(
            cli_search_mode(CliSearchMode::Exact, Some(" ".to_owned())),
            SearchMode::Exact
        );
    }

    #[test]
    fn cli_render_mode_maps_to_reader_render_options() {
        assert_eq!(
            cli_render_options(CliRenderMode::GenericHtml, true),
            RenderOptions {
                mode: RenderMode::GenericHtml,
                include_debug_trace: true,
                ..RenderOptions::default()
            }
        );
        assert_eq!(
            cli_render_options(CliRenderMode::BasicText, false).mode,
            RenderMode::BasicText
        );
    }

    #[test]
    fn search_command_uses_library_scoped_resource_hrefs() {
        let dir = tempfile::tempdir().unwrap();
        write_lved_cli_fixture(dir.path());

        let output = search_command_json(
            &DriverRegistry::default(),
            dir.path(),
            "alp".to_owned(),
            SearchMode::Forward,
            10,
            None,
            RenderOptions::default(),
            true,
            0,
            0,
        )
        .unwrap();

        let title_html = output["hits"][0]["title_html"].as_str().unwrap();
        let display_html = output["rendered_first"]["display_html"].as_str().unwrap();
        assert!(has_scoped_resource_href(title_html));
        assert!(has_scoped_resource_href(display_html));
        assert!(!title_html.contains("src=\"AC6E.svg\""));
        assert!(!display_html.contains("data=\"AC6E.svg\""));
    }

    #[test]
    fn validate_command_reports_advertised_search_modes() {
        let dir = tempfile::tempdir().unwrap();
        write_lved_cli_fixture(dir.path());

        let output = validate_package_json(&DriverRegistry::default(), dir.path(), false);

        assert_eq!(output["status"], "ok");
        assert_eq!(
            output["search_modes"],
            serde_json::json!([
                "exact",
                "forward",
                "backward",
                "partial",
                "full_text",
                { "advanced": "advanced1" },
                { "advanced": "advanced2" },
            ])
        );
    }

    #[test]
    fn search_command_can_render_first_hit_as_basic_text() {
        let dir = tempfile::tempdir().unwrap();
        write_lved_cli_fixture(dir.path());

        let output = search_command_json(
            &DriverRegistry::default(),
            dir.path(),
            "alp".to_owned(),
            SearchMode::Forward,
            10,
            None,
            cli_render_options(CliRenderMode::BasicText, false),
            true,
            0,
            0,
        )
        .unwrap();

        assert!(output["rendered_first"]["display_html"].is_null());
        assert!(
            output["rendered_first"]["basic_text"]
                .as_str()
                .unwrap()
                .contains("body")
        );
    }

    #[test]
    fn resource_command_resolves_rendered_resource_tokens() {
        let dir = tempfile::tempdir().unwrap();
        write_lved_cli_fixture(dir.path());

        let search_output = search_command_json(
            &DriverRegistry::default(),
            dir.path(),
            "alp".to_owned(),
            SearchMode::Forward,
            10,
            None,
            RenderOptions::default(),
            true,
            0,
            0,
        )
        .unwrap();
        let token = search_output["rendered_first"]["resources"][0]["token"]
            .as_str()
            .unwrap()
            .to_owned();

        let resource_output =
            resource_command_json(&DriverRegistry::default(), dir.path(), token).unwrap();

        assert_eq!(resource_output["byte_len"].as_u64(), Some(6));
        assert_eq!(resource_output["resource"]["kind"], "image");
        assert_eq!(resource_output["resource"]["mime_type"], "image/svg+xml");
        assert!(has_scoped_resource_href(
            resource_output["resource"]["href"].as_str().unwrap()
        ));
    }

    fn has_scoped_resource_href(html: &str) -> bool {
        const PREFIX: &str = "lvcore://resource/";
        let Some(start) = html.find(PREFIX) else {
            return false;
        };
        let rest = &html[start + PREFIX.len()..];
        let value = rest
            .split(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '>'))
            .next()
            .unwrap_or_default();
        value.split('/').count() == 2
    }

    fn write_lved_cli_fixture(root: &Path) {
        let payload = root.join("main.data");
        let key = "test-key";
        {
            let connection = Connection::open(&payload).unwrap();
            apply_sqlcipher_key(&connection, key).unwrap();
            connection
                .execute_batch(
                    r#"
                    create table info (id integer, type integer, name text primary key, body text, media text);
                    insert into info values (1, 1, 'about.html', '<h1>Example Dictionary</h1>', '');
                    create table content (id integer primary key, type integer, body text, media text);
                    create table media (id integer primary key, name text, type integer, main blob);
                    create table list (
                      id integer primary key,
                      refid integer,
                      type integer,
                      anchor text,
                      title text,
                      titlesub text
                    );
                    create virtual table search using fts4(
                      forward,
                      back,
                      part,
                      fts,
                      advanced1,
                      advanced2,
                      filter
                    );
                    insert into content values (100, 1, '<article><object data="AC6E.svg"></object><p>body</p></article>', '');
                    insert into media values (1, 'AC6E', 4, X'3C7376672F3E');
                    insert into list values (1, 100, 1, '', '<img src="AC6E.svg"><b>alpha</b>', '');
                    insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                      values (1, 'alpha', 'ahpla', 'alpha', 'alpha body', '', '', '∥alpha∥');
                    "#,
                )
                .unwrap();
        }
        fs::write(root.join("main.key"), key).unwrap();
    }
}

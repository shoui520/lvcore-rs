use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Parser, Subcommand, ValueEnum};
use lvcore::{
    BookId, BookLibrary, BookMetadata, DriverRegistry, Error, HomeSurface, NavigationStatus,
    NavigationSurface, PackageDiscoveryOptions, RenderMode, RenderOptions, ResourceToken, Result,
    SearchMode, SearchQuery, SearchScope, SequenceHint, TargetToken,
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
    /// Open one package and print reader home surfaces.
    Home {
        /// Package root or payload path to inspect.
        path: PathBuf,
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
    /// Open a library/corpus set and run cross-book search.
    LibrarySearch {
        /// Query text.
        query: String,
        /// Package roots, payload paths, or corpus roots to inspect.
        paths: Vec<PathBuf>,
        /// Stop after this many discovered packages.
        #[arg(long)]
        max: Option<usize>,
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
        /// Resolve and render the first hit through library routing.
        #[arg(long)]
        render_first: bool,
        /// Render mode to use with --render-first.
        #[arg(long, default_value = "native")]
        render_mode: CliRenderMode,
        /// Include backend debug trace in rendered output.
        #[arg(long)]
        debug_trace: bool,
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
    /// Resolve a continuous-view window around one target.
    Window {
        /// Package root or payload path to inspect.
        path: PathBuf,
        /// Target token previously returned by search, navigation, or links.
        token: String,
        /// Sequence order to use. Defaults to the package's target-appropriate order.
        #[arg(long)]
        sequence: Option<CliSequenceHint>,
        /// Sequence value for title/menu/panel/search result orders.
        #[arg(long)]
        sequence_value: Option<String>,
        /// Number of previous entries/items to resolve.
        #[arg(long, default_value_t = 0)]
        before: usize,
        /// Number of following entries/items to resolve.
        #[arg(long, default_value_t = 0)]
        after: usize,
        /// Render mode to request for each view in the window.
        #[arg(long, default_value = "native")]
        render_mode: CliRenderMode,
        /// Include backend debug trace in rendered output.
        #[arg(long)]
        debug_trace: bool,
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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliSequenceHint {
    TitleIndexOrder,
    SearchResults,
    BodyOrder,
    MenuOrder,
    PanelOrder,
    LvedListOrder,
    LvedTreeOrder,
    HoureiLawArticleOrder,
    MultiviewTreeOrder,
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

fn cli_sequence_hint(
    hint: Option<CliSequenceHint>,
    value: Option<String>,
) -> Result<Option<SequenceHint>> {
    let Some(hint) = hint else {
        return Ok(None);
    };
    fn required_value(name: &str, value: Option<String>) -> Result<String> {
        let Some(value) = value
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
        else {
            return Err(Error::Driver(format!(
                "--sequence-value is required for {name}"
            )));
        };
        Ok(value)
    }
    Ok(Some(match hint {
        CliSequenceHint::TitleIndexOrder => SequenceHint::TitleIndexOrder {
            value: required_value("title-index-order", value)?,
        },
        CliSequenceHint::SearchResults => SequenceHint::SearchResults {
            value: required_value("search-results", value)?,
        },
        CliSequenceHint::BodyOrder => SequenceHint::BodyOrder,
        CliSequenceHint::MenuOrder => SequenceHint::MenuOrder {
            value: required_value("menu-order", value)?,
        },
        CliSequenceHint::PanelOrder => SequenceHint::PanelOrder {
            value: required_value("panel-order", value)?,
        },
        CliSequenceHint::LvedListOrder => SequenceHint::LvedListOrder,
        CliSequenceHint::LvedTreeOrder => SequenceHint::LvedTreeOrder,
        CliSequenceHint::HoureiLawArticleOrder => SequenceHint::HoureiLawArticleOrder,
        CliSequenceHint::MultiviewTreeOrder => SequenceHint::MultiviewTreeOrder,
    }))
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Detect { path } => {
            let registry = DriverRegistry::default();
            let detected = registry.detect_all(&path, PackageDiscoveryOptions::default())?;
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
                package_paths
                    .extend(registry.discover_roots(&path, PackageDiscoveryOptions { max })?);
                if let Some(limit) = max
                    && package_paths.len() >= limit
                {
                    package_paths.truncate(limit);
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
        Command::Home { path } => {
            let registry = DriverRegistry::default();
            let output = home_command_json(&registry, &path)?;
            println!("{}", serde_json::to_string_pretty(&output)?);
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
        Command::LibrarySearch {
            query,
            paths,
            max,
            mode,
            advanced_column,
            limit,
            cursor,
            render_first,
            render_mode,
            debug_trace,
        } => {
            let registry = DriverRegistry::default();
            let output = library_search_command_json(
                &registry,
                &paths,
                max,
                query,
                cli_search_mode(mode, advanced_column),
                limit,
                cursor,
                cli_render_options(render_mode, debug_trace),
                render_first,
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
        Command::Window {
            path,
            token,
            sequence,
            sequence_value,
            before,
            after,
            render_mode,
            debug_trace,
        } => {
            let registry = DriverRegistry::default();
            let output = window_command_json(
                &registry,
                &path,
                token,
                cli_sequence_hint(sequence, sequence_value)?,
                before,
                after,
                cli_render_options(render_mode, debug_trace),
            )?;
            println!("{}", serde_json::to_string_pretty(&output)?);
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

fn open_library_from_paths(
    registry: &DriverRegistry,
    paths: &[PathBuf],
    max: Option<usize>,
) -> Result<BookLibrary> {
    let mut library = BookLibrary::new();
    library.open_discovered_paths(paths, registry, PackageDiscoveryOptions { max })?;
    Ok(library)
}

fn metadata_for(library: &BookLibrary, book_id: &BookId) -> BookMetadata {
    library
        .book(book_id)
        .expect("book id returned by open_path must exist")
        .metadata()
        .clone()
}

fn home_command_json(registry: &DriverRegistry, path: &Path) -> Result<serde_json::Value> {
    let (library, book_id) = open_single_book_library(registry, path)?;
    let metadata = metadata_for(&library, &book_id);
    let surfaces = library.home_surfaces(&book_id)?;
    Ok(json!({
        "metadata": metadata,
        "surface_count": surfaces.len(),
        "surfaces": surfaces,
    }))
}

#[allow(clippy::too_many_arguments)]
fn library_search_command_json(
    registry: &DriverRegistry,
    paths: &[PathBuf],
    max: Option<usize>,
    query: String,
    mode: SearchMode,
    limit: usize,
    cursor: Option<String>,
    render_options: RenderOptions,
    render_first: bool,
) -> Result<serde_json::Value> {
    let library = open_library_from_paths(registry, paths, max)?;
    let metadata = library.metadata_snapshot();
    let page = library.search(&SearchQuery {
        scope: SearchScope::AllBooks,
        mode,
        query,
        cursor,
        limit,
    })?;
    let rendered_first = if render_first {
        page.hits
            .first()
            .map(|hit| library.render_target_routed(&hit.book_id, &hit.target, &render_options))
            .transpose()?
    } else {
        None
    };
    Ok(json!({
        "books": metadata,
        "book_count": library.len(),
        "hits": page.hits,
        "next_cursor": page.next_cursor,
        "diagnostics": page.diagnostics,
        "rendered_first": rendered_first,
    }))
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
        scope: SearchScope::CurrentBook {
            book_id: book_id.clone(),
        },
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

fn window_command_json(
    registry: &DriverRegistry,
    path: &Path,
    token: String,
    sequence_hint: Option<SequenceHint>,
    before: usize,
    after: usize,
    render_options: RenderOptions,
) -> Result<serde_json::Value> {
    let (library, book_id) = open_single_book_library(registry, path)?;
    let metadata = metadata_for(&library, &book_id);
    let target = TargetToken::from_opaque(token);
    let window = library.resolve_target_window(
        &book_id,
        &target,
        sequence_hint.as_ref(),
        before,
        after,
        &render_options,
    )?;
    Ok(json!({
        "metadata": metadata,
        "sequence_hint": sequence_hint,
        "before": before,
        "after": after,
        "render_options": render_options,
        "window": window,
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

#[cfg(test)]
mod tests {
    use super::*;
    use lvcore::lved_sqlite::apply_sqlcipher_key;
    use rusqlite::Connection;
    use std::fs;

    #[test]
    fn discovery_ignores_resource_directories_with_non_package_idx_files() {
        let dir = tempfile::tempdir().unwrap();
        let resources = dir.path().join("Viewer.app/Contents/Resources");
        fs::create_dir_all(&resources).unwrap();
        fs::write(resources.join("Localizable.idx"), b"not an SSED catalog").unwrap();

        let discovered = DriverRegistry::default()
            .discover_roots(dir.path(), PackageDiscoveryOptions::default())
            .unwrap();

        assert!(discovered.is_empty());
    }

    #[test]
    fn detect_command_recurses_when_root_is_not_a_package() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("NestedDictionary");
        fs::create_dir_all(&package).unwrap();
        write_lved_cli_fixture(&package);

        let detections = DriverRegistry::default()
            .detect_all(dir.path(), PackageDiscoveryOptions::default())
            .unwrap();

        assert_eq!(detections.len(), 1);
        assert_eq!(
            detections[0].format_family,
            lvcore::FormatFamily::LvedSqlite3
        );
        assert_eq!(detections[0].root, package);
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
    fn home_command_reports_metadata_and_surfaces() {
        let dir = tempfile::tempdir().unwrap();
        write_lved_cli_fixture(dir.path());

        let output = home_command_json(&DriverRegistry::default(), dir.path()).unwrap();

        assert_eq!(output["metadata"]["format_family"], "lved_sqlite3");
        assert_eq!(output["surface_count"].as_u64(), Some(4));
        assert!(
            output["surfaces"]
                .as_array()
                .unwrap()
                .iter()
                .any(|surface| surface["surface_id"] == "lved-list"
                    && surface["status"] == "available")
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
    fn library_search_command_uses_all_books_scope_and_routed_rendering() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("FirstDictionary");
        let second = dir.path().join("SecondDictionary");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        write_lved_cli_fixture(&first);
        write_lved_cli_fixture(&second);

        let output = library_search_command_json(
            &DriverRegistry::default(),
            &[dir.path().to_path_buf()],
            None,
            "alp".to_owned(),
            SearchMode::Forward,
            10,
            None,
            RenderOptions::default(),
            true,
        )
        .unwrap();

        assert_eq!(output["book_count"].as_u64(), Some(2));
        assert_eq!(output["hits"].as_array().unwrap().len(), 2);
        assert_eq!(output["rendered_first"]["view"]["kind"], "entry_body");
        assert!(output["rendered_first"]["book_id"].as_str().is_some());
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

    #[test]
    fn window_command_resolves_continuous_view_for_target_tokens() {
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
            false,
            0,
            0,
        )
        .unwrap();
        let target = search_output["hits"][0]["target"]
            .as_str()
            .unwrap()
            .to_owned();

        let output = window_command_json(
            &DriverRegistry::default(),
            dir.path(),
            target,
            Some(SequenceHint::LvedListOrder),
            0,
            1,
            RenderOptions::default(),
        )
        .unwrap();

        assert_eq!(output["window"]["center"]["title"], "alpha");
        assert_eq!(output["window"]["after"].as_array().unwrap().len(), 1);
        assert!(
            output["window"]["after"][0]["display_html"]
                .as_str()
                .unwrap()
                .contains("next body")
        );
        assert_eq!(
            output["sequence_hint"],
            serde_json::json!({ "kind": "lved_list_order" })
        );
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
                    insert into content values (101, 1, '<article><p>next body</p></article>', '');
                    insert into media values (1, 'AC6E', 4, X'3C7376672F3E');
                    insert into list values (1, 100, 1, '', '<img src="AC6E.svg"><b>alpha</b>', '');
                    insert into list values (2, 101, 1, '', '<b>beta</b>', '');
                    insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                      values (1, 'alpha', 'ahpla', 'alpha', 'alpha body', '', '', '∥alpha∥');
                    "#,
                )
                .unwrap();
        }
        fs::write(root.join("main.key"), key).unwrap();
    }
}

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::Instant;

use clap::{Parser, Subcommand, ValueEnum};
use lvcore::{
    BookId, BookLibrary, BookMetadata, DriverRegistry, Error, LibraryImportReport,
    LibraryImportResult, PackageCandidate, PackageDiscoveryOptions, RenderMode, RenderOptions,
    ResourceToken, Result, SearchMode, SearchQuery, SearchScope, SequenceHint, TargetToken,
};
use serde_json::json;

mod validate;

use validate::{validate_detected_package_json, validate_row_has_failure};

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
    /// Recursively discover package candidates without opening/decrypting them.
    LibraryDiscover {
        /// Package roots, payload paths, or corpus roots to inspect.
        paths: Vec<PathBuf>,
        /// Stop after this many discovered package candidates.
        #[arg(long)]
        max: Option<usize>,
        /// Stream one JSON object per candidate as soon as it is discovered.
        #[arg(long)]
        jsonl: bool,
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
        /// Legacy no-op; --deep probes advertised search modes by default.
        #[arg(long)]
        include_expensive_search: bool,
        /// Stream one JSON object per package as soon as it is validated.
        #[arg(long)]
        jsonl: bool,
        /// Exit nonzero if any package or deep exercise reports an error status.
        #[arg(long)]
        fail_on_error: bool,
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
        /// Resolve a cross-book search-result window before the first hit.
        #[arg(long, default_value_t = 0)]
        window_before: usize,
        /// Resolve a cross-book search-result window after the first hit.
        #[arg(long, default_value_t = 0)]
        window_after: usize,
    },
    /// Open a library/corpus set and print frontend-cacheable book metadata.
    LibraryImport {
        /// Package roots, payload paths, or corpus roots to inspect.
        paths: Vec<PathBuf>,
        /// Stop after this many discovered packages.
        #[arg(long)]
        max: Option<usize>,
        /// Stream one JSON object per package plus a final summary.
        #[arg(long)]
        jsonl: bool,
    },
    /// Resolve and render one target through a loaded library/corpus set.
    LibraryRender {
        /// Source book id that emitted the target.
        source_book_id: String,
        /// Target token, or a `lvcore://target/...` href emitted in rendered HTML.
        token: String,
        /// Package roots, payload paths, or corpus roots to inspect.
        paths: Vec<PathBuf>,
        /// Stop after this many discovered packages.
        #[arg(long)]
        max: Option<usize>,
        /// Render mode to request.
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
        /// Target token, or a `lvcore://target/...` href emitted in rendered HTML.
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
        /// Target token, or a `lvcore://target/...` href emitted in rendered HTML.
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
        /// Resource token, or a `lvcore://resource/...` href emitted in rendered HTML.
        token: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CliSearchMode {
    Exact,
    Forward,
    Backward,
    Partial,
    Fulltext,
    Advanced(String),
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
            CliSearchMode::Advanced(name) => Self::Advanced(name),
        }
    }
}

impl std::str::FromStr for CliSearchMode {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let trimmed = value.trim();
        let normalized = trimmed.to_ascii_lowercase();
        match normalized.as_str() {
            "exact" => Ok(Self::Exact),
            "forward" => Ok(Self::Forward),
            "backward" => Ok(Self::Backward),
            "partial" => Ok(Self::Partial),
            "fulltext" | "full-text" | "full_text" | "full-text-search" | "full_text_search" => {
                Ok(Self::Fulltext)
            }
            _ => {
                let Some(name) = trimmed
                    .strip_prefix("advanced:")
                    .or_else(|| trimmed.strip_prefix("advanced="))
                else {
                    return Err(format!(
                        "unknown search mode {trimmed:?}; use exact, forward, backward, partial, fulltext, or advanced:<name>"
                    ));
                };
                let name = name.trim();
                if name.is_empty() {
                    Err("advanced search mode name is empty".to_owned())
                } else {
                    Ok(Self::Advanced(name.to_owned()))
                }
            }
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
            cursor: None,
        },
        CliSequenceHint::SearchResults => SequenceHint::SearchResults {
            value: required_value("search-results", value)?,
        },
        CliSequenceHint::BodyOrder => SequenceHint::BodyOrder,
        CliSequenceHint::MenuOrder => SequenceHint::MenuOrder {
            value: required_value("menu-order", value)?,
            cursor: None,
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
            write_json_pretty(&detected)?;
        }
        Command::LibraryDiscover { paths, max, jsonl } => {
            let registry = DriverRegistry::default();
            if jsonl {
                library_discover_command_jsonl(&registry, &paths, max)?;
            } else {
                let output = library_discover_command_json(&registry, &paths, max)?;
                write_json_pretty(&output)?;
            }
        }
        Command::Validate {
            paths,
            max,
            deep,
            include_expensive_search: _,
            jsonl,
            fail_on_error,
        } => {
            let registry = DriverRegistry::default();
            let mut failures = Vec::new();
            let mut rows = Vec::new();
            let mut seen = 0usize;
            for path in paths {
                let remaining = max.map(|limit| limit.saturating_sub(seen));
                if remaining == Some(0) {
                    break;
                }
                registry.for_each_best_package(
                    &path,
                    PackageDiscoveryOptions { max: remaining },
                    |detected| {
                        seen += 1;
                        eprintln!("lvcore: validating {}", detected.root.display());
                        let started = Instant::now();
                        let failure_path = detected.root.display().to_string();
                        let mut row = validate_detected_package_json(
                            &registry,
                            detected,
                            validate::ValidateOptions { deep },
                        );
                        if let Some(object) = row.as_object_mut() {
                            object.insert(
                                "elapsed_ms".to_owned(),
                                json!(started.elapsed().as_millis()),
                            );
                        }
                        if jsonl {
                            write_stdout_line(&serde_json::to_string(&row)?)?;
                            flush_stdout()?;
                        } else {
                            rows.push(row.clone());
                        }
                        if fail_on_error && validate_row_has_failure(&row) {
                            failures.push(failure_path);
                        }
                        Ok(())
                    },
                )?;
                if let Some(limit) = max
                    && seen >= limit
                {
                    break;
                }
            }
            if !jsonl {
                write_json_pretty(&rows)?;
            }
            if fail_on_error && !failures.is_empty() {
                return Err(Error::Driver(format!(
                    "validate found {} failing package(s): {}",
                    failures.len(),
                    failures.join(", ")
                )));
            }
        }
        Command::Home { path } => {
            let registry = DriverRegistry::default();
            let output = home_command_json(&registry, &path)?;
            write_json_pretty(&output)?;
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
            write_json_pretty(&output)?;
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
            window_before,
            window_after,
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
                window_before,
                window_after,
            )?;
            write_json_pretty(&output)?;
        }
        Command::LibraryImport { paths, max, jsonl } => {
            let registry = DriverRegistry::default();
            if jsonl {
                library_import_command_jsonl(&registry, &paths, max)?;
            } else {
                let output = library_import_command_json(&registry, &paths, max);
                write_json_pretty(&output)?;
            }
        }
        Command::LibraryRender {
            source_book_id,
            token,
            paths,
            max,
            render_mode,
            debug_trace,
        } => {
            let registry = DriverRegistry::default();
            let output = library_render_command_json(
                &registry,
                &paths,
                max,
                source_book_id,
                token,
                cli_render_options(render_mode, debug_trace),
            )?;
            write_json_pretty(&output)?;
        }
        Command::Surface {
            path,
            surface_id,
            cursor,
            limit,
        } => {
            let registry = DriverRegistry::default();
            let (library, book_id) = open_single_book_library(&registry, &path)?;
            let metadata = metadata_for(&library, &book_id)?;
            let surface =
                library.open_surface_page(&book_id, &surface_id, cursor.as_deref(), limit)?;
            write_json_pretty(&json!({
                "metadata": metadata,
                "cursor": cursor,
                "limit": limit,
                "surface": surface,
            }))?;
        }
        Command::Render {
            path,
            token,
            mode,
            debug_trace,
        } => {
            let registry = DriverRegistry::default();
            let render_options = cli_render_options(mode, debug_trace);
            let output = render_command_json(&registry, &path, token, render_options)?;
            write_json_pretty(&output)?;
        }
        Command::RendererInput { path, token } => {
            let registry = DriverRegistry::default();
            let (library, book_id) = open_single_book_library(&registry, &path)?;
            let metadata = metadata_for(&library, &book_id)?;
            let target = TargetToken::from_opaque(token);
            let input = library.renderer_input_for_target(&book_id, &target)?;
            write_json_pretty(&json!({
                "metadata": metadata,
                "renderer_input": input,
            }))?;
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
            write_json_pretty(&output)?;
        }
        Command::Resource { path, token } => {
            let registry = DriverRegistry::default();
            let output = resource_command_json(&registry, &path, token)?;
            write_json_pretty(&output)?;
        }
    }
    Ok(())
}

fn write_json_pretty(value: &impl serde::Serialize) -> Result<()> {
    write_stdout_line(&serde_json::to_string_pretty(value)?)
}

fn write_json_line(value: &impl serde::Serialize) -> Result<()> {
    write_stdout_line(&serde_json::to_string(value)?)
}

fn write_stdout_line(line: &str) -> Result<()> {
    let mut stdout = io::stdout().lock();
    match writeln!(stdout, "{line}") {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => process::exit(0),
        Err(error) => Err(error.into()),
    }
}

fn flush_stdout() -> Result<()> {
    match io::stdout().flush() {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => process::exit(0),
        Err(error) => Err(error.into()),
    }
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
) -> (BookLibrary, LibraryImportReport) {
    let mut library = BookLibrary::new();
    let report =
        library.try_open_discovered_paths(paths, registry, PackageDiscoveryOptions { max });
    (library, report)
}

fn library_discover_command_json(
    registry: &DriverRegistry,
    paths: &[PathBuf],
    max: Option<usize>,
) -> Result<serde_json::Value> {
    let mut candidates: Vec<PackageCandidate> = Vec::new();
    let mut diagnostics: Vec<lvcore::Diagnostic> = Vec::new();
    library_discover_command_jsonl_with_emit(registry, paths, max, |row| {
        match row.get("event").and_then(serde_json::Value::as_str) {
            Some("candidate") => {
                if let Some(candidate) = row.get("candidate") {
                    candidates.push(serde_json::from_value(candidate.clone())?);
                }
            }
            Some("discovery") => {
                if let Some(values) = row.get("diagnostics").and_then(serde_json::Value::as_array) {
                    for value in values {
                        diagnostics
                            .push(serde_json::from_value::<lvcore::Diagnostic>(value.clone())?);
                    }
                }
            }
            Some("summary") | None | Some(_) => {}
        }
        Ok(())
    })?;
    let candidate_count = candidates.len();
    Ok(json!({
        "candidates": candidates,
        "candidate_count": candidate_count,
        "diagnostics": diagnostics,
    }))
}

fn library_discover_command_jsonl(
    registry: &DriverRegistry,
    paths: &[PathBuf],
    max: Option<usize>,
) -> Result<()> {
    library_discover_command_jsonl_with_emit(registry, paths, max, |row| {
        write_json_line(row)?;
        flush_stdout()
    })
}

fn library_discover_command_jsonl_with_emit<F>(
    registry: &DriverRegistry,
    paths: &[PathBuf],
    max: Option<usize>,
    mut emit: F,
) -> Result<()>
where
    F: FnMut(&serde_json::Value) -> Result<()>,
{
    let mut seen = BTreeSet::new();
    let mut candidate_count = 0usize;
    let mut diagnostics = Vec::new();
    for path in paths {
        let remaining = max.map(|limit| limit.saturating_sub(candidate_count));
        if remaining == Some(0) {
            break;
        }
        match registry.discover_package_candidates(path, PackageDiscoveryOptions { max: remaining })
        {
            Ok(rows) => {
                for row in rows {
                    let identity = (row.format_label.clone(), row.root_fingerprint.clone());
                    if seen.insert(identity) {
                        candidate_count = candidate_count.saturating_add(1);
                        emit(&json!({
                            "event": "candidate",
                            "status": "detected",
                            "candidate": row,
                        }))?;
                        if max.is_some_and(|limit| candidate_count >= limit) {
                            break;
                        }
                    }
                }
            }
            Err(error) => {
                let diagnostic = lvcore::Diagnostic::warning(
                    "library_discovery_failed",
                    format!("package discovery failed for {}: {error}", path.display()),
                )
                .with_context("path", path.display().to_string());
                diagnostics.push(diagnostic.clone());
                emit(&json!({
                    "event": "discovery",
                    "status": "error",
                    "path": path,
                    "diagnostics": [diagnostic],
                }))?;
            }
        }
    }
    emit(&json!({
        "event": "summary",
        "status": "ok",
        "candidate_count": candidate_count,
        "diagnostics": diagnostics,
    }))
}

fn metadata_for(library: &BookLibrary, book_id: &BookId) -> Result<BookMetadata> {
    library
        .book(book_id)
        .map(|book| book.metadata().clone())
        .ok_or_else(|| {
            Error::Driver(format!(
                "opened book id {} is not in the library",
                book_id.0
            ))
        })
}

fn library_import_command_json(
    registry: &DriverRegistry,
    paths: &[PathBuf],
    max: Option<usize>,
) -> LibraryImportResult {
    let (library, import_report) = open_library_from_paths(registry, paths, max);
    library.import_result(import_report)
}

fn library_render_command_json(
    registry: &DriverRegistry,
    paths: &[PathBuf],
    max: Option<usize>,
    source_book_id: String,
    token: String,
    render_options: RenderOptions,
) -> Result<serde_json::Value> {
    let (library, import_report) = open_library_from_paths(registry, paths, max);
    let books = library.metadata_snapshot();
    let source_book_id = BookId(source_book_id);
    let source_metadata = metadata_for(&library, &source_book_id)?;
    let routed = if token.starts_with("lvcore://target/") {
        library.render_target_href_routed(&source_book_id, &token, &render_options)?
    } else {
        let target = TargetToken::from_opaque(token);
        library.render_target_routed(&source_book_id, &target, &render_options)?
    };
    let routed_metadata = metadata_for(&library, &routed.book_id)?;
    Ok(json!({
        "books": books,
        "book_count": library.len(),
        "opened_book_ids": import_report.opened,
        "import_diagnostics": import_report.diagnostics,
        "source_book_id": source_book_id,
        "source_metadata": source_metadata,
        "routed_book_id": routed.book_id,
        "routed_metadata": routed_metadata,
        "routing_diagnostics": routed.diagnostics,
        "render_options": render_options,
        "view": routed.view,
    }))
}

fn library_import_command_jsonl(
    registry: &DriverRegistry,
    paths: &[PathBuf],
    max: Option<usize>,
) -> Result<()> {
    library_import_command_jsonl_with_emit(registry, paths, max, write_json_line)
}

fn library_import_command_jsonl_with_emit<F>(
    registry: &DriverRegistry,
    paths: &[PathBuf],
    max: Option<usize>,
    mut emit: F,
) -> Result<()>
where
    F: FnMut(&serde_json::Value) -> Result<()>,
{
    let mut library = BookLibrary::new();
    let mut opened = Vec::new();
    let mut diagnostics = Vec::new();
    for path in paths {
        let remaining = max.map(|limit| limit.saturating_sub(opened.len()));
        if remaining == Some(0) {
            break;
        }
        let discovery_result = registry.for_each_best_package(
            path,
            PackageDiscoveryOptions { max: remaining },
            |detected| {
                if max.is_some_and(|limit| opened.len() >= limit) {
                    return Ok(());
                }
                let root = detected.root.clone();
                let started = Instant::now();
                match registry.open_detected_package(detected) {
                    Ok(package) => {
                        let metadata = package.metadata().clone();
                        let book_id = metadata.book_id.clone();
                        let inserted = library.insert(package);
                        let elapsed_ms = started.elapsed().as_millis();
                        if inserted {
                            opened.push(book_id.clone());
                            emit(&json!({
                                "event": "book",
                                "status": "opened",
                                "path": root,
                                "book_id": book_id,
                                "metadata": metadata,
                                "elapsed_ms": elapsed_ms,
                            }))?;
                        } else {
                            let diagnostic = lvcore::Diagnostic::info(
                                "library_duplicate_book_skipped",
                                format!("duplicate book {} was already opened", book_id.0),
                            )
                            .with_context("path", root.display().to_string())
                            .with_context("book_id", book_id.0.clone());
                            diagnostics.push(diagnostic.clone());
                            emit(&json!({
                                "event": "diagnostic",
                                "status": "skipped_duplicate",
                                "path": root,
                                "book_id": book_id,
                                "diagnostics": [diagnostic],
                                "elapsed_ms": elapsed_ms,
                            }))?;
                        }
                    }
                    Err(error) => {
                        let diagnostic = lvcore::Diagnostic::warning(
                            "book_open_failed",
                            format!("package open failed for {}: {error}", root.display()),
                        )
                        .with_context("path", root.display().to_string());
                        diagnostics.push(diagnostic.clone());
                        emit(&json!({
                            "event": "diagnostic",
                            "status": "error",
                            "path": root,
                            "diagnostics": [diagnostic],
                            "elapsed_ms": started.elapsed().as_millis(),
                        }))?;
                    }
                }
                Ok(())
            },
        );
        if let Err(error) = discovery_result {
            let diagnostic = lvcore::Diagnostic::warning(
                "library_discovery_failed",
                format!("package discovery failed for {}: {error}", path.display()),
            )
            .with_context("path", path.display().to_string());
            diagnostics.push(diagnostic.clone());
            emit(&json!({
                "event": "discovery",
                "status": "error",
                "path": path,
                "diagnostics": [diagnostic],
            }))?;
        }
    }
    emit(&json!({
        "event": "summary",
        "status": "ok",
        "book_count": library.len(),
        "opened_book_ids": opened,
        "diagnostics": diagnostics,
    }))
}

fn home_command_json(registry: &DriverRegistry, path: &Path) -> Result<serde_json::Value> {
    let (library, book_id) = open_single_book_library(registry, path)?;
    let metadata = metadata_for(&library, &book_id)?;
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
    window_before: usize,
    window_after: usize,
) -> Result<serde_json::Value> {
    let (library, import_report) = open_library_from_paths(registry, paths, max);
    let metadata = library.metadata_snapshot();
    let page = library.search(&SearchQuery {
        scope: SearchScope::AllBooks,
        mode,
        query,
        cursor,
        limit,
        gaiji_policy: Some(render_options.gaiji_policy.clone()),
    })?;
    let search_result_sequence = page.result_sequence.clone();
    let sequence_hint = search_result_sequence
        .clone()
        .map(|value| SequenceHint::SearchResults { value });
    let rendered_first = if render_first {
        page.hits
            .first()
            .map(|hit| library.render_target_routed(&hit.book_id, &hit.target, &render_options))
            .transpose()?
    } else {
        None
    };
    let target_window = if window_before > 0 || window_after > 0 {
        match (page.hits.first(), search_result_sequence.as_deref()) {
            (Some(hit), Some(sequence)) => Some(library.resolve_search_result_window_routed(
                &hit.book_id,
                &hit.target,
                sequence,
                window_before,
                window_after,
                &render_options,
            )?),
            _ => None,
        }
    } else {
        None
    };
    Ok(json!({
        "books": metadata,
        "book_count": library.len(),
        "opened_book_ids": import_report.opened,
        "import_diagnostics": import_report.diagnostics,
        "hits": page.hits,
        "next_cursor": page.next_cursor,
        "search_result_sequence": search_result_sequence,
        "sequence_hint": sequence_hint,
        "diagnostics": page.diagnostics,
        "rendered_first": rendered_first,
        "target_window": target_window,
    }))
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
    let metadata = metadata_for(&library, &book_id)?;
    let page = library.search(&SearchQuery {
        scope: SearchScope::CurrentBook {
            book_id: book_id.clone(),
        },
        mode,
        query,
        cursor,
        limit,
        gaiji_policy: Some(render_options.gaiji_policy.clone()),
    })?;
    let search_result_sequence = page.result_sequence.clone();
    let sequence_hint = search_result_sequence
        .clone()
        .map(|value| SequenceHint::SearchResults { value });
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
                    sequence_hint.as_ref(),
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
        "search_result_sequence": search_result_sequence,
        "sequence_hint": sequence_hint,
        "diagnostics": page.diagnostics,
        "rendered_first": rendered_first,
        "target_window": target_window,
    }))
}

fn render_command_json(
    registry: &DriverRegistry,
    path: &Path,
    token: String,
    render_options: RenderOptions,
) -> Result<serde_json::Value> {
    let (library, book_id) = open_single_book_library(registry, path)?;
    let metadata = metadata_for(&library, &book_id)?;
    let (routed_book_id, view, routing_diagnostics) = if token.starts_with("lvcore://target/") {
        let routed = library.render_target_href_routed(&book_id, &token, &render_options)?;
        (routed.book_id, routed.view, routed.diagnostics)
    } else {
        let target = TargetToken::from_opaque(token);
        let view = library.render_target(&book_id, &target, &render_options)?;
        (book_id.clone(), view, Vec::new())
    };
    Ok(json!({
        "metadata": metadata,
        "routed_book_id": routed_book_id,
        "routing_diagnostics": routing_diagnostics,
        "render_options": render_options,
        "view": view,
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
    let metadata = metadata_for(&library, &book_id)?;
    let (routed_book_id, window, routing_diagnostics) = if token.starts_with("lvcore://target/") {
        let routed = library.resolve_target_window_href_routed(
            &book_id,
            &token,
            sequence_hint.as_ref(),
            before,
            after,
            &render_options,
        )?;
        (routed.book_id, routed.window, routed.diagnostics)
    } else {
        let target = TargetToken::from_opaque(token);
        let window = library.resolve_target_window(
            &book_id,
            &target,
            sequence_hint.as_ref(),
            before,
            after,
            &render_options,
        )?;
        (book_id.clone(), window, Vec::new())
    };
    Ok(json!({
        "metadata": metadata,
        "routed_book_id": routed_book_id,
        "routing_diagnostics": routing_diagnostics,
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
    let metadata = metadata_for(&library, &book_id)?;
    let (resource_ref, bytes) = if token.starts_with("lvcore://resource/") {
        (
            library.resolve_scoped_resource_href(&token)?,
            library.read_scoped_resource_href(&token)?,
        )
    } else {
        let resource = ResourceToken::from_opaque(token);
        (
            library.resolve_resource(&book_id, &resource)?,
            library.read_resource(&book_id, &resource)?,
        )
    };
    Ok(json!({
        "metadata": metadata,
        "resource": resource_ref,
        "byte_len": bytes.len(),
    }))
}

#[cfg(test)]
mod tests;

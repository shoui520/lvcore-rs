use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use lvcore::{
    DriverRegistry, RenderOptions, Result, SearchMode, SearchQuery, SearchScope, TargetToken,
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
        /// Maximum hits to return.
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Resolve and render the first hit.
        #[arg(long)]
        render_first: bool,
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
    },
    /// Resolve and render one opaque target token for one package.
    Render {
        /// Package root or payload path to inspect.
        path: PathBuf,
        /// Target token previously returned by search, navigation, or links.
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

fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Detect { path } => {
            let registry = DriverRegistry::default();
            let detected = registry.detect(&path)?;
            println!("{}", serde_json::to_string_pretty(&detected)?);
        }
        Command::Validate { paths, max } => {
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
                let row = match registry.open_best(&path) {
                    Ok(package) => {
                        let metadata = package.metadata();
                        match package.home_surfaces() {
                            Ok(surfaces) => json!({
                                "path": path,
                                "status": "ok",
                                "book_id": metadata.book_id,
                                "format_family": metadata.format_family,
                                "format_label": metadata.format_label,
                                "title": metadata.title,
                                "capabilities": metadata.capabilities,
                                "surface_count": surfaces.len(),
                                "surfaces": surfaces,
                            }),
                            Err(error) => json!({
                                "path": path,
                                "status": "surface_error",
                                "book_id": metadata.book_id,
                                "format_family": metadata.format_family,
                                "format_label": metadata.format_label,
                                "title": metadata.title,
                                "error": error.to_string(),
                            }),
                        }
                    }
                    Err(error) => json!({
                        "path": path,
                        "status": "open_error",
                        "error": error.to_string(),
                    }),
                };
                rows.push(row);
            }
            println!("{}", serde_json::to_string_pretty(&rows)?);
        }
        Command::Search {
            path,
            query,
            mode,
            limit,
            render_first,
            window_before,
            window_after,
        } => {
            let registry = DriverRegistry::default();
            let package = registry.open_best(&path)?;
            let metadata = package.metadata();
            let page = package.search(&SearchQuery {
                scope: SearchScope::CurrentBook(metadata.book_id.clone()),
                mode: mode.into(),
                query,
                cursor: None,
                limit,
            })?;
            let rendered_first = if render_first {
                page.hits
                    .first()
                    .map(|hit| package.render_target(&hit.target, &RenderOptions::default()))
                    .transpose()?
            } else {
                None
            };
            let target_window = if window_before > 0 || window_after > 0 {
                page.hits
                    .first()
                    .map(|hit| {
                        package.resolve_target_window(
                            &hit.target,
                            None,
                            window_before,
                            window_after,
                            &RenderOptions::default(),
                        )
                    })
                    .transpose()?
            } else {
                None
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "metadata": metadata,
                    "hits": page.hits,
                    "next_cursor": page.next_cursor,
                    "diagnostics": page.diagnostics,
                    "rendered_first": rendered_first,
                    "target_window": target_window,
                }))?
            );
        }
        Command::Surface { path, surface_id } => {
            let registry = DriverRegistry::default();
            let package = registry.open_best(&path)?;
            let metadata = package.metadata();
            let surface = package.open_surface(&surface_id)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "metadata": metadata,
                    "surface": surface,
                }))?
            );
        }
        Command::Render { path, token } => {
            let registry = DriverRegistry::default();
            let package = registry.open_best(&path)?;
            let metadata = package.metadata();
            let target = TargetToken::from_opaque(token);
            let view = package.render_target(&target, &RenderOptions::default())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "metadata": metadata,
                    "view": view,
                }))?
            );
        }
    }
    Ok(())
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
    if is_obvious_package_candidate(path)? {
        out.push(path.to_path_buf());
        return Ok(());
    }
    if !registry.detect(path)?.is_empty() {
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
    name == "main.data" || name.ends_with(".dbc") || name.ends_with(".idx")
}

fn is_obvious_package_candidate(path: &Path) -> Result<bool> {
    if path.is_file() {
        return Ok(is_package_file_candidate(path));
    }
    if !path.is_dir() {
        return Ok(false);
    }
    if path.join("main.data").is_file() || directory_has_file_suffix(path, ".dbc")? {
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

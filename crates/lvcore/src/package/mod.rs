mod capabilities;
mod chm_toc;
mod drivers;
mod family_drivers;
mod hc_profile;
mod html;
mod lved_refs;
mod navigation_helpers;
mod render_output;
mod resource_helpers;
mod ssed_body_helpers;
mod ssed_detection;
mod ssed_index_probe;
mod ssed_payload;
mod ssed_search;
mod ssed_search_runtime;
mod ssed_zip;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::lved_sqlite::is_lved_payload_name;
use crate::search::SearchMode;

use crate::body::BodyProvider;
use crate::error::{Error, Result};
use crate::gaiji::GaijiProvider;
use crate::navigation::NavigationProvider;
use crate::render::{RendererInputProvider, RendererProvider};
use crate::resources::ResourceProvider;
use crate::search::SearchProvider;
use crate::sequence::SequenceProvider;
use crate::storage::regular_file_inside_root;

pub use drivers::{
    HoureiDriver, LvedSqliteDriver, LvlMultiViewDriver, ReaderBookPackage, SsedDriver,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BookId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatFamily {
    Ssed,
    LvedSqlite3,
    LvlMultiView,
    Hourei,
    Unknown,
}

impl FormatFamily {
    pub fn ui_label(self) -> &'static str {
        match self {
            Self::Ssed => "SSED",
            Self::LvedSqlite3 => "LVED_SQLITE3",
            Self::LvlMultiView => "LVLMultiView",
            Self::Hourei => "Hourei",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    NativeSearch,
    FullTextSearch,
    TitleIndexBrowse,
    Menu,
    ScreenMenu,
    EncyclopediaIndex,
    AuxiliaryIndex,
    MultiSelector,
    Toc,
    Panels,
    Hanrei,
    Resources,
    Gaiji,
    HcRenderInput,
    PreservedHtml,
    ContinuousView,
    LawNavigation,
    DeferredRendering,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookMetadata {
    pub book_id: BookId,
    pub format_family: FormatFamily,
    pub format_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub root_fingerprint: String,
    pub capabilities: Vec<Capability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub search_modes: Vec<SearchMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BookAliasKind {
    LvedDictCode,
}

/// Non-stable backend routing hint used to resolve native links between open
/// books. This is not a book identity and should not replace `BookId`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookAlias {
    pub kind: BookAliasKind,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedPackage {
    pub root: PathBuf,
    pub format_family: FormatFamily,
    pub confidence: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PackageDiscoveryOptions {
    pub max: Option<usize>,
}

impl PackageDiscoveryOptions {
    pub fn with_max(max: usize) -> Self {
        Self { max: Some(max) }
    }
}

pub trait BookPackage:
    Send
    + Sync
    + SearchProvider
    + NavigationProvider
    + RendererProvider
    + RendererInputProvider
    + ResourceProvider
    + GaijiProvider
    + SequenceProvider
    + BodyProvider
{
    fn metadata(&self) -> &BookMetadata;
    fn root(&self) -> &Path;

    fn routing_aliases(&self) -> &[BookAlias] {
        &[]
    }
}

pub trait BookPackageExt: BookPackage {
    fn book_id(&self) -> &BookId {
        &self.metadata().book_id
    }

    fn format_family(&self) -> FormatFamily {
        self.metadata().format_family
    }
}

impl<T: BookPackage + ?Sized> BookPackageExt for T {}

pub trait PackageDriver: Send + Sync {
    fn family(&self) -> FormatFamily;
    fn detect(&self, root: &Path) -> Result<Option<DetectedPackage>>;
    fn open(&self, root: &Path) -> Result<Box<dyn BookPackage>>;

    fn open_detected(&self, detected: DetectedPackage) -> Result<Box<dyn BookPackage>> {
        self.open(&detected.root)
    }
}

pub struct DriverRegistry {
    drivers: Vec<Box<dyn PackageDriver>>,
}

const MAX_PACKAGE_DISCOVERY_DEPTH: usize = 32;

impl DriverRegistry {
    pub fn new(drivers: Vec<Box<dyn PackageDriver>>) -> Self {
        Self { drivers }
    }

    pub fn detect(&self, root: &Path) -> Result<Vec<DetectedPackage>> {
        let mut rows = Vec::new();
        for driver in &self.drivers {
            if let Some(detected) = driver.detect(root)? {
                rows.push(detected);
            }
        }
        rows.sort_by(|a, b| {
            b.confidence
                .cmp(&a.confidence)
                .then_with(|| {
                    family_priority(b.format_family).cmp(&family_priority(a.format_family))
                })
                .then_with(|| a.root.cmp(&b.root))
        });
        Ok(rows)
    }

    pub fn detect_all(
        &self,
        root: &Path,
        options: PackageDiscoveryOptions,
    ) -> Result<Vec<DetectedPackage>> {
        let direct = self.detect(root)?;
        if !direct.is_empty() {
            return Ok(direct);
        }

        let mut rows = Vec::new();
        for package_root in self.discover_roots(root, options)? {
            rows.extend(self.detect(&package_root)?);
        }
        Ok(rows)
    }

    pub fn discover_roots(
        &self,
        root: &Path,
        options: PackageDiscoveryOptions,
    ) -> Result<Vec<PathBuf>> {
        Ok(self
            .discover_best_packages(root, options)?
            .into_iter()
            .map(|detected| detected.root)
            .collect())
    }

    pub fn discover_best_packages(
        &self,
        root: &Path,
        options: PackageDiscoveryOptions,
    ) -> Result<Vec<DetectedPackage>> {
        let mut rows = Vec::new();
        self.for_each_best_package(root, options, |detected| {
            rows.push(detected);
            Ok(())
        })?;
        Ok(rows)
    }

    pub fn for_each_best_package<F>(
        &self,
        root: &Path,
        options: PackageDiscoveryOptions,
        mut on_package: F,
    ) -> Result<()>
    where
        F: FnMut(DetectedPackage) -> Result<()>,
    {
        let mut visited = BTreeSet::new();
        let mut found = 0usize;
        self.discover_best_packages_with(
            root,
            options.max,
            &mut found,
            0,
            &mut visited,
            &mut on_package,
        )
    }

    pub fn open_best(&self, root: &Path) -> Result<Box<dyn BookPackage>> {
        let detected = self
            .detect(root)?
            .into_iter()
            .next()
            .ok_or(Error::UnrecognizedPackage)?;
        for driver in &self.drivers {
            if driver.family() == detected.format_family {
                return driver.open_detected(detected);
            }
        }
        Err(Error::UnsupportedFamily(
            detected.format_family.ui_label().to_owned(),
        ))
    }

    pub fn open_detected_package(&self, detected: DetectedPackage) -> Result<Box<dyn BookPackage>> {
        for driver in &self.drivers {
            if driver.family() == detected.format_family {
                return driver.open_detected(detected);
            }
        }
        Err(Error::UnsupportedFamily(
            detected.format_family.ui_label().to_owned(),
        ))
    }

    fn discover_best_packages_with<F>(
        &self,
        path: &Path,
        max: Option<usize>,
        found: &mut usize,
        depth: usize,
        visited: &mut BTreeSet<PathBuf>,
        on_package: &mut F,
    ) -> Result<()>
    where
        F: FnMut(DetectedPackage) -> Result<()>,
    {
        if max.is_some_and(|max| *found >= max) {
            return Ok(());
        }
        if depth > MAX_PACKAGE_DISCOVERY_DEPTH {
            return Ok(());
        }
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return Ok(());
        };
        if metadata.file_type().is_symlink() {
            return Ok(());
        }
        if metadata.is_file() && !is_package_file_candidate(path) {
            return Ok(());
        }
        if metadata.is_dir() {
            if is_obvious_resource_only_dir(path) {
                return Ok(());
            }
            if let Ok(canonical) = fs::canonicalize(path)
                && !visited.insert(canonical)
            {
                return Ok(());
            }
        }
        if is_obvious_package_candidate(path)?
            && let Some(detected) = self.detect(path)?.into_iter().next()
        {
            *found += 1;
            (*on_package)(detected)?;
            return Ok(());
        }
        if !metadata.is_dir() {
            return Ok(());
        }

        let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            if entry
                .file_type()
                .is_ok_and(|file_type| file_type.is_symlink())
            {
                continue;
            }
            self.discover_best_packages_with(
                &entry.path(),
                max,
                found,
                depth + 1,
                visited,
                on_package,
            )?;
            if max.is_some_and(|max| *found >= max) {
                break;
            }
        }
        Ok(())
    }
}

fn family_priority(family: FormatFamily) -> u8 {
    match family {
        FormatFamily::Hourei => 40,
        FormatFamily::LvedSqlite3 => 30,
        FormatFamily::LvlMultiView => 20,
        FormatFamily::Ssed => 10,
        FormatFamily::Unknown => 0,
    }
}

fn is_package_file_candidate(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    name == "main.data" || name.ends_with(".dbc") || name.ends_with(".idx") || name.ends_with(".db")
}

fn is_obvious_package_candidate(path: &Path) -> Result<bool> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        return Ok(is_package_file_candidate(path));
    }
    if !fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir()) {
        return Ok(false);
    }
    if regular_file_inside_root(path, &path.join("main.data"))?
        || directory_has_file_suffix(path, ".dbc")?
        || directory_has_lved_payload(path)?
    {
        return Ok(true);
    }
    if directory_has_file_suffix(path, ".idx")? {
        return Ok(true);
    }
    if regular_file_inside_root(path, &path.join("menuData.xml"))?
        && directory_has_multiview_payload(path)?
    {
        return Ok(true);
    }
    let hourei_required = [
        "_DataBase/hore_base.db",
        "_DataBase/hore_search_a.db",
        "_DataBase/horejo_base.db",
    ];
    Ok(hourei_required
        .iter()
        .all(|relative| regular_file_inside_root(path, &path.join(relative)).unwrap_or(false)))
}

fn directory_has_lved_payload(path: &Path) -> Result<bool> {
    if !fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir()) {
        return Ok(false);
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if regular_file_inside_root(path, &entry_path)? && is_lved_payload_name(&entry_path) {
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
    if !fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir()) {
        return Ok(false);
    }
    let suffix = suffix.to_lowercase();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if regular_file_inside_root(path, &entry_path)?
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
    if !fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir()) {
        return Ok(false);
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if !regular_file_inside_root(path, &entry.path())? {
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

impl Default for DriverRegistry {
    fn default() -> Self {
        Self::new(vec![
            Box::new(SsedDriver),
            Box::new(LvedSqliteDriver),
            Box::new(LvlMultiViewDriver),
            Box::new(HoureiDriver),
        ])
    }
}

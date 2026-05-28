use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use encoding_rs::SHIFT_JIS;
use serde_json::json;
use sha2::{Digest, Sha256};
use zip::ZipArchive;
use zip::result::ZipError;

use crate::body::{BodyProvider, BodySourceKind, VisualBody};
use crate::chm::{list_chm_entries, read_chm_entry};
use crate::crypto::{
    decrypt_logofont_cipher_file_to_path, decrypt_logofont_cipher_prefix,
    decrypt_macos_logofont_cipher_file_to_path, decrypt_macos_logofont_cipher_prefix,
};
use crate::diagnostics::Diagnostic;
use crate::error::{Error, Result};
use crate::gaiji::{
    GaijiPolicy, GaijiProvider, GaijiResolution, GaijiSourcePreference, RichLabel,
    normalize_gaiji_identity, parse_uni_gaiji_map, resolve_rich_label,
};
use crate::hourei::{HoureiStore, escape_plain_label_html as escape_hourei_label_html};
use crate::lved_sqlite::{LvedSqliteStore, LvedSqliteSummary};
use crate::multiview::{MultiviewMenuItem, MultiviewStore, parse_menu_data};
use crate::navigation::{
    HomeSurface, NavigationItem, NavigationNode, NavigationProvider, NavigationStatus,
    NavigationSurface, NavigationSurfaceKind, PanelCell,
};
use crate::render::{
    RenderMode, RenderOptions, RendererInput, RendererInputProvider, RendererProvider,
    ResolvedTargetKind, ResolvedTargetView,
};
use crate::resources::{
    InternalResource, ResourceKind, ResourceProvider, ResourceRef, ResourceToken,
};
use crate::search::{SearchHit, SearchMode, SearchPage, SearchProvider, SearchQuery};
use crate::sequence::{SequenceHint, SequenceProvider, TargetWindow};
use crate::ssed::{
    SSEDDATA_MAGIC, SsedCatalog, SsedComponent, SsedComponentRole, SsedDataFile, SsedDataHeader,
};
use crate::ssed_index::{
    INDEX_PAGE_SIZE, SsedIndexPointer, SsedIndexRow, SsedIndexScanState, decode_jis_pair,
    decode_title_text, is_leaf_page, is_simple_leaf_index_type, is_supported_index_type,
    parse_internal_page, parse_simple_leaf_page, parse_supported_leaf_page,
};
use crate::ssed_menu::{SsedMenuRecord, parse_menu_stream};
use crate::ssed_panel::{
    SsedPanelBinRecord, SsedPanelDataRef, SsedPanelInlineCell, parse_panel_bin,
    parse_panel_xml_bytes,
};
use crate::ssed_sidecar::{
    SsedSidecarBodyResolver, SsedSidecarKind, SsedSidecarLookup,
    discover_ssed_sidecar_body_resolvers, lookup_ssed_dense_sidecar_body_with_resolvers,
};
use crate::storage::{DirectoryStorage, StorageBackend};
use crate::target::{InternalTarget, TargetLink, TargetToken};

use super::{
    BookId, BookMetadata, BookPackage, Capability, DetectedPackage, FormatFamily, PackageDriver,
};

pub struct SsedDriver;
pub struct LvedSqliteDriver;
pub struct LvlMultiViewDriver;
pub struct HoureiDriver;

impl PackageDriver for SsedDriver {
    fn family(&self) -> FormatFamily {
        FormatFamily::Ssed
    }

    fn detect(&self, root: &Path) -> Result<Option<DetectedPackage>> {
        let package_root = package_root_for_detection(root);
        let idx_files = if root.is_file()
            && root
                .file_name()
                .map(|v| v.to_string_lossy().to_lowercase().ends_with(".idx"))
                .unwrap_or(false)
        {
            vec![root.to_path_buf()]
        } else {
            files_with_suffix(package_root, ".idx")?
        };
        for path in idx_files {
            if let Ok(catalog) = SsedCatalog::parse_file(&path) {
                return Ok(Some(DetectedPackage {
                    root: package_root.to_path_buf(),
                    format_family: FormatFamily::Ssed,
                    confidence: 95,
                    title: Some(catalog.title),
                    evidence: vec![
                        format!("ssedinfo:{}", display_name(&path)),
                        format!("components:{}", catalog.components.len()),
                    ],
                }));
            }
        }
        Ok(None)
    }

    fn open(&self, root: &Path) -> Result<Box<dyn BookPackage>> {
        let detection = self
            .detect(root)?
            .ok_or_else(|| Error::Driver("not an SSED package".to_owned()))?;
        let catalog = ssed_catalog_for_root(&detection.root)?;
        let package_root = detection.root.clone();
        let capabilities = ssed_capabilities(&catalog, &package_root);
        Ok(Box::new(StubBookPackage::new(
            &package_root,
            detection,
            capabilities,
            StubPackageStores {
                ssed_catalog: Some(catalog),
                gaiji_unicode_map: load_package_uni_gaiji_maps(&package_root),
                ..Default::default()
            },
        )))
    }
}

impl PackageDriver for LvedSqliteDriver {
    fn family(&self) -> FormatFamily {
        FormatFamily::LvedSqlite3
    }

    fn detect(&self, root: &Path) -> Result<Option<DetectedPackage>> {
        let package_root = package_root_for_detection(root);
        if let Some(store) = LvedSqliteStore::discover(root)? {
            let mut evidence = vec![
                store
                    .payload_path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| "lved_sqlite_payload".to_owned()),
            ];
            if let Some(key_file) = &store.key_file {
                evidence.push(format!("key_file:{}", key_file.match_kind));
            }
            let title = match store.title() {
                Ok(title) => title.or_else(|| inferred_folder_title(package_root)),
                Err(_) => return Ok(None),
            };
            return Ok(Some(DetectedPackage {
                root: package_root.to_path_buf(),
                format_family: FormatFamily::LvedSqlite3,
                confidence: 98,
                title,
                evidence,
            }));
        }
        Ok(None)
    }

    fn open(&self, root: &Path) -> Result<Box<dyn BookPackage>> {
        let package_root = package_root_for_detection(root).to_path_buf();
        let store = LvedSqliteStore::discover(root)?
            .ok_or_else(|| Error::Driver("not an LVED_SQLITE3 package".to_owned()))?;
        let summary = store.summary()?;
        let mut evidence = vec![
            store
                .payload_path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "lved_sqlite_payload".to_owned()),
        ];
        if let Some(key_file) = &store.key_file {
            evidence.push(format!("key_file:{}", key_file.match_kind));
        }
        let detection = DetectedPackage {
            root: package_root.clone(),
            format_family: FormatFamily::LvedSqlite3,
            confidence: 98,
            title: summary
                .title
                .clone()
                .or_else(|| inferred_folder_title(&package_root)),
            evidence,
        };
        Ok(Box::new(StubBookPackage::new(
            &package_root,
            detection,
            lved_capabilities(),
            StubPackageStores {
                lved_store: Some(store),
                lved_summary: Some(summary),
                ..Default::default()
            },
        )))
    }
}

impl PackageDriver for LvlMultiViewDriver {
    fn family(&self) -> FormatFamily {
        FormatFamily::LvlMultiView
    }

    fn detect(&self, root: &Path) -> Result<Option<DetectedPackage>> {
        let storage = DirectoryStorage::new(root);
        if !storage.exists(Path::new("menuData.xml"))? {
            return Ok(None);
        }
        let payloads = fs::read_dir(root)?
            .filter_map(std::result::Result::ok)
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                name.len() == 6
                    && name.as_bytes()[1] == b'l'
                    && name.as_bytes()[2] == b'v'
                    && (name.ends_with("bat") || name.ends_with("dat"))
            })
            .count();
        if payloads == 0 {
            return Ok(None);
        }
        let retained_ssed_title = ssed_catalog_for_root(root)
            .ok()
            .and_then(|catalog| usable_multiview_title(&catalog.title));
        let menu_title = multiview_menu_title(root)?;
        Ok(Some(DetectedPackage {
            root: root.to_path_buf(),
            format_family: FormatFamily::LvlMultiView,
            confidence: 98,
            title: retained_ssed_title
                .or(menu_title)
                .or_else(|| inferred_folder_title(root)),
            evidence: vec!["menuData.xml".to_owned(), "*lvbat/*lvdat".to_owned()],
        }))
    }

    fn open(&self, root: &Path) -> Result<Box<dyn BookPackage>> {
        let detection = self
            .detect(root)?
            .ok_or_else(|| Error::Driver("not an LVLMultiView package".to_owned()))?;
        let package_root = detection.root.clone();
        let store = MultiviewStore::discover(&package_root)?;
        Ok(Box::new(StubBookPackage::new(
            &package_root,
            detection,
            multiview_capabilities(),
            StubPackageStores {
                multiview_store: store,
                ..Default::default()
            },
        )))
    }
}

impl PackageDriver for HoureiDriver {
    fn family(&self) -> FormatFamily {
        FormatFamily::Hourei
    }

    fn detect(&self, root: &Path) -> Result<Option<DetectedPackage>> {
        let storage = DirectoryStorage::new(root);
        let required = [
            "_DataBase/hore_base.db",
            "_DataBase/hore_search_a.db",
            "_DataBase/horejo_base.db",
        ];
        if required
            .iter()
            .all(|path| storage.exists(Path::new(path)).unwrap_or(false))
        {
            return Ok(Some(DetectedPackage {
                root: root.to_path_buf(),
                format_family: FormatFamily::Hourei,
                confidence: 98,
                title: Some("LogoVista電子法令 Professional".to_owned()),
                evidence: required.iter().map(|v| (*v).to_owned()).collect(),
            }));
        }
        Ok(None)
    }

    fn open(&self, root: &Path) -> Result<Box<dyn BookPackage>> {
        let detection = self
            .detect(root)?
            .ok_or_else(|| Error::Driver("not a Hourei package".to_owned()))?;
        let package_root = detection.root.clone();
        let store = HoureiStore::discover(&package_root)?;
        Ok(Box::new(StubBookPackage::new(
            &package_root,
            detection,
            hourei_capabilities(),
            StubPackageStores {
                hourei_store: store,
                ..Default::default()
            },
        )))
    }
}

pub struct StubBookPackage {
    root: PathBuf,
    storage: DirectoryStorage,
    metadata: BookMetadata,
    ssed_catalog: Option<SsedCatalog>,
    lved_store: Option<LvedSqliteStore>,
    lved_summary: Option<LvedSqliteSummary>,
    multiview_store: Option<MultiviewStore>,
    hourei_store: Option<HoureiStore>,
    gaiji_unicode_map: BTreeMap<String, String>,
    ssed_sidecar_body_resolvers:
        OnceLock<std::result::Result<Vec<SsedSidecarBodyResolver>, String>>,
}

#[derive(Debug, Default)]
pub struct StubPackageStores {
    pub ssed_catalog: Option<SsedCatalog>,
    pub lved_store: Option<LvedSqliteStore>,
    pub lved_summary: Option<LvedSqliteSummary>,
    pub multiview_store: Option<MultiviewStore>,
    pub hourei_store: Option<HoureiStore>,
    pub gaiji_unicode_map: BTreeMap<String, String>,
}

struct NormalizedHtmlRefs {
    html: String,
    resources: Vec<ResourceRef>,
    links: Vec<TargetLink>,
    diagnostics: Vec<Diagnostic>,
}

type PrefixDecryptFn = fn(&[u8], usize) -> Result<Vec<u8>>;
type FileDecryptFn = fn(&Path, &Path) -> Result<()>;

const SSED_FULLTEXT_BODY_WINDOW_BYTES: usize = 16 * 1024;
const SSED_FULLTEXT_SCAN_WINDOW_BYTES: usize = 256 * 1024;
const SSED_FULLTEXT_SCAN_OVERLAP_BYTES: usize = 512;
const SSED_FULLTEXT_SNIPPET_CHARS: usize = 160;

#[derive(Debug, Clone)]
struct SsedFulltextRow {
    offset: u64,
    row: SsedIndexRow,
}

struct SsedIndexSearchCollector<'a> {
    package: &'a StubBookPackage,
    mode: &'a SearchMode,
    needle: &'a str,
    offset: usize,
    page_limit: usize,
    matched_count: usize,
    hits: Vec<SearchHit>,
    diagnostics: Vec<Diagnostic>,
    seen_targets: HashSet<String>,
}

impl<'a> SsedIndexSearchCollector<'a> {
    fn new(
        package: &'a StubBookPackage,
        mode: &'a SearchMode,
        needle: &'a str,
        offset: usize,
        page_limit: usize,
    ) -> Self {
        Self {
            package,
            mode,
            needle,
            offset,
            page_limit,
            matched_count: 0,
            hits: Vec::new(),
            diagnostics: Vec::new(),
            seen_targets: HashSet::new(),
        }
    }

    fn push_row(&mut self, row: SsedIndexRow) -> Result<bool> {
        let key = row.key.to_lowercase();
        let row_matches = match self.mode {
            SearchMode::Exact => key == self.needle,
            SearchMode::Forward => key.starts_with(self.needle),
            SearchMode::Backward => key.ends_with(self.needle),
            SearchMode::Partial => key.contains(self.needle),
            SearchMode::FullText | SearchMode::Advanced(_) => false,
        };
        if !row_matches {
            return Ok(true);
        }
        let target = match self.package.ssed_target_for_index_pointer(row.body)? {
            Ok(target) => target,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return Ok(true);
            }
        };
        if !self.seen_targets.insert(target.as_str().to_owned()) {
            return Ok(true);
        }
        if self.matched_count < self.offset {
            self.matched_count = self.matched_count.saturating_add(1);
            return Ok(true);
        }
        let title = self.package.ssed_display_text_for_index_row(&row);
        let label = self.package.ssed_rich_label(&title);
        self.hits.push(SearchHit {
            book_id: self.package.metadata.book_id.clone(),
            target,
            title_html: label.html,
            title_text: label.text,
            snippet_html: None,
            diagnostics: label.diagnostics,
        });
        self.matched_count = self.matched_count.saturating_add(1);
        Ok(self.hits.len() < self.page_limit)
    }

    fn has_hits(&self) -> bool {
        !self.hits.is_empty()
    }

    fn extend_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) {
        self.diagnostics.extend(diagnostics);
    }

    fn into_search_page(mut self, limit: usize) -> SearchPage {
        let next_cursor = (self.hits.len() > limit).then(|| (self.offset + limit).to_string());
        self.hits.truncate(limit);
        SearchPage {
            hits: self.hits,
            next_cursor,
            diagnostics: self.diagnostics,
        }
    }
}

impl StubBookPackage {
    pub fn new(
        root: &Path,
        detected: DetectedPackage,
        capabilities: Vec<Capability>,
        stores: StubPackageStores,
    ) -> Self {
        let format_label = detected.format_family.ui_label().to_owned();
        let root_fingerprint = root_fingerprint(root);
        let fingerprint_short = root_fingerprint
            .get(..12)
            .unwrap_or(root_fingerprint.as_str());
        let book_id = BookId(format!(
            "{}:{}:{}",
            format_label,
            root.file_name()
                .map(|v| v.to_string_lossy())
                .unwrap_or_else(|| root.as_os_str().to_string_lossy()),
            fingerprint_short,
        ));
        let metadata = BookMetadata {
            book_id,
            format_family: detected.format_family,
            format_label,
            title: detected.title,
            icon_hint: None,
            root_fingerprint,
            capabilities,
        };
        Self {
            root: root.to_path_buf(),
            storage: DirectoryStorage::new(root),
            metadata,
            ssed_catalog: stores.ssed_catalog,
            lved_store: stores.lved_store,
            lved_summary: stores.lved_summary,
            multiview_store: stores.multiview_store,
            hourei_store: stores.hourei_store,
            gaiji_unicode_map: stores.gaiji_unicode_map,
            ssed_sidecar_body_resolvers: OnceLock::new(),
        }
    }
}

impl BookPackage for StubBookPackage {
    fn metadata(&self) -> &BookMetadata {
        &self.metadata
    }

    fn root(&self) -> &Path {
        &self.root
    }
}

impl SearchProvider for StubBookPackage {
    fn search(&self, query: &SearchQuery) -> Result<SearchPage> {
        if self.metadata.format_family == FormatFamily::Ssed {
            return self.search_ssed_simple_indexes(query);
        }
        if self.metadata.format_family == FormatFamily::LvedSqlite3 {
            return self.search_lved_sqlite(query);
        }
        if self.metadata.format_family == FormatFamily::LvlMultiView {
            return self.search_multiview(query);
        }
        if self.metadata.format_family == FormatFamily::Hourei {
            return self.search_hourei(query);
        }
        Ok(SearchPage::deferred(format!(
            "{} search provider is not implemented yet",
            self.metadata.format_label
        )))
    }
}

impl NavigationProvider for StubBookPackage {
    fn home_surfaces(&self) -> Result<Vec<HomeSurface>> {
        let mut surfaces = Vec::new();
        match self.metadata.format_family {
            FormatFamily::Ssed => {
                if self
                    .ssed_catalog
                    .as_ref()
                    .is_some_and(|catalog| catalog.has_role(SsedComponentRole::Menu))
                    || self.storage.exists(Path::new("MENU.DIC"))?
                {
                    surfaces.push(HomeSurface {
                        surface_id: "menu".to_owned(),
                        kind: NavigationSurfaceKind::Menu,
                        status: NavigationStatus::Available,
                        title_html: "MENU".to_owned(),
                        title_text: "MENU".to_owned(),
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id: "menu".to_owned(),
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: Vec::new(),
                    });
                }
                if self
                    .ssed_catalog
                    .as_ref()
                    .is_some_and(|catalog| catalog.has_role(SsedComponentRole::Toc))
                {
                    surfaces.push(HomeSurface {
                        surface_id: "toc".to_owned(),
                        kind: NavigationSurfaceKind::Toc,
                        status: NavigationStatus::Available,
                        title_html: "TOC".to_owned(),
                        title_text: "TOC".to_owned(),
                        target: Some(TargetToken::new(&InternalTarget::TocItem {
                            surface_id: "toc".to_owned(),
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: Vec::new(),
                    });
                }
                let hanrei_pages = self.discover_ssed_hanrei_pages()?;
                if !hanrei_pages.is_empty() {
                    let diagnostics = hanrei_pages
                        .iter()
                        .flat_map(|page| page.diagnostics.clone())
                        .collect::<Vec<_>>();
                    surfaces.push(HomeSurface {
                        surface_id: "hanrei".to_owned(),
                        kind: NavigationSurfaceKind::Hanrei,
                        status: NavigationStatus::Available,
                        title_html: "凡例".to_owned(),
                        title_text: "凡例".to_owned(),
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id: "hanrei".to_owned(),
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics,
                    });
                }
                push_surface_if_exists(
                    &mut surfaces,
                    &self.storage,
                    "panels",
                    NavigationSurfaceKind::Panel,
                    "Panels",
                    &["Panels.xml", "Panel"],
                )?;
                if self.ssed_catalog.as_ref().is_some_and(|catalog| {
                    catalog.has_role(SsedComponentRole::Title)
                        || catalog.has_role(SsedComponentRole::Index)
                }) {
                    surfaces.push(HomeSurface {
                        surface_id: "title-index".to_owned(),
                        kind: NavigationSurfaceKind::TitleIndexBrowse,
                        status: NavigationStatus::Available,
                        title_html: "Title/Index Browse".to_owned(),
                        title_text: "Title/Index Browse".to_owned(),
                        target: Some(TargetToken::new(&InternalTarget::TitleIndexItem {
                            surface_id: "title-index".to_owned(),
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: vec![Diagnostic::info(
                            "surface_partial",
                            "SSED title/index browsing is available for supported leaf row grammars; exact/forward simple-index search can use internal tree pages while other paths may still scan linearly",
                        )],
                    });
                }
            }
            FormatFamily::LvedSqlite3 => {
                let list_available = self
                    .lved_summary
                    .as_ref()
                    .is_some_and(|summary| summary.list_available);
                let info_available = self
                    .lved_summary
                    .as_ref()
                    .is_some_and(|summary| summary.info_available);
                let tree_available = self
                    .lved_summary
                    .as_ref()
                    .is_some_and(|summary| summary.tree_available);
                surfaces.push(HomeSurface {
                    surface_id: "lved-list".to_owned(),
                    kind: NavigationSurfaceKind::TitleIndexBrowse,
                    status: if list_available {
                        NavigationStatus::Available
                    } else {
                        NavigationStatus::Missing
                    },
                    title_html: "LVED list".to_owned(),
                    title_text: "LVED list".to_owned(),
                    target: list_available
                        .then(|| {
                            TargetToken::new(&InternalTarget::TitleIndexItem {
                                surface_id: "lved-list".to_owned(),
                                item_id: "root".to_owned(),
                            })
                        })
                        .transpose()?,
                    diagnostics: Vec::new(),
                });
                surfaces.push(HomeSurface {
                    surface_id: "info".to_owned(),
                    kind: NavigationSurfaceKind::Info,
                    status: if info_available {
                        NavigationStatus::Available
                    } else {
                        NavigationStatus::Missing
                    },
                    title_html: "Info".to_owned(),
                    title_text: "Info".to_owned(),
                    target: None,
                    diagnostics: Vec::new(),
                });
                surfaces.push(HomeSurface {
                    surface_id: "lved-tree".to_owned(),
                    kind: NavigationSurfaceKind::LvedTree,
                    status: if tree_available {
                        NavigationStatus::Available
                    } else {
                        NavigationStatus::Missing
                    },
                    title_html: "LVED tree".to_owned(),
                    title_text: "LVED tree".to_owned(),
                    target: tree_available
                        .then(|| {
                            TargetToken::new(&InternalTarget::MenuItem {
                                surface_id: "lved-tree".to_owned(),
                                item_id: "root".to_owned(),
                            })
                        })
                        .transpose()?,
                    diagnostics: Vec::new(),
                });
            }
            FormatFamily::LvlMultiView => {
                surfaces.push(HomeSurface {
                    surface_id: "menuData".to_owned(),
                    kind: NavigationSurfaceKind::MultiviewTree,
                    status: NavigationStatus::Available,
                    title_html: "MultiView menu".to_owned(),
                    title_text: "MultiView menu".to_owned(),
                    target: Some(TargetToken::new(&InternalTarget::MultiviewHref {
                        href: "menuData.xml".to_owned(),
                        anchor: None,
                    })?),
                    diagnostics: Vec::new(),
                });
            }
            FormatFamily::Hourei => {
                surfaces.push(HomeSurface {
                    surface_id: "law-tree".to_owned(),
                    kind: NavigationSurfaceKind::LawTree,
                    status: if self.hourei_store.is_some() {
                        NavigationStatus::Available
                    } else {
                        NavigationStatus::Deferred
                    },
                    title_html: "法令".to_owned(),
                    title_text: "法令".to_owned(),
                    target: self
                        .hourei_store
                        .is_some()
                        .then(|| {
                            TargetToken::new(&InternalTarget::MenuItem {
                                surface_id: "law-tree".to_owned(),
                                item_id: "root".to_owned(),
                            })
                        })
                        .transpose()?,
                    diagnostics: if self.hourei_store.is_some() {
                        Vec::new()
                    } else {
                        vec![Diagnostic::info(
                            "surface_deferred",
                            "Hourei law tree requires an opened Hourei store",
                        )]
                    },
                });
            }
            FormatFamily::Unknown => {}
        }
        surfaces.push(HomeSurface {
            surface_id: "search".to_owned(),
            kind: NavigationSurfaceKind::SearchFallback,
            status: NavigationStatus::Available,
            title_html: "Search".to_owned(),
            title_text: "Search".to_owned(),
            target: None,
            diagnostics: Vec::new(),
        });
        Ok(surfaces)
    }

    fn open_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        self.open_surface_page(surface_id, None, 100)
    }

    fn open_surface_page(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        if self.metadata.format_family == FormatFamily::Ssed && surface_id == "title-index" {
            return self.open_ssed_title_index_surface(surface_id, cursor, limit);
        }
        if self.metadata.format_family == FormatFamily::Ssed && surface_id == "menu" {
            return self.open_ssed_menu_surface(surface_id, SsedComponentRole::Menu, "MENU.DIC");
        }
        if self.metadata.format_family == FormatFamily::Ssed && surface_id == "toc" {
            return self.open_ssed_menu_surface(surface_id, SsedComponentRole::Toc, "TOC.DIC");
        }
        if self.metadata.format_family == FormatFamily::Ssed && surface_id == "hanrei" {
            return self.open_ssed_hanrei_surface(surface_id, cursor, limit);
        }
        if self.metadata.format_family == FormatFamily::Ssed
            && (surface_id == "panels" || surface_id.starts_with("panels:"))
        {
            return self.open_ssed_panel_surface(surface_id);
        }
        if self.metadata.format_family == FormatFamily::LvedSqlite3 && surface_id == "lved-list" {
            return self.open_lved_list_surface(surface_id, cursor, limit);
        }
        if self.metadata.format_family == FormatFamily::LvedSqlite3 && surface_id == "info" {
            return self.open_lved_info_surface(surface_id, cursor, limit);
        }
        if self.metadata.format_family == FormatFamily::LvedSqlite3 && surface_id == "lved-tree" {
            return self.open_lved_tree_surface(surface_id);
        }
        if self.metadata.format_family == FormatFamily::LvlMultiView && surface_id == "menuData" {
            return self.open_multiview_menu_surface(surface_id);
        }
        if self.metadata.format_family == FormatFamily::Hourei && surface_id == "law-tree" {
            return self.open_hourei_law_tree_surface(surface_id);
        }
        if self.metadata.format_family == FormatFamily::Ssed {
            let (code, message) = match surface_id {
                "panels" => (
                    "ssed_panels_deferred",
                    "SSED Panels.xml/Panel parsing is not implemented yet",
                ),
                _ => (
                    "surface_open_deferred",
                    "surface parsing is not implemented yet",
                ),
            };
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(code, message)],
            });
        }
        Ok(NavigationSurface::Deferred {
            surface_id: surface_id.to_owned(),
            diagnostics: vec![Diagnostic::info(
                "surface_open_deferred",
                "surface parsing will be implemented by the matching provider",
            )],
        })
    }
}

impl RendererProvider for StubBookPackage {
    fn render_target(
        &self,
        token: &TargetToken,
        options: &crate::render::RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let target = token.decode()?;
        match target {
            InternalTarget::Unsupported { reason } => Ok(ResolvedTargetView::unsupported(
                token.clone(),
                "Unsupported target",
                Diagnostic::warning("target_unsupported", reason),
            )),
            InternalTarget::Resource { resource, anchor } => {
                let decoded_resource = resource.decode()?;
                let resource_ref = self.resolve_resource(&resource)?;
                if let InternalResource::PackageFile {
                    path,
                    resource_kind,
                } = &decoded_resource
                    && (*resource_kind == ResourceKind::Html
                        || path_has_extension(path, &["html", "htm"]))
                {
                    return self.render_package_html_resource(
                        token.clone(),
                        &resource,
                        path,
                        resource_ref,
                        options,
                    );
                }
                if let InternalResource::ChmFile {
                    chm_path,
                    entry_path,
                    resource_kind,
                } = &decoded_resource
                    && (*resource_kind == ResourceKind::Html
                        || path_has_extension(entry_path, &["html", "htm"]))
                {
                    return self.render_chm_html_resource(
                        token.clone(),
                        &resource,
                        chm_path,
                        entry_path,
                        resource_ref,
                        options,
                    );
                }
                let diagnostics = resource_ref.diagnostics.clone();
                Ok(ResolvedTargetView {
                    kind: ResolvedTargetKind::MediaResource,
                    target: token.clone(),
                    title: resource_ref.label.clone(),
                    display_html: None,
                    basic_text: None,
                    scroll_anchor: anchor,
                    surface: None,
                    resources: vec![resource_ref],
                    links: Vec::new(),
                    capabilities: Vec::new(),
                    diagnostics,
                    debug_trace: None,
                })
            }
            InternalTarget::PanelCell { panel_id, .. } => {
                let surface_id = format!("panels:{panel_id}");
                self.view_for_navigation_surface_target(token.clone(), &surface_id, Some(panel_id))
            }
            InternalTarget::MenuItem { surface_id, .. }
            | InternalTarget::TocItem { surface_id, .. }
            | InternalTarget::TitleIndexItem { surface_id, .. } => {
                self.view_for_navigation_surface_target(token.clone(), &surface_id, None)
            }
            InternalTarget::MultiviewHref { href, anchor: _ } if href == "menuData.xml" => self
                .view_for_navigation_surface_target(
                    token.clone(),
                    "menuData",
                    Some("MultiView menu".to_owned()),
                ),
            _ => {
                let input = self.renderer_input_for_target(token)?;
                self.view_for_renderer_input(input, options)
            }
        }
    }
}

impl RendererInputProvider for StubBookPackage {
    fn renderer_input_for_target(&self, token: &TargetToken) -> Result<RendererInput> {
        let body = self.visual_body_for_target(token)?;
        self.renderer_input_from_visual_body(token.clone(), body)
    }
}

impl ResourceProvider for StubBookPackage {
    fn resolve_resource(&self, token: &ResourceToken) -> Result<ResourceRef> {
        match token.decode()? {
            InternalResource::PackageFile {
                path,
                resource_kind,
            } => {
                let relative = Path::new(&path);
                let resolved = self.storage.resolve_casefolded(relative)?;
                let mut diagnostics = Vec::new();
                let href = if resolved.is_some() {
                    Some(format!("lvcore://resource/{}", token.as_str()))
                } else {
                    diagnostics.push(Diagnostic::warning(
                        "resource_missing",
                        format!("{path} was not found in the package"),
                    ));
                    None
                };
                let label = resolved
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .or_else(|| relative.file_name())
                    .map(|value| value.to_string_lossy().to_string());
                Ok(ResourceRef {
                    token: token.clone(),
                    kind: resource_kind,
                    label,
                    href,
                    diagnostics,
                })
            }
            InternalResource::ChmFile {
                chm_path,
                entry_path,
                resource_kind,
            } => {
                let chm_relative = Path::new(&chm_path);
                let resolved = self.storage.resolve_casefolded(chm_relative)?;
                let mut diagnostics = Vec::new();
                let href = if resolved.is_some() {
                    Some(format!("lvcore://resource/{}", token.as_str()))
                } else {
                    diagnostics.push(Diagnostic::warning(
                        "resource_missing",
                        format!("{chm_path} was not found in the package"),
                    ));
                    None
                };
                let label = Path::new(&entry_path)
                    .file_name()
                    .map(|value| value.to_string_lossy().to_string())
                    .or(Some(entry_path));
                Ok(ResourceRef {
                    token: token.clone(),
                    kind: resource_kind,
                    label,
                    href,
                    diagnostics,
                })
            }
            InternalResource::MediaBlob { resource_kind, .. } => Ok(ResourceRef {
                token: token.clone(),
                kind: resource_kind,
                label: media_blob_label(token)?,
                href: self
                    .lved_store
                    .is_some()
                    .then(|| format!("lvcore://resource/{}", token.as_str())),
                diagnostics: if self.lved_store.is_some() {
                    Vec::new()
                } else {
                    vec![Diagnostic::info(
                        "resource_deferred",
                        "media blob resource resolution is not implemented yet for this package",
                    )]
                },
            }),
            InternalResource::Unsupported { reason } => Ok(ResourceRef {
                token: token.clone(),
                kind: ResourceKind::Other,
                label: None,
                href: None,
                diagnostics: vec![Diagnostic::warning("resource_unsupported", reason)],
            }),
        }
    }

    fn read_resource(&self, token: &ResourceToken) -> Result<Vec<u8>> {
        match token.decode()? {
            InternalResource::PackageFile { path, .. } => {
                let relative = Path::new(&path);
                let Some(resolved) = self.storage.resolve_casefolded(relative)? else {
                    return Err(Error::Driver(format!("resource not found: {path}")));
                };
                Ok(fs::read(resolved)?)
            }
            InternalResource::ChmFile {
                chm_path,
                entry_path,
                ..
            } => {
                let relative = Path::new(&chm_path);
                let Some(resolved) = self.storage.resolve_casefolded(relative)? else {
                    return Err(Error::Driver(format!("resource not found: {chm_path}")));
                };
                read_chm_entry(&resolved, &entry_path)
            }
            InternalResource::MediaBlob { store, key, .. } => {
                let Some(lved_store) = &self.lved_store else {
                    return Err(Error::Driver(
                        "media blob resource reading is not implemented yet for this package"
                            .to_owned(),
                    ));
                };
                let Some(bytes) = lved_store.media_blob(&store, &key)? else {
                    return Err(Error::Driver(format!(
                        "media blob not found: {store}:{key}"
                    )));
                };
                Ok(bytes)
            }
            InternalResource::Unsupported { reason } => Err(Error::Driver(reason)),
        }
    }
}

impl GaijiProvider for StubBookPackage {
    fn resolve_gaiji(&self, identity: &str, policy: &GaijiPolicy) -> GaijiResolution {
        let Some(code) = normalize_gaiji_identity(identity) else {
            return GaijiResolution {
                identity: identity.to_owned(),
                preferred_source: None,
                unicode: None,
                resource: None,
                nonliteral_marker: false,
                diagnostics: vec![Diagnostic::warning(
                    "gaiji_identity_invalid",
                    format!("{identity} is not a four-hex-digit LogoVista gaiji identity"),
                )],
            };
        };

        let unicode = self.gaiji_unicode_map.get(&code).cloned();
        let template_resource = self.template_gaiji_resource(&code);
        let ga16_resource = self.ga16_gaiji_resource_ref(&code);
        let preferred_source = policy.priority.iter().copied().find(|source| match source {
            GaijiSourcePreference::Unicode => unicode.is_some(),
            GaijiSourcePreference::ExternalResource => template_resource.is_some(),
            GaijiSourcePreference::Ga16Bitmap => ga16_resource.is_some(),
            GaijiSourcePreference::Unresolved => true,
        });
        let resource = match preferred_source {
            Some(GaijiSourcePreference::ExternalResource) => template_resource,
            Some(GaijiSourcePreference::Ga16Bitmap) => ga16_resource,
            _ => template_resource.or(ga16_resource),
        };
        let diagnostics = if matches!(
            preferred_source,
            None | Some(GaijiSourcePreference::Unresolved)
        ) {
            vec![Diagnostic::info(
                "gaiji_unresolved",
                format!("{code} was not resolved to Unicode, Template, or GA16 resource"),
            )]
        } else {
            Vec::new()
        };

        GaijiResolution {
            identity: code,
            preferred_source,
            unicode,
            resource,
            nonliteral_marker: false,
            diagnostics,
        }
    }
}

impl SequenceProvider for StubBookPackage {
    fn resolve_target_window(
        &self,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<TargetWindow> {
        if self.metadata.format_family == FormatFamily::Ssed
            && sequence_hint.is_none_or(|hint| matches!(hint, SequenceHint::TitleIndexOrder(_)))
            && let Some(window) =
                self.resolve_ssed_title_index_window(target, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::Ssed
            && matches!(sequence_hint, Some(SequenceHint::MenuOrder(_)))
            && let Some(window) =
                self.resolve_ssed_menu_window(target, sequence_hint, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::Ssed
            && matches!(sequence_hint, Some(SequenceHint::PanelOrder(_)))
            && let Some(window) =
                self.resolve_ssed_panel_window(target, sequence_hint, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::LvedSqlite3
            && matches!(sequence_hint, Some(SequenceHint::LvedTreeOrder))
            && let Some(window) = self.resolve_lved_tree_window(target, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::LvedSqlite3
            && sequence_hint.is_none_or(|hint| matches!(hint, SequenceHint::LvedListOrder))
            && let Some(window) = self.resolve_lved_list_window(target, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::LvlMultiView
            && sequence_hint.is_none()
            && let Some(window) =
                self.resolve_multiview_menu_window(target, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::Hourei
            && sequence_hint.is_none_or(|hint| matches!(hint, SequenceHint::HoureiLawArticleOrder))
            && let Some(window) = self.resolve_hourei_law_window(target, before, after, options)?
        {
            return Ok(window);
        }
        Ok(TargetWindow {
            center: self.render_target(target, options)?,
            before: Vec::new(),
            after: Vec::new(),
            diagnostics: vec![Diagnostic::info(
                "sequence_deferred",
                "sequence provider is not implemented yet",
            )],
        })
    }
}

impl BodyProvider for StubBookPackage {
    fn visual_body_for_target(&self, token: &TargetToken) -> Result<VisualBody> {
        match token.decode()? {
            InternalTarget::SsedDenseAnchor {
                anchor,
                resolver_hint,
            } => self.visual_body_for_ssed_dense_anchor(&anchor, resolver_hint.as_deref()),
            InternalTarget::SsedAddress {
                component,
                block,
                offset,
            } => self.visual_body_for_ssed_address(&component, block, offset),
            InternalTarget::LvedRow {
                table,
                row_id,
                anchor: _,
            } => self.visual_body_for_lved_row(&table, row_id),
            InternalTarget::LvedInfoPage { name, anchor: _ } => {
                self.visual_body_for_lved_info_name(&name)
            }
            InternalTarget::MultiviewHref { href, anchor } => {
                self.visual_body_for_multiview_href(&href, anchor.as_deref())
            }
            InternalTarget::HoureiLaw { hore_id, anchor: _ } => {
                self.visual_body_for_hourei_law(&hore_id)
            }
            _ => Ok(VisualBody::Unsupported {
                reason: "body provider deferred".to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "body_deferred",
                    "body provider is not implemented for this target",
                )],
            }),
        }
    }
}

impl StubBookPackage {
    fn search_lved_sqlite(&self, query: &SearchQuery) -> Result<SearchPage> {
        let Some(store) = &self.lved_store else {
            return Ok(SearchPage::deferred(
                "LVED_SQLITE3 search requires an opened SQLCipher store",
            ));
        };
        if query.limit == 0 {
            return Ok(SearchPage {
                hits: Vec::new(),
                next_cursor: None,
                diagnostics: Vec::new(),
            });
        }
        let offset = decode_offset_cursor(query.cursor.as_deref());
        let page_limit = query.limit.saturating_add(1);
        let mut raw_hits = store.search_page(&query.query, &query.mode, offset, page_limit)?;
        let next_cursor =
            (raw_hits.len() > query.limit).then(|| (offset + query.limit).to_string());
        raw_hits.truncate(query.limit);
        let hits = raw_hits
            .into_iter()
            .map(|hit| {
                let target = TargetToken::new(&InternalTarget::LvedRow {
                    table: "content".to_owned(),
                    row_id: hit.content_id,
                    anchor: hit.anchor,
                })?;
                let title_html = self.normalize_lved_label_html(&hit.title_html)?;
                let snippet_html = if hit.subtitle_html.is_empty() {
                    None
                } else {
                    Some(self.normalize_lved_label_html(&hit.subtitle_html)?)
                };
                Ok(SearchHit {
                    book_id: self.metadata.book_id.clone(),
                    target,
                    title_html,
                    title_text: hit.title_text,
                    snippet_html,
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(SearchPage {
            hits,
            next_cursor,
            diagnostics: Vec::new(),
        })
    }

    fn search_ssed_simple_indexes(&self, query: &SearchQuery) -> Result<SearchPage> {
        if query.limit == 0 {
            return Ok(SearchPage {
                hits: Vec::new(),
                next_cursor: None,
                diagnostics: Vec::new(),
            });
        }
        if query.mode == SearchMode::FullText {
            return self.search_ssed_fulltext_body_windows(query);
        }
        if !matches!(
            query.mode,
            SearchMode::Exact | SearchMode::Forward | SearchMode::Backward | SearchMode::Partial
        ) {
            return Ok(SearchPage::deferred(
                "SSED search mode is not implemented for simple title/index scanning yet",
            ));
        }

        let offset = decode_offset_cursor(query.cursor.as_deref());
        let page_limit = query.limit.saturating_add(1);
        let needle = query.query.to_lowercase();
        let mut collector =
            SsedIndexSearchCollector::new(self, &query.mode, &needle, offset, page_limit);
        if matches!(query.mode, SearchMode::Exact | SearchMode::Forward) {
            let scan_diagnostics =
                self.scan_ssed_simple_leaf_index_rows_near_key(&query.mode, &needle, |row| {
                    collector.push_row(row)
                })?;
            collector.extend_diagnostics(scan_diagnostics);
        }
        if !collector.has_hits() {
            let scan_diagnostics =
                self.scan_ssed_simple_index_rows(None, |row| collector.push_row(row))?;
            collector.extend_diagnostics(scan_diagnostics);
        }
        Ok(collector.into_search_page(query.limit))
    }

    fn search_ssed_fulltext_body_windows(&self, query: &SearchQuery) -> Result<SearchPage> {
        let needle = normalize_search_match_text(&query.query);
        if needle.is_empty() {
            return Ok(SearchPage {
                hits: Vec::new(),
                next_cursor: None,
                diagnostics: Vec::new(),
            });
        }

        let mut diagnostics = vec![Diagnostic::info(
            "ssed_fulltext_body_window_scan",
            format!(
                "SSED full-text search is scanning bounded HONMON windows behind native index targets ({} bytes per target)",
                SSED_FULLTEXT_BODY_WINDOW_BYTES
            ),
        )];
        let offset = decode_offset_cursor(query.cursor.as_deref());
        let page_limit = query.limit.saturating_add(1);
        let mut hits = Vec::new();
        let mut matched_count = 0usize;
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(SearchPage {
                hits,
                next_cursor: None,
                diagnostics: vec![Diagnostic::error(
                    "ssed_catalog_missing",
                    "SSED full-text search requires a parsed SSEDINFO catalog",
                )],
            });
        };
        let mut rows_by_component: BTreeMap<String, Vec<SsedFulltextRow>> = BTreeMap::new();
        let scan_diagnostics = self.scan_ssed_simple_index_rows(None, |row| {
            if looks_like_raw_anchor_label(&row.key) {
                return Ok(true);
            }
            let Some(component) = catalog.component_for_address(row.body.block) else {
                diagnostics.push(Diagnostic::warning(
                    "ssed_fulltext_body_component_missing",
                    format!(
                        "no component contains body pointer block {} offset {}",
                        row.body.block, row.body.offset
                    ),
                ));
                return Ok(true);
            };
            if component.role != SsedComponentRole::Honmon {
                return Ok(true);
            }
            let Some(component_offset) = component.relative_offset(row.body.block, row.body.offset)
            else {
                diagnostics.push(
                    Diagnostic::warning(
                        "ssed_fulltext_body_pointer_invalid",
                        format!(
                            "{} does not contain body pointer block {} offset {}",
                            component.filename, row.body.block, row.body.offset
                        ),
                    )
                    .with_context("component", &component.filename),
                );
                return Ok(true);
            };
            rows_by_component
                .entry(component.filename.clone())
                .or_default()
                .push(SsedFulltextRow {
                    offset: component_offset,
                    row,
                });
            Ok(true)
        })?;
        diagnostics.extend(scan_diagnostics);
        for rows in rows_by_component.values_mut() {
            rows.sort_by_key(|row| row.offset);
        }

        'components: for (component_name, rows) in rows_by_component {
            let Some(component) = catalog.component_named(&component_name) else {
                continue;
            };
            let path = match self.resolve_readable_ssed_component_path(component) {
                Ok(Some(path)) => path,
                Ok(None) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_fulltext_body_component_missing",
                            format!("{} is declared but not present on disk", component.filename),
                        )
                        .with_context("component", &component.filename),
                    );
                    continue;
                }
                Err(error) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_fulltext_body_component_decode_failed",
                            format!(
                                "{} is not readable as SSEDDATA: {error}",
                                component.filename
                            ),
                        )
                        .with_context("component", &component.filename),
                    );
                    continue;
                }
            };
            let mut reader = match SsedDataFile::open(&path) {
                Ok(reader) => reader,
                Err(error) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_fulltext_body_component_decode_failed",
                            format!(
                                "{} is not readable as SSEDDATA: {error}",
                                component.filename
                            ),
                        )
                        .with_context("component", &component.filename),
                    );
                    continue;
                }
            };
            let expanded_size = reader.header().expanded_size();
            let mut scan_offset = 0usize;
            let mut verified_offsets = BTreeSet::new();
            while scan_offset < expanded_size {
                let read_size = expanded_size
                    .saturating_sub(scan_offset)
                    .min(SSED_FULLTEXT_SCAN_WINDOW_BYTES + SSED_FULLTEXT_SCAN_OVERLAP_BYTES);
                let data = reader.read_range(scan_offset, read_size)?;
                if data.is_empty() {
                    break;
                }
                let window_text = decode_ssed_body_search_text(&data);
                if normalize_search_match_text(&window_text).contains(&needle) {
                    let lower = scan_offset.saturating_sub(SSED_FULLTEXT_BODY_WINDOW_BYTES) as u64;
                    let upper = scan_offset.saturating_add(read_size) as u64;
                    let start_index = rows.partition_point(|row| row.offset < lower);
                    for candidate_index in start_index..rows.len() {
                        let candidate = &rows[candidate_index];
                        if candidate.offset > upper {
                            break;
                        }
                        if !verified_offsets.insert(candidate.offset) {
                            continue;
                        }
                        let body_window_len = ssed_fulltext_body_window_len(&rows, candidate_index);
                        let body_data = reader.read_range(
                            usize::try_from(candidate.offset).map_err(|_| {
                                Error::Driver("SSED body offset does not fit in usize".to_owned())
                            })?,
                            body_window_len,
                        )?;
                        let body_text = decode_ssed_body_search_text(&body_data);
                        if !normalize_search_match_text(&body_text).contains(&needle) {
                            continue;
                        }
                        if matched_count < offset {
                            matched_count = matched_count.saturating_add(1);
                            continue;
                        }
                        let target = match self.ssed_target_for_index_pointer(candidate.row.body)? {
                            Ok(target) => target,
                            Err(diagnostic) => {
                                diagnostics.push(diagnostic);
                                continue;
                            }
                        };
                        let title = self.ssed_display_text_for_index_row(&candidate.row);
                        if looks_like_raw_anchor_label(&title) {
                            continue;
                        }
                        let label = self.ssed_rich_label(&title);
                        hits.push(SearchHit {
                            book_id: self.metadata.book_id.clone(),
                            target,
                            title_html: label.html,
                            title_text: label.text,
                            snippet_html: ssed_fulltext_snippet_html(&body_text, &query.query),
                            diagnostics: label.diagnostics,
                        });
                        matched_count = matched_count.saturating_add(1);
                        if hits.len() >= page_limit {
                            break 'components;
                        }
                    }
                }
                if scan_offset + SSED_FULLTEXT_SCAN_WINDOW_BYTES >= expanded_size {
                    break;
                }
                scan_offset += SSED_FULLTEXT_SCAN_WINDOW_BYTES;
            }
        }
        let next_cursor = (hits.len() > query.limit).then(|| (offset + query.limit).to_string());
        hits.truncate(query.limit);

        Ok(SearchPage {
            hits,
            next_cursor,
            diagnostics,
        })
    }

    fn search_multiview(&self, query: &SearchQuery) -> Result<SearchPage> {
        let Some(store) = &self.multiview_store else {
            return Ok(SearchPage::deferred(
                "LVLMultiView search requires opened LogoFontCipher SQLite payloads",
            ));
        };
        if query.limit == 0 {
            return Ok(SearchPage {
                hits: Vec::new(),
                next_cursor: None,
                diagnostics: Vec::new(),
            });
        }
        let offset = decode_offset_cursor(query.cursor.as_deref());
        let page_limit = query.limit.saturating_add(1);
        let mut raw_hits = store.search_page(&query.query, &query.mode, offset, page_limit)?;
        let next_cursor =
            (raw_hits.len() > query.limit).then(|| (offset + query.limit).to_string());
        raw_hits.truncate(query.limit);
        let hits = raw_hits
            .into_iter()
            .map(|hit| {
                Ok(SearchHit {
                    book_id: self.metadata.book_id.clone(),
                    target: TargetToken::new(&InternalTarget::MultiviewHref {
                        href: hit.href,
                        anchor: None,
                    })?,
                    title_html: hit.title_html,
                    title_text: hit.title_text,
                    snippet_html: hit.snippet_html,
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(SearchPage {
            hits,
            next_cursor,
            diagnostics: Vec::new(),
        })
    }

    fn search_hourei(&self, query: &SearchQuery) -> Result<SearchPage> {
        let Some(store) = &self.hourei_store else {
            return Ok(SearchPage::deferred(
                "Hourei search requires an opened Hourei store",
            ));
        };
        if query.limit == 0 {
            return Ok(SearchPage {
                hits: Vec::new(),
                next_cursor: None,
                diagnostics: Vec::new(),
            });
        }
        let offset = decode_offset_cursor(query.cursor.as_deref());
        let page_limit = query.limit.saturating_add(1);
        let mut raw_hits = store.search_page(&query.query, &query.mode, offset, page_limit)?;
        let next_cursor =
            (raw_hits.len() > query.limit).then(|| (offset + query.limit).to_string());
        raw_hits.truncate(query.limit);
        let hits = raw_hits
            .into_iter()
            .map(|hit| {
                Ok(SearchHit {
                    book_id: self.metadata.book_id.clone(),
                    target: TargetToken::new(&InternalTarget::HoureiLaw {
                        hore_id: hit.hore_id,
                        anchor: None,
                    })?,
                    title_html: hit.title_html,
                    title_text: hit.title_text,
                    snippet_html: hit.snippet_html,
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(SearchPage {
            hits,
            next_cursor,
            diagnostics: Vec::new(),
        })
    }

    fn open_ssed_title_index_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        if limit == 0 {
            return Ok(NavigationSurface::TitleIndexBrowse {
                surface_id: surface_id.to_owned(),
                items: Vec::new(),
                next_cursor: None,
            });
        }
        let offset = decode_offset_cursor(cursor);
        let (mut rows, mut diagnostics) =
            self.ssed_simple_index_rows_page(offset, limit.saturating_add(1))?;
        let next_cursor = (rows.len() > limit).then(|| (offset + limit).to_string());
        rows.truncate(limit);
        if rows.is_empty() && !diagnostics.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics,
            });
        }
        let mut items = Vec::new();
        for (index, row) in rows.into_iter().enumerate() {
            let label = self
                .ssed_title_text(row.title)
                .unwrap_or_else(|| row.key.clone());
            let label = self.ssed_rich_label(&label);
            let target = match self.ssed_target_for_index_pointer(row.body)? {
                Ok(target) => target,
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    continue;
                }
            };
            items.push(NavigationItem {
                item_id: format!("{}:{}", row.component, offset + index),
                label_html: label.html,
                label_text: label.text,
                target,
                diagnostics: label.diagnostics,
            });
        }
        Ok(NavigationSurface::TitleIndexBrowse {
            surface_id: surface_id.to_owned(),
            items,
            next_cursor,
        })
    }

    fn open_ssed_menu_surface(
        &self,
        surface_id: &str,
        role: SsedComponentRole,
        fallback_name: &str,
    ) -> Result<NavigationSurface> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "ssed_catalog_missing",
                    "SSED MENU/TOC surfaces require a parsed SSEDINFO catalog",
                )],
            });
        };
        let Some(component) = catalog
            .components_by_role(role)
            .find(|component| component.has_positive_range())
            .or_else(|| catalog.component_named(fallback_name))
        else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_navigation_component_missing",
                    format!("{fallback_name} is not declared in this SSED catalog"),
                )],
            });
        };
        let path = match self.resolve_readable_ssed_component_path(component) {
            Ok(Some(path)) => path,
            Ok(None) => {
                return Ok(NavigationSurface::Deferred {
                    surface_id: surface_id.to_owned(),
                    diagnostics: vec![
                        Diagnostic::warning(
                            "ssed_navigation_component_file_missing",
                            format!("{} is declared but not present on disk", component.filename),
                        )
                        .with_context("component", &component.filename),
                    ],
                });
            }
            Err(error) => {
                return Ok(NavigationSurface::Deferred {
                    surface_id: surface_id.to_owned(),
                    diagnostics: vec![
                        Diagnostic::warning(
                            "ssed_navigation_component_decode_failed",
                            format!(
                                "{} is not readable as SSEDDATA: {error}",
                                component.filename
                            ),
                        )
                        .with_context("component", &component.filename),
                    ],
                });
            }
        };
        let mut reader = match SsedDataFile::open(&path) {
            Ok(reader) => reader,
            Err(error) => {
                return Ok(NavigationSurface::Deferred {
                    surface_id: surface_id.to_owned(),
                    diagnostics: vec![
                        Diagnostic::warning(
                            "ssed_navigation_component_decode_failed",
                            format!(
                                "{} is not readable as plain SSEDDATA: {error}",
                                component.filename
                            ),
                        )
                        .with_context("component", &component.filename),
                    ],
                });
            }
        };
        let data = reader.read_range(0, reader.header().expanded_size())?;
        let parsed = parse_menu_stream(&data);
        if parsed.records.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![
                    Diagnostic::info(
                        "ssed_navigation_empty",
                        format!("{} did not decode any navigation rows", component.filename),
                    )
                    .with_context("component", &component.filename),
                ],
            });
        }
        let mut diagnostics = Vec::new();
        let nodes = ssed_menu_records_to_nodes(self, &parsed.records, &mut diagnostics)?;
        if nodes.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics,
            });
        }
        Ok(NavigationSurface::SimpleMenu {
            surface_id: surface_id.to_owned(),
            nodes,
        })
    }

    fn open_ssed_panel_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        let Some(path) = self.storage.resolve_casefolded(Path::new("Panels.xml"))? else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_panels_missing",
                    "Panels.xml was not found",
                )],
            });
        };
        let parsed = match parse_panel_xml_bytes(&fs::read(path)?) {
            Ok(parsed) => parsed,
            Err(error) => {
                return Ok(NavigationSurface::Deferred {
                    surface_id: surface_id.to_owned(),
                    diagnostics: vec![Diagnostic::warning(
                        "ssed_panels_xml_parse_failed",
                        format!("Panels.xml could not be parsed: {error}"),
                    )],
                });
            }
        };
        let requested_panel_id = surface_id
            .strip_prefix("panels:")
            .filter(|id| !id.is_empty());
        let root_panel_id = requested_panel_id.or_else(|| {
            parsed
                .inline_cells
                .first()
                .map(|cell| cell.panel_id.as_str())
        });
        let inline_cells = parsed
            .inline_cells
            .iter()
            .filter(|cell| root_panel_id.is_none_or(|panel_id| cell.panel_id == panel_id))
            .cloned()
            .collect::<Vec<_>>();
        let include_external_bins = requested_panel_id.is_some() || inline_cells.is_empty();
        let mut diagnostics = Vec::new();
        let mut cells = Vec::new();
        for cell in inline_cells {
            cells.push(ssed_panel_inline_cell_to_navigation_cell(self, &cell)?);
        }
        for data_ref in parsed.data_refs.into_iter().filter(|data_ref| {
            include_external_bins
                && requested_panel_id.is_none_or(|panel_id| data_ref.panel_id == panel_id)
        }) {
            let relative = data_ref.filename.replace('\\', "/");
            let Some(path) = self.storage.resolve_casefolded(Path::new(&relative))? else {
                diagnostics.push(Diagnostic::warning(
                    "ssed_panel_bin_missing",
                    format!("Panel BIN {} was not found", data_ref.filename),
                ));
                continue;
            };
            let panel = match parse_panel_bin(&fs::read(path)?) {
                Ok(panel) => panel,
                Err(error) => {
                    diagnostics.push(Diagnostic::warning(
                        "ssed_panel_bin_parse_failed",
                        format!(
                            "Panel BIN {} could not be parsed: {error}",
                            data_ref.filename
                        ),
                    ));
                    continue;
                }
            };
            for record in panel.records {
                cells.push(ssed_panel_bin_record_to_navigation_cell(
                    self,
                    &data_ref,
                    &record,
                    &mut diagnostics,
                )?);
            }
        }
        if cells.is_empty() {
            if diagnostics.is_empty() {
                diagnostics.push(Diagnostic::info(
                    "ssed_panels_empty",
                    "Panels.xml did not expose inline cells or decoded BIN rows",
                ));
            }
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics,
            });
        }
        Ok(NavigationSurface::Panel {
            surface_id: surface_id.to_owned(),
            cells,
        })
    }

    fn open_ssed_hanrei_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        if cursor.is_none()
            && limit > 0
            && let Some(nodes) = self.discover_ssed_hanrei_chm_toc_nodes("HANREI.chm")?
        {
            return Ok(NavigationSurface::HierarchicalTree {
                surface_id: surface_id.to_owned(),
                nodes,
            });
        }
        if limit == 0 {
            return Ok(NavigationSurface::InfoPages {
                surface_id: surface_id.to_owned(),
                pages: Vec::new(),
                next_cursor: None,
            });
        }
        let offset = decode_offset_cursor(cursor);
        let mut pages = self.discover_ssed_hanrei_pages()?;
        if pages.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_hanrei_missing",
                    "SSED HANREI files were not found",
                )],
            });
        }
        let next_cursor = (pages.len() > offset + limit).then(|| (offset + limit).to_string());
        pages = pages.into_iter().skip(offset).take(limit).collect();
        let items = pages
            .into_iter()
            .map(|page| {
                let resource = ResourceToken::new(&page.resource)?;
                Ok(NavigationItem {
                    item_id: page.item_id,
                    label_html: escape_plain_label_html(&page.label),
                    label_text: page.label,
                    target: TargetToken::new(&InternalTarget::Resource {
                        resource,
                        anchor: page.anchor,
                    })?,
                    diagnostics: page.diagnostics,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::InfoPages {
            surface_id: surface_id.to_owned(),
            pages: items,
            next_cursor,
        })
    }

    fn discover_ssed_hanrei_chm_toc_nodes(
        &self,
        chm_path: &str,
    ) -> Result<Option<Vec<NavigationNode>>> {
        if !self.storage.exists(Path::new(chm_path))? {
            return Ok(None);
        }
        let Some(resolved) = self.storage.resolve_casefolded(Path::new(chm_path))? else {
            return Ok(None);
        };
        let Ok(entries) = list_chm_entries(&resolved) else {
            return Ok(None);
        };
        let mut toc_items = Vec::new();
        for entry in &entries {
            if !path_has_extension(&entry.path, &["hhc"]) {
                continue;
            }
            let Ok(bytes) = read_chm_entry(&resolved, &entry.path) else {
                continue;
            };
            let html = decode_package_html_text(&bytes);
            toc_items.extend(parse_chm_hhc_toc(&html));
        }
        if toc_items.is_empty() {
            return Ok(None);
        }
        let nodes = chm_hhc_toc_items_to_nodes(chm_path, &toc_items)?;
        Ok((!nodes.is_empty()).then_some(nodes))
    }

    fn open_lved_list_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        let Some(store) = &self.lved_store else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 list surface requires an opened SQLCipher store",
                )],
            });
        };
        if limit == 0 {
            return Ok(NavigationSurface::TitleIndexBrowse {
                surface_id: surface_id.to_owned(),
                items: Vec::new(),
                next_cursor: None,
            });
        }
        let offset = decode_offset_cursor(cursor);
        let mut rows = store.list_items_page(offset, limit.saturating_add(1))?;
        let next_cursor = (rows.len() > limit).then(|| (offset + limit).to_string());
        rows.truncate(limit);
        if rows.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "surface_missing",
                    "LVED_SQLITE3 list table did not expose renderable rows",
                )],
            });
        }
        let items = rows
            .into_iter()
            .map(|row| {
                let label_html = self.normalize_lved_label_html(&lved_list_label_html(
                    &row.title_html,
                    &row.subtitle_html,
                ))?;
                let label_text = if row.subtitle_html.is_empty() {
                    row.title_text.clone()
                } else {
                    format!("{} {}", row.title_text, html_label_text(&row.subtitle_html))
                };
                Ok(NavigationItem {
                    item_id: row.list_id.to_string(),
                    label_html,
                    label_text,
                    target: TargetToken::new(&InternalTarget::LvedRow {
                        table: "content".to_owned(),
                        row_id: row.content_id,
                        anchor: row.anchor,
                    })?,
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::TitleIndexBrowse {
            surface_id: surface_id.to_owned(),
            items,
            next_cursor,
        })
    }

    fn open_lved_info_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        let Some(store) = &self.lved_store else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 info surface requires an opened SQLCipher store",
                )],
            });
        };
        if limit == 0 {
            return Ok(NavigationSurface::InfoPages {
                surface_id: surface_id.to_owned(),
                pages: Vec::new(),
                next_cursor: None,
            });
        }
        let offset = decode_offset_cursor(cursor);
        let mut pages = store.info_pages_page(offset, limit.saturating_add(1))?;
        let next_cursor = (pages.len() > limit).then(|| (offset + limit).to_string());
        pages.truncate(limit);
        if pages.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "surface_missing",
                    "LVED_SQLITE3 info table did not expose renderable pages",
                )],
            });
        }
        let items = pages
            .into_iter()
            .map(|page| {
                Ok(NavigationItem {
                    item_id: page.name,
                    label_html: page.title_html,
                    label_text: page.title_text,
                    target: TargetToken::new(&InternalTarget::LvedRow {
                        table: "info".to_owned(),
                        row_id: page.id,
                        anchor: None,
                    })?,
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::InfoPages {
            surface_id: surface_id.to_owned(),
            pages: items,
            next_cursor,
        })
    }

    fn open_lved_tree_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        let Some(store) = &self.lved_store else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 tree surface requires an opened SQLCipher store",
                )],
            });
        };
        let rows = store.tree_index_items()?;
        if rows.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "surface_missing",
                    "LVED_SQLITE3 tree.idx did not expose navigation rows",
                )],
            });
        }
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes: lved_tree_items_to_nodes(&rows)?,
        })
    }

    fn open_multiview_menu_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        let bytes = self.storage.read(Path::new("menuData.xml"))?;
        let xml = String::from_utf8(bytes)
            .map_err(|error| Error::Driver(format!("menuData.xml is not valid UTF-8: {error}")))?;
        let items = parse_menu_data(&xml)?;
        let nodes = items
            .iter()
            .enumerate()
            .map(|(index, item)| multiview_menu_item_to_node(item, &index.to_string()))
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
        })
    }

    fn open_hourei_law_tree_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        let Some(store) = &self.hourei_store else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "hourei_store_missing",
                    "Hourei law tree requires an opened Hourei store",
                )],
            });
        };
        let categories = store.categories_with_laws()?;
        let nodes = categories
            .into_iter()
            .map(|category| {
                let children = category
                    .laws
                    .into_iter()
                    .map(|law| {
                        let label = hourei_law_node_label(&law);
                        Ok(NavigationNode {
                            node_id: format!("law:{}", law.hore_id),
                            label_html: escape_hourei_label_html(&label),
                            label_text: label,
                            target: Some(TargetToken::new(&InternalTarget::HoureiLaw {
                                hore_id: law.hore_id,
                                anchor: None,
                            })?),
                            diagnostics: Vec::new(),
                            children: Vec::new(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(NavigationNode {
                    node_id: format!("category:{}", category.id),
                    label_html: escape_hourei_label_html(&category.name),
                    label_text: category.name,
                    target: None,
                    diagnostics: Vec::new(),
                    children,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
        })
    }

    fn ssed_simple_index_rows_page(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<SsedIndexRow>, Vec<Diagnostic>)> {
        if limit == 0 {
            return Ok((Vec::new(), Vec::new()));
        }
        let mut rows = Vec::new();
        let mut seen = 0usize;
        let diagnostics = self.scan_ssed_simple_index_rows(None, |row| {
            if seen >= offset {
                rows.push(row);
            }
            seen = seen.saturating_add(1);
            Ok(rows.len() < limit)
        })?;
        Ok((rows, diagnostics))
    }

    fn scan_ssed_simple_leaf_index_rows_near_key(
        &self,
        mode: &SearchMode,
        needle: &str,
        mut on_row: impl FnMut(SsedIndexRow) -> Result<bool>,
    ) -> Result<Vec<Diagnostic>> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(vec![Diagnostic::error(
                "ssed_catalog_missing",
                "SSED index scanning requires a parsed SSEDINFO catalog",
            )]);
        };
        let mut diagnostics = Vec::new();
        'components: for component in catalog.components_by_role(SsedComponentRole::Index) {
            if !is_simple_leaf_index_type(component.component_type) {
                continue;
            }
            if matches!(mode, SearchMode::Exact | SearchMode::Forward)
                && component.filename.to_ascii_uppercase().starts_with('B')
            {
                continue;
            }
            let path = match self.resolve_readable_ssed_component_path(component) {
                Ok(Some(path)) => path,
                Ok(None) => continue,
                Err(error) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_index_component_decode_failed",
                            format!(
                                "{} is not readable as SSEDDATA: {error}",
                                component.filename
                            ),
                        )
                        .with_context("component", &component.filename),
                    );
                    continue;
                }
            };
            let mut reader = SsedDataFile::open(&path)?;
            let page_count = component.block_count() as usize;
            let start_page =
                match self.ssed_simple_index_candidate_leaf_page(component, &mut reader, needle)? {
                    Some(page_index) => page_index,
                    None => continue,
                };
            let mut saw_match = false;
            for page_index in start_page..page_count {
                let page = reader.read_range(page_index * INDEX_PAGE_SIZE, INDEX_PAGE_SIZE)?;
                if page.len() < 4 {
                    break;
                }
                let word = u16::from_be_bytes([page[0], page[1]]);
                if !is_leaf_page(word) {
                    continue;
                }
                let logical_block = component.start_block + page_index as u32;
                let (rows, unknown) = parse_simple_leaf_page(
                    &component.filename,
                    &page,
                    page_index as u32,
                    logical_block,
                );
                if unknown > 0 {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_index_unknown_leaf_bytes",
                            format!(
                                "{} had {unknown} unknown simple leaf row(s)",
                                component.filename
                            ),
                        )
                        .with_context("component", &component.filename),
                    );
                }
                for row in rows {
                    let key = row.key.to_lowercase();
                    let row_matches = match mode {
                        SearchMode::Exact => key == needle,
                        SearchMode::Forward => key.starts_with(needle),
                        _ => false,
                    };
                    let passed_match_region = match mode {
                        SearchMode::Exact => key.as_str() > needle,
                        SearchMode::Forward => {
                            saw_match && !key.starts_with(needle) && key.as_str() > needle
                        }
                        _ => false,
                    };
                    if row_matches {
                        saw_match = true;
                        if !on_row(row)? {
                            break 'components;
                        }
                    } else if passed_match_region {
                        break;
                    }
                }
            }
        }
        Ok(diagnostics)
    }

    fn ssed_simple_index_candidate_leaf_page(
        &self,
        component: &SsedComponent,
        reader: &mut SsedDataFile,
        needle: &str,
    ) -> Result<Option<usize>> {
        let page_count = component.block_count() as usize;
        if page_count == 0 {
            return Ok(None);
        }
        let mut page_index = 0usize;
        let mut guard = 0usize;
        while page_index < page_count && guard <= page_count {
            guard = guard.saturating_add(1);
            let page = reader.read_range(page_index * INDEX_PAGE_SIZE, INDEX_PAGE_SIZE)?;
            if page.len() < 4 {
                return Ok(None);
            }
            let word = u16::from_be_bytes([page[0], page[1]]);
            if is_leaf_page(word) {
                return Ok(Some(page_index));
            }
            let rows = parse_internal_page(
                &component.filename,
                &page,
                page_index as u32,
                component.start_block + page_index as u32,
            );
            let Some(child_block) = rows
                .iter()
                .find(|row| {
                    row.raw_key.iter().all(|value| *value == 0xff)
                        || row.key.to_lowercase().as_str() >= needle
                })
                .or_else(|| rows.last())
                .map(|row| row.child_block)
            else {
                return Ok(None);
            };
            if child_block < component.start_block {
                return Ok(None);
            }
            page_index = (child_block - component.start_block) as usize;
        }
        Ok(None)
    }

    fn scan_ssed_simple_index_rows(
        &self,
        row_limit: Option<usize>,
        mut on_row: impl FnMut(SsedIndexRow) -> Result<bool>,
    ) -> Result<Vec<Diagnostic>> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(vec![Diagnostic::error(
                "ssed_catalog_missing",
                "SSED index scanning requires a parsed SSEDINFO catalog",
            )]);
        };
        let mut diagnostics = Vec::new();
        let mut row_count = 0usize;
        'components: for component in catalog.components_by_role(SsedComponentRole::Index) {
            if row_limit.is_some_and(|limit| row_count >= limit) {
                break;
            }
            if !is_supported_index_type(component.component_type) {
                diagnostics.push(
                    Diagnostic::info(
                        "ssed_index_variant_deferred",
                        format!("{} is not a supported index component", component.filename),
                    )
                    .with_context("component", &component.filename),
                );
                continue;
            }
            let path = match self.resolve_readable_ssed_component_path(component) {
                Ok(Some(path)) => path,
                Ok(None) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_index_component_missing",
                            format!("{} is declared but not present on disk", component.filename),
                        )
                        .with_context("component", &component.filename),
                    );
                    continue;
                }
                Err(error) => {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_index_component_decode_failed",
                            format!(
                                "{} is not readable as SSEDDATA: {error}",
                                component.filename
                            ),
                        )
                        .with_context("component", &component.filename),
                    );
                    continue;
                }
            };
            let mut reader = SsedDataFile::open(&path)?;
            let page_count = component.block_count() as usize;
            let mut scan_state = SsedIndexScanState::default();
            for page_index in 0..page_count {
                if row_limit.is_some_and(|limit| row_count >= limit) {
                    break;
                }
                let page = reader.read_range(page_index * INDEX_PAGE_SIZE, INDEX_PAGE_SIZE)?;
                if page.len() < 4 {
                    break;
                }
                let word = u16::from_be_bytes([page[0], page[1]]);
                if !is_leaf_page(word) {
                    continue;
                }
                let logical_block = component.start_block + page_index as u32;
                let (page_rows, unknown) = parse_supported_leaf_page(
                    &component.filename,
                    component.component_type,
                    &page,
                    page_index as u32,
                    logical_block,
                    &mut scan_state,
                );
                if unknown > 0 {
                    diagnostics.push(
                        Diagnostic::warning(
                            "ssed_index_unknown_leaf_bytes",
                            format!(
                                "{} had {unknown} unknown simple leaf row(s)",
                                component.filename
                            ),
                        )
                        .with_context("component", &component.filename),
                    );
                }
                for row in page_rows {
                    if row_limit.is_some_and(|limit| row_count >= limit) {
                        break 'components;
                    }
                    row_count = row_count.saturating_add(1);
                    if !on_row(row)? {
                        break 'components;
                    }
                }
            }
        }
        Ok(diagnostics)
    }

    fn ssed_title_text(&self, pointer: SsedIndexPointer) -> Option<String> {
        let catalog = self.ssed_catalog.as_ref()?;
        let component = catalog.component_for_address(pointer.block)?;
        if component.role != SsedComponentRole::Title {
            return None;
        }
        let component_offset = component.relative_offset(pointer.block, pointer.offset)?;
        let path = self
            .resolve_readable_ssed_component_path(component)
            .ok()
            .flatten()?;
        let mut reader = SsedDataFile::open(path).ok()?;
        let data = reader
            .read_range(usize::try_from(component_offset).ok()?, 512)
            .ok()?;
        let title = decode_title_text(&data);
        (!title.is_empty()).then_some(title)
    }

    fn ssed_rich_label(&self, value: &str) -> RichLabel {
        resolve_rich_label(self, value, &GaijiPolicy::default())
    }

    fn ssed_target_for_index_pointer(
        &self,
        pointer: SsedIndexPointer,
    ) -> Result<std::result::Result<TargetToken, Diagnostic>> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(Err(Diagnostic::error(
                "ssed_catalog_missing",
                "SSED index body pointers require a parsed SSEDINFO catalog",
            )));
        };
        let Some(component) = catalog.component_for_address(pointer.block) else {
            return Ok(Err(Diagnostic::warning(
                "ssed_index_body_component_missing",
                format!(
                    "no component contains index body pointer block {} offset {}",
                    pointer.block, pointer.offset
                ),
            )));
        };
        if component
            .relative_offset(pointer.block, pointer.offset)
            .is_none()
        {
            return Ok(Err(Diagnostic::warning(
                "ssed_index_body_pointer_invalid",
                format!(
                    "{} does not contain index body pointer block {} offset {}",
                    component.filename, pointer.block, pointer.offset
                ),
            )
            .with_context("component", &component.filename)));
        }
        Ok(Ok(TargetToken::new(&InternalTarget::SsedAddress {
            component: component.filename.clone(),
            block: pointer.block,
            offset: pointer.offset,
        })?))
    }

    fn resolve_ssed_title_index_window(
        &self,
        target: &TargetToken,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let InternalTarget::SsedAddress {
            component,
            block,
            offset,
        } = target.decode()?
        else {
            return Ok(None);
        };

        let mut rows = Vec::new();
        let mut diagnostics = self.scan_ssed_simple_index_rows(None, |row| {
            rows.push(row);
            Ok(true)
        })?;
        if rows.is_empty() {
            diagnostics.push(Diagnostic::info(
                "sequence_deferred",
                "SSED title/index order is unavailable for this target",
            ));
            return Ok(Some(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics,
            }));
        }

        let center_index = rows.iter().position(|row| {
            row.body.block == block
                && row.body.offset == offset
                && self
                    .ssed_component_for_index_pointer(row.body)
                    .is_some_and(|row_component| row_component.eq_ignore_ascii_case(&component))
        });
        let Some(center_index) = center_index else {
            diagnostics.push(Diagnostic::info(
                "sequence_target_not_in_title_index",
                "target is not present in the simple SSED title/index order",
            ));
            return Ok(Some(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics,
            }));
        };

        let mut center = self.render_target(target, options)?;
        let center_label = self.ssed_index_row_label(&rows[center_index]);
        center.title = Some(center_label.text);
        center.diagnostics.extend(center_label.diagnostics);
        let before_start = center_index.saturating_sub(before);
        let after_end = rows
            .len()
            .min(center_index.saturating_add(after).saturating_add(1));

        let mut before_views = Vec::new();
        for row in &rows[before_start..center_index] {
            if let Some(view) = self.render_ssed_index_row(row, options, &mut diagnostics)? {
                before_views.push(view);
            }
        }
        let mut after_views = Vec::new();
        for row in &rows[center_index + 1..after_end] {
            if let Some(view) = self.render_ssed_index_row(row, options, &mut diagnostics)? {
                after_views.push(view);
            }
        }

        Ok(Some(TargetWindow {
            center,
            before: before_views,
            after: after_views,
            diagnostics,
        }))
    }

    fn render_ssed_index_row(
        &self,
        row: &SsedIndexRow,
        options: &RenderOptions,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Option<ResolvedTargetView>> {
        let target = match self.ssed_target_for_index_pointer(row.body)? {
            Ok(target) => target,
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                return Ok(None);
            }
        };
        let mut view = self.render_target(&target, options)?;
        let label = self.ssed_index_row_label(row);
        view.title = Some(label.text);
        view.diagnostics.extend(label.diagnostics);
        Ok(Some(view))
    }

    fn resolve_ssed_menu_window(
        &self,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let Some(SequenceHint::MenuOrder(surface_id)) = sequence_hint else {
            return Ok(None);
        };
        let surface = self.open_surface(surface_id)?;
        let NavigationSurface::SimpleMenu { nodes, .. } = surface else {
            return Ok(Some(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![Diagnostic::info(
                    "sequence_surface_not_ordered",
                    format!("{surface_id} is not a simple SSED MENU/TOC surface"),
                )],
            }));
        };
        let mut ordered = Vec::new();
        collect_navigation_node_ordered_targets(&nodes, &mut ordered);
        Ok(Some(self.resolve_ordered_target_window(
            target,
            &ordered,
            before,
            after,
            options,
            Diagnostic::info(
                "sequence_target_not_in_ssed_menu",
                "target is not present in the requested SSED MENU/TOC order",
            ),
        )?))
    }

    fn resolve_ssed_panel_window(
        &self,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let Some(SequenceHint::PanelOrder(panel_id)) = sequence_hint else {
            return Ok(None);
        };
        let surface_id = if panel_id == "panels" || panel_id.starts_with("panels:") {
            panel_id.clone()
        } else {
            format!("panels:{panel_id}")
        };
        let surface = self.open_surface(&surface_id)?;
        let NavigationSurface::Panel { cells, .. } = surface else {
            return Ok(Some(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![Diagnostic::info(
                    "sequence_surface_not_ordered",
                    format!("{surface_id} is not an SSED panel surface"),
                )],
            }));
        };
        let mut ordered = Vec::new();
        collect_panel_cell_ordered_targets(&cells, &mut ordered);
        Ok(Some(self.resolve_ordered_target_window(
            target,
            &ordered,
            before,
            after,
            options,
            Diagnostic::info(
                "sequence_target_not_in_ssed_panel",
                "target is not present in the requested SSED panel order",
            ),
        )?))
    }

    fn resolve_ordered_target_window(
        &self,
        target: &TargetToken,
        ordered: &[OrderedSequenceTarget],
        before: usize,
        after: usize,
        options: &RenderOptions,
        not_found_diagnostic: Diagnostic,
    ) -> Result<TargetWindow> {
        let Some(center_index) = ordered
            .iter()
            .position(|candidate| &candidate.target == target)
        else {
            return Ok(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![not_found_diagnostic],
            });
        };

        let mut center = self.render_target(target, options)?;
        if let Some(title) = &ordered[center_index].title {
            center.title = Some(title.clone());
        }

        let before_start = center_index.saturating_sub(before);
        let before_views = ordered[before_start..center_index]
            .iter()
            .map(|item| self.render_ordered_sequence_target(item, options))
            .collect::<Result<Vec<_>>>()?;
        let after_end = (center_index + 1 + after).min(ordered.len());
        let after_views = ordered[center_index + 1..after_end]
            .iter()
            .map(|item| self.render_ordered_sequence_target(item, options))
            .collect::<Result<Vec<_>>>()?;

        Ok(TargetWindow {
            center,
            before: before_views,
            after: after_views,
            diagnostics: Vec::new(),
        })
    }

    fn render_ordered_sequence_target(
        &self,
        item: &OrderedSequenceTarget,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let mut view = self.render_target(&item.target, options)?;
        if let Some(title) = &item.title {
            view.title = Some(title.clone());
        }
        Ok(view)
    }

    fn template_gaiji_resource(&self, code: &str) -> Option<ResourceRef> {
        for extension in ["svg", "png", "gif", "jpg", "jpeg"] {
            let candidate = format!("Templates/{code}.{extension}");
            if self
                .storage
                .resolve_casefolded(Path::new(&candidate))
                .ok()
                .flatten()
                .is_none()
            {
                continue;
            }
            let token = ResourceToken::new(&InternalResource::PackageFile {
                path: candidate,
                resource_kind: ResourceKind::Template,
            })
            .ok()?;
            return self.resolve_resource(&token).ok();
        }
        None
    }

    fn ga16_gaiji_resource_ref(&self, code: &str) -> Option<ResourceRef> {
        let first = code.as_bytes().first()?.to_ascii_uppercase();
        let candidates: &[&str] = match first {
            b'A' => &["GA16HALF", "GAI16H", "GAI16H00"],
            b'B' => &["GA16FULL", "GAI16F", "GAI16F00"],
            _ => &[],
        };
        for candidate in candidates {
            let Some(path) = self
                .storage
                .resolve_casefolded(Path::new(candidate))
                .ok()
                .flatten()
            else {
                continue;
            };
            let Ok(data) = fs::read(&path) else {
                continue;
            };
            if !ga16_resource_covers_code(&data, code) {
                continue;
            }
            let token = ResourceToken::new(&InternalResource::PackageFile {
                path: (*candidate).to_owned(),
                resource_kind: ResourceKind::Other,
            })
            .ok()?;
            let mut resource = self.resolve_resource(&token).ok()?;
            resource.diagnostics.push(Diagnostic::info(
                "ga16_glyph_extraction_deferred",
                format!("{code} maps to a GA16 bitmap resource; glyph extraction is deferred"),
            ));
            return Some(resource);
        }
        None
    }

    fn resolve_lved_list_window(
        &self,
        target: &TargetToken,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let InternalTarget::LvedRow {
            table,
            row_id,
            anchor: _,
        } = target.decode()?
        else {
            return Ok(None);
        };
        if !table.eq_ignore_ascii_case("content") {
            return Ok(None);
        }
        let Some(store) = &self.lved_store else {
            return Ok(None);
        };
        let Some(window) = store.list_window_for_content(row_id, before, after)? else {
            return Ok(Some(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![Diagnostic::info(
                    "sequence_target_not_in_lved_list",
                    "target is not present in the LVED list order",
                )],
            }));
        };

        let mut center = self.render_lved_list_hit(&window.center, options)?;
        center.title = Some(window.center.title_text);
        let before = window
            .before
            .iter()
            .map(|hit| self.render_lved_list_hit(hit, options))
            .collect::<Result<Vec<_>>>()?;
        let after = window
            .after
            .iter()
            .map(|hit| self.render_lved_list_hit(hit, options))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(TargetWindow {
            center,
            before,
            after,
            diagnostics: Vec::new(),
        }))
    }

    fn render_lved_list_hit(
        &self,
        hit: &crate::lved_sqlite::LvedSearchHit,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let target = TargetToken::new(&InternalTarget::LvedRow {
            table: "content".to_owned(),
            row_id: hit.content_id,
            anchor: hit.anchor.clone(),
        })?;
        let mut view = self.render_target(&target, options)?;
        view.title = Some(hit.title_text.clone());
        Ok(view)
    }

    fn resolve_lved_tree_window(
        &self,
        target: &TargetToken,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let InternalTarget::LvedRow {
            table,
            row_id,
            anchor: _,
        } = target.decode()?
        else {
            return Ok(None);
        };
        if !table.eq_ignore_ascii_case("content") {
            return Ok(None);
        }
        let Some(store) = &self.lved_store else {
            return Ok(None);
        };
        let rows = store
            .tree_index_items()?
            .into_iter()
            .filter(|row| row.data_id >= 0)
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return Ok(None);
        }
        let Some(center_index) = rows.iter().position(|row| row.data_id == row_id) else {
            return Ok(Some(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![Diagnostic::info(
                    "sequence_target_not_in_lved_tree",
                    "target is not present in the LVED tree.idx order",
                )],
            }));
        };
        let mut center = self.render_lved_tree_item(&rows[center_index], options)?;
        center.title = Some(rows[center_index].label.clone());
        let before_start = center_index.saturating_sub(before);
        let before_views = rows[before_start..center_index]
            .iter()
            .map(|row| self.render_lved_tree_item(row, options))
            .collect::<Result<Vec<_>>>()?;
        let after_end = (center_index + 1 + after).min(rows.len());
        let after_views = rows[center_index + 1..after_end]
            .iter()
            .map(|row| self.render_lved_tree_item(row, options))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(TargetWindow {
            center,
            before: before_views,
            after: after_views,
            diagnostics: Vec::new(),
        }))
    }

    fn render_lved_tree_item(
        &self,
        item: &crate::lved_sqlite::LvedTreeIndexItem,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let target = TargetToken::new(&InternalTarget::LvedRow {
            table: "content".to_owned(),
            row_id: item.data_id,
            anchor: None,
        })?;
        let mut view = self.render_target(&target, options)?;
        view.title = Some(item.label.clone());
        Ok(view)
    }

    fn resolve_multiview_menu_window(
        &self,
        target: &TargetToken,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let InternalTarget::MultiviewHref { href, anchor } = target.decode()? else {
            return Ok(None);
        };
        let surface = self.open_multiview_menu_surface("menuData")?;
        let NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
            return Ok(None);
        };
        let mut ordered = Vec::new();
        collect_navigation_node_targets(&nodes, &mut ordered);
        let Some(center_index) = ordered.iter().position(|candidate| {
            matches!(
                candidate.decode(),
                Ok(InternalTarget::MultiviewHref {
                    href: candidate_href,
                    anchor: candidate_anchor,
                }) if candidate_href == href && candidate_anchor == anchor
            )
        }) else {
            return Ok(None);
        };
        let before_start = center_index.saturating_sub(before);
        let before_views = ordered[before_start..center_index]
            .iter()
            .map(|token| self.render_target(token, options))
            .collect::<Result<Vec<_>>>()?;
        let after_end = (center_index + 1 + after).min(ordered.len());
        let after_views = ordered[center_index + 1..after_end]
            .iter()
            .map(|token| self.render_target(token, options))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(TargetWindow {
            center: self.render_target(target, options)?,
            before: before_views,
            after: after_views,
            diagnostics: Vec::new(),
        }))
    }

    fn resolve_hourei_law_window(
        &self,
        target: &TargetToken,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let InternalTarget::HoureiLaw { hore_id, .. } = target.decode()? else {
            return Ok(None);
        };
        let Some(store) = &self.hourei_store else {
            return Ok(None);
        };
        let Some(window) = store.law_window(&hore_id, before, after)? else {
            return Ok(Some(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![Diagnostic::info(
                    "sequence_target_not_in_hourei_law_order",
                    "target is not present in the Hourei kana-order law list",
                )],
            }));
        };
        let mut center = self.render_target(target, options)?;
        center.title = Some(hourei_law_node_label(&window.center));
        let before = window
            .before
            .into_iter()
            .map(|entry| self.render_hourei_law_entry(&entry, options))
            .collect::<Result<Vec<_>>>()?;
        let after = window
            .after
            .into_iter()
            .map(|entry| self.render_hourei_law_entry(&entry, options))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(TargetWindow {
            center,
            before,
            after,
            diagnostics: Vec::new(),
        }))
    }

    fn render_hourei_law_entry(
        &self,
        entry: &crate::hourei::HoureiLawEntry,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let target = TargetToken::new(&InternalTarget::HoureiLaw {
            hore_id: entry.hore_id.clone(),
            anchor: None,
        })?;
        let mut view = self.render_target(&target, options)?;
        view.title = Some(hourei_law_node_label(entry));
        Ok(view)
    }

    fn ssed_index_row_label(&self, row: &SsedIndexRow) -> RichLabel {
        let label = self.ssed_display_text_for_index_row(row);
        self.ssed_rich_label(&label)
    }

    fn ssed_display_text_for_index_row(&self, row: &SsedIndexRow) -> String {
        let title = self.ssed_title_text(row.title);
        match title {
            Some(title) if !looks_like_raw_anchor_label(&title) => title,
            _ => row.key.clone(),
        }
    }

    fn ssed_component_for_index_pointer(&self, pointer: SsedIndexPointer) -> Option<&str> {
        self.ssed_catalog
            .as_ref()
            .and_then(|catalog| catalog.component_for_address(pointer.block))
            .map(|component| component.filename.as_str())
    }

    fn renderer_input_from_visual_body(
        &self,
        target: TargetToken,
        body: VisualBody,
    ) -> Result<RendererInput> {
        match body {
            VisualBody::PreservedHtml { html, source } => Ok(RendererInput::PreservedHtml {
                target,
                html,
                source,
            }),
            VisualBody::SsedStream {
                component,
                offset,
                length,
            } => Ok(RendererInput::HcSsedStream {
                target,
                component,
                offset,
                length,
                profile_hint: self.hc_profile_hint()?,
                diagnostics: vec![Diagnostic::info(
                    "hc_renderer_input_ready",
                    "SSED stream was resolved as input for an HC/profile renderer",
                )],
            }),
            VisualBody::SemanticFallback { text } => {
                Ok(RendererInput::SemanticFallback { target, text })
            }
            VisualBody::Unsupported {
                reason,
                diagnostics,
            } => Ok(RendererInput::Unsupported {
                target,
                reason,
                diagnostics,
            }),
        }
    }

    fn view_for_navigation_surface_target(
        &self,
        target: TargetToken,
        surface_id: &str,
        title: Option<String>,
    ) -> Result<ResolvedTargetView> {
        let scroll_anchor = scroll_anchor_for_token(&target)?;
        let surface = self.open_surface(surface_id)?;
        let kind = match &surface {
            NavigationSurface::Panel { .. } => ResolvedTargetKind::PanelSurface,
            NavigationSurface::InfoPages { .. } => ResolvedTargetKind::InfoPage,
            NavigationSurface::Deferred { .. } => ResolvedTargetKind::Deferred,
            _ => ResolvedTargetKind::NavigationSurface,
        };
        let capabilities = if matches!(kind, ResolvedTargetKind::PanelSurface) {
            vec![crate::render::RenderCapability::Panels]
        } else {
            Vec::new()
        };
        let mut diagnostics = Vec::new();
        if let NavigationSurface::Deferred {
            diagnostics: surface_diagnostics,
            ..
        } = &surface
        {
            diagnostics.extend(surface_diagnostics.clone());
        }
        Ok(ResolvedTargetView {
            kind,
            target,
            title: title.or_else(|| Some(surface_id.to_owned())),
            display_html: None,
            basic_text: None,
            scroll_anchor,
            surface: Some(surface),
            resources: Vec::new(),
            links: Vec::new(),
            capabilities,
            diagnostics,
            debug_trace: None,
        })
    }

    fn view_for_renderer_input(
        &self,
        input: RendererInput,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        match input {
            RendererInput::PreservedHtml {
                target,
                html,
                source,
            } => {
                let scroll_anchor = scroll_anchor_for_token(&target)?;
                let view_kind = self.resolved_kind_for_body_target(&target)?;
                let title = self.title_for_body_target(&target)?;
                if options.mode == RenderMode::BasicText {
                    return Ok(ResolvedTargetView {
                        kind: view_kind,
                        target,
                        title: Some(title.unwrap_or_else(|| "Entry".to_owned())),
                        display_html: None,
                        basic_text: Some(html_basic_text(&html)),
                        scroll_anchor,
                        surface: None,
                        resources: Vec::new(),
                        links: Vec::new(),
                        capabilities: Vec::new(),
                        diagnostics: Vec::new(),
                        debug_trace: None,
                    });
                }
                let normalized = match source {
                    BodySourceKind::LvedSqlite => self.normalize_lved_html_refs(&html)?,
                    BodySourceKind::LvlMultiViewSqlite => {
                        self.normalize_multiview_html_refs(&html)?
                    }
                    BodySourceKind::HoureiSqlite => self.normalize_hourei_html_refs(&html)?,
                    _ => NormalizedHtmlRefs {
                        html,
                        resources: Vec::new(),
                        links: Vec::new(),
                        diagnostics: Vec::new(),
                    },
                };
                Ok(ResolvedTargetView {
                    kind: view_kind,
                    target,
                    title: Some(title.unwrap_or_else(|| "Entry".to_owned())),
                    display_html: Some(normalized.html),
                    basic_text: None,
                    scroll_anchor,
                    surface: None,
                    resources: normalized.resources,
                    links: normalized.links,
                    capabilities: vec![crate::render::RenderCapability::Html],
                    diagnostics: normalized.diagnostics,
                    debug_trace: None,
                })
            }
            RendererInput::HcSsedStream {
                target,
                component,
                offset,
                length,
                profile_hint,
                mut diagnostics,
            } => {
                let scroll_anchor = scroll_anchor_for_token(&target)?;
                Ok(ResolvedTargetView {
                    kind: crate::render::ResolvedTargetKind::Deferred,
                    target,
                    title: Some("SSED entry stream".to_owned()),
                    display_html: None,
                    basic_text: None,
                    scroll_anchor,
                    surface: None,
                    resources: Vec::new(),
                    links: Vec::new(),
                    capabilities: vec![crate::render::RenderCapability::HcRenderInput],
                    diagnostics: {
                        diagnostics.push(Diagnostic::info(
                            "hc_render_deferred",
                            "SSED stream resolved successfully; HC/profile rendering is not implemented yet",
                        ));
                        diagnostics
                    },
                    debug_trace: options.include_debug_trace.then(|| {
                        json!({
                            "body": {
                                "kind": "ssed_stream",
                                "component": component,
                                "offset": offset,
                                "length": length,
                                "profile_hint": profile_hint,
                            }
                        })
                        .to_string()
                    }),
                })
            }
            RendererInput::SemanticFallback { target, text } => {
                let scroll_anchor = scroll_anchor_for_token(&target)?;
                Ok(ResolvedTargetView {
                    kind: crate::render::ResolvedTargetKind::EntryBody,
                    target,
                    title: Some("Semantic fallback".to_owned()),
                    display_html: None,
                    basic_text: Some(text),
                    scroll_anchor,
                    surface: None,
                    resources: Vec::new(),
                    links: Vec::new(),
                    capabilities: Vec::new(),
                    diagnostics: vec![Diagnostic::info(
                        "semantic_fallback",
                        "visual renderer is unavailable; semantic fallback was returned",
                    )],
                    debug_trace: None,
                })
            }
            RendererInput::Unsupported {
                target,
                reason,
                diagnostics,
            } => {
                let scroll_anchor = scroll_anchor_for_token(&target)?;
                Ok(ResolvedTargetView {
                    kind: crate::render::ResolvedTargetKind::Unsupported,
                    target,
                    title: Some(reason),
                    display_html: None,
                    basic_text: None,
                    scroll_anchor,
                    surface: None,
                    resources: Vec::new(),
                    links: Vec::new(),
                    capabilities: Vec::new(),
                    diagnostics,
                    debug_trace: None,
                })
            }
        }
    }

    fn render_package_html_resource(
        &self,
        target: TargetToken,
        resource: &ResourceToken,
        path: &str,
        resource_ref: ResourceRef,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let scroll_anchor = scroll_anchor_for_token(&target)?;
        let data = self.read_resource(resource)?;
        let html = decode_package_html_text(&data);
        let title = resource_ref.label.clone();
        if options.mode == RenderMode::BasicText {
            return Ok(ResolvedTargetView {
                kind: resolved_kind_for_package_html_path(path),
                target,
                title,
                display_html: None,
                basic_text: Some(html_basic_text(&html)),
                scroll_anchor,
                surface: None,
                resources: Vec::new(),
                links: Vec::new(),
                capabilities: Vec::new(),
                diagnostics: resource_ref.diagnostics,
                debug_trace: None,
            });
        }

        let mut normalized = self.normalize_package_file_html_refs(&html, path)?;
        let resources = normalized.resources;
        let mut diagnostics = resource_ref.diagnostics;
        diagnostics.append(&mut normalized.diagnostics);
        Ok(ResolvedTargetView {
            kind: resolved_kind_for_package_html_path(path),
            target,
            title,
            display_html: Some(normalized.html),
            basic_text: None,
            scroll_anchor,
            surface: None,
            resources,
            links: normalized.links,
            capabilities: vec![crate::render::RenderCapability::Html],
            diagnostics,
            debug_trace: None,
        })
    }

    fn render_chm_html_resource(
        &self,
        target: TargetToken,
        resource: &ResourceToken,
        chm_path: &str,
        entry_path: &str,
        resource_ref: ResourceRef,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let scroll_anchor = scroll_anchor_for_token(&target)?;
        let data = self.read_resource(resource)?;
        let html = decode_package_html_text(&data);
        let title = resource_ref.label.clone();
        let kind = resolved_kind_for_package_html_path(&format!("{chm_path}/{entry_path}"));
        if options.mode == RenderMode::BasicText {
            return Ok(ResolvedTargetView {
                kind,
                target,
                title,
                display_html: None,
                basic_text: Some(html_basic_text(&html)),
                scroll_anchor,
                surface: None,
                resources: Vec::new(),
                links: Vec::new(),
                capabilities: Vec::new(),
                diagnostics: resource_ref.diagnostics,
                debug_trace: None,
            });
        }

        let mut normalized = self.normalize_chm_html_refs(&html, chm_path, entry_path)?;
        let resources = normalized.resources;
        let mut diagnostics = resource_ref.diagnostics;
        diagnostics.append(&mut normalized.diagnostics);
        Ok(ResolvedTargetView {
            kind,
            target,
            title,
            display_html: Some(normalized.html),
            basic_text: None,
            scroll_anchor,
            surface: None,
            resources,
            links: normalized.links,
            capabilities: vec![crate::render::RenderCapability::Html],
            diagnostics,
            debug_trace: None,
        })
    }

    fn resolved_kind_for_body_target(&self, target: &TargetToken) -> Result<ResolvedTargetKind> {
        match target.decode()? {
            InternalTarget::LvedRow { table, .. } if table.eq_ignore_ascii_case("info") => {
                Ok(ResolvedTargetKind::InfoPage)
            }
            InternalTarget::LvedInfoPage { .. } => Ok(ResolvedTargetKind::InfoPage),
            InternalTarget::HoureiLaw { .. } => Ok(ResolvedTargetKind::LawArticle),
            _ => Ok(ResolvedTargetKind::EntryBody),
        }
    }

    fn title_for_body_target(&self, target: &TargetToken) -> Result<Option<String>> {
        match target.decode()? {
            InternalTarget::HoureiLaw { hore_id, .. } => {
                let Some(store) = &self.hourei_store else {
                    return Ok(None);
                };
                Ok(store
                    .law_entry(&hore_id)?
                    .map(|entry| hourei_law_node_label(&entry)))
            }
            _ => Ok(None),
        }
    }

    fn visual_body_for_ssed_address(
        &self,
        requested_component: &str,
        block: u32,
        offset: u32,
    ) -> Result<VisualBody> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(VisualBody::Unsupported {
                reason: "SSED catalog is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "ssed_catalog_missing",
                    "SSED address targets require a parsed SSEDINFO catalog",
                )],
            });
        };
        let component = catalog
            .component_named(requested_component)
            .or_else(|| catalog.component_for_address(block));
        let Some(component) = component else {
            return Ok(VisualBody::Unsupported {
                reason: "SSED address does not resolve to a catalog component".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "ssed_address_outside_components",
                    format!("no component contains logical block {block}"),
                )],
            });
        };
        let Some(component_offset) = component.relative_offset(block, offset) else {
            return Ok(VisualBody::Unsupported {
                reason: "SSED address is outside the resolved component".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "ssed_address_invalid_for_component",
                    format!(
                        "{} does not contain logical block {block} offset {offset}",
                        component.filename
                    ),
                )],
            });
        };
        if let Err(diagnostic) = self.validate_plain_component(component) {
            return Ok(VisualBody::Unsupported {
                reason: "SSED component is not readable as plain SSEDDATA".to_owned(),
                diagnostics: vec![diagnostic],
            });
        }
        if component.role == SsedComponentRole::Honmon
            && let Some(anchor_id) = self.ssed_dense_anchor_at_component_offset(
                component,
                usize::try_from(component_offset).unwrap_or(usize::MAX),
            )?
        {
            return self.visual_body_for_ssed_dense_anchor(&anchor_id, None);
        }
        Ok(VisualBody::SsedStream {
            component: component.filename.clone(),
            offset: component_offset,
            length: None,
        })
    }

    fn visual_body_for_ssed_dense_anchor(
        &self,
        anchor_id: &str,
        resolver_hint: Option<&str>,
    ) -> Result<VisualBody> {
        match lookup_ssed_dense_sidecar_body_with_resolvers(
            self.ssed_sidecar_body_resolvers()?,
            anchor_id,
            resolver_hint,
        )? {
            SsedSidecarLookup::Resolved(body) => {
                if let Some(html) = body.html {
                    Ok(VisualBody::PreservedHtml {
                        html,
                        source: match body.resolver.kind {
                            SsedSidecarKind::TContents => BodySourceKind::RendererDatabase,
                            _ => BodySourceKind::SidecarHtml,
                        },
                    })
                } else {
                    Ok(VisualBody::SemanticFallback { text: body.text })
                }
            }
            SsedSidecarLookup::MissingRow { diagnostics, .. } => Ok(VisualBody::Unsupported {
                reason: "dense HONMON sidecar row is missing".to_owned(),
                diagnostics,
            }),
            SsedSidecarLookup::NoResolver { diagnostics } => Ok(VisualBody::Unsupported {
                reason: "dense HONMON sidecar resolver is unavailable".to_owned(),
                diagnostics,
            }),
        }
    }

    fn ssed_sidecar_body_resolvers(&self) -> Result<&[SsedSidecarBodyResolver]> {
        let resolvers = self.ssed_sidecar_body_resolvers.get_or_init(|| {
            discover_ssed_sidecar_body_resolvers(
                &self.root,
                inferred_folder_title(&self.root).as_deref(),
            )
            .map_err(|error| error.to_string())
        });
        match resolvers {
            Ok(resolvers) => Ok(resolvers.as_slice()),
            Err(error) => Err(Error::Driver(error.clone())),
        }
    }

    fn ssed_dense_anchor_at_component_offset(
        &self,
        component: &SsedComponent,
        offset: usize,
    ) -> Result<Option<String>> {
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Ok(None);
        };
        let mut reader = SsedDataFile::open(&path)?;
        let mut data = reader.read_range(offset, 256)?;
        if let Some(end) = find_ssed_dense_anchor_record_end(&data) {
            data.truncate(end);
        }
        let decoded = decode_ssed_body_search_text(&data);
        let compact = decoded
            .chars()
            .filter(|ch| !ch.is_whitespace() && *ch != '\0')
            .collect::<String>();
        if compact.len() >= 4
            && compact.len() <= 16
            && compact.chars().all(|ch| ch.is_ascii_digit())
        {
            Ok(Some(compact))
        } else {
            Ok(None)
        }
    }

    fn visual_body_for_lved_row(&self, table: &str, row_id: i64) -> Result<VisualBody> {
        if table.eq_ignore_ascii_case("info") {
            return self.visual_body_for_lved_info_row(row_id);
        }
        if !table.eq_ignore_ascii_case("content") {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 target table is not renderable yet".to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "lved_row_table_deferred",
                    format!("LVED_SQLITE3 table {table} is not a renderable content table"),
                )],
            });
        }
        let Some(store) = &self.lved_store else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 content targets require an opened SQLCipher store",
                )],
            });
        };
        let Some(html) = store.content_html(row_id)? else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 content row was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "lved_content_missing",
                    format!("LVED_SQLITE3 content row {row_id} was not found"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html,
            source: BodySourceKind::LvedSqlite,
        })
    }

    fn visual_body_for_lved_info_row(&self, row_id: i64) -> Result<VisualBody> {
        let Some(store) = &self.lved_store else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 info targets require an opened SQLCipher store",
                )],
            });
        };
        let Some(html) = store.info_html(row_id)? else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 info row was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "lved_info_missing",
                    format!("LVED_SQLITE3 info row {row_id} was not found"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html,
            source: BodySourceKind::LvedSqlite,
        })
    }

    fn visual_body_for_lved_info_name(&self, name: &str) -> Result<VisualBody> {
        let Some(store) = &self.lved_store else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 info targets require an opened SQLCipher store",
                )],
            });
        };
        let Some(html) = store.info_html_by_name(name)? else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 info page was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "lved_info_missing",
                    format!("LVED_SQLITE3 info page {name} was not found"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html,
            source: BodySourceKind::LvedSqlite,
        })
    }

    fn visual_body_for_multiview_href(
        &self,
        href: &str,
        anchor: Option<&str>,
    ) -> Result<VisualBody> {
        let Some(store) = &self.multiview_store else {
            return Ok(VisualBody::Unsupported {
                reason: "LVLMultiView store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "multiview_store_missing",
                    "LVLMultiView targets require opened LogoFontCipher SQLite payloads",
                )],
            });
        };
        let lookup = anchor.unwrap_or(href);
        let Some(body) = store.body_for_href(lookup)? else {
            return Ok(VisualBody::Unsupported {
                reason: "LVLMultiView target was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "multiview_target_missing",
                    format!("LVLMultiView target {lookup} was not found in decoded payloads"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html: body.html,
            source: BodySourceKind::LvlMultiViewSqlite,
        })
    }

    fn visual_body_for_hourei_law(&self, hore_id: &str) -> Result<VisualBody> {
        let Some(store) = &self.hourei_store else {
            return Ok(VisualBody::Unsupported {
                reason: "Hourei store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "hourei_store_missing",
                    "Hourei law targets require an opened Hourei store",
                )],
            });
        };
        let Some(html) = store.law_html(hore_id)? else {
            return Ok(VisualBody::Unsupported {
                reason: "Hourei law body was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "hourei_law_missing",
                    format!("Hourei law {hore_id} was not found in cached HTML or law shard DB"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html,
            source: BodySourceKind::HoureiSqlite,
        })
    }

    fn normalize_lved_html_refs(&self, html: &str) -> Result<NormalizedHtmlRefs> {
        let mut output = String::with_capacity(html.len());
        let mut resources = Vec::new();
        let mut links = Vec::new();
        let mut diagnostics = Vec::new();
        let mut seen_resource_tokens = BTreeSet::new();
        let mut seen_target_tokens = BTreeSet::new();
        let mut cursor = 0usize;
        while let Some((relative_start, ref_kind)) = next_lved_ref(&html[cursor..]) {
            let start = cursor + relative_start;
            output.push_str(&html[cursor..start]);
            let end = html[start..]
                .find(is_lved_ref_terminator)
                .map(|index| start + index)
                .unwrap_or(html.len());
            let raw_ref = &html[start..end];
            match ref_kind {
                LvedHtmlRefKind::Media => {
                    if let Some(resource) = lved_media_resource(raw_ref) {
                        let token = ResourceToken::new(&resource)?;
                        let href = format!("lvcore://resource/{}", token.as_str());
                        if seen_resource_tokens.insert(token.as_str().to_owned()) {
                            let resource_ref = self.resolve_resource(&token)?;
                            diagnostics.extend(resource_ref.diagnostics.clone());
                            resources.push(resource_ref);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "lved_media_ref_unparsed",
                            format!("could not parse LVED media reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::DataId => {
                    if let Some(target) = lved_dataid_target(raw_ref) {
                        let token = TargetToken::new(&target)?;
                        let href = format!("lvcore://target/{}", token.as_str());
                        if seen_target_tokens.insert(token.as_str().to_owned()) {
                            links.push(TargetLink::new(raw_ref, &target)?);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "lved_dataid_ref_unparsed",
                            format!("could not parse LVED dataid reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::Info => {
                    if let Some(target) = lved_info_target(raw_ref) {
                        let token = TargetToken::new(&target)?;
                        let href = format!("lvcore://target/{}", token.as_str());
                        if seen_target_tokens.insert(token.as_str().to_owned()) {
                            links.push(TargetLink::new(raw_ref, &target)?);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "lved_info_ref_unparsed",
                            format!("could not parse LVED info reference {raw_ref}"),
                        ));
                    }
                }
            }
            cursor = end;
        }
        output.push_str(&html[cursor..]);
        let html = self.normalize_lved_direct_resource_attrs(
            &output,
            &mut resources,
            &mut diagnostics,
            &mut seen_resource_tokens,
        )?;
        Ok(NormalizedHtmlRefs {
            html,
            resources,
            links,
            diagnostics,
        })
    }

    fn normalize_lved_label_html(&self, html: &str) -> Result<String> {
        Ok(self.normalize_lved_html_refs(html)?.html)
    }

    fn normalize_lved_direct_resource_attrs(
        &self,
        html: &str,
        resources: &mut Vec<ResourceRef>,
        diagnostics: &mut Vec<Diagnostic>,
        seen_resource_tokens: &mut BTreeSet<String>,
    ) -> Result<String> {
        let mut output = String::with_capacity(html.len());
        let mut cursor = 0usize;
        while let Some(attr) = next_html_href_or_src_attr(html, cursor) {
            output.push_str(&html[cursor..attr.value_start]);
            let raw_value = &html[attr.value_start..attr.value_end];
            if matches!(attr.name, HtmlAttrName::Src | HtmlAttrName::Data)
                && !raw_value.starts_with("lvcore://")
                && let Some(resource) = self.lved_direct_resource(raw_value)?
            {
                let token = ResourceToken::new(&resource)?;
                let href = format!("lvcore://resource/{}", token.as_str());
                if seen_resource_tokens.insert(token.as_str().to_owned()) {
                    let resource_ref = self.resolve_resource(&token)?;
                    diagnostics.extend(resource_ref.diagnostics.clone());
                    resources.push(resource_ref);
                }
                output.push_str(&href);
            } else {
                output.push_str(raw_value);
            }
            cursor = attr.value_end;
        }
        output.push_str(&html[cursor..]);
        Ok(output)
    }

    fn lved_direct_resource(&self, raw_value: &str) -> Result<Option<InternalResource>> {
        let value = html_unescape_minimal(raw_value).trim().replace('\\', "/");
        if value.is_empty()
            || value.starts_with('#')
            || value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("data:")
            || value.starts_with("javascript:")
            || value.starts_with("lvcore://")
            || value.starts_with("lved.")
        {
            return Ok(None);
        }
        let relative = value.split(['#', '?']).next().unwrap_or("").trim();
        if relative.is_empty() {
            return Ok(None);
        }
        let candidates = [relative.to_owned(), format!("res/{relative}")];
        for candidate in candidates {
            if self.storage.exists(Path::new(&candidate))? {
                return Ok(Some(InternalResource::PackageFile {
                    resource_kind: resource_kind_from_path(&candidate),
                    path: candidate,
                }));
            }
        }
        Ok(Some(InternalResource::MediaBlob {
            store: "lved.media".to_owned(),
            key: relative.to_owned(),
            resource_kind: resource_kind_from_path(relative),
        }))
    }

    fn normalize_multiview_html_refs(&self, html: &str) -> Result<NormalizedHtmlRefs> {
        let mut output = String::with_capacity(html.len());
        let mut resources = Vec::new();
        let mut links = Vec::new();
        let mut diagnostics = Vec::new();
        let mut seen_resource_tokens = BTreeSet::new();
        let mut seen_target_tokens = BTreeSet::new();
        let mut cursor = 0usize;

        while let Some(attr) = next_html_href_or_src_attr(html, cursor) {
            output.push_str(&html[cursor..attr.value_start]);
            let raw_value = &html[attr.value_start..attr.value_end];
            if attr.name == HtmlAttrName::Href {
                if let Some(replacement) =
                    self.rewrite_multiview_href(raw_value, &mut links, &mut seen_target_tokens)?
                {
                    output.push_str(&replacement);
                } else {
                    output.push_str(raw_value);
                }
            } else if let Some(resource) = self.multiview_package_resource(raw_value)? {
                let token = ResourceToken::new(&resource)?;
                let href = format!("lvcore://resource/{}", token.as_str());
                if seen_resource_tokens.insert(token.as_str().to_owned()) {
                    let resource_ref = self.resolve_resource(&token)?;
                    diagnostics.extend(resource_ref.diagnostics.clone());
                    resources.push(resource_ref);
                }
                output.push_str(&href);
            } else {
                output.push_str(raw_value);
            }
            cursor = attr.value_end;
        }

        output.push_str(&html[cursor..]);
        Ok(NormalizedHtmlRefs {
            html: output,
            resources,
            links,
            diagnostics,
        })
    }

    fn normalize_hourei_html_refs(&self, html: &str) -> Result<NormalizedHtmlRefs> {
        let mut output = String::with_capacity(html.len());
        let mut resources = Vec::new();
        let mut links = Vec::new();
        let mut diagnostics = Vec::new();
        let mut seen_resource_tokens = BTreeSet::new();
        let mut seen_target_tokens = BTreeSet::new();
        let mut cursor = 0usize;

        while let Some(attr) = next_html_href_or_src_attr(html, cursor) {
            output.push_str(&html[cursor..attr.value_start]);
            let raw_value = &html[attr.value_start..attr.value_end];
            if attr.name == HtmlAttrName::Href {
                if let Some(replacement) =
                    self.rewrite_hourei_href(raw_value, &mut links, &mut seen_target_tokens)?
                {
                    output.push_str(&replacement);
                } else {
                    output.push_str(raw_value);
                }
            } else if let Some(resource) = self.hourei_package_resource(raw_value)? {
                let token = ResourceToken::new(&resource)?;
                let href = format!("lvcore://resource/{}", token.as_str());
                if seen_resource_tokens.insert(token.as_str().to_owned()) {
                    let resource_ref = self.resolve_resource(&token)?;
                    diagnostics.extend(resource_ref.diagnostics.clone());
                    resources.push(resource_ref);
                }
                output.push_str(&href);
            } else {
                output.push_str(raw_value);
            }
            cursor = attr.value_end;
        }

        output.push_str(&html[cursor..]);
        Ok(NormalizedHtmlRefs {
            html: output,
            resources,
            links,
            diagnostics,
        })
    }

    fn normalize_package_file_html_refs(
        &self,
        html: &str,
        path: &str,
    ) -> Result<NormalizedHtmlRefs> {
        let base_dir = package_html_base_dir(path);
        let mut output = String::with_capacity(html.len());
        let mut resources = Vec::new();
        let mut links = Vec::new();
        let mut diagnostics = Vec::new();
        let mut seen_resource_tokens = BTreeSet::new();
        let mut seen_target_tokens = BTreeSet::new();
        let mut cursor = 0usize;

        while let Some(attr) = next_html_href_or_src_attr(html, cursor) {
            output.push_str(&html[cursor..attr.value_start]);
            let raw_value = &html[attr.value_start..attr.value_end];
            if let Some(reference) = package_relative_html_reference(&base_dir, raw_value) {
                if attr.name == HtmlAttrName::Href
                    && path_has_extension(&reference.path, &["html", "htm"])
                {
                    let resource = InternalResource::PackageFile {
                        path: reference.path.clone(),
                        resource_kind: ResourceKind::Html,
                    };
                    let resource = ResourceToken::new(&resource)?;
                    let target = InternalTarget::Resource {
                        resource,
                        anchor: reference.anchor,
                    };
                    let token = TargetToken::new(&target)?;
                    if seen_target_tokens.insert(token.as_str().to_owned()) {
                        links.push(TargetLink::new(raw_value, &target)?);
                    }
                    output.push_str(&format!("lvcore://target/{}", token.as_str()));
                } else {
                    let resource = InternalResource::PackageFile {
                        resource_kind: resource_kind_from_path(&reference.path),
                        path: reference.path,
                    };
                    let token = ResourceToken::new(&resource)?;
                    let href = format!("lvcore://resource/{}", token.as_str());
                    if seen_resource_tokens.insert(token.as_str().to_owned()) {
                        let resource_ref = self.resolve_resource(&token)?;
                        diagnostics.extend(resource_ref.diagnostics.clone());
                        resources.push(resource_ref);
                    }
                    output.push_str(&href);
                    if let Some(anchor) = reference.anchor {
                        output.push('#');
                        output.push_str(&anchor);
                    }
                }
            } else {
                output.push_str(raw_value);
            }
            cursor = attr.value_end;
        }
        output.push_str(&html[cursor..]);

        Ok(NormalizedHtmlRefs {
            html: output,
            resources,
            links,
            diagnostics,
        })
    }

    fn normalize_chm_html_refs(
        &self,
        html: &str,
        chm_path: &str,
        entry_path: &str,
    ) -> Result<NormalizedHtmlRefs> {
        let base_dir = package_html_base_dir(entry_path);
        let mut output = String::with_capacity(html.len());
        let mut resources = Vec::new();
        let mut links = Vec::new();
        let mut diagnostics = Vec::new();
        let mut seen_resource_tokens = BTreeSet::new();
        let mut seen_target_tokens = BTreeSet::new();
        let mut cursor = 0usize;

        while let Some(attr) = next_html_href_or_src_attr(html, cursor) {
            output.push_str(&html[cursor..attr.value_start]);
            let raw_value = &html[attr.value_start..attr.value_end];
            if let Some(reference) = package_relative_html_reference(&base_dir, raw_value) {
                if attr.name == HtmlAttrName::Href
                    && path_has_extension(&reference.path, &["html", "htm"])
                {
                    let resource = InternalResource::ChmFile {
                        chm_path: chm_path.to_owned(),
                        entry_path: reference.path,
                        resource_kind: ResourceKind::Html,
                    };
                    let resource = ResourceToken::new(&resource)?;
                    let target = InternalTarget::Resource {
                        resource,
                        anchor: reference.anchor,
                    };
                    let token = TargetToken::new(&target)?;
                    if seen_target_tokens.insert(token.as_str().to_owned()) {
                        links.push(TargetLink::new(raw_value, &target)?);
                    }
                    output.push_str(&format!("lvcore://target/{}", token.as_str()));
                } else {
                    let resource = InternalResource::ChmFile {
                        resource_kind: resource_kind_from_path(&reference.path),
                        chm_path: chm_path.to_owned(),
                        entry_path: reference.path,
                    };
                    let token = ResourceToken::new(&resource)?;
                    let href = format!("lvcore://resource/{}", token.as_str());
                    if seen_resource_tokens.insert(token.as_str().to_owned()) {
                        let resource_ref = self.resolve_resource(&token)?;
                        diagnostics.extend(resource_ref.diagnostics.clone());
                        resources.push(resource_ref);
                    }
                    output.push_str(&href);
                    if let Some(anchor) = reference.anchor {
                        output.push('#');
                        output.push_str(&anchor);
                    }
                }
            } else {
                output.push_str(raw_value);
            }
            cursor = attr.value_end;
        }
        output.push_str(&html[cursor..]);

        Ok(NormalizedHtmlRefs {
            html: output,
            resources,
            links,
            diagnostics,
        })
    }

    fn rewrite_hourei_href(
        &self,
        raw_value: &str,
        links: &mut Vec<TargetLink>,
        seen_target_tokens: &mut BTreeSet<String>,
    ) -> Result<Option<String>> {
        let value = html_unescape_minimal(raw_value).trim().to_owned();
        if value.is_empty()
            || value.starts_with('#')
            || value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("mailto:")
            || value.starts_with("javascript:")
        {
            return Ok(None);
        }
        if let Some(anchor) = value.strip_prefix("lved_mark&&") {
            return Ok(Some(format!("#{anchor}")));
        }
        if let Some(anchor) = value.strip_prefix("lved_ref&&") {
            return Ok(Some(format!("#{anchor}")));
        }
        if let Some(query) = value.strip_prefix("lved_ref:") {
            let target = InternalTarget::Unsupported {
                reason: format!("Hourei kana-search link is not modeled yet: {query}"),
            };
            let token = TargetToken::new(&target)?;
            if seen_target_tokens.insert(token.as_str().to_owned()) {
                links.push(TargetLink::new(raw_value, &target)?);
            }
            return Ok(Some(format!("lvcore://target/{}", token.as_str())));
        }
        if value.eq_ignore_ascii_case("lved_unsafe") {
            return Ok(Some("#".to_owned()));
        }
        if let Some(rest) = value.strip_prefix("lved_ref&")
            && let Some((mode, body)) = rest.split_once(':')
        {
            if mode == "1" {
                let (hore_id, anchor) = body.split_once('&').unwrap_or((body, ""));
                if hore_id.chars().all(|ch| ch.is_ascii_digit()) {
                    let target = InternalTarget::HoureiLaw {
                        hore_id: hore_id.to_owned(),
                        anchor: (!anchor.is_empty()).then(|| anchor.to_owned()),
                    };
                    let token = TargetToken::new(&target)?;
                    if seen_target_tokens.insert(token.as_str().to_owned()) {
                        links.push(TargetLink::new(raw_value, &target)?);
                    }
                    return Ok(Some(format!("lvcore://target/{}", token.as_str())));
                }
            }
            if mode == "4" {
                let (primary, _) = body.split_once(':').unwrap_or((body, ""));
                if primary.chars().all(|ch| ch.is_ascii_digit()) {
                    let target = InternalTarget::HoureiLaw {
                        hore_id: primary.to_owned(),
                        anchor: None,
                    };
                    let token = TargetToken::new(&target)?;
                    if seen_target_tokens.insert(token.as_str().to_owned()) {
                        let mut link = TargetLink::new(raw_value, &target)?;
                        link.diagnostics.push(Diagnostic::info(
                                "hourei_revision_ref_partial",
                                "Hourei future/revision reference was routed to the primary law; related revision semantics are deferred",
                            ));
                        links.push(link);
                    }
                    return Ok(Some(format!("lvcore://target/{}", token.as_str())));
                }
            }
        }
        Ok(None)
    }

    fn hourei_package_resource(&self, raw_value: &str) -> Result<Option<InternalResource>> {
        let Some(store) = &self.hourei_store else {
            return Ok(None);
        };
        let Some(path) = store.resource_path_by_reference(raw_value)? else {
            return Ok(None);
        };
        let path = path.to_string_lossy().replace('\\', "/");
        Ok(Some(InternalResource::PackageFile {
            resource_kind: resource_kind_from_path(&path),
            path,
        }))
    }

    fn rewrite_multiview_href(
        &self,
        raw_value: &str,
        links: &mut Vec<TargetLink>,
        seen_target_tokens: &mut BTreeSet<String>,
    ) -> Result<Option<String>> {
        let value = html_unescape_minimal(raw_value).trim().to_owned();
        if value.is_empty()
            || value.starts_with('#')
            || value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("mailto:")
            || value.starts_with("javascript:")
        {
            return Ok(None);
        }
        if let Some(anchor) = value
            .strip_prefix("lved_mark:")
            .and_then(|rest| rest.split_once(':').map(|(_, anchor)| anchor))
        {
            return Ok(Some(format!("#{anchor}")));
        }
        let target_href = value
            .strip_prefix("lved_ref:")
            .and_then(|rest| rest.split_once(':').map(|(_, target)| target))
            .unwrap_or(&value);
        let target = InternalTarget::MultiviewHref {
            href: target_href.to_owned(),
            anchor: None,
        };
        let token = TargetToken::new(&target)?;
        if seen_target_tokens.insert(token.as_str().to_owned()) {
            links.push(TargetLink::new(raw_value, &target)?);
        }
        Ok(Some(format!("lvcore://target/{}", token.as_str())))
    }

    fn multiview_package_resource(&self, raw_value: &str) -> Result<Option<InternalResource>> {
        let value = html_unescape_minimal(raw_value).trim().replace('\\', "/");
        if value.is_empty()
            || value.starts_with('#')
            || value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("data:")
        {
            return Ok(None);
        }
        let relative = value.split(['#', '?']).next().unwrap_or("").trim();
        if relative.is_empty() {
            return Ok(None);
        }
        let candidates = [
            relative.to_owned(),
            format!("Templates/{relative}"),
            format!("Help/image/{relative}"),
            format!("Help/{relative}"),
        ];
        for candidate in candidates {
            if self.storage.exists(Path::new(&candidate))? {
                return Ok(Some(InternalResource::PackageFile {
                    resource_kind: resource_kind_from_path(&candidate),
                    path: candidate,
                }));
            }
        }
        Ok(Some(InternalResource::PackageFile {
            resource_kind: resource_kind_from_path(relative),
            path: relative.to_owned(),
        }))
    }

    fn validate_plain_component(
        &self,
        component: &SsedComponent,
    ) -> std::result::Result<(), Diagnostic> {
        if !component.has_positive_range() {
            return Err(Diagnostic::warning(
                "ssed_component_optional_absent",
                format!("{} has no positive block range", component.filename),
            ));
        }
        let path = match self.resolve_readable_ssed_component_path(component) {
            Ok(Some(path)) => path,
            Ok(None) => {
                return Err(Diagnostic::warning(
                    "ssed_component_file_missing",
                    format!("{} is declared but not present on disk", component.filename),
                ));
            }
            Err(err) => {
                return Err(Diagnostic::warning(
                    "ssed_component_decode_deferred",
                    format!(
                        "{} is not readable as SSEDDATA yet: {err}",
                        component.filename
                    ),
                ));
            }
        };
        SsedDataHeader::parse_file(&path).map_err(|err| {
            Diagnostic::warning(
                "ssed_component_decode_deferred",
                format!(
                    "{} does not expose a readable plain SSEDDATA header yet: {err}",
                    component.filename
                ),
            )
        })?;
        Ok(())
    }

    fn resolve_readable_ssed_component_path(
        &self,
        component: &SsedComponent,
    ) -> Result<Option<PathBuf>> {
        let candidates = self.resolve_ssed_component_candidate_paths(component)?;
        if candidates.is_empty() {
            return Ok(None);
        }
        let mut unreadable = Vec::new();
        for path in candidates {
            match self.materialize_readable_ssed_component_path(component, &path)? {
                Some(readable) => return Ok(Some(readable)),
                None => unreadable.push(path),
            }
        }
        Err(Error::Driver(format!(
            "candidate file(s) were found but none decoded as plain, zipped, or encrypted SSEDDATA: {}",
            unreadable
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )))
    }

    fn resolve_ssed_component_candidate_paths(
        &self,
        component: &SsedComponent,
    ) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        let mut seen = BTreeSet::new();
        if let Some(path) = self
            .storage
            .resolve_casefolded(Path::new(&component.filename))?
        {
            seen.insert(path.clone());
            paths.push(path);
        }
        for alias in ssed_component_filename_aliases(component) {
            if let Some(path) = self.storage.resolve_casefolded(Path::new(&alias))?
                && seen.insert(path.clone())
            {
                paths.push(path);
            }
        }
        Ok(paths)
    }

    fn materialize_readable_ssed_component_path(
        &self,
        component: &SsedComponent,
        path: &Path,
    ) -> Result<Option<PathBuf>> {
        if SsedDataHeader::parse_file(path).is_ok() {
            return Ok(Some(path.to_path_buf()));
        }

        if looks_like_zip_file(path)?
            && let Some(extracted) = self.extract_zipped_ssed_component(component, path)?
        {
            return self.materialize_readable_ssed_component_path(component, &extracted);
        }

        if let Some(decrypted) = self.decrypt_ssed_component_if_needed(component, path)? {
            return Ok(Some(decrypted));
        }

        Ok(None)
    }

    fn extract_zipped_ssed_component(
        &self,
        component: &SsedComponent,
        zip_path: &Path,
    ) -> Result<Option<PathBuf>> {
        let member_name = match zip_member_name_for_component(component, zip_path)? {
            Some(member_name) => member_name,
            None => return Ok(None),
        };
        for password in self.mac_honmon_zip_passwords()? {
            let file = File::open(zip_path)?;
            let mut archive = ZipArchive::new(file).map_err(zip_error)?;
            let mut member = match password.as_deref() {
                Some(password) => match archive.by_name_decrypt(&member_name, password) {
                    Ok(member) => member,
                    Err(ZipError::InvalidPassword)
                    | Err(ZipError::UnsupportedArchive(ZipError::PASSWORD_REQUIRED)) => continue,
                    Err(err) => return Err(zip_error(err)),
                },
                None => match archive.by_name(&member_name) {
                    Ok(member) if member.encrypted() => continue,
                    Ok(member) => member,
                    Err(ZipError::UnsupportedArchive(ZipError::PASSWORD_REQUIRED)) => continue,
                    Err(err) => return Err(zip_error(err)),
                },
            };
            let cache_path = self.ssed_component_cache_path(
                component,
                zip_path,
                &format!(
                    "zip:{}:{}",
                    member_name,
                    password
                        .as_deref()
                        .map(hex::encode)
                        .unwrap_or_else(|| "none".to_owned())
                ),
                "bin",
            )?;
            if cache_path.exists() {
                return Ok(Some(cache_path));
            }
            let tmp_path = cache_path.with_extension("tmp");
            {
                let mut outfile = File::create(&tmp_path)?;
                if let Err(error) = std::io::copy(&mut member, &mut outfile) {
                    if password.is_none()
                        && matches!(
                            error.kind(),
                            std::io::ErrorKind::Unsupported
                                | std::io::ErrorKind::PermissionDenied
                                | std::io::ErrorKind::InvalidData
                        )
                    {
                        let _ = fs::remove_file(&tmp_path);
                        continue;
                    }
                    return Err(Error::Io(error));
                }
                outfile.flush()?;
            }
            fs::rename(&tmp_path, &cache_path)?;
            return Ok(Some(cache_path));
        }
        Ok(None)
    }

    fn decrypt_ssed_component_if_needed(
        &self,
        component: &SsedComponent,
        path: &Path,
    ) -> Result<Option<PathBuf>> {
        let mut file = File::open(path)?;
        let mut prefix = vec![0_u8; 4096];
        let read = file.read(&mut prefix)?;
        prefix.truncate(read);
        if prefix.len() < 16 {
            return Ok(None);
        }
        let attempts: [(&str, PrefixDecryptFn, FileDecryptFn); 2] = [
            (
                "macos_logofont_cipher",
                decrypt_macos_logofont_cipher_prefix,
                decrypt_macos_logofont_cipher_file_to_path,
            ),
            (
                "logofont_cipher",
                decrypt_logofont_cipher_prefix,
                decrypt_logofont_cipher_file_to_path,
            ),
        ];
        for (name, prefix_decrypt, file_decrypt) in attempts {
            let decrypted_prefix = prefix_decrypt(&prefix, prefix.len())?;
            if !decrypted_prefix.starts_with(SSEDDATA_MAGIC) {
                continue;
            }
            let cache_path = self.ssed_component_cache_path(component, path, name, "dic")?;
            if SsedDataHeader::parse_file(&cache_path).is_ok() {
                return Ok(Some(cache_path));
            }
            let tmp_path = cache_path.with_extension("tmp");
            file_decrypt(path, &tmp_path)?;
            SsedDataHeader::parse_file(&tmp_path)?;
            fs::rename(&tmp_path, &cache_path)?;
            return Ok(Some(cache_path));
        }
        Ok(None)
    }

    fn mac_honmon_zip_passwords(&self) -> Result<Vec<Option<Vec<u8>>>> {
        let mut passwords = vec![None];
        for path in self.storage.list_dir(Path::new(""))? {
            if !path.is_file() {
                continue;
            }
            let Some(stem) = path.file_stem().map(|value| value.to_string_lossy()) else {
                continue;
            };
            let Some(extension) = path.extension().map(|value| value.to_string_lossy()) else {
                continue;
            };
            if !extension.eq_ignore_ascii_case("idx") {
                continue;
            }
            let lower = stem.to_ascii_lowercase();
            if lower.len() == 8 && lower.chars().all(|ch| ch.is_ascii_hexdigit()) {
                passwords.push(Some(format!("casKet{lower}").into_bytes()));
            }
        }
        Ok(passwords)
    }

    fn ssed_component_cache_path(
        &self,
        component: &SsedComponent,
        source: &Path,
        stage: &str,
        extension: &str,
    ) -> Result<PathBuf> {
        let metadata = fs::metadata(source)?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let mut hasher = Sha256::new();
        hasher.update(self.metadata.root_fingerprint.as_bytes());
        hasher.update(b"\0");
        hasher.update(component.filename.as_bytes());
        hasher.update(b"\0");
        hasher.update(stage.as_bytes());
        hasher.update(b"\0");
        hasher.update(source.as_os_str().to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(metadata.len().to_le_bytes());
        hasher.update(modified.to_le_bytes());
        let hash = hex::encode(hasher.finalize());
        let dir = std::env::temp_dir()
            .join("lvcore-rs")
            .join("ssed-components");
        fs::create_dir_all(&dir)?;
        Ok(dir.join(format!("{hash}.{extension}")))
    }

    fn hc_profile_hint(&self) -> Result<Option<String>> {
        let mut hints = Vec::new();
        for path in self.storage.list_dir(Path::new(""))? {
            let Some(name) = path.file_name().map(|value| value.to_string_lossy()) else {
                continue;
            };
            let upper = name.to_ascii_uppercase();
            if upper.len() == "HC0000.DLL".len()
                && upper.starts_with("HC")
                && upper.ends_with(".DLL")
                && upper[2..6].chars().all(|ch| ch.is_ascii_hexdigit())
            {
                hints.push(upper.trim_end_matches(".DLL").to_owned());
            }
        }
        hints.sort();
        Ok(hints.into_iter().next())
    }

    fn discover_ssed_hanrei_pages(&self) -> Result<Vec<SsedHanreiPage>> {
        let mut pages = Vec::new();
        let mut seen = BTreeSet::new();

        for candidate in [
            "hanrei.html",
            "HANREI.html",
            "HANREI/index.html",
            "HANREI/index.htm",
            "HANREI/hanrei.html",
            "HANREI/hanrei.htm",
        ] {
            self.push_ssed_hanrei_page(candidate, &mut pages, &mut seen)?;
        }

        self.push_ssed_hanrei_folder_pages("HANREI", &mut pages, &mut seen, 0)?;
        self.push_ssed_hanrei_chm_pages("HANREI.chm", &mut pages, &mut seen)?;

        for path in self.storage.list_dir(Path::new(""))? {
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().map(|value| value.to_string_lossy()) else {
                continue;
            };
            if name.starts_with("._") || !name.to_ascii_lowercase().ends_with("_help.localized") {
                continue;
            }
            let root = name.replace('\\', "/");
            for candidate in [
                format!("{root}/index.html"),
                format!("{root}/index.htm"),
                format!("{root}/menu.html"),
                format!("{root}/top.html"),
                format!("{root}/contents/hanrei.html"),
                format!("{root}/contents/hanrei.htm"),
                format!("{root}/contents/copyright.html"),
                format!("{root}/contents/copyright.htm"),
            ] {
                self.push_ssed_hanrei_page(&candidate, &mut pages, &mut seen)?;
            }

            let contents_dir = format!("{root}/contents");
            for child in self.storage.list_dir(Path::new(&contents_dir))? {
                if !child.is_file() {
                    continue;
                }
                let Some(file_name) = child.file_name().map(|value| value.to_string_lossy()) else {
                    continue;
                };
                if file_name.starts_with("._") {
                    continue;
                }
                if !path_has_extension(&file_name, &["html", "htm"]) {
                    continue;
                }
                let candidate = format!("{contents_dir}/{file_name}");
                self.push_ssed_hanrei_page(&candidate, &mut pages, &mut seen)?;
            }
        }

        Ok(pages)
    }

    fn push_ssed_hanrei_folder_pages(
        &self,
        relative_dir: &str,
        pages: &mut Vec<SsedHanreiPage>,
        seen: &mut BTreeSet<String>,
        depth: usize,
    ) -> Result<()> {
        if depth > 8 || !self.storage.exists(Path::new(relative_dir))? {
            return Ok(());
        }
        for child in self.storage.list_dir(Path::new(relative_dir))? {
            let Some(file_name) = child.file_name().map(|value| value.to_string_lossy()) else {
                continue;
            };
            if file_name.starts_with("._") {
                continue;
            }
            let candidate = format!("{relative_dir}/{file_name}");
            if child.is_dir() {
                self.push_ssed_hanrei_folder_pages(&candidate, pages, seen, depth + 1)?;
            } else if child.is_file() && path_has_extension(&file_name, &["html", "htm"]) {
                self.push_ssed_hanrei_page(&candidate, pages, seen)?;
            }
        }
        Ok(())
    }

    fn push_ssed_hanrei_page(
        &self,
        candidate: &str,
        pages: &mut Vec<SsedHanreiPage>,
        seen: &mut BTreeSet<String>,
    ) -> Result<()> {
        let normalized = candidate.replace('\\', "/");
        if normalized
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        {
            return Ok(());
        }
        if !self.storage.exists(Path::new(&normalized))? {
            return Ok(());
        }
        if !seen.insert(normalized.to_ascii_lowercase()) {
            return Ok(());
        }
        let resource_kind = resource_kind_from_path(&normalized);
        pages.push(SsedHanreiPage {
            item_id: normalized.clone(),
            label: ssed_hanrei_page_label(&normalized),
            resource: InternalResource::PackageFile {
                path: normalized,
                resource_kind,
            },
            anchor: None,
            diagnostics: Vec::new(),
        });
        Ok(())
    }

    fn push_ssed_hanrei_chm_pages(
        &self,
        chm_path: &str,
        pages: &mut Vec<SsedHanreiPage>,
        seen: &mut BTreeSet<String>,
    ) -> Result<()> {
        if !self.storage.exists(Path::new(chm_path))? {
            return Ok(());
        }
        let Some(resolved) = self.storage.resolve_casefolded(Path::new(chm_path))? else {
            return Ok(());
        };
        let mut entries = match list_chm_entries(&resolved) {
            Ok(entries) => entries,
            Err(err) => {
                let item_id = chm_path.replace('\\', "/");
                if seen.insert(item_id.to_ascii_lowercase()) {
                    pages.push(SsedHanreiPage {
                        item_id: item_id.clone(),
                        label: ssed_hanrei_page_label(&item_id),
                        resource: InternalResource::PackageFile {
                            path: item_id,
                            resource_kind: ResourceKind::Other,
                        },
                        anchor: None,
                        diagnostics: vec![Diagnostic::info(
                            "ssed_hanrei_chm_deferred",
                            format!("HANREI.chm was found, but CHM decoding failed: {err}"),
                        )],
                    });
                }
                return Ok(());
            }
        };
        entries.sort_by_key(|entry| chm_hanrei_entry_sort_key(&entry.path));
        let mut hhc_items = Vec::new();
        for entry in &entries {
            if !path_has_extension(&entry.path, &["hhc"]) {
                continue;
            }
            if let Ok(bytes) = read_chm_entry(&resolved, &entry.path) {
                let html = decode_package_html_text(&bytes);
                hhc_items.extend(parse_chm_hhc_toc(&html));
            }
        }
        let mut html_count = 0usize;
        for entry in entries.iter().filter(|entry| {
            path_has_extension(&entry.path, &["html", "htm"])
                && chm_hanrei_entry_sort_key(&entry.path).0 == 0
        }) {
            if self.push_ssed_hanrei_chm_entry_page(
                chm_path,
                &entry.path,
                None,
                None,
                pages,
                seen,
            )? {
                html_count += 1;
            }
        }
        for item in hhc_items {
            let Some(local) = item.local.as_deref() else {
                continue;
            };
            let Some(reference) = chm_local_reference(local) else {
                continue;
            };
            if !path_has_extension(&reference.path, &["html", "htm"]) {
                continue;
            }
            if self.push_ssed_hanrei_chm_entry_page(
                chm_path,
                &reference.path,
                reference.anchor,
                Some(item.name),
                pages,
                seen,
            )? {
                html_count += 1;
            }
        }
        for entry in entries {
            if !path_has_extension(&entry.path, &["html", "htm"]) {
                continue;
            }
            if self.push_ssed_hanrei_chm_entry_page(
                chm_path,
                &entry.path,
                None,
                None,
                pages,
                seen,
            )? {
                html_count += 1;
            }
        }
        if html_count == 0 {
            let item_id = chm_path.replace('\\', "/");
            if seen.insert(item_id.to_ascii_lowercase()) {
                pages.push(SsedHanreiPage {
                    item_id: item_id.clone(),
                    label: ssed_hanrei_page_label(&item_id),
                    resource: InternalResource::PackageFile {
                        path: item_id,
                        resource_kind: ResourceKind::Other,
                    },
                    anchor: None,
                    diagnostics: vec![Diagnostic::info(
                        "ssed_hanrei_chm_deferred",
                        "HANREI.chm was found, but no HTML entries were discovered",
                    )],
                });
            }
        }
        Ok(())
    }

    fn push_ssed_hanrei_chm_entry_page(
        &self,
        chm_path: &str,
        entry_path: &str,
        anchor: Option<String>,
        label: Option<String>,
        pages: &mut Vec<SsedHanreiPage>,
        seen: &mut BTreeSet<String>,
    ) -> Result<bool> {
        let item_id = if let Some(anchor) = &anchor {
            format!("{chm_path}!/{entry_path}#{anchor}")
        } else {
            format!("{chm_path}!/{entry_path}")
        };
        if !seen.insert(item_id.to_ascii_lowercase()) {
            return Ok(false);
        }
        pages.push(SsedHanreiPage {
            item_id: item_id.clone(),
            label: label.unwrap_or_else(|| ssed_hanrei_page_label(&item_id)),
            resource: InternalResource::ChmFile {
                chm_path: chm_path.to_owned(),
                entry_path: entry_path.to_owned(),
                resource_kind: ResourceKind::Html,
            },
            anchor,
            diagnostics: Vec::new(),
        });
        Ok(true)
    }
}

fn push_surface_if_exists(
    surfaces: &mut Vec<HomeSurface>,
    storage: &DirectoryStorage,
    surface_id: &str,
    kind: NavigationSurfaceKind,
    title: &str,
    candidates: &[&str],
) -> Result<()> {
    if candidates
        .iter()
        .any(|candidate| storage.exists(Path::new(candidate)).unwrap_or(false))
    {
        surfaces.push(HomeSurface {
            surface_id: surface_id.to_owned(),
            kind,
            status: NavigationStatus::Available,
            title_html: title.to_owned(),
            title_text: title.to_owned(),
            target: Some(TargetToken::new(&InternalTarget::MenuItem {
                surface_id: surface_id.to_owned(),
                item_id: "root".to_owned(),
            })?),
            diagnostics: Vec::new(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct SsedHanreiPage {
    item_id: String,
    label: String,
    resource: InternalResource,
    anchor: Option<String>,
    diagnostics: Vec<Diagnostic>,
}

fn lved_media_resource(raw_ref: &str) -> Option<InternalResource> {
    let (namespace, key) = raw_ref.strip_prefix("lved.media.")?.split_once(':')?;
    if key.is_empty() {
        return None;
    }
    let lower_namespace = namespace.to_lowercase();
    let lower_key = key.to_lowercase();
    let audio = lower_namespace.contains("sound")
        || lower_namespace.contains("audio")
        || lower_key.ends_with(".mp3")
        || lower_key.ends_with(".wav");
    let image = lower_namespace.contains("image")
        || lower_namespace.contains("picture")
        || lower_key.ends_with(".png")
        || lower_key.ends_with(".jpg")
        || lower_key.ends_with(".jpeg")
        || lower_key.ends_with(".gif")
        || lower_key.ends_with(".svg")
        || lower_key.ends_with(".bmp");
    let resource_kind = if audio {
        ResourceKind::Audio
    } else if image {
        ResourceKind::Image
    } else {
        ResourceKind::MediaBlob
    };
    let store = if audio { "lved.mediasub" } else { "lved.media" };
    Some(InternalResource::MediaBlob {
        store: store.to_owned(),
        key: key.to_owned(),
        resource_kind,
    })
}

fn lved_list_label_html(title_html: &str, subtitle_html: &str) -> String {
    if subtitle_html.is_empty() {
        title_html.to_owned()
    } else {
        format!(r#"{title_html}<span class="lvcore-subtitle"> {subtitle_html}</span>"#)
    }
}

fn lved_tree_items_to_nodes(
    rows: &[crate::lved_sqlite::LvedTreeIndexItem],
) -> Result<Vec<NavigationNode>> {
    let mut cursor = 0usize;
    let Some(first) = rows.first() else {
        return Ok(Vec::new());
    };
    lved_tree_level_to_nodes(rows, &mut cursor, first.level)
}

fn lved_tree_level_to_nodes(
    rows: &[crate::lved_sqlite::LvedTreeIndexItem],
    cursor: &mut usize,
    level: u32,
) -> Result<Vec<NavigationNode>> {
    let mut nodes = Vec::new();
    while let Some(item) = rows.get(*cursor) {
        if item.level < level {
            break;
        }
        if item.level > level {
            nodes.extend(lved_tree_level_to_nodes(rows, cursor, item.level)?);
            continue;
        }
        let item_index = *cursor;
        *cursor += 1;
        let children = if rows
            .get(*cursor)
            .is_some_and(|next_item| next_item.level > item.level)
        {
            lved_tree_level_to_nodes(rows, cursor, rows[*cursor].level)?
        } else {
            Vec::new()
        };
        let target = if item.data_id >= 0 {
            Some(TargetToken::new(&InternalTarget::LvedRow {
                table: "content".to_owned(),
                row_id: item.data_id,
                anchor: None,
            })?)
        } else {
            None
        };
        nodes.push(NavigationNode {
            node_id: format!("tree:{}:{}", item.data_id, item_index),
            label_html: escape_plain_label_html(&item.label),
            label_text: item.label.clone(),
            target,
            diagnostics: Vec::new(),
            children,
        });
    }
    Ok(nodes)
}

fn multiview_menu_item_to_node(item: &MultiviewMenuItem, node_id: &str) -> Result<NavigationNode> {
    let target = item
        .href
        .as_ref()
        .map(|href| {
            TargetToken::new(&InternalTarget::MultiviewHref {
                href: href.clone(),
                anchor: item.anchor.clone(),
            })
        })
        .transpose()?;
    let children = item
        .children
        .iter()
        .enumerate()
        .map(|(index, child)| multiview_menu_item_to_node(child, &format!("{node_id}.{index}")))
        .collect::<Result<Vec<_>>>()?;
    Ok(NavigationNode {
        node_id: node_id.to_owned(),
        label_html: escape_plain_label_html(&item.label),
        label_text: item.label.clone(),
        target,
        diagnostics: Vec::new(),
        children,
    })
}

fn ssed_menu_records_to_nodes(
    package: &StubBookPackage,
    records: &[SsedMenuRecord],
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<NavigationNode>> {
    let mut roots = Vec::new();
    let mut path = Vec::<usize>::new();

    for (index, record) in records.iter().enumerate() {
        let label = record.label();
        if label.is_empty() {
            continue;
        }
        let target = ssed_menu_record_target(package, record, diagnostics)?;
        let rich_label = package.ssed_rich_label(label);
        let node = NavigationNode {
            node_id: format!("ssed-menu:{index}"),
            label_html: rich_label.html,
            label_text: rich_label.text,
            target,
            diagnostics: rich_label.diagnostics,
            children: Vec::new(),
        };
        let depth = record.depth.max(1);
        while path.len() >= depth {
            path.pop();
        }
        if path.is_empty() {
            roots.push(node);
            path.push(roots.len() - 1);
        } else if let Some(parent) = navigation_node_mut_at_path(&mut roots, &path) {
            parent.children.push(node);
            path.push(parent.children.len() - 1);
        } else {
            diagnostics.push(Diagnostic::warning(
                "ssed_navigation_tree_depth_invalid",
                format!("could not attach MENU/TOC row {index} at depth {depth}"),
            ));
            roots.push(node);
            path.clear();
            path.push(roots.len() - 1);
        }
    }

    Ok(roots)
}

fn navigation_node_mut_at_path<'a>(
    nodes: &'a mut [NavigationNode],
    path: &[usize],
) -> Option<&'a mut NavigationNode> {
    let (&first, rest) = path.split_first()?;
    let mut node = nodes.get_mut(first)?;
    for index in rest {
        node = node.children.get_mut(*index)?;
    }
    Some(node)
}

fn ssed_menu_record_target(
    package: &StubBookPackage,
    record: &SsedMenuRecord,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<TargetToken>> {
    let Some(destination) = record
        .links
        .iter()
        .filter_map(|link| link.destination.as_ref())
        .find(|destination| !destination.is_null())
    else {
        return Ok(None);
    };
    let Some(catalog) = &package.ssed_catalog else {
        diagnostics.push(Diagnostic::error(
            "ssed_catalog_missing",
            "SSED menu destination cannot be resolved without a catalog",
        ));
        return Ok(None);
    };
    let Some(component) = catalog.component_for_address(destination.block) else {
        diagnostics.push(Diagnostic::warning(
            "ssed_navigation_target_unresolved",
            format!(
                "MENU/TOC target block {} offset {} is outside declared components",
                destination.block, destination.offset
            ),
        ));
        return Ok(None);
    };
    if component
        .relative_offset(destination.block, destination.offset)
        .is_none()
    {
        diagnostics.push(Diagnostic::warning(
            "ssed_navigation_target_invalid",
            format!(
                "{} does not contain MENU/TOC target block {} offset {}",
                component.filename, destination.block, destination.offset
            ),
        ));
        return Ok(None);
    }
    if component.role != SsedComponentRole::Honmon {
        diagnostics.push(
            Diagnostic::info(
                "ssed_navigation_non_body_target_deferred",
                format!(
                    "MENU/TOC target points to {} ({:?}); non-body navigation routing is deferred",
                    component.filename, component.role
                ),
            )
            .with_context("component", &component.filename),
        );
        return Ok(None);
    }
    Ok(Some(TargetToken::new(&InternalTarget::SsedAddress {
        component: component.filename.clone(),
        block: destination.block,
        offset: destination.offset,
    })?))
}

fn ssed_panel_inline_cell_to_navigation_cell(
    package: &StubBookPackage,
    cell: &SsedPanelInlineCell,
) -> Result<PanelCell> {
    let target = if !cell.ref_id.is_empty() {
        Some(TargetToken::new(&InternalTarget::PanelCell {
            panel_id: cell.ref_id.clone(),
            row: 0,
            column: 0,
        })?)
    } else {
        None
    };
    let rich_label = package.ssed_rich_label(&cell.label);
    Ok(PanelCell {
        panel_id: cell.panel_id.clone(),
        row: cell.row.unwrap_or(cell.cell_index),
        column: cell.column.unwrap_or(0),
        label_html: rich_label.html,
        label_text: rich_label.text,
        target,
        diagnostics: rich_label.diagnostics,
    })
}

fn ssed_panel_bin_record_to_navigation_cell(
    package: &StubBookPackage,
    data_ref: &SsedPanelDataRef,
    record: &SsedPanelBinRecord,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<PanelCell> {
    let rich_label = package.ssed_rich_label(&record.text);
    Ok(PanelCell {
        panel_id: data_ref.panel_id.clone(),
        row: record.index,
        column: 0,
        label_html: rich_label.html,
        label_text: rich_label.text,
        target: ssed_panel_record_target(package, record, diagnostics)?,
        diagnostics: rich_label.diagnostics,
    })
}

fn ssed_panel_record_target(
    package: &StubBookPackage,
    record: &SsedPanelBinRecord,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<TargetToken>> {
    if record.block == 0 && record.offset == 0 {
        return Ok(None);
    }
    let Some(catalog) = &package.ssed_catalog else {
        diagnostics.push(Diagnostic::error(
            "ssed_catalog_missing",
            "Panel BIN target cannot be resolved without a catalog",
        ));
        return Ok(None);
    };
    let Some(component) = catalog.component_for_address(record.block) else {
        diagnostics.push(Diagnostic::warning(
            "ssed_panel_target_unresolved",
            format!(
                "Panel target block {} offset {} is outside declared components",
                record.block, record.offset
            ),
        ));
        return Ok(None);
    };
    if component
        .relative_offset(record.block, record.offset)
        .is_none()
    {
        diagnostics.push(Diagnostic::warning(
            "ssed_panel_target_invalid",
            format!(
                "{} does not contain Panel target block {} offset {}",
                component.filename, record.block, record.offset
            ),
        ));
        return Ok(None);
    }
    if component.role != SsedComponentRole::Honmon {
        diagnostics.push(
            Diagnostic::info(
                "ssed_panel_non_body_target_deferred",
                format!(
                    "Panel target points to {} ({:?}); non-body panel routing is deferred",
                    component.filename, component.role
                ),
            )
            .with_context("component", &component.filename),
        );
        return Ok(None);
    }
    Ok(Some(TargetToken::new(&InternalTarget::SsedAddress {
        component: component.filename.clone(),
        block: record.block,
        offset: record.offset,
    })?))
}

#[derive(Debug, Clone)]
struct OrderedSequenceTarget {
    target: TargetToken,
    title: Option<String>,
}

fn hourei_law_node_label(entry: &crate::hourei::HoureiLawEntry) -> String {
    if let Some(name_sub) = &entry.name_sub
        && !name_sub.trim().is_empty()
    {
        return format!("{} {}", entry.name, name_sub);
    }
    if !entry.name.trim().is_empty() {
        return entry.name.clone();
    }
    if let Some(abbr1) = &entry.abbr1
        && !abbr1.trim().is_empty()
    {
        return abbr1.clone();
    }
    entry.hore_id.clone()
}

fn collect_navigation_node_targets(nodes: &[NavigationNode], out: &mut Vec<TargetToken>) {
    for node in nodes {
        if let Some(target) = &node.target {
            out.push(target.clone());
        }
        collect_navigation_node_targets(&node.children, out);
    }
}

fn collect_navigation_node_ordered_targets(
    nodes: &[NavigationNode],
    out: &mut Vec<OrderedSequenceTarget>,
) {
    for node in nodes {
        if let Some(target) = &node.target {
            out.push(OrderedSequenceTarget {
                target: target.clone(),
                title: Some(node.label_text.clone()),
            });
        }
        collect_navigation_node_ordered_targets(&node.children, out);
    }
}

fn collect_panel_cell_ordered_targets(cells: &[PanelCell], out: &mut Vec<OrderedSequenceTarget>) {
    for cell in cells {
        if let Some(target) = &cell.target {
            out.push(OrderedSequenceTarget {
                target: target.clone(),
                title: Some(cell.label_text.clone()),
            });
        }
    }
}

fn escape_plain_label_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn decode_ssed_body_search_text(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len());
    let mut index = 0usize;
    while index < data.len() {
        let byte = data[index];
        if byte == 0 {
            index += 1;
            continue;
        }
        if byte == 0x1f {
            out.push(' ');
            index = index.saturating_add(2);
            if index < data.len() && data[index] <= 0x10 {
                index += 1;
            }
            if index < data.len() && data[index] <= 0x10 {
                index += 1;
            }
            continue;
        }
        if byte < 0x20 {
            out.push(' ');
            index += 1;
            continue;
        }
        if index + 1 < data.len()
            && (0x21..=0x7e).contains(&byte)
            && (0x21..=0x7e).contains(&data[index + 1])
            && let Some(decoded) = decode_jis_pair(byte, data[index + 1])
        {
            out.push(decoded);
            index += 2;
            continue;
        }
        if (0xa1..=0xfe).contains(&byte) {
            out.push(' ');
            index = index.saturating_add(2);
            continue;
        }
        if index + 1 < data.len()
            && ((0x81..=0x9f).contains(&byte) || (0xe0..=0xfc).contains(&byte))
        {
            let (decoded, _encoding, had_errors) = SHIFT_JIS.decode(&data[index..index + 2]);
            if !had_errors {
                out.push_str(decoded.as_ref());
                index += 2;
                continue;
            }
        }
        if byte <= 0x7e {
            out.push(byte as char);
        }
        index += 1;
    }
    collapse_search_whitespace(&narrow_fullwidth_ascii_text(&out))
}

fn ssed_fulltext_snippet_html(body_text: &str, query: &str) -> Option<String> {
    let body_text = collapse_search_whitespace(body_text);
    if body_text.is_empty() {
        return None;
    }
    let normalized_body = normalize_search_match_text(&body_text);
    let normalized_query = normalize_search_match_text(query);
    let start = normalized_body
        .find(&normalized_query)
        .and_then(|byte_index| {
            normalized_body[..byte_index]
                .chars()
                .count()
                .checked_sub(SSED_FULLTEXT_SNIPPET_CHARS / 4)
        })
        .unwrap_or(0);
    let snippet = body_text
        .chars()
        .skip(start)
        .take(SSED_FULLTEXT_SNIPPET_CHARS)
        .collect::<String>();
    Some(escape_plain_label_html(&snippet))
}

fn normalize_search_match_text(value: &str) -> String {
    narrow_fullwidth_ascii_text(value).to_lowercase()
}

fn looks_like_raw_anchor_label(value: &str) -> bool {
    let value = value.trim();
    value.len() >= 4 && value.chars().all(|ch| ch.is_ascii_digit())
}

fn find_ssed_dense_anchor_record_end(data: &[u8]) -> Option<usize> {
    data.windows(2)
        .enumerate()
        .skip(1)
        .find_map(|(index, window)| (window == [0x1f, 0x0a]).then_some(index))
        .or_else(|| {
            data.windows(4)
                .enumerate()
                .skip(1)
                .find_map(|(index, window)| (window == [0x1f, 0x09, 0x00, 0x01]).then_some(index))
        })
}

fn ssed_fulltext_body_window_len(rows: &[SsedFulltextRow], index: usize) -> usize {
    let Some(row) = rows.get(index) else {
        return SSED_FULLTEXT_BODY_WINDOW_BYTES;
    };
    rows[index + 1..]
        .iter()
        .find_map(|next| {
            next.offset
                .checked_sub(row.offset)
                .filter(|length| *length > 0)
        })
        .and_then(|length| usize::try_from(length).ok())
        .map(|length| length.min(SSED_FULLTEXT_BODY_WINDOW_BYTES))
        .unwrap_or(SSED_FULLTEXT_BODY_WINDOW_BYTES)
}

fn collapse_search_whitespace(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut pending_space = false;
    for ch in value.chars() {
        if ch.is_whitespace() || ch.is_control() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
    }
    out.trim().to_owned()
}

fn narrow_fullwidth_ascii_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(ch as u32 - 0xfee0).unwrap_or(ch),
            '\u{3000}' => ' ',
            _ => ch,
        })
        .collect()
}

fn decode_offset_cursor(cursor: Option<&str>) -> usize {
    cursor
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HtmlAttrName {
    Href,
    Src,
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HtmlAttrRange {
    name: HtmlAttrName,
    value_start: usize,
    value_end: usize,
}

fn next_html_href_or_src_attr(html: &str, cursor: usize) -> Option<HtmlAttrRange> {
    let lower = html.to_ascii_lowercase();
    let patterns = [
        ("href=\"", HtmlAttrName::Href),
        ("href='", HtmlAttrName::Href),
        ("src=\"", HtmlAttrName::Src),
        ("src='", HtmlAttrName::Src),
        ("data=\"", HtmlAttrName::Data),
        ("data='", HtmlAttrName::Data),
    ];
    let (attr_start, pattern, name) = patterns
        .iter()
        .filter_map(|(pattern, name)| {
            lower[cursor..]
                .find(pattern)
                .map(|offset| (cursor + offset, *pattern, *name))
        })
        .min_by_key(|(start, _, _)| *start)?;
    let quote = pattern.as_bytes()[pattern.len() - 1];
    let value_start = attr_start + pattern.len();
    let value_end = html.as_bytes()[value_start..]
        .iter()
        .position(|byte| *byte == quote)
        .map(|offset| value_start + offset)?;
    Some(HtmlAttrRange {
        name,
        value_start,
        value_end,
    })
}

fn html_unescape_minimal(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageHtmlReference {
    path: String,
    anchor: Option<String>,
}

fn package_html_base_dir(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(base, _)| base.to_owned())
        .unwrap_or_default()
}

fn package_relative_html_reference(
    base_dir: &str,
    raw_value: &str,
) -> Option<PackageHtmlReference> {
    let value = html_unescape_minimal(raw_value).trim().replace('\\', "/");
    if value.is_empty()
        || value.starts_with('#')
        || value.starts_with('/')
        || value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("mailto:")
        || value.starts_with("javascript:")
        || value.starts_with("data:")
        || value.starts_with("lvcore://")
    {
        return None;
    }
    let (path_part, anchor) = value.split_once('#').unwrap_or((value.as_str(), ""));
    let path_part = path_part.split('?').next().unwrap_or("").trim();
    if path_part.is_empty() {
        return None;
    }
    let joined = if base_dir.is_empty() {
        path_part.to_owned()
    } else {
        format!("{base_dir}/{path_part}")
    };
    Some(PackageHtmlReference {
        path: normalize_package_relative_path(&joined)?,
        anchor: (!anchor.is_empty()).then(|| anchor.to_owned()),
    })
}

fn normalize_package_relative_path(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn path_has_extension(path: &str, extensions: &[&str]) -> bool {
    let extension = path.rsplit_once('.').map(|(_, extension)| extension);
    extension.is_some_and(|extension| {
        extensions
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
    })
}

fn resource_kind_from_path(path: &str) -> ResourceKind {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".mp3") || lower.ends_with(".wav") {
        ResourceKind::Audio
    } else if lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".svg")
        || lower.ends_with(".bmp")
    {
        ResourceKind::Image
    } else if lower.ends_with(".css") {
        ResourceKind::Css
    } else if lower.ends_with(".js") {
        ResourceKind::Javascript
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        ResourceKind::Html
    } else {
        ResourceKind::Other
    }
}

fn decode_package_html_text(data: &[u8]) -> String {
    match std::str::from_utf8(data) {
        Ok(value) => value.to_owned(),
        Err(_) => {
            let (decoded, _, _) = SHIFT_JIS.decode(data);
            decoded.into_owned()
        }
    }
}

fn resolved_kind_for_package_html_path(path: &str) -> ResolvedTargetKind {
    let lower = path.to_ascii_lowercase();
    if lower.contains("hanrei")
        || lower.contains("_help.localized/")
        || lower.starts_with("hanrei/")
        || lower.starts_with("hanrei.")
    {
        ResolvedTargetKind::HanreiPage
    } else {
        ResolvedTargetKind::InfoPage
    }
}

fn scroll_anchor_for_token(target: &TargetToken) -> Result<Option<String>> {
    Ok(match target.decode()? {
        InternalTarget::LvedRow { anchor, .. }
        | InternalTarget::LvedInfoPage { anchor, .. }
        | InternalTarget::HoureiLaw { anchor, .. }
        | InternalTarget::MultiviewHref { anchor, .. }
        | InternalTarget::Resource { anchor, .. } => anchor,
        _ => None,
    })
}

fn ssed_hanrei_page_label(path: &str) -> String {
    if let Some((_chm, entry)) = path.split_once("!/") {
        return format!("CHM: {entry}");
    }
    if path_has_extension(path, &["chm"]) {
        return "HANREI.chm".to_owned();
    }
    if path.contains("_HELP.localized/contents/hanrei.") {
        return "Mac help: 凡例".to_owned();
    }
    if path.contains("_HELP.localized/contents/copyright.") {
        return "Mac help: copyright".to_owned();
    }
    if path.contains("_HELP.localized/menu.") {
        return "Mac help: menu".to_owned();
    }
    if path.contains("_HELP.localized/top.") {
        return "Mac help: top".to_owned();
    }
    if path.contains("_HELP.localized/index.") {
        return "Mac help: index".to_owned();
    }
    path.to_owned()
}

fn chm_hanrei_entry_sort_key(path: &str) -> (u8, String) {
    let file_name = Path::new(path)
        .file_name()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let priority = match file_name.as_str() {
        "top.htm" | "top.html" | "index.htm" | "index.html" => 0,
        "hanrei.htm" | "hanrei.html" => 1,
        "copyright.htm" | "copyright.html" => 9,
        _ => 5,
    };
    (priority, path.to_ascii_lowercase())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChmHhcTocItem {
    name: String,
    local: Option<String>,
    depth: usize,
}

fn parse_chm_hhc_toc(html: &str) -> Vec<ChmHhcTocItem> {
    let lower = html.to_ascii_lowercase();
    let mut items = Vec::new();
    let mut cursor = 0usize;
    let mut depth = 0usize;
    while cursor < lower.len() {
        let next_ul = lower[cursor..].find("<ul").map(|offset| cursor + offset);
        let next_ul_end = lower[cursor..].find("</ul").map(|offset| cursor + offset);
        let next_object = lower[cursor..]
            .find("<object")
            .map(|offset| cursor + offset);
        let Some(next) = [next_ul, next_ul_end, next_object]
            .into_iter()
            .flatten()
            .min()
        else {
            break;
        };
        if Some(next) == next_ul {
            depth += 1;
            cursor = lower[next..]
                .find('>')
                .map(|offset| next + offset + 1)
                .unwrap_or(lower.len());
        } else if Some(next) == next_ul_end {
            depth = depth.saturating_sub(1);
            cursor = lower[next..]
                .find('>')
                .map(|offset| next + offset + 1)
                .unwrap_or(lower.len());
        } else {
            let Some(relative_end) = lower[next..].find("</object>") else {
                break;
            };
            let end = next + relative_end + "</object>".len();
            let block = &html[next..end];
            if block.to_ascii_lowercase().contains("text/sitemap")
                && let Some(name) = chm_hhc_param_value(block, "name")
            {
                items.push(ChmHhcTocItem {
                    name,
                    local: chm_hhc_param_value(block, "local"),
                    depth: depth.saturating_sub(1),
                });
            }
            cursor = end;
        }
    }
    items
}

fn chm_hhc_param_value(block: &str, wanted_name: &str) -> Option<String> {
    let lower = block.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(relative_start) = lower[cursor..].find("<param") {
        let start = cursor + relative_start;
        let Some(relative_end) = lower[start..].find('>') else {
            break;
        };
        let end = start + relative_end + 1;
        let tag = &block[start..end];
        let Some(name) = html_attr_value(tag, "name") else {
            cursor = end;
            continue;
        };
        if name.eq_ignore_ascii_case(wanted_name) {
            return html_attr_value(tag, "value");
        }
        cursor = end;
    }
    None
}

fn chm_hhc_toc_items_to_nodes(
    chm_path: &str,
    items: &[ChmHhcTocItem],
) -> Result<Vec<NavigationNode>> {
    let mut index = 0usize;
    build_chm_hhc_nodes(chm_path, items, &mut index, 0)
}

fn build_chm_hhc_nodes(
    chm_path: &str,
    items: &[ChmHhcTocItem],
    index: &mut usize,
    depth: usize,
) -> Result<Vec<NavigationNode>> {
    let mut nodes = Vec::new();
    while let Some(item) = items.get(*index) {
        if item.depth < depth {
            break;
        }
        if item.depth > depth {
            break;
        }
        let node_index = *index;
        *index += 1;
        let mut node = chm_hhc_item_to_node(chm_path, item, node_index)?;
        node.children = build_chm_hhc_nodes(chm_path, items, index, depth + 1)?;
        nodes.push(node);
    }
    Ok(nodes)
}

fn chm_hhc_item_to_node(
    chm_path: &str,
    item: &ChmHhcTocItem,
    index: usize,
) -> Result<NavigationNode> {
    let target = item
        .local
        .as_deref()
        .and_then(chm_local_reference)
        .filter(|reference| path_has_extension(&reference.path, &["html", "htm"]))
        .map(|reference| {
            let resource = InternalResource::ChmFile {
                chm_path: chm_path.to_owned(),
                entry_path: reference.path,
                resource_kind: ResourceKind::Html,
            };
            let resource = ResourceToken::new(&resource)?;
            TargetToken::new(&InternalTarget::Resource {
                resource,
                anchor: reference.anchor,
            })
        })
        .transpose()?;
    Ok(NavigationNode {
        node_id: format!("hanrei-chm-toc-{index}"),
        label_html: escape_plain_label_html(&item.name),
        label_text: item.name.clone(),
        target,
        diagnostics: Vec::new(),
        children: Vec::new(),
    })
}

fn html_attr_value(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let attr = attr.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(relative_start) = lower[cursor..].find(&attr) {
        let start = cursor + relative_start;
        let before = lower[..start].chars().next_back();
        if before.is_some_and(|ch| !ch.is_ascii_whitespace() && ch != '<') {
            cursor = start + attr.len();
            continue;
        }
        let mut index = start + attr.len();
        index = skip_ascii_whitespace(&lower, index)?;
        if !lower[index..].starts_with('=') {
            cursor = start + attr.len();
            continue;
        }
        index += 1;
        index = skip_ascii_whitespace(&lower, index)?;
        let quote = lower[index..].chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        index += quote.len_utf8();
        let rest = &tag[index..];
        let end = rest.find(quote)?;
        return Some(html_unescape_minimal(&rest[..end]));
    }
    None
}

fn skip_ascii_whitespace(value: &str, mut index: usize) -> Option<usize> {
    while index < value.len() {
        let ch = value[index..].chars().next()?;
        if !ch.is_ascii_whitespace() {
            break;
        }
        index += ch.len_utf8();
    }
    Some(index)
}

fn chm_local_reference(raw_value: &str) -> Option<PackageHtmlReference> {
    let value = html_unescape_minimal(raw_value).trim().replace('\\', "/");
    let value = value.trim_start_matches('/');
    let (path_part, anchor) = value.split_once('#').unwrap_or((value, ""));
    let path_part = path_part.split('?').next().unwrap_or("").trim();
    if path_part.is_empty() {
        return None;
    }
    Some(PackageHtmlReference {
        path: normalize_package_relative_path(path_part)?,
        anchor: (!anchor.is_empty()).then(|| anchor.to_owned()),
    })
}

fn html_label_text(fragment: &str) -> String {
    let mut text = String::with_capacity(fragment.len());
    let mut in_tag = false;
    for ch in fragment.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            _ => text.push(ch),
        }
    }
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .trim()
        .to_owned()
}

fn html_basic_text(fragment: &str) -> String {
    let mut text = String::with_capacity(fragment.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for ch in fragment.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let tag_name = tag
                    .trim_start_matches('/')
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('/')
                    .to_ascii_lowercase();
                if matches!(
                    tag_name.as_str(),
                    "br" | "p"
                        | "div"
                        | "li"
                        | "tr"
                        | "table"
                        | "article"
                        | "section"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                ) {
                    text.push('\n');
                }
            }
            _ if in_tag => tag.push(ch),
            _ => text.push(ch),
        }
    }
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LvedHtmlRefKind {
    Media,
    DataId,
    Info,
}

fn next_lved_ref(value: &str) -> Option<(usize, LvedHtmlRefKind)> {
    let media = value
        .find("lved.media.")
        .map(|index| (index, LvedHtmlRefKind::Media));
    let dataid = value
        .find("lved.dataid:")
        .map(|index| (index, LvedHtmlRefKind::DataId));
    let info = value
        .find("lved.info:")
        .map(|index| (index, LvedHtmlRefKind::Info));
    [media, dataid, info]
        .into_iter()
        .flatten()
        .min_by_key(|found| found.0)
}

fn lved_dataid_target(raw_ref: &str) -> Option<InternalTarget> {
    let value = raw_ref.strip_prefix("lved.dataid:")?;
    let (row_id, anchor) = value.split_once('#').unwrap_or((value, ""));
    let row_id = row_id.parse::<i64>().ok()?;
    Some(InternalTarget::LvedRow {
        table: "content".to_owned(),
        row_id,
        anchor: (!anchor.is_empty()).then(|| anchor.to_owned()),
    })
}

fn lved_info_target(raw_ref: &str) -> Option<InternalTarget> {
    let value = raw_ref.strip_prefix("lved.info:")?;
    let (name, anchor) = value.split_once('#').unwrap_or((value, ""));
    if name.is_empty() {
        return None;
    }
    Some(InternalTarget::LvedInfoPage {
        name: html_unescape_minimal(name),
        anchor: (!anchor.is_empty()).then(|| html_unescape_minimal(anchor)),
    })
}

fn is_lved_ref_terminator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '>' | ')' | ']')
}

fn media_blob_label(token: &ResourceToken) -> Result<Option<String>> {
    match token.decode()? {
        InternalResource::MediaBlob { key, .. } => Ok(Some(key)),
        _ => Ok(None),
    }
}

fn root_fingerprint(root: &Path) -> String {
    let mut names = BTreeSet::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let name = path
                .file_name()
                .map(|v| v.to_string_lossy().to_string())
                .unwrap_or_default();
            names.insert(
                json!({
                    "name": name,
                    "is_file": metadata.is_file(),
                    "len": metadata.len(),
                })
                .to_string(),
            );
        }
    }
    let mut hasher = Sha256::new();
    for name in names {
        hasher.update(name.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

fn files_with_suffix(root: &Path, suffix: &str) -> Result<Vec<PathBuf>> {
    let mut rows = Vec::new();
    if !root.is_dir() {
        return Ok(rows);
    }
    let suffix = suffix.to_lowercase();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_file()
            && path
                .file_name()
                .map(|v| v.to_string_lossy().to_lowercase().ends_with(&suffix))
                .unwrap_or(false)
        {
            rows.push(path);
        }
    }
    rows.sort();
    Ok(rows)
}

fn load_package_uni_gaiji_maps(root: &Path) -> BTreeMap<String, String> {
    let mut merged = BTreeMap::new();
    let Ok(paths) = files_with_suffix(root, ".uni") else {
        return merged;
    };
    for path in paths {
        let Ok(data) = fs::read(&path) else {
            continue;
        };
        merged.extend(parse_uni_gaiji_map(&data));
    }
    merged
}

fn ga16_resource_covers_code(data: &[u8], code: &str) -> bool {
    if data.len() < 14 {
        return false;
    }
    if data[8] == 0 || data[9] == 0 {
        return false;
    }
    let Ok(code) = u16::from_str_radix(code, 16) else {
        return false;
    };
    let start = u16::from_be_bytes([data[10], data[11]]);
    let count = u16::from_be_bytes([data[12], data[13]]) as i32;
    ga16_grid_index(start, code).is_some_and(|index| index >= 0 && index < count)
}

fn ga16_grid_index(start: u16, code: u16) -> Option<i32> {
    let start_row = ((start >> 8) & 0xff) as i32;
    let start_cell = (start & 0xff) as i32;
    let row = ((code >> 8) & 0xff) as i32;
    let cell = (code & 0xff) as i32;
    if !(0x21..=0x7e).contains(&start_cell) || !(0x21..=0x7e).contains(&cell) {
        return Some(code as i32 - start as i32);
    }
    if row < start_row {
        return None;
    }
    Some((row - start_row) * 0x5e + (cell - start_cell))
}

fn package_root_for_detection(path: &Path) -> &Path {
    if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    }
}

fn inferred_folder_title(root: &Path) -> Option<String> {
    root.file_name().map(|name| {
        let raw = name.to_string_lossy();
        raw.strip_prefix("_DCT_").unwrap_or(raw.as_ref()).to_owned()
    })
}

fn multiview_menu_title(root: &Path) -> Result<Option<String>> {
    let path = root.join("menuData.xml");
    if !path.is_file() {
        return Ok(None);
    }
    let xml = fs::read_to_string(path)?;
    let items = parse_menu_data(&xml)?;
    Ok(items
        .into_iter()
        .map(|item| item.label.trim().to_owned())
        .find(|label| !label.is_empty()))
}

fn usable_multiview_title(title: &str) -> Option<String> {
    let title = title.trim();
    if title.is_empty() || title.contains('○') {
        None
    } else {
        Some(title.to_owned())
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn ssed_catalog_for_root(root: &Path) -> Result<SsedCatalog> {
    for path in files_with_suffix(root, ".idx")? {
        if let Ok(catalog) = SsedCatalog::parse_file(&path) {
            return Ok(catalog);
        }
    }
    Err(Error::Driver(
        "SSED catalog vanished after detection".to_owned(),
    ))
}

fn ssed_component_filename_aliases(component: &SsedComponent) -> Vec<String> {
    if component.role != SsedComponentRole::Honmon {
        return Vec::new();
    }
    let upper = component.filename.to_ascii_uppercase();
    if !matches!(upper.as_str(), "HONMON" | "HONMON.DIC" | "HONMON.DIN") {
        return Vec::new();
    }
    ["HONMON", "HONMON.DIC", "HONMON.DIN"]
        .into_iter()
        .filter(|alias| !alias.eq_ignore_ascii_case(&component.filename))
        .map(str::to_owned)
        .collect()
}

fn looks_like_zip_file(path: &Path) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut magic = [0_u8; 4];
    let read = file.read(&mut magic)?;
    Ok(read == magic.len() && magic == *b"PK\x03\x04")
}

fn zip_member_name_for_component(
    component: &SsedComponent,
    zip_path: &Path,
) -> Result<Option<String>> {
    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(file).map_err(zip_error)?;
    let mut desired = BTreeSet::new();
    desired.insert(component.filename.to_ascii_lowercase());
    for alias in ssed_component_filename_aliases(component) {
        desired.insert(alias.to_ascii_lowercase());
    }
    for index in 0..archive.len() {
        let member = archive.by_index_raw(index).map_err(zip_error)?;
        let name = member.name().replace('\\', "/");
        if desired.contains(&name.to_ascii_lowercase()) {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

fn zip_error(error: ZipError) -> Error {
    Error::Driver(format!("ZIP decode error: {error}"))
}

fn ssed_capabilities(catalog: &SsedCatalog, root: &Path) -> Vec<Capability> {
    let mut capabilities = vec![
        Capability::Resources,
        Capability::HcRenderInput,
        Capability::ContinuousView,
        Capability::DeferredRendering,
    ];
    let storage = DirectoryStorage::new(root.to_path_buf());
    if catalog.has_role(SsedComponentRole::Index) {
        capabilities.push(Capability::NativeSearch);
    }
    if catalog.has_role(SsedComponentRole::Index)
        && catalog
            .honmon()
            .is_some_and(|component| has_component_payload_casefolded(&storage, component))
    {
        capabilities.push(Capability::FullTextSearch);
    }
    if catalog.has_role(SsedComponentRole::Title) || catalog.has_role(SsedComponentRole::Index) {
        capabilities.push(Capability::TitleIndexBrowse);
    }
    if catalog.has_role(SsedComponentRole::Menu) {
        capabilities.push(Capability::Menu);
    }
    if catalog.has_role(SsedComponentRole::Toc) {
        capabilities.push(Capability::Toc);
    }
    if has_ssed_hanrei_casefolded(&storage) {
        capabilities.push(Capability::Hanrei);
    }
    if has_any_casefolded(&storage, &["Panels.xml", "Panel"]) {
        capabilities.push(Capability::Panels);
    }
    if catalog.has_role(SsedComponentRole::GaijiFull)
        || catalog.has_role(SsedComponentRole::GaijiHalf)
    {
        capabilities.push(Capability::Gaiji);
    }
    capabilities
}

fn has_any_casefolded(storage: &DirectoryStorage, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| storage.exists(Path::new(candidate)).unwrap_or(false))
}

fn has_ssed_hanrei_casefolded(storage: &DirectoryStorage) -> bool {
    if has_any_casefolded(
        storage,
        &[
            "HANREI.chm",
            "HANREI",
            "hanrei.html",
            "HANREI.html",
            "HANREI/index.html",
            "HANREI/index.htm",
            "HANREI/hanrei.html",
            "HANREI/hanrei.htm",
        ],
    ) {
        return true;
    }
    let Ok(entries) = storage.list_dir(Path::new("")) else {
        return false;
    };
    entries.into_iter().any(|path| {
        if !path.is_dir() {
            return false;
        }
        let Some(name) = path.file_name().map(|value| value.to_string_lossy()) else {
            return false;
        };
        if name.starts_with("._") || !name.to_ascii_lowercase().ends_with("_help.localized") {
            return false;
        }
        let root = name.replace('\\', "/");
        [
            format!("{root}/index.html"),
            format!("{root}/index.htm"),
            format!("{root}/menu.html"),
            format!("{root}/top.html"),
            format!("{root}/contents/hanrei.html"),
            format!("{root}/contents/hanrei.htm"),
            format!("{root}/contents/copyright.html"),
            format!("{root}/contents/copyright.htm"),
        ]
        .into_iter()
        .any(|candidate| storage.exists(Path::new(&candidate)).unwrap_or(false))
    })
}

fn has_component_payload_casefolded(storage: &DirectoryStorage, component: &SsedComponent) -> bool {
    storage
        .exists(Path::new(&component.filename))
        .unwrap_or(false)
        || ssed_component_filename_aliases(component)
            .iter()
            .any(|alias| storage.exists(Path::new(alias)).unwrap_or(false))
}

fn lved_capabilities() -> Vec<Capability> {
    vec![
        Capability::NativeSearch,
        Capability::FullTextSearch,
        Capability::TitleIndexBrowse,
        Capability::Hanrei,
        Capability::Resources,
        Capability::Gaiji,
        Capability::PreservedHtml,
        Capability::ContinuousView,
        Capability::DeferredRendering,
    ]
}

fn multiview_capabilities() -> Vec<Capability> {
    vec![
        Capability::NativeSearch,
        Capability::FullTextSearch,
        Capability::TitleIndexBrowse,
        Capability::Menu,
        Capability::Resources,
        Capability::Gaiji,
        Capability::PreservedHtml,
        Capability::ContinuousView,
        Capability::LawNavigation,
        Capability::DeferredRendering,
    ]
}

fn hourei_capabilities() -> Vec<Capability> {
    vec![
        Capability::NativeSearch,
        Capability::FullTextSearch,
        Capability::TitleIndexBrowse,
        Capability::Resources,
        Capability::PreservedHtml,
        Capability::ContinuousView,
        Capability::LawNavigation,
        Capability::DeferredRendering,
    ]
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rusqlite::Connection;
    use tempfile::tempdir;

    use crate::lved_sqlite::apply_sqlcipher_key;

    use super::*;

    #[test]
    fn parses_chm_hhc_toc_labels_and_anchors() {
        let items = parse_chm_hhc_toc(
            r#"
            <UL>
            <OBJECT type="text/sitemap">
              <param name="Name" value="編集方針">
              <param name="Local" value="Source/contents/hanrei_01.htm#midasigo">
            </OBJECT>
            <UL>
            <OBJECT type="text/sitemap">
              <param name="Name" value="見出し語">
              <param name="Local" value="Source/contents/hanrei_01.htm#midasigo_child">
            </OBJECT>
            </UL>
            <OBJECT type="text/sitemap">
              <param name="Name" value="付録">
            </OBJECT>
            <OBJECT type="text/sitemap">
              <param name="Name" value="著作権">
              <param name="Local" value="Source/contents/copyright.htm">
            </OBJECT>
            </UL>
            "#,
        );
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].name, "編集方針");
        assert_eq!(items[0].depth, 0);
        assert_eq!(items[1].name, "見出し語");
        assert_eq!(items[1].depth, 1);
        assert_eq!(items[2].name, "付録");
        assert!(items[2].local.is_none());
        let reference = chm_local_reference(items[0].local.as_deref().unwrap()).unwrap();
        assert_eq!(reference.path, "Source/contents/hanrei_01.htm");
        assert_eq!(reference.anchor.as_deref(), Some("midasigo"));

        let nodes = chm_hhc_toc_items_to_nodes("HANREI.chm", &items).unwrap();
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].label_text, "編集方針");
        assert!(nodes[0].target.is_some());
        assert_eq!(nodes[0].children.len(), 1);
        assert_eq!(nodes[0].children[0].label_text, "見出し語");
        assert!(nodes[1].target.is_none());
    }

    #[test]
    fn detects_lved_sqlite3_by_main_data_and_key() {
        let dir = tempdir().unwrap();
        write_lved_search_fixture(dir.path());

        let detected = LvedSqliteDriver.detect(dir.path()).unwrap().unwrap();
        assert_eq!(detected.format_family, FormatFamily::LvedSqlite3);
        assert!(
            detected
                .evidence
                .iter()
                .any(|item| item.starts_with("key_file:"))
        );
    }

    #[test]
    fn detects_multiview_by_menu_and_payload() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("menuData.xml"),
            br#"<list><item label="Visible Title" /></list>"#,
        )
        .unwrap();
        fs::write(dir.path().join("blvdat"), b"payload").unwrap();

        let detected = LvlMultiViewDriver.detect(dir.path()).unwrap().unwrap();
        assert_eq!(detected.format_family, FormatFamily::LvlMultiView);
        assert_eq!(detected.title.as_deref(), Some("Visible Title"));
    }

    #[test]
    fn detects_hourei_by_core_databases() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("_DataBase")).unwrap();
        fs::write(dir.path().join("_DataBase/hore_base.db"), b"").unwrap();
        fs::write(dir.path().join("_DataBase/hore_search_a.db"), b"").unwrap();
        fs::write(dir.path().join("_DataBase/horejo_base.db"), b"").unwrap();

        let detected = HoureiDriver.detect(dir.path()).unwrap().unwrap();
        assert_eq!(detected.format_family, FormatFamily::Hourei);
    }

    #[test]
    fn lved_search_hits_resolve_to_preserved_content_html() {
        let dir = tempdir().unwrap();
        write_lved_search_fixture(dir.path());
        let package = LvedSqliteDriver.open(dir.path()).unwrap();
        let surfaces = package.home_surfaces().unwrap();
        assert!(surfaces.iter().any(|surface| {
            surface.kind == NavigationSurfaceKind::TitleIndexBrowse
                && surface.surface_id == "lved-list"
                && surface.status == NavigationStatus::Available
                && surface.target.is_some()
        }));
        assert!(surfaces.iter().any(|surface| {
            surface.kind == NavigationSurfaceKind::Info
                && surface.status == NavigationStatus::Available
        }));
        let list_surface = package.open_surface("lved-list").unwrap();
        let list_items = match list_surface {
            NavigationSurface::TitleIndexBrowse { items, .. } => items,
            _ => panic!("expected LVED list title/index surface"),
        };
        assert_eq!(list_items.len(), 3);
        assert_eq!(list_items[0].label_text, "alpha subtitle");
        assert!(list_items[0].label_html.contains("lvcore://resource/"));
        assert!(!list_items[0].label_html.contains("src=\"AC6E.svg\""));
        assert!(matches!(
            list_items[0].target.decode().unwrap(),
            InternalTarget::LvedRow {
                table,
                row_id: 100,
                anchor: Some(anchor)
            } if table == "content" && anchor == "body-anchor"
        ));
        let info_surface = package.open_surface("info").unwrap();
        let info_target = match info_surface {
            NavigationSurface::InfoPages { pages, .. } => pages[0].target.clone(),
            _ => panic!("expected LVED info pages surface"),
        };
        let info_view = package
            .render_target(&info_target, &RenderOptions::default())
            .unwrap();
        assert_eq!(info_view.kind, ResolvedTargetKind::InfoPage);
        assert_eq!(
            info_view.display_html.as_deref(),
            Some("<h1>Example Dictionary 第2版</h1>")
        );
        let page = package
            .search(&SearchQuery {
                scope: crate::search::SearchScope::CurrentBook(package.metadata().book_id.clone()),
                mode: SearchMode::Forward,
                query: "alp".to_owned(),
                cursor: None,
                limit: 10,
            })
            .unwrap();

        assert_eq!(page.hits.len(), 1);
        assert_eq!(page.hits[0].title_text, "alpha");
        assert!(page.hits[0].title_html.contains("lvcore://resource/"));
        assert!(!page.hits[0].title_html.contains("src=\"AC6E.svg\""));
        assert!(matches!(
            page.hits[0].target.decode().unwrap(),
            InternalTarget::LvedRow {
                table,
                row_id: 100,
                anchor: Some(_)
            } if table == "content"
        ));

        let view = package
            .render_target(&page.hits[0].target, &RenderOptions::default())
            .unwrap();

        assert_eq!(view.kind, ResolvedTargetKind::EntryBody);
        let html = view.display_html.as_deref().unwrap();
        assert!(html.contains("<article><h1>Alpha</h1><p>body</p>"));
        assert!(html.contains("lvcore://resource/"));
        assert!(html.contains("lvcore://target/"));
        assert!(!html.contains("lved.dataid:101"));
        assert!(!html.contains("lved.info:help.html"));
        assert_eq!(view.links.len(), 2);
        assert!(view.links.iter().any(|link| matches!(
            link.token.decode().unwrap(),
            InternalTarget::LvedRow {
                table,
                row_id: 101,
                anchor: Some(anchor)
            } if table == "content" && anchor == "jump"
        )));
        let help_token = view
            .links
            .iter()
            .find_map(|link| match link.token.decode().unwrap() {
                InternalTarget::LvedInfoPage {
                    name,
                    anchor: Some(anchor),
                } if name == "help.html" && anchor == "top" => Some(link.token.clone()),
                _ => None,
            })
            .expect("expected lved.info link to be routed through TargetToken");
        let help_view = package
            .render_target(&help_token, &RenderOptions::default())
            .unwrap();
        assert_eq!(help_view.kind, ResolvedTargetKind::InfoPage);
        assert_eq!(help_view.display_html.as_deref(), Some("<h1>Help</h1>"));
        assert_eq!(view.resources.len(), 2);
        assert!(
            view.resources
                .iter()
                .any(|resource| resource.kind == ResourceKind::Image)
        );
        assert!(
            view.resources
                .iter()
                .any(|resource| resource.kind == ResourceKind::Audio)
        );
        let audio = view
            .resources
            .iter()
            .find(|resource| resource.kind == ResourceKind::Audio)
            .unwrap();
        assert_eq!(
            package.read_resource(&audio.token).unwrap(),
            b"ID3\x03".to_vec()
        );
        let image = view
            .resources
            .iter()
            .find(|resource| resource.kind == ResourceKind::Image)
            .unwrap();
        assert_eq!(
            package.read_resource(&image.token).unwrap(),
            b"<svg/>".to_vec()
        );

        let window = package
            .resolve_target_window(
                &page.hits[0].target,
                Some(&SequenceHint::LvedListOrder),
                0,
                2,
                &RenderOptions::default(),
            )
            .unwrap();
        assert!(window.before.is_empty());
        assert_eq!(window.after.len(), 2);
        assert_eq!(window.after[0].title.as_deref(), Some("beta"));
        assert_eq!(window.after[1].title.as_deref(), Some("gamma"));
    }

    #[test]
    fn dense_honmon_body_is_not_exposed_as_numeric_text() {
        let dir = tempdir().unwrap();
        let catalog = SsedCatalog {
            title: String::new(),
            components: Vec::new(),
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 0,
                trailing_bytes: 0,
            },
        };
        let package = StubBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 1,
                title: None,
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            StubPackageStores::default(),
        );
        let token = TargetToken::new(&InternalTarget::SsedDenseAnchor {
            anchor: "00100050".to_owned(),
            resolver_hint: Some("vlpljbl".to_owned()),
        })
        .unwrap();
        let body = package.visual_body_for_target(&token).unwrap();
        let text = serde_json::to_string(&body).unwrap();
        assert!(!text.contains("00100050"));
        assert!(matches!(body, VisualBody::Unsupported { .. }));
    }

    #[test]
    fn dense_honmon_address_target_resolves_sidecar_html() {
        let dir = tempdir().unwrap();
        let catalog = write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::BodyRows);
        let package = StubBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Dense".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            StubPackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );
        let target = TargetToken::new(&InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 100,
            offset: 32,
        })
        .unwrap();

        let body = package.visual_body_for_target(&target).unwrap();

        assert_eq!(
            body,
            VisualBody::PreservedHtml {
                html: "<div>beta sidecar html</div>".to_owned(),
                source: BodySourceKind::RendererDatabase,
            }
        );
        let view = package
            .render_target(&target, &RenderOptions::default())
            .unwrap();
        assert_eq!(
            view.display_html.as_deref(),
            Some("<div>beta sidecar html</div>")
        );
    }

    #[test]
    fn dense_honmon_search_hit_target_resolves_sidecar_html() {
        let dir = tempdir().unwrap();
        let catalog = write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::BodyRows);
        let package = StubBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Dense".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            StubPackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );
        let page = package
            .search(&SearchQuery {
                scope: crate::search::SearchScope::CurrentBook(package.metadata().book_id.clone()),
                mode: SearchMode::Exact,
                query: "い".to_owned(),
                cursor: None,
                limit: 10,
            })
            .unwrap();

        assert_eq!(page.hits.len(), 1);
        assert_eq!(page.hits[0].title_text, "beta");
        assert!(matches!(
            page.hits[0].target.decode().unwrap(),
            InternalTarget::SsedAddress { .. }
        ));
        let body = package
            .visual_body_for_target(&page.hits[0].target)
            .unwrap();
        assert!(matches!(
            body,
            VisualBody::PreservedHtml {
                source: BodySourceKind::RendererDatabase,
                ..
            }
        ));
    }

    #[test]
    fn title_only_sidecar_does_not_block_dense_body_sidecar() {
        let dir = tempdir().unwrap();
        let catalog = write_ssed_dense_sidecar_fixture(
            dir.path(),
            DenseSidecarFixture::TitleOnlyThenBodyRows,
        );
        let package = StubBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Dense".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            StubPackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );
        let target = TargetToken::new(&InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 100,
            offset: 0,
        })
        .unwrap();

        let body = package.visual_body_for_target(&target).unwrap();

        assert_eq!(
            body,
            VisualBody::PreservedHtml {
                html: "<div>alpha sidecar html</div>".to_owned(),
                source: BodySourceKind::RendererDatabase,
            }
        );
    }

    #[test]
    fn dense_sidecar_decodes_utf8_and_cp932_blob_text() {
        let dir = tempdir().unwrap();
        let catalog =
            write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::BlobBodyRows);
        let package = StubBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Dense".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            StubPackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );
        let beta = TargetToken::new(&InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 100,
            offset: 32,
        })
        .unwrap();

        let body = package.visual_body_for_target(&beta).unwrap();

        assert_eq!(
            body,
            VisualBody::PreservedHtml {
                html: "<div>ベータ html</div>".to_owned(),
                source: BodySourceKind::RendererDatabase,
            }
        );
        assert!(!serde_json::to_string(&body).unwrap().contains("b'"));
    }

    #[test]
    fn dense_sidecar_missing_row_is_unsupported_without_anchor_leak() {
        let dir = tempdir().unwrap();
        let catalog =
            write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::MissingBetaRow);
        let package = StubBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Dense".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            StubPackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );
        let target = TargetToken::new(&InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 100,
            offset: 32,
        })
        .unwrap();

        let body = package.visual_body_for_target(&target).unwrap();
        let json = serde_json::to_string(&body).unwrap();

        assert!(matches!(body, VisualBody::Unsupported { .. }));
        assert!(!json.contains("00000002"));
        assert!(json.contains("ssed_dense_sidecar_row_missing"));
    }

    #[test]
    fn ssed_fulltext_searches_honmon_body_windows() {
        let dir = tempdir().unwrap();
        let catalog = write_ssed_fulltext_fixture(dir.path());
        let package = StubBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Synthetic fulltext".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            StubPackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );
        assert!(
            package
                .metadata()
                .capabilities
                .contains(&Capability::FullTextSearch)
        );

        let page = package
            .search(&SearchQuery {
                scope: crate::search::SearchScope::CurrentBook(package.metadata().book_id.clone()),
                mode: SearchMode::FullText,
                query: "window needle".to_owned(),
                cursor: None,
                limit: 10,
            })
            .unwrap();

        assert_eq!(page.hits.len(), 1);
        assert_eq!(page.hits[0].title_text, "本文見出し");
        assert!(
            page.hits[0]
                .snippet_html
                .as_deref()
                .is_some_and(|snippet| snippet.contains("window needle"))
        );
        assert!(matches!(
            page.hits[0].target.decode().unwrap(),
            InternalTarget::SsedAddress {
                component,
                block: 100,
                offset: 0
            } if component == "HONMON.DIC"
        ));
        assert!(
            page.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "ssed_fulltext_body_window_scan")
        );
    }

    #[test]
    fn ssed_fulltext_matches_fullwidth_ascii_body_text() {
        let dir = tempdir().unwrap();
        let catalog = write_ssed_fulltext_fixture(dir.path());
        let package = StubBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Synthetic fulltext".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            StubPackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );

        let page = package
            .search(&SearchQuery {
                scope: crate::search::SearchScope::CurrentBook(package.metadata().book_id.clone()),
                mode: SearchMode::FullText,
                query: "fullwidth".to_owned(),
                cursor: None,
                limit: 10,
            })
            .unwrap();

        assert_eq!(page.hits.len(), 1);
    }

    enum DenseSidecarFixture {
        BodyRows,
        TitleOnlyThenBodyRows,
        BlobBodyRows,
        MissingBetaRow,
    }

    fn write_ssed_dense_sidecar_fixture(root: &Path, fixture: DenseSidecarFixture) -> SsedCatalog {
        let mut body = Vec::new();
        body.extend_from_slice(&dense_anchor_record("00000001"));
        body.extend_from_slice(&dense_anchor_record("00000002"));
        fs::write(
            root.join("HONMON.DIC"),
            fixture_sseddata_literal_chunks(&[&body], 100, 100),
        )
        .unwrap();

        let mut titles = Vec::new();
        let alpha_title_offset = 0u16;
        titles.extend_from_slice(b"alpha\x1f\x0a");
        let beta_title_offset = u16::try_from(titles.len()).unwrap();
        titles.extend_from_slice(b"beta\x1f\x0a");
        fs::write(
            root.join("FHTITLE.DIC"),
            fixture_sseddata_literal_chunks(&[&titles], 300, 300),
        )
        .unwrap();

        let mut index_page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
        index_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        index_page[2..4].copy_from_slice(&2u16.to_be_bytes());
        let mut pos = 4usize;
        write_simple_index_row(
            &mut index_page,
            &mut pos,
            &body_jis("あ"),
            100,
            0,
            300,
            alpha_title_offset,
        );
        write_simple_index_row(
            &mut index_page,
            &mut pos,
            &body_jis("い"),
            100,
            32,
            300,
            beta_title_offset,
        );
        fs::write(
            root.join("FHINDEX.DIC"),
            fixture_sseddata_literal_chunks(&[&index_page], 200, 200),
        )
        .unwrap();

        match fixture {
            DenseSidecarFixture::BodyRows => {
                write_dense_body_db(root.join("body.db"), true, true, false);
            }
            DenseSidecarFixture::TitleOnlyThenBodyRows => {
                let connection = Connection::open(root.join("a-title-only.db")).unwrap();
                connection
                    .execute_batch(
                        "
                        create table t_contents (f_DataId integer primary key, f_Title text);
                        insert into t_contents values (1, 'alpha title only');
                        ",
                    )
                    .unwrap();
                write_dense_body_db(root.join("body.db"), true, true, false);
            }
            DenseSidecarFixture::BlobBodyRows => {
                write_dense_body_db(root.join("body.db"), true, true, true);
            }
            DenseSidecarFixture::MissingBetaRow => {
                write_dense_body_db(root.join("body.db"), true, false, false);
            }
        }

        SsedCatalog {
            title: "Dense".to_owned(),
            components: vec![
                SsedComponent {
                    index: 0,
                    multi: 0,
                    component_type: 0x00,
                    start_block: 100,
                    end_block: 100,
                    data: [0; 4],
                    filename: "HONMON.DIC".to_owned(),
                    role: SsedComponentRole::Honmon,
                },
                SsedComponent {
                    index: 1,
                    multi: 0,
                    component_type: 0x03,
                    start_block: 300,
                    end_block: 300,
                    data: [0; 4],
                    filename: "FHTITLE.DIC".to_owned(),
                    role: SsedComponentRole::Title,
                },
                SsedComponent {
                    index: 2,
                    multi: 0,
                    component_type: 0x91,
                    start_block: 200,
                    end_block: 200,
                    data: [0; 4],
                    filename: "FHINDEX.DIC".to_owned(),
                    role: SsedComponentRole::Index,
                },
            ],
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 3,
                trailing_bytes: 0,
            },
        }
    }

    fn dense_anchor_record(anchor: &str) -> Vec<u8> {
        let mut record = Vec::new();
        record.extend_from_slice(&[0x1f, 0x09, 0x00, 0x01, 0x1f, 0x41]);
        record.extend_from_slice(&body_jis(anchor));
        record.extend_from_slice(&[0x1f, 0x61, 0x1f, 0x0a]);
        record.resize(32, 0);
        record
    }

    fn write_simple_index_row(
        page: &mut [u8],
        pos: &mut usize,
        key: &[u8],
        body_block: u32,
        body_offset: u16,
        title_block: u32,
        title_offset: u16,
    ) {
        page[*pos] = u8::try_from(key.len()).unwrap();
        *pos += 1;
        page[*pos..*pos + key.len()].copy_from_slice(key);
        *pos += key.len();
        page[*pos..*pos + 4].copy_from_slice(&body_block.to_be_bytes());
        page[*pos + 4..*pos + 6].copy_from_slice(&body_offset.to_be_bytes());
        page[*pos + 6..*pos + 10].copy_from_slice(&title_block.to_be_bytes());
        page[*pos + 10..*pos + 12].copy_from_slice(&title_offset.to_be_bytes());
        *pos += 12;
    }

    fn write_dense_body_db(path: PathBuf, alpha: bool, beta: bool, blob: bool) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "create table t_contents (f_DataId integer primary key, f_Title blob, f_Html blob, f_Plane blob);",
            )
            .unwrap();
        if alpha {
            connection
                .execute(
                    "insert into t_contents values (?, ?, ?, ?)",
                    (
                        1,
                        "alpha".as_bytes(),
                        "<div>alpha sidecar html</div>".as_bytes(),
                        "alpha sidecar body".as_bytes(),
                    ),
                )
                .unwrap();
        }
        if beta {
            if blob {
                connection
                    .execute(
                        "insert into t_contents values (?, ?, ?, ?)",
                        (
                            2,
                            cp932("ベータ"),
                            cp932("<div>ベータ html</div>"),
                            cp932("ベータ body"),
                        ),
                    )
                    .unwrap();
            } else {
                connection
                    .execute(
                        "insert into t_contents values (?, ?, ?, ?)",
                        (
                            2,
                            "beta".as_bytes(),
                            "<div>beta sidecar html</div>".as_bytes(),
                            "beta sidecar body".as_bytes(),
                        ),
                    )
                    .unwrap();
            }
        }
    }

    fn write_ssed_fulltext_fixture(root: &Path) -> SsedCatalog {
        let mut body = Vec::new();
        body.extend_from_slice(&[0x1f, 0x09, 0x00, 0x01, 0x1f, 0x41]);
        body.extend_from_slice(&body_jis(
            "この本文 has a window needle and ＦＵＬＬＷＩＤＴＨ text.",
        ));
        body.extend_from_slice(&[0x1f, 0x61, 0x1f, 0x0a]);
        fs::write(
            root.join("HONMON.DIC"),
            fixture_sseddata_literal_chunks(&[&body], 100, 100),
        )
        .unwrap();

        let title = cp932("本文見出し");
        fs::write(
            root.join("FHTITLE.DIC"),
            fixture_sseddata_literal_chunks(&[&title], 300, 300),
        )
        .unwrap();

        let mut index_page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
        index_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        index_page[2..4].copy_from_slice(&1u16.to_be_bytes());
        index_page[4] = 2;
        index_page[5..7].copy_from_slice(&[0x24, 0x22]);
        index_page[7..11].copy_from_slice(&100u32.to_be_bytes());
        index_page[11..13].copy_from_slice(&0u16.to_be_bytes());
        index_page[13..17].copy_from_slice(&300u32.to_be_bytes());
        index_page[17..19].copy_from_slice(&0u16.to_be_bytes());
        fs::write(
            root.join("FHINDEX.DIC"),
            fixture_sseddata_literal_chunks(&[&index_page], 200, 200),
        )
        .unwrap();

        SsedCatalog {
            title: "Synthetic fulltext".to_owned(),
            components: vec![
                SsedComponent {
                    index: 0,
                    multi: 0,
                    component_type: 0x00,
                    start_block: 100,
                    end_block: 100,
                    data: [0; 4],
                    filename: "HONMON.DIC".to_owned(),
                    role: SsedComponentRole::Honmon,
                },
                SsedComponent {
                    index: 1,
                    multi: 0,
                    component_type: 0x03,
                    start_block: 300,
                    end_block: 300,
                    data: [0; 4],
                    filename: "FHTITLE.DIC".to_owned(),
                    role: SsedComponentRole::Title,
                },
                SsedComponent {
                    index: 2,
                    multi: 0,
                    component_type: 0x71,
                    start_block: 200,
                    end_block: 200,
                    data: [0; 4],
                    filename: "FHINDEX.DIC".to_owned(),
                    role: SsedComponentRole::Index,
                },
            ],
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 3,
                trailing_bytes: 0,
            },
        }
    }

    fn cp932(value: &str) -> Vec<u8> {
        let (encoded, _encoding, _had_errors) = SHIFT_JIS.encode(value);
        encoded.into_owned()
    }

    fn body_jis(value: &str) -> Vec<u8> {
        value
            .chars()
            .flat_map(|ch| {
                let body_ch = if (0x20..=0x7e).contains(&(ch as u32)) {
                    if ch == ' ' {
                        '\u{3000}'
                    } else {
                        char::from_u32(ch as u32 + 0xfee0).unwrap_or(ch)
                    }
                } else {
                    ch
                };
                cp932(&body_ch.to_string())
                    .chunks(2)
                    .next()
                    .and_then(sjis_pair_to_jis_pair)
                    .unwrap_or_default()
            })
            .collect()
    }

    fn sjis_pair_to_jis_pair(sjis: &[u8]) -> Option<Vec<u8>> {
        if sjis.len() != 2 {
            return None;
        }
        let lead = sjis[0];
        let trail = sjis[1];
        let row_base = if (0x81..=0x9f).contains(&lead) {
            (lead - 0x81) * 2
        } else if (0xe0..=0xef).contains(&lead) {
            (lead - 0xc1) * 2
        } else {
            return None;
        };
        let (row, cell) = if (0x9f..=0xfc).contains(&trail) {
            (row_base + 1, trail - 0x9f)
        } else if (0x40..=0xfc).contains(&trail) && trail != 0x7f {
            let adjusted = if trail >= 0x80 { trail - 1 } else { trail };
            (row_base, adjusted - 0x40)
        } else {
            return None;
        };
        let first = row + 0x21;
        let second = cell + 0x21;
        ((0x21..=0x7e).contains(&first) && (0x21..=0x7e).contains(&second))
            .then(|| vec![first, second])
    }

    fn fixture_sseddata_literal_chunks(
        chunks: &[&[u8]],
        start_block: u32,
        end_block: u32,
    ) -> Vec<u8> {
        let chunk_count = chunks.len();
        let first_chunk_offset = 0x40 + chunk_count * 4;
        let mut data = vec![0u8; first_chunk_offset];
        data[..8].copy_from_slice(SSEDDATA_MAGIC);
        data[0x0f] = 1;
        data[0x16..0x18].copy_from_slice(&(chunk_count as u16).to_be_bytes());
        data[0x18..0x1c].copy_from_slice(&start_block.to_be_bytes());
        data[0x1c..0x20].copy_from_slice(&end_block.to_be_bytes());

        let mut compressed_chunks = Vec::with_capacity(chunk_count);
        let mut next_offset = first_chunk_offset;
        for (index, chunk) in chunks.iter().enumerate() {
            data[0x40 + index * 4..0x44 + index * 4]
                .copy_from_slice(&(next_offset as u32).to_be_bytes());
            let compressed = fixture_sseddata_literal_chunk(chunk);
            next_offset += compressed.len();
            compressed_chunks.push(compressed);
        }
        for compressed in compressed_chunks {
            data.extend_from_slice(&compressed);
        }
        data
    }

    fn fixture_sseddata_literal_chunk(literals: &[u8]) -> Vec<u8> {
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&[0, 0]);
        chunk.extend_from_slice(&(literals.len() as u16).to_be_bytes());
        chunk.push(0);
        for literal in literals {
            chunk.extend_from_slice(&[0, 0, *literal]);
        }
        chunk
    }

    fn write_lved_search_fixture(root: &Path) {
        let payload = root.join("main.data");
        let key = "test-key";
        {
            let connection = Connection::open(&payload).unwrap();
            apply_sqlcipher_key(&connection, key).unwrap();
            connection
                .execute_batch(
                    "
                    create table info (id integer, type integer, name text primary key, body text, media text);
                    insert into info values (1, 1, 'about.html', '<h1>Example Dictionary 第2版</h1>', '');
                    insert into info values (2, 1, 'help.html', '<h1>Help</h1>', '');
                    create table content (id integer primary key, type integer, body text, media text);
                    create table media (id integer primary key, name text, type integer, main blob);
                    create table mediasub (id integer primary key, name text, type integer, main blob);
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
                    insert into content values (100, 1, '<article><h1>Alpha</h1><p>body</p><object class=\"icon\" data=\"AC6E.svg\"></object><a href=\"lved.media.sound:00010033.mp3\">sound</a><a href=\"lved.dataid:101#jump\">next</a><a href=\"lved.info:help.html#top\">help</a></article>', '');
                    insert into content values (101, 1, '<article><h1>Beta</h1></article>', '');
                    insert into content values (102, 1, '<article><h1>Gamma</h1></article>', '');
                    insert into media values (1, 'AC6E', 4, X'3C7376672F3E');
                    insert into mediasub values (1, '00010033', 5, X'49443303');
                    insert into list values (1, 100, 1, 'body-anchor', '<img class=\"icon\" src=\"AC6E.svg\"><b>alpha</b>', '<span>subtitle</span>');
                    insert into list values (2, 101, 1, '', '<b>beta</b>', '');
                    insert into list values (3, 102, 1, '', '<b>gamma</b>', '');
                    insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                      values (1, 'alpha', 'ahpla', 'alpha', 'alpha body', '', '', '∥alpha∥');
                    ",
                )
                .unwrap();
        }
        fs::write(root.join("main.key"), key).unwrap();
    }
}

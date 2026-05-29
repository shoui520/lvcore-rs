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

use super::chm_toc::{
    chm_hanrei_entry_sort_key, chm_hhc_toc_items_to_nodes, chm_local_reference, parse_chm_hhc_toc,
};
use super::html::{
    html_basic_text, html_document_label, html_label_text, html_unescape_minimal,
    package_html_base_dir, package_relative_html_reference,
};
use super::ssed_zip::{
    copy_zip_member_with_size_limit, looks_like_zip_file, ssed_component_filename_aliases,
    zip_error, zip_member_name_for_component, zipped_ssed_component_size_limit,
};
use crate::body::{BodyProvider, BodySourceKind, VisualBody};
use crate::chm::{list_chm_entries, read_chm_entry};
use crate::crypto::{
    decrypt_android_diw_file_to_path, decrypt_android_diw_prefix,
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
use crate::image::encode_png_rgba;
use crate::lved_sqlite::{LvedSqliteStore, LvedSqliteSummary, infer_lved_dict_code};
use crate::multiview::{MultiviewMenuItem, MultiviewStore, parse_menu_data};
use crate::navigation::{
    HomeSurface, NavigationItem, NavigationNode, NavigationProvider, NavigationStatus,
    NavigationSurface, NavigationSurfaceKind, PanelCell, ScreenMenuHotspot, ScreenMenuRect,
    ScreenMenuScreen,
};
use crate::render::{
    RenderCapability, RenderMode, RenderOptions, RendererInput, RendererInputProvider,
    RendererProvider, ResolvedTargetKind, ResolvedTargetView,
};
use crate::resources::{
    InternalResource, ResourceKind, ResourceProvider, ResourceRef, ResourceToken,
};
use crate::search::{SearchHit, SearchMode, SearchPage, SearchProvider, SearchQuery};
use crate::sequence::{SequenceHint, SequenceProvider, TargetWindow};
use crate::ssed::{
    ANDROID_LVEDINFO_MAGIC, BLOCK_SIZE, SSEDDATA_MAGIC, SSEDINFO_MAGIC, SsedCatalog, SsedComponent,
    SsedComponentRole, SsedDataFile, SsedDataHeader,
};
use crate::ssed_aux_index::{
    SsedAuxIndexRow, SsedAuxIndexSpec, is_numeric_aux_index_filename,
    parse_aux_index_specs_from_exinfo, parse_aux_index_text_bytes,
};
use crate::ssed_encyclopedia::{SsedEncyclopediaRow, parse_encyclopedia_index};
use crate::ssed_figure::{FigureDimensions, figure_bitmap_to_png};
use crate::ssed_index::{
    INDEX_PAGE_SIZE, SsedIndexPointer, SsedIndexRow, SsedIndexScanState, decode_jis_pair,
    decode_title_text, is_leaf_page, is_simple_leaf_index_type, is_supported_index_type,
    parse_internal_page, parse_simple_leaf_page, parse_supported_leaf_page,
};
use crate::ssed_loose_media::{
    discover_britannica_top_dat_files, discover_britannica_whatday_paths, find_movie_file,
    has_britannica_top_dat_files, has_britannica_whatday_files, parse_lved_address,
    read_pcmu_record, render_britannica_html_fragment, resolve_loose_media_file,
    resolve_pcmu_record,
};
use crate::ssed_menu::{SsedMenuRecord, parse_menu_stream};
use crate::ssed_panel::{
    SsedPanelBinRecord, SsedPanelDataRef, SsedPanelInlineCell, parse_panel_bin,
    parse_panel_xml_bytes,
};
use crate::ssed_pcmdata::{
    PcmDataParseResult, pcmdata_audio_summary, pcmdata_portable_audio_bytes,
};
use crate::ssed_pdfspread::{
    find_pdfspread_database, lookup_pdfspread, normalize_pdfspread_page_id,
};
use crate::ssed_screen_menu::{
    SsedScreenMenuHotspot, SsedScreenMenuParse, parse_screen_menu_stream,
};
use crate::ssed_sidecar::{
    SsedSidecarBodyResolver, SsedSidecarKind, SsedSidecarLookup,
    discover_ssed_sidecar_body_resolvers, lookup_ssed_dense_sidecar_body_with_resolvers,
};
use crate::ssed_sound_data::{SoundDataIndex, load_sounddata_index};
use crate::storage::{DirectoryStorage, StorageBackend};
use crate::target::{InternalTarget, TargetLink, TargetToken};

use super::{
    BookAlias, BookAliasKind, BookId, BookMetadata, BookPackage, Capability, DetectedPackage,
    FormatFamily, PackageDriver,
};

pub struct SsedDriver;
pub struct LvedSqliteDriver;
pub struct LvlMultiViewDriver;
pub struct HoureiDriver;

struct DetectedSsedPackage {
    detected: DetectedPackage,
    catalog: SsedCatalog,
}

impl PackageDriver for SsedDriver {
    fn family(&self) -> FormatFamily {
        FormatFamily::Ssed
    }

    fn detect(&self, root: &Path) -> Result<Option<DetectedPackage>> {
        Ok(detect_ssed_package(root)?.map(|package| package.detected))
    }

    fn open(&self, root: &Path) -> Result<Box<dyn BookPackage>> {
        let detected = detect_ssed_package(root)?
            .ok_or_else(|| Error::Driver("not an SSED package".to_owned()))?;
        let detection = detected.detected;
        let catalog = detected.catalog;
        self.open_with_catalog(detection, catalog)
    }

    fn open_detected(&self, detected: DetectedPackage) -> Result<Box<dyn BookPackage>> {
        let catalog = ssed_catalog_for_root(&detected.root)?;
        self.open_with_catalog(detected, catalog)
    }
}

impl SsedDriver {
    fn open_with_catalog(
        &self,
        detection: DetectedPackage,
        catalog: SsedCatalog,
    ) -> Result<Box<dyn BookPackage>> {
        let package_root = detection.root.clone();
        let capabilities = ssed_capabilities(&catalog, &package_root);
        let search_modes = ssed_search_modes(&catalog, &package_root);
        Ok(Box::new(ReaderBookPackage::new(
            &package_root,
            detection,
            capabilities,
            PackageStores {
                ssed_catalog: Some(catalog),
                gaiji_unicode_map: load_package_uni_gaiji_maps(&package_root),
                search_modes,
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
            if store.android_info.is_some() {
                evidence.push("android_dictinfo".to_owned());
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
        if store.android_info.is_some() {
            evidence.push("android_dictinfo".to_owned());
        }
        self.open_with_store(
            DetectedPackage {
                root: package_root.clone(),
                format_family: FormatFamily::LvedSqlite3,
                confidence: 98,
                title: None,
                evidence,
            },
            store,
        )
    }

    fn open_detected(&self, detected: DetectedPackage) -> Result<Box<dyn BookPackage>> {
        let store = LvedSqliteStore::discover(&detected.root)?
            .ok_or_else(|| Error::Driver("not an LVED_SQLITE3 package".to_owned()))?;
        self.open_with_store(detected, store)
    }
}

impl LvedSqliteDriver {
    fn open_with_store(
        &self,
        mut detection: DetectedPackage,
        store: LvedSqliteStore,
    ) -> Result<Box<dyn BookPackage>> {
        let package_root = detection.root.clone();
        let summary = store.summary()?;
        let search_modes = store.search_modes()?;
        detection.title = summary
            .title
            .clone()
            .or_else(|| inferred_folder_title(&package_root));
        Ok(Box::new(ReaderBookPackage::new(
            &package_root,
            detection,
            lved_capabilities(&search_modes),
            PackageStores {
                lved_store: Some(store),
                lved_summary: Some(summary),
                search_modes,
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
        self.open_detected(detection)
    }

    fn open_detected(&self, detection: DetectedPackage) -> Result<Box<dyn BookPackage>> {
        let package_root = detection.root.clone();
        let store = MultiviewStore::discover(&package_root)?;
        Ok(Box::new(ReaderBookPackage::new(
            &package_root,
            detection,
            multiview_capabilities(),
            PackageStores {
                multiview_store: store,
                search_modes: standard_search_modes(),
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
        self.open_detected(detection)
    }

    fn open_detected(&self, detection: DetectedPackage) -> Result<Box<dyn BookPackage>> {
        let package_root = detection.root.clone();
        let store = HoureiStore::discover(&package_root)?;
        Ok(Box::new(ReaderBookPackage::new(
            &package_root,
            detection,
            hourei_capabilities(),
            PackageStores {
                hourei_store: store,
                search_modes: standard_search_modes(),
                ..Default::default()
            },
        )))
    }
}

pub struct ReaderBookPackage {
    root: PathBuf,
    storage: DirectoryStorage,
    metadata: BookMetadata,
    routing_aliases: Vec<BookAlias>,
    ssed_catalog: Option<SsedCatalog>,
    lved_store: Option<LvedSqliteStore>,
    lved_summary: Option<LvedSqliteSummary>,
    multiview_store: Option<MultiviewStore>,
    hourei_store: Option<HoureiStore>,
    gaiji_unicode_map: BTreeMap<String, String>,
    ssed_sidecar_body_resolvers:
        OnceLock<std::result::Result<Vec<SsedSidecarBodyResolver>, String>>,
    ssed_pdfspread_database: OnceLock<std::result::Result<Option<PathBuf>, String>>,
    ssed_sounddata_index: OnceLock<std::result::Result<Option<SoundDataIndex>, String>>,
}

#[derive(Debug, Default)]
pub struct PackageStores {
    pub ssed_catalog: Option<SsedCatalog>,
    pub lved_store: Option<LvedSqliteStore>,
    pub lved_summary: Option<LvedSqliteSummary>,
    pub multiview_store: Option<MultiviewStore>,
    pub hourei_store: Option<HoureiStore>,
    pub search_modes: Vec<SearchMode>,
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
const SSED_ENTRY_MARKER: [u8; 4] = [0x1f, 0x09, 0x00, 0x01];
const MONOSCR_WIDTH: u32 = 64;
const MONOSCR_HEIGHT: u32 = 64;
const MONOSCR_BITMAP_BYTES: usize = (MONOSCR_WIDTH as usize * MONOSCR_HEIGHT as usize) / 8;

#[derive(Debug, Clone)]
struct SsedFulltextRow {
    offset: u64,
    row: SsedIndexRow,
}

#[derive(Debug, Default)]
struct SsedNearKeyScanResult {
    scanned_components: usize,
    needs_linear_fallback: bool,
    diagnostics: Vec<Diagnostic>,
}

struct SsedIndexSearchCollector<'a> {
    package: &'a ReaderBookPackage,
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
        package: &'a ReaderBookPackage,
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
        let key = normalize_search_match_text(&row.key);
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

impl ReaderBookPackage {
    pub fn new(
        root: &Path,
        detected: DetectedPackage,
        capabilities: Vec<Capability>,
        stores: PackageStores,
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
            root_fingerprint,
            capabilities,
            search_modes: if stores.search_modes.is_empty() {
                default_search_modes_for_family(detected.format_family)
            } else {
                stores.search_modes.clone()
            },
        };
        let routing_aliases = routing_aliases_for_package(detected.format_family, &stores);
        Self {
            root: root.to_path_buf(),
            storage: DirectoryStorage::new(root),
            metadata,
            routing_aliases,
            ssed_catalog: stores.ssed_catalog,
            lved_store: stores.lved_store,
            lved_summary: stores.lved_summary,
            multiview_store: stores.multiview_store,
            hourei_store: stores.hourei_store,
            gaiji_unicode_map: stores.gaiji_unicode_map,
            ssed_sidecar_body_resolvers: OnceLock::new(),
            ssed_pdfspread_database: OnceLock::new(),
            ssed_sounddata_index: OnceLock::new(),
        }
    }
}

impl BookPackage for ReaderBookPackage {
    fn metadata(&self) -> &BookMetadata {
        &self.metadata
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn routing_aliases(&self) -> &[BookAlias] {
        &self.routing_aliases
    }
}

fn routing_aliases_for_package(
    format_family: FormatFamily,
    stores: &PackageStores,
) -> Vec<BookAlias> {
    if format_family != FormatFamily::LvedSqlite3 {
        return Vec::new();
    }
    stores
        .lved_store
        .as_ref()
        .and_then(|store| infer_lved_dict_code(&store.payload_path))
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(|value| {
            vec![BookAlias {
                kind: BookAliasKind::LvedDictCode,
                value,
            }]
        })
        .unwrap_or_default()
}

impl SearchProvider for ReaderBookPackage {
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

impl NavigationProvider for ReaderBookPackage {
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
                    let empty_diagnostic = self.ssed_navigation_empty_sentinel_diagnostic(
                        SsedComponentRole::Menu,
                        "MENU.DIC",
                    )?;
                    let is_empty = empty_diagnostic.is_some();
                    surfaces.push(HomeSurface {
                        surface_id: "menu".to_owned(),
                        kind: NavigationSurfaceKind::Menu,
                        status: if is_empty {
                            NavigationStatus::Empty
                        } else {
                            NavigationStatus::Available
                        },
                        title_html: "MENU".to_owned(),
                        title_text: "MENU".to_owned(),
                        target: if is_empty {
                            None
                        } else {
                            Some(TargetToken::new(&InternalTarget::MenuItem {
                                surface_id: "menu".to_owned(),
                                item_id: "root".to_owned(),
                            })?)
                        },
                        diagnostics: empty_diagnostic.into_iter().collect(),
                    });
                }
                if self
                    .ssed_catalog
                    .as_ref()
                    .is_some_and(|catalog| catalog.has_role(SsedComponentRole::Toc))
                {
                    let empty_diagnostic = self.ssed_navigation_empty_sentinel_diagnostic(
                        SsedComponentRole::Toc,
                        "TOC.DIC",
                    )?;
                    let is_empty = empty_diagnostic.is_some();
                    surfaces.push(HomeSurface {
                        surface_id: "toc".to_owned(),
                        kind: NavigationSurfaceKind::Toc,
                        status: if is_empty {
                            NavigationStatus::Empty
                        } else {
                            NavigationStatus::Available
                        },
                        title_html: "TOC".to_owned(),
                        title_text: "TOC".to_owned(),
                        target: if is_empty {
                            None
                        } else {
                            Some(TargetToken::new(&InternalTarget::TocItem {
                                surface_id: "toc".to_owned(),
                                item_id: "root".to_owned(),
                            })?)
                        },
                        diagnostics: empty_diagnostic.into_iter().collect(),
                    });
                }
                if self
                    .ssed_catalog
                    .as_ref()
                    .is_some_and(|catalog| catalog.has_role(SsedComponentRole::ScreenMenu))
                    || self.storage.exists(Path::new("SCRMENU.DIC"))?
                {
                    surfaces.push(HomeSurface {
                        surface_id: "screen-menu".to_owned(),
                        kind: NavigationSurfaceKind::ScreenMenu,
                        status: NavigationStatus::Available,
                        title_html: "Screen Menu".to_owned(),
                        title_text: "Screen Menu".to_owned(),
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id: "screen-menu".to_owned(),
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: vec![Diagnostic::info(
                            "ssed_screen_menu",
                            "SCRMENU.DIC exposes a bitmap-backed screen-map navigation surface",
                        )],
                    });
                }
                if self.storage.exists(Path::new("encyclop.idx"))? {
                    surfaces.push(HomeSurface {
                        surface_id: "encyclopedia".to_owned(),
                        kind: NavigationSurfaceKind::EncyclopediaIndex,
                        status: NavigationStatus::Available,
                        title_html: "Multimedia Index".to_owned(),
                        title_text: "Multimedia Index".to_owned(),
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id: "encyclopedia".to_owned(),
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: vec![Diagnostic::info(
                            "ssed_encyclopedia_index",
                            "encyclop.idx exposes an LVEDBRSR tab-indented multimedia navigation index",
                        )],
                    });
                }
                if has_britannica_whatday_files(&self.root)? {
                    surfaces.push(HomeSurface {
                        surface_id: "britannica-whatday".to_owned(),
                        kind: NavigationSurfaceKind::Info,
                        status: NavigationStatus::Available,
                        title_html: "Britannica What Happened Today".to_owned(),
                        title_text: "Britannica What Happened Today".to_owned(),
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id: "britannica-whatday".to_owned(),
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: vec![Diagnostic::info(
                            "ssed_britannica_whatday",
                            "Britannica loose whatday HTML fragments are available as info pages",
                        )],
                    });
                }
                if has_britannica_top_dat_files(&self.root)? {
                    surfaces.push(HomeSurface {
                        surface_id: "britannica-top".to_owned(),
                        kind: NavigationSurfaceKind::AuxiliaryIndex,
                        status: NavigationStatus::Available,
                        title_html: "Britannica Top Media Index".to_owned(),
                        title_text: "Britannica Top Media Index".to_owned(),
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id: "britannica-top".to_owned(),
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: vec![Diagnostic::info(
                            "ssed_britannica_top",
                            "Britannica loose top_*.dat media indexes are available",
                        )],
                    });
                }
                let aux_specs = self.ssed_aux_index_specs()?;
                let mut declared_aux_paths = BTreeSet::new();
                for spec in &aux_specs {
                    declared_aux_paths.insert(spec.info.to_ascii_lowercase());
                    let relative = Path::new(&spec.info);
                    if !path_has_extension(&spec.info, &["idx"]) {
                        continue;
                    }
                    if !self.storage.exists(relative)? {
                        continue;
                    }
                    let title = if spec.name.is_empty() {
                        spec.info.clone()
                    } else {
                        spec.name.clone()
                    };
                    let surface_id = format!("aux-index:{}", spec.index);
                    surfaces.push(HomeSurface {
                        surface_id: surface_id.clone(),
                        kind: NavigationSurfaceKind::AuxiliaryIndex,
                        status: NavigationStatus::Available,
                        title_html: escape_plain_label_html(&title),
                        title_text: title,
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id,
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: vec![Diagnostic::info(
                            "ssed_auxiliary_index",
                            "EXINFO.INI declares a tab-indented auxiliary navigation index",
                        )],
                    });
                }
                for spec in self.ssed_numeric_aux_index_specs(&declared_aux_paths)? {
                    let title = spec.info.clone();
                    let surface_id = format!("numeric-aux:{}", spec.info);
                    surfaces.push(HomeSurface {
                        surface_id: surface_id.clone(),
                        kind: NavigationSurfaceKind::AuxiliaryIndex,
                        status: NavigationStatus::Available,
                        title_html: escape_plain_label_html(&title),
                        title_text: title,
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id,
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: vec![Diagnostic::info(
                            "ssed_numeric_auxiliary_index",
                            "Numeric tab-indented auxiliary index is present without an EXINFO declaration",
                        )],
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
        if self.metadata.format_family == FormatFamily::Ssed && surface_id == "screen-menu" {
            return self.open_ssed_screen_menu_surface(surface_id);
        }
        if self.metadata.format_family == FormatFamily::Ssed && surface_id == "encyclopedia" {
            return self.open_ssed_encyclopedia_surface(surface_id);
        }
        if self.metadata.format_family == FormatFamily::Ssed && surface_id == "britannica-whatday" {
            return self.open_britannica_whatday_surface(surface_id, cursor, limit);
        }
        if self.metadata.format_family == FormatFamily::Ssed && surface_id == "britannica-top" {
            return self.open_britannica_top_surface(surface_id);
        }
        if self.metadata.format_family == FormatFamily::Ssed
            && (surface_id.starts_with("aux-index:") || surface_id.starts_with("numeric-aux:"))
        {
            return self.open_ssed_aux_index_surface(surface_id);
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

impl RendererProvider for ReaderBookPackage {
    fn render_target(
        &self,
        token: &TargetToken,
        options: &crate::render::RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let target = token.decode()?;
        let view = match target {
            InternalTarget::Unsupported { reason } => Ok(ResolvedTargetView::unsupported(
                token.clone(),
                "Unsupported target",
                Diagnostic::warning("target_unsupported", reason),
            )),
            InternalTarget::LvedCrossBook {
                link_kind,
                dict_code,
                content_id,
                ..
            } => Ok(ResolvedTargetView::unsupported(
                token.clone(),
                "Cross-dictionary LVED link",
                Diagnostic::info(
                    "lved_cross_book_deferred",
                    format!(
                        "LVED {link_kind} link to dictionary {dict_code} content {content_id} requires library-wide routing"
                    ),
                ),
            )),
            InternalTarget::LvedViewerHook { hook, value } => Ok(ResolvedTargetView::unsupported(
                token.clone(),
                "LVED viewer hook",
                Diagnostic::info(
                    "lved_viewer_hook_deferred",
                    format!("LVED viewer hook {hook} is intentionally not executed: {value}"),
                ),
            )),
            InternalTarget::Resource { resource, anchor } => {
                let decoded_resource = resource.decode()?;
                let resource_ref = self.resolve_resource(&resource)?;
                Ok(
                    if let InternalResource::PackageFile {
                        path,
                        resource_kind,
                    } = &decoded_resource
                        && (*resource_kind == ResourceKind::Html
                            || path_has_extension(path, &["html", "htm"]))
                    {
                        self.render_package_html_resource(
                            token.clone(),
                            &resource,
                            path,
                            resource_ref,
                            options,
                        )?
                    } else if let InternalResource::SsedLooseFile {
                        path,
                        resource_kind,
                        ..
                    } = &decoded_resource
                        && (*resource_kind == ResourceKind::Html
                            || path_has_extension(path, &["html", "htm", "body", "top"]))
                    {
                        self.render_ssed_loose_html_resource(
                            token.clone(),
                            &resource,
                            path,
                            resource_ref,
                            options,
                        )?
                    } else if let InternalResource::ChmFile {
                        chm_path,
                        entry_path,
                        resource_kind,
                    } = &decoded_resource
                        && (*resource_kind == ResourceKind::Html
                            || path_has_extension(entry_path, &["html", "htm"]))
                    {
                        self.render_chm_html_resource(
                            token.clone(),
                            &resource,
                            chm_path,
                            entry_path,
                            resource_ref,
                            options,
                        )?
                    } else {
                        let diagnostics = resource_ref.diagnostics.clone();
                        ResolvedTargetView {
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
                        }
                    },
                )
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
            InternalTarget::MultiviewHref { href, anchor } => {
                if anchor.is_none()
                    && let Some(view) =
                        self.view_for_multiview_navigation_target(token.clone(), &href)?
                {
                    Ok(view)
                } else {
                    let input = self.renderer_input_for_target(token)?;
                    self.view_for_renderer_input(input, options)
                }
            }
            _ => {
                let input = self.renderer_input_for_target(token)?;
                self.view_for_renderer_input(input, options)
            }
        }?;
        Ok(finalize_resolved_view(view, options))
    }
}

impl RendererInputProvider for ReaderBookPackage {
    fn renderer_input_for_target(&self, token: &TargetToken) -> Result<RendererInput> {
        let body = self.visual_body_for_target(token)?;
        self.renderer_input_from_visual_body(token.clone(), body)
    }
}

impl ResourceProvider for ReaderBookPackage {
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
                    mime_type: resource_mime_type(resource_kind, Some(&path)).map(str::to_owned),
                    diagnostics,
                })
            }
            InternalResource::SsedLooseFile {
                root_name,
                path,
                resource_kind,
            } => {
                let resolved = resolve_loose_media_file(&self.root, &root_name, &path)?;
                let mut diagnostics = Vec::new();
                let href = if resolved.is_some() {
                    Some(format!("lvcore://resource/{}", token.as_str()))
                } else {
                    diagnostics.push(Diagnostic::warning(
                        "resource_missing",
                        format!("{root_name}/{path} was not found next to the SSED package"),
                    ));
                    None
                };
                Ok(ResourceRef {
                    token: token.clone(),
                    kind: resource_kind,
                    label: Some(path.clone()),
                    href,
                    mime_type: resource_mime_type(resource_kind, Some(&path)).map(str::to_owned),
                    diagnostics,
                })
            }
            InternalResource::SsedComponentAddress {
                component,
                block,
                offset,
                resource_kind,
            } => {
                if resource_kind == ResourceKind::PcmData
                    && let Some(record) = resolve_pcmu_record(&self.root, block)?
                {
                    return Ok(ResourceRef {
                        token: token.clone(),
                        kind: resource_kind,
                        label: Some(format!("_PCM_U/{}", record.stem)),
                        href: Some(format!("lvcore://resource/{}", token.as_str())),
                        mime_type: Some("audio/mpeg".to_owned()),
                        diagnostics: Vec::new(),
                    });
                }
                let resolved = self
                    .ssed_component_by_name(&component)
                    .and_then(|component| self.resolve_readable_ssed_component_path(component).ok())
                    .flatten();
                let mut diagnostics = Vec::new();
                let href = if resolved.is_some() {
                    Some(format!("lvcore://resource/{}", token.as_str()))
                } else {
                    diagnostics.push(Diagnostic::warning(
                        "resource_missing",
                        format!("{component} was not found in the package"),
                    ));
                    None
                };
                Ok(ResourceRef {
                    token: token.clone(),
                    kind: resource_kind,
                    label: Some(format!("{component}:{block:08}:{offset:04}")),
                    href,
                    mime_type: resource_mime_type(resource_kind, Some(&component))
                        .map(str::to_owned),
                    diagnostics,
                })
            }
            InternalResource::SsedFigure {
                component,
                block,
                offset,
                width,
                height,
            } => {
                let resolved = self
                    .ssed_component_by_name(&component)
                    .and_then(|component| self.resolve_readable_ssed_component_path(component).ok())
                    .flatten();
                let mut diagnostics = Vec::new();
                let href = if resolved.is_some() && FigureDimensions::new(width, height).is_ok() {
                    Some(format!("lvcore://resource/{}", token.as_str()))
                } else {
                    diagnostics.push(Diagnostic::warning(
                        "resource_missing",
                        format!(
                            "{component} figure resource was not found or has invalid dimensions"
                        ),
                    ));
                    None
                };
                Ok(ResourceRef {
                    token: token.clone(),
                    kind: ResourceKind::Image,
                    label: Some(format!(
                        "{component}:{block:08}:{offset:04}:{width}x{height}"
                    )),
                    href,
                    mime_type: Some("image/png".to_owned()),
                    diagnostics,
                })
            }
            InternalResource::SsedPcmDataRange {
                component,
                start_block,
                start_offset,
                end_block,
                end_offset,
            } => {
                if let Some(record) = resolve_pcmu_record(&self.root, start_block)? {
                    return Ok(ResourceRef {
                        token: token.clone(),
                        kind: ResourceKind::PcmData,
                        label: Some(format!("_PCM_U/{}", record.stem)),
                        href: Some(format!("lvcore://resource/{}", token.as_str())),
                        mime_type: Some("audio/mpeg".to_owned()),
                        diagnostics: Vec::new(),
                    });
                }
                let resolved = self
                    .ssed_component_by_name(&component)
                    .and_then(|component| self.resolve_readable_ssed_component_path(component).ok())
                    .flatten();
                let mut diagnostics = Vec::new();
                let href = if resolved.is_some() {
                    Some(format!("lvcore://resource/{}", token.as_str()))
                } else {
                    diagnostics.push(Diagnostic::warning(
                        "resource_missing",
                        format!("{component} was not found in the package"),
                    ));
                    None
                };
                let mime_type = if href.is_some() {
                    match self.ssed_pcmdata_range_summary(
                        &component,
                        start_block,
                        start_offset,
                        end_block,
                        end_offset,
                    ) {
                        Ok(summary) => Some(summary.media_kind.mime_type().to_owned()),
                        Err(err) => {
                            diagnostics.push(Diagnostic::warning(
                                "resource_decode_deferred",
                                err.to_string(),
                            ));
                            None
                        }
                    }
                } else {
                    None
                };
                Ok(ResourceRef {
                    token: token.clone(),
                    kind: ResourceKind::PcmData,
                    label: Some(format!(
                        "{component}:{start_block:08}:{start_offset:04}-{end_block:08}:{end_offset:04}"
                    )),
                    href,
                    mime_type,
                    diagnostics,
                })
            }
            InternalResource::LooseMovie { movie_id } => {
                let resolved = find_movie_file(&self.root, &movie_id)?;
                let mut diagnostics = Vec::new();
                let href = if resolved.is_some() {
                    Some(format!("lvcore://resource/{}", token.as_str()))
                } else {
                    diagnostics.push(Diagnostic::warning(
                        "resource_missing",
                        format!("_MOVIE file {movie_id} was not found in the package"),
                    ));
                    None
                };
                Ok(ResourceRef {
                    token: token.clone(),
                    kind: ResourceKind::Video,
                    label: Some(movie_id),
                    href,
                    mime_type: Some("video/mpeg".to_owned()),
                    diagnostics,
                })
            }
            InternalResource::SsedPdfSpread { page_id } => {
                let lookup = self.lookup_pdfspread_page(&page_id)?;
                let mut diagnostics = Vec::new();
                let href = if lookup.is_some() {
                    Some(format!("lvcore://resource/{}", token.as_str()))
                } else {
                    diagnostics.push(Diagnostic::warning(
                        "resource_missing",
                        format!("PDFSpread page {page_id} was not found in the package"),
                    ));
                    None
                };
                Ok(ResourceRef {
                    token: token.clone(),
                    kind: ResourceKind::Pdf,
                    label: Some(format!("PDFSpread/{page_id}")),
                    href,
                    mime_type: Some("application/pdf".to_owned()),
                    diagnostics,
                })
            }
            InternalResource::SoundData { sound_id } => {
                let resolved = self
                    .ssed_sounddata_index()?
                    .and_then(|index| index.record(sound_id));
                let mut diagnostics = Vec::new();
                let href = if resolved.is_some() {
                    Some(format!("lvcore://resource/{}", token.as_str()))
                } else {
                    diagnostics.push(Diagnostic::warning(
                        "resource_missing",
                        format!("SoundData record {sound_id:08x} was not found in the package"),
                    ));
                    None
                };
                Ok(ResourceRef {
                    token: token.clone(),
                    kind: ResourceKind::SoundData,
                    label: Some(format!("SoundData/{sound_id:08x}")),
                    href,
                    mime_type: Some("audio/wav".to_owned()),
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
                    .or_else(|| Some(entry_path.clone()));
                Ok(ResourceRef {
                    token: token.clone(),
                    kind: resource_kind,
                    label,
                    href,
                    mime_type: resource_mime_type(resource_kind, Some(&entry_path))
                        .map(str::to_owned),
                    diagnostics,
                })
            }
            InternalResource::MediaBlob {
                key, resource_kind, ..
            } => Ok(ResourceRef {
                token: token.clone(),
                kind: resource_kind,
                label: Some(key.clone()),
                href: self
                    .lved_store
                    .is_some()
                    .then(|| format!("lvcore://resource/{}", token.as_str())),
                mime_type: resource_mime_type(resource_kind, Some(&key)).map(str::to_owned),
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
                mime_type: None,
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
            InternalResource::SsedLooseFile {
                root_name, path, ..
            } => {
                let Some(resolved) = resolve_loose_media_file(&self.root, &root_name, &path)?
                else {
                    return Err(Error::Driver(format!(
                        "loose SSED resource not found: {root_name}/{path}"
                    )));
                };
                Ok(fs::read(resolved)?)
            }
            InternalResource::SsedComponentAddress {
                component,
                block,
                offset,
                resource_kind,
            } => {
                if resource_kind == ResourceKind::Colscr {
                    return self.read_ssed_colscr_image(&component, block, offset);
                }
                if resource_kind == ResourceKind::Image
                    && self.is_ssed_monoscr_component(&component)
                {
                    return self.read_ssed_monoscr_png(&component, block, offset);
                }
                if resource_kind == ResourceKind::PcmData {
                    if let Some(bytes) = read_pcmu_record(&self.root, block)? {
                        return Ok(bytes);
                    }
                    return Err(Error::Driver(format!(
                        "_PCM_U audio for PCMDATA.DIC block {block} was not found"
                    )));
                }
                Err(Error::Driver(format!(
                    "SSED component-address resources are not readable for {resource_kind:?}"
                )))
            }
            InternalResource::SsedFigure {
                component,
                block,
                offset,
                width,
                height,
            } => self.read_ssed_figure_resource(&component, block, offset, width, height),
            InternalResource::SsedPcmDataRange {
                component,
                start_block,
                start_offset,
                end_block,
                end_offset,
            } => {
                if let Some(bytes) = read_pcmu_record(&self.root, start_block)? {
                    return Ok(bytes);
                }
                self.read_ssed_pcmdata_range(
                    &component,
                    start_block,
                    start_offset,
                    end_block,
                    end_offset,
                )
            }
            InternalResource::LooseMovie { movie_id } => {
                let Some(path) = find_movie_file(&self.root, &movie_id)? else {
                    return Err(Error::Driver(format!("_MOVIE file not found: {movie_id}")));
                };
                Ok(fs::read(path)?)
            }
            InternalResource::SsedPdfSpread { page_id } => {
                let Some(lookup) = self.lookup_pdfspread_page(&page_id)? else {
                    return Err(Error::Driver(format!(
                        "PDFSpread page not found: {page_id}"
                    )));
                };
                Ok(lookup.pdf)
            }
            InternalResource::SoundData { sound_id } => {
                let Some(index) = self.ssed_sounddata_index()? else {
                    return Err(Error::Driver("SoundData index not found".to_owned()));
                };
                let Some(bytes) = index.read_record(sound_id)? else {
                    return Err(Error::Driver(format!(
                        "SoundData record not found: {sound_id:08x}"
                    )));
                };
                Ok(bytes)
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

impl GaijiProvider for ReaderBookPackage {
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

impl SequenceProvider for ReaderBookPackage {
    fn resolve_target_window(
        &self,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<TargetWindow> {
        if self.metadata.format_family == FormatFamily::Ssed
            && sequence_hint.is_none_or(|hint| {
                matches!(
                    hint,
                    SequenceHint::TitleIndexOrder { .. } | SequenceHint::BodyOrder
                )
            })
            && let Some(window) =
                self.resolve_ssed_title_index_window(target, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::Ssed
            && matches!(sequence_hint, Some(SequenceHint::MenuOrder { .. }))
            && let Some(window) =
                self.resolve_ssed_menu_window(target, sequence_hint, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::Ssed
            && matches!(sequence_hint, Some(SequenceHint::PanelOrder { .. }))
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
            && sequence_hint.is_none_or(|hint| {
                matches!(hint, SequenceHint::LvedListOrder | SequenceHint::BodyOrder)
            })
            && let Some(window) = self.resolve_lved_list_window(target, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::LvlMultiView
            && sequence_hint.is_none_or(|hint| {
                matches!(
                    hint,
                    SequenceHint::MultiviewTreeOrder | SequenceHint::BodyOrder
                )
            })
            && let Some(window) =
                self.resolve_multiview_menu_window(target, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::Hourei
            && sequence_hint.is_none_or(|hint| {
                matches!(
                    hint,
                    SequenceHint::HoureiLawArticleOrder | SequenceHint::BodyOrder
                )
            })
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

impl BodyProvider for ReaderBookPackage {
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
                query: _,
            } => self.visual_body_for_lved_row(&table, row_id),
            InternalTarget::LvedInfoPage { name, anchor: _ } => {
                self.visual_body_for_lved_info_name(&name)
            }
            InternalTarget::LvedNamedPage {
                table,
                name,
                anchor: _,
            } => self.visual_body_for_lved_named_page(&table, &name),
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

impl ReaderBookPackage {
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
                    query: None,
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
        let needle = normalize_search_match_text(&query.query);
        let mut collector =
            SsedIndexSearchCollector::new(self, &query.mode, &needle, offset, page_limit);
        let mut optimized_scan_components = 0usize;
        let mut scan_needs_linear_fallback = false;
        if matches!(query.mode, SearchMode::Exact | SearchMode::Forward) {
            let scan_result =
                self.scan_ssed_simple_leaf_index_rows_near_key(&query.mode, &needle, |row| {
                    collector.push_row(row)
                })?;
            optimized_scan_components = scan_result.scanned_components;
            scan_needs_linear_fallback = scan_result.needs_linear_fallback;
            collector.extend_diagnostics(scan_result.diagnostics);
        }
        if !collector.has_hits() && (optimized_scan_components == 0 || scan_needs_linear_fallback) {
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
            let (code, message) = if parsed.empty_sentinel {
                (
                    "ssed_navigation_empty_sentinel",
                    format!(
                        "{} contains an explicit empty navigation sentinel",
                        component.filename
                    ),
                )
            } else {
                (
                    "ssed_navigation_empty",
                    format!("{} did not decode any navigation rows", component.filename),
                )
            };
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![
                    Diagnostic::info(code, message).with_context("component", &component.filename),
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

    fn ssed_navigation_empty_sentinel_diagnostic(
        &self,
        role: SsedComponentRole,
        fallback_name: &str,
    ) -> Result<Option<Diagnostic>> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(None);
        };
        let Some(component) = catalog
            .components_by_role(role)
            .find(|component| component.has_positive_range())
            .or_else(|| catalog.component_named(fallback_name))
        else {
            return Ok(None);
        };
        let path = match self.resolve_readable_ssed_component_path(component) {
            Ok(Some(path)) => path,
            Ok(None) | Err(_) => return Ok(None),
        };
        let mut reader = match SsedDataFile::open(&path) {
            Ok(reader) => reader,
            Err(_) => return Ok(None),
        };
        if reader.header().expanded_size() > BLOCK_SIZE as usize {
            return Ok(None);
        }
        let data = reader.read_range(0, reader.header().expanded_size())?;
        let parsed = parse_menu_stream(&data);
        if parsed.records.is_empty() && parsed.empty_sentinel {
            return Ok(Some(
                Diagnostic::info(
                    "ssed_navigation_empty_sentinel",
                    format!(
                        "{} contains an explicit empty navigation sentinel",
                        component.filename
                    ),
                )
                .with_context("component", &component.filename),
            ));
        }
        Ok(None)
    }

    fn open_ssed_screen_menu_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "ssed_catalog_missing",
                    "SSED screen-menu surfaces require a parsed SSEDINFO catalog",
                )],
            });
        };
        let Some(component) = catalog
            .components_by_role(SsedComponentRole::ScreenMenu)
            .find(|component| component.has_positive_range())
            .or_else(|| catalog.component_named("SCRMENU.DIC"))
        else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_screen_menu_missing",
                    "SCRMENU.DIC is not declared in this SSED catalog",
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
                            "ssed_screen_menu_file_missing",
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
                            "ssed_screen_menu_decode_failed",
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
        let mut reader = SsedDataFile::open(&path)?;
        let data = reader.read_range(0, reader.header().expanded_size())?;
        let parsed = parse_screen_menu_stream(&data, Some(catalog));
        if parsed.screens.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![
                    Diagnostic::info(
                        "ssed_screen_menu_empty",
                        format!(
                            "{} did not decode any screen-menu screens",
                            component.filename
                        ),
                    )
                    .with_context("component", &component.filename),
                ],
            });
        }
        let screens = self.ssed_screen_menu_screens(surface_id, &parsed)?;
        Ok(NavigationSurface::ScreenMenu {
            surface_id: surface_id.to_owned(),
            screens,
            stats: parsed.stats,
            diagnostics: Vec::new(),
        })
    }

    fn ssed_screen_menu_screens(
        &self,
        surface_id: &str,
        parsed: &SsedScreenMenuParse,
    ) -> Result<Vec<ScreenMenuScreen>> {
        parsed
            .screens
            .iter()
            .map(|screen| {
                let background = screen
                    .image
                    .as_ref()
                    .and_then(|pointer| pointer.target.as_ref().map(|target| (pointer, target)))
                    .filter(|(_, target)| target.role == SsedComponentRole::Colscr)
                    .map(|(pointer, target)| {
                        let resource =
                            ResourceToken::new(&InternalResource::SsedComponentAddress {
                                component: target.component.clone(),
                                block: pointer.block,
                                offset: pointer.offset,
                                resource_kind: ResourceKind::Colscr,
                            })?;
                        self.resolve_resource(&resource)
                    })
                    .transpose()?;
                let hotspots = screen
                    .hotspots
                    .iter()
                    .enumerate()
                    .map(|(index, hotspot)| {
                        let (target, target_kind) =
                            self.ssed_screen_menu_hotspot_target(surface_id, parsed, hotspot)?;
                        Ok(ScreenMenuHotspot {
                            hotspot_id: format!("hotspot-{index}"),
                            rect: ScreenMenuRect {
                                x: hotspot.rect.x,
                                y: hotspot.rect.y,
                                width: hotspot.rect.width,
                                height: hotspot.rect.height,
                            },
                            target,
                            target_kind,
                            diagnostics: Vec::new(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(ScreenMenuScreen {
                    screen_id: format!("screen-{}", screen.screen_index),
                    screen_index: screen.screen_index,
                    width: screen.width,
                    height: screen.height,
                    background,
                    hotspots,
                    diagnostics: Vec::new(),
                })
            })
            .collect()
    }

    fn ssed_screen_menu_hotspot_target(
        &self,
        surface_id: &str,
        parsed: &SsedScreenMenuParse,
        hotspot: &SsedScreenMenuHotspot,
    ) -> Result<(Option<TargetToken>, Option<String>)> {
        if let Some(target) = &hotspot.destination.target
            && target.role == SsedComponentRole::Honmon
        {
            return Ok((
                Some(TargetToken::new(&InternalTarget::SsedAddress {
                    component: target.component.clone(),
                    block: hotspot.destination.block,
                    offset: hotspot.destination.offset,
                })?),
                Some("body".to_owned()),
            ));
        }
        if let Some(screen_index) = hotspot.target_screen_index {
            return Ok((
                Some(TargetToken::new(&InternalTarget::MenuItem {
                    surface_id: surface_id.to_owned(),
                    item_id: format!("screen:{screen_index}"),
                })?),
                Some("screen".to_owned()),
            ));
        }
        if let (Some(screen_index), Some(direct_index)) = (
            hotspot.target_direct_screen_index,
            hotspot.target_direct_index,
        ) {
            let direct = parsed
                .screens
                .get(screen_index as usize)
                .and_then(|screen| screen.direct_targets.get(direct_index as usize));
            if let Some(direct) = direct
                && let Some(target) = &direct.destination.target
                && target.role == SsedComponentRole::Honmon
            {
                return Ok((
                    Some(TargetToken::new(&InternalTarget::SsedAddress {
                        component: target.component.clone(),
                        block: direct.destination.block,
                        offset: direct.destination.offset,
                    })?),
                    Some("body".to_owned()),
                ));
            }
        }
        Ok((None, None))
    }

    fn open_ssed_encyclopedia_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        let Some(path) = self.storage.resolve_casefolded(Path::new("encyclop.idx"))? else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_encyclopedia_index_missing",
                    "encyclop.idx is not present in this SSED package",
                )],
            });
        };
        let parsed = match parse_encyclopedia_index(&path) {
            Ok(parsed) => parsed,
            Err(error) => {
                return Ok(NavigationSurface::Deferred {
                    surface_id: surface_id.to_owned(),
                    diagnostics: vec![Diagnostic::warning(
                        "ssed_encyclopedia_index_parse_failed",
                        format!("failed to parse encyclop.idx: {error}"),
                    )],
                });
            }
        };
        let mut diagnostics = Vec::new();
        let nodes = ssed_encyclopedia_rows_to_nodes(self, &parsed.rows, &mut diagnostics)?;
        if nodes.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_encyclopedia_index_empty",
                    "encyclop.idx did not expose navigation rows",
                )],
            });
        }
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
        })
    }

    fn open_britannica_whatday_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        if limit == 0 {
            return Ok(NavigationSurface::InfoPages {
                surface_id: surface_id.to_owned(),
                pages: Vec::new(),
                next_cursor: None,
            });
        }
        let offset = decode_offset_cursor(cursor);
        let files = discover_britannica_whatday_paths(&self.root)?;
        if files.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_britannica_whatday_missing",
                    "Britannica loose whatday files were not found",
                )],
            });
        }
        let next_cursor = (files.len() > offset + limit).then(|| (offset + limit).to_string());
        let pages = files
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|file| {
                let resource = ResourceToken::new(&InternalResource::SsedLooseFile {
                    root_name: file.root_name.clone(),
                    path: file.relative_path.clone(),
                    resource_kind: ResourceKind::Html,
                })?;
                let label = format!(
                    "{}月{}日 {}",
                    file.month,
                    file.day,
                    file.fragment_kind.as_str()
                );
                Ok(NavigationItem {
                    item_id: format!(
                        "{}:{}",
                        file.root_name,
                        file.relative_path.replace('\\', "/")
                    ),
                    label_html: escape_plain_label_html(&label),
                    label_text: label,
                    target: TargetToken::new(&InternalTarget::Resource {
                        resource,
                        anchor: None,
                    })?,
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::InfoPages {
            surface_id: surface_id.to_owned(),
            pages,
            next_cursor,
        })
    }

    fn open_britannica_top_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        let dat_files = discover_britannica_top_dat_files(&self.root)?;
        if dat_files.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_britannica_top_missing",
                    "Britannica loose top_*.dat files were not found",
                )],
            });
        }
        let mut diagnostics = Vec::new();
        let mut nodes = Vec::new();
        for dat in dat_files {
            let mut children = Vec::new();
            for record in dat.records {
                let label = self.ssed_rich_label(&record.title);
                let label_html = if let Some(image) = &record.image_resource {
                    let resource = InternalResource::SsedLooseFile {
                        root_name: image.root_name.clone(),
                        path: image.relative_path.clone(),
                        resource_kind: ResourceKind::Image,
                    };
                    let token = ResourceToken::new(&resource)?;
                    format!(
                        r#"<img class="lv-britannica-top-thumb" src="lvcore://resource/{}" alt=""> {}"#,
                        token.as_str(),
                        label.html
                    )
                } else {
                    label.html
                };
                let target = self.ssed_target_for_loose_address(
                    record.address.block,
                    record.address.offset,
                    &mut diagnostics,
                )?;
                let mut node_diagnostics = label.diagnostics;
                if record.image_resource.is_none() && !record.image_name.is_empty() {
                    node_diagnostics.push(Diagnostic::info(
                        "ssed_britannica_top_image_missing",
                        format!(
                            "top_*.dat image {} was not found next to the media index",
                            record.image_name
                        ),
                    ));
                }
                children.push(NavigationNode {
                    node_id: format!("{}:{}", dat.relative_path, record.index),
                    label_html,
                    label_text: label.text,
                    target,
                    diagnostics: node_diagnostics,
                    children: Vec::new(),
                });
            }
            let category = dat.category.clone();
            nodes.push(NavigationNode {
                node_id: dat.relative_path,
                label_html: escape_plain_label_html(&category),
                label_text: category,
                target: None,
                diagnostics: Vec::new(),
                children,
            });
        }
        if nodes.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics,
            });
        }
        if !diagnostics.is_empty() {
            nodes.insert(
                0,
                NavigationNode {
                    node_id: "diagnostics".to_owned(),
                    label_html: "Diagnostics".to_owned(),
                    label_text: "Diagnostics".to_owned(),
                    target: None,
                    diagnostics,
                    children: Vec::new(),
                },
            );
        }
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
        })
    }

    fn open_ssed_aux_index_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        let spec = match self.ssed_aux_index_spec_for_surface(surface_id) {
            Ok(spec) => spec,
            Err(error) => {
                return Ok(NavigationSurface::Deferred {
                    surface_id: surface_id.to_owned(),
                    diagnostics: vec![Diagnostic::warning(
                        "ssed_auxiliary_index_invalid_surface",
                        error.to_string(),
                    )],
                });
            }
        };
        if !path_has_extension(&spec.info, &["idx"]) {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_auxiliary_index_unsupported_target",
                    format!(
                        "EXINFO auxiliary target {} is not a text IDX tree",
                        spec.info
                    ),
                )],
            });
        }
        let Some(path) = self.storage.resolve_casefolded(Path::new(&spec.info))? else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_auxiliary_index_file_missing",
                    format!("EXINFO auxiliary index {} was not found", spec.info),
                )],
            });
        };
        let rows = parse_aux_index_text_bytes(&fs::read(path)?)?;
        let mut diagnostics = Vec::new();
        let nodes = ssed_aux_index_rows_to_nodes(self, &rows, &mut diagnostics)?;
        if nodes.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_auxiliary_index_empty",
                    format!("EXINFO auxiliary index {} did not expose rows", spec.info),
                )],
            });
        }
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
        })
    }

    fn ssed_aux_index_spec_for_surface(&self, surface_id: &str) -> Result<SsedAuxIndexSpec> {
        if let Some(raw_index) = surface_id.strip_prefix("aux-index:") {
            let Ok(index) = raw_index.parse::<usize>() else {
                return Err(Error::Driver(
                    "auxiliary index surface id does not contain a numeric EXINFO index".to_owned(),
                ));
            };
            return self
                .ssed_aux_index_specs()?
                .into_iter()
                .find(|spec| spec.index == index)
                .ok_or_else(|| {
                    Error::Driver(
                        "EXINFO.INI did not declare the requested auxiliary index".to_owned(),
                    )
                });
        }
        if let Some(name) = surface_id.strip_prefix("numeric-aux:") {
            let excluded = self
                .ssed_aux_index_specs()?
                .into_iter()
                .map(|spec| spec.info.to_ascii_lowercase())
                .collect::<BTreeSet<_>>();
            return self
                .ssed_numeric_aux_index_specs(&excluded)?
                .into_iter()
                .find(|spec| spec.info.eq_ignore_ascii_case(name))
                .ok_or_else(|| {
                    Error::Driver(format!("numeric auxiliary index was not found: {name}"))
                });
        }
        Err(Error::Driver(
            "auxiliary index surface id is malformed".to_owned(),
        ))
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
                        query: None,
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
                        query: None,
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

    fn multiview_navigation_surface_for_href(
        &self,
        href: &str,
    ) -> Result<Option<(String, NavigationSurface)>> {
        let Some(store) = &self.multiview_store else {
            return Ok(None);
        };
        let Some(list) = store.law_list_for_href(href)? else {
            return Ok(None);
        };
        let title = list.title;
        let items = list
            .items
            .into_iter()
            .map(|item| {
                let target = TargetToken::new(&InternalTarget::MultiviewHref {
                    href: item.code.clone(),
                    anchor: None,
                })?;
                let label_text = if item.kana.is_empty() {
                    item.name
                } else {
                    format!("{} ({})", item.name, item.kana)
                };
                Ok(NavigationItem {
                    item_id: item.code,
                    label_html: escape_plain_label_html(&label_text),
                    label_text,
                    target,
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Some((
            title,
            NavigationSurface::TitleIndexBrowse {
                surface_id: format!("multiview:{href}"),
                items,
                next_cursor: None,
            },
        )))
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
    ) -> Result<SsedNearKeyScanResult> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(SsedNearKeyScanResult {
                scanned_components: 0,
                needs_linear_fallback: true,
                diagnostics: vec![Diagnostic::error(
                    "ssed_catalog_missing",
                    "SSED index scanning requires a parsed SSEDINFO catalog",
                )],
            });
        };
        let mut diagnostics = Vec::new();
        let mut scanned_components = 0usize;
        let mut needs_linear_fallback = false;
        let needle_key = encode_ssed_index_search_key(needle);
        if needle_key.is_empty() && !needle.is_empty() {
            return Ok(SsedNearKeyScanResult {
                scanned_components: 0,
                needs_linear_fallback: true,
                diagnostics,
            });
        }
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
            let start_page = match self.ssed_simple_index_candidate_leaf_page(
                component,
                &mut reader,
                &needle_key,
            )? {
                Some(page_index) => page_index,
                None => continue,
            };
            scanned_components = scanned_components.saturating_add(1);
            let mut last_key = None::<Vec<u8>>;
            'pages: for page_index in start_page..page_count {
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
                if rows.windows(2).any(|pair| {
                    ssed_index_row_order_key(&pair[1]) < ssed_index_row_order_key(&pair[0])
                }) {
                    needs_linear_fallback = true;
                }
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
                    let key = normalize_search_match_text(&row.key);
                    let key_bytes = ssed_index_row_order_key(&row);
                    let key_has_needle_prefix =
                        !needle_key.is_empty() && key_bytes.starts_with(&needle_key);
                    if last_key
                        .as_ref()
                        .is_some_and(|last_key| key_bytes.as_slice() < last_key.as_slice())
                    {
                        needs_linear_fallback = true;
                    }
                    last_key = Some(key_bytes.clone());
                    let row_matches = match mode {
                        SearchMode::Exact => key == needle,
                        SearchMode::Forward => key.starts_with(needle),
                        _ => false,
                    };
                    let passed_match_region = match mode {
                        SearchMode::Exact => {
                            !needs_linear_fallback && key_bytes.as_slice() > needle_key.as_slice()
                        }
                        SearchMode::Forward => {
                            !needs_linear_fallback
                                && !key_has_needle_prefix
                                && key_bytes.as_slice() > needle_key.as_slice()
                        }
                        _ => false,
                    };
                    if row_matches {
                        if !on_row(row)? {
                            break 'components;
                        }
                    } else if passed_match_region {
                        break 'pages;
                    }
                }
            }
        }
        Ok(SsedNearKeyScanResult {
            scanned_components,
            needs_linear_fallback,
            diagnostics,
        })
    }

    fn ssed_simple_index_candidate_leaf_page(
        &self,
        component: &SsedComponent,
        reader: &mut SsedDataFile,
        needle_key: &[u8],
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
                        || row.raw_key.as_slice() >= needle_key
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

    fn ssed_target_for_loose_address(
        &self,
        block: u32,
        offset: u32,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Option<TargetToken>> {
        let Some(catalog) = &self.ssed_catalog else {
            diagnostics.push(Diagnostic::error(
                "ssed_catalog_missing",
                "loose SSED address links require a parsed SSEDINFO catalog",
            ));
            return Ok(None);
        };
        let Some(component) = catalog.component_for_address(block) else {
            diagnostics.push(Diagnostic::warning(
                "ssed_loose_address_unresolved",
                format!(
                    "loose SSED address {:08x}:{:04x} is outside declared components",
                    block, offset
                ),
            ));
            return Ok(None);
        };
        if component.relative_offset(block, offset).is_none() {
            diagnostics.push(
                Diagnostic::warning(
                    "ssed_loose_address_invalid",
                    format!(
                        "{} does not contain loose address {:08x}:{:04x}",
                        component.filename, block, offset
                    ),
                )
                .with_context("component", &component.filename),
            );
            return Ok(None);
        }
        Ok(Some(TargetToken::new(&InternalTarget::SsedAddress {
            component: component.filename.clone(),
            block,
            offset,
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
        let Some(SequenceHint::MenuOrder { value: surface_id }) = sequence_hint else {
            return Ok(None);
        };
        let surface = self.open_surface(surface_id)?;
        let nodes = match surface {
            NavigationSurface::SimpleMenu { nodes, .. }
            | NavigationSurface::HierarchicalTree { nodes, .. } => nodes,
            _ => {
                return Ok(Some(TargetWindow {
                    center: self.render_target(target, options)?,
                    before: Vec::new(),
                    after: Vec::new(),
                    diagnostics: vec![Diagnostic::info(
                        "sequence_surface_not_ordered",
                        format!("{surface_id} is not an ordered SSED navigation surface"),
                    )],
                }));
            }
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
        let Some(SequenceHint::PanelOrder { value: panel_id }) = sequence_hint else {
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
            query: _,
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

        let mut center = self.render_target(target, options)?;
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
            query: None,
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
            query: _,
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
            query: item.query.clone(),
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
            } => {
                let (resources, mut diagnostics) =
                    self.ssed_stream_renderer_resources(&component, offset, length)?;
                diagnostics.insert(
                    0,
                    Diagnostic::info(
                        "hc_renderer_input_ready",
                        "SSED stream was resolved as input for an HC/profile renderer",
                    ),
                );
                Ok(RendererInput::HcSsedStream {
                    target,
                    component,
                    offset,
                    length,
                    profile_hint: self.hc_profile_hint()?,
                    resources,
                    diagnostics,
                })
            }
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

    fn ssed_stream_renderer_resources(
        &self,
        component_name: &str,
        offset: u64,
        length: Option<u64>,
    ) -> Result<(Vec<ResourceRef>, Vec<Diagnostic>)> {
        const RESOURCE_SCAN_LIMIT: usize = 256 * 1024;

        let Some(component) = self.ssed_component_by_name(component_name) else {
            return Ok((
                Vec::new(),
                vec![Diagnostic::warning(
                    "ssed_renderer_resource_scan_skipped",
                    format!("{component_name} is not declared in the SSED catalog"),
                )],
            ));
        };
        if let Err(diagnostic) = self.validate_plain_component(component) {
            return Ok((Vec::new(), vec![diagnostic]));
        }
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Ok((
                Vec::new(),
                vec![Diagnostic::warning(
                    "ssed_renderer_resource_scan_skipped",
                    format!("{} was not found in the package", component.filename),
                )],
            ));
        };

        let mut reader = SsedDataFile::open(path)?;
        let start = usize::try_from(offset)
            .map_err(|_| Error::Driver("SSED stream offset is too large".to_owned()))?;
        let available = reader.header().expanded_size().saturating_sub(start);
        let explicit_length = length.and_then(|length| usize::try_from(length).ok());
        let requested = explicit_length.unwrap_or(RESOURCE_SCAN_LIMIT);
        let size = available.min(requested).min(RESOURCE_SCAN_LIMIT);
        let data = reader.read_range(start, size)?;
        let mut candidates = self.ssed_renderer_resource_candidates(&data);
        if let Some(pdfspread) = self.ssed_pdfspread_resource_candidate(&data)? {
            candidates.push(pdfspread);
        }
        let mut seen = BTreeSet::new();
        let mut resources = Vec::new();
        let mut diagnostics = Vec::new();

        for resource in candidates {
            let token = ResourceToken::new(&resource)?;
            if !seen.insert(token.as_str().to_owned()) {
                continue;
            }
            match self.resolve_resource(&token) {
                Ok(resource_ref) => resources.push(resource_ref),
                Err(err) => diagnostics.push(Diagnostic::warning(
                    "ssed_renderer_resource_unresolved",
                    err.to_string(),
                )),
            }
        }
        if explicit_length.is_none() && available > size {
            diagnostics.push(Diagnostic::info(
                "ssed_renderer_resource_scan_bounded",
                format!(
                    "scanned {size} of {available} available SSED stream bytes for media resources"
                ),
            ));
        }
        Ok((resources, diagnostics))
    }

    fn ssed_renderer_resource_candidates(&self, data: &[u8]) -> Vec<InternalResource> {
        let mut candidates = Vec::new();
        let mut latest_figure_descriptor: Option<Vec<u8>> = None;
        let mut pos = 0usize;
        while pos + 2 <= data.len() {
            if data[pos] != 0x1f {
                pos += 1;
                continue;
            }
            let op = data[pos + 1];
            let arg_len = ssed_control_arg_length(data, pos);
            let payload = data.get(pos + 2..pos + 2 + arg_len).unwrap_or(&[]);
            match op {
                0x3c | 0x4d if payload.len() == 18 => {
                    if let Some((block, offset)) = parse_colscr_pointer(payload)
                        && let Some(component) = self.ssed_component_for_role_or_name(
                            SsedComponentRole::Colscr,
                            "COLSCR.DIC",
                        )
                    {
                        candidates.push(InternalResource::SsedComponentAddress {
                            component: component.filename.clone(),
                            block,
                            offset,
                            resource_kind: ResourceKind::Colscr,
                        });
                    }
                }
                0x44 if payload.len() == 10 => {
                    latest_figure_descriptor = Some(payload.to_vec());
                }
                0x4a if payload.len() >= 16 => {
                    if let Some((start_block, start_offset, end_block, end_offset)) =
                        parse_pcmdata_range_pointer(payload)
                    {
                        let component = self
                            .ssed_component_for_role_or_name(
                                SsedComponentRole::PcmData,
                                "PCMDATA.DIC",
                            )
                            .map(|component| component.filename.clone())
                            .unwrap_or_else(|| "PCMDATA.DIC".to_owned());
                        candidates.push(InternalResource::SsedPcmDataRange {
                            component,
                            start_block,
                            start_offset,
                            end_block,
                            end_offset,
                        });
                    }
                }
                0x64 if payload.len() == 6 => {
                    if let Some((block, offset)) = parse_packed_bcd_pointer(payload) {
                        if let Some(descriptor) = latest_figure_descriptor.as_deref()
                            && let Some(dimensions) =
                                crate::ssed_figure::parse_figure_dimensions(descriptor)
                            && let Some(component) = self.ssed_component_for_role_or_name(
                                SsedComponentRole::Figure,
                                "FIGURE.DIC",
                            )
                            && component.contains_block(block)
                        {
                            candidates.push(InternalResource::SsedFigure {
                                component: component.filename.clone(),
                                block,
                                offset,
                                width: dimensions.width,
                                height: dimensions.height,
                            });
                        } else if let Some(component) = self.ssed_component_for_role_or_name(
                            SsedComponentRole::MonoScr,
                            "MONOSCR.DIC",
                        ) && component.contains_block(block)
                        {
                            candidates.push(InternalResource::SsedComponentAddress {
                                component: component.filename.clone(),
                                block,
                                offset,
                                resource_kind: ResourceKind::Image,
                            });
                        }
                    }
                    latest_figure_descriptor = None;
                }
                _ => {}
            }
            pos += 2 + arg_len;
        }
        candidates
    }

    fn ssed_pdfspread_resource_candidate(&self, data: &[u8]) -> Result<Option<InternalResource>> {
        if self.ssed_pdfspread_database()?.is_none() {
            return Ok(None);
        }
        let text = hc03e9_pdfspread_anchor_text(data);
        let Some(page_id) = normalize_pdfspread_page_id(&text) else {
            return Ok(None);
        };
        Ok(Some(InternalResource::SsedPdfSpread { page_id }))
    }

    fn lookup_pdfspread_page(
        &self,
        page_id: &str,
    ) -> Result<Option<crate::ssed_pdfspread::PdfSpreadLookup>> {
        let Some(path) = self.ssed_pdfspread_database()? else {
            return Ok(None);
        };
        lookup_pdfspread(path, page_id)
    }

    fn ssed_pdfspread_database(&self) -> Result<Option<&PathBuf>> {
        let database = self
            .ssed_pdfspread_database
            .get_or_init(|| find_pdfspread_database(&self.root).map_err(|error| error.to_string()));
        match database {
            Ok(path) => Ok(path.as_ref()),
            Err(error) => Err(Error::Driver(error.clone())),
        }
    }

    fn ssed_sounddata_index(&self) -> Result<Option<&SoundDataIndex>> {
        let index = self
            .ssed_sounddata_index
            .get_or_init(|| load_sounddata_index(&self.root).map_err(|error| error.to_string()));
        match index {
            Ok(index) => Ok(index.as_ref()),
            Err(error) => Err(Error::Driver(error.clone())),
        }
    }

    fn ssed_component_for_role_or_name(
        &self,
        role: SsedComponentRole,
        name: &str,
    ) -> Option<&SsedComponent> {
        let catalog = self.ssed_catalog.as_ref()?;
        catalog
            .components_by_role(role)
            .next()
            .or_else(|| catalog.component_named(name))
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

    fn view_for_multiview_navigation_target(
        &self,
        target: TargetToken,
        href: &str,
    ) -> Result<Option<ResolvedTargetView>> {
        let Some((title, surface)) = self.multiview_navigation_surface_for_href(href)? else {
            return Ok(None);
        };
        Ok(Some(ResolvedTargetView {
            kind: ResolvedTargetKind::NavigationSurface,
            target,
            title: Some(title),
            display_html: None,
            basic_text: None,
            scroll_anchor: None,
            surface: Some(surface),
            resources: Vec::new(),
            links: Vec::new(),
            capabilities: Vec::new(),
            diagnostics: Vec::new(),
            debug_trace: None,
        }))
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
                resources,
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
                    resources,
                    links: Vec::new(),
                    capabilities: vec![crate::render::RenderCapability::HcRenderInput],
                    diagnostics: {
                        diagnostics.push(Diagnostic::info(
                            "hc_render_deferred",
                            "SSED stream resolved successfully; HC/profile rendering is not implemented yet",
                        ));
                        diagnostics
                    },
                    debug_trace: (options.include_debug_trace || options.mode == RenderMode::Debug)
                        .then(|| {
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

    fn render_ssed_loose_html_resource(
        &self,
        target: TargetToken,
        resource: &ResourceToken,
        path: &str,
        resource_ref: ResourceRef,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let scroll_anchor = scroll_anchor_for_token(&target)?;
        let data = self.read_resource(resource)?;
        let raw_html = decode_package_html_text(&data);
        let html = if path_has_extension(path, &["body", "top"]) {
            render_britannica_html_fragment(&raw_html)
        } else {
            raw_html
        };
        let title = resource_ref.label.clone();
        if options.mode == RenderMode::BasicText {
            return Ok(ResolvedTargetView {
                kind: ResolvedTargetKind::InfoPage,
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

        let mut normalized = self.normalize_britannica_loose_html_refs(&html)?;
        let resources = normalized.resources;
        let mut diagnostics = resource_ref.diagnostics;
        diagnostics.append(&mut normalized.diagnostics);
        Ok(ResolvedTargetView {
            kind: ResolvedTargetKind::InfoPage,
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
            InternalTarget::LvedNamedPage { .. } => Ok(ResolvedTargetKind::InfoPage),
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
            && self.ssed_pdfspread_database()?.is_none()
            && let Some(anchor_id) = self.ssed_dense_anchor_at_component_offset(
                component,
                usize::try_from(component_offset).unwrap_or(usize::MAX),
            )?
        {
            return self.visual_body_for_ssed_dense_anchor(&anchor_id, None);
        }
        let stream_offset = self.ssed_stream_start_offset(component, component_offset);
        let length = self.infer_ssed_stream_length(component, stream_offset);
        Ok(VisualBody::SsedStream {
            component: component.filename.clone(),
            offset: stream_offset,
            length,
        })
    }

    fn ssed_stream_start_offset(&self, component: &SsedComponent, component_offset: u64) -> u64 {
        if component.role != SsedComponentRole::Honmon || component_offset < 2 {
            return component_offset;
        }
        let Some(prefix_offset) = component_offset.checked_sub(2) else {
            return component_offset;
        };
        let Some(path) = self
            .resolve_readable_ssed_component_path(component)
            .ok()
            .flatten()
        else {
            return component_offset;
        };
        let Ok(mut reader) = SsedDataFile::open(path) else {
            return component_offset;
        };
        let Ok(prefix_offset_usize) = usize::try_from(prefix_offset) else {
            return component_offset;
        };
        let Ok(data) = reader.read_range(prefix_offset_usize, SSED_ENTRY_MARKER.len() + 2) else {
            return component_offset;
        };
        if data.starts_with(&[0x1f, 0x02])
            && data
                .get(2..2 + SSED_ENTRY_MARKER.len())
                .is_some_and(|marker| marker == SSED_ENTRY_MARKER)
        {
            prefix_offset
        } else {
            component_offset
        }
    }

    fn infer_ssed_stream_length(
        &self,
        component: &SsedComponent,
        component_offset: u64,
    ) -> Option<u64> {
        if component.role != SsedComponentRole::Honmon {
            return None;
        }
        let path = self
            .resolve_readable_ssed_component_path(component)
            .ok()
            .flatten()?;
        let mut reader = SsedDataFile::open(path).ok()?;
        let start = usize::try_from(component_offset).ok()?;
        if start >= reader.header().expanded_size() {
            return None;
        }
        if let Some(marker_len) = ssed_reader_generic_entry_marker_len(&mut reader, start).ok()? {
            return ssed_find_next_entry_marker_offset(
                &mut reader,
                start.saturating_add(marker_len),
            )
            .ok()
            .flatten()
            .map(|next| next.saturating_sub(start) as u64)
            .or_else(|| Some((reader.header().expanded_size() - start) as u64));
        }
        if let Some(next_offset) =
            self.infer_next_ssed_index_body_offset(component, component_offset)
            && next_offset > component_offset
        {
            return Some(next_offset - component_offset);
        }
        ssed_find_next_entry_marker_offset(&mut reader, start.saturating_add(1))
            .ok()
            .flatten()
            .filter(|next| *next > start)
            .map(|next| (next - start) as u64)
    }

    fn infer_next_ssed_index_body_offset(
        &self,
        component: &SsedComponent,
        component_offset: u64,
    ) -> Option<u64> {
        let mut next_offset: Option<u64> = None;
        self.scan_ssed_simple_index_rows(None, |row| {
            let Some(row_component) = self
                .ssed_catalog
                .as_ref()
                .and_then(|catalog| catalog.component_for_address(row.body.block))
            else {
                return Ok(true);
            };
            if !row_component
                .filename
                .eq_ignore_ascii_case(&component.filename)
            {
                return Ok(true);
            }
            let Some(row_offset) = row_component.relative_offset(row.body.block, row.body.offset)
            else {
                return Ok(true);
            };
            if row_offset > component_offset
                && next_offset.is_none_or(|current| row_offset < current)
            {
                next_offset = Some(row_offset);
            }
            Ok(true)
        })
        .ok()?;
        next_offset
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
        if let Some(anchor_id) = parse_observed_ssed_dense_anchor_id(&data) {
            return Ok(Some(anchor_id));
        }
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

    fn visual_body_for_lved_named_page(&self, table: &str, name: &str) -> Result<VisualBody> {
        let Some(store) = &self.lved_store else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 named page targets require an opened SQLCipher store",
                )],
            });
        };
        let Some(html) = store.named_html_by_name(table, name)? else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 named page was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "lved_named_page_missing",
                    format!("LVED_SQLITE3 {table} page {name} was not found"),
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
        let lower = html.to_ascii_lowercase();
        while let Some((relative_start, ref_kind)) = next_lved_ref(&lower[cursor..]) {
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
                LvedHtmlRefKind::Image => {
                    if let Some(resource) = lved_image_resource(raw_ref) {
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
                            "lved_image_ref_unparsed",
                            format!("could not parse LVED image reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::Pdf => {
                    if let Some(resource) = lved_pdf_resource(raw_ref) {
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
                            "lved_pdf_ref_unparsed",
                            format!("could not parse LVED PDF reference {raw_ref}"),
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
                LvedHtmlRefKind::CrossBook => {
                    if let Some(target) = lved_cross_book_target(raw_ref) {
                        let token = TargetToken::new(&target)?;
                        let href = format!("lvcore://target/{}", token.as_str());
                        if seen_target_tokens.insert(token.as_str().to_owned()) {
                            let mut link = TargetLink::new(raw_ref, &target)?;
                            link.diagnostics.push(Diagnostic::info(
                                "lved_cross_book_deferred",
                                "cross-dictionary LVED link requires library-wide routing",
                            ));
                            links.push(link);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "lved_cross_book_ref_unparsed",
                            format!("could not parse cross-dictionary LVED reference {raw_ref}"),
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
                LvedHtmlRefKind::Binran => {
                    if let Some(target) = lved_binran_target(raw_ref) {
                        let token = TargetToken::new(&target)?;
                        let href = format!("lvcore://target/{}", token.as_str());
                        if seen_target_tokens.insert(token.as_str().to_owned()) {
                            links.push(TargetLink::new(raw_ref, &target)?);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "lved_binran_ref_unparsed",
                            format!("could not parse LVED binran reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::ViewerHook => {
                    let target = lved_viewer_hook_target(raw_ref);
                    let token = TargetToken::new(&target)?;
                    let href = format!("lvcore://target/{}", token.as_str());
                    if seen_target_tokens.insert(token.as_str().to_owned()) {
                        let mut link = TargetLink::new(raw_ref, &target)?;
                        link.diagnostics.push(Diagnostic::info(
                            "lved_viewer_hook_deferred",
                            "LVED viewer hook is preserved as a non-executed target",
                        ));
                        links.push(link);
                    }
                    output.push_str(&href);
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
        let lower = html.to_ascii_lowercase();
        while let Some(attr) = next_html_href_or_src_attr(html, &lower, cursor) {
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
        let lower = html.to_ascii_lowercase();

        while let Some(attr) = next_html_href_or_src_attr(html, &lower, cursor) {
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
        let lower = html.to_ascii_lowercase();

        while let Some(attr) = next_html_href_or_src_attr(html, &lower, cursor) {
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

    fn normalize_britannica_loose_html_refs(&self, html: &str) -> Result<NormalizedHtmlRefs> {
        let inline = self.expand_britannica_inline_address_markers(html)?;
        let mut output = String::with_capacity(inline.html.len());
        let mut links = inline.links;
        let resources = Vec::new();
        let mut diagnostics = inline.diagnostics;
        let mut seen_target_tokens = BTreeSet::new();
        for link in &links {
            seen_target_tokens.insert(link.token.as_str().to_owned());
        }
        let mut cursor = 0usize;
        let lower = inline.html.to_ascii_lowercase();

        while let Some(attr) = next_html_href_or_src_attr(&inline.html, &lower, cursor) {
            output.push_str(&inline.html[cursor..attr.value_start]);
            let raw_value = &inline.html[attr.value_start..attr.value_end];
            if attr.name == HtmlAttrName::Href
                && let Some(address) = parse_lved_address(raw_value)
                && let Some(target) = self.ssed_target_for_loose_address(
                    address.block,
                    address.offset,
                    &mut diagnostics,
                )?
            {
                let decoded = target.decode()?;
                if seen_target_tokens.insert(target.as_str().to_owned()) {
                    links.push(TargetLink::new(raw_value, &decoded)?);
                }
                output.push_str(&format!("lvcore://target/{}", target.as_str()));
            } else {
                output.push_str(raw_value);
            }
            cursor = attr.value_end;
        }
        output.push_str(&inline.html[cursor..]);

        Ok(NormalizedHtmlRefs {
            html: output,
            resources,
            links,
            diagnostics,
        })
    }

    fn expand_britannica_inline_address_markers(&self, html: &str) -> Result<NormalizedHtmlRefs> {
        let mut output = String::with_capacity(html.len());
        let mut links = Vec::new();
        let resources = Vec::new();
        let mut diagnostics = Vec::new();
        let mut cursor = 0usize;
        let mut seen_target_tokens = BTreeSet::new();

        while let Some((marker_start, marker_kind)) = next_britannica_inline_marker(html, cursor) {
            output.push_str(&html[cursor..marker_start]);
            let spec_start = marker_start + marker_kind.start.len();
            let Some(spec) = html.get(spec_start..spec_start + 13) else {
                output.push_str(&html[marker_start..]);
                cursor = html.len();
                break;
            };
            let Some((block_hex, offset_hex)) = spec.split_once(':') else {
                output.push_str(&html[marker_start..marker_start + marker_kind.start.len()]);
                cursor = marker_start + marker_kind.start.len();
                continue;
            };
            if block_hex.len() != 8
                || offset_hex.len() != 4
                || !block_hex.bytes().all(|byte| byte.is_ascii_hexdigit())
                || !offset_hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                output.push_str(&html[marker_start..marker_start + marker_kind.start.len()]);
                cursor = marker_start + marker_kind.start.len();
                continue;
            }
            let label_start = spec_start + 13;
            let Some(end_relative) = html[label_start..].find(marker_kind.end) else {
                output.push_str(&html[marker_start..]);
                cursor = html.len();
                break;
            };
            let label_end = label_start + end_relative;
            let label = &html[label_start..label_end];
            let block = u32::from_str_radix(block_hex, 16).unwrap_or_default();
            let offset = u32::from_str_radix(offset_hex, 16).unwrap_or_default();
            if let Some(target) =
                self.ssed_target_for_loose_address(block, offset, &mut diagnostics)?
            {
                let decoded = target.decode()?;
                if seen_target_tokens.insert(target.as_str().to_owned()) {
                    links.push(TargetLink::new(label, &decoded)?);
                }
                output.push_str(&format!(
                    r#"<a class="link" href="lvcore://target/{}">{}</a>"#,
                    target.as_str(),
                    escape_plain_label_html(label)
                ));
            } else {
                output.push_str(&escape_plain_label_html(label));
            }
            cursor = label_end + marker_kind.end.len();
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
        let lower = html.to_ascii_lowercase();

        while let Some(attr) = next_html_href_or_src_attr(html, &lower, cursor) {
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
        let lower = html.to_ascii_lowercase();

        while let Some(attr) = next_html_href_or_src_attr(html, &lower, cursor) {
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

    fn ssed_component_by_name(&self, component_name: &str) -> Option<&SsedComponent> {
        self.ssed_catalog
            .as_ref()?
            .components
            .iter()
            .find(|component| {
                component.filename.eq_ignore_ascii_case(component_name)
                    || (component_name.eq_ignore_ascii_case("COLSCR.DIC")
                        && component.role == SsedComponentRole::Colscr)
            })
    }

    fn read_ssed_colscr_image(
        &self,
        component_name: &str,
        block: u32,
        offset: u32,
    ) -> Result<Vec<u8>> {
        let Some(component) = self.ssed_component_by_name(component_name) else {
            return Err(Error::Driver(format!(
                "SSED component not declared: {component_name}"
            )));
        };
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Err(Error::Driver(format!(
                "SSED component not found: {}",
                component.filename
            )));
        };
        let mut reader = SsedDataFile::open(path)?;
        if offset >= BLOCK_SIZE {
            return Err(Error::Driver(format!(
                "invalid COLSCR offset {offset}; block offsets must be less than {BLOCK_SIZE}"
            )));
        }
        let start_block = reader.header().start_block;
        if block < start_block {
            return Err(Error::Driver(format!(
                "COLSCR block {block} is before component start block {start_block}"
            )));
        }
        let relative_offset =
            (block - start_block) as usize * BLOCK_SIZE as usize + offset as usize;
        let header = reader.read_range(relative_offset, 70)?;
        let Some(payload_size) = parse_colscr_wrapped_payload_size(&header) else {
            return Err(Error::Driver(format!(
                "COLSCR image header did not decode at {component_name}:{block:08}:{offset:04}"
            )));
        };
        let wrapped = reader.read_range(relative_offset, 8 + payload_size)?;
        if wrapped.len() != 8 + payload_size {
            return Err(Error::Driver(format!(
                "COLSCR image at {component_name}:{block:08}:{offset:04} is truncated"
            )));
        }
        Ok(wrapped[8..].to_vec())
    }

    fn is_ssed_monoscr_component(&self, component_name: &str) -> bool {
        self.ssed_component_by_name(component_name)
            .is_some_and(|component| {
                component.role == SsedComponentRole::MonoScr
                    || component.filename.eq_ignore_ascii_case("MONOSCR.DIC")
            })
    }

    fn read_ssed_monoscr_png(
        &self,
        component_name: &str,
        block: u32,
        offset: u32,
    ) -> Result<Vec<u8>> {
        let Some(component) = self.ssed_component_by_name(component_name) else {
            return Err(Error::Driver(format!(
                "SSED component not declared: {component_name}"
            )));
        };
        if component.role != SsedComponentRole::MonoScr
            && !component.filename.eq_ignore_ascii_case("MONOSCR.DIC")
        {
            return Err(Error::Driver(format!(
                "{} is not a MONOSCR component",
                component.filename
            )));
        }
        let Some(relative_offset) = component.relative_offset(block, offset) else {
            return Err(Error::Driver(format!(
                "MONOSCR address {component_name}:{block:08}:{offset:04} is outside the component range"
            )));
        };
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Err(Error::Driver(format!(
                "SSED component not found: {}",
                component.filename
            )));
        };
        let mut reader = SsedDataFile::open(path)?;
        let bitmap = reader.read_range(relative_offset as usize, MONOSCR_BITMAP_BYTES)?;
        if bitmap.len() != MONOSCR_BITMAP_BYTES {
            return Err(Error::Driver(format!(
                "MONOSCR cell at {component_name}:{block:08}:{offset:04} is truncated"
            )));
        }
        encode_png_rgba(
            MONOSCR_WIDTH,
            MONOSCR_HEIGHT,
            &monoscr_bitmap_to_rgba(&bitmap),
        )
    }

    fn read_ssed_figure_resource(
        &self,
        component_name: &str,
        block: u32,
        offset: u32,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>> {
        let Some(component) = self.ssed_component_by_name(component_name) else {
            return Err(Error::Driver(format!(
                "SSED component not declared: {component_name}"
            )));
        };
        if component.role != SsedComponentRole::Figure
            && !component.filename.eq_ignore_ascii_case("FIGURE.DIC")
        {
            return Err(Error::Driver(format!(
                "{} is not a FIGURE component",
                component.filename
            )));
        }
        let dimensions = FigureDimensions::new(width, height)?;
        let Some(relative_offset) = component.relative_offset(block, offset) else {
            return Err(Error::Driver(format!(
                "FIGURE address {component_name}:{block:08}:{offset:04} is outside the component range"
            )));
        };
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Err(Error::Driver(format!(
                "SSED component not found: {}",
                component.filename
            )));
        };
        let size = dimensions.bitmap_bytes()?;
        let mut reader = SsedDataFile::open(path)?;
        let relative_offset = usize::try_from(relative_offset)
            .map_err(|_| Error::Driver("FIGURE offset is too large".to_owned()))?;
        let bitmap = reader.read_range(relative_offset, size)?;
        if bitmap.len() != size {
            return Err(Error::Driver(format!(
                "FIGURE bitmap at {component_name}:{block:08}:{offset:04} is truncated"
            )));
        }
        figure_bitmap_to_png(&bitmap, dimensions)
    }

    fn read_ssed_pcmdata_range(
        &self,
        component_name: &str,
        start_block: u32,
        start_offset: u32,
        end_block: u32,
        end_offset: u32,
    ) -> Result<Vec<u8>> {
        let (start_relative, raw, prefix) = self.read_ssed_pcmdata_raw_range(
            component_name,
            start_block,
            start_offset,
            end_block,
            end_offset,
        )?;
        let (portable, _summary) = pcmdata_portable_audio_bytes(start_relative, &raw, &prefix)?;
        Ok(portable)
    }

    fn ssed_pcmdata_range_summary(
        &self,
        component_name: &str,
        start_block: u32,
        start_offset: u32,
        end_block: u32,
        end_offset: u32,
    ) -> Result<PcmDataParseResult> {
        let (start_relative, raw, prefix) = self.read_ssed_pcmdata_raw_range(
            component_name,
            start_block,
            start_offset,
            end_block,
            end_offset,
        )?;
        pcmdata_audio_summary(start_relative, &raw, &prefix)
    }

    fn read_ssed_pcmdata_raw_range(
        &self,
        component_name: &str,
        start_block: u32,
        start_offset: u32,
        end_block: u32,
        end_offset: u32,
    ) -> Result<(usize, Vec<u8>, Vec<u8>)> {
        let Some(component) = self.ssed_component_by_name(component_name) else {
            return Err(Error::Driver(format!(
                "SSED component not declared: {component_name}"
            )));
        };
        if component.role != SsedComponentRole::PcmData
            && !component.filename.eq_ignore_ascii_case("PCMDATA.DIC")
        {
            return Err(Error::Driver(format!(
                "{} is not a PCMDATA component",
                component.filename
            )));
        }
        if start_offset >= BLOCK_SIZE || end_offset >= BLOCK_SIZE {
            return Err(Error::Driver(format!(
                "invalid PCMDATA offsets {start_offset}..{end_offset}; block offsets must be less than {BLOCK_SIZE}"
            )));
        }
        let Some(start_relative) = component.relative_offset(start_block, start_offset) else {
            return Err(Error::Driver(format!(
                "PCMDATA start address {component_name}:{start_block:08}:{start_offset:04} is outside the component range"
            )));
        };
        let Some(end_relative) = component.relative_offset(end_block, end_offset) else {
            return Err(Error::Driver(format!(
                "PCMDATA end address {component_name}:{end_block:08}:{end_offset:04} is outside the component range"
            )));
        };
        if end_relative < start_relative {
            return Err(Error::Driver(format!(
                "PCMDATA range end is before start: {component_name}:{start_block:08}:{start_offset:04}-{end_block:08}:{end_offset:04}"
            )));
        }
        let size = usize::try_from(end_relative - start_relative + 1)
            .map_err(|_| Error::Driver("PCMDATA range is too large".to_owned()))?;
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Err(Error::Driver(format!(
                "SSED component not found: {}",
                component.filename
            )));
        };
        let mut reader = SsedDataFile::open(path)?;
        let start_relative = usize::try_from(start_relative)
            .map_err(|_| Error::Driver("PCMDATA start offset is too large".to_owned()))?;
        let raw = reader.read_range(start_relative, size)?;
        if raw.len() != size {
            return Err(Error::Driver(format!(
                "PCMDATA range {component_name}:{start_block:08}:{start_offset:04}-{end_block:08}:{end_offset:04} is truncated"
            )));
        }
        let prefix = reader.read_range(0, 2048)?;
        Ok((start_relative, raw, prefix))
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
            let size_limit =
                zipped_ssed_component_size_limit(component, &member_name, member.size())?;
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
                if let Err(error) =
                    copy_zip_member_with_size_limit(&mut member, &mut outfile, size_limit)
                {
                    let _ = fs::remove_file(&tmp_path);
                    match error {
                        Error::Io(error) => {
                            if password.is_none()
                                && matches!(
                                    error.kind(),
                                    std::io::ErrorKind::Unsupported
                                        | std::io::ErrorKind::PermissionDenied
                                        | std::io::ErrorKind::InvalidData
                                )
                            {
                                continue;
                            }
                            return Err(Error::Io(error));
                        }
                        error => return Err(error),
                    }
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
        let attempts: [(&str, PrefixDecryptFn, FileDecryptFn); 3] = [
            (
                "android_honmon_diw",
                decrypt_android_diw_prefix,
                decrypt_android_diw_file_to_path,
            ),
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
        if let Some(hint) = hints.into_iter().next() {
            return Ok(Some(hint));
        }
        self.exinfo_hc_profile_hint()
    }

    fn exinfo_hc_profile_hint(&self) -> Result<Option<String>> {
        let relative = Path::new("EXINFO.INI");
        if !self.storage.exists(relative)? {
            return Ok(None);
        }
        let bytes = self.storage.read(relative)?;
        let (text, _, _) = SHIFT_JIS.decode(&bytes);
        let mut in_general = false;
        for raw_line in text.lines() {
            let line = raw_line.trim_start_matches('\u{feff}').trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                in_general = line[1..line.len() - 1]
                    .trim()
                    .eq_ignore_ascii_case("GENERAL");
                continue;
            }
            if !in_general {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim().eq_ignore_ascii_case("HTMLDLL")
                && let Some(hint) = extract_hc_profile_hint(value)
            {
                return Ok(Some(hint));
            }
        }
        Ok(None)
    }

    fn ssed_aux_index_specs(&self) -> Result<Vec<SsedAuxIndexSpec>> {
        let relative = Path::new("EXINFO.INI");
        if !self.storage.exists(relative)? {
            return Ok(Vec::new());
        }
        let bytes = self.storage.read(relative)?;
        Ok(parse_aux_index_specs_from_exinfo(&bytes))
    }

    fn ssed_numeric_aux_index_specs(
        &self,
        excluded_infos: &BTreeSet<String>,
    ) -> Result<Vec<SsedAuxIndexSpec>> {
        let mut specs = Vec::new();
        for path in self.storage.list_dir(Path::new(""))? {
            let Some(name) = path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
            else {
                continue;
            };
            if !is_numeric_aux_index_filename(&name) {
                continue;
            }
            if excluded_infos.contains(&name.to_ascii_lowercase()) {
                continue;
            }
            if file_starts_with_ssedinfo_magic(&path)? {
                continue;
            }
            let index = specs.len();
            specs.push(SsedAuxIndexSpec {
                index,
                name: name.clone(),
                info: name,
            });
        }
        Ok(specs)
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
            label: self.ssed_hanrei_package_page_label(&normalized),
            resource: InternalResource::PackageFile {
                path: normalized,
                resource_kind,
            },
            anchor: None,
            diagnostics: Vec::new(),
        });
        Ok(())
    }

    fn ssed_hanrei_package_page_label(&self, normalized: &str) -> String {
        if path_has_extension(normalized, &["html", "htm"])
            && let Ok(data) = self.storage.read(Path::new(normalized))
        {
            let html = decode_package_html_text(&data);
            if let Some(label) = html_document_label(&html) {
                return label;
            }
        }
        ssed_hanrei_page_label(normalized)
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
    let (namespace, key) = if let Some(value) = raw_ref.strip_prefix("lved.media.") {
        value.split_once(':')?
    } else if let Some(key) = raw_ref.strip_prefix("lved.media:") {
        ("media", key)
    } else if let Some(key) = raw_ref.strip_prefix("lved.sound:") {
        ("sound", key)
    } else {
        return None;
    };
    let key = lved_resource_key(key)?;
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
    let video = lower_namespace.contains("video")
        || lower_namespace.contains("movie")
        || lower_key.ends_with(".mp4")
        || lower_key.ends_with(".m4v")
        || lower_key.ends_with(".mpg")
        || lower_key.ends_with(".mpeg")
        || lower_key.ends_with(".mov");
    let resource_kind = if audio {
        ResourceKind::Audio
    } else if video {
        ResourceKind::Video
    } else if image {
        ResourceKind::Image
    } else {
        ResourceKind::MediaBlob
    };
    let store = if audio { "lved.mediasub" } else { "lved.media" };
    Some(InternalResource::MediaBlob {
        store: store.to_owned(),
        key,
        resource_kind,
    })
}

fn lved_image_resource(raw_ref: &str) -> Option<InternalResource> {
    let key = raw_ref
        .strip_prefix("lved.image:")
        .or_else(|| raw_ref.strip_prefix("lved.imag:"))
        .and_then(lved_resource_key)?;
    Some(InternalResource::MediaBlob {
        store: "lved.media".to_owned(),
        key,
        resource_kind: ResourceKind::Image,
    })
}

fn lved_pdf_resource(raw_ref: &str) -> Option<InternalResource> {
    let key = raw_ref
        .strip_prefix("lved.pdf:")
        .and_then(lved_resource_key)?;
    Some(InternalResource::MediaBlob {
        store: "lved.media".to_owned(),
        key,
        resource_kind: ResourceKind::Pdf,
    })
}

fn lved_resource_key(value: &str) -> Option<String> {
    let value = value
        .split_once('?')
        .map_or(value, |(head, _)| head)
        .split_once('#')
        .map_or(value, |(head, _)| head)
        .trim();
    (!value.is_empty()).then(|| html_unescape_minimal(value))
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
        let target = if item.data_id > 0 {
            Some(TargetToken::new(&InternalTarget::LvedRow {
                table: "content".to_owned(),
                row_id: item.data_id,
                anchor: None,
                query: item.query.clone(),
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
    package: &ReaderBookPackage,
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

fn ssed_encyclopedia_rows_to_nodes(
    package: &ReaderBookPackage,
    rows: &[SsedEncyclopediaRow],
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<NavigationNode>> {
    let mut roots = Vec::new();
    let mut path = Vec::<usize>::new();

    for (index, row) in rows.iter().enumerate() {
        let rich_label = package.ssed_rich_label(&row.label);
        let node = NavigationNode {
            node_id: format!("encyclopedia:{}:{index}", row.index),
            label_html: rich_label.html,
            label_text: rich_label.text,
            target: ssed_encyclopedia_row_target(package, row, diagnostics)?,
            diagnostics: rich_label.diagnostics,
            children: Vec::new(),
        };
        let depth = row.depth as usize;
        while path.len() > depth {
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
                "ssed_encyclopedia_tree_depth_invalid",
                format!("could not attach encyclop.idx row {index} at depth {depth}"),
            ));
            roots.push(node);
            path.clear();
            path.push(roots.len() - 1);
        }
    }

    Ok(roots)
}

fn ssed_encyclopedia_row_target(
    package: &ReaderBookPackage,
    row: &SsedEncyclopediaRow,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<TargetToken>> {
    if !row.has_target() {
        return Ok(None);
    }
    let Some(catalog) = &package.ssed_catalog else {
        diagnostics.push(Diagnostic::warning(
            "ssed_encyclopedia_catalog_missing",
            format!(
                "encyclop.idx row {} points to {:08x}:{:04x}, but no SSED catalog is available",
                row.index, row.block, row.offset
            ),
        ));
        return Ok(None);
    };
    let Some(component) = catalog.component_for_address(row.block) else {
        diagnostics.push(Diagnostic::warning(
            "ssed_encyclopedia_target_unresolved",
            format!(
                "encyclop.idx row {} points outside declared components: {:08x}:{:04x}",
                row.index, row.block, row.offset
            ),
        ));
        return Ok(None);
    };
    Ok(Some(TargetToken::new(&InternalTarget::SsedAddress {
        component: component.filename.clone(),
        block: row.block,
        offset: row.offset,
    })?))
}

fn ssed_aux_index_rows_to_nodes(
    package: &ReaderBookPackage,
    rows: &[SsedAuxIndexRow],
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<NavigationNode>> {
    let mut roots = Vec::new();
    let mut path = Vec::<usize>::new();

    for (index, row) in rows.iter().enumerate() {
        let rich_label = package.ssed_rich_label(&row.label);
        let node = NavigationNode {
            node_id: format!("aux-index:{}:{index}", row.line_number),
            label_html: rich_label.html,
            label_text: rich_label.text,
            target: ssed_aux_index_row_target(package, row, diagnostics)?,
            diagnostics: rich_label.diagnostics,
            children: Vec::new(),
        };
        let depth = row.depth.max(1) as usize;
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
                "ssed_auxiliary_index_tree_depth_invalid",
                format!("could not attach auxiliary index row {index} at depth {depth}"),
            ));
            roots.push(node);
            path.clear();
            path.push(roots.len() - 1);
        }
    }

    Ok(roots)
}

fn ssed_aux_index_row_target(
    package: &ReaderBookPackage,
    row: &SsedAuxIndexRow,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<TargetToken>> {
    if !row.has_target() {
        return Ok(None);
    }
    if let Some(selector) = row.virtual_selector() {
        diagnostics.push(Diagnostic::info(
            "ssed_auxiliary_index_virtual_selector_deferred",
            format!(
                "auxiliary index row {} points to virtual selector {selector}",
                row.line_number
            ),
        ));
        return Ok(None);
    }
    let Some(catalog) = &package.ssed_catalog else {
        diagnostics.push(Diagnostic::warning(
            "ssed_auxiliary_index_catalog_missing",
            format!(
                "auxiliary index row {} points to {:08x}:{:04x}, but no SSED catalog is available",
                row.line_number, row.block, row.offset
            ),
        ));
        return Ok(None);
    };
    let Some(component) = catalog.component_for_address(row.block) else {
        diagnostics.push(Diagnostic::warning(
            "ssed_auxiliary_index_target_unresolved",
            format!(
                "auxiliary index row {} points outside declared components: {:08x}:{:04x}",
                row.line_number, row.block, row.offset
            ),
        ));
        return Ok(None);
    };
    Ok(Some(TargetToken::new(&InternalTarget::SsedAddress {
        component: component.filename.clone(),
        block: row.block,
        offset: row.offset,
    })?))
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
    package: &ReaderBookPackage,
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
    package: &ReaderBookPackage,
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
    package: &ReaderBookPackage,
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
    package: &ReaderBookPackage,
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

pub(super) fn escape_plain_label_html(value: &str) -> String {
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

fn ssed_index_row_order_key(row: &SsedIndexRow) -> Vec<u8> {
    if row.raw_key.is_empty() {
        encode_ssed_index_search_key(&row.key.to_lowercase())
    } else {
        row.raw_key.clone()
    }
}

fn encode_ssed_index_search_key(value: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in value.chars() {
        let ch = match ch {
            ' ' => '\u{3000}',
            ch if (0x21..=0x7e).contains(&(ch as u32)) => {
                char::from_u32(ch as u32 + 0xfee0).unwrap_or(ch)
            }
            ch => ch,
        };
        let mut text = [0_u8; 4];
        let text = ch.encode_utf8(&mut text);
        let (encoded, _encoding, had_errors) = SHIFT_JIS.encode(text);
        if had_errors {
            continue;
        }
        match encoded.as_ref() {
            [single] => out.push(*single),
            [lead, trail] => {
                if let Some((first, second)) = shift_jis_pair_to_jis_key_pair(*lead, *trail) {
                    out.push(first);
                    out.push(second);
                }
            }
            _ => {}
        }
    }
    out
}

fn shift_jis_pair_to_jis_key_pair(lead: u8, trail: u8) -> Option<(u8, u8)> {
    let row = if (0x81..=0x9f).contains(&lead) {
        (lead - 0x81) * 2
    } else if (0xe0..=0xef).contains(&lead) {
        (lead - 0xc1) * 2
    } else {
        return None;
    };
    let (row, cell) = if trail >= 0x9f {
        (row + 1, trail.checked_sub(0x9f)?)
    } else if trail >= 0x80 {
        (row, trail.checked_sub(0x41)?)
    } else if trail >= 0x40 {
        (row, trail.checked_sub(0x40)?)
    } else {
        return None;
    };
    Some((row.checked_add(0x21)?, cell.checked_add(0x21)?))
}

fn looks_like_raw_anchor_label(value: &str) -> bool {
    let value = value.trim();
    value.len() >= 4 && value.chars().all(|ch| ch.is_ascii_digit())
}

fn parse_observed_ssed_dense_anchor_id(data: &[u8]) -> Option<String> {
    for marker_start in [0usize, 2] {
        if data.get(marker_start..marker_start + SSED_ENTRY_MARKER.len())
            != Some(SSED_ENTRY_MARKER.as_slice())
        {
            continue;
        }
        if data.get(marker_start + 4..marker_start + 6) != Some([0x1f, 0x41].as_slice()) {
            continue;
        }

        let styled_digits_start = marker_start + 10;
        let styled_digits_end = marker_start + 26;
        if data.get(marker_start + 8..marker_start + 10) == Some([0x1f, 0x04].as_slice())
            && data.get(styled_digits_end..styled_digits_end + 2) == Some([0x1f, 0x05].as_slice())
            && let Some(anchor) =
                parse_jis_digit_anchor_pairs(data.get(styled_digits_start..styled_digits_end)?)
        {
            return Some(anchor);
        }

        let plain_digits_start = marker_start + 6;
        let plain_digits_end = data
            .get(plain_digits_start..)?
            .windows(2)
            .position(|window| window == [0x1f, 0x61] || window == [0x1f, 0x0a])
            .map(|relative| plain_digits_start + relative)
            .unwrap_or_else(|| data.len().min(plain_digits_start + 32));
        if let Some(anchor) =
            parse_jis_digit_anchor_pairs(data.get(plain_digits_start..plain_digits_end)?)
        {
            return Some(anchor);
        }
    }
    None
}

fn parse_jis_digit_anchor_pairs(data: &[u8]) -> Option<String> {
    if !data.len().is_multiple_of(2) {
        return None;
    }
    let mut digits = String::new();
    for pair in data.chunks_exact(2) {
        match pair {
            [0x21, 0x21] => {}
            [0x23, trail] if (0x30..=0x39).contains(trail) => digits.push(char::from(*trail)),
            _ => return None,
        }
    }
    (!digits.is_empty()).then_some(digits)
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

fn ssed_reader_generic_entry_marker_len(
    reader: &mut SsedDataFile,
    offset: usize,
) -> Result<Option<usize>> {
    let data = reader.read_range(offset, SSED_ENTRY_MARKER.len() + 2)?;
    if data.starts_with(&[0x1f, 0x02])
        && data
            .get(2..2 + SSED_ENTRY_MARKER.len())
            .is_some_and(|marker| marker == SSED_ENTRY_MARKER)
    {
        return Ok(Some(SSED_ENTRY_MARKER.len() + 2));
    }
    if data.starts_with(&SSED_ENTRY_MARKER) {
        return Ok(Some(SSED_ENTRY_MARKER.len()));
    }
    Ok(None)
}

fn ssed_find_next_entry_marker_offset(
    reader: &mut SsedDataFile,
    start_offset: usize,
) -> Result<Option<usize>> {
    const SCAN_CHUNK_BYTES: usize = 64 * 1024;
    let expanded_size = reader.header().expanded_size();
    if start_offset >= expanded_size {
        return Ok(None);
    }
    let mut read_offset = start_offset;
    let mut carry = Vec::new();
    let mut carry_base = start_offset;
    let tail_size = SSED_ENTRY_MARKER.len() + 2 - 1;

    while read_offset < expanded_size {
        let read_size = expanded_size
            .saturating_sub(read_offset)
            .min(SCAN_CHUNK_BYTES);
        let chunk = reader.read_range(read_offset, read_size)?;
        if chunk.is_empty() {
            break;
        }
        let base = if carry.is_empty() {
            read_offset
        } else {
            carry_base
        };
        let mut buffer = Vec::with_capacity(carry.len() + chunk.len());
        buffer.extend_from_slice(&carry);
        buffer.extend_from_slice(&chunk);

        let mut search_from = 0usize;
        while let Some(found) = find_bytes(&buffer[search_from..], &SSED_ENTRY_MARKER) {
            let marker_position = search_from + found;
            let absolute = base + marker_position;
            let start = if marker_position >= 2
                && buffer[marker_position - 2..marker_position] == [0x1f, 0x02]
            {
                absolute.saturating_sub(2)
            } else {
                absolute
            };
            if start >= start_offset {
                return Ok(Some(start));
            }
            search_from = marker_position.saturating_add(1);
        }

        let retained = tail_size.min(buffer.len());
        carry = buffer[buffer.len() - retained..].to_vec();
        carry_base = base + buffer.len() - retained;
        read_offset = read_offset.saturating_add(chunk.len());
    }
    Ok(None)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
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

fn ssed_control_arg_length(data: &[u8], offset: usize) -> usize {
    if offset + 1 >= data.len() || data[offset] != 0x1f {
        return 0;
    }
    let op = data[offset + 1];
    match op {
        0x09 | 0x14 | 0x1a | 0x1c | 0x41 | 0x4c | 0xe0 | 0xe2 | 0xe4 | 0xe6 => 2,
        0x15 | 0x42 | 0x43 | 0x59 | 0x69 => 0,
        0x36 => 12,
        0x37 | 0x44 | 0x48 | 0x49 => 10,
        0x39 | 0x3c | 0x4d => 18,
        0x4a => match be16_at(data, offset + 2).map(|word| word & 0x000f) {
            Some(0) => 14,
            Some(1 | 2) => 16,
            Some(_) => 2,
            None => 16,
        },
        0x4b | 0x62 | 0x63 | 0x64 => 6,
        0x4e => match be16_at(data, offset + 2).map(|word| word & 0x0f00) {
            Some(0) => 38,
            Some(0x0100 | 0x0200) => 40,
            Some(_) => 2,
            None => 38,
        },
        0x4f => {
            if data.get(offset + 2..offset + 4) == Some(&[0x1f, 0x6f]) {
                48
            } else {
                34
            }
        }
        _ => 0,
    }
}

fn hc03e9_pdfspread_anchor_text(data: &[u8]) -> String {
    let mut text = String::new();
    let mut offset = 0usize;
    while offset < data.len() {
        let byte = data[offset];
        if byte == 0x1f {
            offset += 2 + ssed_control_arg_length(data, offset);
            continue;
        }
        if offset + 1 < data.len()
            && (0x21..=0x7e).contains(&byte)
            && (0x21..=0x7e).contains(&data[offset + 1])
        {
            if let Some(ch) = decode_jis_pair(byte, data[offset + 1]) {
                text.push(ch);
            }
            offset += 2;
            continue;
        }
        if offset + 1 < data.len() && byte >= 0xa1 {
            offset += 2;
            continue;
        }
        offset += 1;
    }
    text
}

fn extract_hc_profile_hint(value: &str) -> Option<String> {
    let upper = value.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    for offset in 0..bytes.len().saturating_sub(5) {
        if bytes[offset] != b'H' || bytes[offset + 1] != b'C' {
            continue;
        }
        let code = &upper[offset + 2..offset + 6];
        if code.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Some(format!("HC{code}"));
        }
    }
    None
}

fn parse_colscr_pointer(payload: &[u8]) -> Option<(u32, u32)> {
    if payload.len() != 18 {
        return None;
    }
    Some((
        decode_bcd_decimal(&payload[12..16])?,
        decode_bcd_decimal(&payload[16..18])?,
    ))
}

fn parse_pcmdata_range_pointer(payload: &[u8]) -> Option<(u32, u32, u32, u32)> {
    if payload.len() < 16 {
        return None;
    }
    Some((
        decode_bcd_decimal(&payload[4..8])?,
        decode_bcd_decimal(&payload[8..10])?,
        decode_bcd_decimal(&payload[10..14])?,
        decode_bcd_decimal(&payload[14..16])?,
    ))
}

fn parse_packed_bcd_pointer(payload: &[u8]) -> Option<(u32, u32)> {
    if payload.len() < 6 {
        return None;
    }
    Some((
        decode_bcd_decimal(&payload[..4])?,
        decode_bcd_decimal(&payload[4..6])?,
    ))
}

fn decode_bcd_decimal(data: &[u8]) -> Option<u32> {
    let mut value = 0_u32;
    for byte in data {
        let high = byte >> 4;
        let low = byte & 0x0f;
        if high > 9 || low > 9 {
            return None;
        }
        value = value.checked_mul(100)?;
        value = value.checked_add(u32::from(high) * 10 + u32::from(low))?;
    }
    Some(value)
}

fn be16_at(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
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

fn next_html_href_or_src_attr(html: &str, lower: &str, cursor: usize) -> Option<HtmlAttrRange> {
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

#[derive(Debug, Clone, Copy)]
struct BritannicaInlineMarker {
    start: &'static str,
    end: &'static str,
}

fn next_britannica_inline_marker(
    html: &str,
    cursor: usize,
) -> Option<(usize, BritannicaInlineMarker)> {
    const MARKERS: [BritannicaInlineMarker; 2] = [
        BritannicaInlineMarker {
            start: "##S",
            end: "E##",
        },
        BritannicaInlineMarker {
            start: "＃＃Ｓ",
            end: "Ｅ＃＃",
        },
    ];
    MARKERS
        .into_iter()
        .filter_map(|marker| {
            html[cursor..]
                .find(marker.start)
                .map(|offset| (cursor + offset, marker))
        })
        .min_by_key(|(offset, _)| *offset)
}

pub(super) fn path_has_extension(path: &str, extensions: &[&str]) -> bool {
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
    } else if lower.ends_with(".mp4")
        || lower.ends_with(".m4v")
        || lower.ends_with(".mpg")
        || lower.ends_with(".mpeg")
        || lower.ends_with(".mov")
    {
        ResourceKind::Video
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
    } else if lower.ends_with(".pdf") {
        ResourceKind::Pdf
    } else {
        ResourceKind::Other
    }
}

fn resource_mime_type(kind: ResourceKind, path_hint: Option<&str>) -> Option<&'static str> {
    let lower = path_hint.map(str::to_ascii_lowercase).unwrap_or_default();
    let from_path = if lower.ends_with(".svg") {
        Some("image/svg+xml")
    } else if lower.ends_with(".png") {
        Some("image/png")
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some("image/jpeg")
    } else if lower.ends_with(".gif") {
        Some("image/gif")
    } else if lower.ends_with(".bmp") {
        Some("image/bmp")
    } else if lower.ends_with(".webp") {
        Some("image/webp")
    } else if lower.ends_with(".mp3") {
        Some("audio/mpeg")
    } else if lower.ends_with(".wav") {
        Some("audio/wav")
    } else if lower.ends_with(".ogg") {
        Some("audio/ogg")
    } else if lower.ends_with(".m4a") {
        Some("audio/mp4")
    } else if lower.ends_with(".mp4") || lower.ends_with(".m4v") {
        Some("video/mp4")
    } else if lower.ends_with(".mpg") || lower.ends_with(".mpeg") {
        Some("video/mpeg")
    } else if lower.ends_with(".mov") {
        Some("video/quicktime")
    } else if lower.ends_with(".css") {
        Some("text/css; charset=utf-8")
    } else if lower.ends_with(".js") {
        Some("text/javascript; charset=utf-8")
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        Some("text/html; charset=utf-8")
    } else if lower.ends_with(".pdf") {
        Some("application/pdf")
    } else if lower.ends_with(".ttf") {
        Some("font/ttf")
    } else if lower.ends_with(".otf") {
        Some("font/otf")
    } else if lower.ends_with(".woff") {
        Some("font/woff")
    } else if lower.ends_with(".woff2") {
        Some("font/woff2")
    } else {
        None
    };
    from_path.or(match kind {
        ResourceKind::Html => Some("text/html; charset=utf-8"),
        ResourceKind::Css => Some("text/css; charset=utf-8"),
        ResourceKind::Javascript => Some("text/javascript; charset=utf-8"),
        ResourceKind::Pdf => Some("application/pdf"),
        ResourceKind::Image => Some("image/png"),
        ResourceKind::Colscr => Some("image/bmp"),
        ResourceKind::PcmData => Some("audio/wav"),
        ResourceKind::SoundData => Some("audio/wav"),
        ResourceKind::Video => Some("video/mpeg"),
        _ => None,
    })
}

fn parse_colscr_wrapped_payload_size(data: &[u8]) -> Option<usize> {
    if data.len() < 12 || &data[..4] != b"data" {
        return None;
    }
    let payload_size = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
    if payload_size == 0 {
        return None;
    }
    let image = &data[8..];
    if image.starts_with(b"BM")
        || image.starts_with(b"\xff\xd8\xff")
        || image.starts_with(b"\x89PNG\r\n\x1a\n")
    {
        return Some(payload_size);
    }
    None
}

fn monoscr_bitmap_to_rgba(bitmap: &[u8]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(MONOSCR_WIDTH as usize * MONOSCR_HEIGHT as usize * 4);
    for byte in bitmap {
        for bit in 0..8 {
            if byte & (0x80 >> bit) != 0 {
                pixels.extend_from_slice(&[0, 0, 0, 255]);
            } else {
                pixels.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    pixels
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

fn finalize_resolved_view(
    mut view: ResolvedTargetView,
    options: &RenderOptions,
) -> ResolvedTargetView {
    update_visual_capabilities(&mut view);

    match options.mode {
        RenderMode::Native => {}
        RenderMode::BasicText => {
            if let Some(html) = view.display_html.take() {
                view.basic_text = Some(html_basic_text(&html));
                view.resources.clear();
                view.links.clear();
                view.capabilities.clear();
            }
        }
        RenderMode::GenericHtml => {
            if view.display_html.as_deref().is_some_and(|html| {
                html.contains("lvcore://target/") || html.contains("lvcore://resource/")
            }) {
                view.diagnostics.push(Diagnostic::info(
                    "generic_html_router_required",
                    "GenericHtml currently preserves lvcore:// links and resources; callers must provide a router or request Native/BasicText output",
                ));
            }
        }
        RenderMode::Debug => {}
    }

    if (options.include_debug_trace || options.mode == RenderMode::Debug)
        && view.debug_trace.is_none()
    {
        view.debug_trace = Some(
            json!({
                "mode": options.mode,
                "kind": view.kind,
                "target": view.target.clone(),
                "title": view.title.clone(),
                "has_display_html": view.display_html.is_some(),
                "has_basic_text": view.basic_text.is_some(),
                "resource_count": view.resources.len(),
                "link_count": view.links.len(),
                "capabilities": view.capabilities.clone(),
                "diagnostics": view.diagnostics.clone(),
            })
            .to_string(),
        );
    }

    view
}

fn update_visual_capabilities(view: &mut ResolvedTargetView) {
    if let Some(html) = view.display_html.as_deref() {
        push_render_capability_once(&mut view.capabilities, RenderCapability::Html);

        let lower = html.to_ascii_lowercase();
        if lower.contains("<script") || lower.contains(".js") {
            push_render_capability_once(&mut view.capabilities, RenderCapability::Javascript);
        }
        if lower.contains("<style") || lower.contains("stylesheet") || lower.contains(".css") {
            push_render_capability_once(&mut view.capabilities, RenderCapability::Css);
        }
        if lower.contains("mathjax")
            || lower.contains("tex-mml")
            || lower.contains("<math")
            || html.contains(r"\(")
            || html.contains(r"\[")
            || html.contains("$$")
        {
            push_render_capability_once(&mut view.capabilities, RenderCapability::MathJax);
        }
        if lower.contains("writing-mode")
            || lower.contains("vertical-rl")
            || lower.contains("tb-rl")
            || lower.contains("tategaki")
        {
            push_render_capability_once(&mut view.capabilities, RenderCapability::VerticalText);
        }
    }

    for resource in &view.resources {
        match resource.kind {
            ResourceKind::Image | ResourceKind::Template | ResourceKind::Colscr => {
                push_render_capability_once(&mut view.capabilities, RenderCapability::Images);
            }
            ResourceKind::Audio | ResourceKind::PcmData | ResourceKind::SoundData => {
                push_render_capability_once(&mut view.capabilities, RenderCapability::Audio);
            }
            ResourceKind::Video => {
                push_render_capability_once(&mut view.capabilities, RenderCapability::Video);
            }
            ResourceKind::Css => {
                push_render_capability_once(&mut view.capabilities, RenderCapability::Css);
            }
            ResourceKind::Javascript => {
                push_render_capability_once(&mut view.capabilities, RenderCapability::Javascript);
            }
            _ => {}
        }
    }
}

fn push_render_capability_once(
    capabilities: &mut Vec<RenderCapability>,
    capability: RenderCapability,
) {
    if !capabilities.contains(&capability) {
        capabilities.push(capability);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LvedHtmlRefKind {
    Media,
    Image,
    Pdf,
    DataId,
    CrossBook,
    Info,
    Binran,
    ViewerHook,
}

fn next_lved_ref(value: &str) -> Option<(usize, LvedHtmlRefKind)> {
    let patterns = [
        ("lved.media.", LvedHtmlRefKind::Media),
        ("lved.media:", LvedHtmlRefKind::Media),
        ("lved.sound:", LvedHtmlRefKind::Media),
        ("lved.image:", LvedHtmlRefKind::Image),
        ("lved.imag:", LvedHtmlRefKind::Image),
        ("lved.pdf:", LvedHtmlRefKind::Pdf),
        ("lved.dataid.dict.", LvedHtmlRefKind::CrossBook),
        ("lved.contentlink:", LvedHtmlRefKind::CrossBook),
        ("lved.dataid.result:", LvedHtmlRefKind::DataId),
        ("lved.dataid:", LvedHtmlRefKind::DataId),
        ("lved.dataid", LvedHtmlRefKind::DataId),
        ("lved.info:", LvedHtmlRefKind::Info),
        ("lved.binran:", LvedHtmlRefKind::Binran),
        ("lved.bookmark:", LvedHtmlRefKind::ViewerHook),
        ("lved.plugin:", LvedHtmlRefKind::ViewerHook),
        ("lved.sql:", LvedHtmlRefKind::ViewerHook),
        ("lved.findnum:", LvedHtmlRefKind::ViewerHook),
        ("lved.select:", LvedHtmlRefKind::ViewerHook),
        ("lved.group.", LvedHtmlRefKind::ViewerHook),
        ("lved.browser.", LvedHtmlRefKind::ViewerHook),
    ];
    let mut cursor = 0usize;
    while let Some(relative_index) = value[cursor..].find("lved") {
        let index = cursor + relative_index;
        let rest = &value[index..];
        if let Some((_, kind)) = patterns
            .iter()
            .find(|(pattern, _)| rest.starts_with(pattern))
        {
            return Some((index, *kind));
        }
        cursor = index.saturating_add("lved".len());
    }
    None
}

fn lved_dataid_target(raw_ref: &str) -> Option<InternalTarget> {
    let value = raw_ref
        .strip_prefix("lved.dataid.result:")
        .or_else(|| raw_ref.strip_prefix("lved.dataid:"))
        .or_else(|| raw_ref.strip_prefix("lved.dataid"))?;
    let value = value.strip_prefix(':').unwrap_or(value);
    if value.is_empty() || !value.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let (row_id, anchor) = split_lved_target_anchor(value);
    let row_id = row_id.parse::<i64>().ok()?;
    Some(InternalTarget::LvedRow {
        table: "content".to_owned(),
        row_id,
        anchor: (!anchor.is_empty()).then(|| anchor.to_owned()),
        query: None,
    })
}

fn lved_cross_book_target(raw_ref: &str) -> Option<InternalTarget> {
    if let Some(value) = raw_ref.strip_prefix("lved.dataid.dict.") {
        let (dict_code, target) = value.split_once(':')?;
        let (content_id, anchor) = split_lved_target_anchor(target);
        if dict_code.is_empty() || content_id.is_empty() {
            return None;
        }
        return Some(InternalTarget::LvedCrossBook {
            link_kind: "dataid-dict".to_owned(),
            dict_code: dict_code.to_owned(),
            content_id: content_id.to_owned(),
            anchor: (!anchor.is_empty()).then(|| anchor.to_owned()),
        });
    }
    if let Some(value) = raw_ref.strip_prefix("lved.contentlink:") {
        let (dict_code, target) = value.split_once('.')?;
        let (content_id, anchor) = split_lved_target_anchor(target);
        if dict_code.is_empty() || content_id.is_empty() {
            return None;
        }
        return Some(InternalTarget::LvedCrossBook {
            link_kind: "contentlink".to_owned(),
            dict_code: dict_code.to_owned(),
            content_id: content_id.to_owned(),
            anchor: (!anchor.is_empty()).then(|| anchor.to_owned()),
        });
    }
    None
}

fn lved_info_target(raw_ref: &str) -> Option<InternalTarget> {
    let value = raw_ref.strip_prefix("lved.info:")?;
    let (name, anchor) = split_lved_target_anchor(value);
    if name.is_empty() {
        return None;
    }
    Some(InternalTarget::LvedInfoPage {
        name: html_unescape_minimal(name),
        anchor: (!anchor.is_empty()).then(|| html_unescape_minimal(anchor)),
    })
}

fn lved_binran_target(raw_ref: &str) -> Option<InternalTarget> {
    let value = raw_ref.strip_prefix("lved.binran:")?;
    let (name, anchor) = split_lved_target_anchor(value);
    if name.is_empty() {
        return None;
    }
    Some(InternalTarget::LvedNamedPage {
        table: "binran".to_owned(),
        name: html_unescape_minimal(name),
        anchor: (!anchor.is_empty()).then(|| html_unescape_minimal(anchor)),
    })
}

fn lved_viewer_hook_target(raw_ref: &str) -> InternalTarget {
    let hook = raw_ref
        .strip_prefix("lved.")
        .and_then(|rest| {
            rest.split([':', '.'])
                .next()
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("unknown");
    InternalTarget::LvedViewerHook {
        hook: hook.to_owned(),
        value: html_unescape_minimal(raw_ref),
    }
}

fn split_lved_target_anchor(value: &str) -> (&str, &str) {
    let value = value.split_once('?').map_or(value, |(head, _)| head);
    value.split_once('#').unwrap_or((value, ""))
}

fn is_lved_ref_terminator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '>' | ')' | ']')
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

fn detect_ssed_package(root: &Path) -> Result<Option<DetectedSsedPackage>> {
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
            let detected = DetectedPackage {
                root: package_root.to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some(catalog.title.clone()),
                evidence: vec![
                    format!("ssedinfo:{}", display_name(&path)),
                    format!("components:{}", catalog.components.len()),
                ],
            };
            return Ok(Some(DetectedSsedPackage { detected, catalog }));
        }
    }
    Ok(None)
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
    if ssed_navigation_component_has_non_empty_surface(
        catalog,
        &storage,
        SsedComponentRole::Menu,
        "MENU.DIC",
    ) {
        capabilities.push(Capability::Menu);
    }
    if ssed_navigation_component_has_non_empty_surface(
        catalog,
        &storage,
        SsedComponentRole::Toc,
        "TOC.DIC",
    ) {
        capabilities.push(Capability::Toc);
    }
    if catalog
        .components_by_role(SsedComponentRole::ScreenMenu)
        .any(|component| {
            component.has_positive_range() && has_component_payload_casefolded(&storage, component)
        })
    {
        capabilities.push(Capability::ScreenMenu);
    }
    if has_any_casefolded(&storage, &["encyclop.idx"]) {
        capabilities.push(Capability::EncyclopediaIndex);
    }
    let has_exinfo_aux = if storage.exists(Path::new("EXINFO.INI")).unwrap_or(false) {
        storage.read(Path::new("EXINFO.INI")).is_ok_and(|exinfo| {
            parse_aux_index_specs_from_exinfo(&exinfo)
                .iter()
                .filter(|spec| path_has_extension(&spec.info, &["idx"]))
                .any(|spec| storage.exists(Path::new(&spec.info)).unwrap_or(false))
        })
    } else {
        false
    };
    if has_exinfo_aux || has_numeric_aux_index_casefolded(&storage) {
        capabilities.push(Capability::AuxiliaryIndex);
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

fn ssed_navigation_component_has_non_empty_surface(
    catalog: &SsedCatalog,
    storage: &DirectoryStorage,
    role: SsedComponentRole,
    fallback_name: &str,
) -> bool {
    let Some(component) = catalog
        .components_by_role(role)
        .find(|component| component.has_positive_range())
        .or_else(|| catalog.component_named(fallback_name))
    else {
        return false;
    };

    let mut candidates = Vec::new();
    if let Ok(Some(path)) = storage.resolve_casefolded(Path::new(&component.filename)) {
        candidates.push(path);
    }
    for alias in ssed_component_filename_aliases(component) {
        if let Ok(Some(path)) = storage.resolve_casefolded(Path::new(&alias)) {
            candidates.push(path);
        }
    }

    for path in candidates {
        let Ok(mut reader) = SsedDataFile::open(&path) else {
            continue;
        };
        if reader.header().expanded_size() > BLOCK_SIZE as usize {
            return true;
        }
        let Ok(data) = reader.read_range(0, reader.header().expanded_size()) else {
            continue;
        };
        let parsed = parse_menu_stream(&data);
        return !(parsed.records.is_empty() && parsed.empty_sentinel);
    }

    true
}

fn has_any_casefolded(storage: &DirectoryStorage, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| storage.exists(Path::new(candidate)).unwrap_or(false))
}

fn has_numeric_aux_index_casefolded(storage: &DirectoryStorage) -> bool {
    let Ok(entries) = storage.list_dir(Path::new("")) else {
        return false;
    };
    entries.into_iter().any(|path| {
        let Some(name) = path.file_name().map(|value| value.to_string_lossy()) else {
            return false;
        };
        is_numeric_aux_index_filename(&name)
            && !file_starts_with_ssedinfo_magic(&path).unwrap_or(true)
    })
}

fn file_starts_with_ssedinfo_magic(path: &Path) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut prefix = [0u8; 8];
    let size = file.read(&mut prefix)?;
    Ok(size == 8 && (&prefix == SSEDINFO_MAGIC || &prefix == ANDROID_LVEDINFO_MAGIC))
}

fn has_ssed_hanrei_casefolded(storage: &DirectoryStorage) -> bool {
    if has_any_casefolded(
        storage,
        &[
            "HANREI.chm",
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
    if has_html_file_under_casefolded(storage, "HANREI", 0) {
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

fn has_html_file_under_casefolded(
    storage: &DirectoryStorage,
    relative_dir: &str,
    depth: usize,
) -> bool {
    if depth > 8 || !storage.exists(Path::new(relative_dir)).unwrap_or(false) {
        return false;
    }
    let Ok(children) = storage.list_dir(Path::new(relative_dir)) else {
        return false;
    };
    children.into_iter().any(|child| {
        let Some(file_name) = child.file_name().map(|value| value.to_string_lossy()) else {
            return false;
        };
        if file_name.starts_with("._") {
            return false;
        }
        let candidate = format!("{relative_dir}/{file_name}");
        if child.is_dir() {
            return has_html_file_under_casefolded(storage, &candidate, depth + 1);
        }
        child.is_file() && path_has_extension(&file_name, &["html", "htm"])
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

fn lved_capabilities(search_modes: &[SearchMode]) -> Vec<Capability> {
    let mut capabilities = vec![
        Capability::TitleIndexBrowse,
        Capability::Hanrei,
        Capability::Resources,
        Capability::Gaiji,
        Capability::PreservedHtml,
        Capability::ContinuousView,
        Capability::DeferredRendering,
    ];
    if !search_modes.is_empty() {
        capabilities.push(Capability::NativeSearch);
    }
    if search_modes.contains(&SearchMode::FullText) {
        capabilities.push(Capability::FullTextSearch);
    }
    capabilities
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

fn standard_search_modes() -> Vec<SearchMode> {
    vec![
        SearchMode::Exact,
        SearchMode::Forward,
        SearchMode::Backward,
        SearchMode::Partial,
        SearchMode::FullText,
    ]
}

fn default_search_modes_for_family(format_family: FormatFamily) -> Vec<SearchMode> {
    match format_family {
        FormatFamily::LvlMultiView | FormatFamily::Hourei => standard_search_modes(),
        _ => Vec::new(),
    }
}

fn ssed_search_modes(catalog: &SsedCatalog, root: &Path) -> Vec<SearchMode> {
    if !catalog.has_role(SsedComponentRole::Index) {
        return Vec::new();
    }
    let mut modes = vec![
        SearchMode::Exact,
        SearchMode::Forward,
        SearchMode::Backward,
        SearchMode::Partial,
    ];
    let storage = DirectoryStorage::new(root.to_path_buf());
    if catalog
        .honmon()
        .is_some_and(|component| has_component_payload_casefolded(&storage, component))
    {
        modes.push(SearchMode::FullText);
    }
    modes
}

#[cfg(test)]
mod tests {
    use std::fs;

    use aes::Aes128;
    use aes::cipher::{BlockEncrypt, KeyInit};
    use rusqlite::Connection;
    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    use crate::lved_sqlite::apply_sqlcipher_key;
    use crate::target::TargetKind;

    use super::*;

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
                anchor: Some(anchor),
                query: None
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
                scope: crate::search::SearchScope::CurrentBook {
                    book_id: package.metadata().book_id.clone(),
                },
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
                anchor: Some(_),
                query: None
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
                anchor: Some(anchor),
                query: None
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
        assert!(view.capabilities.contains(&RenderCapability::Html));
        assert!(view.capabilities.contains(&RenderCapability::Images));
        assert!(view.capabilities.contains(&RenderCapability::Audio));
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
        assert_eq!(audio.mime_type.as_deref(), Some("audio/mpeg"));
        assert_eq!(
            package.read_resource(&audio.token).unwrap(),
            b"ID3\x03".to_vec()
        );
        let image = view
            .resources
            .iter()
            .find(|resource| resource.kind == ResourceKind::Image)
            .unwrap();
        assert_eq!(image.mime_type.as_deref(), Some("image/svg+xml"));
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
    fn render_modes_are_explicit_for_preserved_lved_html() {
        let dir = tempdir().unwrap();
        write_lved_search_fixture(dir.path());
        let package = LvedSqliteDriver.open(dir.path()).unwrap();
        let page = package
            .search(&SearchQuery {
                scope: crate::search::SearchScope::CurrentBook {
                    book_id: package.metadata().book_id.clone(),
                },
                mode: SearchMode::Forward,
                query: "alp".to_owned(),
                cursor: None,
                limit: 10,
            })
            .unwrap();
        let target = &page.hits[0].target;

        let basic = package
            .render_target(
                target,
                &RenderOptions {
                    mode: RenderMode::BasicText,
                    ..RenderOptions::default()
                },
            )
            .unwrap();
        assert!(basic.display_html.is_none());
        assert!(basic.basic_text.as_deref().unwrap().contains("Alpha"));
        assert!(basic.resources.is_empty());
        assert!(basic.links.is_empty());

        let generic = package
            .render_target(
                target,
                &RenderOptions {
                    mode: RenderMode::GenericHtml,
                    ..RenderOptions::default()
                },
            )
            .unwrap();
        assert!(
            generic
                .display_html
                .as_deref()
                .unwrap()
                .contains("lvcore://target/")
        );
        assert!(
            generic
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "generic_html_router_required")
        );

        let debug = package
            .render_target(
                target,
                &RenderOptions {
                    mode: RenderMode::Debug,
                    ..RenderOptions::default()
                },
            )
            .unwrap();
        let debug_trace = debug.debug_trace.as_deref().unwrap();
        assert!(debug_trace.contains(r#""mode":"debug""#));
        assert!(debug_trace.contains(r#""has_display_html":true"#));
    }

    #[test]
    fn visual_capabilities_are_derived_from_html_and_resources() {
        let target = TargetToken::new(&InternalTarget::Unsupported {
            reason: "synthetic".to_owned(),
        })
        .unwrap();
        let resource = ResourceToken::new(&InternalResource::PackageFile {
            path: "sound.mp3".to_owned(),
            resource_kind: ResourceKind::Audio,
        })
        .unwrap();
        let view = finalize_resolved_view(
            ResolvedTargetView {
                kind: ResolvedTargetKind::EntryBody,
                target,
                title: None,
                display_html: Some(
                    r#"<p>\(x+1\)</p><link rel="stylesheet" href="style.css">"#.to_owned(),
                ),
                basic_text: None,
                scroll_anchor: None,
                surface: None,
                resources: vec![ResourceRef {
                    token: resource,
                    kind: ResourceKind::Audio,
                    label: None,
                    href: None,
                    mime_type: Some("audio/mpeg".to_owned()),
                    diagnostics: Vec::new(),
                }],
                links: Vec::new(),
                capabilities: Vec::new(),
                diagnostics: Vec::new(),
                debug_trace: None,
            },
            &RenderOptions::default(),
        );

        assert!(view.capabilities.contains(&RenderCapability::Html));
        assert!(view.capabilities.contains(&RenderCapability::Css));
        assert!(view.capabilities.contains(&RenderCapability::MathJax));
        assert!(view.capabilities.contains(&RenderCapability::Audio));
    }

    #[test]
    fn lved_protocol_router_preserves_observed_non_entry_hooks() {
        let dir = tempdir().unwrap();
        let payload = dir.path().join("main.data");
        let key = "test-key";
        {
            let connection = Connection::open(&payload).unwrap();
            apply_sqlcipher_key(&connection, key).unwrap();
            connection
                .execute_batch(
                    r#"
                    create table content (id integer primary key, type integer, body text, media text);
                    create table list (id integer primary key, refid integer, type integer, anchor text, title text, titlesub text);
                    create table media (id integer primary key, name text, type integer, main blob);
                    create table binran (id integer primary key, name text, body text);
                    insert into content values (
                      200,
                      1,
                      '<article>
                        <a href="lved.dataid.result:201#detail">result</a>
                        <a href="lved.dataid202#legacy">legacy</a>
                        <a href="lved.dataid.dict.STEDABBR:300#cross">dict</a>
                        <a href="lved.contentlink:BUREI.400#note">contentlink</a>
                        <a href="lved.binran:usage.html#top">binran</a>
                        <a href="lved.bookmark:C001">bookmark</a>
                        <img src="lved.image:fig01.png">
                        <a href="lved.pdf:manual.pdf">pdf</a>
                      </article>',
                      ''
                    );
                    insert into content values (201, 1, '<article>result detail</article>', '');
                    insert into content values (202, 1, '<article>legacy detail</article>', '');
                    insert into list values (1, 200, 1, '', 'router', '');
                    insert into media values (1, 'fig01', 4, X'89504E470D0A1A0A');
                    insert into media values (2, 'manual', 6, X'255044462D312E37');
                    insert into binran values (1, 'usage.html', '<h1>Binran</h1>');
                    "#,
                )
                .unwrap();
        }
        fs::write(dir.path().join("main.key"), key).unwrap();

        let package = LvedSqliteDriver.open(dir.path()).unwrap();
        let target = TargetToken::new(&InternalTarget::LvedRow {
            table: "content".to_owned(),
            row_id: 200,
            anchor: None,
            query: None,
        })
        .unwrap();
        let view = package
            .render_target(&target, &RenderOptions::default())
            .unwrap();
        let html = view.display_html.as_deref().unwrap();

        for raw in [
            "lved.dataid.result:",
            "lved.dataid202",
            "lved.dataid.dict.",
            "lved.contentlink:",
            "lved.binran:",
            "lved.bookmark:",
            "lved.image:",
            "lved.pdf:",
        ] {
            assert!(!html.contains(raw), "{raw} leaked through normalized HTML");
        }
        assert_eq!(
            view.resources
                .iter()
                .map(|resource| resource.kind)
                .collect::<Vec<_>>(),
            vec![ResourceKind::Image, ResourceKind::Pdf]
        );
        assert_eq!(
            view.links.iter().map(|link| link.kind).collect::<Vec<_>>(),
            vec![
                TargetKind::LvedRow,
                TargetKind::LvedRow,
                TargetKind::LvedCrossBook,
                TargetKind::LvedCrossBook,
                TargetKind::LvedNamedPage,
                TargetKind::LvedViewerHook,
            ]
        );

        let binran = view
            .links
            .iter()
            .find(|link| link.kind == TargetKind::LvedNamedPage)
            .unwrap();
        let binran_view = package
            .render_target(&binran.token, &RenderOptions::default())
            .unwrap();
        assert_eq!(binran_view.kind, ResolvedTargetKind::InfoPage);
        assert_eq!(binran_view.display_html.as_deref(), Some("<h1>Binran</h1>"));

        let cross = view
            .links
            .iter()
            .find(|link| link.kind == TargetKind::LvedCrossBook)
            .unwrap();
        let cross_view = package
            .render_target(&cross.token, &RenderOptions::default())
            .unwrap();
        assert_eq!(cross_view.kind, ResolvedTargetKind::Unsupported);
        assert!(
            cross_view
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "lved_cross_book_deferred")
        );
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
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 1,
                title: None,
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            PackageStores::default(),
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
    fn ssed_screen_menu_surface_exposes_backgrounds_and_hotspot_targets() {
        let dir = tempdir().unwrap();
        let mut screen_menu = Vec::new();
        screen_menu.extend_from_slice(&[0x1f, 0x4c, 0x00, 0x00]);
        screen_menu.extend_from_slice(&screen_menu_image_control(800, 600, 200, 0));
        screen_menu.extend_from_slice(&screen_menu_hotspot_control(10, 20, 30, 40, 100, 0));
        screen_menu.extend_from_slice(&[0x1f, 0x6c]);
        fs::write(
            dir.path().join("SCRMENU.DIC"),
            fixture_sseddata_literal_chunks(&[&screen_menu], 50, 50),
        )
        .unwrap();
        let bmp = b"BMscreen";
        let mut colscr_record = Vec::new();
        colscr_record.extend_from_slice(b"data");
        colscr_record.extend_from_slice(&(bmp.len() as u32).to_le_bytes());
        colscr_record.extend_from_slice(bmp);
        fs::write(
            dir.path().join("COLSCR.DIC"),
            fixture_sseddata_literal_chunks(&[&colscr_record], 200, 200),
        )
        .unwrap();
        fs::write(
            dir.path().join("HONMON.DIC"),
            fixture_sseddata_literal_chunks(&[b"body"], 100, 100),
        )
        .unwrap();
        let catalog = SsedCatalog {
            title: "Screen".to_owned(),
            components: vec![
                SsedComponent {
                    index: 0,
                    multi: 0,
                    component_type: 0x10,
                    start_block: 50,
                    end_block: 50,
                    data: [0; 4],
                    filename: "SCRMENU.DIC".to_owned(),
                    role: SsedComponentRole::ScreenMenu,
                },
                SsedComponent {
                    index: 1,
                    multi: 0,
                    component_type: 0xd2,
                    start_block: 200,
                    end_block: 200,
                    data: [0; 4],
                    filename: "COLSCR.DIC".to_owned(),
                    role: SsedComponentRole::Colscr,
                },
                SsedComponent {
                    index: 2,
                    multi: 0,
                    component_type: 0x00,
                    start_block: 100,
                    end_block: 100,
                    data: [0; 4],
                    filename: "HONMON.DIC".to_owned(),
                    role: SsedComponentRole::Honmon,
                },
            ],
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 3,
                trailing_bytes: 0,
            },
        };
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Screen".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            PackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );

        assert!(
            package
                .metadata()
                .capabilities
                .contains(&Capability::ScreenMenu)
        );
        assert!(package.home_surfaces().unwrap().iter().any(|surface| {
            surface.kind == NavigationSurfaceKind::ScreenMenu
                && surface.status == NavigationStatus::Available
        }));
        let surface = package.open_surface("screen-menu").unwrap();
        let NavigationSurface::ScreenMenu { screens, stats, .. } = surface else {
            panic!("expected screen-menu surface");
        };
        assert_eq!(stats["screens"], 1);
        assert_eq!(screens[0].width, Some(800));
        assert_eq!(screens[0].height, Some(600));
        let background = screens[0].background.as_ref().unwrap();
        assert_eq!(background.kind, ResourceKind::Colscr);
        assert_eq!(package.read_resource(&background.token).unwrap(), bmp);
        assert!(matches!(
            screens[0].hotspots[0].target.as_ref().unwrap().decode().unwrap(),
            InternalTarget::SsedAddress {
                component,
                block: 100,
                offset: 0
            } if component == "HONMON.DIC"
        ));
    }

    #[test]
    fn ssed_encyclopedia_index_opens_as_navigation_tree() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("encyclop.idx"),
            cp932(
                "#LVEDBRSR encyclopedia#Ver.1.0 2008.01.07\t\t\n\
                 #図・写真\t\t\n\
                 00000000\t00000000\t図・写真\t\t\n\
                 00000000\t00000000\t\t動物\t\n\
                 000059f9\t000006dc\t\t\t哺乳類\n",
            ),
        )
        .unwrap();
        let catalog = SsedCatalog {
            title: "KOJIEN6".to_owned(),
            components: vec![SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0x00,
                start_block: 0x5900,
                end_block: 0x5a00,
                data: [0; 4],
                filename: "HONMON.DIC".to_owned(),
                role: SsedComponentRole::Honmon,
            }],
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 1,
                trailing_bytes: 0,
            },
        };
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("KOJIEN6".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            PackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );

        assert!(
            package
                .metadata()
                .capabilities
                .contains(&Capability::EncyclopediaIndex)
        );
        assert!(package.home_surfaces().unwrap().iter().any(|surface| {
            surface.kind == NavigationSurfaceKind::EncyclopediaIndex
                && surface.status == NavigationStatus::Available
        }));
        let surface = package.open_surface("encyclopedia").unwrap();
        let NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
            panic!("expected encyclopedia navigation tree");
        };
        assert_eq!(nodes[0].label_text, "図・写真");
        assert_eq!(nodes[0].children[0].label_text, "動物");
        let target = nodes[0].children[0].children[0]
            .target
            .as_ref()
            .unwrap()
            .decode()
            .unwrap();
        assert!(matches!(
            target,
            InternalTarget::SsedAddress {
                component,
                block: 0x59f9,
                offset: 0x06dc
            } if component == "HONMON.DIC"
        ));
    }

    #[test]
    fn ssed_exinfo_auxiliary_index_opens_as_navigation_tree() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("EXINFO.INI"),
            cp932("[GENERAL]\nIDXCOUNT=1\nIDXNAME0=分野\nIDXINFO0=0000015E.IDX\n"),
        )
        .unwrap();
        fs::write(
            dir.path().join("0000015E.IDX"),
            cp932(
                "00000000\t00000000\t大辞林 第四版\n\
                 00005221\t00000722\t\t季語\n\
                 00005221\t000007C2\t\t\t春\n",
            ),
        )
        .unwrap();
        let catalog = SsedCatalog {
            title: "DAIJIRIN".to_owned(),
            components: vec![SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0x00,
                start_block: 0x5221,
                end_block: 0x5230,
                data: [0; 4],
                filename: "HONMON.DIC".to_owned(),
                role: SsedComponentRole::Honmon,
            }],
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 1,
                trailing_bytes: 0,
            },
        };
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("DAIJIRIN".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            PackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );

        assert!(
            package
                .metadata()
                .capabilities
                .contains(&Capability::AuxiliaryIndex)
        );
        let home = package.home_surfaces().unwrap();
        assert!(home.iter().any(|surface| {
            surface.surface_id == "aux-index:0"
                && surface.kind == NavigationSurfaceKind::AuxiliaryIndex
                && surface.title_text == "分野"
        }));
        let surface = package.open_surface("aux-index:0").unwrap();
        let NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
            panic!("expected auxiliary navigation tree");
        };
        assert_eq!(nodes[0].label_text, "大辞林 第四版");
        assert_eq!(nodes[0].children[0].label_text, "季語");
        let target = nodes[0].children[0].children[0]
            .target
            .as_ref()
            .unwrap()
            .decode()
            .unwrap();
        assert!(matches!(
            target,
            InternalTarget::SsedAddress {
                component,
                block: 0x5221,
                offset: 0x07c2
            } if component == "HONMON.DIC"
        ));
        let center = nodes[0].children[0].children[0]
            .target
            .as_ref()
            .unwrap()
            .clone();
        let window = package
            .resolve_target_window(
                &center,
                Some(&SequenceHint::MenuOrder {
                    value: "aux-index:0".to_owned(),
                }),
                1,
                0,
                &RenderOptions::default(),
            )
            .unwrap();
        assert_eq!(window.before.len(), 1);
        assert_eq!(window.before[0].title.as_deref(), Some("季語"));
    }

    #[test]
    fn ssed_numeric_auxiliary_index_opens_without_exinfo() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("0000015f.idx"),
            cp932(
                "00000000\t00000000\tRoot\n\
                 00005221\t00000722\t\tChild\n",
            ),
        )
        .unwrap();
        fs::write(dir.path().join("00000001.idx"), SSEDINFO_MAGIC).unwrap();
        let catalog = SsedCatalog {
            title: "Numeric".to_owned(),
            components: vec![SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0x00,
                start_block: 0x5221,
                end_block: 0x5230,
                data: [0; 4],
                filename: "HONMON.DIC".to_owned(),
                role: SsedComponentRole::Honmon,
            }],
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 1,
                trailing_bytes: 0,
            },
        };
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Numeric".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            PackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );

        let home = package.home_surfaces().unwrap();
        assert!(
            package
                .metadata()
                .capabilities
                .contains(&Capability::AuxiliaryIndex)
        );
        assert!(home.iter().any(|surface| {
            surface.surface_id == "numeric-aux:0000015f.idx"
                && surface.kind == NavigationSurfaceKind::AuxiliaryIndex
        }));
        assert!(
            !home
                .iter()
                .any(|surface| surface.surface_id == "numeric-aux:00000001.idx")
        );

        let surface = package.open_surface("numeric-aux:0000015f.idx").unwrap();
        let NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
            panic!("expected numeric auxiliary navigation tree");
        };
        let target = nodes[0].children[0]
            .target
            .as_ref()
            .unwrap()
            .decode()
            .unwrap();
        assert!(matches!(
            target,
            InternalTarget::SsedAddress {
                component,
                block: 0x5221,
                offset: 0x0722
            } if component == "HONMON.DIC"
        ));
    }

    #[test]
    fn ssed_pcmdata_address_uses_loose_pcmu_audio_when_component_is_absent() {
        let dir = tempdir().unwrap();
        let package_root = dir.path().join("_DCT_SAMPLE");
        let pcmu_root = dir.path().join("_DCT_SAMPLE_PCM_U");
        fs::create_dir(&package_root).unwrap();
        fs::create_dir(&pcmu_root).unwrap();
        fs::write(pcmu_root.join("WaveFile.map"), b"00000001 269094\n").unwrap();
        fs::write(
            pcmu_root.join("00000001"),
            encrypt_logofont_cipher_for_test(b"ID3\x03\x00\x00sample mp3 bytes"),
        )
        .unwrap();

        let package = ReaderBookPackage::new(
            &package_root,
            DetectedPackage {
                root: package_root.clone(),
                format_family: FormatFamily::Ssed,
                confidence: 80,
                title: Some("Sample".to_owned()),
                evidence: Vec::new(),
            },
            Vec::new(),
            PackageStores::default(),
        );
        let token = ResourceToken::new(&InternalResource::SsedComponentAddress {
            component: "PCMDATA.DIC".to_owned(),
            block: 269094,
            offset: 0,
            resource_kind: ResourceKind::PcmData,
        })
        .unwrap();

        let resource = package.resolve_resource(&token).unwrap();
        assert_eq!(resource.kind, ResourceKind::PcmData);
        assert_eq!(resource.label.as_deref(), Some("_PCM_U/00000001"));
        assert_eq!(resource.mime_type.as_deref(), Some("audio/mpeg"));
        assert!(resource.href.is_some());
        assert!(resource.diagnostics.is_empty());
        assert_eq!(
            package.read_resource(&token).unwrap(),
            b"ID3\x03\x00\x00sample mp3 bytes"
        );
    }

    #[test]
    fn ssed_pcmdata_range_reads_portable_wave_audio() {
        let dir = tempdir().unwrap();
        let pcm_chunks = pcmdata_wave_chunks_for_test(1, b"\x80\x81\x82");
        fs::write(
            dir.path().join("PCMDATA.DIC"),
            fixture_sseddata_literal_chunks(&[&pcm_chunks], 500, 500),
        )
        .unwrap();
        let catalog = SsedCatalog {
            title: "Pcm".to_owned(),
            components: vec![SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0xd8,
                start_block: 500,
                end_block: 500,
                data: [0; 4],
                filename: "PCMDATA.DIC".to_owned(),
                role: SsedComponentRole::PcmData,
            }],
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 1,
                trailing_bytes: 0,
            },
        };
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 80,
                title: Some("Pcm".to_owned()),
                evidence: Vec::new(),
            },
            Vec::new(),
            PackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );
        let token = ResourceToken::new(&InternalResource::SsedPcmDataRange {
            component: "PCMDATA.DIC".to_owned(),
            start_block: 500,
            start_offset: 0,
            end_block: 500,
            end_offset: u32::try_from(pcm_chunks.len() - 1).unwrap(),
        })
        .unwrap();

        let resource = package.resolve_resource(&token).unwrap();
        assert_eq!(resource.kind, ResourceKind::PcmData);
        assert_eq!(resource.mime_type.as_deref(), Some("audio/wav"));
        assert!(resource.href.is_some());
        let audio = package.read_resource(&token).unwrap();
        assert!(audio.starts_with(b"RIFF"));
        assert!(audio.ends_with(b"\x80\x81\x82"));
    }

    #[test]
    fn monoscr_component_address_reads_png_bitmap_cell() {
        let dir = tempdir().unwrap();
        let mut bitmap = vec![0_u8; MONOSCR_BITMAP_BYTES];
        bitmap[0] = 0x80;
        fs::write(
            dir.path().join("MONOSCR.DIC"),
            fixture_sseddata_literal_chunks(&[&bitmap], 400, 400),
        )
        .unwrap();
        let catalog = SsedCatalog {
            title: "Mono".to_owned(),
            components: vec![SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0xd0,
                start_block: 400,
                end_block: 400,
                data: [0; 4],
                filename: "MONOSCR.DIC".to_owned(),
                role: SsedComponentRole::MonoScr,
            }],
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 1,
                trailing_bytes: 0,
            },
        };
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 80,
                title: Some("Mono".to_owned()),
                evidence: Vec::new(),
            },
            Vec::new(),
            PackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );
        let token = ResourceToken::new(&InternalResource::SsedComponentAddress {
            component: "MONOSCR.DIC".to_owned(),
            block: 400,
            offset: 0,
            resource_kind: ResourceKind::Image,
        })
        .unwrap();

        let resource = package.resolve_resource(&token).unwrap();
        assert_eq!(resource.kind, ResourceKind::Image);
        assert_eq!(resource.mime_type.as_deref(), Some("image/png"));
        assert!(resource.href.is_some());
        let png = package.read_resource(&token).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn figure_resource_reads_variable_bitmap_png() {
        let dir = tempdir().unwrap();
        let mut payload = vec![0_u8; 17];
        payload.extend_from_slice(&[0x80, 0x80, 0x7f, 0x00]);
        fs::write(
            dir.path().join("FIGURE.DIC"),
            fixture_sseddata_literal_chunks(&[&payload], 1200, 1200),
        )
        .unwrap();
        let catalog = SsedCatalog {
            title: "Figure".to_owned(),
            components: vec![SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0xd0,
                start_block: 1200,
                end_block: 1200,
                data: [0; 4],
                filename: "FIGURE.DIC".to_owned(),
                role: SsedComponentRole::Figure,
            }],
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 1,
                trailing_bytes: 0,
            },
        };
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 80,
                title: Some("Figure".to_owned()),
                evidence: Vec::new(),
            },
            Vec::new(),
            PackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );
        let token = ResourceToken::new(&InternalResource::SsedFigure {
            component: "FIGURE.DIC".to_owned(),
            block: 1200,
            offset: 17,
            width: 9,
            height: 2,
        })
        .unwrap();

        let resource = package.resolve_resource(&token).unwrap();
        assert_eq!(resource.kind, ResourceKind::Image);
        assert_eq!(resource.mime_type.as_deref(), Some("image/png"));
        assert_eq!(
            resource.label.as_deref(),
            Some("FIGURE.DIC:00001200:0017:9x2")
        );
        let png = package.read_resource(&token).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn ssed_hc_renderer_input_carries_stream_resource_refs() {
        let dir = tempdir().unwrap();
        let pcm_chunks = pcmdata_wave_chunks_for_test(1, b"\x80\x81\x82");
        let mut figure_payload = vec![0_u8; 17];
        figure_payload.extend_from_slice(&[0x80, 0x80, 0x7f, 0x00]);
        let mut honmon = Vec::new();
        honmon.extend_from_slice(&[
            0x1f, 0x4a, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x05, 0x00, 0x00, 0x34,
        ]);
        honmon.extend_from_slice(&[
            0x1f, 0x44, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x09,
        ]);
        honmon.extend_from_slice(&[0x1f, 0x64, 0x00, 0x00, 0x12, 0x00, 0x00, 0x17]);
        fs::write(
            dir.path().join("HONMON.DIC"),
            fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
        )
        .unwrap();
        fs::write(
            dir.path().join("PCMDATA.DIC"),
            fixture_sseddata_literal_chunks(&[&pcm_chunks], 500, 500),
        )
        .unwrap();
        fs::write(
            dir.path().join("FIGURE.DIC"),
            fixture_sseddata_literal_chunks(&[&figure_payload], 1200, 1200),
        )
        .unwrap();
        let catalog = SsedCatalog {
            title: "Renderer resources".to_owned(),
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
                    component_type: 0xd8,
                    start_block: 500,
                    end_block: 500,
                    data: [0; 4],
                    filename: "PCMDATA.DIC".to_owned(),
                    role: SsedComponentRole::PcmData,
                },
                SsedComponent {
                    index: 2,
                    multi: 0,
                    component_type: 0xd0,
                    start_block: 1200,
                    end_block: 1200,
                    data: [0; 4],
                    filename: "FIGURE.DIC".to_owned(),
                    role: SsedComponentRole::Figure,
                },
            ],
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 3,
                trailing_bytes: 0,
            },
        };
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 80,
                title: Some("Renderer resources".to_owned()),
                evidence: Vec::new(),
            },
            Vec::new(),
            PackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );
        let token = TargetToken::new(&InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 100,
            offset: 0,
        })
        .unwrap();

        let input = package.renderer_input_for_target(&token).unwrap();
        let RendererInput::HcSsedStream {
            resources,
            diagnostics,
            ..
        } = input
        else {
            panic!("SSED address should produce HC renderer input");
        };
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "hc_renderer_input_ready")
        );
        assert!(resources.iter().any(|resource| {
            resource.kind == ResourceKind::PcmData
                && resource.mime_type.as_deref() == Some("audio/wav")
        }));
        assert!(resources.iter().any(|resource| {
            resource.kind == ResourceKind::Image
                && resource.label.as_deref() == Some("FIGURE.DIC:00001200:0017:9x2")
        }));

        let view = package
            .render_target(&token, &RenderOptions::default())
            .unwrap();
        assert_eq!(view.kind, ResolvedTargetKind::Deferred);
        assert_eq!(view.resources.len(), resources.len());
        assert!(view.capabilities.contains(&RenderCapability::HcRenderInput));
        assert!(view.capabilities.contains(&RenderCapability::Images));
        assert!(view.capabilities.contains(&RenderCapability::Audio));
    }

    #[test]
    fn ssed_hc03e9_pdfspread_resource_is_exposed_from_page_anchor() {
        let dir = tempdir().unwrap();
        let page_anchor = [
            0x23, 0x30, 0x23, 0x30, 0x23, 0x30, 0x23, 0x30, 0x23, 0x30, 0x23, 0x30, 0x23, 0x31,
            0x23, 0x37,
        ];
        fs::write(
            dir.path().join("HONMON.DIC"),
            fixture_sseddata_literal_chunks(&[&page_anchor], 100, 100),
        )
        .unwrap();
        let connection = Connection::open(dir.path().join("HKRKIKHY2.db")).unwrap();
        connection
            .execute_batch(
                r#"
                create table PDFSpread (IDRight text primary key, IDLeft text, PDF blob);
                insert into PDFSpread values ('００００００１７', '００００００１６', X'255044462d706466737072656164');
                "#,
            )
            .unwrap();
        drop(connection);
        fs::write(dir.path().join("._HKRKIKHY2.db"), b"metadata").unwrap();
        let catalog = SsedCatalog {
            title: "PDFSpread".to_owned(),
            components: vec![SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0x00,
                start_block: 100,
                end_block: 100,
                data: [0; 4],
                filename: "HONMON.DIC".to_owned(),
                role: SsedComponentRole::Honmon,
            }],
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 1,
                trailing_bytes: 0,
            },
        };
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 80,
                title: Some("PDFSpread".to_owned()),
                evidence: Vec::new(),
            },
            Vec::new(),
            PackageStores {
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

        let input = package.renderer_input_for_target(&target).unwrap();
        let RendererInput::HcSsedStream { resources, .. } = input else {
            panic!("SSED address should produce HC renderer input");
        };
        let pdf = resources
            .iter()
            .find(|resource| resource.kind == ResourceKind::Pdf)
            .expect("PDFSpread resource should be exposed");

        assert_eq!(pdf.label.as_deref(), Some("PDFSpread/００００００１７"));
        assert_eq!(pdf.mime_type.as_deref(), Some("application/pdf"));
        assert_eq!(
            package.read_resource(&pdf.token).unwrap(),
            b"%PDF-pdfspread"
        );
    }

    #[test]
    fn ssed_hc_profile_hint_uses_exinfo_htmldll_without_binary() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("HONMON.DIC"),
            fixture_sseddata_literal_chunks(&[b"body"], 100, 100),
        )
        .unwrap();
        fs::write(
            dir.path().join("EXINFO.INI"),
            b"[GENERAL]\r\nHTMLDLL=HC03E9.dll\r\n",
        )
        .unwrap();
        let catalog = SsedCatalog {
            title: "EXINFO".to_owned(),
            components: vec![SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0x00,
                start_block: 100,
                end_block: 100,
                data: [0; 4],
                filename: "HONMON.DIC".to_owned(),
                role: SsedComponentRole::Honmon,
            }],
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 1,
                trailing_bytes: 0,
            },
        };
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 80,
                title: Some("EXINFO".to_owned()),
                evidence: Vec::new(),
            },
            Vec::new(),
            PackageStores {
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

        let input = package.renderer_input_for_target(&target).unwrap();
        let RendererInput::HcSsedStream { profile_hint, .. } = input else {
            panic!("SSED address should produce HC renderer input");
        };
        assert_eq!(profile_hint.as_deref(), Some("HC03E9"));
    }

    #[test]
    fn ssed_hc_renderer_input_uses_marker_entry_length_for_resource_scan() {
        let dir = tempdir().unwrap();
        let first_pcm = pcmdata_wave_chunks_for_test(1, b"\x80");
        let second_pcm = pcmdata_wave_chunks_for_test(1, b"\x81");
        let first_audio = pcmdata_range_control_for_test(
            500,
            0,
            500,
            u32::try_from(first_pcm.len() - 1).unwrap(),
        );
        let second_audio = pcmdata_range_control_for_test(
            501,
            0,
            501,
            u32::try_from(second_pcm.len() - 1).unwrap(),
        );
        let mut honmon = Vec::new();
        honmon.extend_from_slice(&SSED_ENTRY_MARKER);
        honmon.extend_from_slice(b"first");
        honmon.extend_from_slice(&first_audio);
        let second_entry_offset = honmon.len();
        honmon.extend_from_slice(&SSED_ENTRY_MARKER);
        honmon.extend_from_slice(b"second");
        honmon.extend_from_slice(&second_audio);

        fs::write(
            dir.path().join("HONMON.DIC"),
            fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
        )
        .unwrap();
        fs::write(
            dir.path().join("PCMDATA.DIC"),
            fixture_sseddata_literal_chunks(&[&first_pcm, &second_pcm], 500, 501),
        )
        .unwrap();
        let catalog = SsedCatalog {
            title: "Bounded renderer scan".to_owned(),
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
                    component_type: 0xd8,
                    start_block: 500,
                    end_block: 501,
                    data: [0; 4],
                    filename: "PCMDATA.DIC".to_owned(),
                    role: SsedComponentRole::PcmData,
                },
            ],
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 2,
                trailing_bytes: 0,
            },
        };
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 80,
                title: Some("Bounded renderer scan".to_owned()),
                evidence: Vec::new(),
            },
            Vec::new(),
            PackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );
        let token = TargetToken::new(&InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 100,
            offset: 0,
        })
        .unwrap();

        let input = package.renderer_input_for_target(&token).unwrap();
        let RendererInput::HcSsedStream {
            length,
            resources,
            diagnostics,
            ..
        } = input
        else {
            panic!("SSED address should produce HC renderer input");
        };
        assert_eq!(length, Some(second_entry_offset as u64));
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "ssed_renderer_resource_scan_bounded")
        );
        assert_eq!(resources.len(), 1);
        let expected_label = format!(
            "PCMDATA.DIC:00000500:0000-00000500:{:04}",
            first_pcm.len() - 1
        );
        assert_eq!(resources[0].label.as_deref(), Some(expected_label.as_str()));
    }

    #[test]
    fn ssed_hc_renderer_input_uses_index_boundary_for_marker_variants() {
        let dir = tempdir().unwrap();
        let mut honmon = Vec::new();
        honmon.extend_from_slice(&[0x1f, 0x09, 0x00, 0x02]);
        honmon.extend_from_slice(b"first");
        let second_entry_offset = honmon.len();
        honmon.extend_from_slice(&[0x1f, 0x09, 0x00, 0x02]);
        honmon.extend_from_slice(b"second");
        fs::write(
            dir.path().join("HONMON.DIC"),
            fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
        )
        .unwrap();
        fs::write(
            dir.path().join("FHINDEX.DIC"),
            fixture_sseddata_literal_chunks(
                &[&simple_index_page_for_test(&[
                    (&[0x24, 0x22], 100, 0),
                    (
                        &[0x24, 0x24],
                        100,
                        u16::try_from(second_entry_offset).unwrap(),
                    ),
                ])],
                200,
                200,
            ),
        )
        .unwrap();
        let catalog = SsedCatalog {
            title: "Index boundaries".to_owned(),
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
                component_count: 2,
                trailing_bytes: 0,
            },
        };
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 80,
                title: Some("Index boundaries".to_owned()),
                evidence: Vec::new(),
            },
            Vec::new(),
            PackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );
        let token = TargetToken::new(&InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 100,
            offset: 0,
        })
        .unwrap();

        let input = package.renderer_input_for_target(&token).unwrap();
        let RendererInput::HcSsedStream { length, .. } = input else {
            panic!("SSED address should produce HC renderer input");
        };
        assert_eq!(length, Some(second_entry_offset as u64));
    }

    #[test]
    fn ssed_hc_renderer_input_preserves_prefixed_entry_marker_start() {
        let dir = tempdir().unwrap();
        let mut honmon = Vec::new();
        honmon.extend_from_slice(&[0x1f, 0x02]);
        honmon.extend_from_slice(&SSED_ENTRY_MARKER);
        honmon.extend_from_slice(b"first");
        let second_entry_offset = honmon.len();
        honmon.extend_from_slice(&SSED_ENTRY_MARKER);
        honmon.extend_from_slice(b"second");
        fs::write(
            dir.path().join("HONMON.DIC"),
            fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
        )
        .unwrap();
        let catalog = SsedCatalog {
            title: "Prefixed marker".to_owned(),
            components: vec![SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0x00,
                start_block: 100,
                end_block: 100,
                data: [0; 4],
                filename: "HONMON.DIC".to_owned(),
                role: SsedComponentRole::Honmon,
            }],
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 1,
                trailing_bytes: 0,
            },
        };
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 80,
                title: Some("Prefixed marker".to_owned()),
                evidence: Vec::new(),
            },
            Vec::new(),
            PackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );
        let token = TargetToken::new(&InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 100,
            offset: 2,
        })
        .unwrap();

        let input = package.renderer_input_for_target(&token).unwrap();
        let RendererInput::HcSsedStream { offset, length, .. } = input else {
            panic!("SSED address should produce HC renderer input");
        };
        assert_eq!(offset, 0);
        assert_eq!(length, Some(second_entry_offset as u64));
    }

    #[test]
    fn loose_movie_resource_resolves_and_reads_movie_file() {
        let dir = tempdir().unwrap();
        let package_root = dir.path().join("_DCT_SAMPLE");
        let movie_root = dir.path().join("_DCT_SAMPLE_MOVIE");
        fs::create_dir(&package_root).unwrap();
        fs::create_dir(&movie_root).unwrap();
        fs::write(movie_root.join("12345678"), b"movie bytes").unwrap();

        let package = ReaderBookPackage::new(
            &package_root,
            DetectedPackage {
                root: package_root.clone(),
                format_family: FormatFamily::Ssed,
                confidence: 80,
                title: Some("Sample".to_owned()),
                evidence: Vec::new(),
            },
            Vec::new(),
            PackageStores::default(),
        );
        let token = ResourceToken::new(&InternalResource::LooseMovie {
            movie_id: "12345678".to_owned(),
        })
        .unwrap();

        let resource = package.resolve_resource(&token).unwrap();
        assert_eq!(resource.kind, ResourceKind::Video);
        assert_eq!(resource.mime_type.as_deref(), Some("video/mpeg"));
        assert!(resource.href.is_some());
        assert!(resource.diagnostics.is_empty());
        assert_eq!(package.read_resource(&token).unwrap(), b"movie bytes");
    }

    #[test]
    fn sounddata_resource_resolves_and_reads_wave_record() {
        let dir = tempdir().unwrap();
        let sound_root = dir.path().join("Sound");
        fs::create_dir(&sound_root).unwrap();
        fs::write(
            sound_root.join("SoundData"),
            b"RIFF\x04\x00\x00\x00WAVEignored trailing bytes",
        )
        .unwrap();
        fs::write(
            sound_root.join("WaveFile.map"),
            b"0000000000000000:001b 10\n",
        )
        .unwrap();

        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 80,
                title: Some("Sample".to_owned()),
                evidence: Vec::new(),
            },
            Vec::new(),
            PackageStores::default(),
        );
        let token = ResourceToken::new(&InternalResource::SoundData { sound_id: 10 }).unwrap();

        let resource = package.resolve_resource(&token).unwrap();
        assert_eq!(resource.kind, ResourceKind::SoundData);
        assert_eq!(resource.label.as_deref(), Some("SoundData/0000000a"));
        assert_eq!(resource.mime_type.as_deref(), Some("audio/wav"));
        assert!(resource.href.is_some());
        assert!(resource.diagnostics.is_empty());
        assert_eq!(
            package.read_resource(&token).unwrap(),
            b"RIFF\x04\x00\x00\x00WAVE"
        );
    }

    #[test]
    fn dense_honmon_address_target_resolves_sidecar_html() {
        let dir = tempdir().unwrap();
        let catalog = write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::BodyRows);
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Dense".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            PackageStores {
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
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Dense".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            PackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );
        let page = package
            .search(&SearchQuery {
                scope: crate::search::SearchScope::CurrentBook {
                    book_id: package.metadata().book_id.clone(),
                },
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
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Dense".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            PackageStores {
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
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Dense".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            PackageStores {
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
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Dense".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            PackageStores {
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
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Synthetic fulltext".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            PackageStores {
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
                scope: crate::search::SearchScope::CurrentBook {
                    book_id: package.metadata().book_id.clone(),
                },
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
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("Synthetic fulltext".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            PackageStores {
                ssed_catalog: Some(catalog),
                ..Default::default()
            },
        );

        let page = package
            .search(&SearchQuery {
                scope: crate::search::SearchScope::CurrentBook {
                    book_id: package.metadata().book_id.clone(),
                },
                mode: SearchMode::FullText,
                query: "fullwidth".to_owned(),
                cursor: None,
                limit: 10,
            })
            .unwrap();

        assert_eq!(page.hits.len(), 1);
    }

    #[test]
    fn ssed_index_search_key_uses_jis_fullwidth_ascii_order() {
        assert_eq!(encode_ssed_index_search_key(".c"), body_jis(".c"));
        assert_eq!(encode_ssed_index_search_key("30"), body_jis("30"));
        assert_eq!(encode_ssed_index_search_key("３０"), body_jis("30"));
    }

    #[test]
    fn parses_observed_styled_dense_anchor_records() {
        let mut record = Vec::new();
        record.extend_from_slice(&SSED_ENTRY_MARKER);
        record.extend_from_slice(&[0x1f, 0x41, 0x01, 0x60, 0x1f, 0x04]);
        record.extend_from_slice(&body_jis("00000005"));
        record.extend_from_slice(&[0x1f, 0x05, 0x1f, 0x61, 0x1f, 0x0a]);

        assert_eq!(
            parse_observed_ssed_dense_anchor_id(&record),
            Some("00000005".to_owned())
        );
    }

    enum DenseSidecarFixture {
        BodyRows,
        AndroidRowidTimesFiveBodyRows,
        TitleOnlyThenBodyRows,
        BlobBodyRows,
        MissingBetaRow,
    }

    fn write_ssed_dense_sidecar_fixture(root: &Path, fixture: DenseSidecarFixture) -> SsedCatalog {
        let mut body = Vec::new();
        let (alpha_anchor, beta_anchor) = match fixture {
            DenseSidecarFixture::AndroidRowidTimesFiveBodyRows => ("00000005", "00000010"),
            _ => ("00000001", "00000002"),
        };
        body.extend_from_slice(&dense_anchor_record(alpha_anchor));
        body.extend_from_slice(&dense_anchor_record(beta_anchor));
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
            DenseSidecarFixture::AndroidRowidTimesFiveBodyRows => {
                write_android_body_db(root.join("DENSE.db"), "DENSE");
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

    #[test]
    fn android_ssed_body_database_uses_rowid_times_five_anchor_rule() {
        let dir = tempdir().unwrap();
        let catalog = write_ssed_dense_sidecar_fixture(
            dir.path(),
            DenseSidecarFixture::AndroidRowidTimesFiveBodyRows,
        );
        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 95,
                title: Some("DENSE".to_owned()),
                evidence: Vec::new(),
            },
            ssed_capabilities(&catalog, dir.path()),
            PackageStores {
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
                html: "<div>android beta html</div>".to_owned(),
                source: BodySourceKind::SidecarHtml,
            }
        );
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

    fn write_android_body_db(path: PathBuf, table: &str) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(&format!(
                "create table {} (Html text);",
                quote_fixture_sql_identifier(table)
            ))
            .unwrap();
        connection
            .execute(
                &format!(
                    "insert into {} (Html) values (?), (?)",
                    quote_fixture_sql_identifier(table)
                ),
                (
                    "<div>android alpha html</div>",
                    "<div>android beta html</div>",
                ),
            )
            .unwrap();
    }

    fn quote_fixture_sql_identifier(name: &str) -> String {
        format!("\"{}\"", name.replace('"', "\"\""))
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

    fn screen_menu_image_control(width: u32, height: u32, block: u32, offset: u32) -> Vec<u8> {
        let mut payload = vec![0u8; 20];
        payload[0] = 0x1f;
        payload[1] = 0x4d;
        payload[10..12].copy_from_slice(&bcd_word(width));
        payload[12..14].copy_from_slice(&bcd_word(height));
        payload[14..18].copy_from_slice(&bcd_u32(block));
        payload[18..20].copy_from_slice(&bcd_word(offset));
        payload
    }

    fn screen_menu_hotspot_control(
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        block: u32,
        offset: u32,
    ) -> Vec<u8> {
        let mut payload = vec![0u8; 36];
        payload[0] = 0x1f;
        payload[1] = 0x4f;
        payload[8..10].copy_from_slice(&bcd_word(x));
        payload[10..12].copy_from_slice(&bcd_word(y));
        payload[12..14].copy_from_slice(&bcd_word(width));
        payload[14..16].copy_from_slice(&bcd_word(height));
        payload[28..32].copy_from_slice(&bcd_u32(block));
        payload[32..34].copy_from_slice(&bcd_word(offset));
        payload
    }

    fn bcd_word(value: u32) -> [u8; 2] {
        let s = format!("{value:04}");
        [
            ((s.as_bytes()[0] - b'0') << 4) | (s.as_bytes()[1] - b'0'),
            ((s.as_bytes()[2] - b'0') << 4) | (s.as_bytes()[3] - b'0'),
        ]
    }

    fn bcd_u32(value: u32) -> [u8; 4] {
        let s = format!("{value:08}");
        [
            ((s.as_bytes()[0] - b'0') << 4) | (s.as_bytes()[1] - b'0'),
            ((s.as_bytes()[2] - b'0') << 4) | (s.as_bytes()[3] - b'0'),
            ((s.as_bytes()[4] - b'0') << 4) | (s.as_bytes()[5] - b'0'),
            ((s.as_bytes()[6] - b'0') << 4) | (s.as_bytes()[7] - b'0'),
        ]
    }

    fn encrypt_logofont_cipher_for_test(data: &[u8]) -> Vec<u8> {
        let digest = Sha256::digest(b"LogoFontCipher");
        let key = &digest[..16];
        let mut previous = [0_u8; 16];
        previous.copy_from_slice(&digest[16..32]);
        let cipher = Aes128::new_from_slice(key).unwrap();
        let mut padded = data.to_vec();
        let padding = 16 - (padded.len() % 16);
        padded.extend(std::iter::repeat_n(padding as u8, padding));
        let mut encrypted = Vec::with_capacity(padded.len());
        for chunk in padded.chunks_exact(16) {
            let mut block = [0_u8; 16];
            for index in 0..16 {
                block[index] = chunk[index] ^ previous[index];
            }
            let mut block = aes::Block::from(block);
            cipher.encrypt_block(&mut block);
            previous.copy_from_slice(&block);
            encrypted.extend_from_slice(&block);
        }
        encrypted
    }

    fn pcmdata_wave_chunks_for_test(format_tag: u16, data: &[u8]) -> Vec<u8> {
        let mut fmt_payload = Vec::new();
        fmt_payload.extend_from_slice(&format_tag.to_le_bytes());
        fmt_payload.extend_from_slice(&1_u16.to_le_bytes());
        fmt_payload.extend_from_slice(&8000_u32.to_le_bytes());
        fmt_payload.extend_from_slice(&8000_u32.to_le_bytes());
        fmt_payload.extend_from_slice(&1_u16.to_le_bytes());
        fmt_payload.extend_from_slice(&8_u16.to_le_bytes());

        let mut chunks = Vec::new();
        chunks.extend_from_slice(b"fmt ");
        chunks.extend_from_slice(&(fmt_payload.len() as u32).to_le_bytes());
        chunks.extend_from_slice(&fmt_payload);
        chunks.extend_from_slice(b"data");
        chunks.extend_from_slice(&(data.len() as u32).to_le_bytes());
        chunks.extend_from_slice(data);
        chunks
    }

    fn pcmdata_range_control_for_test(
        start_block: u32,
        start_offset: u32,
        end_block: u32,
        end_offset: u32,
    ) -> Vec<u8> {
        let mut control = vec![0x1f, 0x4a, 0x00, 0x01, 0x00, 0x00];
        control.extend_from_slice(&bcd_decimal_for_test(start_block, 4));
        control.extend_from_slice(&bcd_decimal_for_test(start_offset, 2));
        control.extend_from_slice(&bcd_decimal_for_test(end_block, 4));
        control.extend_from_slice(&bcd_decimal_for_test(end_offset, 2));
        control
    }

    fn simple_index_page_for_test(rows: &[(&[u8], u32, u16)]) -> Vec<u8> {
        let mut page = vec![0_u8; crate::ssed::BLOCK_SIZE as usize];
        page[0..2].copy_from_slice(&0xc000_u16.to_be_bytes());
        page[2..4].copy_from_slice(&(rows.len() as u16).to_be_bytes());
        let mut pos = 4usize;
        for (key, block, offset) in rows {
            page[pos] = key.len() as u8;
            pos += 1;
            page[pos..pos + key.len()].copy_from_slice(key);
            pos += key.len();
            page[pos..pos + 4].copy_from_slice(&block.to_be_bytes());
            pos += 4;
            page[pos..pos + 2].copy_from_slice(&offset.to_be_bytes());
            pos += 2;
            page[pos..pos + 4].copy_from_slice(&0_u32.to_be_bytes());
            pos += 4;
            page[pos..pos + 2].copy_from_slice(&0_u16.to_be_bytes());
            pos += 2;
        }
        page
    }

    fn bcd_decimal_for_test(mut value: u32, bytes: usize) -> Vec<u8> {
        let mut out = vec![0_u8; bytes];
        for byte in out.iter_mut().rev() {
            let low = value % 10;
            value /= 10;
            let high = value % 10;
            value /= 10;
            *byte = ((high as u8) << 4) | low as u8;
        }
        out
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

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::body::{BodyProvider, BodySourceKind, VisualBody};
use crate::diagnostics::Diagnostic;
use crate::error::{Error, Result};
use crate::gaiji::{GaijiPolicy, GaijiProvider, GaijiResolution};
use crate::lved_sqlite::LvedSqliteStore;
use crate::navigation::{
    HomeSurface, NavigationItem, NavigationProvider, NavigationStatus, NavigationSurface,
    NavigationSurfaceKind,
};
use crate::render::{RenderOptions, RendererProvider, ResolvedTargetKind, ResolvedTargetView};
use crate::resources::{
    InternalResource, ResourceKind, ResourceProvider, ResourceRef, ResourceToken,
};
use crate::search::{SearchHit, SearchMode, SearchPage, SearchProvider, SearchQuery};
use crate::sequence::{SequenceHint, SequenceProvider, TargetWindow};
use crate::ssed::{SsedCatalog, SsedComponent, SsedComponentRole, SsedDataFile, SsedDataHeader};
use crate::ssed_index::{
    INDEX_PAGE_SIZE, SsedIndexPointer, SsedIndexRow, decode_title_text, is_leaf_page,
    is_simple_leaf_index_type, parse_simple_leaf_page,
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
        let capabilities = ssed_capabilities(&catalog);
        let package_root = detection.root.clone();
        Ok(Box::new(StubBookPackage::new(
            &package_root,
            detection,
            capabilities,
            Some(catalog),
            None,
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
        let detection = self
            .detect(root)?
            .ok_or_else(|| Error::Driver("not an LVED_SQLITE3 package".to_owned()))?;
        let package_root = detection.root.clone();
        let store = LvedSqliteStore::discover(root)?;
        Ok(Box::new(StubBookPackage::new(
            &package_root,
            detection,
            lved_capabilities(),
            None,
            store,
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
        Ok(Some(DetectedPackage {
            root: root.to_path_buf(),
            format_family: FormatFamily::LvlMultiView,
            confidence: 98,
            title: inferred_folder_title(root),
            evidence: vec!["menuData.xml".to_owned(), "*lvbat/*lvdat".to_owned()],
        }))
    }

    fn open(&self, root: &Path) -> Result<Box<dyn BookPackage>> {
        let detection = self
            .detect(root)?
            .ok_or_else(|| Error::Driver("not an LVLMultiView package".to_owned()))?;
        let package_root = detection.root.clone();
        Ok(Box::new(StubBookPackage::new(
            &package_root,
            detection,
            multiview_capabilities(),
            None,
            None,
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
        Ok(Box::new(StubBookPackage::new(
            &package_root,
            detection,
            hourei_capabilities(),
            None,
            None,
        )))
    }
}

pub struct StubBookPackage {
    root: PathBuf,
    storage: DirectoryStorage,
    metadata: BookMetadata,
    ssed_catalog: Option<SsedCatalog>,
    lved_store: Option<LvedSqliteStore>,
}

struct NormalizedHtmlRefs {
    html: String,
    resources: Vec<ResourceRef>,
    links: Vec<TargetLink>,
    diagnostics: Vec<Diagnostic>,
}

impl StubBookPackage {
    pub fn new(
        root: &Path,
        detected: DetectedPackage,
        capabilities: Vec<Capability>,
        ssed_catalog: Option<SsedCatalog>,
        lved_store: Option<LvedSqliteStore>,
    ) -> Self {
        let format_label = detected.format_family.ui_label().to_owned();
        let book_id = BookId(format!(
            "{}:{}",
            format_label,
            root.file_name()
                .map(|v| v.to_string_lossy())
                .unwrap_or_else(|| root.as_os_str().to_string_lossy())
        ));
        let metadata = BookMetadata {
            book_id,
            format_family: detected.format_family,
            format_label,
            title: detected.title,
            icon_hint: None,
            root_fingerprint: root_fingerprint(root),
            capabilities,
        };
        Self {
            root: root.to_path_buf(),
            storage: DirectoryStorage::new(root),
            metadata,
            ssed_catalog,
            lved_store,
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
                push_surface_if_exists(
                    &mut surfaces,
                    &self.storage,
                    "hanrei",
                    NavigationSurfaceKind::Hanrei,
                    "凡例",
                    &["HANREI.chm", "HANREI", "hanrei.html"],
                )?;
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
                            "SSED simple leaf title/index browsing is available; grouped/internal variants remain deferred",
                        )],
                    });
                }
            }
            FormatFamily::LvedSqlite3 => {
                surfaces.push(HomeSurface {
                    surface_id: "lved-list".to_owned(),
                    kind: NavigationSurfaceKind::TitleIndexBrowse,
                    status: NavigationStatus::Deferred,
                    title_html: "LVED list".to_owned(),
                    title_text: "LVED list".to_owned(),
                    target: None,
                    diagnostics: vec![Diagnostic::info(
                        "surface_deferred",
                        "LVED_SQLITE3 list browsing is deferred; search hits can resolve content rows",
                    )],
                });
                surfaces.push(HomeSurface {
                    surface_id: "info".to_owned(),
                    kind: NavigationSurfaceKind::Info,
                    status: NavigationStatus::Deferred,
                    title_html: "Info".to_owned(),
                    title_text: "Info".to_owned(),
                    target: None,
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
                    })?),
                    diagnostics: Vec::new(),
                });
            }
            FormatFamily::Hourei => {
                surfaces.push(HomeSurface {
                    surface_id: "law-tree".to_owned(),
                    kind: NavigationSurfaceKind::LawTree,
                    status: NavigationStatus::Deferred,
                    title_html: "法令".to_owned(),
                    title_text: "法令".to_owned(),
                    target: None,
                    diagnostics: vec![Diagnostic::info(
                        "surface_deferred",
                        "Hourei law tree opening is not wired in this milestone",
                    )],
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
        if self.metadata.format_family == FormatFamily::Ssed && surface_id == "title-index" {
            return self.open_ssed_title_index_surface(surface_id, 100);
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
            InternalTarget::Resource { resource } => {
                let resource_ref = self.resolve_resource(&resource)?;
                let diagnostics = resource_ref.diagnostics.clone();
                Ok(ResolvedTargetView {
                    kind: ResolvedTargetKind::MediaResource,
                    target: token.clone(),
                    title: resource_ref.label.clone(),
                    display_html: None,
                    basic_text: None,
                    resources: vec![resource_ref],
                    links: Vec::new(),
                    capabilities: Vec::new(),
                    diagnostics,
                    debug_trace: None,
                })
            }
            _ => {
                let body = self.visual_body_for_target(token)?;
                self.view_for_visual_body(token.clone(), body, options)
            }
        }
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
    fn resolve_gaiji(&self, identity: &str, _policy: &GaijiPolicy) -> GaijiResolution {
        GaijiResolution {
            identity: identity.to_owned(),
            unicode: None,
            resource: None,
            nonliteral_marker: false,
            diagnostics: vec![Diagnostic::info(
                "gaiji_deferred",
                "gaiji provider is not implemented yet",
            )],
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
        if self.metadata.format_family == FormatFamily::LvedSqlite3
            && sequence_hint.is_none_or(|hint| matches!(hint, SequenceHint::LvedListOrder))
            && let Some(window) = self.resolve_lved_list_window(target, before, after, options)?
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
            InternalTarget::SsedDenseAnchor { .. } => Ok(VisualBody::Unsupported {
                reason: "dense HONMON target requires sidecar/renderer database dereference"
                    .to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "dense_honmon_dereference_required",
                    "raw dense HONMON anchors must not be displayed directly",
                )],
            }),
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
        let hits = store.search(&query.query, &query.mode, query.limit)?;
        let hits = hits
            .into_iter()
            .map(|hit| {
                let target = TargetToken::new(&InternalTarget::LvedRow {
                    table: "content".to_owned(),
                    row_id: hit.content_id,
                    anchor: hit.anchor,
                })?;
                Ok(SearchHit {
                    book_id: self.metadata.book_id.clone(),
                    target,
                    title_html: hit.title_html,
                    title_text: hit.title_text,
                    snippet_html: (!hit.subtitle_html.is_empty()).then_some(hit.subtitle_html),
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(SearchPage {
            hits,
            next_cursor: None,
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
        if !matches!(
            query.mode,
            SearchMode::Exact | SearchMode::Forward | SearchMode::Partial
        ) {
            return Ok(SearchPage::deferred(
                "SSED search mode is not implemented for simple title/index scanning yet",
            ));
        }

        let mut diagnostics = Vec::new();
        let needle = query.query.to_lowercase();
        let mut hits = Vec::new();
        let scan_diagnostics = self.scan_ssed_simple_index_rows(None, |row| {
            let key = row.key.to_lowercase();
            let matched = match query.mode {
                SearchMode::Exact => key == needle,
                SearchMode::Forward => key.starts_with(&needle),
                SearchMode::Partial => key.contains(&needle),
                SearchMode::Backward | SearchMode::FullText | SearchMode::Advanced(_) => false,
            };
            if !matched {
                return Ok(true);
            }
            let target = match self.ssed_target_for_index_pointer(row.body)? {
                Ok(target) => target,
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    return Ok(true);
                }
            };
            let title = self
                .ssed_title_text(row.title)
                .unwrap_or_else(|| row.key.clone());
            hits.push(SearchHit {
                book_id: self.metadata.book_id.clone(),
                target,
                title_html: title.clone(),
                title_text: title,
                snippet_html: None,
                diagnostics: Vec::new(),
            });
            Ok(hits.len() < query.limit)
        })?;
        diagnostics.extend(scan_diagnostics);

        Ok(SearchPage {
            hits,
            next_cursor: None,
            diagnostics,
        })
    }

    fn open_ssed_title_index_surface(
        &self,
        surface_id: &str,
        limit: usize,
    ) -> Result<NavigationSurface> {
        let (rows, mut diagnostics) = self.ssed_simple_index_rows(limit)?;
        if rows.is_empty() && !diagnostics.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics,
            });
        }
        let mut items = Vec::new();
        for (index, row) in rows.into_iter().enumerate().take(limit) {
            let label = self
                .ssed_title_text(row.title)
                .unwrap_or_else(|| row.key.clone());
            let target = match self.ssed_target_for_index_pointer(row.body)? {
                Ok(target) => target,
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    continue;
                }
            };
            items.push(NavigationItem {
                item_id: format!("{}:{}", row.component, index),
                label_html: label.clone(),
                label_text: label,
                target,
            });
        }
        Ok(NavigationSurface::TitleIndexBrowse {
            surface_id: surface_id.to_owned(),
            items,
            next_cursor: None,
        })
    }

    fn ssed_simple_index_rows(&self, limit: usize) -> Result<(Vec<SsedIndexRow>, Vec<Diagnostic>)> {
        let mut rows = Vec::new();
        let diagnostics = self.scan_ssed_simple_index_rows(Some(limit), |row| {
            rows.push(row);
            Ok(rows.len() < limit)
        })?;
        Ok((rows, diagnostics))
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
            if !is_simple_leaf_index_type(component.component_type) {
                diagnostics.push(
                    Diagnostic::info(
                        "ssed_index_variant_deferred",
                        format!(
                            "{} is not a simple leaf index component",
                            component.filename
                        ),
                    )
                    .with_context("component", &component.filename),
                );
                continue;
            }
            let Some(path) = self
                .storage
                .resolve_casefolded(Path::new(&component.filename))?
            else {
                diagnostics.push(
                    Diagnostic::warning(
                        "ssed_index_component_missing",
                        format!("{} is declared but not present on disk", component.filename),
                    )
                    .with_context("component", &component.filename),
                );
                continue;
            };
            let mut reader = SsedDataFile::open(&path)?;
            let page_count = component.block_count() as usize;
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
                    diagnostics.push(
                        Diagnostic::info(
                            "ssed_index_internal_page_deferred",
                            format!("{} contains internal index pages", component.filename),
                        )
                        .with_context("component", &component.filename),
                    );
                    continue;
                }
                let logical_block = component.start_block + page_index as u32;
                let (page_rows, unknown) = parse_simple_leaf_page(
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
            .storage
            .resolve_casefolded(Path::new(&component.filename))
            .ok()
            .flatten()?;
        let mut reader = SsedDataFile::open(path).ok()?;
        let data = reader
            .read_range(usize::try_from(component_offset).ok()?, 512)
            .ok()?;
        let title = decode_title_text(&data);
        (!title.is_empty()).then_some(title)
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
        center.title = Some(self.ssed_index_row_label(&rows[center_index]));
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
        view.title = Some(self.ssed_index_row_label(row));
        Ok(Some(view))
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

    fn ssed_index_row_label(&self, row: &SsedIndexRow) -> String {
        self.ssed_title_text(row.title)
            .unwrap_or_else(|| row.key.clone())
    }

    fn ssed_component_for_index_pointer(&self, pointer: SsedIndexPointer) -> Option<&str> {
        self.ssed_catalog
            .as_ref()
            .and_then(|catalog| catalog.component_for_address(pointer.block))
            .map(|component| component.filename.as_str())
    }

    fn view_for_visual_body(
        &self,
        target: TargetToken,
        body: VisualBody,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        match body {
            VisualBody::PreservedHtml { html, source } => {
                let normalized = if source == BodySourceKind::LvedSqlite {
                    self.normalize_lved_html_refs(&html)?
                } else {
                    NormalizedHtmlRefs {
                        html,
                        resources: Vec::new(),
                        links: Vec::new(),
                        diagnostics: Vec::new(),
                    }
                };
                Ok(ResolvedTargetView {
                    kind: crate::render::ResolvedTargetKind::EntryBody,
                    target,
                    title: Some("Entry".to_owned()),
                    display_html: Some(normalized.html),
                    basic_text: None,
                    resources: normalized.resources,
                    links: normalized.links,
                    capabilities: vec![crate::render::RenderCapability::Html],
                    diagnostics: normalized.diagnostics,
                    debug_trace: None,
                })
            }
            VisualBody::SsedStream {
                component,
                offset,
                length,
            } => Ok(ResolvedTargetView {
                kind: crate::render::ResolvedTargetKind::Deferred,
                target,
                title: Some("SSED entry stream".to_owned()),
                display_html: None,
                basic_text: None,
                resources: Vec::new(),
                links: Vec::new(),
                capabilities: Vec::new(),
                diagnostics: vec![Diagnostic::info(
                    "hc_render_deferred",
                    "SSED stream resolved successfully; HC/profile rendering is not implemented yet",
                )],
                debug_trace: options.include_debug_trace.then(|| {
                    json!({
                        "body": {
                            "kind": "ssed_stream",
                            "component": component,
                            "offset": offset,
                            "length": length,
                        }
                    })
                    .to_string()
                }),
            }),
            VisualBody::SemanticFallback { text } => Ok(ResolvedTargetView {
                kind: crate::render::ResolvedTargetKind::EntryBody,
                target,
                title: Some("Semantic fallback".to_owned()),
                display_html: None,
                basic_text: Some(text),
                resources: Vec::new(),
                links: Vec::new(),
                capabilities: Vec::new(),
                diagnostics: vec![Diagnostic::info(
                    "semantic_fallback",
                    "visual renderer is unavailable; semantic fallback was returned",
                )],
                debug_trace: None,
            }),
            VisualBody::Unsupported {
                reason,
                diagnostics,
            } => Ok(ResolvedTargetView {
                kind: crate::render::ResolvedTargetKind::Unsupported,
                target,
                title: Some(reason),
                display_html: None,
                basic_text: None,
                resources: Vec::new(),
                links: Vec::new(),
                capabilities: Vec::new(),
                diagnostics,
                debug_trace: None,
            }),
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
        Ok(VisualBody::SsedStream {
            component: component.filename.clone(),
            offset: component_offset,
            length: None,
        })
    }

    fn visual_body_for_lved_row(&self, table: &str, row_id: i64) -> Result<VisualBody> {
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
            }
            cursor = end;
        }
        output.push_str(&html[cursor..]);
        Ok(NormalizedHtmlRefs {
            html: output,
            resources,
            links,
            diagnostics,
        })
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
        let relative = Path::new(&component.filename);
        let Some(path) = self
            .storage
            .resolve_casefolded(relative)
            .map_err(|err| Diagnostic::error("ssed_component_lookup_failed", err.to_string()))?
        else {
            return Err(Diagnostic::warning(
                "ssed_component_file_missing",
                format!("{} is declared but not present on disk", component.filename),
            ));
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LvedHtmlRefKind {
    Media,
    DataId,
}

fn next_lved_ref(value: &str) -> Option<(usize, LvedHtmlRefKind)> {
    let media = value
        .find("lved.media.")
        .map(|index| (index, LvedHtmlRefKind::Media));
    let dataid = value
        .find("lved.dataid:")
        .map(|index| (index, LvedHtmlRefKind::DataId));
    match (media, dataid) {
        (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
        (Some(found), None) | (None, Some(found)) => Some(found),
        (None, None) => None,
    }
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

fn ssed_capabilities(catalog: &SsedCatalog) -> Vec<Capability> {
    let mut capabilities = vec![
        Capability::Resources,
        Capability::HcRenderInput,
        Capability::ContinuousView,
        Capability::DeferredRendering,
    ];
    if catalog.has_role(SsedComponentRole::Index) {
        capabilities.push(Capability::NativeSearch);
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
    if catalog.has_role(SsedComponentRole::GaijiFull)
        || catalog.has_role(SsedComponentRole::GaijiHalf)
    {
        capabilities.push(Capability::Gaiji);
    }
    capabilities
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
        fs::write(dir.path().join("menuData.xml"), b"<menu/>").unwrap();
        fs::write(dir.path().join("blvdat"), b"payload").unwrap();

        let detected = LvlMultiViewDriver.detect(dir.path()).unwrap().unwrap();
        assert_eq!(detected.format_family, FormatFamily::LvlMultiView);
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
        assert_eq!(view.links.len(), 1);
        assert!(matches!(
            view.links[0].token.decode().unwrap(),
            InternalTarget::LvedRow {
                table,
                row_id: 101,
                anchor: Some(anchor)
            } if table == "content" && anchor == "jump"
        ));
        assert_eq!(view.resources.len(), 1);
        assert_eq!(view.resources[0].kind, ResourceKind::Audio);
        assert_eq!(
            package.read_resource(&view.resources[0].token).unwrap(),
            b"ID3\x03".to_vec()
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
        let package = StubBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 1,
                title: None,
                evidence: Vec::new(),
            },
            ssed_capabilities(&SsedCatalog {
                title: String::new(),
                components: Vec::new(),
                layout: crate::ssed::SsedInfoLayout {
                    component_count_offset: 0,
                    record_start: 0,
                    record_size: 0x30,
                    component_count: 0,
                    trailing_bytes: 0,
                },
            }),
            None,
            None,
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
                    create table content (id integer primary key, type integer, body text, media text);
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
                    insert into content values (100, 1, '<article><h1>Alpha</h1><p>body</p><a href=\"lved.media.sound:00010033.mp3\">sound</a><a href=\"lved.dataid:101#jump\">next</a></article>', '');
                    insert into content values (101, 1, '<article><h1>Beta</h1></article>', '');
                    insert into content values (102, 1, '<article><h1>Gamma</h1></article>', '');
                    insert into mediasub values (1, '00010033', 5, X'49443303');
                    insert into list values (1, 100, 1, 'body-anchor', '<b>alpha</b>', '<span>subtitle</span>');
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

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::body::{BodyProvider, VisualBody};
use crate::diagnostics::Diagnostic;
use crate::error::{Error, Result};
use crate::gaiji::{GaijiPolicy, GaijiProvider, GaijiResolution};
use crate::navigation::{
    HomeSurface, NavigationProvider, NavigationStatus, NavigationSurface, NavigationSurfaceKind,
};
use crate::render::{RenderOptions, RendererProvider, ResolvedTargetKind, ResolvedTargetView};
use crate::resources::{
    InternalResource, ResourceKind, ResourceProvider, ResourceRef, ResourceToken,
};
use crate::search::{SearchPage, SearchProvider, SearchQuery};
use crate::sequence::{SequenceHint, SequenceProvider, TargetWindow};
use crate::ssed::{SsedCatalog, SsedComponent, SsedComponentRole, SsedDataHeader};
use crate::storage::{DirectoryStorage, StorageBackend};
use crate::target::{InternalTarget, TargetToken};

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
        )))
    }
}

impl PackageDriver for LvedSqliteDriver {
    fn family(&self) -> FormatFamily {
        FormatFamily::LvedSqlite3
    }

    fn detect(&self, root: &Path) -> Result<Option<DetectedPackage>> {
        let package_root = package_root_for_detection(root);
        let storage = DirectoryStorage::new(package_root);
        let file_name = root
            .file_name()
            .map(|v| v.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let is_payload_file =
            root.is_file() && (file_name == "main.data" || file_name.ends_with(".dbc"));
        let has_main_data = is_payload_file && file_name == "main.data"
            || storage.exists(Path::new("main.data"))?;
        let has_dbc = is_payload_file && file_name.ends_with(".dbc")
            || !files_with_suffix(package_root, ".dbc")?.is_empty();
        if has_main_data || has_dbc {
            let mut evidence = Vec::new();
            if has_main_data {
                evidence.push("main.data".to_owned());
            }
            if has_dbc {
                evidence.push("*.dbc".to_owned());
            }
            if storage.exists(Path::new(".key"))?
                || !files_with_suffix(package_root, ".key")?.is_empty()
            {
                evidence.push("local_key_file".to_owned());
            }
            return Ok(Some(DetectedPackage {
                root: package_root.to_path_buf(),
                format_family: FormatFamily::LvedSqlite3,
                confidence: 90,
                title: inferred_folder_title(package_root),
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
        Ok(Box::new(StubBookPackage::new(
            &package_root,
            detection,
            lved_capabilities(),
            None,
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
            confidence: 95,
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
                confidence: 95,
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
        )))
    }
}

pub struct StubBookPackage {
    root: PathBuf,
    storage: DirectoryStorage,
    metadata: BookMetadata,
    ssed_catalog: Option<SsedCatalog>,
}

impl StubBookPackage {
    pub fn new(
        root: &Path,
        detected: DetectedPackage,
        capabilities: Vec<Capability>,
        ssed_catalog: Option<SsedCatalog>,
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
    fn search(&self, _query: &SearchQuery) -> Result<SearchPage> {
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
                        status: NavigationStatus::Deferred,
                        title_html: "Title/Index Browse".to_owned(),
                        title_text: "Title/Index Browse".to_owned(),
                        target: None,
                        diagnostics: vec![Diagnostic::info(
                            "surface_deferred",
                            "SSED title/index parsing is not wired in this milestone",
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
                        "LVED_SQLITE3 list/content opening is not wired in this milestone",
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
                Ok(self.view_for_visual_body(token.clone(), body, options))
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
                label: None,
                href: None,
                diagnostics: vec![Diagnostic::info(
                    "resource_deferred",
                    "media blob resource resolution is not implemented yet",
                )],
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
            InternalResource::MediaBlob { .. } => Err(Error::Driver(
                "media blob resource reading is not implemented yet".to_owned(),
            )),
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
        _sequence_hint: Option<&SequenceHint>,
        _before: usize,
        _after: usize,
        options: &RenderOptions,
    ) -> Result<TargetWindow> {
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
    fn view_for_visual_body(
        &self,
        target: TargetToken,
        body: VisualBody,
        options: &RenderOptions,
    ) -> ResolvedTargetView {
        match body {
            VisualBody::PreservedHtml { html, source: _ } => ResolvedTargetView {
                kind: crate::render::ResolvedTargetKind::EntryBody,
                target,
                title: Some("Entry".to_owned()),
                display_html: Some(html),
                basic_text: None,
                resources: Vec::new(),
                links: Vec::new(),
                capabilities: vec![crate::render::RenderCapability::Html],
                diagnostics: Vec::new(),
                debug_trace: None,
            },
            VisualBody::SsedStream {
                component,
                offset,
                length,
            } => ResolvedTargetView {
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
            },
            VisualBody::SemanticFallback { text } => ResolvedTargetView {
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
            },
            VisualBody::Unsupported {
                reason,
                diagnostics,
            } => ResolvedTargetView {
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
            },
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

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn detects_lved_sqlite3_by_main_data_and_key() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("main.data"), b"encrypted").unwrap();
        fs::write(dir.path().join("book.key"), b"key").unwrap();

        let detected = LvedSqliteDriver.detect(dir.path()).unwrap().unwrap();
        assert_eq!(detected.format_family, FormatFamily::LvedSqlite3);
        assert!(detected.evidence.contains(&"local_key_file".to_owned()));
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
}

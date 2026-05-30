use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

mod body;
mod gaiji;
mod navigation;
mod renderer;
mod renderer_input;
mod resources;
mod search;
mod sequence;
mod ssed_components;
mod ssed_index;
mod ssed_navigation;

use encoding_rs::SHIFT_JIS;
use serde_json::json;
use sha2::{Digest, Sha256};
use zip::ZipArchive;
use zip::result::ZipError;

use super::capabilities::default_search_modes_for_family;
use super::chm_toc::{
    chm_hanrei_entry_sort_key, chm_hhc_toc_items_to_nodes, chm_local_reference, parse_chm_hhc_toc,
};
use super::hc_profile::hc_renderer_profile;
use super::html::{
    HtmlAttrName, escape_plain_label_html, html_basic_text, html_document_label, html_label_text,
    html_unescape_minimal, next_html_href_or_src_attr, package_html_base_dir,
    package_relative_html_reference, path_has_extension,
};
use super::lved_refs::{
    LvedHtmlRefKind, is_lved_ref_terminator, lved_binran_target, lved_cross_book_target,
    lved_dataid_target, lved_image_resource, lved_info_target, lved_media_resource,
    lved_pdf_resource, lved_viewer_hook_target, next_lved_ref,
};
use super::navigation_helpers::{
    OrderedSequenceTarget, collect_navigation_node_ordered_targets,
    collect_navigation_node_targets, collect_panel_cell_ordered_targets,
    home_surface_reader_priority, lved_list_label_html, lved_tree_items_to_nodes,
    multiview_menu_item_to_node, navigation_node_mut_at_path,
};
use super::render_output::{
    finalize_generic_html_view as finalize_generic_html_display, finalize_resolved_view,
    generic_html_data_url, generic_html_inline_resource_max_bytes,
};
use super::resource_helpers::{
    MONOSCR_BITMAP_BYTES, MONOSCR_HEIGHT, MONOSCR_WIDTH, monoscr_bitmap_to_rgba,
    parse_colscr_wrapped_payload_size, resolved_kind_for_package_html_path,
    resource_kind_from_path, resource_mime_type,
};
use super::ssed_body_helpers::{
    SSED_ENTRY_MARKER, decode_offset_cursor, find_ssed_dense_anchor_record_end,
    hc03e9_pdfspread_anchor_text, looks_like_raw_anchor_label, parse_colscr_pointer,
    parse_observed_ssed_dense_anchor_id, parse_packed_bcd_pointer, parse_pcmdata_range_pointer,
    ssed_control_arg_length, ssed_find_next_entry_marker_offset,
    ssed_reader_generic_entry_marker_len,
};
use super::ssed_detection::{
    SSED_NAVIGATION_DETECTION_MAX_BYTES, file_starts_with_ssedinfo_magic, inferred_folder_title,
    read_ssed_navigation_detection_bytes, root_fingerprint, ssed_hanrei_page_label,
};
use super::ssed_index_probe::has_decodable_ssed_index_rows;
use super::ssed_payload::file_starts_with_android_wrapped_sseddata;
use super::ssed_search::{
    decode_ssed_body_search_text, normalize_search_match_text, reverse_search_match_text,
    ssed_ascii_key_needs_linear_safety_net, ssed_fulltext_snippet_html, ssed_index_row_order_key,
    ssed_index_search_key_candidates,
};
use super::ssed_search_runtime::{
    SSED_FULLTEXT_BODY_WINDOW_BYTES, SSED_FULLTEXT_SCAN_OVERLAP_BYTES,
    SSED_FULLTEXT_SCAN_WINDOW_BYTES, SsedFulltextRow, SsedIndexSearchCollector,
    SsedNearKeyScanResult, ssed_fulltext_body_window_len, ssed_index_component_name_is_backward,
    ssed_index_row_match_text,
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
    normalize_android_wrapped_sseddata_file_to_path,
};
use crate::diagnostics::Diagnostic;
use crate::error::{Error, Result};
use crate::gaiji::{
    GaijiPolicy, GaijiProvider, GaijiResolution, GaijiSourcePreference, RichLabel,
    normalize_gaiji_identity, resolve_rich_label,
};
use crate::hourei::{HoureiStore, escape_plain_label_html as escape_hourei_label_html};
use crate::image::encode_png_rgba;
use crate::lved_sqlite::{LvedSqliteStore, LvedSqliteSummary, infer_lved_dict_code};
use crate::multiview::{MultiviewStore, parse_menu_data};
use crate::navigation::{
    HomeSurface, NavigationItem, NavigationNode, NavigationProvider, NavigationStatus,
    NavigationSurface, NavigationSurfaceKind, PanelCell, ScreenMenuHotspot, ScreenMenuRect,
    ScreenMenuScreen,
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
    BLOCK_SIZE, SSEDDATA_MAGIC, SsedCatalog, SsedComponent, SsedComponentRole, SsedDataFile,
    SsedDataHeader,
};
use crate::ssed_aux_index::{
    SsedAuxIndexRow, SsedAuxIndexSpec, is_numeric_aux_index_filename,
    parse_aux_index_specs_from_exinfo, parse_aux_index_text_bytes,
};
use crate::ssed_encyclopedia::{SsedEncyclopediaRow, parse_encyclopedia_index};
use crate::ssed_figure::{FigureDimensions, figure_bitmap_to_png};
use crate::ssed_ga16::{ga16_glyph_png, ga16_resource_covers_code};
use crate::ssed_index::{
    INDEX_PAGE_SIZE, SsedIndexPointer, SsedIndexRow, SsedIndexScanState, decode_title_text,
    is_leaf_page, is_simple_leaf_index_type, is_supported_index_type, parse_internal_page,
    parse_simple_leaf_page, parse_supported_leaf_page,
};
use crate::ssed_loose_media::{
    discover_britannica_top_dat_files, discover_britannica_whatday_paths, find_loose_media_root,
    find_movie_file, has_britannica_top_dat_files, has_britannica_whatday_files,
    parse_lved_address, read_pcmu_record, render_britannica_html_fragment,
    resolve_loose_media_file, resolve_pcmu_record,
};
use crate::ssed_menu::{SsedMenuRecord, parse_menu_stream};
use crate::ssed_multi::{
    SsedMultiComponentRef, SsedMultiDescriptor, SsedMultiRecord, parse_multi_descriptor,
};
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
    search_ssed_dense_sidecar_bodies_with_resolvers,
};
use crate::ssed_sound_data::{SoundDataIndex, load_sounddata_index};
use crate::storage::{DirectoryStorage, StorageBackend, path_stays_inside_root, private_cache_dir};
use crate::target::{InternalTarget, TargetLink, TargetToken};

use self::ssed_navigation::{
    SsedHanreiPage, hourei_law_node_label, parse_ssed_multi_surface_id,
    read_path_inside_loose_root, read_path_inside_resolved_parent, ssed_aux_index_rows_to_nodes,
    ssed_encyclopedia_rows_to_nodes, ssed_menu_records_to_nodes, ssed_multi_record_index_ref,
    ssed_multi_record_menu_ref, ssed_multi_record_surface_id, ssed_multi_root_surface_id,
    ssed_multi_selector_records_to_nodes, ssed_panel_bin_record_to_navigation_cell,
    ssed_panel_inline_cell_to_navigation_cell,
};
use super::{
    BookAlias, BookAliasKind, BookId, BookMetadata, BookPackage, Capability, DetectedPackage,
    FormatFamily,
};

pub struct SsedDriver;
pub struct LvedSqliteDriver;
pub struct LvlMultiViewDriver;
pub struct HoureiDriver;

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

    pub(super) fn book_id_for_hit(&self) -> BookId {
        self.metadata.book_id.clone()
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

fn deferred_surface(surface_id: &str, diagnostics: Vec<Diagnostic>) -> NavigationSurface {
    NavigationSurface::Deferred {
        surface_id: surface_id.to_owned(),
        diagnostics,
    }
}

fn deferred_surface_info(
    surface_id: &str,
    code: impl Into<String>,
    message: impl Into<String>,
) -> NavigationSurface {
    deferred_surface(surface_id, vec![Diagnostic::info(code, message)])
}

fn deferred_surface_error(
    surface_id: &str,
    code: impl Into<String>,
    message: impl Into<String>,
) -> NavigationSurface {
    deferred_surface(surface_id, vec![Diagnostic::error(code, message)])
}

fn deferred_component_surface_info(
    surface_id: &str,
    code: impl Into<String>,
    message: impl Into<String>,
    component: &SsedComponent,
) -> NavigationSurface {
    deferred_surface(
        surface_id,
        vec![Diagnostic::info(code, message).with_context("component", &component.filename)],
    )
}

fn deferred_component_surface_warning(
    surface_id: &str,
    code: impl Into<String>,
    message: impl Into<String>,
    component: &SsedComponent,
) -> NavigationSurface {
    deferred_surface(
        surface_id,
        vec![Diagnostic::warning(code, message).with_context("component", &component.filename)],
    )
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

fn decode_package_html_text(data: &[u8]) -> String {
    match std::str::from_utf8(data) {
        Ok(value) => value.to_owned(),
        Err(_) => {
            let (decoded, _, _) = SHIFT_JIS.decode(data);
            decoded.into_owned()
        }
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

#[cfg(test)]
mod tests {
    use std::fs;

    use aes::Aes128;
    use aes::cipher::{BlockEncrypt, KeyInit};
    use rusqlite::Connection;
    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    use crate::lved_sqlite::apply_sqlcipher_key;
    use crate::render::{HcRendererProfileSource, HcRendererProfileStatus, RenderCapability};
    use crate::ssed::SSEDINFO_MAGIC;
    use crate::target::TargetKind;

    use super::super::PackageDriver;
    use super::super::capabilities::ssed_search_modes;
    use super::super::ssed_detection::ssed_capabilities;
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
        let generic_html = generic.display_html.as_deref().unwrap();
        assert!(!generic_html.contains("lvcore://target/"));
        assert!(!generic_html.contains("lvcore://resource/"));
        assert!(generic_html.contains("#lvcore-target-"));
        assert!(generic_html.contains("data:image/svg+xml;base64,"));
        assert!(
            generic
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "generic_html_resources_inlined")
        );
        assert!(
            generic
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "generic_html_targets_fragmentized")
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
                 00005221\t000007C2\t\t\t春\n\
                 10000000\t0000FFFF\t\t西和ABC順\n\
                 01000000\t0000FFFF\t\t五十音\n",
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
        let panel_target = nodes[0].children[1]
            .target
            .as_ref()
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(
            panel_target,
            InternalTarget::PanelCell {
                panel_id: "10000000".to_owned(),
                row: 0,
                column: 0,
            }
        );
        let panel_target = nodes[0].children[2]
            .target
            .as_ref()
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(
            panel_target,
            InternalTarget::PanelCell {
                panel_id: "01000000".to_owned(),
                row: 0,
                column: 0,
            }
        );
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
        let RendererInput::HcSsedStream {
            profile_hint,
            hc_profile,
            ..
        } = input
        else {
            panic!("SSED address should produce HC renderer input");
        };
        assert_eq!(profile_hint.as_deref(), Some("HC03E9"));
        let hc_profile = hc_profile.expect("EXINFO HTMLDLL should become HC profile metadata");
        assert_eq!(hc_profile.profile_id, "HC03E9");
        assert_eq!(hc_profile.source, HcRendererProfileSource::ExinfoHtmlDll);
        assert_eq!(hc_profile.status, HcRendererProfileStatus::InputOnly);
        assert_eq!(hc_profile.dll_sha256, None);
        assert_eq!(hc_profile.dll_size, None);
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
    fn dense_honmon_fulltext_searches_sidecar_body() {
        let dir = tempdir().unwrap();
        let catalog = write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::BodyRows);
        let search_modes = ssed_search_modes(&catalog, dir.path());
        assert!(search_modes.contains(&SearchMode::FullText));
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
                search_modes,
                ..Default::default()
            },
        );

        let page = package
            .search(&SearchQuery {
                scope: crate::search::SearchScope::CurrentBook {
                    book_id: package.metadata().book_id.clone(),
                },
                mode: SearchMode::FullText,
                query: "beta sidecar body".to_owned(),
                cursor: None,
                limit: 10,
            })
            .unwrap();

        assert_eq!(page.hits.len(), 1);
        assert_eq!(page.hits[0].title_text, "beta");
        assert!(matches!(
            page.hits[0].target.decode().unwrap(),
            InternalTarget::SsedDenseAnchor { anchor, .. } if anchor == "2"
        ));
        assert!(
            page.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "ssed_fulltext_sidecar_scan")
        );
        let view = package
            .render_target(&page.hits[0].target, &RenderOptions::default())
            .unwrap();
        assert_eq!(
            view.display_html.as_deref(),
            Some("<div>beta sidecar html</div>")
        );
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
    fn sharded_t_contents_sidecar_tables_are_all_considered() {
        let dir = tempdir().unwrap();
        let catalog = write_ssed_dense_sidecar_fixture(
            dir.path(),
            DenseSidecarFixture::ShardedTContentsBodyRows,
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
            offset: 32,
        })
        .unwrap();

        let body = package.visual_body_for_target(&target).unwrap();

        assert_eq!(
            body,
            VisualBody::PreservedHtml {
                html: "<div>beta sharded html</div>".to_owned(),
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
        let search_modes = ssed_search_modes(&catalog, dir.path());
        assert!(search_modes.contains(&SearchMode::FullText));
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
                search_modes,
                ..Default::default()
            },
        );
        assert!(
            package
                .metadata()
                .capabilities
                .contains(&Capability::FullTextSearch)
        );
        assert!(
            package
                .metadata()
                .search_modes
                .contains(&SearchMode::FullText)
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
        let search_modes = ssed_search_modes(&catalog, dir.path());
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
                search_modes,
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
    fn ssed_fulltext_metadata_requires_supported_honmon_payload() {
        let dir = tempdir().unwrap();
        let catalog = write_ssed_fulltext_fixture(dir.path());
        fs::write(dir.path().join("HONMON.DIC"), b"not an SSED payload").unwrap();

        let capabilities = ssed_capabilities(&catalog, dir.path());
        let search_modes = ssed_search_modes(&catalog, dir.path());

        assert!(!capabilities.contains(&Capability::FullTextSearch));
        assert!(!search_modes.contains(&SearchMode::FullText));
        assert!(search_modes.contains(&SearchMode::Exact));
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
        ShardedTContentsBodyRows,
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
            DenseSidecarFixture::ShardedTContentsBodyRows => {
                write_sharded_t_contents_body_db(root.join("body.db"));
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

    fn write_sharded_t_contents_body_db(path: PathBuf) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "
                create table t_contents_1 (f_DataId text primary key, f_Title text, f_Html text);
                create table t_contents_2 (f_DataId text primary key, f_Title text, f_Html text);
                insert into t_contents_2 values ('00000002', 'beta', '<div>beta sharded html</div>');
                ",
            )
            .unwrap();
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

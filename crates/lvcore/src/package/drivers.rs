use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

mod body;
mod body_hourei;
mod body_lved;
mod body_multiview;
mod body_ssed;
mod gaiji;
mod hourei_labels;
mod hourei_navigation;
mod html_resource_render;
mod law_render_refs;
mod lved_navigation;
mod lved_render_refs;
mod multiview_navigation;
mod navigation;
mod package_files;
mod renderer;
mod renderer_input;
mod resource_common;
mod resource_ssed;
mod resources;
mod search;
mod search_hourei;
mod search_lved;
mod search_multiview;
mod search_ssed;
mod sequence;
mod sequence_hourei;
mod sequence_lved;
mod sequence_multiview;
mod sequence_ssed;
mod ssed_aux_surfaces;
mod ssed_component_resources;
mod ssed_components;
mod ssed_hanrei_discovery;
mod ssed_hanrei_surfaces;
mod ssed_index;
mod ssed_multi_ids;
mod ssed_multi_surfaces;
mod ssed_navigation;
mod ssed_panel_navigation;
mod ssed_panel_surfaces;
mod ssed_renderer_resources;
mod ssed_screen_surfaces;
mod ssed_surfaces;

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
    ssed_ascii_key_needs_linear_safety_net, ssed_body_search_byte_candidates,
    ssed_body_window_may_contain_query, ssed_fulltext_snippet_html, ssed_index_row_order_key,
    ssed_index_search_key_candidates, ssed_raw_search_key_prefilter_candidates,
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
use crate::sequence::{SearchResultSequence, SequenceHint, SequenceProvider, TargetWindow};
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
use crate::ssed_hc::{HcBasicTextGaiji, decode_hc_stream_basic_text_with_gaiji};
use crate::ssed_index::{
    INDEX_PAGE_SIZE, SsedIndexPointer, SsedIndexRow, SsedIndexScanState, decode_title_text,
    is_leaf_page, is_simple_leaf_index_type, is_supported_index_type, parse_internal_page,
    parse_simple_leaf_page, parse_supported_leaf_page,
};
use crate::ssed_loose_media::{
    BRITANNICA_CHRONOLOGY_SOURCE_ID, discover_britannica_top_dat_files,
    discover_britannica_whatday_paths, find_loose_media_root, find_movie_file,
    has_britannica_top_dat_files, has_britannica_whatday_files,
    lookup_britannica_chronology_record, parse_lved_address, read_pcmu_record,
    render_britannica_html_fragment, resolve_loose_media_file, resolve_pcmu_record,
    search_britannica_chronology_records,
};
use crate::ssed_menu::{SsedMenuRecord, parse_menu_stream, parse_menu_stream_page};
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
    SsedSidecarBodyResolver, SsedSidecarKind, SsedSidecarLookup, SsedSidecarSearchPage,
    discover_ssed_sidecar_body_resolvers, lookup_ssed_dense_sidecar_body_with_resolvers,
    search_ssed_dense_sidecar_bodies_with_resolvers,
};
use crate::ssed_sound_data::{SoundDataIndex, load_sounddata_index};
use crate::storage::{
    DirectoryStorage, StorageBackend, path_stays_inside_root, private_cache_dir,
    regular_file_inside_root,
};
use crate::target::{InternalTarget, TargetLink, TargetToken};

use self::hourei_labels::hourei_law_node_label;
use self::ssed_multi_ids::{
    parse_ssed_multi_surface_id, ssed_multi_record_index_ref, ssed_multi_record_menu_ref,
    ssed_multi_record_surface_id, ssed_multi_root_surface_id,
};
use self::ssed_navigation::{
    SsedHanreiPage, read_path_inside_loose_root, read_path_inside_resolved_parent,
    ssed_aux_index_rows_to_flat_nodes, ssed_aux_index_rows_to_nodes,
    ssed_encyclopedia_rows_to_nodes, ssed_menu_records_to_nodes_from,
    ssed_multi_selector_records_to_nodes,
};
use self::ssed_panel_navigation::{
    ssed_panel_bin_record_to_navigation_cell, ssed_panel_inline_cell_to_navigation_cell,
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
        | InternalTarget::SsedAuxRecord { anchor, .. }
        | InternalTarget::HoureiLaw { anchor, .. }
        | InternalTarget::MultiviewHref { anchor, .. }
        | InternalTarget::Resource { anchor, .. } => anchor,
        _ => None,
    })
}

#[cfg(test)]
mod tests;

use std::path::Path;

use super::ssed_index_probe::has_decodable_ssed_index_rows;
use super::ssed_payload::has_supported_sseddata_component_payload_casefolded;
use super::{Capability, FormatFamily};
use crate::error::Result;
use crate::lved_sqlite::LvedSqliteSummary;
use crate::search::SearchMode;
use crate::ssed::SsedCatalog;
use crate::ssed_sidecar::discover_ssed_sidecar_body_resolvers_with_candidates;
use crate::storage::DirectoryStorage;

pub(super) fn lved_capabilities(
    search_modes: &[SearchMode],
    summary: &LvedSqliteSummary,
) -> Vec<Capability> {
    let mut capabilities = vec![
        Capability::Resources,
        Capability::Gaiji,
        Capability::PreservedHtml,
        Capability::ContinuousView,
    ];
    if summary.list_available || summary.tree_available {
        capabilities.push(Capability::TitleIndexBrowse);
    }
    if summary.info_available {
        capabilities.push(Capability::Hanrei);
    }
    if !search_modes.is_empty() {
        capabilities.push(Capability::NativeSearch);
    }
    if search_modes.contains(&SearchMode::FullText) {
        capabilities.push(Capability::FullTextSearch);
    }
    capabilities
}

pub(super) fn multiview_capabilities(has_law_navigation: bool) -> Vec<Capability> {
    let mut capabilities = vec![
        Capability::NativeSearch,
        Capability::FullTextSearch,
        Capability::Menu,
        Capability::Resources,
        Capability::Gaiji,
        Capability::PreservedHtml,
        Capability::ContinuousView,
    ];
    if has_law_navigation {
        capabilities.push(Capability::TitleIndexBrowse);
        capabilities.push(Capability::LawNavigation);
    }
    capabilities
}

pub(super) fn hourei_capabilities() -> Vec<Capability> {
    vec![
        Capability::NativeSearch,
        Capability::FullTextSearch,
        Capability::TitleIndexBrowse,
        Capability::Resources,
        Capability::PreservedHtml,
        Capability::ContinuousView,
        Capability::LawNavigation,
    ]
}

pub(super) fn standard_search_modes() -> Vec<SearchMode> {
    vec![
        SearchMode::Exact,
        SearchMode::Forward,
        SearchMode::Backward,
        SearchMode::Partial,
        SearchMode::FullText,
    ]
}

pub(super) fn default_search_modes_for_family(format_family: FormatFamily) -> Vec<SearchMode> {
    match format_family {
        FormatFamily::LvlMultiView | FormatFamily::Hourei => standard_search_modes(),
        _ => Vec::new(),
    }
}

pub(super) fn ssed_search_modes(catalog: &SsedCatalog, root: &Path) -> Vec<SearchMode> {
    let storage = DirectoryStorage::new(root.to_path_buf());
    if !has_decodable_ssed_index_rows(catalog, &storage) {
        return Vec::new();
    }
    let mut modes = vec![
        SearchMode::Exact,
        SearchMode::Forward,
        SearchMode::Backward,
        SearchMode::Partial,
    ];
    if catalog.honmon().is_some_and(|component| {
        has_supported_sseddata_component_payload_casefolded(&storage, component)
    }) {
        modes.push(SearchMode::FullText);
    }
    modes
}

pub(super) fn ssed_sidecar_search_modes(
    root: &Path,
    dict_id_hint: Option<&str>,
) -> Result<Vec<SearchMode>> {
    let resolvers = discover_ssed_sidecar_body_resolvers_with_candidates(root, dict_id_hint, &[])?;
    if resolvers.is_empty() {
        return Ok(Vec::new());
    }
    // Partial sidecar title matching exists below the search layer, but the
    // public partial cursor flow is still native-index-led.
    Ok(vec![
        SearchMode::Exact,
        SearchMode::Forward,
        SearchMode::Backward,
        SearchMode::FullText,
    ])
}

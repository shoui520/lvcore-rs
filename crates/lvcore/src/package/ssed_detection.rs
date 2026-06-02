use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::json;
use sha2::{Digest, Sha256};

use super::html::path_has_extension;
use super::ssed_index_probe::has_decodable_ssed_index_rows;
use super::ssed_payload::{
    has_component_payload_casefolded, has_supported_sseddata_component_payload_casefolded,
};
use super::ssed_zip::ssed_component_filename_aliases;
use super::{Capability, DetectedPackage, FormatFamily};
use crate::error::{Error, Result};
use crate::gaiji::{parse_ccaltstr_gaiji_map, parse_uni_gaiji_map};
use crate::multiview::parse_menu_data;
use crate::ssed::{ANDROID_LVEDINFO_MAGIC, SSEDINFO_MAGIC, SsedCatalog, SsedComponentRole};
use crate::ssed_aux_index::{is_numeric_aux_index_filename, parse_aux_index_specs_from_exinfo};
use crate::ssed_menu::parse_menu_stream;
use crate::storage::{DirectoryStorage, StorageBackend, regular_file_inside_root};

pub(super) const SSED_NAVIGATION_DETECTION_MAX_BYTES: usize = 1024 * 1024;

pub(super) struct DetectedSsedPackage {
    pub(super) detected: DetectedPackage,
    pub(super) catalog: SsedCatalog,
}

pub(super) fn ssed_hanrei_page_label(path: &str) -> String {
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

pub(super) fn root_fingerprint(root: &Path) -> String {
    let mut names = BTreeSet::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.path().symlink_metadata() else {
                continue;
            };
            let file_type = metadata.file_type();
            let name = path
                .file_name()
                .map(|v| v.to_string_lossy().to_string())
                .unwrap_or_default();
            names.insert(
                json!({
                    "name": name,
                    "is_file": file_type.is_file(),
                    "is_dir": file_type.is_dir(),
                    "is_symlink": file_type.is_symlink(),
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

pub(super) fn files_with_suffix(root: &Path, suffix: &str) -> Result<Vec<PathBuf>> {
    let mut rows = Vec::new();
    if !root.is_dir() {
        return Ok(rows);
    }
    let suffix = suffix.to_lowercase();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if regular_file_inside_root(root, &path)?
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

pub(super) fn load_package_uni_gaiji_maps(root: &Path) -> BTreeMap<String, String> {
    let mut merged = BTreeMap::new();
    for name in ["CCALTSTR.HA", "CCALTSTR.FU"] {
        let path = root.join(name);
        if !regular_file_inside_root(root, &path).unwrap_or(false) {
            continue;
        }
        let Ok(data) = fs::read(&path) else {
            continue;
        };
        merged.extend(parse_ccaltstr_gaiji_map(&data));
    }
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

pub(super) fn package_root_for_detection(path: &Path) -> &Path {
    if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    }
}

pub(super) fn inferred_folder_title(root: &Path) -> Option<String> {
    root.file_name().map(|name| {
        let raw = name.to_string_lossy();
        raw.strip_prefix("_DCT_").unwrap_or(raw.as_ref()).to_owned()
    })
}

pub(super) fn multiview_menu_title(root: &Path) -> Result<Option<String>> {
    let path = root.join("menuData.xml");
    if !regular_file_inside_root(root, &path)? {
        return Ok(None);
    }
    let xml = fs::read_to_string(path)?;
    let items = parse_menu_data(&xml)?;
    Ok(items
        .into_iter()
        .map(|item| item.label.trim().to_owned())
        .find(|label| !label.is_empty()))
}

pub(super) fn usable_multiview_title(title: &str) -> Option<String> {
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

pub(super) fn detect_ssed_package(root: &Path) -> Result<Option<DetectedSsedPackage>> {
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

pub(super) fn ssed_catalog_for_root(root: &Path) -> Result<SsedCatalog> {
    for path in files_with_suffix(root, ".idx")? {
        if let Ok(catalog) = SsedCatalog::parse_file(&path) {
            return Ok(catalog);
        }
    }
    Err(Error::Driver(
        "SSED catalog vanished after detection".to_owned(),
    ))
}

pub(super) fn ssed_capabilities(catalog: &SsedCatalog, root: &Path) -> Vec<Capability> {
    let mut capabilities = vec![
        Capability::Resources,
        Capability::HcRenderInput,
        Capability::ContinuousView,
        Capability::DeferredRendering,
    ];
    let storage = DirectoryStorage::new(root.to_path_buf());
    let has_decodable_index_rows = has_decodable_ssed_index_rows(catalog, &storage);
    if has_decodable_index_rows {
        capabilities.push(Capability::NativeSearch);
    }
    if has_decodable_index_rows
        && catalog.honmon().is_some_and(|component| {
            has_supported_sseddata_component_payload_casefolded(&storage, component)
        })
    {
        capabilities.push(Capability::FullTextSearch);
    }
    if has_decodable_index_rows {
        capabilities.push(Capability::TitleIndexBrowse);
    }
    if ssed_navigation_component_has_non_empty_surface(catalog, &storage, "MENU.DIC") {
        capabilities.push(Capability::Menu);
    }
    if ssed_navigation_component_has_non_empty_surface(catalog, &storage, "TOC.DIC") {
        capabilities.push(Capability::Toc);
    }
    if catalog
        .components_by_role(SsedComponentRole::MultiDescriptor)
        .any(|component| {
            component.has_positive_range() && has_component_payload_casefolded(&storage, component)
        })
    {
        capabilities.push(Capability::MultiSelector);
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
    if has_ssed_panel_metadata_casefolded(root, &storage) {
        capabilities.push(Capability::Panels);
    }
    if catalog.has_role(SsedComponentRole::GaijiFull)
        || catalog.has_role(SsedComponentRole::GaijiHalf)
    {
        capabilities.push(Capability::Gaiji);
    }
    capabilities
}

fn has_ssed_panel_metadata_casefolded(root: &Path, storage: &DirectoryStorage) -> bool {
    if has_any_casefolded(
        storage,
        &[
            "Panels.xml",
            "Panels.plist",
            "menu.plist",
            "menu_.plist",
            "menu_iPad.plist",
        ],
    ) {
        return true;
    }
    let Some(parent) = root.parent() else {
        return false;
    };
    [
        "Panels.plist",
        "menu.plist",
        "menu_.plist",
        "menu_iPad.plist",
    ]
    .iter()
    .map(|name| parent.join(name))
    .any(|candidate| {
        regular_file_inside_root(parent, &candidate).unwrap_or(false) && candidate.is_file()
    })
}

fn ssed_navigation_component_has_non_empty_surface(
    catalog: &SsedCatalog,
    storage: &DirectoryStorage,
    fallback_name: &str,
) -> bool {
    let Some(component) = catalog
        .component_named(fallback_name)
        .filter(|component| component.has_positive_range())
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
        let Ok(Some(data)) = read_ssed_navigation_detection_bytes(&path) else {
            continue;
        };
        let parsed = parse_menu_stream(&data);
        if !parsed.records.is_empty() {
            return true;
        }
    }

    false
}

pub(super) fn read_ssed_navigation_detection_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    super::ssed_payload::read_ssed_navigation_detection_bytes(
        path,
        SSED_NAVIGATION_DETECTION_MAX_BYTES,
    )
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
            && regular_file_inside_root(storage.root(), &path).unwrap_or(false)
            && !file_starts_with_ssedinfo_magic(&path).unwrap_or(true)
    })
}

pub(super) fn file_starts_with_ssedinfo_magic(path: &Path) -> Result<bool> {
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

#[cfg(test)]
mod tests {
    use super::root_fingerprint;
    use std::fs;
    use tempfile::tempdir;

    #[cfg(unix)]
    #[test]
    fn root_fingerprint_does_not_follow_symlink_targets() {
        use std::os::unix::fs::symlink;

        let package = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.bin");
        fs::write(&outside_file, b"short").unwrap();
        symlink(&outside_file, package.path().join("payload-link")).unwrap();

        let before = root_fingerprint(package.path());
        fs::write(&outside_file, b"much longer outside target").unwrap();
        let after = root_fingerprint(package.path());

        assert_eq!(
            before, after,
            "package fingerprint must hash the symlink entry, not outside target metadata",
        );
    }
}

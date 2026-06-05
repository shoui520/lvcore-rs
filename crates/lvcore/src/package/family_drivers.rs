use std::fs;
use std::path::Path;

use super::capabilities::{
    hourei_capabilities, lved_capabilities, multiview_capabilities, ssed_search_modes,
    standard_search_modes,
};
use super::drivers::{
    HoureiDriver, LvedSqliteDriver, LvlMultiViewDriver, PackageStores, ReaderBookPackage,
    SsedDriver,
};
use super::ssed_detection::{
    detect_ssed_package, inferred_folder_title, load_package_uni_gaiji_maps, multiview_menu_files,
    multiview_menu_title, package_root_for_detection, ssed_capabilities, ssed_catalog_for_root,
    usable_multiview_title,
};
use super::{BookPackage, DetectedPackage, FormatFamily, PackageDriver};
use crate::error::{Error, Result};
use crate::hourei::HoureiStore;
use crate::ios_dictlist::discover_ios_dictlist_info;
use crate::lved_sqlite::{AndroidDictInfo, LvedSqliteStore};
use crate::multiview::MultiviewStore;
use crate::search::SearchMode;
use crate::ssed::SsedCatalog;
use crate::storage::{DirectoryStorage, StorageBackend, regular_file_inside_root};

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
        let mut capabilities = ssed_capabilities(&catalog, &package_root);
        let retained_ios_dictlist = discover_ios_dictlist_info(&package_root)?;
        let mut search_modes = ssed_search_modes(&catalog, &package_root);
        let mut retained_ios_unresolved = retained_ios_dictlist.clone();
        let mut lved_store = None;
        let mut lved_summary = None;
        if search_modes.is_empty()
            && let Some(info) = &retained_ios_dictlist
            && let Some((store, summary, modes)) = open_retained_ios_lved_store(info)?
        {
            extend_unique_capabilities(&mut capabilities, lved_capabilities(&modes, &summary));
            search_modes = modes;
            lved_summary = Some(summary);
            lved_store = Some(store);
            retained_ios_unresolved = None;
        }
        if search_modes.is_empty()
            && let Some(info) = &retained_ios_dictlist
            && !info.fts_payloads.is_empty()
        {
            search_modes = info.search_modes.clone();
        }
        Ok(Box::new(ReaderBookPackage::new(
            &package_root,
            detection,
            capabilities,
            PackageStores {
                ssed_catalog: Some(catalog),
                lved_store,
                lved_summary,
                retained_ios_dictlist: retained_ios_unresolved,
                gaiji_unicode_map: load_package_uni_gaiji_maps(&package_root),
                search_modes,
                ..Default::default()
            },
        )))
    }
}

fn open_retained_ios_lved_store(
    info: &crate::ios_dictlist::IosDictListInfo,
) -> Result<
    Option<(
        LvedSqliteStore,
        crate::lved_sqlite::LvedSqliteSummary,
        Vec<SearchMode>,
    )>,
> {
    for payload in &info.fts_payloads {
        let Some(dict_id) = payload.dict_id else {
            continue;
        };
        if !payload.absolute_path.is_file() || payload.dict_code.is_empty() {
            continue;
        }
        let store = LvedSqliteStore::from_payload_with_derived_key_info(
            payload.absolute_path.clone(),
            AndroidDictInfo {
                dict_id,
                dict_code: payload.dict_code.clone(),
                title: payload.dictionary_name.clone().unwrap_or_default(),
                name: payload.dictionary_name.clone().unwrap_or_default(),
                fonts: Vec::new(),
            },
        );
        let Ok(summary) = store.summary() else {
            continue;
        };
        let modes = store.search_modes()?;
        return Ok(Some((store, summary, modes)));
    }
    Ok(None)
}

fn extend_unique_capabilities(
    capabilities: &mut Vec<super::Capability>,
    extra: Vec<super::Capability>,
) {
    for capability in extra {
        if !capabilities.contains(&capability) {
            capabilities.push(capability);
        }
    }
}

fn multiview_title(
    menu_title: Option<String>,
    retained_ssed_title: Option<String>,
) -> Option<String> {
    match (menu_title, retained_ssed_title) {
        (Some(menu), Some(retained))
            if retained.chars().count() > menu.chars().count()
                && compact_title(&retained).starts_with(&compact_title(&menu)) =>
        {
            Some(retained)
        }
        (Some(menu), _) => Some(menu),
        (None, retained) => retained,
    }
}

fn compact_title(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '　')
        .collect()
}

impl PackageDriver for LvedSqliteDriver {
    fn family(&self) -> FormatFamily {
        FormatFamily::LvedSqlite3
    }

    fn detect(&self, root: &Path) -> Result<Option<DetectedPackage>> {
        let package_root = package_root_for_detection(root);
        if detect_ssed_package(package_root)?.is_some() {
            return Ok(None);
        }
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
            lved_capabilities(&search_modes, &summary),
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
        let menu_files = multiview_menu_files(root)?;
        if menu_files.is_empty() {
            return Ok(None);
        }
        let payloads = fs::read_dir(root)?
            .filter_map(std::result::Result::ok)
            .filter(|entry| {
                let path = entry.path();
                if !regular_file_inside_root(root, &path).unwrap_or(false) {
                    return false;
                }
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
        let menu_title = multiview_menu_title(root)?;
        let retained_ssed_title = ssed_catalog_for_root(root)
            .ok()
            .and_then(|catalog| usable_multiview_title(&catalog.title));
        Ok(Some(DetectedPackage {
            root: root.to_path_buf(),
            format_family: FormatFamily::LvlMultiView,
            confidence: 98,
            title: multiview_title(menu_title, retained_ssed_title)
                .or_else(|| inferred_folder_title(root)),
            evidence: vec![
                format!("menu_xml:{}", menu_files.join(",")),
                "*lvbat/*lvdat".to_owned(),
            ],
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
        let has_law_navigation = store
            .as_ref()
            .map(MultiviewStore::has_law_navigation)
            .transpose()?
            .unwrap_or(false);
        Ok(Box::new(ReaderBookPackage::new(
            &package_root,
            detection,
            multiview_capabilities(has_law_navigation),
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

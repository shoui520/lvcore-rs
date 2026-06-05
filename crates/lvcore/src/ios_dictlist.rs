use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::plist_xml::{PlistValue, parse_xml_plist};
use crate::search::SearchMode;
use crate::storage::regular_file_inside_root;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IosDictListInfo {
    pub fts_payloads: Vec<IosDictFtsPayload>,
    pub full_db_payloads: Vec<IosDictFullDbPayload>,
    pub search_payloads: Vec<IosDictSearchPayload>,
    pub search_modes: Vec<SearchMode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IosDictFtsPayload {
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub dict_code: String,
    pub dict_id: Option<i64>,
    pub dictionary_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IosDictFullDbPayload {
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub dict_code: String,
    pub dictionary_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IosDictSearchPayload {
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub dict_code: String,
    pub dictionary_name: Option<String>,
}

pub(crate) fn discover_ios_dictlist_info(root: &Path) -> Result<Option<IosDictListInfo>> {
    let candidates = [
        root.join("DictList.plist"),
        root.parent()
            .map(|parent| parent.join("DictList.plist"))
            .unwrap_or_else(|| root.join("DictList.plist")),
    ];
    for candidate in candidates {
        if !regular_file_inside_root(candidate.parent().unwrap_or(root), &candidate)
            .unwrap_or(false)
        {
            continue;
        }
        if !candidate.is_file() {
            continue;
        }
        let bytes = fs::read(&candidate)?;
        let value = parse_xml_plist(&bytes, "DictList.plist")?;
        let plist_dir = candidate.parent().unwrap_or(root);
        let Some(info) = ios_dictlist_info_from_plist(plist_dir, &value)? else {
            continue;
        };
        return Ok(Some(info));
    }
    Ok(None)
}

fn ios_dictlist_info_from_plist(
    plist_dir: &Path,
    value: &PlistValue,
) -> Result<Option<IosDictListInfo>> {
    let Some(dict) = value.as_dict() else {
        return Ok(None);
    };
    let mut fts_payloads = Vec::new();
    let mut full_db_payloads = Vec::new();
    let mut search_payloads = Vec::new();
    if let Some(items) = dict.get("ItemArray").and_then(PlistValue::as_array) {
        for item in items.iter().filter_map(PlistValue::as_dict) {
            let dict_code = item
                .get("DictFolder")
                .and_then(PlistValue::as_str)
                .map(normalize_ios_dict_code)
                .filter(|value| !value.is_empty())
                .unwrap_or_default();
            let dictionary_name = item
                .get("DictName")
                .and_then(PlistValue::as_str)
                .map(str::to_owned);
            if let Some(relative_path) = item.get("DictFtsDB").and_then(PlistValue::as_str) {
                let relative_path = relative_path.trim();
                if !relative_path.is_empty() {
                    let absolute_path = plist_dir.join(relative_path);
                    if regular_file_inside_root(plist_dir, &absolute_path)? {
                        let dict_code = if dict_code.is_empty() {
                            ios_dict_code_from_path(relative_path).unwrap_or_default()
                        } else {
                            dict_code.clone()
                        };
                        let dict_id = ios_lved_sqlcipher_dict_id(&dict_code);
                        fts_payloads.push(IosDictFtsPayload {
                            relative_path: relative_path.to_owned(),
                            absolute_path,
                            dict_code,
                            dict_id,
                            dictionary_name: dictionary_name.clone(),
                        });
                    }
                }
            }
            if let Some(relative_path) = item.get("DictSearchDB").and_then(PlistValue::as_str) {
                let relative_path = relative_path.trim();
                if !relative_path.is_empty() {
                    let absolute_path = plist_dir.join(relative_path);
                    if regular_file_inside_root(plist_dir, &absolute_path)? {
                        let dict_code = if dict_code.is_empty() {
                            ios_dict_code_from_path(relative_path).unwrap_or_default()
                        } else {
                            dict_code.clone()
                        };
                        search_payloads.push(IosDictSearchPayload {
                            relative_path: relative_path.to_owned(),
                            absolute_path,
                            dict_code,
                            dictionary_name: dictionary_name.clone(),
                        });
                    }
                }
            }
            if let Some(relative_path) = item.get("DictFULLDB").and_then(PlistValue::as_str) {
                let relative_path = relative_path.trim();
                if !relative_path.is_empty() {
                    let absolute_path = plist_dir.join(relative_path);
                    if regular_file_inside_root(plist_dir, &absolute_path)? {
                        let dict_code = if dict_code.is_empty() {
                            ios_dict_code_from_path(relative_path).unwrap_or_default()
                        } else {
                            dict_code
                        };
                        full_db_payloads.push(IosDictFullDbPayload {
                            relative_path: relative_path.to_owned(),
                            absolute_path,
                            dict_code,
                            dictionary_name,
                        });
                    }
                }
            }
        }
    }
    let mut modes = Vec::new();
    if let Some(statuses) = dict.get("StatusArray").and_then(PlistValue::as_array) {
        for status in statuses.iter().filter_map(PlistValue::as_dict) {
            let Some(methods) = status.get("SearchMethod").and_then(PlistValue::as_array) else {
                continue;
            };
            for method in methods.iter().filter_map(PlistValue::as_dict) {
                if !method
                    .get("use")
                    .and_then(PlistValue::as_bool)
                    .unwrap_or(false)
                {
                    continue;
                }
                if let Some(mode) = method
                    .get("key")
                    .and_then(PlistValue::as_str)
                    .and_then(ios_search_mode_from_key)
                {
                    push_unique_search_mode(&mut modes, mode);
                }
            }
        }
    }
    sort_ios_search_modes(&mut modes);
    if fts_payloads.is_empty()
        && full_db_payloads.is_empty()
        && search_payloads.is_empty()
        && modes.is_empty()
    {
        return Ok(None);
    }
    Ok(Some(IosDictListInfo {
        fts_payloads,
        full_db_payloads,
        search_payloads,
        search_modes: modes.into_iter().collect(),
    }))
}

pub(crate) fn ios_lved_sqlcipher_dict_id(dict_code: &str) -> Option<i64> {
    match normalize_ios_dict_code(dict_code).as_str() {
        "OXFPEU4" => Some(750),
        "KQCMPROS" => Some(751),
        _ => None,
    }
}

fn ios_dict_code_from_path(relative_path: &str) -> Option<String> {
    let normalized = relative_path.replace('\\', "/");
    normalized
        .split('/')
        .find(|part| !part.trim().is_empty())
        .or_else(|| {
            Path::new(relative_path)
                .file_stem()
                .and_then(|value| value.to_str())
        })
        .map(normalize_ios_dict_code)
        .filter(|value| !value.is_empty())
}

fn normalize_ios_dict_code(value: &str) -> String {
    value
        .trim()
        .strip_prefix("_DCT_")
        .unwrap_or(value.trim())
        .trim_start_matches('.')
        .to_ascii_uppercase()
}

fn ios_search_mode_from_key(key: &str) -> Option<SearchMode> {
    match key {
        "Literal" => Some(SearchMode::Exact),
        "Forward" => Some(SearchMode::Forward),
        "Backward" => Some(SearchMode::Backward),
        "Part" => Some(SearchMode::Partial),
        "All" => Some(SearchMode::FullText),
        "Phrase" => Some(SearchMode::Advanced("phrase".to_owned())),
        "Example" => Some(SearchMode::Advanced("example".to_owned())),
        "Condition" => Some(SearchMode::Advanced("condition".to_owned())),
        "Sakuin" => Some(SearchMode::Advanced("sakuin".to_owned())),
        "Gyaku" => Some(SearchMode::Advanced("gyaku".to_owned())),
        _ => None,
    }
}

fn push_unique_search_mode(modes: &mut Vec<SearchMode>, mode: SearchMode) {
    if !modes.contains(&mode) {
        modes.push(mode);
    }
}

fn sort_ios_search_modes(modes: &mut [SearchMode]) {
    modes.sort_by(|left, right| {
        search_mode_sort_key(left)
            .cmp(&search_mode_sort_key(right))
            .then_with(|| advanced_search_mode_name(left).cmp(advanced_search_mode_name(right)))
    });
}

fn search_mode_sort_key(mode: &SearchMode) -> u8 {
    match mode {
        SearchMode::Exact => 0,
        SearchMode::Forward => 1,
        SearchMode::Backward => 2,
        SearchMode::Partial => 3,
        SearchMode::FullText => 4,
        SearchMode::Advanced(_) => 5,
    }
}

fn advanced_search_mode_name(mode: &SearchMode) -> &str {
    match mode {
        SearchMode::Advanced(value) => value,
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ios_dictlist_fts_payload_and_enabled_search_modes() {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("DICT");
        fs::create_dir(&package).unwrap();
        fs::write(package.join("DICT.dbc"), b"encrypted").unwrap();
        fs::write(
            root.path().join("DictList.plist"),
            br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>ItemArray</key><array><dict>
    <key>DictName</key><string>Sample Dictionary</string>
    <key>DictFtsDB</key><string>DICT/DICT.dbc</string>
  </dict></array>
  <key>StatusArray</key><array><dict>
    <key>SearchMethod</key><array>
      <dict><key>key</key><string>Forward</string><key>use</key><true/></dict>
      <dict><key>key</key><string>All</string><key>use</key><true/></dict>
      <dict><key>key</key><string>Example</string><key>use</key><false/></dict>
    </array>
  </dict></array>
</dict></plist>"#,
        )
        .unwrap();

        let info = discover_ios_dictlist_info(&package).unwrap().unwrap();
        assert_eq!(info.fts_payloads.len(), 1);
        assert_eq!(info.fts_payloads[0].relative_path, "DICT/DICT.dbc");
        assert!(info.full_db_payloads.is_empty());
        assert!(info.search_payloads.is_empty());
        assert_eq!(
            info.fts_payloads[0].dictionary_name.as_deref(),
            Some("Sample Dictionary")
        );
        assert_eq!(
            info.search_modes,
            vec![SearchMode::Forward, SearchMode::FullText]
        );
    }

    #[test]
    fn parses_ios_dictlist_fulldb_payload() {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("DICT");
        fs::create_dir(&package).unwrap();
        fs::write(package.join("DICT.sql"), b"SQLite format 3\0").unwrap();
        fs::write(
            root.path().join("DictList.plist"),
            br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>ItemArray</key><array><dict>
    <key>DictName</key><string>Sample FullDB</string>
    <key>DictFolder</key><string>DICT</string>
    <key>DictFULLDB</key><string>DICT/DICT.sql</string>
  </dict></array>
</dict></plist>"#,
        )
        .unwrap();

        let info = discover_ios_dictlist_info(&package).unwrap().unwrap();

        assert!(info.fts_payloads.is_empty());
        assert_eq!(info.full_db_payloads.len(), 1);
        assert_eq!(info.full_db_payloads[0].relative_path, "DICT/DICT.sql");
        assert_eq!(info.full_db_payloads[0].dict_code, "DICT");
        assert_eq!(
            info.full_db_payloads[0].dictionary_name.as_deref(),
            Some("Sample FullDB")
        );
    }

    #[test]
    fn parses_ios_dictlist_search_payload() {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("DICT");
        fs::create_dir(&package).unwrap();
        fs::write(package.join("DICT_Search.sql"), b"SQLite format 3\0").unwrap();
        fs::write(
            root.path().join("DictList.plist"),
            br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>ItemArray</key><array><dict>
    <key>DictName</key><string>Sample SearchDB</string>
    <key>DictFolder</key><string>DICT</string>
    <key>DictSearchDB</key><string>DICT/DICT_Search.sql</string>
  </dict></array>
  <key>StatusArray</key><array><dict>
    <key>SearchMethod</key><array>
      <dict><key>key</key><string>Example</string><key>use</key><true/></dict>
    </array>
  </dict></array>
</dict></plist>"#,
        )
        .unwrap();

        let info = discover_ios_dictlist_info(&package).unwrap().unwrap();

        assert!(info.fts_payloads.is_empty());
        assert!(info.full_db_payloads.is_empty());
        assert_eq!(info.search_payloads.len(), 1);
        assert_eq!(
            info.search_payloads[0].relative_path,
            "DICT/DICT_Search.sql"
        );
        assert_eq!(info.search_payloads[0].dict_code, "DICT");
        assert_eq!(
            info.search_payloads[0].dictionary_name.as_deref(),
            Some("Sample SearchDB")
        );
        assert_eq!(
            info.search_modes,
            vec![SearchMode::Advanced("example".to_owned())]
        );
    }

    #[test]
    fn maps_observed_ios_lved_sqlcipher_dict_ids() {
        assert_eq!(ios_lved_sqlcipher_dict_id("OXFPEU4"), Some(750));
        assert_eq!(ios_lved_sqlcipher_dict_id("_DCT_KQCMPROS"), Some(751));
        assert_eq!(ios_lved_sqlcipher_dict_id("unknown"), None);
    }
}

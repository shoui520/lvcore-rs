use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::{Error, Result};
use crate::search::SearchMode;
use crate::storage::regular_file_inside_root;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IosDictListInfo {
    pub fts_payloads: Vec<IosDictFtsPayload>,
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
enum PlistValue {
    Dict(BTreeMap<String, PlistValue>),
    Array(Vec<PlistValue>),
    String(String),
    Bool(bool),
    Integer,
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
        let value = parse_xml_plist(&bytes)?;
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
    if let Some(items) = dict.get("ItemArray").and_then(PlistValue::as_array) {
        for item in items.iter().filter_map(PlistValue::as_dict) {
            let Some(relative_path) = item.get("DictFtsDB").and_then(PlistValue::as_str) else {
                continue;
            };
            let relative_path = relative_path.trim();
            if relative_path.is_empty() {
                continue;
            }
            let absolute_path = plist_dir.join(relative_path);
            if !regular_file_inside_root(plist_dir, &absolute_path)? {
                continue;
            }
            let dict_code = item
                .get("DictFolder")
                .and_then(PlistValue::as_str)
                .map(normalize_ios_dict_code)
                .filter(|value| !value.is_empty())
                .or_else(|| ios_dict_code_from_fts_path(relative_path))
                .unwrap_or_default();
            let dict_id = ios_lved_sqlcipher_dict_id(&dict_code);
            fts_payloads.push(IosDictFtsPayload {
                relative_path: relative_path.to_owned(),
                absolute_path,
                dict_code,
                dict_id,
                dictionary_name: item
                    .get("DictName")
                    .and_then(PlistValue::as_str)
                    .map(str::to_owned),
            });
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
    if fts_payloads.is_empty() && modes.is_empty() {
        return Ok(None);
    }
    Ok(Some(IosDictListInfo {
        fts_payloads,
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

fn ios_dict_code_from_fts_path(relative_path: &str) -> Option<String> {
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

fn parse_xml_plist(bytes: &[u8]) -> Result<PlistValue> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| Error::Driver(format!("DictList.plist is not UTF-8 XML: {error}")))?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.name().as_ref() == b"plist" => {
                return parse_plist_root(&mut reader);
            }
            Ok(Event::Decl(_)) | Ok(Event::DocType(_)) | Ok(Event::Comment(_)) => {}
            Ok(Event::Eof) => {
                return Err(Error::Driver(
                    "DictList.plist ended before plist root".to_owned(),
                ));
            }
            Err(error) => return Err(plist_xml_error(&reader, error)),
            _ => {}
        }
    }
}

fn parse_plist_root(reader: &mut Reader<&[u8]>) -> Result<PlistValue> {
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => return parse_plist_value(reader, event),
            Ok(Event::Empty(event)) => return parse_empty_plist_value(event),
            Ok(Event::End(event)) if event.name().as_ref() == b"plist" => {
                return Err(Error::Driver("empty DictList.plist".to_owned()));
            }
            Ok(Event::Comment(_)) => {}
            Ok(Event::Eof) => {
                return Err(Error::Driver(
                    "DictList.plist ended inside plist root".to_owned(),
                ));
            }
            Err(error) => return Err(plist_xml_error(reader, error)),
            _ => {}
        }
    }
}

fn parse_plist_value(reader: &mut Reader<&[u8]>, event: BytesStart<'_>) -> Result<PlistValue> {
    match event.name().as_ref() {
        b"dict" => parse_plist_dict(reader),
        b"array" => parse_plist_array(reader),
        b"string" | b"key" => {
            parse_text_value(reader, event.name().as_ref()).map(PlistValue::String)
        }
        b"integer" => {
            let raw = parse_text_value(reader, b"integer")?;
            let value = raw.trim().parse::<i64>().map_err(|error| {
                Error::Driver(format!("invalid DictList.plist integer {raw:?}: {error}"))
            })?;
            let _value = value;
            Ok(PlistValue::Integer)
        }
        b"true" => {
            consume_until_end(reader, b"true")?;
            Ok(PlistValue::Bool(true))
        }
        b"false" => {
            consume_until_end(reader, b"false")?;
            Ok(PlistValue::Bool(false))
        }
        name => Err(Error::Driver(format!(
            "unsupported DictList.plist value element <{}>",
            String::from_utf8_lossy(name)
        ))),
    }
}

fn parse_empty_plist_value(event: BytesStart<'_>) -> Result<PlistValue> {
    match event.name().as_ref() {
        b"array" => Ok(PlistValue::Array(Vec::new())),
        b"dict" => Ok(PlistValue::Dict(BTreeMap::new())),
        b"string" => Ok(PlistValue::String(String::new())),
        b"integer" => Ok(PlistValue::Integer),
        b"true" => Ok(PlistValue::Bool(true)),
        b"false" => Ok(PlistValue::Bool(false)),
        name => Err(Error::Driver(format!(
            "unsupported empty DictList.plist value <{}>",
            String::from_utf8_lossy(name)
        ))),
    }
}

fn parse_plist_dict(reader: &mut Reader<&[u8]>) -> Result<PlistValue> {
    let mut rows = BTreeMap::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.name().as_ref() == b"key" => {
                let key = parse_text_value(reader, b"key")?;
                let value = parse_next_value(reader)?;
                rows.insert(key, value);
            }
            Ok(Event::End(event)) if event.name().as_ref() == b"dict" => {
                return Ok(PlistValue::Dict(rows));
            }
            Ok(Event::Comment(_)) => {}
            Ok(Event::Eof) => {
                return Err(Error::Driver("DictList.plist ended inside dict".to_owned()));
            }
            Err(error) => return Err(plist_xml_error(reader, error)),
            _ => {}
        }
    }
}

fn parse_plist_array(reader: &mut Reader<&[u8]>) -> Result<PlistValue> {
    let mut rows = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => rows.push(parse_plist_value(reader, event)?),
            Ok(Event::Empty(event)) => rows.push(parse_empty_plist_value(event)?),
            Ok(Event::End(event)) if event.name().as_ref() == b"array" => {
                return Ok(PlistValue::Array(rows));
            }
            Ok(Event::Comment(_)) => {}
            Ok(Event::Eof) => {
                return Err(Error::Driver(
                    "DictList.plist ended inside array".to_owned(),
                ));
            }
            Err(error) => return Err(plist_xml_error(reader, error)),
            _ => {}
        }
    }
}

fn parse_next_value(reader: &mut Reader<&[u8]>) -> Result<PlistValue> {
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => return parse_plist_value(reader, event),
            Ok(Event::Empty(event)) => return parse_empty_plist_value(event),
            Ok(Event::Comment(_)) => {}
            Ok(Event::Eof) => {
                return Err(Error::Driver(
                    "DictList.plist ended before dict value".to_owned(),
                ));
            }
            Err(error) => return Err(plist_xml_error(reader, error)),
            _ => {}
        }
    }
}

fn parse_text_value(reader: &mut Reader<&[u8]>, expected_end: &[u8]) -> Result<String> {
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Text(event)) => {
                let value = event.xml_content().map_err(|error| {
                    Error::Driver(format!(
                        "DictList.plist text decode error at byte {}: {error}",
                        reader.buffer_position()
                    ))
                })?;
                text.push_str(&value);
            }
            Ok(Event::CData(event)) => {
                let value = event.xml_content().map_err(|error| {
                    Error::Driver(format!(
                        "DictList.plist CDATA decode error at byte {}: {error}",
                        reader.buffer_position()
                    ))
                })?;
                text.push_str(&value);
            }
            Ok(Event::End(event)) if event.name().as_ref() == expected_end => return Ok(text),
            Ok(Event::Eof) => {
                return Err(Error::Driver(format!(
                    "DictList.plist ended inside <{}>",
                    String::from_utf8_lossy(expected_end)
                )));
            }
            Err(error) => return Err(plist_xml_error(reader, error)),
            _ => {}
        }
    }
}

fn consume_until_end(reader: &mut Reader<&[u8]>, expected_end: &[u8]) -> Result<()> {
    loop {
        match reader.read_event() {
            Ok(Event::End(event)) if event.name().as_ref() == expected_end => return Ok(()),
            Ok(Event::Eof) => {
                return Err(Error::Driver(format!(
                    "DictList.plist ended inside <{}>",
                    String::from_utf8_lossy(expected_end)
                )));
            }
            Err(error) => return Err(plist_xml_error(reader, error)),
            _ => {}
        }
    }
}

fn plist_xml_error(reader: &Reader<&[u8]>, error: quick_xml::Error) -> Error {
    Error::Driver(format!(
        "DictList.plist XML error at byte {}: {error}",
        reader.buffer_position()
    ))
}

impl PlistValue {
    fn as_dict(&self) -> Option<&BTreeMap<String, PlistValue>> {
        match self {
            Self::Dict(value) => Some(value),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&[PlistValue]> {
        match self {
            Self::Array(value) => Some(value),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
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
        assert_eq!(
            info.fts_payloads[0].dictionary_name.as_deref(),
            Some("Sample Dictionary")
        );
        assert_eq!(
            info.search_modes,
            vec![SearchMode::Forward, SearchMode::FullText]
        );
    }
}

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use super::{AndroidDictInfo, LvedKeyFile, files_with_suffix, normalize_lved_dict_code};
use crate::error::{Error, Result};
use crate::storage::regular_file_inside_root;

pub fn lved_payload_path(root: &Path) -> Result<Option<PathBuf>> {
    if root
        .parent()
        .is_some_and(|parent| regular_file_inside_root(parent, root).unwrap_or(false))
        && is_lved_payload_name(root)
    {
        return Ok(Some(root.to_path_buf()));
    }
    if !root.is_dir() {
        return Ok(None);
    }
    let main_data = root.join("main.data");
    if regular_file_inside_root(root, &main_data)? {
        return Ok(Some(main_data));
    }
    let mut dbc_files = files_with_suffix(root, ".dbc")?;
    dbc_files.sort();
    if let Some(path) = dbc_files.into_iter().next() {
        return Ok(Some(path));
    }
    let mut db_files = fs::read_dir(root)?
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| {
            regular_file_inside_root(root, path).unwrap_or(false) && is_lved_payload_name(path)
        })
        .collect::<Vec<_>>();
    db_files.sort();
    Ok(db_files.into_iter().next())
}

pub fn is_lved_payload_name(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    name == "main.data" || name.ends_with(".dbc") || is_android_lved_sqlcipher_payload(path)
}

pub fn is_android_lved_sqlcipher_payload(path: &Path) -> bool {
    let Some(extension) = path.extension() else {
        return false;
    };
    if !extension.eq_ignore_ascii_case("db") {
        return false;
    }
    if path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("thumbs.db"))
    {
        return false;
    }
    let Some(stem) = path
        .file_stem()
        .map(|value| normalize_lved_dict_code(&value.to_string_lossy()))
    else {
        return false;
    };
    let Some(parent) = path
        .parent()
        .and_then(|parent| parent.file_name())
        .map(|value| normalize_lved_dict_code(&value.to_string_lossy()))
    else {
        return false;
    };
    if stem.is_empty() || stem != parent {
        return false;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() % 4096 != 0
    {
        return false;
    }
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut header = [0_u8; 16];
    if file.read_exact(&mut header).is_err() {
        return false;
    }
    if &header == b"SQLite format 3\0" {
        return false;
    }
    let Some(root) = path.parent() else {
        return false;
    };
    regular_file_inside_root(root, &root.join("resource/conf.ini")).unwrap_or(false)
        || regular_file_inside_root(root, &root.join("resource/property.data")).unwrap_or(false)
}

pub fn infer_lved_dict_code(payload_path: &Path) -> Option<String> {
    if is_android_lved_sqlcipher_payload(payload_path) {
        return payload_path
            .file_stem()
            .map(|name| normalize_lved_dict_code(&name.to_string_lossy()));
    }
    if payload_path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("main.data"))
    {
        return payload_path
            .parent()
            .and_then(|parent| parent.file_name())
            .map(|name| normalize_lved_dict_code(&name.to_string_lossy()));
    }
    payload_path
        .file_stem()
        .map(|name| normalize_lved_dict_code(&name.to_string_lossy()))
}

pub fn derive_android_lved_sqlcipher_key(dict_id: i64, dict_code: &str) -> String {
    let code = normalize_lved_dict_code(dict_code);
    let mut chars = code.chars();
    let first = chars.next().unwrap_or_default();
    let last = code.chars().last().unwrap_or(first);
    let key_code = format!("{first}{last}").to_lowercase();
    // The original Android path effectively uses wrapping integer arithmetic.
    let key_id = dict_id.wrapping_mul(19286);
    format!("jlasgoiahoiampvsjhosDHfopj{key_code}{key_id}")
}

pub fn android_dictinfo_for_payload(path: &Path) -> Result<Option<AndroidDictInfo>> {
    let Some(dict_code) = infer_lved_dict_code(path) else {
        return Ok(None);
    };
    for info_path in discover_android_dictinfo_files(path) {
        let rows = parse_android_dictinfo(&info_path)?;
        if let Some(row) = rows.into_iter().find(|row| row.dict_code == dict_code) {
            return Ok(Some(row));
        }
    }
    Ok(None)
}

pub fn parse_android_dictinfo(path: &Path) -> Result<Vec<AndroidDictInfo>> {
    let xml = fs::read_to_string(path)?;
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);

    let mut rows = Vec::new();
    let mut current = None::<AndroidDictInfoBuilder>;
    let mut current_field = None::<AndroidDictInfoField>;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.name().as_ref() == b"dict" => {
                current = Some(android_dictinfo_builder_from_event(&reader, &event)?);
                current_field = None;
            }
            Ok(Event::Start(event)) if current.is_some() => {
                current_field = AndroidDictInfoField::from_name(event.name().as_ref());
            }
            Ok(Event::Text(text)) => {
                if let (Some(builder), Some(field)) = (&mut current, current_field) {
                    let value = text.xml_content().map_err(|error| {
                        Error::Driver(format!(
                            "Android dictinfo.xml text decode error at byte {}: {error}",
                            reader.buffer_position()
                        ))
                    })?;
                    if !value.trim().is_empty() {
                        builder.push_field(field, value.into_owned());
                    }
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if let (Some(builder), Some(field)) = (&mut current, current_field)
                    && let Some(value) = decode_xml_reference(reference.as_ref())
                {
                    builder.push_field(field, value);
                }
            }
            Ok(Event::End(event)) if event.name().as_ref() == b"dict" => {
                if let Some(row) = current.take().and_then(AndroidDictInfoBuilder::finish) {
                    rows.push(row);
                }
                current_field = None;
            }
            Ok(Event::End(_)) => current_field = None,
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(Error::Driver(format!(
                    "Android dictinfo.xml parse error at byte {}: {error}",
                    reader.buffer_position()
                )));
            }
            _ => {}
        }
    }
    Ok(rows)
}

#[derive(Debug, Default)]
struct AndroidDictInfoBuilder {
    dict_id: Option<i64>,
    dict_code: Option<String>,
    title: Option<String>,
    name: Option<String>,
    fonts: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum AndroidDictInfoField {
    Code,
    Title,
    Name,
    Font,
}

impl AndroidDictInfoField {
    fn from_name(name: &[u8]) -> Option<Self> {
        match name {
            b"code" => Some(Self::Code),
            b"title" => Some(Self::Title),
            b"name" => Some(Self::Name),
            b"font" | b"multi_font" | b"font_bold" => Some(Self::Font),
            _ => None,
        }
    }
}

impl AndroidDictInfoBuilder {
    fn push_field(&mut self, field: AndroidDictInfoField, value: String) {
        match field {
            AndroidDictInfoField::Code => append_android_dictinfo_field(&mut self.dict_code, value),
            AndroidDictInfoField::Title => append_android_dictinfo_field(&mut self.title, value),
            AndroidDictInfoField::Name => append_android_dictinfo_field(&mut self.name, value),
            AndroidDictInfoField::Font => self.fonts.push(value),
        }
    }

    fn finish(self) -> Option<AndroidDictInfo> {
        let dict_id = self.dict_id?;
        let dict_code = normalize_lved_dict_code(&self.dict_code?);
        if dict_code.is_empty() {
            return None;
        }
        let title = self.title.unwrap_or_default();
        let name = self.name.unwrap_or_else(|| title.clone());
        Some(AndroidDictInfo {
            dict_id,
            dict_code,
            title,
            name,
            fonts: self.fonts,
        })
    }
}

fn android_dictinfo_builder_from_event(
    reader: &Reader<&[u8]>,
    event: &BytesStart<'_>,
) -> Result<AndroidDictInfoBuilder> {
    let mut builder = AndroidDictInfoBuilder::default();
    for attr in event.attributes().flatten() {
        match attr.key.as_ref() {
            b"id" => {
                let value = attr
                    .decode_and_unescape_value(reader.decoder())
                    .map_err(|error| Error::Driver(format!("invalid Android dict id: {error}")))?;
                builder.dict_id = value.trim().parse::<i64>().ok();
            }
            b"name" => {
                let value = attr
                    .decode_and_unescape_value(reader.decoder())
                    .map_err(|error| {
                        Error::Driver(format!("invalid Android dict name: {error}"))
                    })?;
                builder.name = Some(value.trim().to_owned());
            }
            _ => {}
        }
    }
    Ok(builder)
}

fn append_android_dictinfo_field(slot: &mut Option<String>, value: String) {
    match slot {
        Some(existing) => existing.push_str(&value),
        None => *slot = Some(value),
    }
}

fn decode_xml_reference(value: &[u8]) -> Option<String> {
    let value = std::str::from_utf8(value).ok()?;
    let decoded = match value {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ if value.starts_with("#x") => {
            let code = u32::from_str_radix(&value[2..], 16).ok()?;
            char::from_u32(code)?
        }
        _ if value.starts_with('#') => {
            let code = value[1..].parse::<u32>().ok()?;
            char::from_u32(code)?
        }
        _ => return None,
    };
    Some(decoded.to_string())
}

fn discover_android_dictinfo_files(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = Vec::<PathBuf>::new();
    let mut current = if path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
    {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    };

    for _ in 0..6 {
        push_android_dictinfo_candidate(&current.join("dictinfo.xml"), &mut out, &mut seen);
        for child_name in ["android viewer", "resources", "res", "xml"] {
            let child = current.join(child_name);
            if child
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
            {
                collect_dictinfo_recursive(&child, &mut out, &mut seen);
            }
        }
        let child_names = match fs::read_dir(&current) {
            Ok(entries) => entries
                .filter_map(std::result::Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
                .collect::<Vec<_>>(),
            Err(_) => {
                let Some(parent) = current.parent() else {
                    break;
                };
                current = parent.to_path_buf();
                continue;
            }
        };
        if out.is_empty()
            && child_names
                .iter()
                .any(|name| matches!(name.as_str(), "sqlite" | "ssed"))
        {
            collect_dictinfo_recursive(&current, &mut out, &mut seen);
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent.to_path_buf();
    }

    out.sort();
    out
}

fn collect_dictinfo_recursive(root: &Path, out: &mut Vec<PathBuf>, seen: &mut Vec<PathBuf>) {
    let mut seen_dirs = std::collections::BTreeSet::new();
    collect_dictinfo_recursive_inner(root, out, seen, &mut seen_dirs, 0);
}

fn collect_dictinfo_recursive_inner(
    root: &Path,
    out: &mut Vec<PathBuf>,
    seen: &mut Vec<PathBuf>,
    seen_dirs: &mut std::collections::BTreeSet<PathBuf>,
    depth: usize,
) {
    if depth > 32 {
        return;
    }
    let Ok(metadata) = fs::symlink_metadata(root) else {
        return;
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return;
    }
    let Ok(canonical) = fs::canonicalize(root) else {
        return;
    };
    if !seen_dirs.insert(canonical) {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_dictinfo_recursive_inner(&path, out, seen, seen_dirs, depth + 1);
        } else if file_type.is_file()
            && path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("dictinfo.xml"))
        {
            push_android_dictinfo_candidate(&path, out, seen);
        }
    }
}

fn push_android_dictinfo_candidate(path: &Path, out: &mut Vec<PathBuf>, seen: &mut Vec<PathBuf>) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return;
    }
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if seen.iter().any(|seen_path| seen_path == &resolved) {
        return;
    }
    seen.push(resolved);
    out.push(path.to_path_buf());
}

pub fn discover_lved_key_file(payload_path: &Path) -> Result<Option<LvedKeyFile>> {
    let parent = payload_path.parent().unwrap_or_else(|| Path::new("."));
    let mut candidates = Vec::new();
    if let Some(dict_code) = infer_lved_dict_code(payload_path) {
        candidates.push((
            parent.join(format!("{dict_code}.key")),
            "inferred_dict_code",
        ));
        candidates.push((
            parent.join(format!("{}.key", dict_code.to_lowercase())),
            "inferred_dict_code_lower",
        ));
    }
    if payload_path.extension().is_some() {
        candidates.push((
            payload_path.with_extension(format!(
                "{}.key",
                payload_path
                    .extension()
                    .map(|value| value.to_string_lossy())
                    .unwrap_or_default()
            )),
            "payload_name",
        ));
    }
    if let Some(stem) = payload_path.file_stem() {
        candidates.push((
            parent.join(format!("{}.key", stem.to_string_lossy())),
            "payload_stem",
        ));
    }

    let mut seen = Vec::<PathBuf>::new();
    for (path, match_kind) in candidates {
        let resolved = path.canonicalize().unwrap_or_else(|_| path.clone());
        if seen.iter().any(|item| item == &resolved) {
            continue;
        }
        seen.push(resolved);
        if regular_file_inside_root(parent, &path)? {
            return Ok(Some(LvedKeyFile {
                path,
                match_kind: match_kind.to_owned(),
            }));
        }
    }

    let mut key_files = files_with_suffix(parent, ".key")?;
    key_files.sort();
    if key_files.len() == 1 {
        return Ok(Some(LvedKeyFile {
            path: key_files.remove(0),
            match_kind: "single_key_in_payload_dir".to_owned(),
        }));
    }
    Ok(None)
}

pub fn read_lved_key_file(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)?.trim().to_owned())
}

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::storage::path_stays_inside_root;

mod britannica_html;
mod pcmu;

pub use britannica_html::render_britannica_html_fragment;
pub use pcmu::{PcmuIndex, PcmuMapRecord, load_pcmu_index, read_pcmu_record, resolve_pcmu_record};

use britannica_html::plain_text_from_html;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LooseAddress {
    pub raw: String,
    pub block: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BritannicaMediaRoot {
    pub root_name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BritannicaWhatdayFile {
    pub root_name: String,
    pub relative_path: String,
    pub month: u8,
    pub day: u8,
    pub fragment_kind: BritannicaWhatdayKind,
    pub html: String,
    pub text: String,
    pub references: Vec<LooseAddress>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BritannicaWhatdayPath {
    pub root_name: String,
    pub relative_path: String,
    pub month: u8,
    pub day: u8,
    pub fragment_kind: BritannicaWhatdayKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BritannicaWhatdayKind {
    Body,
    Top,
}

impl BritannicaWhatdayKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Body => "body",
            Self::Top => "top",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BritannicaTopRecord {
    pub index: usize,
    pub item_id: String,
    pub title: String,
    pub description: String,
    pub address: LooseAddress,
    pub image_name: String,
    pub image_resource: Option<BritannicaLooseResourcePath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BritannicaTopDat {
    pub root_name: String,
    pub relative_path: String,
    pub category: String,
    pub records: Vec<BritannicaTopRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BritannicaLooseResourcePath {
    pub root_name: String,
    pub relative_path: String,
}

pub fn find_movie_file(package_root: &Path, movie_id: &str) -> Result<Option<PathBuf>> {
    if movie_id.len() != 8 || !movie_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(None);
    }
    let Some(directory) = find_loose_media_dir(package_root, "_MOVIE", false)? else {
        return Ok(None);
    };
    let Some(path) = find_child_casefolded(&directory, movie_id)? else {
        return Ok(None);
    };
    if !path.is_file() {
        return Ok(None);
    }
    if !path_stays_inside_root(&directory, &path)? {
        return Err(Error::Driver(format!(
            "_MOVIE file is outside its loose media root: {}",
            path.display()
        )));
    }
    Ok(Some(path))
}

pub fn discover_britannica_media_roots(package_root: &Path) -> Result<Vec<BritannicaMediaRoot>> {
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();
    for name in britannica_media_candidate_names(package_root) {
        let Some(path) = find_loose_media_root(package_root, &name)? else {
            continue;
        };
        if !path.is_dir() || !looks_like_britannica_media_root(&path)? {
            continue;
        }
        let key = path.canonicalize().unwrap_or_else(|_| path.clone());
        if seen.insert(key) {
            roots.push(BritannicaMediaRoot {
                root_name: path
                    .file_name()
                    .map(|value| value.to_string_lossy().to_string())
                    .unwrap_or(name),
                path,
            });
        }
    }
    roots.sort_by(|a, b| a.root_name.cmp(&b.root_name));
    Ok(roots)
}

pub fn find_loose_media_root(package_root: &Path, root_name: &str) -> Result<Option<PathBuf>> {
    if root_name.is_empty()
        || root_name.contains('/')
        || root_name.contains('\\')
        || root_name == "."
        || root_name == ".."
    {
        return Ok(None);
    }
    for parent in [Some(package_root), package_root.parent()]
        .into_iter()
        .flatten()
    {
        if let Some(path) = find_child_casefolded(parent, root_name)?
            && path.is_dir()
            && path_stays_inside_root(parent, &path)?
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

pub fn resolve_loose_media_file(
    package_root: &Path,
    root_name: &str,
    relative_path: &str,
) -> Result<Option<PathBuf>> {
    let Some(root) = find_loose_media_root(package_root, root_name)? else {
        return Ok(None);
    };
    let Some(normalized) = normalize_relative_path(relative_path) else {
        return Ok(None);
    };
    let mut current = root.clone();
    for part in normalized.split('/') {
        let Some(next) = find_child_casefolded(&current, part)? else {
            return Ok(None);
        };
        current = next;
    }
    if !current.is_file() {
        return Ok(None);
    }
    if !path_stays_inside_root(&root, &current)? {
        return Err(Error::Driver(format!(
            "loose media file is outside its media root: {}",
            current.display()
        )));
    }
    Ok(Some(current))
}

pub fn discover_britannica_whatday_files(
    package_root: &Path,
) -> Result<Vec<BritannicaWhatdayFile>> {
    discover_britannica_whatday_paths(package_root)?
        .into_iter()
        .map(|entry| {
            parse_britannica_whatday_file(package_root, &entry.root_name, &entry.relative_path)
        })
        .collect()
}

pub fn has_britannica_whatday_files(package_root: &Path) -> Result<bool> {
    for root in discover_britannica_media_roots(package_root)? {
        for (directory, _) in whatday_directories(&root)? {
            for entry in fs::read_dir(directory)? {
                let path = entry?.path();
                if path.is_file() && parse_whatday_filename(&path).is_some() {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

pub fn discover_britannica_whatday_paths(
    package_root: &Path,
) -> Result<Vec<BritannicaWhatdayPath>> {
    let mut files = Vec::new();
    for root in discover_britannica_media_roots(package_root)? {
        for (directory, relative_prefix) in whatday_directories(&root)? {
            for entry in fs::read_dir(directory)? {
                let path = entry?.path();
                if !path.is_file() {
                    continue;
                }
                let Some((month, day, fragment_kind)) = parse_whatday_filename(&path) else {
                    continue;
                };
                let relative_path = if relative_prefix.is_empty() {
                    path.file_name()
                        .map(|value| value.to_string_lossy().to_string())
                        .unwrap_or_default()
                } else {
                    format!(
                        "{}/{}",
                        relative_prefix,
                        path.file_name()
                            .map(|value| value.to_string_lossy())
                            .unwrap_or_default()
                    )
                };
                files.push(BritannicaWhatdayPath {
                    root_name: root.root_name.clone(),
                    relative_path,
                    month,
                    day,
                    fragment_kind,
                });
            }
        }
    }
    files.sort_by_key(|file| {
        (
            file.month,
            file.day,
            match file.fragment_kind {
                BritannicaWhatdayKind::Top => 0,
                BritannicaWhatdayKind::Body => 1,
            },
            file.root_name.clone(),
            file.relative_path.clone(),
        )
    });
    Ok(files)
}

pub fn parse_britannica_whatday_file(
    package_root: &Path,
    root_name: &str,
    relative_path: &str,
) -> Result<BritannicaWhatdayFile> {
    let Some(path) = resolve_loose_media_file(package_root, root_name, relative_path)? else {
        return Err(Error::Driver(format!(
            "Britannica whatday file not found: {root_name}/{relative_path}"
        )));
    };
    let Some((month, day, fragment_kind)) = parse_whatday_filename(&path) else {
        return Err(Error::Driver(format!(
            "not a Britannica whatday filename: {}",
            path.display()
        )));
    };
    let raw_html = decode_loose_text(&read_loose_media_file_checked(
        package_root,
        root_name,
        &path,
    )?);
    let html = render_britannica_html_fragment(&raw_html);
    Ok(BritannicaWhatdayFile {
        root_name: root_name.to_owned(),
        relative_path: relative_path.to_owned(),
        month,
        day,
        fragment_kind,
        text: plain_text_from_html(&html),
        references: extract_loose_addresses(&html),
        html,
    })
}

pub fn discover_britannica_top_dat_files(package_root: &Path) -> Result<Vec<BritannicaTopDat>> {
    let mut dat_files = Vec::new();
    for root in discover_britannica_media_roots(package_root)? {
        for (directory, relative_prefix) in top_directories(&root)? {
            for entry in fs::read_dir(directory)? {
                let path = entry?.path();
                if !path.is_file() || top_dat_category(&path).is_none() {
                    continue;
                }
                let relative_path = if relative_prefix.is_empty() {
                    path.file_name()
                        .map(|value| value.to_string_lossy().to_string())
                        .unwrap_or_default()
                } else {
                    format!(
                        "{}/{}",
                        relative_prefix,
                        path.file_name()
                            .map(|value| value.to_string_lossy())
                            .unwrap_or_default()
                    )
                };
                dat_files.push(parse_britannica_top_dat_path(
                    &root.root_name,
                    &relative_path,
                    &path,
                    &root.path,
                )?);
            }
        }
    }
    dat_files.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.root_name.cmp(&b.root_name))
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    Ok(dat_files)
}

pub fn has_britannica_top_dat_files(package_root: &Path) -> Result<bool> {
    for root in discover_britannica_media_roots(package_root)? {
        for (directory, _) in top_directories(&root)? {
            for entry in fs::read_dir(directory)? {
                let path = entry?.path();
                if path.is_file() && top_dat_category(&path).is_some() {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

pub fn parse_lved_address(value: &str) -> Option<LooseAddress> {
    parse_lved_dot_addr(value).or_else(|| parse_lvaddr_url(value))
}

pub(super) fn find_loose_media_dir(
    package_root: &Path,
    suffix: &str,
    require_wave_map: bool,
) -> Result<Option<PathBuf>> {
    let mut seen = BTreeSet::new();
    for (parent, name) in loose_media_candidate_names(package_root, suffix) {
        let Some(directory) = find_child_casefolded(&parent, &name)? else {
            continue;
        };
        let key = directory
            .canonicalize()
            .unwrap_or_else(|_| directory.clone());
        if !seen.insert(key) || !directory.is_dir() {
            continue;
        }
        if !path_stays_inside_root(&parent, &directory)? {
            continue;
        }
        if require_wave_map && find_child_casefolded(&directory, "WaveFile.map")?.is_none() {
            continue;
        }
        return Ok(Some(directory));
    }
    Ok(None)
}

fn loose_media_candidate_names(package_root: &Path, suffix: &str) -> Vec<(PathBuf, String)> {
    let mut names = Vec::new();
    if let Some(package_name) = package_root
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
    {
        names.push(format!("{package_name}{suffix}"));
        if let Some(stripped) = package_name.strip_prefix("_DCT_") {
            names.push(format!("{stripped}{suffix}"));
        }
    }
    names.push(suffix.to_owned());
    let mut candidates = Vec::new();
    for name in names {
        candidates.push((package_root.to_path_buf(), name.clone()));
        if let Some(parent) = package_root.parent() {
            candidates.push((parent.to_path_buf(), name));
        }
    }
    candidates
}

pub(super) fn find_child_casefolded(directory: &Path, name: &str) -> Result<Option<PathBuf>> {
    if !directory.is_dir() {
        return Ok(None);
    }
    let wanted = name.to_lowercase();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let Some(found) = path.file_name() else {
            continue;
        };
        if found.to_string_lossy().to_lowercase() == wanted {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn britannica_media_candidate_names(package_root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let package_name = package_root
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    if !package_name.is_empty() {
        names.push(format!("{package_name}_Media"));
        names.push(format!("{package_name}_Media_whatday"));
        names.push(format!("{package_name}_Media_top"));
        if let Some(stripped) = package_name.strip_prefix("_DCT_") {
            names.push(format!("{stripped}_Media"));
            names.push(format!("{stripped}_Media_whatday"));
            names.push(format!("{stripped}_Media_top"));
        }
    }
    names.push("Media".to_owned());
    names
}

fn looks_like_britannica_media_root(path: &Path) -> Result<bool> {
    if find_child_casefolded(path, "whatday")?.is_some()
        || find_child_casefolded(path, "top")?.is_some()
        || path
            .file_name()
            .map(|name| {
                let lower = name.to_string_lossy().to_lowercase();
                lower.ends_with("_media_whatday") || lower.ends_with("_media_top")
            })
            .unwrap_or(false)
    {
        return Ok(true);
    }
    Ok(false)
}

fn whatday_directories(root: &BritannicaMediaRoot) -> Result<Vec<(PathBuf, String)>> {
    let mut dirs = Vec::new();
    if root
        .path
        .file_name()
        .map(|name| {
            name.to_string_lossy()
                .to_lowercase()
                .ends_with("_media_whatday")
        })
        .unwrap_or(false)
    {
        dirs.push((root.path.clone(), String::new()));
    }
    if let Some(path) = find_child_casefolded(&root.path, "whatday")?
        && path.is_dir()
    {
        dirs.push((path, "whatday".to_owned()));
    }
    Ok(dirs)
}

fn top_directories(root: &BritannicaMediaRoot) -> Result<Vec<(PathBuf, String)>> {
    let mut dirs = Vec::new();
    if root
        .path
        .file_name()
        .map(|name| {
            name.to_string_lossy()
                .to_lowercase()
                .ends_with("_media_top")
        })
        .unwrap_or(false)
    {
        dirs.push((root.path.clone(), String::new()));
    }
    if let Some(path) = find_child_casefolded(&root.path, "top")?
        && path.is_dir()
    {
        dirs.push((path, "top".to_owned()));
    }
    Ok(dirs)
}

fn parse_whatday_filename(path: &Path) -> Option<(u8, u8, BritannicaWhatdayKind)> {
    let name = path.file_name()?.to_string_lossy();
    let (date, extension) = name.rsplit_once('.')?;
    let (month, day) = date.split_once('-')?;
    let month = month.parse::<u8>().ok()?;
    let day = day.parse::<u8>().ok()?;
    let fragment_kind = if extension.eq_ignore_ascii_case("body") {
        BritannicaWhatdayKind::Body
    } else if extension.eq_ignore_ascii_case("top") {
        BritannicaWhatdayKind::Top
    } else {
        return None;
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some((month, day, fragment_kind))
}

fn top_dat_category(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy();
    let lower = name.to_lowercase();
    if !lower.starts_with("top_") || !lower.ends_with(".dat") {
        return None;
    }
    let category = &name[4..name.len().saturating_sub(4)];
    (!category.is_empty()).then(|| category.to_lowercase())
}

fn parse_britannica_top_dat_path(
    root_name: &str,
    relative_path: &str,
    path: &Path,
    media_root: &Path,
) -> Result<BritannicaTopDat> {
    let Some(category) = top_dat_category(path) else {
        return Err(Error::Driver(format!(
            "not a Britannica top DAT filename: {}",
            path.display()
        )));
    };
    if !path_stays_inside_root(media_root, path)? {
        return Err(Error::Driver(format!(
            "Britannica top DAT path is outside its media root: {}",
            path.display()
        )));
    }
    let text = decode_loose_text(&fs::read(path)?);
    let lines = text
        .lines()
        .map(|line| line.trim_end_matches('\r').trim().to_owned())
        .collect::<Vec<_>>();
    let mut records = Vec::new();
    let mut cursor = 0usize;
    while cursor < lines.len() {
        while cursor < lines.len() && lines[cursor].is_empty() {
            cursor += 1;
        }
        if cursor >= lines.len() {
            break;
        }
        if cursor + 5 > lines.len() {
            return Err(Error::Driver(format!(
                "truncated Britannica top DAT record in {} at line {}",
                path.display(),
                cursor + 1
            )));
        }
        let item_id = lines[cursor].clone();
        let title = lines[cursor + 1].clone();
        let description = lines[cursor + 2].clone();
        let address = parse_top_address(&lines[cursor + 3])?;
        let image_name = lines[cursor + 4].clone();
        records.push(BritannicaTopRecord {
            index: records.len(),
            item_id,
            title,
            description,
            address,
            image_resource: resolve_top_image_resource(media_root, path, &image_name)?,
            image_name,
        });
        cursor += 5;
    }
    Ok(BritannicaTopDat {
        root_name: root_name.to_owned(),
        relative_path: relative_path.to_owned(),
        category,
        records,
    })
}

fn parse_top_address(value: &str) -> Result<LooseAddress> {
    let Some((block, offset)) = value.trim().split_once(':') else {
        return Err(Error::Driver(format!(
            "not a Britannica top address: {value}"
        )));
    };
    let block = u32::from_str_radix(block, 16)
        .map_err(|_| Error::Driver(format!("invalid Britannica top block: {value}")))?;
    let offset = u32::from_str_radix(offset, 16)
        .map_err(|_| Error::Driver(format!("invalid Britannica top offset: {value}")))?;
    Ok(LooseAddress {
        raw: value.trim().to_owned(),
        block,
        offset,
    })
}

fn resolve_top_image_resource(
    media_root: &Path,
    dat_path: &Path,
    image_name: &str,
) -> Result<Option<BritannicaLooseResourcePath>> {
    if image_name.is_empty() || image_name.contains('/') || image_name.contains('\\') {
        return Ok(None);
    }
    let media_root_name = media_root
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut candidates = Vec::new();
    if dat_path
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().eq_ignore_ascii_case("top"))
        .unwrap_or(false)
    {
        candidates.extend([
            format!("thumb/{image_name}"),
            format!("mini/{image_name}"),
            format!("full/{image_name}"),
            image_name.to_owned(),
        ]);
    } else if media_root
        .file_name()
        .map(|name| {
            name.to_string_lossy()
                .to_lowercase()
                .ends_with("_media_top")
        })
        .unwrap_or(false)
    {
        let root_name = media_root
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        if let Some(base) = root_name.strip_suffix("_top")
            && let Some(parent) = media_root.parent()
        {
            for suffix in ["_thumb", "_mini", "_full"] {
                let candidate_root = format!("{base}{suffix}");
                if let Some(root) = find_child_casefolded(parent, &candidate_root)?
                    && find_child_casefolded(&root, image_name)?.is_some()
                {
                    return Ok(Some(BritannicaLooseResourcePath {
                        root_name: candidate_root,
                        relative_path: image_name.to_owned(),
                    }));
                }
            }
        }
        candidates.push(image_name.to_owned());
    }
    for candidate in candidates {
        let Some(normalized) = normalize_relative_path(&candidate) else {
            continue;
        };
        let mut current = media_root.to_path_buf();
        let mut found = true;
        for part in normalized.split('/') {
            if let Some(next) = find_child_casefolded(&current, part)? {
                current = next;
            } else {
                found = false;
                break;
            }
        }
        if found && current.is_file() {
            return Ok(Some(BritannicaLooseResourcePath {
                root_name: media_root_name.clone(),
                relative_path: normalized,
            }));
        }
    }
    Ok(None)
}

fn decode_loose_text(data: &[u8]) -> String {
    match std::str::from_utf8(data) {
        Ok(value) => value.trim_start_matches('\u{feff}').to_owned(),
        Err(_) => {
            let (decoded, _, _) = encoding_rs::SHIFT_JIS.decode(data);
            decoded.into_owned()
        }
    }
}

fn read_loose_media_file_checked(
    package_root: &Path,
    root_name: &str,
    path: &Path,
) -> Result<Vec<u8>> {
    let Some(root) = find_loose_media_root(package_root, root_name)? else {
        return Err(Error::Driver(format!(
            "loose media root not found: {root_name}"
        )));
    };
    if !path_stays_inside_root(&root, path)? {
        return Err(Error::Driver(format!(
            "loose media file is outside its media root: {}",
            path.display()
        )));
    }
    Ok(fs::read(path)?)
}

fn extract_loose_addresses(fragment: &str) -> Vec<LooseAddress> {
    let mut addresses = Vec::new();
    let mut cursor = 0usize;
    while let Some(address) = parse_lved_address(&fragment[cursor..]) {
        let raw_offset = fragment[cursor..].find(&address.raw).unwrap_or_default();
        cursor += raw_offset + address.raw.len();
        addresses.push(address);
    }
    addresses
}

fn parse_lved_dot_addr(value: &str) -> Option<LooseAddress> {
    let start = value.to_ascii_lowercase().find("lved.addr")?;
    let after = start + "lved.addr".len();
    let block_hex = value.get(after..after + 8)?;
    let colon = value.as_bytes().get(after + 8).copied()?;
    if colon != b':' {
        return None;
    }
    let offset_hex = value.get(after + 9..after + 13)?;
    if !is_hex(block_hex) || !is_hex(offset_hex) {
        return None;
    }
    Some(LooseAddress {
        raw: value[start..after + 13].to_owned(),
        block: u32::from_str_radix(block_hex, 16).ok()?,
        offset: u32::from_str_radix(offset_hex, 16).ok()?,
    })
}

fn parse_lvaddr_url(value: &str) -> Option<LooseAddress> {
    let start = value.to_ascii_lowercase().find("lvaddr://")?;
    let after = start + "lvaddr://".len();
    let block_hex = value.get(after..after + 8)?;
    if value.as_bytes().get(after + 8).copied()? != b'/' {
        return None;
    }
    let offset_hex = value.get(after + 9..after + 13)?;
    if !is_hex(block_hex) || !is_hex(offset_hex) {
        return None;
    }
    Some(LooseAddress {
        raw: value[start..after + 13].to_owned(),
        block: u32::from_str_radix(block_hex, 16).ok()?,
        offset: u32::from_str_radix(offset_hex, 16).ok()?,
    })
}

fn is_hex(value: &str) -> bool {
    value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn normalize_relative_path(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    let normalized = path.replace('\\', "/");
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lved_addr_links() {
        let parsed = parse_lved_address(r#"href="lved.addr0000f768:00a2""#).unwrap();
        assert_eq!(parsed.block, 0x0000f768);
        assert_eq!(parsed.offset, 0x00a2);
    }

    #[test]
    fn britannica_whatday_table_drops_spacer_column() {
        let html = r#"<BODY><TABLE><TR><TD colSpan=3>head</TD></TR><TR><TD>603年</TD><TD>　</TD><TD>event</TD></TR></TABLE></BODY>"#;
        let rendered = render_britannica_html_fragment(html);
        assert!(rendered.contains("colSpan=2"));
        assert!(rendered.contains("<TD>603年</TD><TD>event</TD>"));
        assert!(!rendered.contains("<BODY>"));
    }

    #[cfg(unix)]
    #[test]
    fn pcmu_record_symlink_escape_is_not_readable() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("dict");
        let pcmu = package.join("_PCM_U");
        std::fs::create_dir_all(&pcmu).unwrap();
        std::fs::write(pcmu.join("WaveFile.map"), b"sound.bin 123\n").unwrap();
        let outside = dir.path().join("outside.bin");
        std::fs::write(&outside, b"outside").unwrap();
        std::os::unix::fs::symlink(&outside, pcmu.join("sound.bin")).unwrap();

        let error = read_pcmu_record(&package, 123).unwrap_err();
        assert!(error.to_string().contains("outside its loose media root"));
    }

    #[cfg(unix)]
    #[test]
    fn britannica_whatday_symlink_escape_is_not_readable() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("dict");
        let whatday = package.join("Media").join("whatday");
        std::fs::create_dir_all(&whatday).unwrap();
        let outside = dir.path().join("outside.body");
        std::fs::write(&outside, b"<body>outside</body>").unwrap();
        std::os::unix::fs::symlink(&outside, whatday.join("1-1.body")).unwrap();

        let error =
            parse_britannica_whatday_file(&package, "Media", "whatday/1-1.body").unwrap_err();
        assert!(error.to_string().contains("outside its media root"));
    }

    #[cfg(unix)]
    #[test]
    fn britannica_top_dat_symlink_escape_is_not_readable() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("dict");
        let top = package.join("Media").join("top");
        std::fs::create_dir_all(&top).unwrap();
        let outside = dir.path().join("top_people.dat");
        std::fs::write(&outside, b"id\ntitle\ndesc\n00000001:0000\nimage.jpg\n").unwrap();
        std::os::unix::fs::symlink(&outside, top.join("top_people.dat")).unwrap();

        let error = discover_britannica_top_dat_files(&package).unwrap_err();
        assert!(error.to_string().contains("outside its media root"));
    }

    #[cfg(unix)]
    #[test]
    fn loose_movie_symlink_escape_is_not_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("dict");
        let movie = package.join("_MOVIE");
        std::fs::create_dir_all(&movie).unwrap();
        let outside = dir.path().join("00000001");
        std::fs::write(&outside, b"outside").unwrap();
        std::os::unix::fs::symlink(&outside, movie.join("00000001")).unwrap();

        let error = find_movie_file(&package, "00000001").unwrap_err();
        assert!(error.to_string().contains("outside its loose media root"));
    }

    #[cfg(unix)]
    #[test]
    fn loose_media_root_symlink_escape_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("dict");
        std::fs::create_dir_all(&package).unwrap();
        let outside = dir.path().join("outside-media");
        std::fs::create_dir(&outside).unwrap();
        std::fs::create_dir(outside.join("whatday")).unwrap();
        std::fs::write(outside.join("whatday").join("1-1.body"), b"<body>x</body>").unwrap();
        std::os::unix::fs::symlink(&outside, package.join("Media")).unwrap();

        let roots = discover_britannica_media_roots(&package).unwrap();
        assert!(roots.is_empty());
    }
}

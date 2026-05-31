use std::fs;
use std::path::{Path, PathBuf};

use encoding_rs::SHIFT_JIS;

use super::title::html_to_text;
use super::{LvedTreeIndexItem, decode_sqlite_text};
use crate::error::Result;
use crate::storage::regular_file_inside_root;

pub(super) fn lved_tree_index_candidate_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    push_lved_tree_index_path(&mut paths, root, root.join("res/tree.idx"));
    push_lved_tree_index_path(&mut paths, root, root.join("tree.idx"));
    for entry in fs::read_dir(root)?.collect::<std::io::Result<Vec<_>>>()? {
        let path = entry.path();
        if regular_file_inside_root(root, &path)?
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("idx"))
        {
            push_lved_tree_index_path(&mut paths, root, path);
        }
    }
    let res_dir = root.join("res");
    if fs::symlink_metadata(&res_dir)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
    {
        for entry in fs::read_dir(&res_dir)?.collect::<std::io::Result<Vec<_>>>()? {
            let path = entry.path();
            if regular_file_inside_root(root, &path)?
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("idx"))
            {
                push_lved_tree_index_path(&mut paths, root, path);
            }
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

pub(super) fn parse_lved_tree_index(bytes: &[u8], source: &str) -> Result<Vec<LvedTreeIndexItem>> {
    let text = decode_sqlite_text(bytes);
    let mut items = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r').trim_start_matches('\u{feff}');
        if line.trim().is_empty() {
            continue;
        }
        let mut columns = line.splitn(3, '\t');
        let Some(raw_target) = columns.next() else {
            continue;
        };
        let Some((data_id, query)) = parse_lved_tree_target(raw_target) else {
            continue;
        };
        let Some(level) = columns.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let Some(label) = columns.next() else {
            continue;
        };
        items.push(LvedTreeIndexItem {
            source: source.to_owned(),
            raw_target: raw_target.trim().to_owned(),
            data_id,
            query,
            level,
            label: label.to_owned(),
        });
    }
    Ok(items)
}

pub(super) fn parse_lved_tree_target(value: &str) -> Option<(i64, Option<String>)> {
    let stripped = value.trim();
    let (target, query) = match stripped.split_once('?') {
        Some((target, query)) => (target, Some(query.to_owned())),
        None => (stripped, None),
    };
    if target.is_empty()
        || !target
            .bytes()
            .all(|byte| byte == b'-' || byte.is_ascii_digit())
    {
        return None;
    }
    target.parse::<i64>().ok().map(|value| (value, query))
}

pub(super) fn is_lved_text_tree_index(bytes: &[u8]) -> bool {
    let Some((_, text)) = decode_retained_text(bytes) else {
        return false;
    };
    for line in text.lines() {
        let line = line.trim_end_matches('\r').trim_start_matches('\u{feff}');
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let Some(first) = parts.next().map(str::trim) else {
            return false;
        };
        let Some(second) = parts.next().map(str::trim) else {
            return false;
        };
        if is_eight_digit_hex(first) && is_eight_digit_hex(second) {
            return false;
        }
        return parse_lved_tree_target(first).is_some() && second.parse::<u32>().is_ok();
    }
    false
}

pub(super) fn decode_retained_text(bytes: &[u8]) -> Option<(&'static str, String)> {
    if let Ok(value) = std::str::from_utf8(bytes) {
        return Some(("utf-8", value.trim_start_matches('\u{feff}').to_owned()));
    }
    let (decoded, _, had_errors) = SHIFT_JIS.decode(bytes);
    (!had_errors).then(|| ("cp932", decoded.into_owned()))
}

fn is_eight_digit_hex(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn push_lved_tree_index_path(paths: &mut Vec<PathBuf>, package_root: &Path, path: PathBuf) {
    if regular_file_inside_root(package_root, &path).unwrap_or(false)
        && !paths.iter().any(|existing| existing == &path)
    {
        paths.push(path);
    }
}

pub(super) fn usable_lved_tree_title(label: &str) -> bool {
    let value = html_to_text(label).trim().to_owned();
    !value.is_empty() && !matches!(value.as_str(), "見出し語索引" | "索引" | "目次")
}

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::crypto::decrypt_logofont_cipher_bytes;
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcmuMapRecord {
    pub stem: String,
    pub start_block: u32,
    pub line_index: u32,
    pub path: PathBuf,
    pub encrypted_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcmuIndex {
    pub directory: PathBuf,
    pub map_path: PathBuf,
    pub rows: Vec<PcmuMapRecord>,
}

impl PcmuIndex {
    pub fn record_for_start_block(&self, start_block: u32) -> Option<&PcmuMapRecord> {
        self.rows.iter().find(|row| row.start_block == start_block)
    }
}

pub fn load_pcmu_index(package_root: &Path) -> Result<Option<PcmuIndex>> {
    let Some(directory) = find_loose_media_dir(package_root, "_PCM_U", true)? else {
        return Ok(None);
    };
    let Some(map_path) = find_child_casefolded(&directory, "WaveFile.map")? else {
        return Ok(None);
    };
    let text = fs::read_to_string(&map_path)?;
    let mut rows = Vec::new();
    for (line_index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(stem), Some(block), None) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let Ok(start_block) = block.parse::<u32>() else {
            continue;
        };
        let Some(path) = find_child_casefolded(&directory, stem)? else {
            continue;
        };
        if !path.is_file() {
            continue;
        }
        rows.push(PcmuMapRecord {
            stem: stem.to_owned(),
            start_block,
            line_index: line_index as u32 + 1,
            encrypted_bytes: path.metadata()?.len(),
            path,
        });
    }
    if rows.is_empty() {
        return Ok(None);
    }
    Ok(Some(PcmuIndex {
        directory,
        map_path,
        rows,
    }))
}

pub fn resolve_pcmu_record(package_root: &Path, start_block: u32) -> Result<Option<PcmuMapRecord>> {
    Ok(load_pcmu_index(package_root)?
        .and_then(|index| index.record_for_start_block(start_block).cloned()))
}

pub fn read_pcmu_record(package_root: &Path, start_block: u32) -> Result<Option<Vec<u8>>> {
    let Some(record) = resolve_pcmu_record(package_root, start_block)? else {
        return Ok(None);
    };
    decrypt_logofont_cipher_bytes(&fs::read(record.path)?).map(Some)
}

pub fn find_movie_file(package_root: &Path, movie_id: &str) -> Result<Option<PathBuf>> {
    if movie_id.len() != 8 || !movie_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(None);
    }
    let Some(directory) = find_loose_media_dir(package_root, "_MOVIE", false)? else {
        return Ok(None);
    };
    Ok(find_child_casefolded(&directory, movie_id)?.filter(|path| path.is_file()))
}

fn find_loose_media_dir(
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

fn find_child_casefolded(directory: &Path, name: &str) -> Result<Option<PathBuf>> {
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

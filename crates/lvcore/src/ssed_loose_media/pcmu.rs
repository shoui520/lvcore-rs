use std::fs;
use std::path::{Path, PathBuf};

use crate::crypto::decrypt_logofont_cipher_bytes;
use crate::error::Result;

use super::{find_child_casefolded, find_loose_media_dir};

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

use std::path::Path;

use chmlib::{ChmFile, Continuation, Filter};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChmEntry {
    pub path: String,
    pub length: u64,
}

pub fn list_chm_entries(path: &Path) -> Result<Vec<ChmEntry>> {
    let mut chm = open_chm(path)?;
    let mut entries = Vec::new();
    chm.for_each(Filter::NORMAL | Filter::FILES, |_chm, unit| {
        if !unit.is_file() || !unit.is_normal() {
            return Continuation::Continue;
        }
        let Some(path) = unit.path() else {
            return Continuation::Continue;
        };
        let path = normalize_chm_entry_path(&path.to_string_lossy());
        if path.is_empty() {
            return Continuation::Continue;
        }
        entries.push(ChmEntry {
            path,
            length: unit.length(),
        });
        Continuation::Continue
    })
    .map_err(|err| Error::Driver(format!("failed to enumerate CHM entries: {err}")))?;
    entries.sort_by_key(|entry| entry.path.to_lowercase());
    entries.dedup_by(|a, b| a.path.eq_ignore_ascii_case(&b.path));
    Ok(entries)
}

pub fn read_chm_entry(path: &Path, entry_path: &str) -> Result<Vec<u8>> {
    let mut chm = open_chm(path)?;
    let lookup = format!("/{}", normalize_chm_entry_path(entry_path));
    let Some(unit) = chm.find(&lookup) else {
        return Err(Error::Driver(format!(
            "CHM entry not found: {}",
            normalize_chm_entry_path(entry_path)
        )));
    };
    let length = usize::try_from(unit.length())
        .map_err(|_| Error::Driver(format!("CHM entry is too large: {entry_path}")))?;
    if length > 64 * 1024 * 1024 {
        return Err(Error::Driver(format!(
            "CHM entry exceeds reader safety limit: {entry_path}"
        )));
    }
    let mut data = vec![0; length];
    let read = chm
        .read(&unit, 0, &mut data)
        .map_err(|err| Error::Driver(format!("failed to read CHM entry {entry_path}: {err}")))?;
    if read != length {
        return Err(Error::Driver(format!(
            "short CHM read for {entry_path}: expected {length}, got {read}"
        )));
    }
    Ok(data)
}

fn open_chm(path: &Path) -> Result<ChmFile> {
    ChmFile::open(path).map_err(|err| Error::Driver(format!("failed to open CHM file: {err}")))
}

fn normalize_chm_entry_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches('/')
        .split('/')
        .filter(|part| !part.is_empty() && *part != "." && *part != "..")
        .collect::<Vec<_>>()
        .join("/")
}

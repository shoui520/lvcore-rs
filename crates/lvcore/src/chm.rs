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
    let normalized = normalize_chm_entry_path(entry_path);
    for candidate in chm_entry_lookup_candidates(&normalized) {
        let lookup = format!("/{candidate}");
        let Some(unit) = chm.find(&lookup) else {
            continue;
        };
        let length = usize::try_from(unit.length())
            .map_err(|_| Error::Driver(format!("CHM entry is too large: {candidate}")))?;
        if length > 64 * 1024 * 1024 {
            return Err(Error::Driver(format!(
                "CHM entry exceeds reader safety limit: {candidate}"
            )));
        }
        let mut data = vec![0; length];
        let read = chm
            .read(&unit, 0, &mut data)
            .map_err(|err| Error::Driver(format!("failed to read CHM entry {candidate}: {err}")))?;
        if read != length {
            return Err(Error::Driver(format!(
                "short CHM read for {candidate}: expected {length}, got {read}"
            )));
        }
        return Ok(data);
    }
    Err(Error::Driver(format!("CHM entry not found: {normalized}")))
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

fn chm_entry_lookup_candidates(normalized_path: &str) -> Vec<String> {
    let mut candidates = vec![normalized_path.to_owned()];
    let parts = normalized_path.split('/').collect::<Vec<_>>();
    let Some(first_dir) = parts.first().copied().filter(|part| !part.is_empty()) else {
        return candidates;
    };
    let Some(file_name) = parts.last().copied() else {
        return candidates;
    };

    match file_name.to_ascii_lowercase().as_str() {
        "font.js" => push_unique_candidate(&mut candidates, format!("{first_dir}/font.js")),
        "css.css" => push_unique_candidate(&mut candidates, format!("{first_dir}/contents.css")),
        _ => {}
    }

    candidates
}

fn push_unique_candidate(candidates: &mut Vec<String>, candidate: String) {
    if !candidates
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&candidate))
    {
        candidates.push(candidate);
    }
}

#[cfg(test)]
mod tests {
    use super::chm_entry_lookup_candidates;

    #[test]
    fn chm_lookup_candidates_include_observed_shared_hanrei_assets() {
        assert_eq!(
            chm_entry_lookup_candidates("Source/text/font.js"),
            vec!["Source/text/font.js", "Source/font.js"]
        );
        assert_eq!(
            chm_entry_lookup_candidates("Source/contents/font.js"),
            vec!["Source/contents/font.js", "Source/font.js"]
        );
        assert_eq!(
            chm_entry_lookup_candidates("Source/css.css"),
            vec!["Source/css.css", "Source/contents.css"]
        );
    }
}

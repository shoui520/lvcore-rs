use std::path::Path;

use encoding_rs::SHIFT_JIS;
use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::render::{HcRendererProfile, HcRendererProfileSource, HcRendererProfileStatus};
use crate::storage::StorageBackend;

pub(super) fn hc_renderer_profile(
    storage: &impl StorageBackend,
) -> Result<Option<HcRendererProfile>> {
    let mut dlls = Vec::new();
    for path in storage.list_dir(Path::new(""))? {
        let Some(name) = path.file_name().map(|value| value.to_string_lossy()) else {
            continue;
        };
        let upper = name.to_ascii_uppercase();
        if upper.len() == "HC0000.DLL".len()
            && upper.starts_with("HC")
            && upper.ends_with(".DLL")
            && upper[2..6].chars().all(|ch| ch.is_ascii_hexdigit())
        {
            dlls.push((upper.trim_end_matches(".DLL").to_owned(), name.to_string()));
        }
    }
    dlls.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    if let Some((profile_id, filename)) = dlls.into_iter().next() {
        let bytes = storage.read(Path::new(&filename))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        return Ok(Some(HcRendererProfile {
            profile_id,
            source: HcRendererProfileSource::HcDll,
            status: HcRendererProfileStatus::InputOnly,
            dll_sha256: Some(hex::encode(hasher.finalize())),
            dll_size: Some(bytes.len() as u64),
        }));
    }
    exinfo_hc_renderer_profile(storage)
}

fn exinfo_hc_renderer_profile(storage: &impl StorageBackend) -> Result<Option<HcRendererProfile>> {
    let relative = Path::new("EXINFO.INI");
    if !storage.exists(relative)? {
        return Ok(None);
    }
    let bytes = storage.read(relative)?;
    let (text, _, _) = SHIFT_JIS.decode(&bytes);
    let mut in_general = false;
    for raw_line in text.lines() {
        let line = raw_line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_general = line[1..line.len() - 1]
                .trim()
                .eq_ignore_ascii_case("GENERAL");
            continue;
        }
        if !in_general {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("HTMLDLL")
            && let Some(hint) = extract_hc_profile_hint(value)
        {
            return Ok(Some(HcRendererProfile {
                profile_id: hint,
                source: HcRendererProfileSource::ExinfoHtmlDll,
                status: HcRendererProfileStatus::InputOnly,
                dll_sha256: None,
                dll_size: None,
            }));
        }
    }
    Ok(None)
}

fn extract_hc_profile_hint(value: &str) -> Option<String> {
    let upper = value.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    for offset in 0..bytes.len().saturating_sub(5) {
        if bytes[offset] != b'H' || bytes[offset + 1] != b'C' {
            continue;
        }
        let code = &upper[offset + 2..offset + 6];
        if code.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Some(format!("HC{code}"));
        }
    }
    None
}

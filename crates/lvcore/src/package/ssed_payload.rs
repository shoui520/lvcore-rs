use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use super::ssed_zip::{looks_like_zip_file, ssed_component_filename_aliases};
use crate::crypto::{
    decrypt_android_diw_prefix, decrypt_logofont_cipher_prefix,
    decrypt_macos_logofont_cipher_prefix, normalize_android_wrapped_sseddata_bytes,
};
use crate::error::Result;
use crate::ssed::{SSEDDATA_MAGIC, SsedComponent, SsedDataFile, SsedDataHeader, SsedDataReader};
use crate::storage::{DirectoryStorage, StorageBackend};

type PrefixDecryptFn = fn(&[u8], usize) -> Result<Vec<u8>>;

pub(super) fn read_ssed_navigation_detection_bytes(
    path: &Path,
    max_bytes: usize,
) -> Result<Option<Vec<u8>>> {
    if let Ok(mut reader) = SsedDataFile::open(path) {
        let read_len = reader.header().expanded_size().min(max_bytes);
        return Ok(Some(reader.read_range(0, read_len)?));
    }

    if file_starts_with_android_wrapped_sseddata(path).unwrap_or(false) {
        let raw = fs::read(path)?;
        let normalized = normalize_android_wrapped_sseddata_bytes(&raw);
        let reader = SsedDataReader::parse_bytes(&normalized)?;
        let read_len = reader.header().expanded_size().min(max_bytes);
        return Ok(Some(reader.read(0, read_len).to_vec()));
    }

    Ok(None)
}

pub(super) fn has_component_payload_casefolded(
    storage: &DirectoryStorage,
    component: &SsedComponent,
) -> bool {
    storage
        .exists(Path::new(&component.filename))
        .unwrap_or(false)
        || ssed_component_filename_aliases(component)
            .iter()
            .any(|alias| storage.exists(Path::new(alias)).unwrap_or(false))
}

pub(super) fn has_supported_sseddata_component_payload_casefolded(
    storage: &DirectoryStorage,
    component: &SsedComponent,
) -> bool {
    let mut candidates = Vec::new();
    if let Ok(Some(path)) = storage.resolve_casefolded(Path::new(&component.filename)) {
        candidates.push(path);
    }
    for alias in ssed_component_filename_aliases(component) {
        if let Ok(Some(path)) = storage.resolve_casefolded(Path::new(&alias)) {
            candidates.push(path);
        }
    }
    candidates
        .iter()
        .any(|path| is_supported_sseddata_payload_path(path).unwrap_or(false))
}

fn is_supported_sseddata_payload_path(path: &Path) -> Result<bool> {
    if SsedDataHeader::parse_file(path).is_ok() {
        return Ok(true);
    }
    if file_starts_with_android_wrapped_sseddata(path)? {
        return Ok(true);
    }
    if looks_like_zip_file(path)? {
        return Ok(true);
    }

    let mut file = File::open(path)?;
    let mut prefix = vec![0_u8; 4096];
    let read = file.read(&mut prefix)?;
    prefix.truncate(read);
    if prefix.len() < 16 {
        return Ok(false);
    }
    for prefix_decrypt in [
        decrypt_android_diw_prefix as PrefixDecryptFn,
        decrypt_macos_logofont_cipher_prefix,
        decrypt_logofont_cipher_prefix,
    ] {
        if prefix_decrypt(&prefix, 64).is_ok_and(|decrypted| decrypted.starts_with(SSEDDATA_MAGIC))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn file_starts_with_android_wrapped_sseddata(path: &Path) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut prefix = [0_u8; 11];
    let read = file.read(&mut prefix)?;
    Ok(read == prefix.len() && &prefix == b"LV_SSEDDATA")
}

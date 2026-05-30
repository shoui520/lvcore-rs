use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::ssed_zip::ssed_component_filename_aliases;
use crate::crypto::{
    decrypt_android_diw_file_to_path, decrypt_android_diw_prefix,
    decrypt_logofont_cipher_file_to_path, decrypt_logofont_cipher_prefix,
    decrypt_macos_logofont_cipher_file_to_path, decrypt_macos_logofont_cipher_prefix,
    normalize_android_wrapped_sseddata_file_to_path,
};
use crate::error::Result;
use crate::ssed::{
    SSEDDATA_MAGIC, SsedCatalog, SsedComponent, SsedComponentRole, SsedDataFile, SsedDataHeader,
};
use crate::ssed_index::{
    INDEX_PAGE_SIZE, SsedIndexScanState, is_leaf_page, is_supported_index_type,
    parse_supported_leaf_page,
};
use crate::storage::{DirectoryStorage, StorageBackend, private_cache_dir};

type PrefixDecryptFn = fn(&[u8], usize) -> Result<Vec<u8>>;
type FileDecryptFn = fn(&Path, &Path) -> Result<()>;

pub(super) fn has_decodable_ssed_index_rows(
    catalog: &SsedCatalog,
    storage: &DirectoryStorage,
) -> bool {
    catalog
        .components_by_role(SsedComponentRole::Index)
        .filter(|component| {
            component.has_positive_range() && is_supported_index_type(component.component_type)
        })
        .any(|component| {
            index_component_has_decodable_target_row(catalog, storage, component).unwrap_or(false)
        })
}

fn index_component_has_decodable_target_row(
    catalog: &SsedCatalog,
    storage: &DirectoryStorage,
    component: &SsedComponent,
) -> Result<bool> {
    for path in component_candidate_paths_casefolded(storage, component)? {
        let Some(readable) = materialize_readable_component_for_probe(storage, component, &path)?
        else {
            continue;
        };
        if index_file_has_decodable_target_row(catalog, component, &readable)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn index_file_has_decodable_target_row(
    catalog: &SsedCatalog,
    component: &SsedComponent,
    path: &Path,
) -> Result<bool> {
    let mut reader = SsedDataFile::open(path)?;
    let page_count = component.block_count() as usize;
    let mut scan_state = SsedIndexScanState::default();
    for page_index in 0..page_count {
        let page = reader.read_range(page_index * INDEX_PAGE_SIZE, INDEX_PAGE_SIZE)?;
        if page.len() < 4 {
            break;
        }
        let word = u16::from_be_bytes([page[0], page[1]]);
        if !is_leaf_page(word) {
            continue;
        }
        let logical_block = component.start_block + page_index as u32;
        let (rows, _unknown) = parse_supported_leaf_page(
            &component.filename,
            component.component_type,
            &page,
            page_index as u32,
            logical_block,
            &mut scan_state,
        );
        if rows
            .iter()
            .any(|row| catalog.component_for_address(row.body.block).is_some())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn component_candidate_paths_casefolded(
    storage: &DirectoryStorage,
    component: &SsedComponent,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(path) = storage.resolve_casefolded(Path::new(&component.filename))? {
        seen.insert(path.clone());
        paths.push(path);
    }
    for alias in ssed_component_filename_aliases(component) {
        if let Some(path) = storage.resolve_casefolded(Path::new(&alias))?
            && seen.insert(path.clone())
        {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn materialize_readable_component_for_probe(
    storage: &DirectoryStorage,
    component: &SsedComponent,
    path: &Path,
) -> Result<Option<PathBuf>> {
    if SsedDataHeader::parse_file(path).is_ok() {
        return Ok(Some(path.to_path_buf()));
    }

    if file_starts_with_android_wrapped_sseddata(path)? {
        let cache_path =
            component_probe_cache_path(storage, component, path, "android_lved_wrapped", "dic")?;
        if SsedDataHeader::parse_file(&cache_path).is_ok() {
            return Ok(Some(cache_path));
        }
        let tmp_path = cache_path.with_extension("tmp");
        if tmp_path.exists() {
            fs::remove_file(&tmp_path)?;
        }
        normalize_android_wrapped_sseddata_file_to_path(path, &tmp_path)?;
        SsedDataHeader::parse_file(&tmp_path)?;
        fs::rename(&tmp_path, &cache_path)?;
        return Ok(Some(cache_path));
    }

    let mut file = File::open(path)?;
    let mut prefix = vec![0_u8; 4096];
    let read = file.read(&mut prefix)?;
    prefix.truncate(read);
    if prefix.len() < 16 {
        return Ok(None);
    }
    let attempts: [(&str, PrefixDecryptFn, FileDecryptFn); 3] = [
        (
            "android_honmon_diw",
            decrypt_android_diw_prefix,
            decrypt_android_diw_file_to_path,
        ),
        (
            "macos_logofont_cipher",
            decrypt_macos_logofont_cipher_prefix,
            decrypt_macos_logofont_cipher_file_to_path,
        ),
        (
            "logofont_cipher",
            decrypt_logofont_cipher_prefix,
            decrypt_logofont_cipher_file_to_path,
        ),
    ];
    for (name, prefix_decrypt, file_decrypt) in attempts {
        let decrypted_prefix = prefix_decrypt(&prefix, prefix.len())?;
        if !decrypted_prefix.starts_with(SSEDDATA_MAGIC) {
            continue;
        }
        let cache_path = component_probe_cache_path(storage, component, path, name, "dic")?;
        if SsedDataHeader::parse_file(&cache_path).is_ok() {
            return Ok(Some(cache_path));
        }
        let tmp_path = cache_path.with_extension("tmp");
        if tmp_path.exists() {
            fs::remove_file(&tmp_path)?;
        }
        file_decrypt(path, &tmp_path)?;
        SsedDataHeader::parse_file(&tmp_path)?;
        fs::rename(&tmp_path, &cache_path)?;
        return Ok(Some(cache_path));
    }

    Ok(None)
}

fn component_probe_cache_path(
    storage: &DirectoryStorage,
    component: &SsedComponent,
    source: &Path,
    stage: &str,
    extension: &str,
) -> Result<PathBuf> {
    let metadata = fs::metadata(source)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(storage.root().as_os_str().to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(component.filename.as_bytes());
    hasher.update(b"\0");
    hasher.update(stage.as_bytes());
    hasher.update(b"\0");
    hasher.update(source.as_os_str().to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(metadata.len().to_le_bytes());
    hasher.update(modified.to_le_bytes());
    let hash = hex::encode(hasher.finalize());
    let dir = private_cache_dir("ssed-component-probes")?;
    Ok(dir.join(format!("{hash}.{extension}")))
}

fn file_starts_with_android_wrapped_sseddata(path: &Path) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut prefix = [0_u8; 11];
    let read = file.read(&mut prefix)?;
    Ok(read == prefix.len() && &prefix == b"LV_SSEDDATA")
}

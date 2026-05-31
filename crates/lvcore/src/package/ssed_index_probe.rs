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
use crate::storage::{
    DirectoryStorage, StorageBackend, private_cache_dir, regular_file_inside_root,
};

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
    if let Some(path) = storage.resolve_casefolded(Path::new(&component.filename))?
        && regular_file_inside_root(storage.root(), &path)?
    {
        seen.insert(path.clone());
        paths.push(path);
    }
    for alias in ssed_component_filename_aliases(component) {
        if let Some(path) = storage.resolve_casefolded(Path::new(&alias))?
            && regular_file_inside_root(storage.root(), &path)?
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::ssed::BLOCK_SIZE;

    use super::*;

    #[cfg(unix)]
    #[test]
    fn index_probe_ignores_symlinked_component_escape() {
        use std::os::unix::fs::symlink;

        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let page = simple_leaf_index_page_for_test(b"alpha", 100, 0);
        fs::write(
            outside.path().join("FHINDEX.DIC"),
            fixture_sseddata_literal_chunks(&[&page], 200, 200),
        )
        .unwrap();
        symlink(
            outside.path().join("FHINDEX.DIC"),
            root.path().join("FHINDEX.DIC"),
        )
        .unwrap();
        let catalog = SsedCatalog {
            title: "Symlink".to_owned(),
            components: vec![
                SsedComponent {
                    index: 0,
                    multi: 0,
                    component_type: 0x00,
                    start_block: 100,
                    end_block: 100,
                    data: [0; 4],
                    filename: "HONMON.DIC".to_owned(),
                    role: SsedComponentRole::Honmon,
                },
                SsedComponent {
                    index: 1,
                    multi: 0,
                    component_type: 0x91,
                    start_block: 200,
                    end_block: 200,
                    data: [0; 4],
                    filename: "FHINDEX.DIC".to_owned(),
                    role: SsedComponentRole::Index,
                },
            ],
            layout: crate::ssed::SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 2,
                trailing_bytes: 0,
            },
        };
        let storage = DirectoryStorage::new(root.path());

        assert!(!has_decodable_ssed_index_rows(&catalog, &storage));
    }

    fn simple_leaf_index_page_for_test(key: &[u8], body_block: u32, body_offset: u16) -> Vec<u8> {
        let mut page = vec![0_u8; BLOCK_SIZE as usize];
        page[0..2].copy_from_slice(&0xc000_u16.to_be_bytes());
        page[2..4].copy_from_slice(&1_u16.to_be_bytes());
        let mut pos = 4usize;
        page[pos] = key.len() as u8;
        pos += 1;
        page[pos..pos + key.len()].copy_from_slice(key);
        pos += key.len();
        page[pos..pos + 4].copy_from_slice(&body_block.to_be_bytes());
        pos += 4;
        page[pos..pos + 2].copy_from_slice(&body_offset.to_be_bytes());
        pos += 2;
        page[pos..pos + 4].copy_from_slice(&0_u32.to_be_bytes());
        pos += 4;
        page[pos..pos + 2].copy_from_slice(&0_u16.to_be_bytes());
        page
    }

    fn fixture_sseddata_literal_chunks(
        chunks: &[&[u8]],
        start_block: u32,
        end_block: u32,
    ) -> Vec<u8> {
        let chunk_count = chunks.len();
        let first_chunk_offset = 0x40 + chunk_count * 4;
        let mut data = vec![0u8; first_chunk_offset];
        data[..8].copy_from_slice(SSEDDATA_MAGIC);
        data[0x0f] = 1;
        data[0x16..0x18].copy_from_slice(&(chunk_count as u16).to_be_bytes());
        data[0x18..0x1c].copy_from_slice(&start_block.to_be_bytes());
        data[0x1c..0x20].copy_from_slice(&end_block.to_be_bytes());

        let mut compressed_chunks = Vec::with_capacity(chunk_count);
        let mut next_offset = first_chunk_offset;
        for (index, chunk) in chunks.iter().enumerate() {
            data[0x40 + index * 4..0x44 + index * 4]
                .copy_from_slice(&(next_offset as u32).to_be_bytes());
            let compressed = fixture_sseddata_literal_chunk(chunk);
            next_offset += compressed.len();
            compressed_chunks.push(compressed);
        }
        for compressed in compressed_chunks {
            data.extend_from_slice(&compressed);
        }
        data
    }

    fn fixture_sseddata_literal_chunk(literals: &[u8]) -> Vec<u8> {
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&[0, 0]);
        chunk.extend_from_slice(&(literals.len() as u16).to_be_bytes());
        chunk.push(0);
        for literal in literals {
            chunk.extend_from_slice(&[0, 0, *literal]);
        }
        chunk
    }
}

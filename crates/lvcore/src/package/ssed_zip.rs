use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use zip::ZipArchive;
use zip::result::ZipError;

use crate::error::{Error, Result};
use crate::ssed::{BLOCK_SIZE, SsedComponent, SsedComponentRole};

const MIN_ZIPPED_SSED_COMPONENT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ZIPPED_SSED_COMPONENT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const ZIPPED_SSED_COMPONENT_OVERHEAD_BYTES: u64 = 16 * 1024 * 1024;
const ZIPPED_SSED_COMPONENT_EXPANSION_FACTOR: u64 = 4;

pub(super) fn looks_like_zip_file(path: &Path) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut magic = [0_u8; 4];
    let read = file.read(&mut magic)?;
    Ok(read == magic.len() && magic == *b"PK\x03\x04")
}

pub(super) fn zip_member_name_for_component(
    component: &SsedComponent,
    zip_path: &Path,
) -> Result<Option<String>> {
    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(file).map_err(zip_error)?;
    let mut desired = BTreeSet::new();
    desired.insert(component.filename.to_ascii_lowercase());
    for alias in ssed_component_filename_aliases(component) {
        desired.insert(alias.to_ascii_lowercase());
    }
    for index in 0..archive.len() {
        let member = archive.by_index_raw(index).map_err(zip_error)?;
        let name = member.name().replace('\\', "/");
        if desired.contains(&name.to_ascii_lowercase()) {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

pub(super) fn zip_error(error: ZipError) -> Error {
    Error::Driver(format!("ZIP decode error: {error}"))
}

pub(super) fn zipped_ssed_component_size_limit(
    component: &SsedComponent,
    member_name: &str,
    declared_member_size: u64,
) -> Result<u64> {
    let declared_component_bytes =
        u64::from(component.block_count()).saturating_mul(u64::from(BLOCK_SIZE));
    let component_limit = declared_component_bytes
        .saturating_mul(ZIPPED_SSED_COMPONENT_EXPANSION_FACTOR)
        .saturating_add(ZIPPED_SSED_COMPONENT_OVERHEAD_BYTES)
        .clamp(
            MIN_ZIPPED_SSED_COMPONENT_BYTES,
            MAX_ZIPPED_SSED_COMPONENT_BYTES,
        );
    if declared_member_size > component_limit {
        return Err(Error::Driver(format!(
            "ZIP member {member_name} expands to {declared_member_size} bytes, exceeding the {component_limit} byte limit for {}",
            component.filename
        )));
    }
    Ok(component_limit)
}

pub(super) fn copy_zip_member_with_size_limit<R: Read>(
    member: &mut R,
    outfile: &mut File,
    size_limit: u64,
) -> Result<()> {
    let mut limited = member.take(size_limit.saturating_add(1));
    let copied = std::io::copy(&mut limited, outfile)?;
    if copied > size_limit {
        return Err(Error::Driver(format!(
            "ZIP member exceeded {size_limit} byte extraction limit"
        )));
    }
    Ok(())
}

pub(super) fn ssed_component_filename_aliases(component: &SsedComponent) -> Vec<String> {
    if component.role != SsedComponentRole::Honmon {
        return Vec::new();
    }
    let upper = component.filename.to_ascii_uppercase();
    if !matches!(
        upper.as_str(),
        "HONMON" | "HONMON.DIC" | "HONMON.DIN" | "HONMON.DIW"
    ) {
        return Vec::new();
    }
    ["HONMON", "HONMON.DIC", "HONMON.DIN", "HONMON.DIW"]
        .into_iter()
        .filter(|alias| !alias.eq_ignore_ascii_case(&component.filename))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn zipped_ssed_component_size_limit_rejects_declared_zip_bombs() {
        let component = SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0,
            start_block: 1,
            end_block: 1,
            data: [0; 4],
            filename: "HONMON.DIN".to_owned(),
            role: SsedComponentRole::Honmon,
        };

        let limit = zipped_ssed_component_size_limit(&component, "HONMON.DIN", 1024).unwrap();
        assert_eq!(limit, MIN_ZIPPED_SSED_COMPONENT_BYTES);
        let error =
            zipped_ssed_component_size_limit(&component, "HONMON.DIN", limit + 1).unwrap_err();
        assert!(error.to_string().contains("exceeding"));
    }

    #[test]
    fn zipped_ssed_component_copy_stops_when_member_lies_about_size() {
        let mut member = Cursor::new(vec![0x41; 17]);
        let dir = tempdir().unwrap();
        let mut out = File::create(dir.path().join("member.bin")).unwrap();

        let error = copy_zip_member_with_size_limit(&mut member, &mut out, 16).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("exceeded 16 byte extraction limit")
        );
    }
}

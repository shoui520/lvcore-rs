use std::cmp::Reverse;
use std::path::Path;

use encoding_rs::SHIFT_JIS;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const BLOCK_SIZE: u32 = 2048;
pub const SSEDINFO_MAGIC: &[u8; 8] = b"SSEDINFO";
pub const SSEDDATA_MAGIC: &[u8; 8] = b"SSEDDATA";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SsedComponentRole {
    Honmon,
    Menu,
    Right,
    Title,
    Index,
    Toc,
    IdxJump,
    MultiDescriptor,
    Colscr,
    PcmData,
    Figure,
    GaijiFull,
    GaijiHalf,
    Resource,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedComponent {
    pub index: u8,
    pub multi: u8,
    pub component_type: u8,
    pub start_block: u32,
    pub end_block: u32,
    pub data: [u8; 4],
    pub filename: String,
    pub role: SsedComponentRole,
}

impl SsedComponent {
    pub fn block_count(&self) -> u32 {
        if self.start_block == 0 && self.end_block == 0 {
            0
        } else {
            self.end_block.saturating_sub(self.start_block) + 1
        }
    }

    pub fn has_positive_range(&self) -> bool {
        self.block_count() > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedInfoLayout {
    pub component_count_offset: usize,
    pub record_start: usize,
    pub record_size: usize,
    pub component_count: u8,
    pub trailing_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedCatalog {
    pub title: String,
    pub components: Vec<SsedComponent>,
    pub layout: SsedInfoLayout,
}

impl SsedCatalog {
    pub fn parse_file(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::parse_bytes(&bytes)
    }

    pub fn parse_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < SSEDINFO_MAGIC.len() || &data[..8] != SSEDINFO_MAGIC {
            return Err(Error::Driver("not SSEDINFO".to_owned()));
        }
        let title_len = data.get(0x0c).copied().unwrap_or(0) as usize;
        let title_end = 0x0dusize.saturating_add(title_len).min(data.len());
        let title_bytes = &data[0x0d..title_end];
        let title = decode_cp932_lossy(split_nul(title_bytes));

        let mut parsed = Vec::new();
        for (component_count_offset, record_start) in
            [(0x4d, 0x80), (0x4c, 0x7f), (0x4c, 0x80), (0x4d, 0x7f)]
        {
            if let Ok((components, layout, score)) =
                parse_records(data, component_count_offset, record_start)
            {
                parsed.push((
                    score,
                    usize::from(component_count_offset == 0x4d && record_start == 0x80),
                    components,
                    layout,
                ));
            }
        }
        parsed.sort_by_key(|item| Reverse((item.0, item.1)));
        let Some((score, _preferred, components, layout)) = parsed.into_iter().next() else {
            return Err(Error::Driver(
                "could not parse SSEDINFO component records".to_owned(),
            ));
        };
        if score != usize::from(layout.component_count) {
            return Err(Error::Driver(
                "could not identify SSEDINFO component filename layout".to_owned(),
            ));
        }
        Ok(Self {
            title,
            components,
            layout,
        })
    }

    pub fn components_by_role(
        &self,
        role: SsedComponentRole,
    ) -> impl Iterator<Item = &SsedComponent> {
        self.components
            .iter()
            .filter(move |component| component.role == role)
    }

    pub fn has_role(&self, role: SsedComponentRole) -> bool {
        self.components_by_role(role)
            .any(SsedComponent::has_positive_range)
    }

    pub fn honmon(&self) -> Option<&SsedComponent> {
        self.components
            .iter()
            .find(|component| component.role == SsedComponentRole::Honmon)
    }
}

fn parse_records(
    data: &[u8],
    component_count_offset: usize,
    record_start: usize,
) -> Result<(Vec<SsedComponent>, SsedInfoLayout, usize)> {
    const RECORD_SIZE: usize = 0x30;
    let Some(&component_count) = data.get(component_count_offset) else {
        return Err(Error::Driver(
            "component count offset outside SSEDINFO".to_owned(),
        ));
    };
    if component_count == 0 {
        return Err(Error::Driver("empty SSEDINFO component table".to_owned()));
    }
    let end = record_start.saturating_add(usize::from(component_count) * RECORD_SIZE);
    if end > data.len() {
        return Err(Error::Driver(
            "component records outside SSEDINFO".to_owned(),
        ));
    }

    let mut components = Vec::with_capacity(usize::from(component_count));
    let mut valid_filenames = 0usize;
    for index in 0..component_count {
        let pos = record_start + usize::from(index) * RECORD_SIZE;
        let rec = &data[pos..pos + RECORD_SIZE];
        let (filename, valid_filename) = decode_component_filename(rec);
        valid_filenames += usize::from(valid_filename);
        let component_type = rec[3];
        components.push(SsedComponent {
            index,
            multi: rec[2],
            component_type,
            start_block: be32(rec, 4),
            end_block: be32(rec, 8),
            data: [rec[12], rec[13], rec[14], rec[15]],
            filename: filename.clone(),
            role: component_role(component_type, &filename),
        });
    }

    let layout = SsedInfoLayout {
        component_count_offset,
        record_start,
        record_size: RECORD_SIZE,
        component_count,
        trailing_bytes: data.len() - end,
    };
    Ok((components, layout, valid_filenames))
}

fn decode_component_filename(rec: &[u8]) -> (String, bool) {
    if rec.len() < 0x11 {
        return (String::new(), false);
    }
    let length = rec[0x10] as usize;
    if (1..=rec.len() - 0x11).contains(&length) {
        let raw = &rec[0x11..0x11 + length];
        if is_ascii_filename(raw) {
            return (String::from_utf8_lossy(raw).into_owned(), true);
        }
    }
    let raw = split_nul(&rec[0x11..]);
    (
        String::from_utf8_lossy(raw).into_owned(),
        is_ascii_filename(raw),
    )
}

fn component_role(component_type: u8, filename: &str) -> SsedComponentRole {
    let upper = filename.to_ascii_uppercase();
    if matches!(upper.as_str(), "HONMON.DIC" | "HONMON.DIN" | "HONMON") {
        return SsedComponentRole::Honmon;
    }
    match component_type {
        0x00 => SsedComponentRole::Honmon,
        0x01 => SsedComponentRole::Menu,
        0x02 => SsedComponentRole::Right,
        0x03 | 0x04 | 0x05 | 0x06 | 0x07 | 0x09 | 0x0a | 0x0d => SsedComponentRole::Title,
        0x20 => SsedComponentRole::Toc,
        0x28 => SsedComponentRole::IdxJump,
        0x30 | 0x60 | 0x70 | 0x71 | 0x72 | 0x80 | 0x81 | 0x90 | 0x91 | 0x92 | 0xa1 => {
            SsedComponentRole::Index
        }
        0xd0 => SsedComponentRole::Figure,
        0xd2 => SsedComponentRole::Colscr,
        0xd8 => SsedComponentRole::PcmData,
        0xf1 => SsedComponentRole::GaijiFull,
        0xf2 => SsedComponentRole::GaijiHalf,
        0xff => SsedComponentRole::MultiDescriptor,
        _ => SsedComponentRole::Unknown,
    }
}

fn decode_cp932_lossy(data: &[u8]) -> String {
    let (decoded, _encoding_used, _had_errors) = SHIFT_JIS.decode(data);
    decoded.into_owned()
}

fn split_nul(data: &[u8]) -> &[u8] {
    let end = data
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(data.len());
    &data[..end]
}

fn is_ascii_filename(data: &[u8]) -> bool {
    !data.is_empty() && data.iter().all(|value| (0x20..0x7f).contains(value))
}

fn be32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_ssedinfo_catalog() {
        let data = fixture_ssedinfo(0x4d, 0x80);
        let catalog = SsedCatalog::parse_bytes(&data).unwrap();
        assert_eq!(catalog.title, "Test Dictionary");
        assert_eq!(catalog.layout.component_count_offset, 0x4d);
        assert_eq!(catalog.components.len(), 3);
        assert_eq!(catalog.honmon().unwrap().filename, "HONMON.DIC");
        assert!(catalog.has_role(SsedComponentRole::Menu));
        assert!(catalog.has_role(SsedComponentRole::Index));
    }

    #[test]
    fn parses_shifted_multiview_facade_layout() {
        let data = fixture_ssedinfo(0x4c, 0x7f);
        let catalog = SsedCatalog::parse_bytes(&data).unwrap();
        assert_eq!(catalog.layout.component_count_offset, 0x4c);
        assert_eq!(catalog.layout.record_start, 0x7f);
    }

    fn fixture_ssedinfo(component_count_offset: usize, record_start: usize) -> Vec<u8> {
        let mut data = vec![0u8; record_start + 3 * 0x30];
        data[..8].copy_from_slice(SSEDINFO_MAGIC);
        let title = b"Test Dictionary";
        data[0x0c] = title.len() as u8;
        data[0x0d..0x0d + title.len()].copy_from_slice(title);
        data[component_count_offset] = 3;
        write_record(
            &mut data[record_start..record_start + 0x30],
            0x00,
            1,
            10,
            "HONMON.DIC",
        );
        write_record(
            &mut data[record_start + 0x30..record_start + 0x60],
            0x01,
            11,
            12,
            "MENU.DIC",
        );
        write_record(
            &mut data[record_start + 0x60..record_start + 0x90],
            0x91,
            13,
            14,
            "FHINDEX.DIC",
        );
        data
    }

    fn write_record(rec: &mut [u8], component_type: u8, start: u32, end: u32, filename: &str) {
        rec[3] = component_type;
        rec[4..8].copy_from_slice(&start.to_be_bytes());
        rec[8..12].copy_from_slice(&end.to_be_bytes());
        rec[0x10] = filename.len() as u8;
        rec[0x11..0x11 + filename.len()].copy_from_slice(filename.as_bytes());
    }
}

use std::cmp::{Reverse, min};
use std::path::Path;

use encoding_rs::SHIFT_JIS;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const BLOCK_SIZE: u32 = 2048;
pub const CHUNK_SIZE: usize = 0x8000;
pub const WINDOW_SIZE: usize = 0x0ff0;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedDataHeader {
    pub kind: u8,
    pub chunk_count: u16,
    pub start_block: u32,
    pub end_block: u32,
    pub chunk_offsets: Vec<u32>,
}

impl SsedDataHeader {
    pub fn expanded_size(&self) -> usize {
        self.end_block
            .saturating_sub(self.start_block)
            .saturating_add(1) as usize
            * BLOCK_SIZE as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsedDataReader {
    header: SsedDataHeader,
    expanded: Vec<u8>,
}

impl SsedDataReader {
    pub fn parse_file(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::parse_bytes(&bytes)
    }

    pub fn parse_bytes(data: &[u8]) -> Result<Self> {
        let header = parse_sseddata_header(data)?;
        let expanded = expand_sseddata_bytes_with_header(data, &header)?;
        Ok(Self { header, expanded })
    }

    pub fn header(&self) -> &SsedDataHeader {
        &self.header
    }

    pub fn expanded(&self) -> &[u8] {
        &self.expanded
    }

    pub fn read(&self, offset: usize, size: usize) -> &[u8] {
        if offset >= self.expanded.len() {
            return &[];
        }
        let end = min(offset.saturating_add(size), self.expanded.len());
        &self.expanded[offset..end]
    }
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

fn be16(data: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([data[offset], data[offset + 1]])
}

pub fn parse_sseddata_header(data: &[u8]) -> Result<SsedDataHeader> {
    if data.len() < 0x40 || &data[..8] != SSEDDATA_MAGIC {
        return Err(Error::Driver("not plain SSEDDATA".to_owned()));
    }
    let chunk_count = be16(data, 0x16);
    let offset_table_end = 0x40usize.saturating_add(usize::from(chunk_count) * 4);
    if offset_table_end > data.len() {
        return Err(Error::Driver(
            "SSEDDATA chunk offset table outside file".to_owned(),
        ));
    }
    let mut chunk_offsets = Vec::with_capacity(usize::from(chunk_count));
    for index in 0..usize::from(chunk_count) {
        chunk_offsets.push(be32(data, 0x40 + index * 4));
    }
    Ok(SsedDataHeader {
        kind: data[0x0f],
        chunk_count,
        start_block: be32(data, 0x18),
        end_block: be32(data, 0x1c),
        chunk_offsets,
    })
}

pub fn expand_sseddata_bytes(data: &[u8]) -> Result<Vec<u8>> {
    let header = parse_sseddata_header(data)?;
    expand_sseddata_bytes_with_header(data, &header)
}

fn expand_sseddata_bytes_with_header(data: &[u8], header: &SsedDataHeader) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(header.expanded_size());
    for &chunk_offset in &header.chunk_offsets {
        let chunk_offset = chunk_offset as usize;
        if chunk_offset >= data.len() {
            return Err(Error::Driver(
                "SSEDDATA chunk offset outside file".to_owned(),
            ));
        }
        out.extend(expand_sseddata_chunk(data, chunk_offset)?);
    }
    if out.len() > header.expanded_size() {
        out.truncate(header.expanded_size());
    }
    Ok(out)
}

pub fn expand_sseddata_chunk(data: &[u8], chunk_offset: usize) -> Result<Vec<u8>> {
    let mut pos = chunk_offset.saturating_add(2);
    if pos + 3 > data.len() {
        return Err(Error::Driver("short SSEDDATA chunk header".to_owned()));
    }
    let command_count = be16(data, pos);
    let init = data[pos + 2];
    pos += 3;

    let mut window = vec![init; WINDOW_SIZE];
    let mut window_top = 0usize;
    let mut chunk_out = Vec::with_capacity(CHUNK_SIZE);

    for command_index in 0..usize::from(command_count) {
        if pos + 3 > data.len() {
            break;
        }
        let b0 = data[pos];
        let b1 = data[pos + 1];
        let literal = data[pos + 2];
        pos += 3;

        let window_offset = (usize::from(b0) << 4) | (usize::from(b1) >> 4);
        let copy_length = usize::from(b1 & 0x0f);

        for _ in 0..copy_length {
            if chunk_out.len() >= CHUNK_SIZE
                || (command_index == usize::from(command_count) - 1
                    && chunk_out.len() % BLOCK_SIZE as usize == 0)
            {
                break;
            }
            let mut window_pos = window_offset + window_top;
            if window_pos >= WINDOW_SIZE {
                window_pos -= WINDOW_SIZE;
            }
            let value = window[window_pos];
            window[window_top] = value;
            window_top = (window_top + 1) % WINDOW_SIZE;
            chunk_out.push(value);
        }

        if chunk_out.len() >= CHUNK_SIZE
            || (command_index == usize::from(command_count) - 1
                && chunk_out.len() % BLOCK_SIZE as usize == 0)
        {
            break;
        }

        window[window_top] = literal;
        window_top = (window_top + 1) % WINDOW_SIZE;
        chunk_out.push(literal);
    }

    Ok(chunk_out)
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

    #[test]
    fn expands_literal_only_sseddata_chunk() {
        let data = fixture_sseddata_literal(b"abc");
        let reader = SsedDataReader::parse_bytes(&data).unwrap();
        assert_eq!(reader.header().chunk_count, 1);
        assert_eq!(reader.header().start_block, 1);
        assert_eq!(reader.expanded(), b"abc");
        assert_eq!(reader.read(1, 2), b"bc");
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

    fn fixture_sseddata_literal(literals: &[u8]) -> Vec<u8> {
        let chunk_offset = 0x44usize;
        let mut data = vec![0u8; chunk_offset];
        data[..8].copy_from_slice(SSEDDATA_MAGIC);
        data[0x0f] = 1;
        data[0x16..0x18].copy_from_slice(&1u16.to_be_bytes());
        data[0x18..0x1c].copy_from_slice(&1u32.to_be_bytes());
        data[0x1c..0x20].copy_from_slice(&1u32.to_be_bytes());
        data[0x40..0x44].copy_from_slice(&(chunk_offset as u32).to_be_bytes());
        data.extend_from_slice(&[0, 0]);
        data.extend_from_slice(&(literals.len() as u16).to_be_bytes());
        data.push(0);
        for literal in literals {
            data.extend_from_slice(&[0, 0, *literal]);
        }
        data
    }
}

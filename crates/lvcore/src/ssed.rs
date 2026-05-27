use std::cmp::{Reverse, min};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

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

    pub fn contains_block(&self, block: u32) -> bool {
        self.has_positive_range() && (self.start_block..=self.end_block).contains(&block)
    }

    pub fn relative_offset(&self, block: u32, offset: u32) -> Option<u64> {
        if !self.contains_block(block) || offset >= BLOCK_SIZE {
            return None;
        }
        let block_delta = u64::from(block - self.start_block);
        Some(block_delta * u64::from(BLOCK_SIZE) + u64::from(offset))
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
    pub fn parse_file(path: &Path) -> Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut fixed_header = [0u8; 0x40];
        file.read_exact(&mut fixed_header)?;
        if &fixed_header[..8] != SSEDDATA_MAGIC {
            return Err(Error::Driver("not plain SSEDDATA".to_owned()));
        }
        let chunk_count = be16(&fixed_header, 0x16);
        let offset_table_len = usize::from(chunk_count) * 4;
        let mut header = Vec::with_capacity(0x40 + offset_table_len);
        header.extend_from_slice(&fixed_header);
        header.resize(0x40 + offset_table_len, 0);
        file.read_exact(&mut header[0x40..])?;
        parse_sseddata_header(&header)
    }

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

#[derive(Debug)]
pub struct SsedDataFile {
    path: PathBuf,
    file: File,
    file_len: u64,
    header: SsedDataHeader,
    cached_chunk_index: Option<usize>,
    cached_chunk: Vec<u8>,
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

impl SsedDataFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let header = SsedDataHeader::parse_file(&path)?;
        let file = File::open(&path)?;
        let file_len = file.metadata()?.len();
        Ok(Self {
            path,
            file,
            file_len,
            header,
            cached_chunk_index: None,
            cached_chunk: Vec::new(),
        })
    }

    pub fn header(&self) -> &SsedDataHeader {
        &self.header
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read_range(&mut self, offset: usize, size: usize) -> Result<Vec<u8>> {
        if size == 0 || offset >= self.header.expanded_size() {
            return Ok(Vec::new());
        }

        let end = min(offset.saturating_add(size), self.header.expanded_size());
        let first_chunk = offset / CHUNK_SIZE;
        let last_chunk = (end - 1) / CHUNK_SIZE;
        let mut out = Vec::with_capacity(end - offset);

        for chunk_index in first_chunk..=last_chunk {
            let expanded = self.read_expanded_chunk(chunk_index)?;
            let chunk_start = chunk_index * CHUNK_SIZE;
            let start_in_chunk = offset.saturating_sub(chunk_start);
            let end_in_chunk = min(end.saturating_sub(chunk_start), expanded.len());
            if start_in_chunk < end_in_chunk {
                out.extend_from_slice(&expanded[start_in_chunk..end_in_chunk]);
            }
        }

        Ok(out)
    }

    fn read_expanded_chunk(&mut self, chunk_index: usize) -> Result<Vec<u8>> {
        if self.cached_chunk_index == Some(chunk_index) {
            return Ok(self.cached_chunk.clone());
        }
        let start = *self
            .header
            .chunk_offsets
            .get(chunk_index)
            .ok_or_else(|| Error::Driver("SSEDDATA chunk index outside header".to_owned()))?
            as u64;
        let end = self
            .header
            .chunk_offsets
            .get(chunk_index + 1)
            .map(|offset| u64::from(*offset))
            .unwrap_or(self.file_len);
        if start >= self.file_len || end < start || end > self.file_len {
            return Err(Error::Driver(
                "SSEDDATA chunk byte range outside file".to_owned(),
            ));
        }
        let size = usize::try_from(end - start)
            .map_err(|_| Error::Driver("SSEDDATA chunk is too large".to_owned()))?;
        let mut bytes = vec![0u8; size];
        self.file.seek(SeekFrom::Start(start))?;
        self.file.read_exact(&mut bytes)?;
        let expanded = expand_sseddata_chunk(&bytes, 0)?;
        self.cached_chunk_index = Some(chunk_index);
        self.cached_chunk = expanded.clone();
        Ok(expanded)
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

    pub fn component_named(&self, name: &str) -> Option<&SsedComponent> {
        self.components
            .iter()
            .find(|component| component.filename.eq_ignore_ascii_case(name))
    }

    pub fn component_for_address(&self, block: u32) -> Option<&SsedComponent> {
        self.components
            .iter()
            .find(|component| component.contains_block(block))
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
    use std::fs;

    use tempfile::tempdir;

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
    fn maps_logical_block_address_to_component_offset() {
        let data = fixture_ssedinfo(0x4d, 0x80);
        let catalog = SsedCatalog::parse_bytes(&data).unwrap();
        let honmon = catalog.component_named("honmon.dic").unwrap();
        assert_eq!(honmon.relative_offset(1, 2), Some(2));
        assert_eq!(honmon.relative_offset(2, 0), Some(2048));
        assert_eq!(honmon.relative_offset(11, 0), None);
        assert!(catalog.component_for_address(11).is_some());
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

    #[test]
    fn file_backed_reader_reads_only_requested_expanded_range() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("HONMON.DIC");
        let first_chunk = vec![b'a'; CHUNK_SIZE];
        fs::write(
            &path,
            fixture_sseddata_literal_chunks(&[&first_chunk, b"tail"], 1, 17),
        )
        .unwrap();

        let mut file = SsedDataFile::open(&path).unwrap();
        assert_eq!(file.header().chunk_count, 2);
        assert_eq!(file.read_range(CHUNK_SIZE - 2, 6).unwrap(), b"aatail");
        assert_eq!(file.read_range(CHUNK_SIZE + 1, 2).unwrap(), b"ai");
        assert!(
            file.read_range(file.header().expanded_size(), 2)
                .unwrap()
                .is_empty()
        );
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
        fixture_sseddata_literal_chunks(&[literals], 1, 1)
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

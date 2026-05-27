use encoding_rs::SHIFT_JIS;
use serde::{Deserialize, Serialize};

use crate::ssed::BLOCK_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedIndexPointer {
    pub block: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedIndexRow {
    pub component: String,
    pub page_index: u32,
    pub logical_block: u32,
    pub row_index: u32,
    pub key: String,
    pub target_key: String,
    pub body: SsedIndexPointer,
    pub title: SsedIndexPointer,
}

pub fn is_simple_leaf_index_type(component_type: u8) -> bool {
    matches!(component_type, 0x71 | 0x72 | 0x91 | 0x92)
}

pub fn is_leaf_page(page_word: u16) -> bool {
    page_word & 0x8000 != 0
}

pub fn parse_simple_leaf_page(
    component: &str,
    page: &[u8],
    page_index: u32,
    logical_block: u32,
) -> (Vec<SsedIndexRow>, usize) {
    if page.len() < 4 {
        return (Vec::new(), 1);
    }
    let count = be16(page, 2);
    let mut pos = 4usize;
    let mut rows = Vec::new();
    let mut unknown = 0usize;

    for row_index in 1..=u32::from(count) {
        if pos >= page.len() {
            break;
        }
        let key_len = page[pos] as usize;
        if key_len == 0 {
            break;
        }
        pos += 1;
        if pos + key_len + 12 > page.len() {
            unknown += 1;
            break;
        }
        let key = decode_index_key(&page[pos..pos + key_len]);
        pos += key_len;
        let body = SsedIndexPointer {
            block: be32(page, pos),
            offset: u32::from(be16(page, pos + 4)),
        };
        let title = SsedIndexPointer {
            block: be32(page, pos + 6),
            offset: u32::from(be16(page, pos + 10)),
        };
        pos += 12;
        rows.push(SsedIndexRow {
            component: component.to_owned(),
            page_index,
            logical_block,
            row_index,
            key: key.clone(),
            target_key: key,
            body,
            title,
        });
    }

    (rows, unknown)
}

pub fn decode_index_key(data: &[u8]) -> String {
    let mut out = String::new();
    let mut index = 0usize;
    while index < data.len() {
        let byte = data[index];
        if byte == 0 {
            break;
        }
        if index + 1 < data.len()
            && (0x21..=0x7e).contains(&byte)
            && (0x21..=0x7e).contains(&data[index + 1])
        {
            if let Some(decoded) = decode_jis_pair(byte, data[index + 1]) {
                out.push(decoded);
            }
            index += 2;
            continue;
        }
        if (0x20..=0x7e).contains(&byte) {
            out.push(byte as char);
        }
        index += 1;
    }
    narrow_fullwidth_ascii(&out)
}

pub fn decode_title_text(data: &[u8]) -> String {
    let end = data
        .windows(2)
        .position(|pair| pair == [0x1f, 0x0a])
        .or_else(|| {
            data.iter()
                .position(|byte| matches!(*byte, 0x00 | b'\n' | b'\r'))
        })
        .unwrap_or(data.len());
    let mut filtered = Vec::with_capacity(end);
    let mut index = 0usize;
    while index < end {
        if data[index] == 0x1f {
            index = index.saturating_add(2);
            continue;
        }
        filtered.push(data[index]);
        index += 1;
    }
    let (decoded, _encoding, _had_errors) = SHIFT_JIS.decode(&filtered);
    decoded.trim().to_owned()
}

fn decode_jis_pair(first: u8, second: u8) -> Option<char> {
    let row = first.checked_sub(0x21)?;
    let cell = second.checked_sub(0x21)?;
    let mut lead = (row >> 1).saturating_add(0x81);
    if lead > 0x9f {
        lead = lead.saturating_add(0x40);
    }
    let trail = if row & 1 != 0 {
        cell.saturating_add(0x9f)
    } else {
        let mut trail = cell.saturating_add(0x40);
        if trail >= 0x7f {
            trail = trail.saturating_add(1);
        }
        trail
    };
    let bytes = [lead, trail];
    let (decoded, _encoding, had_errors) = SHIFT_JIS.decode(&bytes);
    if had_errors {
        return None;
    }
    decoded.chars().next()
}

fn narrow_fullwidth_ascii(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(ch as u32 - 0xfee0).unwrap_or(ch),
            '\u{3000}' => ' ',
            _ => ch,
        })
        .collect()
}

fn be16(data: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([data[offset], data[offset + 1]])
}

fn be32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

pub const INDEX_PAGE_SIZE: usize = BLOCK_SIZE as usize;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_leaf_page() {
        let mut page = vec![0u8; INDEX_PAGE_SIZE];
        page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        page[2..4].copy_from_slice(&1u16.to_be_bytes());
        page[4] = 2;
        page[5..7].copy_from_slice(&[0x24, 0x22]);
        page[7..19].copy_from_slice(&[0, 0, 0, 1, 0, 2, 0, 0, 0, 3, 0, 4]);

        let (rows, unknown) = parse_simple_leaf_page("FHINDEX.DIC", &page, 0, 100);

        assert_eq!(unknown, 0);
        assert_eq!(rows[0].key, "あ");
        assert_eq!(
            rows[0].body,
            SsedIndexPointer {
                block: 1,
                offset: 2
            }
        );
        assert_eq!(
            rows[0].title,
            SsedIndexPointer {
                block: 3,
                offset: 4
            }
        );
    }
}

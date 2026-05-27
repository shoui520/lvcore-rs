use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::diagnostics::Diagnostic;
use crate::resources::ResourceRef;

const UNI_MAGIC: &[u8; 6] = b"Ver2  ";
const UNI_VER2_HEADER_SIZE: usize = 10;
const UNI_SIMPLE_HEADER_SIZE: usize = 4;
const UNI_VER2_RECORD_SIZE: usize = 16;
const UNI_SIMPLE_RECORD_SIZE: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GaijiSourcePreference {
    Unicode,
    ExternalResource,
    Ga16Bitmap,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GaijiPolicy {
    pub priority: Vec<GaijiSourcePreference>,
}

impl Default for GaijiPolicy {
    fn default() -> Self {
        Self {
            priority: vec![
                GaijiSourcePreference::Unicode,
                GaijiSourcePreference::ExternalResource,
                GaijiSourcePreference::Ga16Bitmap,
                GaijiSourcePreference::Unresolved,
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GaijiResolution {
    pub identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_source: Option<GaijiSourcePreference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unicode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceRef>,
    pub nonliteral_marker: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

pub trait GaijiProvider: Send + Sync {
    fn resolve_gaiji(&self, identity: &str, policy: &GaijiPolicy) -> GaijiResolution;
}

pub fn normalize_gaiji_identity(identity: &str) -> Option<String> {
    let trimmed = identity
        .trim()
        .trim_start_matches("<z")
        .trim_start_matches('z')
        .trim_end_matches('>');
    if trimmed.len() != 4 || !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    Some(trimmed.to_ascii_uppercase())
}

pub fn parse_uni_gaiji_map(data: &[u8]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if data.len() < 8 {
        return map;
    }
    if data.get(..UNI_MAGIC.len()) == Some(UNI_MAGIC) {
        let half_count = be_u32(data, 6).unwrap_or_default() as usize;
        let half_offset = UNI_VER2_HEADER_SIZE;
        parse_uni_records(
            data,
            half_offset,
            half_count,
            UNI_VER2_RECORD_SIZE,
            &mut map,
        );
        let full_count_offset = half_offset + half_count.saturating_mul(UNI_VER2_RECORD_SIZE);
        let Some(full_count) = be_u32(data, full_count_offset) else {
            return map;
        };
        parse_uni_records(
            data,
            full_count_offset + 4,
            full_count as usize,
            UNI_VER2_RECORD_SIZE,
            &mut map,
        );
        return map;
    }

    let half_count = be_u32(data, 0).unwrap_or_default() as usize;
    let half_offset = UNI_SIMPLE_HEADER_SIZE;
    let full_count_offset = half_offset + half_count.saturating_mul(UNI_SIMPLE_RECORD_SIZE);
    parse_uni_records(
        data,
        half_offset,
        half_count,
        UNI_SIMPLE_RECORD_SIZE,
        &mut map,
    );
    if full_count_offset == data.len() {
        return map;
    }
    let Some(full_count) = be_u32(data, full_count_offset) else {
        return map;
    };
    parse_uni_records(
        data,
        full_count_offset + 4,
        full_count as usize,
        UNI_SIMPLE_RECORD_SIZE,
        &mut map,
    );
    map
}

fn parse_uni_records(
    data: &[u8],
    offset: usize,
    count: usize,
    record_size: usize,
    map: &mut BTreeMap<String, String>,
) {
    for index in 0..count {
        let start = offset + index.saturating_mul(record_size);
        let end = start + record_size;
        let Some(record) = data.get(start..end) else {
            break;
        };
        let code = hex::encode_upper(&record[0..2]);
        let display = decode_uni_code_units(&record[4..record_size.min(8)]);
        if !display.is_empty() {
            map.insert(code, display);
        }
    }
}

fn decode_uni_code_units(data: &[u8]) -> String {
    let mut out = String::new();
    let mut units = Vec::new();
    for chunk in data.chunks_exact(2) {
        let unit = u16::from_be_bytes([chunk[0], chunk[1]]);
        if unit != 0 {
            units.push(unit);
        }
    }
    for ch in char::decode_utf16(units).flatten() {
        out.push(ch);
    }
    out
}

fn be_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaiji_policy_can_reorder_sources() {
        let policy = GaijiPolicy {
            priority: vec![
                GaijiSourcePreference::Ga16Bitmap,
                GaijiSourcePreference::Unresolved,
                GaijiSourcePreference::ExternalResource,
                GaijiSourcePreference::Unicode,
            ],
        };
        assert_eq!(policy.priority[0], GaijiSourcePreference::Ga16Bitmap);
    }

    #[test]
    fn parses_ver2_uni_gaiji_records() {
        let mut data = Vec::new();
        data.extend_from_slice(UNI_MAGIC);
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&[
            0xA1, 0x28, 0x00, 0x00, 0x26, 0x05, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0,
        ]);
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&[
            0xB1, 0x23, 0x00, 0x00, 0x4E, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0,
        ]);

        let map = parse_uni_gaiji_map(&data);
        assert_eq!(map.get("A128").map(String::as_str), Some("★"));
        assert_eq!(map.get("B123").map(String::as_str), Some("一"));
    }
}

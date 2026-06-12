use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::diagnostics::Diagnostic;
use crate::resources::ResourceRef;

const CCALTSTR_HEADER_SIZE: usize = 16;
const CCALTSTR_RECORD_SIZE: usize = 62;
const CCALTSTR_VALUE_SIZE: usize = 60;
const CCALTSTR_HALF_MAGIC: &[u8; 8] = b"SDICALTH";
const CCALTSTR_FULL_MAGIC: &[u8; 8] = b"SDICALTF";
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RichLabel {
    pub html: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

pub fn normalize_gaiji_identity(identity: &str) -> Option<String> {
    let trimmed = identity.trim();
    let trimmed = trimmed
        .strip_prefix("<z")
        .or_else(|| trimmed.strip_prefix("<Z"))
        .or_else(|| trimmed.strip_prefix("<h"))
        .or_else(|| trimmed.strip_prefix("<H"))
        .unwrap_or(trimmed);
    let trimmed = trimmed
        .strip_prefix('z')
        .or_else(|| trimmed.strip_prefix('Z'))
        .or_else(|| trimmed.strip_prefix('h'))
        .or_else(|| trimmed.strip_prefix('H'))
        .unwrap_or(trimmed)
        .trim_end_matches('>');
    if trimmed.len() != 4 || !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    Some(trimmed.to_ascii_uppercase())
}

pub fn logovista_gaiji_placeholder(identity: &str) -> Option<String> {
    let code = normalize_gaiji_identity(identity)?;
    let high = u8::from_str_radix(&code[..2], 16).ok()?;
    let prefix = if high < 0xb0 { 'h' } else { 'z' };
    Some(format!("<{prefix}{code}>"))
}

pub fn resolve_rich_label(
    provider: &(impl GaijiProvider + ?Sized),
    value: &str,
    policy: &GaijiPolicy,
) -> RichLabel {
    let mut html = String::with_capacity(value.len());
    let mut text = String::with_capacity(value.len());
    let mut diagnostics = Vec::new();
    let mut cursor = 0usize;

    while let Some(marker) = next_gaiji_marker(value, cursor) {
        html.push_str(&escape_html(&value[cursor..marker.start]));
        text.push_str(&value[cursor..marker.start]);

        let resolution = provider.resolve_gaiji(marker.raw, policy);
        diagnostics.extend(resolution.diagnostics.clone());
        if resolution.nonliteral_marker {
            cursor = marker.end;
            continue;
        }
        let fallback_text = gaiji_resolution_text(&resolution);
        text.push_str(fallback_text);
        append_gaiji_html(&mut html, &resolution, fallback_text);

        cursor = marker.end;
    }

    html.push_str(&escape_html(&value[cursor..]));
    text.push_str(&value[cursor..]);

    RichLabel {
        html,
        text,
        diagnostics,
    }
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

pub fn parse_ccaltstr_gaiji_map(data: &[u8]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if data.len() < CCALTSTR_HEADER_SIZE {
        return map;
    }
    if data.get(..8) != Some(CCALTSTR_HALF_MAGIC) && data.get(..8) != Some(CCALTSTR_FULL_MAGIC) {
        return map;
    }

    let Some(start_code) = be_u16(data, 10) else {
        return map;
    };
    let Some(record_count) = be_u16(data, 12) else {
        return map;
    };
    let record_count = usize::from(record_count);
    let expected_size = CCALTSTR_HEADER_SIZE + record_count.saturating_mul(CCALTSTR_RECORD_SIZE);
    if expected_size != data.len() {
        return map;
    }

    let mut offset = CCALTSTR_HEADER_SIZE;
    for index in 0..record_count {
        let Some(record) = data.get(offset..offset + CCALTSTR_RECORD_SIZE) else {
            return BTreeMap::new();
        };
        let Some(code) = be_u16(record, 0) else {
            return BTreeMap::new();
        };
        let expected_code = gaiji_grid_code_for_index(start_code, index);
        if code != expected_code {
            return BTreeMap::new();
        }
        let value = decode_ccaltstr_value(&record[2..2 + CCALTSTR_VALUE_SIZE]);
        if !value.is_empty() {
            map.insert(format!("{code:04X}"), value);
        }
        offset += CCALTSTR_RECORD_SIZE;
    }

    map
}

#[derive(Debug, Clone, Copy)]
struct GaijiMarker<'a> {
    start: usize,
    end: usize,
    raw: &'a str,
}

fn next_gaiji_marker(value: &str, cursor: usize) -> Option<GaijiMarker<'_>> {
    let bytes = value.as_bytes();
    let mut index = cursor;
    while index < bytes.len() {
        if bytes[index] == b'<' {
            if let Some(end) = angle_gaiji_marker_end(bytes, index) {
                return Some(GaijiMarker {
                    start: index,
                    end,
                    raw: &value[index..end],
                });
            }
        } else if matches!(bytes[index], b'z' | b'Z')
            && let Some(end) = z_gaiji_marker_end(bytes, index)
        {
            return Some(GaijiMarker {
                start: index,
                end,
                raw: &value[index..end],
            });
        }
        index += 1;
    }
    None
}

fn angle_gaiji_marker_end(bytes: &[u8], start: usize) -> Option<usize> {
    let prefixed_end = start + 7;
    if matches!(bytes.get(start + 1), Some(b'z' | b'Z' | b'h' | b'H'))
        && bytes.get(start + 6) == Some(&b'>')
        && bytes
            .get(start + 2..start + 6)?
            .iter()
            .all(u8::is_ascii_hexdigit)
    {
        return Some(prefixed_end);
    }
    let plain_end = start + 6;
    if bytes.get(start + 5) == Some(&b'>')
        && bytes
            .get(start + 1..start + 5)?
            .iter()
            .all(u8::is_ascii_hexdigit)
    {
        return Some(plain_end);
    }
    None
}

fn z_gaiji_marker_end(bytes: &[u8], start: usize) -> Option<usize> {
    let end = start + 5;
    if !bytes.get(start + 1..end)?.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
        return None;
    }
    if bytes.get(end).is_some_and(u8::is_ascii_alphanumeric) {
        return None;
    }
    Some(end)
}

fn gaiji_resolution_text(resolution: &GaijiResolution) -> &str {
    if matches!(
        resolution.preferred_source,
        Some(GaijiSourcePreference::Unresolved)
    ) {
        return "〓";
    }
    resolution.unicode.as_deref().unwrap_or("〓")
}

fn append_gaiji_html(html: &mut String, resolution: &GaijiResolution, fallback_text: &str) {
    match resolution.preferred_source {
        Some(GaijiSourcePreference::Unicode) if resolution.unicode.is_some() => {
            html.push_str(&escape_html(fallback_text));
        }
        Some(GaijiSourcePreference::Unresolved) => {
            append_unresolved_gaiji_html(html, &resolution.identity);
        }
        Some(GaijiSourcePreference::ExternalResource | GaijiSourcePreference::Ga16Bitmap) => {
            if let Some(resource) = &resolution.resource
                && let Some(href) = resource.href.as_deref()
            {
                let class = match resolution.preferred_source {
                    Some(GaijiSourcePreference::Ga16Bitmap) => "lvcore-gaiji-ga16",
                    _ => "lvcore-gaiji-external",
                };
                html.push_str(r#"<img class="lvcore-gaiji "#);
                html.push_str(class);
                html.push_str(r#"" src=""#);
                html.push_str(&escape_html_attr(href));
                html.push_str(r#"" alt=""#);
                html.push_str(&escape_html_attr(fallback_text));
                html.push_str(r#"" title=""#);
                html.push_str(&escape_html_attr(&resolution.identity));
                html.push_str(r#"">"#);
                return;
            }
            if resolution.unicode.is_some() {
                html.push_str(&escape_html(fallback_text));
            } else {
                append_unresolved_gaiji_html(html, &resolution.identity);
            }
        }
        _ if resolution.unicode.is_some() => {
            html.push_str(&escape_html(fallback_text));
        }
        _ => append_unresolved_gaiji_html(html, &resolution.identity),
    }
}

fn append_unresolved_gaiji_html(html: &mut String, identity: &str) {
    html.push_str(r#"<span class="lvcore-gaiji-unresolved" data-gaiji=""#);
    html.push_str(&escape_html_attr(identity));
    html.push_str(r#"">〓</span>"#);
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn escape_html_attr(value: &str) -> String {
    escape_html(value)
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

fn decode_ccaltstr_value(data: &[u8]) -> String {
    let value = data.split(|byte| *byte == 0).next().unwrap_or_default();
    if value.is_empty() {
        return String::new();
    }
    if let Ok(ascii) = std::str::from_utf8(value)
        && ascii.is_ascii()
    {
        return ascii.to_owned();
    }
    let (decoded, _encoding, _had_errors) = encoding_rs::SHIFT_JIS.decode(value);
    decoded.into_owned()
}

fn gaiji_grid_code_for_index(start_code: u16, index: usize) -> u16 {
    let row = u32::from(start_code >> 8);
    let cell = u32::from(start_code & 0x00ff);
    let zero_based = row
        .saturating_sub(0x21)
        .saturating_mul(94)
        .saturating_add(cell.saturating_sub(0x21))
        .saturating_add(u32::try_from(index).unwrap_or(u32::MAX));
    let next_row = zero_based / 94 + 0x21;
    let next_cell = zero_based % 94 + 0x21;
    ((next_row << 8) | next_cell) as u16
}

fn be_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
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

    #[test]
    fn parses_ccaltstr_alt_strings_in_jis_grid_order() {
        fn record(code: u16, value: &[u8]) -> Vec<u8> {
            let mut row = Vec::new();
            row.extend_from_slice(&code.to_be_bytes());
            row.extend_from_slice(value);
            row.resize(CCALTSTR_RECORD_SIZE, 0);
            row
        }

        let mut data = Vec::new();
        data.extend_from_slice(CCALTSTR_HALF_MAGIC);
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&0xA17Eu16.to_be_bytes());
        data.extend_from_slice(&2u16.to_be_bytes());
        data.extend_from_slice(&[0, 0]);
        data.extend_from_slice(&record(0xA17E, b"x"));
        data.extend_from_slice(&record(0xA221, b"ae"));

        let map = parse_ccaltstr_gaiji_map(&data);
        assert_eq!(map.get("A17E").map(String::as_str), Some("x"));
        assert_eq!(map.get("A221").map(String::as_str), Some("ae"));
    }

    #[test]
    fn rejects_ccaltstr_sequence_mismatch() {
        fn record(code: u16) -> Vec<u8> {
            let mut row = Vec::new();
            row.extend_from_slice(&code.to_be_bytes());
            row.resize(CCALTSTR_RECORD_SIZE, 0);
            row
        }

        let mut data = Vec::new();
        data.extend_from_slice(CCALTSTR_HALF_MAGIC);
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&0xA121u16.to_be_bytes());
        data.extend_from_slice(&2u16.to_be_bytes());
        data.extend_from_slice(&[0, 0]);
        data.extend_from_slice(&record(0xA121));
        data.extend_from_slice(&record(0xA123));

        assert!(parse_ccaltstr_gaiji_map(&data).is_empty());
    }

    #[test]
    fn normalizes_uppercase_z_gaiji_markers() {
        assert_eq!(normalize_gaiji_identity("Z8f42").as_deref(), Some("8F42"));
        assert_eq!(normalize_gaiji_identity("<Z8f42>").as_deref(), Some("8F42"));
    }

    #[test]
    fn normalizes_halfwidth_gaiji_markers_from_logovista_tools() {
        assert_eq!(normalize_gaiji_identity("<hA13e>").as_deref(), Some("A13E"));
        assert_eq!(normalize_gaiji_identity("HA13e").as_deref(), Some("A13E"));
    }

    #[test]
    fn logovista_gaiji_placeholders_use_source_marker_family() {
        assert_eq!(
            logovista_gaiji_placeholder("A13e").as_deref(),
            Some("<hA13E>")
        );
        assert_eq!(
            logovista_gaiji_placeholder("<zB123>").as_deref(),
            Some("<zB123>")
        );
        assert_eq!(logovista_gaiji_placeholder("not-gaiji"), None);
    }

    #[test]
    fn rich_label_resolves_halfwidth_angle_gaiji_markers() {
        struct Provider;

        impl GaijiProvider for Provider {
            fn resolve_gaiji(&self, identity: &str, _policy: &GaijiPolicy) -> GaijiResolution {
                assert_eq!(identity, "<hA13E>");
                GaijiResolution {
                    identity: "A13E".to_owned(),
                    preferred_source: Some(GaijiSourcePreference::Unicode),
                    unicode: Some("ó".to_owned()),
                    resource: None,
                    nonliteral_marker: false,
                    diagnostics: Vec::new(),
                }
            }
        }

        let label = resolve_rich_label(&Provider, "AR<hA13E>UMENTACION", &GaijiPolicy::default());
        assert_eq!(label.text, "ARóUMENTACION");
        assert_eq!(label.html, "ARóUMENTACION");
    }

    #[test]
    fn rich_label_honors_explicit_unresolved_gaiji_policy() {
        struct Provider;

        impl GaijiProvider for Provider {
            fn resolve_gaiji(&self, identity: &str, _policy: &GaijiPolicy) -> GaijiResolution {
                assert_eq!(identity, "<zB123>");
                GaijiResolution {
                    identity: "B123".to_owned(),
                    preferred_source: Some(GaijiSourcePreference::Unresolved),
                    unicode: Some("一".to_owned()),
                    resource: None,
                    nonliteral_marker: false,
                    diagnostics: Vec::new(),
                }
            }
        }

        let policy = GaijiPolicy {
            priority: vec![
                GaijiSourcePreference::Unresolved,
                GaijiSourcePreference::Unicode,
            ],
        };
        let label = resolve_rich_label(&Provider, "A<zB123>B", &policy);
        assert_eq!(label.text, "A〓B");
        assert_eq!(
            label.html,
            r#"A<span class="lvcore-gaiji-unresolved" data-gaiji="B123">〓</span>B"#
        );
    }
}

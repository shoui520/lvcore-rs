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
        .unwrap_or(trimmed);
    let trimmed = trimmed
        .strip_prefix('z')
        .or_else(|| trimmed.strip_prefix('Z'))
        .unwrap_or(trimmed)
        .trim_end_matches('>');
    if trimmed.len() != 4 || !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    Some(trimmed.to_ascii_uppercase())
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
        let fallback_text = resolution.unicode.as_deref().unwrap_or("〓");
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
    let z_end = start + 7;
    if matches!(bytes.get(start + 1), Some(b'z' | b'Z'))
        && bytes.get(start + 6) == Some(&b'>')
        && bytes
            .get(start + 2..start + 6)?
            .iter()
            .all(u8::is_ascii_hexdigit)
    {
        return Some(z_end);
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

fn append_gaiji_html(html: &mut String, resolution: &GaijiResolution, fallback_text: &str) {
    match resolution.preferred_source {
        Some(GaijiSourcePreference::Unicode) if resolution.unicode.is_some() => {
            html.push_str(&escape_html(fallback_text));
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
    fn normalizes_uppercase_z_gaiji_markers() {
        assert_eq!(normalize_gaiji_identity("Z8f42").as_deref(), Some("8F42"));
        assert_eq!(normalize_gaiji_identity("<Z8f42>").as_deref(), Some("8F42"));
    }
}

use encoding_rs::SHIFT_JIS;

use super::html::escape_plain_label_html;
use crate::ssed_index::{SsedIndexRow, decode_jis_pair};

const SSED_FULLTEXT_SNIPPET_CHARS: usize = 160;

pub(super) fn decode_ssed_body_search_text(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len());
    let mut index = 0usize;
    while index < data.len() {
        let byte = data[index];
        if byte == 0 {
            index += 1;
            continue;
        }
        if byte == 0x1f {
            out.push(' ');
            index = index.saturating_add(2);
            if index < data.len() && data[index] <= 0x10 {
                index += 1;
            }
            if index < data.len() && data[index] <= 0x10 {
                index += 1;
            }
            continue;
        }
        if byte < 0x20 {
            out.push(' ');
            index += 1;
            continue;
        }
        if index + 1 < data.len()
            && (0x21..=0x7e).contains(&byte)
            && (0x21..=0x7e).contains(&data[index + 1])
            && let Some(decoded) = decode_jis_pair(byte, data[index + 1])
        {
            out.push(decoded);
            index += 2;
            continue;
        }
        if (0xa1..=0xfe).contains(&byte) {
            out.push(' ');
            index = index.saturating_add(2);
            continue;
        }
        if index + 1 < data.len()
            && ((0x81..=0x9f).contains(&byte) || (0xe0..=0xfc).contains(&byte))
        {
            let (decoded, _encoding, had_errors) = SHIFT_JIS.decode(&data[index..index + 2]);
            if !had_errors {
                out.push_str(decoded.as_ref());
                index += 2;
                continue;
            }
        }
        if byte <= 0x7e {
            out.push(byte as char);
        }
        index += 1;
    }
    collapse_search_whitespace(&narrow_fullwidth_ascii_text(&out))
}

pub(super) fn ssed_fulltext_snippet_html(body_text: &str, query: &str) -> Option<String> {
    let body_text = collapse_search_whitespace(body_text);
    if body_text.is_empty() {
        return None;
    }
    let normalized_body = normalize_search_match_text(&body_text);
    let normalized_query = normalize_search_match_text(query);
    let start = normalized_body
        .find(&normalized_query)
        .and_then(|byte_index| {
            normalized_body[..byte_index]
                .chars()
                .count()
                .checked_sub(SSED_FULLTEXT_SNIPPET_CHARS / 4)
        })
        .unwrap_or(0);
    let snippet = body_text
        .chars()
        .skip(start)
        .take(SSED_FULLTEXT_SNIPPET_CHARS)
        .collect::<String>();
    Some(escape_plain_label_html(&snippet))
}

pub(super) fn normalize_search_match_text(value: &str) -> String {
    narrow_fullwidth_ascii_text(value).to_lowercase()
}

pub(super) fn reverse_search_match_text(value: &str) -> String {
    value.chars().rev().collect()
}

pub(super) fn ssed_ascii_key_needs_linear_safety_net(needle: &str) -> bool {
    needle.is_ascii() && needle.bytes().any(|byte| byte.is_ascii_alphabetic())
}

pub(super) fn ssed_index_search_key_candidates(needle: &str) -> Vec<Vec<u8>> {
    let mut candidates = Vec::new();
    push_unique_search_key(&mut candidates, encode_ssed_index_search_key(needle));
    if needle.is_ascii() {
        push_unique_search_key(&mut candidates, needle.as_bytes().to_vec());
        push_unique_search_key(&mut candidates, needle.to_ascii_uppercase().into_bytes());
        push_unique_search_key(&mut candidates, needle.to_ascii_lowercase().into_bytes());
    }
    candidates
}

fn push_unique_search_key(candidates: &mut Vec<Vec<u8>>, key: Vec<u8>) {
    if !key.is_empty() && !candidates.iter().any(|candidate| candidate == &key) {
        candidates.push(key);
    }
}

pub(super) fn ssed_index_row_order_key(row: &SsedIndexRow) -> Vec<u8> {
    if row.raw_key.is_empty() {
        encode_ssed_index_search_key(&row.key.to_lowercase())
    } else {
        row.raw_key.clone()
    }
}

pub(super) fn encode_ssed_index_search_key(value: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in value.chars() {
        let ch = match ch {
            ' ' => '\u{3000}',
            ch if (0x21..=0x7e).contains(&(ch as u32)) => {
                char::from_u32(ch as u32 + 0xfee0).unwrap_or(ch)
            }
            ch => ch,
        };
        let mut text = [0_u8; 4];
        let text = ch.encode_utf8(&mut text);
        let (encoded, _encoding, had_errors) = SHIFT_JIS.encode(text);
        if had_errors {
            continue;
        }
        match encoded.as_ref() {
            [single] => out.push(*single),
            [lead, trail] => {
                if let Some((first, second)) = shift_jis_pair_to_jis_key_pair(*lead, *trail) {
                    out.push(first);
                    out.push(second);
                }
            }
            _ => {}
        }
    }
    out
}

fn shift_jis_pair_to_jis_key_pair(lead: u8, trail: u8) -> Option<(u8, u8)> {
    let row = if (0x81..=0x9f).contains(&lead) {
        (lead - 0x81) * 2
    } else if (0xe0..=0xef).contains(&lead) {
        (lead - 0xc1) * 2
    } else {
        return None;
    };
    let (row, cell) = if trail >= 0x9f {
        (row + 1, trail.checked_sub(0x9f)?)
    } else if trail >= 0x80 {
        (row, trail.checked_sub(0x41)?)
    } else if trail >= 0x40 {
        (row, trail.checked_sub(0x40)?)
    } else {
        return None;
    };
    Some((row.checked_add(0x21)?, cell.checked_add(0x21)?))
}

fn collapse_search_whitespace(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut last_was_space = false;
    for ch in value.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(ch);
            last_was_space = false;
        }
    }
    out.trim().to_owned()
}

fn narrow_fullwidth_ascii_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch as u32 {
            0xff01..=0xff5e => char::from_u32(ch as u32 - 0xfee0).unwrap_or(ch),
            0x3000 => ' ',
            _ => ch,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_search_key_uses_jis_fullwidth_ascii_order() {
        assert_eq!(encode_ssed_index_search_key(".c"), body_jis(".c"));
        assert_eq!(encode_ssed_index_search_key("30"), body_jis("30"));
        assert_eq!(encode_ssed_index_search_key("３０"), body_jis("30"));
    }

    fn body_jis(value: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for ch in value.chars() {
            let ch = match ch {
                ch if ch.is_ascii() => char::from_u32(ch as u32 + 0xfee0).unwrap(),
                ch => ch,
            };
            let mut buf = [0_u8; 4];
            let text = ch.encode_utf8(&mut buf);
            let (encoded, _encoding, had_errors) = SHIFT_JIS.encode(text);
            assert!(!had_errors);
            let bytes = encoded.as_ref();
            assert_eq!(bytes.len(), 2);
            let (lead, trail) = (bytes[0], bytes[1]);
            let (first, second) = shift_jis_pair_to_jis_key_pair(lead, trail).unwrap();
            out.extend_from_slice(&[first, second]);
        }
        out
    }
}

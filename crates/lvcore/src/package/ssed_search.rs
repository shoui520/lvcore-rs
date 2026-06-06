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

pub(super) fn ssed_body_search_byte_candidates(query: &str) -> Vec<Vec<u8>> {
    let raw_candidates = ssed_raw_search_key_prefilter_candidates(query);
    if raw_candidates.is_empty() {
        return ssed_body_search_anchor_candidates(query);
    }
    let mut candidates = raw_candidates;
    let query = query.trim();
    let (encoded, _encoding, had_errors) = SHIFT_JIS.encode(query);
    if !had_errors {
        push_unique_search_key(&mut candidates, encoded.into_owned());
    }
    candidates
}

pub(super) fn ssed_raw_search_key_prefilter_candidates(query: &str) -> Vec<Vec<u8>> {
    let query = query.trim();
    if query.is_empty()
        || query.chars().any(char::is_whitespace)
        || query.bytes().any(|byte| byte.is_ascii_alphabetic())
    {
        return Vec::new();
    }
    ssed_index_search_key_candidates(query)
}

fn ssed_body_search_anchor_candidates(query: &str) -> Vec<Vec<u8>> {
    let query = query.trim();
    if query.is_empty() || query.chars().any(char::is_whitespace) {
        return Vec::new();
    }
    let mut best = String::new();
    let mut current = String::new();
    for ch in query.chars() {
        if ch.is_ascii_alphabetic() || ch.is_whitespace() {
            if current.chars().count() > best.chars().count() {
                best = std::mem::take(&mut current);
            } else {
                current.clear();
            }
            continue;
        }
        current.push(ch);
    }
    if current.chars().count() > best.chars().count() {
        best = current;
    }
    if best.is_empty() {
        return Vec::new();
    }
    let mut candidates = ssed_index_search_key_candidates(&best);
    let (encoded, _encoding, had_errors) = SHIFT_JIS.encode(&best);
    if !had_errors {
        push_unique_search_key(&mut candidates, encoded.into_owned());
    }
    candidates
}

pub(super) fn ssed_index_page_prefilter_candidates(query: &str) -> Vec<Vec<u8>> {
    let query = query.trim();
    if query.is_empty() || query.chars().any(char::is_whitespace) {
        return Vec::new();
    }
    let mut candidates = ssed_index_search_key_candidates(query);
    let (encoded, _encoding, had_errors) = SHIFT_JIS.encode(query);
    if !had_errors {
        push_unique_search_key(&mut candidates, encoded.into_owned());
    }
    candidates
}

pub(super) fn ssed_body_window_may_contain_query(data: &[u8], candidates: &[Vec<u8>]) -> bool {
    candidates.is_empty()
        || candidates
            .iter()
            .any(|candidate| contains_subslice(data, candidate))
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
    katakana_to_hiragana_text(&narrow_fullwidth_ascii_text(value)).to_lowercase()
}

pub(super) fn reverse_search_match_text(value: &str) -> String {
    value.chars().rev().collect()
}

pub(super) fn ssed_index_search_key_candidates(needle: &str) -> Vec<Vec<u8>> {
    let mut candidates = Vec::new();
    let mut values = vec![needle.to_owned()];
    push_unique_search_value(&mut values, katakana_to_hiragana_text(needle));
    push_unique_search_value(&mut values, hiragana_to_katakana_text(needle));
    for value in values {
        push_unique_search_key(&mut candidates, encode_ssed_index_search_key(&value));
        if value.is_ascii() {
            let upper = value.to_ascii_uppercase();
            let lower = value.to_ascii_lowercase();
            push_unique_search_key(&mut candidates, encode_ssed_index_search_key(&upper));
            push_unique_search_key(&mut candidates, encode_ssed_index_search_key(&lower));
            push_unique_search_key(&mut candidates, value.as_bytes().to_vec());
            push_unique_search_key(&mut candidates, upper.into_bytes());
            push_unique_search_key(&mut candidates, lower.into_bytes());
        }
    }
    candidates
}

fn push_unique_search_value(values: &mut Vec<String>, value: String) {
    if !value.is_empty() && !values.iter().any(|candidate| candidate == &value) {
        values.push(value);
    }
}

fn push_unique_search_key(candidates: &mut Vec<Vec<u8>>, key: Vec<u8>) {
    if !key.is_empty() && !candidates.iter().any(|candidate| candidate == &key) {
        candidates.push(key);
    }
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && needle.len() <= haystack.len()
        && memchr::memmem::find(haystack, needle).is_some()
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

fn katakana_to_hiragana_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch as u32 {
            0x30a1..=0x30f6 => char::from_u32(ch as u32 - 0x60).unwrap_or(ch),
            0x30fd => '\u{309d}',
            0x30fe => '\u{309e}',
            _ => ch,
        })
        .collect()
}

fn hiragana_to_katakana_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch as u32 {
            0x3041..=0x3096 => char::from_u32(ch as u32 + 0x60).unwrap_or(ch),
            0x309d => '\u{30fd}',
            0x309e => '\u{30fe}',
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

    #[test]
    fn search_match_normalization_is_kana_insensitive() {
        assert_eq!(
            normalize_search_match_text("アカウンタビリティー"),
            "あかうんたびりてぃー"
        );
        assert_eq!(
            normalize_search_match_text("ＡＣＴＡ アクタ"),
            "acta あくた"
        );
    }

    #[test]
    fn index_search_key_candidates_include_both_kana_forms() {
        let candidates = ssed_index_search_key_candidates("あか");
        assert!(candidates.contains(&encode_ssed_index_search_key("あか")));
        assert!(candidates.contains(&encode_ssed_index_search_key("アカ")));
    }

    #[test]
    fn body_byte_prefilter_uses_jis_and_cp932_candidates_for_japanese_queries() {
        let candidates = ssed_body_search_byte_candidates("新和");
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate == &body_jis("新和"))
        );
        let (cp932, _encoding, had_errors) = SHIFT_JIS.encode("新和");
        assert!(!had_errors);
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate == cp932.as_ref())
        );
        assert!(ssed_body_window_may_contain_query(
            &body_jis("これは新和です"),
            &candidates
        ));
        assert!(!ssed_body_window_may_contain_query(
            &body_jis("これは別の語です"),
            &candidates
        ));
    }

    #[test]
    fn body_byte_prefilter_disables_for_ascii_words() {
        assert!(ssed_body_search_byte_candidates("fulltext").is_empty());
        assert!(ssed_body_search_byte_candidates("two words").is_empty());
        assert!(ssed_raw_search_key_prefilter_candidates("fulltext").is_empty());
    }

    #[test]
    fn body_byte_prefilter_uses_non_alpha_anchor_for_mixed_ascii_queries() {
        let candidates = ssed_body_search_byte_candidates("【c");
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate == &body_jis("【"))
        );
        assert!(
            candidates
                .iter()
                .all(|candidate| candidate != &body_jis("【ｃ"))
        );

        let digit_candidates = ssed_body_search_byte_candidates("O1");
        assert!(
            digit_candidates
                .iter()
                .any(|candidate| candidate == &body_jis("１"))
        );
        assert!(digit_candidates.iter().any(|candidate| candidate == b"1"));
    }

    #[test]
    fn index_page_prefilter_keeps_ascii_queries() {
        let candidates = ssed_index_page_prefilter_candidates(".N");
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate == &body_jis(".N"))
        );
        assert!(candidates.iter().any(|candidate| candidate == b".N"));
        assert!(!candidates.is_empty());
    }

    #[test]
    fn index_key_candidates_include_uppercase_jis_for_ascii_words() {
        let candidates = ssed_index_search_key_candidates("dog");
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate == &body_jis("dog"))
        );
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate == &body_jis("DOG"))
        );
        assert!(candidates.iter().any(|candidate| candidate == b"dog"));
        assert!(candidates.iter().any(|candidate| candidate == b"DOG"));
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

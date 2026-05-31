use std::collections::BTreeSet;

use encoding_rs::SHIFT_JIS;
use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::ssed_index::decode_jis_pair;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HcTextRender {
    pub text: String,
    pub stats: HcTextStats,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HcTextStats {
    pub controls: usize,
    pub line_breaks: usize,
    pub style_controls: usize,
    pub link_controls: usize,
    pub media_controls: usize,
    pub private_controls: usize,
    pub nonprinting_controls: usize,
    pub unknown_controls: usize,
    pub truncated_controls: usize,
    pub jis_pairs: usize,
    pub cp932_pairs: usize,
    pub ascii_bytes: usize,
    pub skipped_gaiji_pairs: usize,
    pub resolved_gaiji_pairs: usize,
    pub placeholder_gaiji_pairs: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcBasicTextGaiji {
    pub text: String,
    pub resolved: bool,
}

const STYLE_START_OPS: &[(u8, HcTextStyle)] = &[
    (0x04, HcTextStyle::HalfWidth),
    (0x06, HcTextStyle::Other),
    (0x0b, HcTextStyle::Other),
    (0x0e, HcTextStyle::Other),
    (0x10, HcTextStyle::Other),
    (0x12, HcTextStyle::Other),
    (0x41, HcTextStyle::Other),
    (0xe0, HcTextStyle::Other),
];

const STYLE_END_OPS: &[(u8, HcTextStyle)] = &[
    (0x05, HcTextStyle::HalfWidth),
    (0x07, HcTextStyle::Other),
    (0x0c, HcTextStyle::Other),
    (0x0f, HcTextStyle::Other),
    (0x11, HcTextStyle::Other),
    (0x13, HcTextStyle::Other),
    (0x61, HcTextStyle::Other),
    (0xe1, HcTextStyle::Other),
];

const LINK_START_OPS: &[u8] = &[0x3b, 0x42, 0x43, 0x44, 0x49];
const LINK_END_OPS: &[u8] = &[0x5b, 0x62, 0x63, 0x64, 0x69];
const MEDIA_OPS: &[u8] = &[0x39, 0x3c, 0x4a, 0x4d, 0x59, 0x6a];
const PRIVATE_OPS: &[u8] = &[0xe2, 0xe3, 0xe4, 0xe6];
const VERTICAL_HINT_OPS: &[u8] = &[0x36, 0x37, 0x4b, 0x4c];
const PRIVATE_RENDERER_DIRECTIVE_OPS: &[u8] = &[0x4e, 0x4f];
const COMMON_RENDERER_STATE_OPS: &[u8] = &[0x6d];
const KNOWN_NONPRINTING_OPS: &[u8] = &[
    0x00, 0x02, 0x03, 0x09, 0x14, 0x15, 0x1a, 0x1c, 0x36, 0x37, 0x39, 0x48, 0x4b, 0x4c, 0x4e, 0x4f,
    0x59, 0xe4, 0xe6,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HcTextStyle {
    HalfWidth,
    Other,
}

pub fn decode_hc_stream_basic_text(data: &[u8]) -> HcTextRender {
    decode_hc_stream_basic_text_with_gaiji(data, |_code| None)
}

pub fn decode_hc_stream_basic_text_with_gaiji(
    data: &[u8],
    mut gaiji_text: impl FnMut(&str) -> Option<HcBasicTextGaiji>,
) -> HcTextRender {
    let mut text = String::with_capacity(data.len());
    let mut stats = HcTextStats::default();
    let mut unknown_ops = BTreeSet::new();
    let mut halfwidth_depth = 0usize;
    let mut offset = 0usize;

    while offset < data.len() {
        let byte = data[offset];
        if byte == 0 {
            offset += 1;
            continue;
        }

        if byte == 0x1f {
            if offset + 1 >= data.len() {
                stats.truncated_controls += 1;
                break;
            }
            stats.controls += 1;
            let op = data[offset + 1];
            let arg_len = hc_control_arg_length(data, offset);
            let next = offset.saturating_add(2).saturating_add(arg_len);
            if next > data.len() {
                stats.truncated_controls += 1;
                break;
            }

            if op == 0x0a {
                push_line_break(&mut text);
                stats.line_breaks += 1;
                offset = next;
                continue;
            }

            if let Some(style) = style_start(op) {
                stats.style_controls += 1;
                if style == HcTextStyle::HalfWidth {
                    halfwidth_depth = halfwidth_depth.saturating_add(1);
                }
                offset = next;
                continue;
            }

            if let Some(style) = style_end(op) {
                stats.style_controls += 1;
                if style == HcTextStyle::HalfWidth {
                    halfwidth_depth = halfwidth_depth.saturating_sub(1);
                }
                offset = next;
                continue;
            }

            if LINK_START_OPS.contains(&op) || LINK_END_OPS.contains(&op) {
                stats.link_controls += 1;
                offset = next;
                continue;
            }

            if MEDIA_OPS.contains(&op) {
                stats.media_controls += 1;
                offset = next;
                continue;
            }

            if PRIVATE_OPS.contains(&op)
                || PRIVATE_RENDERER_DIRECTIVE_OPS.contains(&op)
                || VERTICAL_HINT_OPS.contains(&op)
                || COMMON_RENDERER_STATE_OPS.contains(&op)
            {
                stats.private_controls += 1;
                offset = next;
                continue;
            }

            if KNOWN_NONPRINTING_OPS.contains(&op) {
                stats.nonprinting_controls += 1;
                offset = next;
                continue;
            }

            stats.unknown_controls += 1;
            unknown_ops.insert(op);
            offset = next;
            continue;
        }

        if byte < 0x20 {
            if byte == b'\n' {
                push_line_break(&mut text);
                stats.line_breaks += 1;
            } else if byte == b'\t' {
                text.push('\t');
            } else {
                text.push(' ');
            }
            offset += 1;
            continue;
        }

        if offset + 1 < data.len()
            && (0x21..=0x7e).contains(&byte)
            && (0x21..=0x7e).contains(&data[offset + 1])
            && let Some(decoded) = decode_jis_pair(byte, data[offset + 1])
        {
            if halfwidth_depth > 0 {
                text.push_str(&narrow_fullwidth_ascii(&decoded.to_string()));
            } else {
                text.push(decoded);
            }
            stats.jis_pairs += 1;
            offset += 2;
            continue;
        }

        if offset + 1 < data.len()
            && ((0x81..=0x9f).contains(&byte) || (0xe0..=0xfc).contains(&byte))
        {
            let (decoded, _encoding, had_errors) = SHIFT_JIS.decode(&data[offset..offset + 2]);
            if !had_errors {
                text.push_str(decoded.as_ref());
                stats.cp932_pairs += 1;
                offset += 2;
                continue;
            }
        }

        if offset + 1 < data.len() && (0xa1..=0xfe).contains(&byte) {
            let second = data[offset + 1];
            let code = format!("{byte:02X}{second:02X}");
            if let Some(resolved) = gaiji_text(&code) {
                text.push_str(&resolved.text);
                if resolved.resolved {
                    stats.resolved_gaiji_pairs += 1;
                } else {
                    stats.placeholder_gaiji_pairs += 1;
                }
            } else {
                stats.skipped_gaiji_pairs += 1;
            }
            offset += 2;
            continue;
        }

        if byte <= 0x7e {
            text.push(byte as char);
            stats.ascii_bytes += 1;
        }
        offset += 1;
    }

    let mut diagnostics = Vec::new();
    if !unknown_ops.is_empty() {
        diagnostics.push(Diagnostic::warning(
            "hc_basic_text_unknown_controls",
            format!(
                "skipped {} unknown HC/SSED control opcode(s): {}",
                unknown_ops.len(),
                unknown_ops
                    .iter()
                    .map(|op| format!("1f{op:02x}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    if stats.truncated_controls > 0 {
        diagnostics.push(Diagnostic::warning(
            "hc_basic_text_truncated_control",
            format!(
                "{} truncated HC/SSED control record(s) were skipped",
                stats.truncated_controls
            ),
        ));
    }
    if stats.skipped_gaiji_pairs > 0 {
        diagnostics.push(Diagnostic::info(
            "hc_basic_text_gaiji_pairs_skipped",
            format!(
                "{} raw gaiji/control byte pair(s) require gaiji resolution beyond BasicText",
                stats.skipped_gaiji_pairs
            ),
        ));
    }
    if stats.placeholder_gaiji_pairs > 0 {
        diagnostics.push(Diagnostic::info(
            "hc_basic_text_gaiji_placeholders",
            format!(
                "{} raw gaiji byte pair(s) could not be resolved to BasicText Unicode and were rendered as placeholders",
                stats.placeholder_gaiji_pairs
            ),
        ));
    }

    HcTextRender {
        text,
        stats,
        diagnostics,
    }
}

fn style_start(op: u8) -> Option<HcTextStyle> {
    STYLE_START_OPS
        .iter()
        .find_map(|(candidate, style)| (*candidate == op).then_some(*style))
}

fn style_end(op: u8) -> Option<HcTextStyle> {
    STYLE_END_OPS
        .iter()
        .find_map(|(candidate, style)| (*candidate == op).then_some(*style))
}

fn push_line_break(text: &mut String) {
    if !text.ends_with('\n') {
        text.push('\n');
    }
}

fn narrow_fullwidth_ascii(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '\u{3000}' => ' ',
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(ch as u32 - 0xfee0).unwrap_or(ch),
            _ => ch,
        })
        .collect()
}

fn hc_control_arg_length(data: &[u8], offset: usize) -> usize {
    if offset + 1 >= data.len() || data[offset] != 0x1f {
        return 0;
    }
    let op = data[offset + 1];
    match op {
        0x09 | 0x14 | 0x1a | 0x1c | 0x41 | 0x4c | 0xe0 | 0xe2 | 0xe4 | 0xe6 => 2,
        0x15 | 0x42 | 0x43 | 0x59 | 0x69 => 0,
        0x36 => 12,
        0x37 | 0x44 | 0x48 | 0x49 => 10,
        0x39 | 0x3c | 0x4d => 18,
        0x4a => match be16_at(data, offset + 2).map(|word| word & 0x000f) {
            Some(0) => 14,
            Some(1 | 2) => 16,
            Some(_) => 2,
            None => 16,
        },
        0x4b | 0x62 | 0x63 | 0x64 => 6,
        0x4e => match be16_at(data, offset + 2).map(|word| word & 0x0f00) {
            Some(0) => 38,
            Some(0x0100 | 0x0200) => 40,
            Some(_) => 2,
            None => 38,
        },
        0x4f => {
            if data.get(offset + 2..offset + 4) == Some(&[0x1f, 0x6f]) {
                48
            } else {
                34
            }
        }
        _ => 0,
    }
}

fn be16_at(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use encoding_rs::SHIFT_JIS;

    use super::{
        HcBasicTextGaiji, decode_hc_stream_basic_text, decode_hc_stream_basic_text_with_gaiji,
    };

    #[test]
    fn basic_text_decodes_jis_controls_and_halfwidth_scope() {
        let mut data = body_jis("見出し");
        data.extend_from_slice(&[0x1f, 0x0a]);
        data.extend_from_slice(&[0x1f, 0x04]);
        data.extend_from_slice(&body_jis("ＡＢＣ"));
        data.extend_from_slice(&[0x1f, 0x05]);
        data.extend_from_slice(&body_jis("本文"));

        let rendered = decode_hc_stream_basic_text(&data);

        assert_eq!(rendered.text, "見出し\nABC本文");
        assert_eq!(rendered.stats.line_breaks, 1);
        assert_eq!(rendered.stats.style_controls, 2);
        assert!(rendered.diagnostics.is_empty());
    }

    #[test]
    fn basic_text_reports_unknown_and_truncated_controls() {
        let rendered = decode_hc_stream_basic_text(&[0x1f, 0x99, 0x1f]);

        assert_eq!(rendered.stats.unknown_controls, 1);
        assert_eq!(rendered.stats.truncated_controls, 1);
        assert!(
            rendered
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "hc_basic_text_unknown_controls")
        );
        assert!(
            rendered
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "hc_basic_text_truncated_control")
        );
    }

    #[test]
    fn basic_text_can_resolve_raw_gaiji_pairs() {
        let mut data = body_jis("本文");
        data.extend_from_slice(&[0xb1, 0x23]);
        data.extend_from_slice(&[0xb9, 0x99]);

        let rendered = decode_hc_stream_basic_text_with_gaiji(&data, |code| match code {
            "B123" => Some(HcBasicTextGaiji {
                text: "一".to_owned(),
                resolved: true,
            }),
            "B999" => Some(HcBasicTextGaiji {
                text: "〓".to_owned(),
                resolved: false,
            }),
            _ => None,
        });

        assert_eq!(rendered.text, "本文一〓");
        assert_eq!(rendered.stats.resolved_gaiji_pairs, 1);
        assert_eq!(rendered.stats.placeholder_gaiji_pairs, 1);
        assert!(
            rendered
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "hc_basic_text_gaiji_placeholders")
        );
    }

    fn body_jis(value: &str) -> Vec<u8> {
        value
            .chars()
            .flat_map(|ch| {
                let body_ch = if (0x20..=0x7e).contains(&(ch as u32)) {
                    if ch == ' ' {
                        '\u{3000}'
                    } else {
                        char::from_u32(ch as u32 + 0xfee0).unwrap_or(ch)
                    }
                } else {
                    ch
                };
                cp932(&body_ch.to_string())
                    .chunks(2)
                    .next()
                    .and_then(sjis_pair_to_jis_pair)
                    .unwrap_or_default()
            })
            .collect()
    }

    fn cp932(value: &str) -> Vec<u8> {
        let (encoded, _encoding, _had_errors) = SHIFT_JIS.encode(value);
        encoded.into_owned()
    }

    fn sjis_pair_to_jis_pair(sjis: &[u8]) -> Option<Vec<u8>> {
        if sjis.len() != 2 {
            return None;
        }
        let lead = sjis[0];
        let trail = sjis[1];
        let row_base = if (0x81..=0x9f).contains(&lead) {
            (lead - 0x81) * 2
        } else if (0xe0..=0xef).contains(&lead) {
            (lead - 0xc1) * 2
        } else {
            return None;
        };
        let (row, cell) = if (0x9f..=0xfc).contains(&trail) {
            (row_base + 1, trail - 0x9f)
        } else if (0x40..=0xfc).contains(&trail) && trail != 0x7f {
            let adjusted = if trail >= 0x80 { trail - 1 } else { trail };
            (row_base, adjusted - 0x40)
        } else {
            return None;
        };
        Some(vec![row + 0x21, cell + 0x21])
    }
}

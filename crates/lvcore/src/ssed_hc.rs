use std::collections::{BTreeMap, BTreeSet};

use encoding_rs::SHIFT_JIS;
use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::ssed_color_sample::ColorSampleRecord;
use crate::ssed_index::decode_jis_pair;
use crate::ssed_sound_data::parse_sounddata_marker_at;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HcTextRender {
    pub text: String,
    pub stats: HcTextStats,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HcCommonHtmlRender {
    pub html: String,
    pub text: String,
    pub stats: HcTextStats,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<HcCommonHtmlLink>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<HcCommonHtmlMedia>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub color_samples: Vec<HcCommonHtmlColorSample>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HcCommonHtmlLink {
    pub href: String,
    pub block: u32,
    pub offset: u32,
    pub control: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HcCommonHtmlMedia {
    pub index: usize,
    pub control: String,
    pub payload_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HcCommonHtmlColorSample {
    pub sample_key: String,
    pub status: HcCommonHtmlColorSampleStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<ColorSampleRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HcCommonHtmlColorSampleStatus {
    ResolvedMunsell,
    UnresolvedSampleKey,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HcTextStats {
    pub controls: usize,
    pub line_breaks: usize,
    pub style_controls: usize,
    pub link_controls: usize,
    pub media_controls: usize,
    #[serde(default)]
    pub color_samples: usize,
    #[serde(default)]
    pub color_sample_ends: usize,
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
    pub suppressed_gaiji_pairs: usize,
    pub numeric_character_references: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcBasicTextGaiji {
    pub text: String,
    pub resolved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcCommonHtmlGaiji {
    pub text: String,
    pub html: Option<String>,
    pub resolved: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HcMarkerProfile {
    pub renderer_code: Option<String>,
    pub nonliteral_gaiji_codes: BTreeSet<String>,
    pub gaiji_aliases: BTreeMap<String, String>,
}

impl HcMarkerProfile {
    pub fn suppresses_gaiji_code(&self, code: &str) -> bool {
        let normalized = normalize_gaiji_code(code);
        !normalized.is_empty() && self.nonliteral_gaiji_codes.contains(&normalized)
    }

    pub fn gaiji_lookup_code(&self, code: &str) -> String {
        let normalized = normalize_gaiji_code(code);
        self.gaiji_aliases
            .get(&normalized)
            .cloned()
            .unwrap_or(normalized)
    }
}

pub fn hc_marker_profile_for_renderer(renderer_code: Option<&str>) -> HcMarkerProfile {
    let code = normalize_renderer_code(renderer_code);
    let mut nonliteral_gaiji_codes = BTreeSet::new();
    let mut gaiji_aliases = BTreeMap::new();

    match code.as_deref() {
        Some("00A3") => {
            gaiji_aliases.insert("B261".to_owned(), "B167".to_owned());
        }
        Some("009F") => {
            gaiji_aliases.insert("B261".to_owned(), "B167".to_owned());
        }
        Some("013A") => {
            extend_codes(
                &mut nonliteral_gaiji_codes,
                [
                    "A225", "A226", "B261", "B262", "B264", "B265", "B26A", "B26B",
                ],
            );
        }
        Some("013C") => {
            extend_codes(&mut nonliteral_gaiji_codes, ["B121", "B122"]);
        }
        Some("013F") => {
            extend_codes(
                &mut nonliteral_gaiji_codes,
                ["B15B", "B15C", "B15E", "B162", "B163"],
            );
        }
        Some("0158") => {
            for marker in 0xB353_u16..=0xB37E {
                nonliteral_gaiji_codes.insert(format!("{marker:04X}"));
            }
        }
        _ => {}
    }

    HcMarkerProfile {
        renderer_code: code,
        nonliteral_gaiji_codes,
        gaiji_aliases,
    }
}

fn extend_codes<const N: usize>(set: &mut BTreeSet<String>, codes: [&str; N]) {
    set.extend(codes.into_iter().map(str::to_owned));
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
const MEDIA_OPS: &[u8] = &[0x39, 0x3c, 0x4a, 0x4d, 0x59, 0x64, 0x6a];
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HcHtmlStyle {
    start_op: u8,
    tag: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HcHtmlInline {
    Link,
    Url,
}

pub fn decode_hc_stream_basic_text(data: &[u8]) -> HcTextRender {
    decode_hc_stream_basic_text_with_gaiji(data, |_code| None)
}

pub fn decode_hc_stream_common_html(data: &[u8]) -> HcCommonHtmlRender {
    decode_hc_stream_common_html_with_gaiji(data, |_code| None)
}

pub fn decode_hc_stream_common_html_with_gaiji(
    data: &[u8],
    mut gaiji_text: impl FnMut(&str) -> Option<HcBasicTextGaiji>,
) -> HcCommonHtmlRender {
    decode_hc_stream_common_html_with_gaiji_policy(data, &mut gaiji_text, |_code| false)
}

pub fn decode_hc_stream_common_html_with_gaiji_policy(
    data: &[u8],
    mut gaiji_text: impl FnMut(&str) -> Option<HcBasicTextGaiji>,
    mut suppress_gaiji: impl FnMut(&str) -> bool,
) -> HcCommonHtmlRender {
    decode_hc_stream_common_html_with_gaiji_render_policy(
        data,
        |code| {
            gaiji_text(code).map(|gaiji| HcCommonHtmlGaiji {
                text: gaiji.text,
                html: None,
                resolved: gaiji.resolved,
            })
        },
        &mut suppress_gaiji,
    )
}

pub fn decode_hc_stream_common_html_with_gaiji_render_policy(
    data: &[u8],
    mut gaiji_render: impl FnMut(&str) -> Option<HcCommonHtmlGaiji>,
    mut suppress_gaiji: impl FnMut(&str) -> bool,
) -> HcCommonHtmlRender {
    decode_hc_stream_common_html_with_gaiji_render_policy_and_color_samples(
        data,
        &mut gaiji_render,
        &mut suppress_gaiji,
        |_sample_key| None,
    )
}

pub fn decode_hc_stream_common_html_with_gaiji_render_policy_and_color_samples(
    data: &[u8],
    mut gaiji_render: impl FnMut(&str) -> Option<HcCommonHtmlGaiji>,
    mut suppress_gaiji: impl FnMut(&str) -> bool,
    mut color_sample: impl FnMut(&str) -> Option<ColorSampleRecord>,
) -> HcCommonHtmlRender {
    let mut html = String::with_capacity(data.len().saturating_mul(2));
    let mut text = String::with_capacity(data.len());
    let mut stats = HcTextStats::default();
    let mut links = Vec::new();
    let mut media = Vec::new();
    let mut color_samples = Vec::new();
    let mut unknown_ops = BTreeSet::new();
    let mut style_stack: Vec<HcHtmlStyle> = Vec::new();
    let mut inline_stack: Vec<HcHtmlInline> = Vec::new();
    let mut halfwidth_depth = 0usize;
    let mut private_depth = 0usize;
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
            let payload = &data[offset + 2..next];

            if op == 0xe2 {
                stats.private_controls += 1;
                private_depth = private_depth.saturating_add(1);
                offset = next;
                continue;
            }

            if op == 0xe3 {
                stats.private_controls += 1;
                private_depth = private_depth.saturating_sub(1);
                offset = next;
                continue;
            }

            if private_depth > 0 {
                stats.private_controls += 1;
                offset = next;
                continue;
            }

            if op == 0x0a {
                push_html_line_break(&mut html, &mut text);
                stats.line_breaks += 1;
                offset = next;
                continue;
            }

            if op == 0x0b
                && let Some((decoded, end)) = decode_unicode_scalar_run(data, next)
            {
                push_html_text(&mut html, &decoded);
                text.push_str(&decoded);
                stats.controls += 1;
                stats.style_controls += 2;
                offset = end;
                continue;
            }

            if let Some((tag, attrs)) = html_style_start_spec(op) {
                stats.style_controls += 1;
                html.push('<');
                html.push_str(tag);
                html.push_str(attrs);
                html.push('>');
                style_stack.push(HcHtmlStyle { start_op: op, tag });
                if op == 0x04 {
                    halfwidth_depth = halfwidth_depth.saturating_add(1);
                }
                offset = next;
                continue;
            }

            if let Some(start_op) = html_style_end_start_op(op) {
                stats.style_controls += 1;
                close_html_style(&mut html, &mut style_stack, start_op, &mut halfwidth_depth);
                offset = next;
                continue;
            }

            if op == 0x3b {
                stats.link_controls += 1;
                html.push_str("<span class=\"lv-hc-url\">");
                inline_stack.push(HcHtmlInline::Url);
                offset = next;
                continue;
            }

            if op == 0x5b {
                stats.link_controls += 1;
                if let Some(position) = inline_stack
                    .iter()
                    .rposition(|inline| *inline == HcHtmlInline::Url)
                {
                    inline_stack.remove(position);
                    html.push_str("</span>");
                }
                offset = next;
                continue;
            }

            if LINK_START_OPS.contains(&op) {
                stats.link_controls += 1;
                let target_payload = if payload.len() >= 6 {
                    &payload[payload.len() - 6..]
                } else {
                    payload
                };
                let target = decode_pointer_payload(target_payload);
                if let Some((block, offset_value)) = target {
                    links.push(HcCommonHtmlLink {
                        href: lvaddr_href(block, offset_value),
                        block,
                        offset: offset_value,
                        control: format!("1f{op:02x}"),
                    });
                }
                append_link_start(&mut html, op, target);
                inline_stack.push(HcHtmlInline::Link);
                offset = next;
                continue;
            }

            if LINK_END_OPS.contains(&op) {
                if let Some(position) = inline_stack
                    .iter()
                    .rposition(|inline| *inline == HcHtmlInline::Link)
                {
                    stats.link_controls += 1;
                    inline_stack.remove(position);
                    html.push_str("</a>");
                    offset = next;
                    continue;
                }
                if op != 0x64 {
                    stats.link_controls += 1;
                    offset = next;
                    continue;
                }
            }

            if MEDIA_OPS.contains(&op) {
                stats.media_controls += 1;
                let media_index = media.len();
                media.push(HcCommonHtmlMedia {
                    index: media_index,
                    control: format!("1f{op:02x}"),
                    payload_hex: hex_lower(payload),
                });
                append_media_placeholder(&mut html, op, payload, media_index);
                offset = next;
                continue;
            }

            if op == 0x14 {
                let sample_key = hex_lower(payload);
                let record = color_sample(&sample_key);
                let status = if record.is_some() {
                    HcCommonHtmlColorSampleStatus::ResolvedMunsell
                } else {
                    HcCommonHtmlColorSampleStatus::UnresolvedSampleKey
                };
                append_color_sample_html(&mut html, &sample_key, record.as_ref());
                color_samples.push(HcCommonHtmlColorSample {
                    sample_key,
                    status,
                    record,
                });
                stats.color_samples += 1;
                offset = next;
                continue;
            }

            if op == 0x15 {
                stats.color_sample_ends += 1;
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

        if private_depth > 0 {
            offset = skip_private_hc_payload(data, offset, &mut stats);
            continue;
        }

        if let Some(reference) = parse_sounddata_marker_at(data, offset) {
            stats.media_controls += 1;
            let media_index = media.len();
            media.push(HcCommonHtmlMedia {
                index: media_index,
                control: "sounddata".to_owned(),
                payload_hex: reference.code_hex.to_ascii_lowercase(),
            });
            append_media_placeholder_for_control(
                &mut html,
                "sounddata",
                &reference.code_hex.to_ascii_lowercase(),
                media_index,
            );
            offset = reference.end;
            continue;
        }

        if byte < 0x20 {
            if byte == b'\n' {
                push_html_line_break(&mut html, &mut text);
                stats.line_breaks += 1;
            } else if byte == b'\t' {
                html.push('\t');
                text.push('\t');
            } else {
                html.push(' ');
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
            let visible = if halfwidth_depth > 0 {
                narrow_fullwidth_ascii(&decoded.to_string())
            } else {
                decoded.to_string()
            };
            push_html_text(&mut html, &visible);
            text.push_str(&visible);
            stats.jis_pairs += 1;
            offset += 2;
            continue;
        }

        if offset + 1 < data.len()
            && ((0x81..=0x9f).contains(&byte) || (0xe0..=0xfc).contains(&byte))
        {
            let (decoded, _encoding, had_errors) = SHIFT_JIS.decode(&data[offset..offset + 2]);
            if !had_errors {
                push_html_text(&mut html, decoded.as_ref());
                text.push_str(decoded.as_ref());
                stats.cp932_pairs += 1;
                offset += 2;
                continue;
            }
        }

        if offset + 1 < data.len() && (0xa1..=0xfe).contains(&byte) {
            let second = data[offset + 1];
            let code = format!("{byte:02X}{second:02X}");
            if suppress_gaiji(&code) {
                stats.suppressed_gaiji_pairs += 1;
            } else if let Some(resolved) = gaiji_render(&code) {
                if let Some(fragment) = resolved.html.as_deref() {
                    html.push_str(fragment);
                } else {
                    push_html_text(&mut html, &resolved.text);
                }
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
            let ch = byte as char;
            push_html_char(&mut html, ch);
            text.push(ch);
            stats.ascii_bytes += 1;
        }
        offset += 1;
    }

    let closed_unbalanced_state =
        !inline_stack.is_empty() || !style_stack.is_empty() || halfwidth_depth > 0;
    while let Some(inline) = inline_stack.pop() {
        match inline {
            HcHtmlInline::Link => html.push_str("</a>"),
            HcHtmlInline::Url => html.push_str("</span>"),
        }
    }
    while let Some(style) = style_stack.pop() {
        if style.start_op == 0x04 {
            halfwidth_depth = halfwidth_depth.saturating_sub(1);
        }
        html.push_str("</");
        html.push_str(style.tag);
        html.push('>');
    }

    let (decoded_html, html_refs) = decode_hc_html_numeric_character_references(&html);
    let (decoded_text, text_refs) = decode_hc_text_numeric_character_references(&text);
    html = decoded_html;
    text = decoded_text;
    stats.numeric_character_references = html_refs.max(text_refs);

    let mut diagnostics = hc_text_diagnostics(&stats, &unknown_ops);
    if closed_unbalanced_state {
        diagnostics.push(Diagnostic::warning(
            "hc_common_html_unbalanced_state",
            "common HC HTML fallback closed unbalanced style/link state at end of stream",
        ));
    }

    HcCommonHtmlRender {
        html: format!("<div class=\"lv-hc-common-html-fallback\">{html}</div>"),
        text,
        stats,
        links,
        media,
        color_samples,
        diagnostics,
    }
}

pub fn decode_hc_stream_basic_text_with_gaiji(
    data: &[u8],
    mut gaiji_text: impl FnMut(&str) -> Option<HcBasicTextGaiji>,
) -> HcTextRender {
    decode_hc_stream_basic_text_with_gaiji_policy(data, &mut gaiji_text, |_code| false)
}

pub fn decode_hc_stream_basic_text_with_gaiji_policy(
    data: &[u8],
    mut gaiji_text: impl FnMut(&str) -> Option<HcBasicTextGaiji>,
    mut suppress_gaiji: impl FnMut(&str) -> bool,
) -> HcTextRender {
    let mut text = String::with_capacity(data.len());
    let mut stats = HcTextStats::default();
    let mut unknown_ops = BTreeSet::new();
    let mut halfwidth_depth = 0usize;
    let mut private_depth = 0usize;
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

            if op == 0xe2 {
                stats.private_controls += 1;
                private_depth = private_depth.saturating_add(1);
                offset = next;
                continue;
            }

            if op == 0xe3 {
                stats.private_controls += 1;
                private_depth = private_depth.saturating_sub(1);
                offset = next;
                continue;
            }

            if private_depth > 0 {
                stats.private_controls += 1;
                offset = next;
                continue;
            }

            if op == 0x0a {
                push_line_break(&mut text);
                stats.line_breaks += 1;
                offset = next;
                continue;
            }

            if op == 0x0b
                && let Some((decoded, end)) = decode_unicode_scalar_run(data, next)
            {
                text.push_str(&decoded);
                stats.controls += 1;
                stats.style_controls += 2;
                offset = end;
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

        if private_depth > 0 {
            offset = skip_private_hc_payload(data, offset, &mut stats);
            continue;
        }

        if let Some(reference) = parse_sounddata_marker_at(data, offset) {
            stats.media_controls += 1;
            offset = reference.end;
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
            if suppress_gaiji(&code) {
                stats.suppressed_gaiji_pairs += 1;
            } else if let Some(resolved) = gaiji_text(&code) {
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

    let (decoded_text, numeric_refs) = decode_hc_text_numeric_character_references(&text);
    text = decoded_text;
    stats.numeric_character_references = numeric_refs;

    let diagnostics = hc_text_diagnostics(&stats, &unknown_ops);

    HcTextRender {
        text,
        stats,
        diagnostics,
    }
}

fn skip_private_hc_payload(data: &[u8], offset: usize, stats: &mut HcTextStats) -> usize {
    let byte = data[offset];
    if let Some(reference) = parse_sounddata_marker_at(data, offset) {
        stats.media_controls += 1;
        return reference.end;
    }
    if byte < 0x20 {
        if byte == b'\n' {
            stats.line_breaks += 1;
        }
        return offset + 1;
    }
    if offset + 1 < data.len()
        && (0x21..=0x7e).contains(&byte)
        && (0x21..=0x7e).contains(&data[offset + 1])
    {
        stats.jis_pairs += 1;
        return offset + 2;
    }
    if offset + 1 < data.len() && ((0x81..=0x9f).contains(&byte) || (0xe0..=0xfc).contains(&byte)) {
        stats.cp932_pairs += 1;
        return offset + 2;
    }
    if offset + 1 < data.len() && (0xa1..=0xfe).contains(&byte) {
        stats.suppressed_gaiji_pairs += 1;
        return offset + 2;
    }
    if byte <= 0x7e {
        stats.ascii_bytes += 1;
    }
    offset + 1
}

fn hc_text_diagnostics(stats: &HcTextStats, unknown_ops: &BTreeSet<u8>) -> Vec<Diagnostic> {
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
    if stats.numeric_character_references > 0 {
        diagnostics.push(Diagnostic::info(
            "hc_numeric_character_references_decoded",
            format!(
                "{} embedded numeric character reference(s) were decoded to Unicode",
                stats.numeric_character_references
            ),
        ));
    }

    diagnostics
}

fn decode_hc_text_numeric_character_references(value: &str) -> (String, usize) {
    decode_hc_numeric_character_references(value, true, true, false)
}

fn decode_hc_html_numeric_character_references(value: &str) -> (String, usize) {
    decode_hc_numeric_character_references(value, false, true, true)
}

fn decode_hc_numeric_character_references(
    value: &str,
    allow_ascii: bool,
    allow_fullwidth: bool,
    allow_escaped_ascii: bool,
) -> (String, usize) {
    let chars = value.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(value.len());
    let mut decoded = 0usize;
    let mut index = 0usize;
    while index < chars.len() {
        let parsed = if allow_fullwidth {
            parse_fullwidth_numeric_character_reference(&chars, index)
        } else {
            None
        }
        .or_else(|| {
            if allow_ascii {
                parse_ascii_numeric_character_reference(&chars, index)
            } else {
                None
            }
        })
        .or_else(|| {
            if allow_escaped_ascii {
                parse_escaped_ascii_numeric_character_reference(&chars, index)
            } else {
                None
            }
        });
        if let Some((ch, next_index)) = parsed {
            output.push(ch);
            decoded = decoded.saturating_add(1);
            index = next_index;
            continue;
        }
        output.push(chars[index]);
        index = index.saturating_add(1);
    }
    (output, decoded)
}

fn parse_ascii_numeric_character_reference(chars: &[char], index: usize) -> Option<(char, usize)> {
    if chars.get(index) != Some(&'&') || chars.get(index + 1) != Some(&'#') {
        return None;
    }
    parse_numeric_character_reference_body(chars, index + 2, is_ascii_semicolon)
}

fn parse_fullwidth_numeric_character_reference(
    chars: &[char],
    index: usize,
) -> Option<(char, usize)> {
    if chars.get(index) != Some(&'＆') || chars.get(index + 1) != Some(&'＃') {
        return None;
    }
    parse_numeric_character_reference_body(chars, index + 2, is_fullwidth_or_ascii_semicolon)
}

fn parse_escaped_ascii_numeric_character_reference(
    chars: &[char],
    index: usize,
) -> Option<(char, usize)> {
    let prefix = ['&', 'a', 'm', 'p', ';', '#'];
    if chars.get(index..index + prefix.len())? != prefix {
        return None;
    }
    parse_numeric_character_reference_body(chars, index + prefix.len(), is_ascii_semicolon)
}

fn parse_numeric_character_reference_body(
    chars: &[char],
    mut index: usize,
    semicolon: fn(char) -> bool,
) -> Option<(char, usize)> {
    let radix = if chars
        .get(index)
        .is_some_and(|ch| matches!(*ch, 'x' | 'X' | 'ｘ' | 'Ｘ'))
    {
        index = index.saturating_add(1);
        16
    } else {
        10
    };
    let digits_start = index;
    let mut value = 0u32;
    while let Some(ch) = chars.get(index).copied() {
        if semicolon(ch) {
            if index == digits_start {
                return None;
            }
            return char::from_u32(value)
                .filter(|decoded| *decoded != '\0')
                .map(|decoded| (decoded, index + 1));
        }
        let digit = if radix == 16 {
            numeric_hex_digit_value(ch)?
        } else {
            numeric_decimal_digit_value(ch)?
        };
        value = value.checked_mul(radix)?.checked_add(digit)?;
        if value > 0x10ffff {
            return None;
        }
        index = index.saturating_add(1);
    }
    None
}

fn numeric_decimal_digit_value(ch: char) -> Option<u32> {
    match ch {
        '0'..='9' => Some(ch as u32 - '0' as u32),
        '０'..='９' => Some(ch as u32 - '０' as u32),
        _ => None,
    }
}

fn numeric_hex_digit_value(ch: char) -> Option<u32> {
    match ch {
        '0'..='9' => Some(ch as u32 - '0' as u32),
        '０'..='９' => Some(ch as u32 - '０' as u32),
        'a'..='f' => Some(10 + ch as u32 - 'a' as u32),
        'A'..='F' => Some(10 + ch as u32 - 'A' as u32),
        'ａ'..='ｆ' => Some(10 + ch as u32 - 'ａ' as u32),
        'Ａ'..='Ｆ' => Some(10 + ch as u32 - 'Ａ' as u32),
        _ => None,
    }
}

fn is_ascii_semicolon(ch: char) -> bool {
    ch == ';'
}

fn is_fullwidth_or_ascii_semicolon(ch: char) -> bool {
    matches!(ch, '；' | ';')
}

fn normalize_renderer_code(renderer_code: Option<&str>) -> Option<String> {
    let mut code = renderer_code?.trim().to_ascii_uppercase();
    if let Some(stripped) = code.strip_prefix("HC") {
        code = stripped.to_owned();
    }
    if let Some((before_ext, _)) = code.split_once('.') {
        code = before_ext.to_owned();
    }
    if code.is_empty() {
        None
    } else {
        Some(
            code.chars()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
        )
    }
}

fn normalize_gaiji_code(code: &str) -> String {
    code.chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_uppercase()
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

fn push_html_line_break(html: &mut String, text: &mut String) {
    if !html.ends_with("<br>") {
        html.push_str("<br>");
    }
    push_line_break(text);
}

fn push_html_text(html: &mut String, value: &str) {
    for ch in value.chars() {
        push_html_char(html, ch);
    }
}

fn push_html_char(html: &mut String, ch: char) {
    match ch {
        '&' => html.push_str("&amp;"),
        '<' => html.push_str("&lt;"),
        '>' => html.push_str("&gt;"),
        '"' => html.push_str("&quot;"),
        '\'' => html.push_str("&#39;"),
        _ => html.push(ch),
    }
}

fn push_html_attr(html: &mut String, value: &str) {
    for ch in value.chars() {
        push_html_char(html, ch);
    }
}

fn html_style_start_spec(op: u8) -> Option<(&'static str, &'static str)> {
    match op {
        0x04 => Some(("span", " class=\"lv-hc-halfwidth\"")),
        0x06 => Some(("sub", "")),
        0x0b => Some(("span", " class=\"lv-hc-literal\"")),
        0x0e => Some(("sup", "")),
        0x10 => Some(("i", "")),
        0x12 => Some(("em", "")),
        0x41 => Some(("span", " class=\"lv-hc-heading\"")),
        0xe0 => Some(("b", "")),
        _ => None,
    }
}

fn html_style_end_start_op(op: u8) -> Option<u8> {
    match op {
        0x05 => Some(0x04),
        0x07 => Some(0x06),
        0x0c => Some(0x0b),
        0x0f => Some(0x0e),
        0x11 => Some(0x10),
        0x13 => Some(0x12),
        0x61 => Some(0x41),
        0xe1 => Some(0xe0),
        _ => None,
    }
}

fn close_html_style(
    html: &mut String,
    style_stack: &mut Vec<HcHtmlStyle>,
    start_op: u8,
    halfwidth_depth: &mut usize,
) {
    let Some(position) = style_stack
        .iter()
        .rposition(|style| style.start_op == start_op)
    else {
        return;
    };
    while style_stack.len() > position {
        let Some(style) = style_stack.pop() else {
            return;
        };
        if style.start_op == 0x04 {
            *halfwidth_depth = halfwidth_depth.saturating_sub(1);
        }
        html.push_str("</");
        html.push_str(style.tag);
        html.push('>');
        if style.start_op == start_op {
            break;
        }
    }
}

fn decode_pointer_payload(payload: &[u8]) -> Option<(u32, u32)> {
    if payload.len() < 6 {
        return None;
    }
    let block = u32::from_be_bytes(payload.get(0..4)?.try_into().ok()?);
    let offset = u16::from_be_bytes(payload.get(4..6)?.try_into().ok()?);
    Some((block, u32::from(offset)))
}

fn append_link_start(html: &mut String, op: u8, target: Option<(u32, u32)>) {
    html.push_str("<a class=\"lv-hc-link\"");
    match target {
        Some((block, offset)) => {
            html.push_str(" href=\"");
            push_html_attr(html, &lvaddr_href(block, offset));
            html.push('"');
            html.push_str(" data-lv-block=\"");
            push_html_attr(html, &block.to_string());
            html.push('"');
            html.push_str(" data-lv-offset=\"");
            push_html_attr(html, &offset.to_string());
            html.push('"');
            html.push_str(" data-lv-link-status=\"resolved_address\"");
        }
        None => {
            html.push_str(
                " href=\"lvaddr://unresolved\" data-lv-link-status=\"unresolved_target\"",
            );
        }
    }
    html.push_str(" data-lv-control=\"1f");
    push_html_attr(html, &format!("{op:02x}"));
    html.push_str("\">");
}

fn lvaddr_href(block: u32, offset: u32) -> String {
    format!("lvaddr://{block:08}/{offset:04}")
}

fn append_media_placeholder(html: &mut String, op: u8, payload: &[u8], media_index: usize) {
    append_media_placeholder_for_control(
        html,
        &format!("1f{op:02x}"),
        &hex_lower(payload),
        media_index,
    );
}

fn append_media_placeholder_for_control(
    html: &mut String,
    control: &str,
    payload_hex: &str,
    media_index: usize,
) {
    html.push_str("<span class=\"lv-hc-media-placeholder\" data-lv-control=\"");
    push_html_attr(html, control);
    html.push('"');
    html.push_str(" data-lv-media-index=\"");
    push_html_attr(html, &media_index.to_string());
    html.push('"');
    if !payload_hex.is_empty() {
        html.push_str(" data-lv-payload=\"");
        push_html_attr(html, payload_hex);
        html.push('"');
    }
    html.push_str("></span>");
}

fn append_color_sample_html(
    html: &mut String,
    sample_key: &str,
    record: Option<&ColorSampleRecord>,
) {
    html.push_str("<span class=\"media lv-hc-color-sample");
    let Some(record) = record else {
        html.push_str(" lv-hc-color-sample-unresolved\" data-lv-color-sample=\"");
        push_html_attr(html, sample_key);
        html.push_str("\" data-lv-color-sample-status=\"unresolved_sample_key\">??</span>");
        return;
    };

    let rgb_hex = record.estimated_rgb_hex.as_deref();
    if rgb_hex.is_none() {
        html.push_str(" lv-hc-color-sample-unresolved");
    }
    html.push_str("\" data-lv-color-sample=\"");
    push_html_attr(html, sample_key);
    html.push_str("\" data-lv-color-sample-status=\"resolved_munsell\"");
    if !record.label.is_empty() {
        html.push_str(" data-lv-color-label=\"");
        push_html_attr(html, &record.label);
        html.push('"');
    }
    if !record.munsell.is_empty() {
        html.push_str(" data-lv-munsell=\"");
        push_html_attr(html, &record.munsell);
        html.push('"');
    }
    if let Some(rgb_hex) = rgb_hex {
        html.push_str(" data-lv-rgb-estimated=\"");
        push_html_attr(html, rgb_hex);
        html.push_str("\" style=\"background-color:#");
        push_html_attr(html, rgb_hex);
        html.push('"');
    }
    let title = match (!record.label.is_empty(), !record.munsell.is_empty()) {
        (true, true) => Some(format!("{} {}", record.label, record.munsell)),
        (true, false) => Some(record.label.clone()),
        (false, true) => Some(record.munsell.clone()),
        (false, false) => None,
    };
    if let Some(title) = title {
        html.push_str(" title=\"");
        push_html_attr(html, &title);
        html.push_str("\" aria-label=\"");
        push_html_attr(html, &title);
        html.push('"');
    }
    html.push_str(">&#12288;&#12288;</span>");
}

fn hex_lower(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 2);
    for byte in data {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn narrow_fullwidth_ascii(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '\u{3000}' => ' ',
            '\u{2212}' => '-',
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(ch as u32 - 0xfee0).unwrap_or(ch),
            _ => ch,
        })
        .collect()
}

fn decode_unicode_scalar_run(data: &[u8], start: usize) -> Option<(String, usize)> {
    let mut cursor = start;
    let mut digits = String::new();
    while cursor + 1 < data.len() {
        if data[cursor] == 0x1f && data[cursor + 1] == 0x0c {
            if digits.is_empty() || !digits.len().is_multiple_of(4) {
                return None;
            }
            let mut decoded = String::new();
            for chunk in digits.as_bytes().chunks_exact(4) {
                let scalar = std::str::from_utf8(chunk).ok()?;
                let codepoint = u32::from_str_radix(scalar, 16).ok()?;
                let ch = char::from_u32(codepoint)?;
                decoded.push(ch);
            }
            return Some((decoded, cursor + 2));
        }
        if data[cursor] == 0x1f {
            return None;
        }
        let ch = decode_jis_pair(data[cursor], data[cursor + 1])?;
        digits.push(ascii_hex_digit(ch)?);
        cursor += 2;
    }
    None
}

fn ascii_hex_digit(ch: char) -> Option<char> {
    let ascii = match ch {
        '0'..='9' | 'A'..='F' | 'a'..='f' => ch,
        '０'..='９' => char::from_u32(ch as u32 - '０' as u32 + '0' as u32)?,
        'Ａ'..='Ｆ' => char::from_u32(ch as u32 - 'Ａ' as u32 + 'A' as u32)?,
        'ａ'..='ｆ' => char::from_u32(ch as u32 - 'ａ' as u32 + 'a' as u32)?,
        _ => return None,
    };
    Some(ascii.to_ascii_uppercase())
}

pub(crate) fn hc_control_arg_length(data: &[u8], offset: usize) -> usize {
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

    use crate::ssed_color_sample::{ColorSampleRecord, ColorSampleRgbStatus};

    use super::{
        HcBasicTextGaiji, HcCommonHtmlColorSampleStatus, HcCommonHtmlGaiji,
        decode_hc_html_numeric_character_references, decode_hc_stream_basic_text,
        decode_hc_stream_basic_text_with_gaiji, decode_hc_stream_basic_text_with_gaiji_policy,
        decode_hc_stream_common_html, decode_hc_stream_common_html_with_gaiji,
        decode_hc_stream_common_html_with_gaiji_render_policy,
        decode_hc_stream_common_html_with_gaiji_render_policy_and_color_samples,
        decode_hc_text_numeric_character_references, hc_marker_profile_for_renderer,
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

    #[test]
    fn basic_text_suppresses_profile_nonliteral_gaiji_markers_before_resolution() {
        let marker_profile = hc_marker_profile_for_renderer(Some("HC013A.dll"));
        let mut data = body_jis("前");
        data.extend_from_slice(&[0xb2, 0x64]);
        data.extend_from_slice(&body_jis("後"));

        let rendered = decode_hc_stream_basic_text_with_gaiji_policy(
            &data,
            |_code| {
                Some(HcBasicTextGaiji {
                    text: "〓".to_owned(),
                    resolved: false,
                })
            },
            |code| marker_profile.suppresses_gaiji_code(code),
        );

        assert_eq!(rendered.text, "前後");
        assert_eq!(rendered.stats.suppressed_gaiji_pairs, 1);
        assert_eq!(rendered.stats.placeholder_gaiji_pairs, 0);
        assert!(
            rendered
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "hc_basic_text_gaiji_placeholders")
        );
    }

    #[test]
    fn basic_text_suppresses_private_spans_like_logovista_tools() {
        let mut data = body_jis("前");
        data.extend_from_slice(&[0x1f, 0xe2, 0x00, 0x00]);
        data.extend_from_slice(&body_jis("隠"));
        data.extend_from_slice(&[0xb1, 0x23]);
        data.extend_from_slice(&[0x1f, 0xe3]);
        data.extend_from_slice(&body_jis("後"));

        let rendered = decode_hc_stream_basic_text_with_gaiji(&data, |_code| {
            panic!("private gaiji should not be resolved into visible text")
        });

        assert_eq!(rendered.text, "前後");
        assert_eq!(rendered.stats.private_controls, 2);
        assert_eq!(rendered.stats.suppressed_gaiji_pairs, 1);
    }

    #[test]
    fn marker_profiles_capture_known_renderer_gaiji_controls_and_aliases() {
        let hc013c = hc_marker_profile_for_renderer(Some("HC013C.dll"));
        assert!(hc013c.suppresses_gaiji_code("B121"));
        assert!(hc013c.suppresses_gaiji_code("b122"));

        let hc0158 = hc_marker_profile_for_renderer(Some("HC0158.dll"));
        assert!(hc0158.suppresses_gaiji_code("B353"));
        assert!(hc0158.suppresses_gaiji_code("B37E"));
        assert!(!hc0158.suppresses_gaiji_code("B352"));

        let hc00a3 = hc_marker_profile_for_renderer(Some("HC00A3.dll"));
        assert_eq!(hc00a3.gaiji_lookup_code("B261"), "B167");
        assert_eq!(hc00a3.gaiji_lookup_code("B121"), "B121");
    }

    #[test]
    fn common_html_renders_balanced_common_styles_and_halfwidth_text() {
        let mut data = body_jis("見出し");
        data.extend_from_slice(&[0x1f, 0x0a]);
        data.extend_from_slice(&[0x1f, 0x04]);
        data.extend_from_slice(&body_jis("ＡＢＣ"));
        data.extend_from_slice(&[0x1f, 0x05]);
        data.extend_from_slice(&[0x1f, 0x06]);
        data.extend_from_slice(&body_jis("小"));
        data.extend_from_slice(&[0x1f, 0x07]);
        data.extend_from_slice(&[0x1f, 0xe0, 0x00, 0x00]);
        data.extend_from_slice(&body_jis("太"));
        data.extend_from_slice(&[0x1f, 0xe1]);

        let rendered = decode_hc_stream_common_html(&data);

        assert_eq!(rendered.text, "見出し\nABC小太");
        assert_eq!(rendered.stats.line_breaks, 1);
        assert_eq!(rendered.stats.style_controls, 6);
        assert_eq!(
            rendered.html,
            "<div class=\"lv-hc-common-html-fallback\">見出し<br><span class=\"lv-hc-halfwidth\">ABC</span><sub>小</sub><b>太</b></div>"
        );
        assert!(rendered.diagnostics.is_empty());
    }

    #[test]
    fn common_html_renders_links_and_closes_unterminated_state() {
        let mut data = body_jis("前");
        data.extend_from_slice(&[
            0x1f, 0x44, 0xaa, 0xbb, 0xcc, 0xdd, 0x00, 0x00, 0x00, 0x03, 0x12, 0x34,
        ]);
        data.extend_from_slice(&body_jis("リンク"));
        data.extend_from_slice(&[0x1f, 0x64, 0, 0, 0, 0, 0, 0]);

        let rendered = decode_hc_stream_common_html(&data);

        assert_eq!(rendered.text, "前リンク");
        assert_eq!(rendered.links.len(), 1);
        assert_eq!(rendered.links[0].href, "lvaddr://00000003/4660");
        assert_eq!(rendered.links[0].control, "1f44");
        assert!(
            rendered.html.contains(
                "<a class=\"lv-hc-link\" href=\"lvaddr://00000003/4660\" data-lv-block=\"3\" data-lv-offset=\"4660\" data-lv-link-status=\"resolved_address\" data-lv-control=\"1f44\">リンク</a>"
            )
        );
        assert!(rendered.diagnostics.is_empty());
    }

    #[test]
    fn common_html_suppresses_private_spans_like_logovista_tools() {
        let mut data = body_jis("前");
        data.extend_from_slice(&[0x1f, 0xe2, 0x00, 0x00]);
        data.extend_from_slice(&[
            0x1f, 0x44, 0xaa, 0xbb, 0xcc, 0xdd, 0x00, 0x00, 0x00, 0x03, 0x12, 0x34,
        ]);
        data.extend_from_slice(&body_jis("隠"));
        data.extend_from_slice(&sounddata_marker("00007F93"));
        data.extend_from_slice(&[0x1f, 0x64, 0, 0, 0, 0, 0, 0]);
        data.extend_from_slice(&[0x1f, 0xe3]);
        data.extend_from_slice(&body_jis("後"));

        let rendered = decode_hc_stream_common_html(&data);

        assert_eq!(rendered.text, "前後");
        assert_eq!(
            rendered.html,
            "<div class=\"lv-hc-common-html-fallback\">前後</div>"
        );
        assert!(rendered.links.is_empty());
        assert!(rendered.media.is_empty());
        assert_eq!(rendered.stats.private_controls, 4);
        assert_eq!(rendered.stats.media_controls, 1);
    }

    #[test]
    fn common_html_records_media_placeholders_with_stable_indexes() {
        let mut data = body_jis("音");
        data.extend_from_slice(&[
            0x1f, 0x4a, 0x00, 0x01, 0x00, 0x00, 0x12, 0x34, 0x00, 0x00, 0x12, 0x35, 0x00, 0x00,
            0x12, 0x36, 0x00, 0x00,
        ]);
        data.extend_from_slice(&[0x1f, 0x64, 0x00, 0x00, 0x12, 0x00, 0x00, 0x17]);

        let rendered = decode_hc_stream_common_html(&data);

        assert_eq!(rendered.media.len(), 2);
        assert_eq!(rendered.media[0].index, 0);
        assert_eq!(rendered.media[0].control, "1f4a");
        assert_eq!(rendered.media[1].index, 1);
        assert_eq!(rendered.media[1].control, "1f64");
        assert!(rendered.html.contains("data-lv-media-index=\"0\""));
        assert!(rendered.html.contains("data-lv-media-index=\"1\""));
    }

    #[test]
    fn common_html_renders_resolved_and_unresolved_color_samples() {
        let data = [0x1f, 0x14, 0x1e, 0x01, 0x1f, 0x15, 0x1f, 0x14, 0x1e, 0x02];
        let record = ColorSampleRecord {
            record_index: 0,
            sample_number: 1,
            sample_key: Some("1e01".to_owned()),
            munsell_raw_hex: "f2d7c2f361f5".to_owned(),
            munsell: "2PB3/5".to_owned(),
            label_raw_hex: "4d75".to_owned(),
            label: "藍".to_owned(),
            estimated_rgb: Some([58, 57, 96]),
            estimated_rgb_hex: Some("3a3960".to_owned()),
            rgb_status: ColorSampleRgbStatus::EstimatedFromMunsell,
        };

        let rendered = decode_hc_stream_common_html_with_gaiji_render_policy_and_color_samples(
            &data,
            |_code| None,
            |_code| false,
            |sample_key| (sample_key == "1e01").then(|| record.clone()),
        );

        assert_eq!(rendered.stats.color_samples, 2);
        assert_eq!(rendered.stats.color_sample_ends, 1);
        assert_eq!(rendered.color_samples.len(), 2);
        assert_eq!(
            rendered.color_samples[0].status,
            HcCommonHtmlColorSampleStatus::ResolvedMunsell
        );
        assert_eq!(
            rendered.color_samples[1].status,
            HcCommonHtmlColorSampleStatus::UnresolvedSampleKey
        );
        assert!(rendered.html.contains("data-lv-color-sample=\"1e01\""));
        assert!(rendered.html.contains("data-lv-munsell=\"2PB3/5\""));
        assert!(rendered.html.contains("data-lv-rgb-estimated=\"3a3960\""));
        assert!(rendered.html.contains("style=\"background-color:#3a3960\""));
        assert!(rendered.html.contains("data-lv-color-sample=\"1e02\""));
        assert!(
            rendered
                .html
                .contains("data-lv-color-sample-status=\"unresolved_sample_key\"")
        );
    }

    #[test]
    fn common_html_decodes_unicode_scalar_runs_between_1f0b_and_1f0c() {
        let mut data = body_jis("前");
        data.extend_from_slice(&[0x1f, 0x0b]);
        data.extend_from_slice(&body_jis("00B7201302C803B5"));
        data.extend_from_slice(&[0x1f, 0x0c]);
        data.extend_from_slice(&body_jis("後"));

        let rendered = decode_hc_stream_common_html(&data);

        assert_eq!(rendered.text, "前·–ˈε後");
        assert!(rendered.html.contains("前·–ˈε後"));
        assert!(!rendered.html.contains("00B7"));
        assert_eq!(rendered.stats.style_controls, 2);
    }

    #[test]
    fn basic_text_decodes_unicode_scalar_runs_between_1f0b_and_1f0c() {
        let mut data = body_jis("前");
        data.extend_from_slice(&[0x1f, 0x0b]);
        data.extend_from_slice(&body_jis("257102C8"));
        data.extend_from_slice(&[0x1f, 0x0c]);
        data.extend_from_slice(&body_jis("後"));

        let rendered = decode_hc_stream_basic_text(&data);

        assert_eq!(rendered.text, "前╱ˈ後");
        assert_eq!(rendered.stats.style_controls, 2);
    }

    #[test]
    fn basic_text_decodes_numeric_character_references_stored_as_body_text() {
        let mut data = body_jis("前＆＃ｘ９５７８；");
        data.extend_from_slice(&body_jis("&#38263;後"));

        let rendered = decode_hc_stream_basic_text(&data);

        assert_eq!(rendered.text, "前镸長後");
        assert_eq!(rendered.stats.numeric_character_references, 2);
        assert!(
            rendered
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "hc_numeric_character_references_decoded")
        );
    }

    #[test]
    fn common_html_decodes_numeric_character_references_without_decoding_own_escapes() {
        let mut data = body_jis("前＆＃ｘ９５７８；");
        data.extend_from_slice(&body_jis("＆＃３８２６３；"));
        data.push(b'\'');

        let rendered = decode_hc_stream_common_html(&data);

        assert_eq!(rendered.text, "前镸長'");
        assert_eq!(rendered.stats.numeric_character_references, 2);
        assert!(rendered.html.contains("前镸長&#39;"));
        assert!(!rendered.html.contains("＆＃ｘ９５７８；"));
        assert!(rendered.html.contains("&#39;"));
    }

    #[test]
    fn numeric_character_reference_decoder_accepts_observed_ascii_variants() {
        let (text, text_refs) =
            decode_hc_text_numeric_character_references("&#x9578;&#38263;＆＃ｘ９５７７；");
        let (html, html_refs) =
            decode_hc_html_numeric_character_references("&amp;#x9578;&amp;#38263;＆＃ｘ９５７７；");

        assert_eq!(text, "镸長長");
        assert_eq!(text_refs, 3);
        assert_eq!(html, "镸長長");
        assert_eq!(html_refs, 3);
    }

    #[test]
    fn common_html_records_sounddata_marker_as_media_without_leaking_id() {
        let mut data = body_jis("音");
        data.extend_from_slice(&sounddata_marker("00007F93"));
        data.extend_from_slice(&body_jis("声"));

        let rendered = decode_hc_stream_common_html(&data);

        assert_eq!(rendered.text, "音声");
        assert_eq!(rendered.media.len(), 1);
        assert_eq!(rendered.media[0].control, "sounddata");
        assert_eq!(rendered.media[0].payload_hex, "00007f93");
        assert!(rendered.html.contains("data-lv-control=\"sounddata\""));
        assert!(!rendered.html.contains("00007F93"));
        assert_eq!(rendered.stats.media_controls, 1);
    }

    #[test]
    fn basic_text_suppresses_sounddata_marker_without_leaking_id() {
        let mut data = body_jis("音");
        data.extend_from_slice(&sounddata_marker("00007F93"));
        data.extend_from_slice(&body_jis("声"));

        let rendered = decode_hc_stream_basic_text(&data);

        assert_eq!(rendered.text, "音声");
        assert_eq!(rendered.stats.media_controls, 1);
    }

    #[test]
    fn common_html_resolves_gaiji_without_python_bytes_repr_or_raw_codes() {
        let mut data = body_jis("本文");
        data.extend_from_slice(&[0xb1, 0x23]);
        data.extend_from_slice(&[0xb9, 0x99]);

        let rendered = decode_hc_stream_common_html_with_gaiji(&data, |code| match code {
            "B123" => Some(HcBasicTextGaiji {
                text: "<一>".to_owned(),
                resolved: true,
            }),
            "B999" => Some(HcBasicTextGaiji {
                text: "〓".to_owned(),
                resolved: false,
            }),
            _ => None,
        });

        assert_eq!(rendered.text, "本文<一>〓");
        assert!(rendered.html.contains("本文&lt;一&gt;〓"));
        assert!(!rendered.html.contains("B123"));
        assert_eq!(rendered.stats.resolved_gaiji_pairs, 1);
        assert_eq!(rendered.stats.placeholder_gaiji_pairs, 1);
    }

    #[test]
    fn common_html_can_render_resource_backed_gaiji_fragments() {
        let mut data = body_jis("本文");
        data.extend_from_slice(&[0xb1, 0x23]);
        let fragment = "<img class=\"lvcore-gaiji lvcore-gaiji-external\" src=\"lvcore://resource/token\" alt=\"〓\" title=\"B123\">";

        let rendered = decode_hc_stream_common_html_with_gaiji_render_policy(
            &data,
            |code| {
                (code == "B123").then(|| HcCommonHtmlGaiji {
                    text: "〓".to_owned(),
                    html: Some(fragment.to_owned()),
                    resolved: true,
                })
            },
            |_code| false,
        );

        assert_eq!(rendered.text, "本文〓");
        assert!(rendered.html.contains("lvcore://resource/token"));
        assert_eq!(rendered.stats.resolved_gaiji_pairs, 1);
        assert_eq!(rendered.stats.placeholder_gaiji_pairs, 0);
        assert!(
            rendered
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "hc_basic_text_gaiji_placeholders")
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

    fn sounddata_marker(code_hex: &str) -> Vec<u8> {
        let mut data = vec![0xa4, 0x27];
        data.extend_from_slice(&body_jis(code_hex));
        data.extend_from_slice(&[0xa4, 0x28]);
        data
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

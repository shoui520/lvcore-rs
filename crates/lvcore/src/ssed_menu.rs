use encoding_rs::SHIFT_JIS;
use serde::{Deserialize, Serialize};

use crate::gaiji::logovista_gaiji_placeholder;
use crate::ssed::BLOCK_SIZE;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedMenuDestination {
    pub block: u32,
    pub offset: u32,
    pub encoding: SsedMenuDestinationEncoding,
}

impl SsedMenuDestination {
    pub fn is_null(&self) -> bool {
        self.block == 0 && self.offset == 0
    }

    pub fn absolute_offset(&self) -> u64 {
        u64::from(self.block.saturating_sub(1)) * u64::from(BLOCK_SIZE) + u64::from(self.offset)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SsedMenuDestinationEncoding {
    Bcd,
    BigEndian,
    TocBigEndian,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedMenuLink {
    pub label: String,
    pub destination: Option<SsedMenuDestination>,
    pub control: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedMenuRecord {
    pub line_index: usize,
    pub section_codes: Vec<String>,
    pub text: String,
    pub links: Vec<SsedMenuLink>,
    pub depth: usize,
}

impl SsedMenuRecord {
    pub fn label(&self) -> &str {
        if !self.text.is_empty() {
            &self.text
        } else if let Some(link) = self.links.first() {
            &link.label
        } else {
            ""
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedMenuParse {
    pub records: Vec<SsedMenuRecord>,
    pub unknown_controls: usize,
    pub empty_sentinel: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedMenuPageParse {
    pub records: Vec<SsedMenuRecord>,
    pub unknown_controls: usize,
    pub empty_sentinel: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug)]
struct ActiveLink {
    control: String,
    parts: Vec<String>,
    start_payload: Option<Vec<u8>>,
}

#[derive(Debug)]
struct LineBuilder {
    line_index: usize,
    parts: Vec<String>,
    section_codes: Vec<String>,
    links: Vec<SsedMenuLink>,
    active_link: Option<ActiveLink>,
}

impl LineBuilder {
    fn new(line_index: usize) -> Self {
        Self {
            line_index,
            parts: Vec::new(),
            section_codes: Vec::new(),
            links: Vec::new(),
            active_link: None,
        }
    }

    fn add_text(&mut self, value: impl Into<String>) {
        let value = value.into();
        if let Some(link) = &mut self.active_link {
            link.parts.push(value.clone());
        }
        self.parts.push(value);
    }
}

pub fn parse_menu_stream(data: &[u8]) -> SsedMenuParse {
    parse_menu_stream_inner(data, None).0
}

pub fn parse_menu_stream_page(data: &[u8], offset: usize, limit: usize) -> SsedMenuPageParse {
    if limit == 0 {
        return SsedMenuPageParse {
            records: Vec::new(),
            unknown_controls: 0,
            empty_sentinel: false,
            next_cursor: None,
        };
    }
    let (parsed, next_record_index) = parse_menu_stream_inner(data, Some((offset, limit)));
    SsedMenuPageParse {
        records: parsed.records,
        unknown_controls: parsed.unknown_controls,
        empty_sentinel: parsed.empty_sentinel,
        next_cursor: next_record_index.map(|index| index.to_string()),
    }
}

fn parse_menu_stream_inner(
    data: &[u8],
    page: Option<(usize, usize)>,
) -> (SsedMenuParse, Option<usize>) {
    let mut records = Vec::new();
    let mut line = LineBuilder::new(1);
    let mut i = 0usize;
    let mut halfwidth_depth = 0usize;
    let mut private_depth = 0usize;
    let mut unknown_controls = 0usize;
    let mut record_count = 0usize;
    let mut next_record_index = None;
    let mut stopped = false;

    while i < data.len() {
        let b = data[i];
        if b == 0 {
            i += 1;
            continue;
        }

        if b == 0x1f && i + 1 < data.len() {
            let op = data[i + 1];
            match op {
                0x09 => {
                    if i + 4 <= data.len() && private_depth == 0 {
                        line.section_codes.push(hex_lower(&data[i + 2..i + 4]));
                    }
                    i += 4;
                    continue;
                }
                0x0a => {
                    if private_depth == 0 {
                        stopped = finish_parsed_line(
                            &mut records,
                            line,
                            &mut record_count,
                            page,
                            &mut next_record_index,
                        );
                        line = LineBuilder::new(record_count + 1);
                        if stopped {
                            break;
                        }
                    }
                    i += 2;
                    continue;
                }
                0x42 | 0x43 => {
                    if private_depth == 0 {
                        line.active_link = Some(ActiveLink {
                            control: format!("1f{op:02x}"),
                            parts: Vec::new(),
                            start_payload: None,
                        });
                    }
                    i += 2;
                    continue;
                }
                0x49 | 0x4a => {
                    let arg_len = control_arg_length(data, i);
                    if private_depth == 0 {
                        let payload = data
                            .get(i + 2..i + 2 + arg_len)
                            .unwrap_or_default()
                            .to_vec();
                        line.active_link = Some(ActiveLink {
                            control: format!("1f{op:02x}"),
                            parts: Vec::new(),
                            start_payload: (!payload.is_empty()).then_some(payload),
                        });
                    }
                    i += 2 + arg_len;
                    continue;
                }
                0x62 | 0x63 => {
                    let destination = data.get(i + 2..i + 8).and_then(parse_menu_destination);
                    if private_depth == 0 {
                        finish_active_link(&mut line, destination);
                    }
                    i += 8;
                    continue;
                }
                0x69 => {
                    let destination = line
                        .active_link
                        .as_ref()
                        .and_then(|link| link.start_payload.as_deref())
                        .and_then(parse_toc_destination);
                    if private_depth == 0 {
                        finish_active_link(&mut line, destination);
                    }
                    i += 2;
                    continue;
                }
                0x6a => {
                    if private_depth == 0 {
                        finish_active_link(&mut line, None);
                    }
                    i += 2;
                    continue;
                }
                0x41 | 0xe0 | 0xe2 => {
                    if op == 0xe2 {
                        private_depth += 1;
                    }
                    i += 4;
                    continue;
                }
                0x04 => {
                    halfwidth_depth += 1;
                    i += 2;
                    continue;
                }
                0x05 => {
                    halfwidth_depth = halfwidth_depth.saturating_sub(1);
                    i += 2;
                    continue;
                }
                0x00 | 0x02 | 0x03 | 0x61 | 0xe1 | 0xe3 => {
                    if op == 0xe3 {
                        private_depth = private_depth.saturating_sub(1);
                    }
                    i += 2;
                    continue;
                }
                _ => {
                    if !is_known_nonprinting_control(op) {
                        unknown_controls += 1;
                    }
                    i += 2 + control_arg_length(data, i);
                    continue;
                }
            }
        }

        if i + 1 < data.len() && (0x21..=0x7e).contains(&b) && (0x21..=0x7e).contains(&data[i + 1])
        {
            if private_depth == 0
                && let Some(ch) = decode_jis_pair(b, data[i + 1])
            {
                let value = if halfwidth_depth > 0 {
                    narrow_fullwidth_ascii_char(ch).to_string()
                } else {
                    ch.to_string()
                };
                line.add_text(value);
            }
            i += 2;
            continue;
        }

        if i + 1 < data.len() && (0xa1..=0xfe).contains(&b) {
            if private_depth == 0 {
                line.add_text(gaiji_placeholder(b, data[i + 1]));
            }
            i += 2;
            continue;
        }

        i += 1;
    }

    if !stopped {
        let _ = finish_parsed_line(
            &mut records,
            line,
            &mut record_count,
            page,
            &mut next_record_index,
        );
    }
    let empty_sentinel = record_count == 0 && is_empty_menu_sentinel(data);
    annotate_depths(&mut records);
    (
        SsedMenuParse {
            records,
            unknown_controls,
            empty_sentinel,
        },
        next_record_index,
    )
}

pub fn is_empty_menu_sentinel(data: &[u8]) -> bool {
    data.starts_with(&[0x1f, 0x03]) && data[2..].iter().all(|byte| *byte == 0)
}

fn finish_active_link(line: &mut LineBuilder, destination: Option<SsedMenuDestination>) {
    let Some(link) = line.active_link.take() else {
        return;
    };
    let label = clean_text(&link.parts.join(""));
    line.links.push(SsedMenuLink {
        label,
        destination,
        control: link.control,
    });
}

fn finish_line_record(mut line: LineBuilder) -> Option<SsedMenuRecord> {
    if line.active_link.is_some() {
        finish_active_link(&mut line, None);
    }
    let text = clean_text(&line.parts.join(""));
    if text.is_empty() && line.section_codes.is_empty() && line.links.is_empty() {
        return None;
    }
    Some(SsedMenuRecord {
        line_index: line.line_index,
        section_codes: line.section_codes,
        text,
        links: line.links,
        depth: 1,
    })
}

fn finish_parsed_line(
    records: &mut Vec<SsedMenuRecord>,
    line: LineBuilder,
    record_count: &mut usize,
    page: Option<(usize, usize)>,
    next_record_index: &mut Option<usize>,
) -> bool {
    let Some(record) = finish_line_record(line) else {
        return false;
    };
    let index = *record_count;
    *record_count += 1;
    let Some((offset, limit)) = page else {
        records.push(record);
        return false;
    };
    let page_end = offset.saturating_add(limit);
    if index < offset {
        return false;
    }
    if index >= page_end {
        *next_record_index = Some(page_end);
        return true;
    }
    records.push(record);
    false
}

fn annotate_depths(records: &mut [SsedMenuRecord]) {
    let mut section_codes: Vec<String> = records
        .iter()
        .filter_map(|record| record.section_codes.first().cloned())
        .collect();
    section_codes.sort();
    section_codes.dedup();

    for record in records {
        if let Some(code) = record.section_codes.first() {
            record.depth = section_codes
                .iter()
                .position(|candidate| candidate == code)
                .map(|index| index + 1)
                .unwrap_or(1);
        }
    }
}

pub fn parse_menu_destination(payload: &[u8]) -> Option<SsedMenuDestination> {
    if payload.len() != 6 {
        return None;
    }
    if let (Some(block), Some(offset)) = (
        decode_bcd_decimal(&payload[..4]),
        decode_bcd_decimal(&payload[4..]),
    ) {
        return Some(SsedMenuDestination {
            block,
            offset,
            encoding: SsedMenuDestinationEncoding::Bcd,
        });
    }
    Some(SsedMenuDestination {
        block: be32(payload, 0),
        offset: u32::from(be16(payload, 4)),
        encoding: SsedMenuDestinationEncoding::BigEndian,
    })
}

pub fn parse_toc_destination(payload: &[u8]) -> Option<SsedMenuDestination> {
    if payload.len() != 10 {
        return None;
    }
    let target = &payload[4..10];
    Some(SsedMenuDestination {
        block: be32(target, 0),
        offset: u32::from(be16(target, 4)),
        encoding: SsedMenuDestinationEncoding::TocBigEndian,
    })
}

fn clean_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_bcd_decimal(data: &[u8]) -> Option<u32> {
    let mut value = 0u32;
    for byte in data {
        let high = byte >> 4;
        let low = byte & 0x0f;
        if high > 9 || low > 9 {
            return None;
        }
        value = value * 100 + u32::from(high) * 10 + u32::from(low);
    }
    Some(value)
}

fn control_arg_length(data: &[u8], offset: usize) -> usize {
    if offset + 1 >= data.len() || data[offset] != 0x1f {
        return 0;
    }
    let op = data[offset + 1];
    match op {
        0x09 | 0x1a | 0x1c | 0x41 | 0x4c | 0xe0 | 0xe2 | 0xe4 | 0xe6 => 2,
        0x36 => 12,
        0x37 | 0x44 | 0x48 | 0x49 => 10,
        0x4a => match be16_checked(data, offset + 2).map(|word| word & 0x000f) {
            Some(0) => 14,
            Some(1 | 2) => 16,
            Some(_) => 2,
            None => 16,
        },
        0x4b | 0x62 | 0x63 | 0x64 => 6,
        0x4d => 18,
        0x4e => match be16_checked(data, offset + 2).map(|word| word & 0x0f00) {
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

fn is_known_nonprinting_control(op: u8) -> bool {
    matches!(
        op,
        0x00 | 0x02
            | 0x03
            | 0x1a
            | 0x1c
            | 0x36
            | 0x37
            | 0x48
            | 0x4b
            | 0x4c
            | 0x4e
            | 0x4f
            | 0xe4
            | 0xe6
    )
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
    (!had_errors).then(|| decoded.chars().next()).flatten()
}

fn narrow_fullwidth_ascii_char(ch: char) -> char {
    match ch {
        '\u{2212}' => '-',
        '\u{ff01}'..='\u{ff5e}' => char::from_u32(ch as u32 - 0xfee0).unwrap_or(ch),
        '\u{3000}' => ' ',
        _ => ch,
    }
}

fn gaiji_placeholder(first: u8, second: u8) -> String {
    let identity = format!("{first:02X}{second:02X}");
    logovista_gaiji_placeholder(&identity).unwrap_or_else(|| format!("<z{identity}>"))
}

fn hex_lower(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn be16(data: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([data[offset], data[offset + 1]])
}

fn be16_checked(data: &[u8], offset: usize) -> Option<u16> {
    (offset + 2 <= data.len()).then(|| be16(data, offset))
}

fn be32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bcd_menu_link_destination() {
        let data = b"\x1f\x09\x00\x01\x1f\x42\x24\x22\x1f\x62\x00\x00\x00\x10\x00\x05\x1f\x0a";
        let parsed = parse_menu_stream(data);

        assert_eq!(parsed.records.len(), 1);
        assert!(!parsed.empty_sentinel);
        assert_eq!(parsed.records[0].label(), "あ");
        assert_eq!(parsed.records[0].depth, 1);
        let destination = parsed.records[0].links[0].destination.as_ref().unwrap();
        assert_eq!(destination.block, 10);
        assert_eq!(destination.offset, 5);
        assert_eq!(destination.encoding, SsedMenuDestinationEncoding::Bcd);
    }

    #[test]
    fn recognizes_empty_navigation_sentinel() {
        let data = b"\x1f\x03\x00\x00\x00\x00";
        let parsed = parse_menu_stream(data);

        assert!(parsed.records.is_empty());
        assert!(parsed.empty_sentinel);
        assert!(is_empty_menu_sentinel(data));
    }

    #[test]
    fn parses_menu_stream_pages_without_materializing_all_records() {
        let data = concat!(
            "\x1f\x09\x00\x01\x1f\x42\x24\x22\x1f\x62\x00\x00\x00\x10\x00\x01\x1f\x0a",
            "\x1f\x09\x00\x01\x1f\x42\x24\x24\x1f\x62\x00\x00\x00\x10\x00\x02\x1f\x0a",
            "\x1f\x09\x00\x01\x1f\x42\x24\x26\x1f\x62\x00\x00\x00\x10\x00\x03\x1f\x0a",
        );
        let first = parse_menu_stream_page(data.as_bytes(), 0, 2);
        assert_eq!(first.records.len(), 2);
        assert_eq!(first.records[0].label(), "あ");
        assert_eq!(first.records[1].label(), "い");
        assert_eq!(first.next_cursor.as_deref(), Some("2"));

        let second = parse_menu_stream_page(data.as_bytes(), 2, 2);
        assert_eq!(second.records.len(), 1);
        assert_eq!(second.records[0].label(), "う");
        assert!(second.next_cursor.is_none());
    }

    #[test]
    fn menu_gaiji_placeholders_use_logovista_halfwidth_marker_family() {
        let data = b"\x1f\x09\x00\x01\xa1\x40\xb1\x23\x1f\x0a";
        let parsed = parse_menu_stream(data);

        assert_eq!(parsed.records[0].label(), "<hA140><zB123>");
    }

    #[test]
    fn truncated_controls_do_not_panic() {
        let data = [
            0x1f, 0x09, 0x00, 0x01, 0x1f, 0x49, 0x00, 0x01, 0x00, 0x00, 0x00, 0x10, 0x00, 0x05,
            0x00, 0x00, 0x24, 0x22, 0x1f, 0x62, 0x00, 0x00, 0x00, 0x10, 0x00, 0x05, 0x1f, 0x4f,
            0x00, 0x00, 0x00,
        ];
        for len in 0..=data.len() {
            let _ = parse_menu_stream(&data[..len]);
        }
    }
}

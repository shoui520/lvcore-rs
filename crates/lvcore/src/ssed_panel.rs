use encoding_rs::SHIFT_JIS;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedPanelDataRef {
    pub panel_id: String,
    pub panel_type: String,
    pub title: String,
    pub filename: String,
    pub data_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedPanelInlineCell {
    pub panel_id: String,
    pub panel_type: String,
    pub title: String,
    pub cell_index: u32,
    pub row: Option<u32>,
    pub column: Option<u32>,
    pub label: String,
    pub ref_id: String,
    pub action_verb: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedPanelXml {
    pub data_refs: Vec<SsedPanelDataRef>,
    pub inline_cells: Vec<SsedPanelInlineCell>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedPanelBinRecord {
    pub index: u32,
    pub record_id: Option<u32>,
    pub block: u32,
    pub offset: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedPanelBin {
    pub declared_record_count: u32,
    pub actual_record_count: u32,
    pub text_width: u32,
    pub format: String,
    pub records: Vec<SsedPanelBinRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CurrentPanel {
    panel_id: String,
    panel_type: String,
    title: String,
    columns: Option<u32>,
    cell_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CurrentCell {
    ref_id: String,
    action_verb: String,
    text: String,
}

pub fn parse_panel_xml_bytes(data: &[u8]) -> Result<SsedPanelXml> {
    let text = decode_xml_bytes(data);
    parse_panel_xml(&text)
}

pub fn parse_panel_xml(xml: &str) -> Result<SsedPanelXml> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut parsed = SsedPanelXml {
        data_refs: Vec::new(),
        inline_cells: Vec::new(),
    };
    let mut panel: Option<CurrentPanel> = None;
    let mut in_title = false;
    let mut in_data = false;
    let mut current_cell: Option<CurrentCell> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.name().as_ref() == b"panel" => {
                panel = Some(CurrentPanel {
                    panel_id: attr_value(&event, b"index").unwrap_or_default(),
                    panel_type: attr_value(&event, b"paneltype").unwrap_or_default(),
                    title: String::new(),
                    columns: attr_value(&event, b"count_x").and_then(|value| value.parse().ok()),
                    cell_index: 0,
                });
            }
            Ok(Event::End(event)) if event.name().as_ref() == b"panel" => {
                panel = None;
            }
            Ok(Event::Start(event)) if event.name().as_ref() == b"title" => {
                in_title = true;
            }
            Ok(Event::End(event)) if event.name().as_ref() == b"title" => {
                in_title = false;
            }
            Ok(Event::Start(event)) if event.name().as_ref() == b"data" => {
                if let Some(filename) = attr_value(&event, b"filename") {
                    push_data_ref(&mut parsed, panel.as_ref(), &event, filename);
                } else {
                    in_data = true;
                }
            }
            Ok(Event::Empty(event)) if event.name().as_ref() == b"data" => {
                if let Some(filename) = attr_value(&event, b"filename") {
                    push_data_ref(&mut parsed, panel.as_ref(), &event, filename);
                }
            }
            Ok(Event::End(event)) if event.name().as_ref() == b"data" => {
                in_data = false;
            }
            Ok(Event::Start(event)) if event.name().as_ref() == b"cell" => {
                current_cell = Some(CurrentCell {
                    ref_id: attr_value(&event, b"ref").unwrap_or_default(),
                    action_verb: attr_value(&event, b"action_verb").unwrap_or_default(),
                    text: String::new(),
                });
            }
            Ok(Event::Empty(event)) if event.name().as_ref() == b"cell" => {
                let cell = CurrentCell {
                    ref_id: attr_value(&event, b"ref").unwrap_or_default(),
                    action_verb: attr_value(&event, b"action_verb").unwrap_or_default(),
                    text: String::new(),
                };
                push_inline_cell(&mut parsed, panel.as_mut(), cell);
            }
            Ok(Event::End(event)) if event.name().as_ref() == b"cell" => {
                if let Some(cell) = current_cell.take()
                    && in_data
                {
                    push_inline_cell(&mut parsed, panel.as_mut(), cell);
                }
            }
            Ok(Event::Text(event)) => {
                let value = String::from_utf8_lossy(event.as_ref()).into_owned();
                if let Some(cell) = &mut current_cell {
                    cell.text.push_str(&value);
                } else if in_title && let Some(panel) = &mut panel {
                    panel.title.push_str(&value);
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(Error::Driver(format!(
                    "Panels.xml XML parse error at byte {}: {error}",
                    reader.buffer_position()
                )));
            }
            _ => {}
        }
    }

    Ok(parsed)
}

pub fn parse_panel_bin(data: &[u8]) -> Result<SsedPanelBin> {
    if data.len() < 8 {
        return Err(Error::Driver(
            "Panel BIN is shorter than the 8-byte header".to_owned(),
        ));
    }
    let declared_record_count = le32(data, 0);
    let text_width = le32(data, 4);
    let variants = [
        ("address_label", false, declared_record_count),
        ("id_address_label", true, declared_record_count),
        (
            "address_label_declared_count_plus_one",
            false,
            declared_record_count.saturating_sub(1),
        ),
        (
            "id_address_label_declared_count_plus_one",
            true,
            declared_record_count.saturating_sub(1),
        ),
    ];
    for (format, has_record_id, actual_count) in variants {
        let stride = (if has_record_id { 12 } else { 8 }) + text_width as usize;
        if data.len() == 8 + actual_count as usize * stride {
            return Ok(parse_panel_records(
                data,
                declared_record_count,
                actual_count,
                text_width,
                format,
                has_record_id,
            ));
        }
    }
    for (format, has_record_id) in [
        ("address_label_declared_count_mismatch", false),
        ("id_address_label_declared_count_mismatch", true),
    ] {
        let stride = (if has_record_id { 12 } else { 8 }) + text_width as usize;
        let payload_len = data.len().saturating_sub(8);
        if stride > 0 && payload_len.is_multiple_of(stride) {
            let actual_count = (payload_len / stride) as u32;
            if actual_count <= declared_record_count {
                return Ok(parse_panel_records(
                    data,
                    declared_record_count,
                    actual_count,
                    text_width,
                    format,
                    has_record_id,
                ));
            }
        }
    }
    Err(Error::Driver(format!(
        "Panel BIN size mismatch: count={declared_record_count} text_width={text_width} actual={}",
        data.len()
    )))
}

fn parse_panel_records(
    data: &[u8],
    declared_record_count: u32,
    actual_record_count: u32,
    text_width: u32,
    format: &str,
    has_record_id: bool,
) -> SsedPanelBin {
    let stride = (if has_record_id { 12 } else { 8 }) + text_width as usize;
    let mut records = Vec::new();
    for index in 0..actual_record_count {
        let pos = 8 + index as usize * stride;
        let (record_id, block_pos) = if has_record_id {
            (Some(le32(data, pos)), pos + 4)
        } else {
            (None, pos)
        };
        let text_start = block_pos + 8;
        let text_end = text_start + text_width as usize;
        records.push(SsedPanelBinRecord {
            index,
            record_id,
            block: le32(data, block_pos),
            offset: le32(data, block_pos + 4),
            text: decode_panel_text(&data[text_start..text_end]),
        });
    }
    SsedPanelBin {
        declared_record_count,
        actual_record_count,
        text_width,
        format: format.to_owned(),
        records,
    }
}

fn push_data_ref(
    parsed: &mut SsedPanelXml,
    panel: Option<&CurrentPanel>,
    event: &BytesStart<'_>,
    filename: String,
) {
    let Some(panel) = panel else {
        return;
    };
    parsed.data_refs.push(SsedPanelDataRef {
        panel_id: panel.panel_id.clone(),
        panel_type: panel.panel_type.clone(),
        title: panel.title.clone(),
        filename,
        data_type: attr_value(event, b"type").unwrap_or_default(),
    });
}

fn push_inline_cell(
    parsed: &mut SsedPanelXml,
    panel: Option<&mut CurrentPanel>,
    cell: CurrentCell,
) {
    let Some(panel) = panel else {
        return;
    };
    let label = cell.text.trim().to_owned();
    if label.is_empty() && cell.ref_id.is_empty() && cell.action_verb.is_empty() {
        panel.cell_index += 1;
        return;
    }
    let (row, column) = if let Some(columns) = panel.columns.filter(|value| *value > 0) {
        (
            Some(panel.cell_index / columns),
            Some(panel.cell_index % columns),
        )
    } else {
        (None, None)
    };
    parsed.inline_cells.push(SsedPanelInlineCell {
        panel_id: panel.panel_id.clone(),
        panel_type: panel.panel_type.clone(),
        title: panel.title.clone(),
        cell_index: panel.cell_index,
        row,
        column,
        label,
        ref_id: cell.ref_id,
        action_verb: cell.action_verb,
    });
    panel.cell_index += 1;
}

fn decode_xml_bytes(data: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(data) {
        return text.to_owned();
    }
    let (decoded, _encoding, _had_errors) = SHIFT_JIS.decode(data);
    decoded.into_owned()
}

fn attr_value(event: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    event.attributes().flatten().find_map(|attr| {
        (attr.key.as_ref() == name)
            .then(|| String::from_utf8_lossy(attr.value.as_ref()).into_owned())
    })
}

fn decode_panel_text(data: &[u8]) -> String {
    let mut out = String::new();
    let data = data.strip_suffix(b"\0").unwrap_or(data);
    let mut i = 0usize;
    let mut halfwidth_depth = 0usize;
    while i < data.len() {
        if data[i] == 0 {
            break;
        }
        if data[i] == 0x1f && i + 1 < data.len() {
            match data[i + 1] {
                0x04 => halfwidth_depth += 1,
                0x05 => halfwidth_depth = halfwidth_depth.saturating_sub(1),
                _ => {}
            }
            i += 2;
            continue;
        }
        if i + 1 < data.len()
            && (0x21..=0x7e).contains(&data[i])
            && (0x21..=0x7e).contains(&data[i + 1])
        {
            if let Some(ch) = decode_jis_pair(data[i], data[i + 1]) {
                if halfwidth_depth > 0 {
                    out.push(narrow_fullwidth_ascii_char(ch));
                } else {
                    out.push(ch);
                }
            }
            i += 2;
            continue;
        }
        if i + 1 < data.len() && (0xa1..=0xfe).contains(&data[i]) {
            out.push_str(&format!(
                "<z{}{:02X}>",
                if data[i] < 0xb0 { "A" } else { "B" },
                data[i + 1]
            ));
            i += 2;
            continue;
        }
        i += 1;
    }
    out
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
        '\u{ff01}'..='\u{ff5e}' => char::from_u32(ch as u32 - 0xfee0).unwrap_or(ch),
        '\u{3000}' => ' ',
        _ => ch,
    }
}

fn le32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
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
    fn parses_panel_xml_data_refs_and_cells() {
        let xml = r#"
<panels>
  <panel index="01000000" paneltype="menu" count_x="2">
    <title>五十音</title>
    <data><cell action_verb="lved.panel:01010000" ref="01010000">あ</cell></data>
  </panel>
  <panel index="01010000" paneltype="contents">
    <title>あ</title>
    <data type="bin" filename="Panel\All-A.bin" />
  </panel>
</panels>"#;
        let parsed = parse_panel_xml(xml).unwrap();

        assert_eq!(parsed.inline_cells[0].label, "あ");
        assert_eq!(parsed.inline_cells[0].row, Some(0));
        assert_eq!(parsed.data_refs[0].filename, r"Panel\All-A.bin");
    }

    #[test]
    fn parses_panel_bin_address_records() {
        let data = (1u32)
            .to_le_bytes()
            .into_iter()
            .chain((4u32).to_le_bytes())
            .chain((3u32).to_le_bytes())
            .chain((0x20u32).to_le_bytes())
            .chain([0x24, 0x22, 0, 0])
            .collect::<Vec<_>>();
        let parsed = parse_panel_bin(&data).unwrap();

        assert_eq!(parsed.records[0].block, 3);
        assert_eq!(parsed.records[0].offset, 0x20);
        assert_eq!(parsed.records[0].text, "あ");
    }
}

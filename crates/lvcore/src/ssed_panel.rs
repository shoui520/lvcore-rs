use encoding_rs::SHIFT_JIS;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::plist_xml::{PlistValue, parse_xml_plist};

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
    pub target_block: Option<u32>,
    pub target_offset: Option<u32>,
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

pub fn parse_panel_plist_bytes(data: &[u8], source_label: &str) -> Result<SsedPanelXml> {
    parse_panel_plist_panel_bytes(data, source_label, None)
}

pub fn parse_panel_plist_panel_bytes(
    data: &[u8],
    source_label: &str,
    requested_panel_id: Option<&str>,
) -> Result<SsedPanelXml> {
    let plist = parse_xml_plist(data, source_label)?;
    parse_panel_plist_value_for_panel(&plist, requested_panel_id)
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
        let Some((_, expected_len)) = panel_bin_layout_len(actual_count, text_width, has_record_id)
        else {
            continue;
        };
        if data.len() == expected_len {
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
        let Some((stride, _)) = panel_bin_layout_len(0, text_width, has_record_id) else {
            continue;
        };
        let payload_len = data.len().saturating_sub(8);
        if stride > 0 && payload_len.is_multiple_of(stride) {
            let actual_count = (payload_len / stride) as u32;
            if actual_count <= declared_record_count
                && (actual_count > 0 || declared_record_count == 0)
            {
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
    if let Some(panel) = parse_big_endian_utf8_panel_records(data) {
        return Ok(panel);
    }
    Err(Error::Driver(format!(
        "Panel BIN size mismatch: count={declared_record_count} text_width={text_width} actual={}",
        data.len()
    )))
}

pub(crate) fn exinfo_general_value(data: &[u8], wanted_key: &str) -> Option<String> {
    let (text, _encoding, _had_errors) = SHIFT_JIS.decode(data);
    let mut in_general = false;
    for raw_line in text.lines() {
        let line = raw_line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_general = line[1..line.len() - 1]
                .trim()
                .eq_ignore_ascii_case("GENERAL");
            continue;
        }
        if !in_general {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case(wanted_key) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

pub(crate) fn exinfo_panel_metadata_name(data: &[u8]) -> Option<String> {
    if let Some(value) = exinfo_general_value(data, "PANELXML") {
        return Some(value);
    }
    exinfo_general_value(data, "ROSQLNAME")
        .filter(|value| panel_metadata_name_from_exinfo_value(value))
}

fn panel_metadata_name_from_exinfo_value(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.ends_with(".xml") || value.ends_with(".plist")
}

fn panel_bin_layout_len(
    record_count: u32,
    text_width: u32,
    has_record_id: bool,
) -> Option<(usize, usize)> {
    let fixed = if has_record_id { 12usize } else { 8usize };
    let stride = fixed.checked_add(text_width as usize)?;
    let payload_len = (record_count as usize).checked_mul(stride)?;
    Some((stride, 8usize.checked_add(payload_len)?))
}

fn parse_big_endian_utf8_panel_records(data: &[u8]) -> Option<SsedPanelBin> {
    if data.len() < 12 {
        return None;
    }
    let max_stride = data.len().min(512);
    let stride = (12..=max_stride)
        .filter(|stride| data.len().is_multiple_of(*stride))
        .find(|stride| looks_like_big_endian_utf8_panel_records(data, *stride))?;
    let record_count = data.len() / stride;
    let text_width = u32::try_from(stride.saturating_sub(8)).ok()?;
    let actual_record_count = u32::try_from(record_count).ok()?;
    let mut records = Vec::with_capacity(record_count);
    for index in 0..record_count {
        let pos = index * stride;
        let text = std::str::from_utf8(trim_trailing_nuls(&data[pos + 8..pos + stride]))
            .ok()?
            .to_owned();
        records.push(SsedPanelBinRecord {
            index: u32::try_from(index).ok()?,
            record_id: None,
            block: be32(data, pos),
            offset: be32(data, pos + 4),
            text,
        });
    }
    Some(SsedPanelBin {
        declared_record_count: actual_record_count,
        actual_record_count,
        text_width,
        format: "big_endian_address_utf8_label_no_header".to_owned(),
        records,
    })
}

fn looks_like_big_endian_utf8_panel_records(data: &[u8], stride: usize) -> bool {
    if stride <= 8 || !data.len().is_multiple_of(stride) {
        return false;
    }
    let record_count = data.len() / stride;
    if record_count == 0 {
        return false;
    }
    let sample_limit = record_count.min(256);
    let mut non_empty = 0usize;
    for index in 0..sample_limit {
        let pos = index * stride;
        let block = be32(data, pos);
        let offset = be32(data, pos + 4);
        if block > 0x100000 || offset > 0x100000 {
            return false;
        }
        let text_bytes = trim_trailing_nuls(&data[pos + 8..pos + stride]);
        if text_bytes.is_empty() {
            continue;
        }
        let Ok(text) = std::str::from_utf8(text_bytes) else {
            return false;
        };
        if text.contains('\0') {
            return false;
        }
        non_empty = non_empty.saturating_add(1);
    }
    non_empty >= usize::max(1, sample_limit / 2)
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
        target_block: None,
        target_offset: None,
    });
    panel.cell_index += 1;
}

pub(crate) fn parse_panel_plist_value_for_panel(
    value: &PlistValue,
    requested_panel_id: Option<&str>,
) -> Result<SsedPanelXml> {
    let mut parsed = SsedPanelXml {
        data_refs: Vec::new(),
        inline_cells: Vec::new(),
    };
    if let Some(dict) = value.as_dict()
        && let Some(panels) = dict.get("panel").and_then(PlistValue::as_dict)
    {
        for (panel_id, panel_value) in panels {
            if requested_panel_id.is_some_and(|requested| requested != panel_id) {
                continue;
            }
            let Some(panel) = panel_value.as_dict() else {
                continue;
            };
            parse_mac_panel_plist_panel(&mut parsed, panel_id, panel);
        }
        return Ok(parsed);
    }
    if let Some(items) = plist_root_menu_items(value) {
        parse_mobile_menu_plist_panel(&mut parsed, &items, requested_panel_id.unwrap_or("root"));
        return Ok(parsed);
    }
    Ok(parsed)
}

fn parse_mac_panel_plist_panel(
    parsed: &mut SsedPanelXml,
    panel_id: &str,
    panel: &std::collections::BTreeMap<String, PlistValue>,
) {
    let panel_type = plist_string(panel, &["paneltype", "type"]);
    let title = plist_string(panel, &["title", "item", "text"]);
    let columns = plist_i64(panel, "count_x")
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0);
    let data_items = plist_data_items(panel);
    let mut cell_index = 0u32;
    for data in data_items {
        if let Some(filename) = plist_string_opt(data, &["filename", "path"]) {
            parsed.data_refs.push(SsedPanelDataRef {
                panel_id: panel_id.to_owned(),
                panel_type: panel_type.clone(),
                title: title.clone(),
                filename,
                data_type: plist_string_opt(data, &["type"]).unwrap_or_else(|| "bin".to_owned()),
            });
            continue;
        }
        if let Some(cells) = data.get("cell").and_then(PlistValue::as_array) {
            for cell in cells.iter().filter_map(PlistValue::as_dict) {
                push_plist_inline_cell(
                    parsed,
                    PlistInlineCellContext {
                        panel_id,
                        panel_type: &panel_type,
                        title: &title,
                        columns,
                        cell_index,
                    },
                    cell,
                );
                cell_index = cell_index.saturating_add(1);
            }
            continue;
        }
        push_plist_inline_cell(
            parsed,
            PlistInlineCellContext {
                panel_id,
                panel_type: &panel_type,
                title: &title,
                columns,
                cell_index,
            },
            data,
        );
        cell_index = cell_index.saturating_add(1);
    }
}

fn parse_mobile_menu_plist_panel(
    parsed: &mut SsedPanelXml,
    root_items: &[&PlistValue],
    panel_id: &str,
) {
    if panel_id == "root" {
        parse_mobile_menu_plist_items(parsed, "root", "menu", "Top", root_items);
        return;
    }
    let Some(item) = mobile_menu_item_for_panel_id(root_items, panel_id) else {
        return;
    };
    let title = plist_string(item, &["item", "text", "title", "label"]);
    if let Some(path) = plist_string_opt(item, &["path"]) {
        parsed.data_refs.push(SsedPanelDataRef {
            panel_id: panel_id.to_owned(),
            panel_type: "contents".to_owned(),
            title,
            filename: mobile_panel_bin_filename(&path),
            data_type: "bin".to_owned(),
        });
        return;
    }
    if let Some(children) = item.get("child").and_then(PlistValue::as_array) {
        let child_items = children.iter().collect::<Vec<_>>();
        parse_mobile_menu_plist_items(parsed, panel_id, "menu", &title, &child_items);
    }
}

fn parse_mobile_menu_plist_items(
    parsed: &mut SsedPanelXml,
    panel_id: &str,
    panel_type: &str,
    title: &str,
    items: &[&PlistValue],
) {
    let mut cell_index = 0u32;
    for (index, item) in items.iter().enumerate() {
        let Some(dict) = item.as_dict() else {
            continue;
        };
        let label = plist_string(dict, &["item", "text", "title", "label"]);
        if label.trim().is_empty()
            && dict.get("child").and_then(PlistValue::as_array).is_none()
            && plist_string_opt(dict, &["path"]).is_none()
            && !has_non_zero_address(dict)
        {
            continue;
        }
        let child_panel_id = format!("{panel_id}.{index:04}");
        let has_children = dict
            .get("child")
            .and_then(PlistValue::as_array)
            .is_some_and(|children| !children.is_empty());
        let path = plist_string_opt(dict, &["path"]);
        let ref_id = if has_children || path.is_some() {
            child_panel_id.clone()
        } else {
            String::new()
        };
        let (row, column) = (Some(cell_index), Some(0));
        parsed.inline_cells.push(SsedPanelInlineCell {
            panel_id: panel_id.to_owned(),
            panel_type: panel_type.to_owned(),
            title: title.to_owned(),
            cell_index,
            row,
            column,
            label,
            ref_id,
            action_verb: String::new(),
            target_block: plist_u32(dict, "block").filter(|value| *value > 0),
            target_offset: plist_u32(dict, "offset"),
        });
        cell_index = cell_index.saturating_add(1);
        if let Some(path) = path {
            parsed.data_refs.push(SsedPanelDataRef {
                panel_id: child_panel_id.clone(),
                panel_type: "contents".to_owned(),
                title: plist_string(dict, &["item", "text", "title", "label"]),
                filename: mobile_panel_bin_filename(&path),
                data_type: "bin".to_owned(),
            });
        }
    }
}

fn mobile_menu_item_for_panel_id<'a>(
    root_items: &[&'a PlistValue],
    panel_id: &str,
) -> Option<&'a std::collections::BTreeMap<String, PlistValue>> {
    let mut parts = panel_id.split('.');
    if parts.next()? != "root" {
        return None;
    }
    let index = mobile_panel_part_index(parts.next()?)?;
    let mut item = root_items.get(index)?.as_dict()?;
    for part in parts {
        let index = mobile_panel_part_index(part)?;
        let children = item.get("child")?.as_array()?;
        item = children.get(index)?.as_dict()?;
    }
    Some(item)
}

fn mobile_panel_part_index(value: &str) -> Option<usize> {
    value.parse::<usize>().ok()
}

struct PlistInlineCellContext<'a> {
    panel_id: &'a str,
    panel_type: &'a str,
    title: &'a str,
    columns: Option<u32>,
    cell_index: u32,
}

fn push_plist_inline_cell(
    parsed: &mut SsedPanelXml,
    context: PlistInlineCellContext<'_>,
    cell: &std::collections::BTreeMap<String, PlistValue>,
) {
    let label = plist_string(cell, &["text", "title", "item", "label"]);
    let ref_id = plist_string(cell, &["ref"]);
    let action_verb = plist_string(cell, &["action_verb", "action"]);
    if label.trim().is_empty() && ref_id.is_empty() && action_verb.is_empty() {
        return;
    }
    let (row, column) = if let Some(columns) = context.columns {
        (
            Some(context.cell_index / columns),
            Some(context.cell_index % columns),
        )
    } else {
        (None, None)
    };
    parsed.inline_cells.push(SsedPanelInlineCell {
        panel_id: context.panel_id.to_owned(),
        panel_type: context.panel_type.to_owned(),
        title: context.title.to_owned(),
        cell_index: context.cell_index,
        row,
        column,
        label,
        ref_id,
        action_verb,
        target_block: plist_u32(cell, "block").filter(|value| *value > 0),
        target_offset: plist_u32(cell, "offset"),
    });
}

fn plist_root_menu_items(value: &PlistValue) -> Option<Vec<&PlistValue>> {
    if let Some(items) = value.as_array() {
        return Some(items.iter().collect());
    }
    let dict = value.as_dict()?;
    if dict.values().all(|value| value.as_dict().is_some()) {
        let mut items = dict
            .iter()
            .filter_map(|(key, value)| {
                value
                    .as_dict()
                    .map(|_| (plist_numeric_sort_key(key), value))
            })
            .collect::<Vec<_>>();
        items.sort_by(|left, right| left.0.cmp(&right.0));
        return Some(items.into_iter().map(|(_, value)| value).collect());
    }
    None
}

fn plist_data_items(
    panel: &std::collections::BTreeMap<String, PlistValue>,
) -> Vec<&std::collections::BTreeMap<String, PlistValue>> {
    let Some(data) = panel.get("data") else {
        return Vec::new();
    };
    if let Some(dict) = data.as_dict() {
        return vec![dict];
    }
    data.as_array()
        .map(|items| items.iter().filter_map(PlistValue::as_dict).collect())
        .unwrap_or_default()
}

fn plist_string(dict: &std::collections::BTreeMap<String, PlistValue>, keys: &[&str]) -> String {
    plist_string_opt(dict, keys).unwrap_or_default()
}

fn plist_string_opt(
    dict: &std::collections::BTreeMap<String, PlistValue>,
    keys: &[&str],
) -> Option<String> {
    keys.iter().find_map(|key| {
        dict.get(*key)
            .and_then(PlistValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn plist_i64(dict: &std::collections::BTreeMap<String, PlistValue>, key: &str) -> Option<i64> {
    dict.get(key).and_then(PlistValue::as_i64)
}

fn plist_u32(dict: &std::collections::BTreeMap<String, PlistValue>, key: &str) -> Option<u32> {
    plist_i64(dict, key).and_then(|value| u32::try_from(value).ok())
}

fn has_non_zero_address(dict: &std::collections::BTreeMap<String, PlistValue>) -> bool {
    plist_u32(dict, "block").is_some_and(|value| value > 0)
        || plist_u32(dict, "offset").is_some_and(|value| value > 0)
}

fn mobile_panel_bin_filename(path: &str) -> String {
    let normalized = path.trim().trim_start_matches('/').replace('\\', "/");
    let with_ext = if normalized.to_ascii_lowercase().ends_with(".bin") {
        normalized
    } else {
        format!("{normalized}.bin")
    };
    format!("bin/{with_ext}")
}

fn plist_numeric_sort_key(key: &str) -> (u8, u32, &str) {
    key.parse::<u32>()
        .map(|value| (0, value, key))
        .unwrap_or((1, u32::MAX, key))
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
            if let Some(identity) = panel_compressed_gaiji_identity(data[i], data[i + 1]) {
                out.push_str("<z");
                out.push_str(&identity);
                out.push('>');
            } else {
                out.push_str(&format!(
                    "<z{}{:02X}>",
                    if data[i] < 0xb0 { "A" } else { "B" },
                    data[i + 1]
                ));
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    out
}

fn panel_compressed_gaiji_identity(first: u8, second: u8) -> Option<String> {
    let (plane, base) = if (0xa1..=0xaf).contains(&first) {
        ('A', 0xa1)
    } else if (0xb1..=0xbf).contains(&first) {
        ('B', 0xb1)
    } else {
        return None;
    };
    let variant = first.checked_sub(base)?;
    (variant <= 0x0f).then(|| format!("{plane}{second:02X}{variant:X}"))
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

fn be32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn trim_trailing_nuls(data: &[u8]) -> &[u8] {
    let end = data
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(0, |index| index + 1);
    &data[..end]
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

    #[test]
    fn parses_headerless_big_endian_utf8_panel_records() {
        const WIDTH: usize = 12;
        fn row(block: u32, offset: u32, label: &str) -> Vec<u8> {
            let mut bytes = block
                .to_be_bytes()
                .into_iter()
                .chain(offset.to_be_bytes())
                .collect::<Vec<_>>();
            let mut label_bytes = label.as_bytes().to_vec();
            label_bytes.resize(WIDTH, 0);
            bytes.extend(label_bytes);
            bytes
        }
        let data = row(2, 0x92, "亜")
            .into_iter()
            .chain(row(3, 0x180, "ア"))
            .collect::<Vec<_>>();

        let parsed = parse_panel_bin(&data).unwrap();

        assert_eq!(parsed.format, "big_endian_address_utf8_label_no_header");
        assert_eq!(parsed.text_width, WIDTH as u32);
        assert_eq!(parsed.records[0].block, 2);
        assert_eq!(parsed.records[0].offset, 0x92);
        assert_eq!(parsed.records[0].text, "亜");
        assert_eq!(parsed.records[1].text, "ア");
    }

    #[test]
    fn truncated_panel_bin_records_do_not_panic() {
        let data = (2u32)
            .to_le_bytes()
            .into_iter()
            .chain((4u32).to_le_bytes())
            .chain((3u32).to_le_bytes())
            .chain((0x20u32).to_le_bytes())
            .chain([0x24, 0x22, 0, 0])
            .chain((4u32).to_le_bytes())
            .chain((0x30u32).to_le_bytes())
            .chain([0x24, 0x24, 0, 0])
            .collect::<Vec<_>>();

        for len in 0..=data.len() {
            let _ = parse_panel_bin(&data[..len]);
        }
    }

    #[test]
    fn panel_bin_rejects_overflowing_layout_header() {
        let data = u32::MAX
            .to_le_bytes()
            .into_iter()
            .chain(u32::MAX.to_le_bytes())
            .collect::<Vec<_>>();

        let error = parse_panel_bin(&data).unwrap_err();
        assert!(error.to_string().contains("Panel BIN size mismatch"));
    }
}

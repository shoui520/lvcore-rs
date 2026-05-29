use encoding_rs::SHIFT_JIS;

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsedAuxIndexSpec {
    pub index: usize,
    pub name: String,
    pub info: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsedAuxIndexRow {
    pub line_number: usize,
    pub block: u32,
    pub offset: u32,
    pub depth: u32,
    pub label: String,
}

impl SsedAuxIndexRow {
    pub fn has_target(&self) -> bool {
        self.block != 0 || self.offset != 0
    }

    pub fn virtual_selector(&self) -> Option<String> {
        if self.offset != 0xffff || self.block & 0x0fff_ffff != 0 {
            return None;
        }
        let selector = self.block >> 28;
        (selector != 0).then(|| format!("{selector:x}"))
    }
}

pub fn parse_aux_index_specs_from_exinfo(data: &[u8]) -> Vec<SsedAuxIndexSpec> {
    let general = parse_exinfo_general(data);
    let count = general
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("IDXCOUNT"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut specs = Vec::new();
    for index in 0..count {
        let mut name =
            exinfo_general_value(&general, &format!("IDXNAME{index}")).unwrap_or_default();
        if name.is_empty() && index == 0 {
            name = exinfo_general_value(&general, "IDXTITLE").unwrap_or_default();
        }
        let info = exinfo_general_value(&general, &format!("IDXINFO{index}")).unwrap_or_default();
        if !info.is_empty() {
            specs.push(SsedAuxIndexSpec { index, name, info });
        }
    }
    if specs.is_empty()
        && let Some(info) = exinfo_general_value(&general, "IDXINFO")
    {
        specs.push(SsedAuxIndexSpec {
            index: 0,
            name: exinfo_general_value(&general, "IDXTITLE").unwrap_or_default(),
            info,
        });
    }
    specs
}

pub fn is_numeric_aux_index_filename(name: &str) -> bool {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    if !extension.eq_ignore_ascii_case("idx") {
        return false;
    }
    let base = stem.split_once('_').map(|(base, _)| base).unwrap_or(stem);
    base.len() == 8 && base.chars().all(|ch| ch.is_ascii_hexdigit())
}

pub fn parse_aux_index_text_bytes(data: &[u8]) -> Result<Vec<SsedAuxIndexRow>> {
    let (text, _, _) = SHIFT_JIS.decode(data);
    let mut rows = Vec::new();
    for (line_index, raw_line) in text.lines().enumerate() {
        if raw_line.trim().is_empty() {
            continue;
        }
        let fields = raw_line.split('\t').collect::<Vec<_>>();
        if fields.len() < 3 {
            continue;
        }
        let Some((label, depth)) = aux_label_and_depth(&fields) else {
            continue;
        };
        let block = hex_field(fields[0]).map_err(|error| {
            Error::Driver(format!(
                "invalid auxiliary index block on line {}: {error}",
                line_index + 1
            ))
        })?;
        let offset = hex_field(fields[1]).map_err(|error| {
            Error::Driver(format!(
                "invalid auxiliary index offset on line {}: {error}",
                line_index + 1
            ))
        })?;
        rows.push(SsedAuxIndexRow {
            line_number: line_index + 1,
            block,
            offset,
            depth,
            label,
        });
    }
    Ok(rows)
}

fn parse_exinfo_general(data: &[u8]) -> Vec<(String, String)> {
    let (text, _, _) = SHIFT_JIS.decode(data);
    let mut rows = Vec::new();
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
        rows.push((key.trim().to_owned(), value.trim().to_owned()));
    }
    rows
}

fn exinfo_general_value(general: &[(String, String)], key: &str) -> Option<String> {
    general
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.clone())
}

fn aux_label_and_depth(fields: &[&str]) -> Option<(String, u32)> {
    fields
        .iter()
        .skip(2)
        .enumerate()
        .find_map(|(depth, field)| {
            let label = field.trim();
            (!label.is_empty()).then(|| (label.to_owned(), depth as u32 + 1))
        })
}

fn hex_field(value: &str) -> std::result::Result<u32, std::num::ParseIntError> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(0);
    }
    u32::from_str_radix(value, 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exinfo_aux_index_specs() {
        let (data, _, _) = SHIFT_JIS.encode(
            "[GENERAL]\nIDXCOUNT=2\nIDXTITLE=索引\nIDXINFO0=00000152.idx\nIDXNAME1=オプション\nIDXINFO1=select.html\n",
        );

        let specs = parse_aux_index_specs_from_exinfo(&data);

        assert_eq!(specs[0].name, "索引");
        assert_eq!(specs[0].info, "00000152.idx");
        assert_eq!(specs[1].name, "オプション");
    }

    #[test]
    fn recognizes_numeric_aux_index_filenames() {
        assert!(is_numeric_aux_index_filename("0000015f.idx"));
        assert!(is_numeric_aux_index_filename("0000015F_1.IDX"));
        assert!(!is_numeric_aux_index_filename("DICT.IDX"));
        assert!(!is_numeric_aux_index_filename("0000015.idx"));
    }

    #[test]
    fn parses_aux_index_text_rows_and_virtual_selectors() {
        let (data, _, _) = SHIFT_JIS.encode(
            "00000000\t00000000\t大辞林 第四版\n\
             00005221\t00000722\t\t季語\n\
             10000000\t0000FFFF\t\t西和ABC順\n",
        );

        let rows = parse_aux_index_text_bytes(&data).unwrap();

        assert_eq!(rows[0].label, "大辞林 第四版");
        assert_eq!(rows[0].depth, 1);
        assert_eq!(rows[1].block, 0x5221);
        assert_eq!(rows[1].offset, 0x0722);
        assert_eq!(rows[2].virtual_selector().as_deref(), Some("1"));
    }
}

use serde::{Deserialize, Serialize};

use crate::ssed::BLOCK_SIZE;
use crate::ssed_index::decode_jis_pair;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorSampleRecord {
    pub record_index: u32,
    pub sample_number: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_key: Option<String>,
    pub munsell_raw_hex: String,
    pub munsell: String,
    pub label_raw_hex: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorSampleTable {
    pub records: Vec<ColorSampleRecord>,
}

impl ColorSampleTable {
    pub fn by_sample_key(&self, sample_key: &str) -> Option<&ColorSampleRecord> {
        self.records
            .iter()
            .find(|record| record.sample_key.as_deref() == Some(sample_key))
    }
}

pub fn parse_color_sample_table(data: &[u8]) -> ColorSampleTable {
    let mut records = Vec::new();
    for (record_index, record) in data.chunks(BLOCK_SIZE as usize).enumerate() {
        if !record.iter().any(|byte| *byte != 0) {
            continue;
        }
        let munsell_raw = trim_null(record.get(..20).unwrap_or(record));
        let label_raw = trim_null(record.get(20..148).unwrap_or(&[]));
        let sample_number = record_index as u32 + 1;
        records.push(ColorSampleRecord {
            record_index: record_index as u32,
            sample_number,
            sample_key: sample_key_for_number(sample_number),
            munsell_raw_hex: hex_string(munsell_raw),
            munsell: decode_munsell_notation(munsell_raw),
            label_raw_hex: hex_string(label_raw),
            label: decode_color_sample_label(label_raw),
        });
    }
    ColorSampleTable { records }
}

fn trim_null(data: &[u8]) -> &[u8] {
    data.iter()
        .position(|byte| *byte == 0)
        .map_or(data, |index| &data[..index])
}

fn decode_munsell_notation(raw: &[u8]) -> String {
    let mut value = String::new();
    for byte in raw {
        match munsell_byte(*byte) {
            Some(ch) => value.push_str(ch),
            None => value.push_str(&format!("<{byte:02x}>")),
        }
    }
    value
}

fn munsell_byte(byte: u8) -> Option<&'static str> {
    match byte {
        0xf0 => Some("0"),
        0xf1 => Some("1"),
        0xf2 => Some("2"),
        0xf3 => Some("3"),
        0xf4 => Some("4"),
        0xf5 => Some("5"),
        0xf6 => Some("6"),
        0xf7 => Some("7"),
        0xf8 => Some("8"),
        0xf9 => Some("9"),
        0x4b => Some("."),
        0x61 => Some("/"),
        0xc2 => Some("B"),
        0xc7 => Some("G"),
        0xd5 => Some("N"),
        0xd7 => Some("P"),
        0xd9 => Some("R"),
        0xe8 => Some("Y"),
        _ => None,
    }
}

fn decode_color_sample_label(raw: &[u8]) -> String {
    let mut label = String::new();
    for pair in raw.chunks_exact(2) {
        let first = pair[0];
        let second = pair[1];
        if (0x21..=0x7e).contains(&first) && (0x21..=0x7e).contains(&second) {
            if let Some(ch) = decode_jis_pair(first, second) {
                label.push(ch);
            } else {
                label.push_str(&format!("<{first:02x}{second:02x}>"));
            }
        } else {
            label.push_str(&format!("<{first:02x}{second:02x}>"));
        }
    }
    label
}

fn sample_key_for_number(sample_number: u32) -> Option<String> {
    if !(1..=99).contains(&sample_number) {
        return None;
    }
    let tens = sample_number / 10;
    let ones = sample_number % 10;
    Some(format!("1e{tens:x}{ones:x}"))
}

fn hex_string(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_color_sample_munsell_and_jis_label() {
        let mut record = vec![0_u8; BLOCK_SIZE as usize];
        record[..7].copy_from_slice(&[0xf5, 0xd9, 0xf4, 0x61, 0xf1, 0xf2, 0x00]);
        record[20..24].copy_from_slice(&[0x24, 0x22, 0x24, 0x24]);

        let table = parse_color_sample_table(&record);

        assert_eq!(table.records.len(), 1);
        assert_eq!(table.records[0].sample_key.as_deref(), Some("1e01"));
        assert_eq!(table.records[0].munsell, "5R4/12");
        assert_eq!(table.records[0].label, "あい");
        assert!(table.by_sample_key("1e01").is_some());
    }
}

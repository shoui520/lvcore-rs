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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_rgb: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_rgb_hex: Option<String>,
    #[serde(default)]
    pub rgb_status: ColorSampleRgbStatus,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSampleRgbStatus {
    EstimatedFromMunsell,
    #[default]
    Unresolved,
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
        let munsell = decode_munsell_notation(munsell_raw);
        let estimated_rgb = munsell_to_estimated_rgb(&munsell);
        records.push(ColorSampleRecord {
            record_index: record_index as u32,
            sample_number,
            sample_key: sample_key_for_number(sample_number),
            munsell_raw_hex: hex_string(munsell_raw),
            munsell,
            label_raw_hex: hex_string(label_raw),
            label: decode_color_sample_label(label_raw),
            estimated_rgb,
            estimated_rgb_hex: estimated_rgb.map(rgb_hex_string),
            rgb_status: if estimated_rgb.is_some() {
                ColorSampleRgbStatus::EstimatedFromMunsell
            } else {
                ColorSampleRgbStatus::Unresolved
            },
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct MunsellSpec {
    hue_number: Option<f64>,
    hue_letters: Option<&'static str>,
    value: f64,
    chroma: Option<f64>,
}

fn parse_munsell_spec(notation: &str) -> Option<MunsellSpec> {
    if let Some(rest) = notation.strip_prefix('N') {
        let (value_text, chroma_text) = split_optional_chroma(rest)?;
        let chroma = parse_optional_chroma(chroma_text)?;
        return Some(MunsellSpec {
            hue_number: None,
            hue_letters: None,
            value: parse_hue_or_value_number_exact(value_text)?,
            chroma,
        });
    }

    let (hue_number, rest) = parse_hue_or_value_number_prefix(notation)?;
    let (hue_letters, rest) = parse_hue_letters_prefix(rest)?;
    let (value_text, chroma_text) = split_optional_chroma(rest)?;
    let chroma = parse_optional_chroma(chroma_text)?;
    Some(MunsellSpec {
        hue_number: Some(hue_number),
        hue_letters: Some(hue_letters),
        value: parse_hue_or_value_number_exact(value_text)?,
        chroma,
    })
}

fn split_optional_chroma(value: &str) -> Option<(&str, Option<&str>)> {
    let mut parts = value.split('/');
    let value = parts.next()?;
    let chroma = parts.next();
    if parts.next().is_some() {
        return None;
    }
    Some((value, chroma))
}

fn parse_optional_chroma(value: Option<&str>) -> Option<Option<f64>> {
    match value {
        Some(value) => Some(Some(parse_chroma_number_exact(value)?)),
        None => Some(None),
    }
}

fn parse_hue_or_value_number_prefix(value: &str) -> Option<(f64, &str)> {
    if let Some(rest) = value.strip_prefix("10") {
        return Some((10.0, rest));
    }
    let bytes = value.as_bytes();
    let first = *bytes.first()?;
    if !first.is_ascii_digit() {
        return None;
    }
    let mut end = 1usize;
    if bytes.get(1) == Some(&b'.') && bytes.get(2).is_some_and(u8::is_ascii_digit) {
        end = 3;
    }
    Some((value[..end].parse().ok()?, &value[end..]))
}

fn parse_hue_or_value_number_exact(value: &str) -> Option<f64> {
    if value == "10" {
        return Some(10.0);
    }
    let bytes = value.as_bytes();
    match bytes {
        [digit] if digit.is_ascii_digit() => value.parse().ok(),
        [digit, b'.', decimal] if digit.is_ascii_digit() && decimal.is_ascii_digit() => {
            value.parse().ok()
        }
        _ => None,
    }
}

fn parse_chroma_number_exact(value: &str) -> Option<f64> {
    let bytes = value.as_bytes();
    match bytes {
        [digit] if digit.is_ascii_digit() => value.parse().ok(),
        [tens, ones] if tens.is_ascii_digit() && ones.is_ascii_digit() => value.parse().ok(),
        [digit, b'.', decimal] if digit.is_ascii_digit() && decimal.is_ascii_digit() => {
            value.parse().ok()
        }
        [tens, ones, b'.', decimal]
            if tens.is_ascii_digit() && ones.is_ascii_digit() && decimal.is_ascii_digit() =>
        {
            value.parse().ok()
        }
        _ => None,
    }
}

fn parse_hue_letters_prefix(value: &str) -> Option<(&'static str, &str)> {
    for hue in ["YR", "GY", "BG", "PB", "RP", "R", "Y", "G", "B", "P"] {
        if let Some(rest) = value.strip_prefix(hue) {
            return Some((hue, rest));
        }
    }
    None
}

fn munsell_to_estimated_rgb(notation: &str) -> Option<[u8; 3]> {
    let spec = parse_munsell_spec(notation)?;
    let lightness = (spec.value / 10.0).clamp(0.0, 1.0);
    let Some(hue_letters) = spec.hue_letters else {
        let channel = rgb_channel(lightness);
        return Some([channel, channel, channel]);
    };
    let chroma = spec.chroma?;
    let base = hue_base_degrees(hue_letters)?;
    let hue_number = spec.hue_number.unwrap_or(5.0);
    let hue = ((base + ((hue_number - 5.0) / 10.0) * 36.0).rem_euclid(360.0)) / 360.0;
    let saturation = ((chroma / 16.0) * 0.82).clamp(0.0, 0.82);
    let (red, green, blue) = hls_to_rgb(hue, lightness, saturation);
    Some([rgb_channel(red), rgb_channel(green), rgb_channel(blue)])
}

fn hue_base_degrees(hue: &str) -> Option<f64> {
    match hue {
        "R" => Some(0.0),
        "YR" => Some(36.0),
        "Y" => Some(72.0),
        "GY" => Some(108.0),
        "G" => Some(144.0),
        "BG" => Some(180.0),
        "B" => Some(216.0),
        "PB" => Some(252.0),
        "P" => Some(288.0),
        "RP" => Some(324.0),
        _ => None,
    }
}

fn hls_to_rgb(hue: f64, lightness: f64, saturation: f64) -> (f64, f64, f64) {
    if saturation == 0.0 {
        return (lightness, lightness, lightness);
    }
    let m2 = if lightness <= 0.5 {
        lightness * (1.0 + saturation)
    } else {
        lightness + saturation - (lightness * saturation)
    };
    let m1 = 2.0 * lightness - m2;
    (
        hue_to_rgb(m1, m2, hue + (1.0 / 3.0)),
        hue_to_rgb(m1, m2, hue),
        hue_to_rgb(m1, m2, hue - (1.0 / 3.0)),
    )
}

fn hue_to_rgb(m1: f64, m2: f64, mut hue: f64) -> f64 {
    if hue < 0.0 {
        hue += 1.0;
    }
    if hue > 1.0 {
        hue -= 1.0;
    }
    if hue * 6.0 < 1.0 {
        return m1 + (m2 - m1) * hue * 6.0;
    }
    if hue * 2.0 < 1.0 {
        return m2;
    }
    if hue * 3.0 < 2.0 {
        return m1 + (m2 - m1) * ((2.0 / 3.0) - hue) * 6.0;
    }
    m1
}

fn rgb_channel(value: f64) -> u8 {
    (value * 255.0).round().clamp(0.0, 255.0) as u8
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

fn rgb_hex_string(rgb: [u8; 3]) -> String {
    format!("{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
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
        assert_eq!(table.records[0].estimated_rgb, Some([165, 39, 39]));
        assert_eq!(
            table.records[0].estimated_rgb_hex.as_deref(),
            Some("a52727")
        );
        assert_eq!(
            table.records[0].rgb_status,
            ColorSampleRgbStatus::EstimatedFromMunsell
        );
        assert!(table.by_sample_key("1e01").is_some());
    }

    #[test]
    fn estimates_neutral_and_decimal_munsell_colors() {
        assert_eq!(munsell_to_estimated_rgb("N5"), Some([128, 128, 128]));
        assert_eq!(munsell_to_estimated_rgb("10YR9/2"), Some([232, 232, 227]));
        assert_eq!(munsell_to_estimated_rgb("2.5PB3/8"), Some([48, 45, 108]));
        assert_eq!(munsell_to_estimated_rgb("5Y8.5/1"), Some([218, 219, 215]));
        assert_eq!(munsell_to_estimated_rgb("<ff>"), None);
    }
}

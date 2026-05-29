use std::fs;
use std::path::{Path, PathBuf};

use encoding_rs::SHIFT_JIS;

use crate::error::{Error, Result};

pub const ENCYCLOPEDIA_HEADER: &str = "#LVEDBRSR encyclopedia#";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsedEncyclopediaSection {
    pub index: usize,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsedEncyclopediaRow {
    pub index: usize,
    pub section: String,
    pub depth: u32,
    pub label: String,
    pub block: u32,
    pub offset: u32,
}

impl SsedEncyclopediaRow {
    pub fn has_target(&self) -> bool {
        self.block != 0 || self.offset != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsedEncyclopediaIndex {
    pub path: Option<PathBuf>,
    pub header: String,
    pub sections: Vec<SsedEncyclopediaSection>,
    pub rows: Vec<SsedEncyclopediaRow>,
}

pub fn parse_encyclopedia_index(path: &Path) -> Result<SsedEncyclopediaIndex> {
    let data = fs::read(path)?;
    let mut parsed = parse_encyclopedia_index_bytes(&data)?;
    parsed.path = Some(path.to_path_buf());
    Ok(parsed)
}

pub fn parse_encyclopedia_index_bytes(data: &[u8]) -> Result<SsedEncyclopediaIndex> {
    let (text, _, _) = SHIFT_JIS.decode(data);
    let mut lines = text.lines();
    let Some(first_line) = lines.next() else {
        return Err(Error::Driver(
            "empty LVEDBRSR encyclopedia index".to_owned(),
        ));
    };
    let header = first_line.trim().to_owned();
    if !header.starts_with(ENCYCLOPEDIA_HEADER) {
        return Err(Error::Driver(format!(
            "not an LVEDBRSR encyclopedia index: {header:?}"
        )));
    }

    let mut sections = Vec::new();
    let mut rows = Vec::new();
    let mut current_section = String::new();
    for (line_index, raw_line) in lines.enumerate() {
        let index = line_index + 2;
        if raw_line.trim().is_empty() {
            continue;
        }
        if let Some(stripped) = raw_line.strip_prefix('#') {
            let title = stripped.trim().to_owned();
            current_section = title.clone();
            sections.push(SsedEncyclopediaSection { index, title });
            continue;
        }
        let fields = raw_line.split('\t').collect::<Vec<_>>();
        if fields.len() < 3 {
            continue;
        }
        let Some((label, depth)) = label_and_depth(&fields) else {
            continue;
        };
        let (Ok(block), Ok(offset)) = (hex_field(fields[0]), hex_field(fields[1])) else {
            continue;
        };
        rows.push(SsedEncyclopediaRow {
            index,
            section: current_section.clone(),
            depth,
            label,
            block,
            offset,
        });
    }
    Ok(SsedEncyclopediaIndex {
        path: None,
        header,
        sections,
        rows,
    })
}

fn label_and_depth(fields: &[&str]) -> Option<(String, u32)> {
    fields
        .iter()
        .skip(2)
        .enumerate()
        .find_map(|(depth, field)| {
            let label = field.trim();
            (!label.is_empty()).then(|| (label.to_owned(), depth as u32))
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
    fn parses_lvedbrsr_encyclopedia_index_rows() {
        let (data, _, _) = SHIFT_JIS.encode(
            "#LVEDBRSR encyclopedia#Ver.1.0 2008.01.07\t\t\n\
             #図・写真\t\t\n\
             00000000\t00000000\t図・写真\t\t\n\
             00000000\t00000000\t\t動物\t\n\
             000059f9\t000006dc\t\t\t哺乳類\n",
        );

        let parsed = parse_encyclopedia_index_bytes(&data).unwrap();

        assert_eq!(parsed.sections[0].title, "図・写真");
        assert_eq!(
            parsed.rows.iter().map(|row| row.depth).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(parsed.rows[2].label, "哺乳類");
        assert_eq!(parsed.rows[2].block, 0x59f9);
        assert_eq!(parsed.rows[2].offset, 0x06dc);
        assert!(parsed.rows[2].has_target());
    }
}

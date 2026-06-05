use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::ssed_index::decode_index_key;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedMultiDescriptor {
    pub record_count: u16,
    pub reserved: Vec<u8>,
    pub records: Vec<SsedMultiRecord>,
    pub descriptor_bytes: usize,
    pub trailing_nonzero_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedMultiRecord {
    pub index: u16,
    pub offset: usize,
    pub component_count: u8,
    pub subtype: u8,
    pub label: String,
    pub label_raw: Vec<u8>,
    pub refs: Vec<SsedMultiComponentRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedMultiComponentRef {
    pub component_type: u8,
    pub subtype: u8,
    pub start_block: u32,
    pub block_count: u32,
    pub flags: [u8; 6],
}

pub fn parse_multi_descriptor(data: &[u8]) -> Result<SsedMultiDescriptor> {
    if data.len() < 0x10 {
        return Err(Error::Driver(
            "MULTI descriptor is shorter than its 16-byte header".to_owned(),
        ));
    }
    let record_count = be16(data, 0);
    let reserved = data[2..0x10].to_vec();
    let mut records = Vec::with_capacity(usize::from(record_count));
    let mut pos = 0x10usize;
    for index in 1..=record_count {
        if pos + 0x20 > data.len() {
            return Err(Error::Driver(format!(
                "MULTI record {index} header is truncated at offset {pos}"
            )));
        }
        let offset = pos;
        let component_count = data[pos];
        let subtype = data[pos + 1];
        let label_raw = data[pos + 2..pos + 0x20].to_vec();
        let label = decode_index_key(&label_raw);
        pos += 0x20;

        let mut refs = Vec::with_capacity(usize::from(component_count));
        for ref_index in 1..=component_count {
            if pos + 0x10 > data.len() {
                return Err(Error::Driver(format!(
                    "MULTI record {index} component reference {ref_index} is truncated at offset {pos}"
                )));
            }
            let mut flags = [0_u8; 6];
            flags.copy_from_slice(&data[pos + 10..pos + 16]);
            refs.push(SsedMultiComponentRef {
                component_type: data[pos],
                subtype: data[pos + 1],
                start_block: be32(data, pos + 2),
                block_count: be32(data, pos + 6),
                flags,
            });
            pos += 0x10;
        }

        records.push(SsedMultiRecord {
            index,
            offset,
            component_count,
            subtype,
            label,
            label_raw,
            refs,
        });
    }
    Ok(SsedMultiDescriptor {
        record_count,
        reserved,
        records,
        descriptor_bytes: pos,
        trailing_nonzero_bytes: data[pos..].iter().filter(|byte| **byte != 0).count(),
    })
}

fn be16(data: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([data[offset], data[offset + 1]])
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
    fn parses_descriptor_records_and_component_refs() {
        let mut data = vec![0u8; 0x10];
        data[0..2].copy_from_slice(&1u16.to_be_bytes());
        data.resize(0x30, 0);
        data[0x10] = 2;
        data[0x11] = 0x40;
        data[0x12..0x16].copy_from_slice(b"TEST");
        data.extend_from_slice(&[
            0x01, 0x00, 0, 0, 0, 10, 0, 0, 0, 1, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        ]);
        data.extend_from_slice(&[0x91, 0x00, 0, 0, 0, 11, 0, 0, 0, 2, 0, 1, 2, 3, 4, 5]);
        data.extend_from_slice(&[1, 0, 2]);

        let parsed = parse_multi_descriptor(&data).unwrap();

        assert_eq!(parsed.record_count, 1);
        assert_eq!(parsed.records[0].component_count, 2);
        assert_eq!(parsed.records[0].subtype, 0x40);
        assert_eq!(parsed.records[0].label, "TEST");
        assert_eq!(parsed.records[0].refs[0].component_type, 0x01);
        assert_eq!(parsed.records[0].refs[1].component_type, 0x91);
        assert_eq!(parsed.records[0].refs[1].block_count, 2);
        assert_eq!(parsed.trailing_nonzero_bytes, 2);
    }
}

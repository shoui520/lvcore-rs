use crate::error::Result;
use crate::ssed::SsedDataFile;
use crate::ssed_index::decode_jis_pair;

pub(super) const SSED_ENTRY_MARKER: [u8; 4] = [0x1f, 0x09, 0x00, 0x01];

pub(super) fn looks_like_raw_anchor_label(value: &str) -> bool {
    let value = value.trim();
    value.len() >= 4 && value.chars().all(|ch| ch.is_ascii_digit())
}

pub(super) fn parse_observed_ssed_dense_anchor_id(data: &[u8]) -> Option<String> {
    for marker_start in [0usize, 2] {
        if data.get(marker_start..marker_start + SSED_ENTRY_MARKER.len())
            != Some(SSED_ENTRY_MARKER.as_slice())
        {
            continue;
        }
        if data.get(marker_start + 4..marker_start + 6) != Some([0x1f, 0x41].as_slice()) {
            continue;
        }

        let styled_digits_start = marker_start + 10;
        let styled_digits_end = marker_start + 26;
        if data.get(marker_start + 8..marker_start + 10) == Some([0x1f, 0x04].as_slice())
            && data.get(styled_digits_end..styled_digits_end + 2) == Some([0x1f, 0x05].as_slice())
            && let Some(anchor) =
                parse_jis_digit_anchor_pairs(data.get(styled_digits_start..styled_digits_end)?)
        {
            return Some(anchor);
        }

        let plain_digits_start = marker_start + 6;
        let plain_digits_end = data
            .get(plain_digits_start..)?
            .windows(2)
            .position(|window| window == [0x1f, 0x61] || window == [0x1f, 0x0a])
            .map(|relative| plain_digits_start + relative)
            .unwrap_or_else(|| data.len().min(plain_digits_start + 32));
        if let Some(anchor) =
            parse_jis_digit_anchor_pairs(data.get(plain_digits_start..plain_digits_end)?)
        {
            return Some(anchor);
        }
    }
    None
}

fn parse_jis_digit_anchor_pairs(data: &[u8]) -> Option<String> {
    if !data.len().is_multiple_of(2) {
        return None;
    }
    let mut digits = String::new();
    for pair in data.chunks_exact(2) {
        match pair {
            [0x21, 0x21] => {}
            [0x23, trail] if (0x30..=0x39).contains(trail) => digits.push(char::from(*trail)),
            _ => return None,
        }
    }
    (!digits.is_empty()).then_some(digits)
}

pub(super) fn find_ssed_dense_anchor_record_end(data: &[u8]) -> Option<usize> {
    data.windows(2)
        .enumerate()
        .skip(1)
        .find_map(|(index, window)| (window == [0x1f, 0x0a]).then_some(index))
        .or_else(|| {
            data.windows(4)
                .enumerate()
                .skip(1)
                .find_map(|(index, window)| (window == [0x1f, 0x09, 0x00, 0x01]).then_some(index))
        })
}

pub(super) fn ssed_reader_generic_entry_marker_len(
    reader: &mut SsedDataFile,
    offset: usize,
) -> Result<Option<usize>> {
    let data = reader.read_range(offset, SSED_ENTRY_MARKER.len() + 2)?;
    if data.starts_with(&[0x1f, 0x02])
        && data
            .get(2..2 + SSED_ENTRY_MARKER.len())
            .is_some_and(|marker| marker == SSED_ENTRY_MARKER)
    {
        return Ok(Some(SSED_ENTRY_MARKER.len() + 2));
    }
    if data.starts_with(&SSED_ENTRY_MARKER) {
        return Ok(Some(SSED_ENTRY_MARKER.len()));
    }
    Ok(None)
}

pub(super) fn ssed_find_next_entry_marker_offset(
    reader: &mut SsedDataFile,
    start_offset: usize,
) -> Result<Option<usize>> {
    const SCAN_CHUNK_BYTES: usize = 64 * 1024;
    let expanded_size = reader.header().expanded_size();
    if start_offset >= expanded_size {
        return Ok(None);
    }
    let mut read_offset = start_offset;
    let mut carry = Vec::new();
    let mut carry_base = start_offset;
    let tail_size = SSED_ENTRY_MARKER.len() + 2 - 1;

    while read_offset < expanded_size {
        let read_size = expanded_size
            .saturating_sub(read_offset)
            .min(SCAN_CHUNK_BYTES);
        let chunk = reader.read_range(read_offset, read_size)?;
        if chunk.is_empty() {
            break;
        }
        let base = if carry.is_empty() {
            read_offset
        } else {
            carry_base
        };
        let mut buffer = Vec::with_capacity(carry.len() + chunk.len());
        buffer.extend_from_slice(&carry);
        buffer.extend_from_slice(&chunk);

        let mut search_from = 0usize;
        while let Some(found) = find_bytes(&buffer[search_from..], &SSED_ENTRY_MARKER) {
            let marker_position = search_from + found;
            let absolute = base + marker_position;
            let start = if marker_position >= 2
                && buffer[marker_position - 2..marker_position] == [0x1f, 0x02]
            {
                absolute.saturating_sub(2)
            } else {
                absolute
            };
            if start >= start_offset {
                return Ok(Some(start));
            }
            search_from = marker_position.saturating_add(1);
        }

        let retained = tail_size.min(buffer.len());
        carry = buffer[buffer.len() - retained..].to_vec();
        carry_base = base + buffer.len() - retained;
        read_offset = read_offset.saturating_add(chunk.len());
    }
    Ok(None)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub(super) fn ssed_control_arg_length(data: &[u8], offset: usize) -> usize {
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

pub(super) fn hc03e9_pdfspread_anchor_text(data: &[u8]) -> String {
    let mut text = String::new();
    let mut offset = 0usize;
    while offset < data.len() {
        let byte = data[offset];
        if byte == 0x1f {
            offset += 2 + ssed_control_arg_length(data, offset);
            continue;
        }
        if offset + 1 < data.len()
            && (0x21..=0x7e).contains(&byte)
            && (0x21..=0x7e).contains(&data[offset + 1])
        {
            if let Some(ch) = decode_jis_pair(byte, data[offset + 1]) {
                text.push(ch);
            }
            offset += 2;
            continue;
        }
        if offset + 1 < data.len() && byte >= 0xa1 {
            offset += 2;
            continue;
        }
        offset += 1;
    }
    text
}

pub(super) fn parse_colscr_pointer(payload: &[u8]) -> Option<(u32, u32)> {
    if payload.len() != 18 {
        return None;
    }
    Some((
        decode_bcd_decimal(&payload[12..16])?,
        decode_bcd_decimal(&payload[16..18])?,
    ))
}

pub(super) fn parse_pcmdata_range_pointer(payload: &[u8]) -> Option<(u32, u32, u32, u32)> {
    if payload.len() < 16 {
        return None;
    }
    Some((
        decode_bcd_decimal(&payload[4..8])?,
        decode_bcd_decimal(&payload[8..10])?,
        decode_bcd_decimal(&payload[10..14])?,
        decode_bcd_decimal(&payload[14..16])?,
    ))
}

pub(super) fn parse_packed_bcd_pointer(payload: &[u8]) -> Option<(u32, u32)> {
    if payload.len() < 6 {
        return None;
    }
    Some((
        decode_bcd_decimal(&payload[..4])?,
        decode_bcd_decimal(&payload[4..6])?,
    ))
}

fn decode_bcd_decimal(data: &[u8]) -> Option<u32> {
    let mut value = 0_u32;
    for byte in data {
        let high = byte >> 4;
        let low = byte & 0x0f;
        if high > 9 || low > 9 {
            return None;
        }
        value = value.checked_mul(100)?;
        value = value.checked_add(u32::from(high) * 10 + u32::from(low))?;
    }
    Some(value)
}

fn be16_at(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

pub(super) fn decode_offset_cursor(cursor: Option<&str>) -> usize {
    cursor
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default()
}

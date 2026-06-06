use encoding_rs::SHIFT_JIS;

pub fn decode_index_key(data: &[u8]) -> String {
    let filtered = data.split(|byte| *byte == 0).next().unwrap_or_default();
    if looks_like_plain_ascii_title(filtered) && !looks_like_jis_x0208_title_bytes(filtered) {
        return String::from_utf8_lossy(filtered).trim().to_owned();
    }
    decode_index_key_payload(data)
}

fn decode_index_key_payload(data: &[u8]) -> String {
    let mut out = String::new();
    let mut index = 0usize;
    while index < data.len() {
        let byte = data[index];
        if byte == 0 {
            break;
        }
        if index + 1 < data.len()
            && (0x21..=0x7e).contains(&byte)
            && (0x21..=0x7e).contains(&data[index + 1])
        {
            if let Some(decoded) = decode_jis_pair(byte, data[index + 1]) {
                out.push(decoded);
            }
            index += 2;
            continue;
        }
        if (0x20..=0x7e).contains(&byte) {
            out.push(byte as char);
        }
        index += 1;
    }
    narrow_fullwidth_ascii(&out)
}

pub fn decode_title_text(data: &[u8]) -> String {
    decode_title_text_with_gaiji_filter(data, |_| true)
}

pub fn decode_title_text_with_gaiji_filter(
    data: &[u8],
    mut keep_gaiji: impl FnMut(&str) -> bool,
) -> String {
    let filtered = title_payload_bytes(data);
    if looks_like_jis_x0208_title_bytes(&filtered) {
        return decode_title_payload_text(data, &mut keep_gaiji);
    }
    if looks_like_plain_ascii_title(&filtered) {
        return String::from_utf8_lossy(&filtered).trim().to_owned();
    }
    decode_title_payload_text(data, &mut keep_gaiji)
}

fn title_payload_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut index = 0usize;
    while index < data.len() {
        if data[index] == 0x1f {
            if data.get(index + 1) == Some(&0x0a) {
                break;
            }
            index = skip_control(data, index, data.len());
            continue;
        }
        if matches!(data[index], 0x00 | b'\n' | b'\r') {
            break;
        }
        out.push(data[index]);
        index += 1;
    }
    out
}

fn decode_title_payload_text(data: &[u8], keep_gaiji: &mut impl FnMut(&str) -> bool) -> String {
    let mut out = String::new();
    let mut index = 0usize;
    let mut halfwidth_depth = 0usize;
    while index < data.len() {
        if data[index] == 0x1f {
            if let Some(op) = data.get(index + 1).copied() {
                match op {
                    0x04 => halfwidth_depth = halfwidth_depth.saturating_add(1),
                    0x05 => halfwidth_depth = halfwidth_depth.saturating_sub(1),
                    0x0a => break,
                    _ => {}
                }
            }
            index = skip_control(data, index, data.len());
            continue;
        }
        if matches!(data[index], 0 | b'\n' | b'\r') {
            break;
        }
        if index + 1 < data.len() && data[index..index + 2] == [0x11, 0x03] {
            index += 2;
            continue;
        }
        if data[index] < 0x20 {
            out.push(' ');
            index += 1;
            continue;
        }
        if index + 1 < data.len()
            && (0x21..=0x7e).contains(&data[index])
            && (0x21..=0x7e).contains(&data[index + 1])
            && let Some(decoded) = decode_jis_pair(data[index], data[index + 1])
        {
            if halfwidth_depth > 0 {
                out.push_str(&narrow_fullwidth_ascii(&decoded.to_string()));
            } else {
                out.push(decoded);
            }
            index += 2;
            continue;
        }
        if index + 1 < data.len()
            && ((0x81..=0x9f).contains(&data[index]) || (0xe0..=0xfc).contains(&data[index]))
        {
            let (decoded, _encoding, had_errors) = SHIFT_JIS.decode(&data[index..index + 2]);
            if !had_errors {
                out.push_str(decoded.as_ref());
                index += 2;
                continue;
            }
        }
        if index + 1 < data.len() && (0xa1..=0xfe).contains(&data[index]) {
            if let Some(identity) = title_gaiji_marker_identity(data[index], data[index + 1])
                && keep_gaiji(&identity)
            {
                out.push_str("<z");
                out.push_str(&identity);
                out.push('>');
            }
            index += 2;
            continue;
        }
        if data[index] <= 0x7e {
            out.push(data[index] as char);
        }
        index += 1;
    }
    out.trim().to_owned()
}

fn title_gaiji_marker_identity(first: u8, second: u8) -> Option<String> {
    // English title streams commonly use A-plane gaiji for accented letters.
    // Preserve those markers for the reader-side rich-label resolver instead
    // of dropping user-visible characters before Unicode/GA16 lookup can run.
    if (0xa1..=0xaf).contains(&first) && (0x21..=0x7e).contains(&second) {
        return Some(format!("{first:02X}{second:02X}"));
    }
    None
}

fn looks_like_plain_ascii_title(data: &[u8]) -> bool {
    if data.is_empty() || !data.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
        return false;
    }
    let alnum_or_space = data
        .iter()
        .filter(|byte| byte.is_ascii_alphanumeric() || byte.is_ascii_whitespace())
        .count();
    let jis_like_punctuation = data
        .iter()
        .filter(|byte| matches!(**byte, b'!' | b'#' | b'$' | b'%'))
        .count();
    alnum_or_space * 2 >= data.len() && jis_like_punctuation * 3 <= data.len()
}

fn looks_like_jis_x0208_title_bytes(data: &[u8]) -> bool {
    if data.len() < 4 || !data.len().is_multiple_of(2) {
        return false;
    }
    if !data.iter().all(|byte| (0x21..=0x7e).contains(byte)) {
        return false;
    }
    if data.iter().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                *byte,
                b' ' | b'-' | b'\'' | b'.' | b',' | b'/' | b'+' | b'&' | b':'
            )
    }) {
        return false;
    }

    let mut japanese = 0usize;
    for pair in data.chunks_exact(2) {
        let Some(decoded) = decode_jis_pair(pair[0], pair[1]) else {
            return false;
        };
        japanese += usize::from(is_japanese_title_char(decoded));
    }
    japanese * 2 >= data.len() / 2
}

fn is_japanese_title_char(value: char) -> bool {
    matches!(
        value,
        '\u{3000}'..='\u{30ff}'
            | '\u{3400}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{ff00}'..='\u{ffef}'
            | '\u{25cb}'..='\u{25ef}'
            | '\u{2010}'..='\u{2015}'
    )
}

pub(crate) fn decode_jis_pair(first: u8, second: u8) -> Option<char> {
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
    if had_errors {
        return None;
    }
    decoded.chars().next()
}

fn narrow_fullwidth_ascii(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(ch as u32 - 0xfee0).unwrap_or(ch),
            '\u{3000}' => ' ',
            _ => ch,
        })
        .collect()
}

fn skip_control(data: &[u8], index: usize, end: usize) -> usize {
    let mut next = index.saturating_add(2).min(end);
    if next < end && data[next] <= 0x10 {
        next += 1;
    }
    if next < end && data[next] <= 0x10 {
        next += 1;
    }
    next
}

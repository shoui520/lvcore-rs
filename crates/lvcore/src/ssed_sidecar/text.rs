pub(super) fn strip_html_tags(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    collapse_whitespace(&decode_text_entities(&out))
}

fn decode_text_entities(value: &str) -> String {
    let mut decoded = value.to_owned();
    for _ in 0..2 {
        let next = decode_text_entities_once(&decoded);
        if next == decoded {
            return decoded;
        }
        decoded = next;
    }
    decoded
}

fn decode_text_entities_once(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut cursor = 0usize;
    while cursor < value.len() {
        let Some(relative_start) = value[cursor..].find('&') else {
            out.push_str(&value[cursor..]);
            break;
        };
        let start = cursor + relative_start;
        out.push_str(&value[cursor..start]);
        let rest = &value[start..];
        if let Some((entity, replacement)) = decode_named_entity(rest) {
            out.push_str(replacement);
            cursor = start + entity.len();
            continue;
        }
        if let Some((entity_len, ch)) = decode_numeric_entity(rest) {
            out.push(ch);
            cursor = start + entity_len;
            continue;
        }
        out.push('&');
        cursor = start + 1;
    }
    out
}

fn decode_named_entity(value: &str) -> Option<(&'static str, &'static str)> {
    [
        ("&nbsp;", " "),
        ("&#160;", " "),
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&apos;", "'"),
    ]
    .into_iter()
    .find(|(entity, _)| value.starts_with(entity))
}

fn decode_numeric_entity(value: &str) -> Option<(usize, char)> {
    let value = value.strip_prefix("&#")?;
    let end = value.find(';')?;
    let digits = &value[..end];
    if digits.is_empty() {
        return None;
    }
    let codepoint = if let Some(hex) = digits
        .strip_prefix('x')
        .or_else(|| digits.strip_prefix('X'))
    {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        digits.parse::<u32>().ok()?
    };
    let ch = char::from_u32(codepoint)?;
    Some((2 + end + 1, ch))
}

fn collapse_whitespace(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut pending_space = false;
    for ch in value.chars() {
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
    }
    out.trim().to_owned()
}

pub(super) fn normalize_sidecar_search_text(value: &str) -> String {
    narrow_fullwidth_ascii_text(&collapse_whitespace(value)).to_lowercase()
}

fn narrow_fullwidth_ascii_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\u{3000}' => ' ',
            ch if ('\u{ff01}'..='\u{ff5e}').contains(&ch) => {
                char::from_u32((ch as u32) - 0xfee0).unwrap_or(ch)
            }
            ch => ch,
        })
        .collect()
}

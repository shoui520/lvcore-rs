pub(super) fn strip_html_tags(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_tag = false;
    let mut entity = String::new();
    let mut in_entity = false;
    for ch in value.chars() {
        if in_entity {
            entity.push(ch);
            if ch == ';' || entity.len() > 16 {
                out.push_str(decode_basic_entity(&entity).unwrap_or(entity.as_str()));
                entity.clear();
                in_entity = false;
            }
            continue;
        }
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => in_tag = false,
            '&' if !in_tag => {
                entity.push('&');
                in_entity = true;
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    if in_entity {
        out.push_str(&entity);
    }
    collapse_whitespace(&out)
}

fn decode_basic_entity(entity: &str) -> Option<&'static str> {
    match entity {
        "&amp;" => Some("&"),
        "&lt;" => Some("<"),
        "&gt;" => Some(">"),
        "&quot;" => Some("\""),
        "&#39;" | "&apos;" => Some("'"),
        _ => None,
    }
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

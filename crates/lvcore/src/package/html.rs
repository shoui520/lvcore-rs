#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PackageHtmlReference {
    pub(super) path: String,
    pub(super) anchor: Option<String>,
}

pub(super) fn html_unescape_minimal(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

pub(super) fn package_html_base_dir(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(base, _)| base.to_owned())
        .unwrap_or_default()
}

pub(super) fn package_relative_html_reference(
    base_dir: &str,
    raw_value: &str,
) -> Option<PackageHtmlReference> {
    let value = html_unescape_minimal(raw_value).trim().replace('\\', "/");
    if value.is_empty()
        || value.starts_with('#')
        || value.starts_with('/')
        || value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("mailto:")
        || value.starts_with("javascript:")
        || value.starts_with("data:")
        || value.starts_with("lvcore://")
    {
        return None;
    }
    let (path_part, anchor) = value.split_once('#').unwrap_or((value.as_str(), ""));
    let path_part = path_part.split('?').next().unwrap_or("").trim();
    if path_part.is_empty() {
        return None;
    }
    let joined = if base_dir.is_empty() {
        path_part.to_owned()
    } else {
        format!("{base_dir}/{path_part}")
    };
    Some(PackageHtmlReference {
        path: normalize_package_relative_path(&joined)?,
        anchor: (!anchor.is_empty()).then(|| anchor.to_owned()),
    })
}

pub(super) fn normalize_package_relative_path(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

pub(super) fn html_document_label(html: &str) -> Option<String> {
    ["title", "h1", "h2", "h3"]
        .into_iter()
        .find_map(|tag| html_tag_text(html, tag))
        .and_then(|label| {
            let label = collapse_label_whitespace(&strip_html_tags_for_label(&label));
            (!label.is_empty()).then_some(label)
        })
}

fn html_tag_text(html: &str, tag: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open_pattern = format!("<{tag}");
    let close_pattern = format!("</{tag}>");
    let mut cursor = 0usize;
    while let Some(relative_start) = lower[cursor..].find(&open_pattern) {
        let open_start = cursor + relative_start;
        let tag_end_byte = lower
            .as_bytes()
            .get(open_start + open_pattern.len())
            .copied()
            .unwrap_or(b'>');
        if !matches!(tag_end_byte, b'>' | b'/' | b' ' | b'\t' | b'\r' | b'\n') {
            cursor = open_start + open_pattern.len();
            continue;
        }
        let content_start = lower[open_start..]
            .find('>')
            .map(|offset| open_start + offset + 1)?;
        let content_end = lower[content_start..]
            .find(&close_pattern)
            .map(|offset| content_start + offset)?;
        return Some(html[content_start..content_end].to_owned());
    }
    None
}

fn strip_html_tags_for_label(value: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    html_unescape_label(&output)
}

fn html_unescape_label(value: &str) -> String {
    html_decode_text_entities(value)
}

fn collapse_label_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn html_attr_value(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let attr = attr.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(relative_start) = lower[cursor..].find(&attr) {
        let start = cursor + relative_start;
        let before = lower[..start].chars().next_back();
        if before.is_some_and(|ch| !ch.is_ascii_whitespace() && ch != '<') {
            cursor = start + attr.len();
            continue;
        }
        let mut index = start + attr.len();
        index = skip_ascii_whitespace(&lower, index)?;
        if !lower[index..].starts_with('=') {
            cursor = start + attr.len();
            continue;
        }
        index += 1;
        index = skip_ascii_whitespace(&lower, index)?;
        let quote = lower[index..].chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        index += quote.len_utf8();
        let rest = &tag[index..];
        let end = rest.find(quote)?;
        return Some(html_unescape_minimal(&rest[..end]));
    }
    None
}

fn skip_ascii_whitespace(value: &str, mut index: usize) -> Option<usize> {
    while index < value.len() {
        let ch = value[index..].chars().next()?;
        if !ch.is_ascii_whitespace() {
            break;
        }
        index += ch.len_utf8();
    }
    Some(index)
}

pub(super) fn html_label_text(fragment: &str) -> String {
    let mut text = String::with_capacity(fragment.len());
    let mut in_tag = false;
    for ch in fragment.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            _ => text.push(ch),
        }
    }
    html_decode_text_entities(&text).trim().to_owned()
}

pub(super) fn html_basic_text(fragment: &str) -> String {
    let mut text = String::with_capacity(fragment.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for ch in fragment.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let tag_name = tag
                    .trim_start_matches('/')
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('/')
                    .to_ascii_lowercase();
                if matches!(
                    tag_name.as_str(),
                    "br" | "p"
                        | "div"
                        | "li"
                        | "tr"
                        | "table"
                        | "article"
                        | "section"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                ) {
                    text.push('\n');
                }
            }
            _ if in_tag => tag.push(ch),
            _ => text.push(ch),
        }
    }
    html_decode_text_entities(&text)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn escape_plain_label_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

pub(super) fn sanitize_rich_label_html(fragment: &str) -> String {
    let mut output = String::with_capacity(fragment.len());
    let mut cursor = 0usize;
    while cursor < fragment.len() {
        let Some(relative_start) = fragment[cursor..].find('<') else {
            output.push_str(&escape_plain_label_html(&html_decode_text_entities(
                &fragment[cursor..],
            )));
            break;
        };
        let tag_start = cursor + relative_start;
        output.push_str(&escape_plain_label_html(&html_decode_text_entities(
            &fragment[cursor..tag_start],
        )));
        let Some(tag_end) = html_tag_end(fragment, tag_start) else {
            output.push_str("&lt;");
            cursor = tag_start + 1;
            continue;
        };
        if let Some(safe_tag) = sanitize_label_tag(&fragment[tag_start..tag_end]) {
            output.push_str(&safe_tag);
        }
        cursor = tag_end;
    }
    output
}

fn html_decode_text_entities(value: &str) -> String {
    let mut decoded = value.to_owned();
    for _ in 0..2 {
        let next = html_decode_text_entities_once(&decoded);
        if next == decoded {
            return decoded;
        }
        decoded = next;
    }
    decoded
}

fn html_decode_text_entities_once(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0usize;
    while cursor < value.len() {
        let Some(relative_start) = value[cursor..].find('&') else {
            output.push_str(&value[cursor..]);
            break;
        };
        let start = cursor + relative_start;
        output.push_str(&value[cursor..start]);
        let rest = &value[start..];
        if let Some((entity, replacement)) = decode_named_html_entity(rest) {
            output.push_str(replacement);
            cursor = start + entity.len();
            continue;
        }
        if let Some((entity_len, ch)) = decode_numeric_html_entity(rest) {
            output.push(ch);
            cursor = start + entity_len;
            continue;
        }
        output.push('&');
        cursor = start + 1;
    }
    output
}

fn decode_named_html_entity(value: &str) -> Option<(&'static str, &'static str)> {
    [
        ("&nbsp;", " "),
        ("&#160;", " "),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&amp;", "&"),
    ]
    .into_iter()
    .find(|(entity, _)| value.starts_with(entity))
}

fn decode_numeric_html_entity(value: &str) -> Option<(usize, char)> {
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

fn html_tag_end(fragment: &str, tag_start: usize) -> Option<usize> {
    let mut quote = None;
    let mut index = tag_start + 1;
    while index < fragment.len() {
        let ch = fragment[index..].chars().next()?;
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            }
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
        } else if ch == '>' {
            return Some(index + ch.len_utf8());
        }
        index += ch.len_utf8();
    }
    None
}

fn sanitize_label_tag(raw_tag: &str) -> Option<String> {
    let inner = raw_tag
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim();
    if inner.is_empty() || inner.starts_with('!') || inner.starts_with('?') {
        return None;
    }
    if let Some(closing) = inner.strip_prefix('/') {
        let name = label_tag_name(closing);
        return safe_label_container_tag(&name).then(|| format!("</{name}>"));
    }

    let name = label_tag_name(inner);
    if name == "br" {
        return Some("<br>".to_owned());
    }
    if safe_label_container_tag(&name) && name != "span" {
        return Some(format!("<{name}>"));
    }
    if name == "span" {
        let class = safe_label_class_attr(raw_tag);
        return if class.is_empty() {
            Some("<span>".to_owned())
        } else {
            Some(format!(r#"<span class="{class}">"#))
        };
    }
    if name == "img" {
        return sanitize_label_img(raw_tag);
    }
    None
}

fn label_tag_name(inner: &str) -> String {
    inner
        .trim()
        .trim_end_matches('/')
        .split_ascii_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn safe_label_container_tag(name: &str) -> bool {
    matches!(name, "b" | "strong" | "i" | "em" | "sub" | "sup" | "span")
}

fn safe_label_class_attr(raw_tag: &str) -> String {
    html_attr_value(raw_tag, "class")
        .unwrap_or_default()
        .split_whitespace()
        .filter(|class| {
            if class.starts_with("scl_") {
                return true;
            }
            matches!(
                *class,
                "lvcore-subtitle"
                    | "lvcore-gaiji"
                    | "lvcore-gaiji-external"
                    | "lvcore-gaiji-ga16"
                    | "lvcore-gaiji-unresolved"
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn sanitize_label_img(raw_tag: &str) -> Option<String> {
    let src = html_attr_value(raw_tag, "src")?;
    if !src.starts_with("lvcore://resource/") {
        return None;
    }
    let class = safe_label_class_attr(raw_tag);
    let alt = html_attr_value(raw_tag, "alt").unwrap_or_default();
    let title = html_attr_value(raw_tag, "title").unwrap_or_default();
    let mut output = String::from("<img");
    if !class.is_empty() {
        output.push_str(r#" class=""#);
        output.push_str(&escape_plain_label_html(&class));
        output.push('"');
    }
    output.push_str(r#" src=""#);
    output.push_str(&escape_plain_label_html(&src));
    output.push('"');
    if !alt.is_empty() {
        output.push_str(r#" alt=""#);
        output.push_str(&escape_plain_label_html(&alt));
        output.push('"');
    }
    if !title.is_empty() {
        output.push_str(r#" title=""#);
        output.push_str(&escape_plain_label_html(&title));
        output.push('"');
    }
    output.push('>');
    Some(output)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HtmlAttrName {
    Href,
    Src,
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HtmlAttrRange {
    pub(super) name: HtmlAttrName,
    pub(super) tag_start: usize,
    pub(super) tag_end: usize,
    pub(super) value_start: usize,
    pub(super) value_end: usize,
}

pub(super) fn next_html_href_or_src_attr(
    html: &str,
    lower: &str,
    cursor: usize,
) -> Option<HtmlAttrRange> {
    let mut search = cursor.min(lower.len());
    while search < lower.len() {
        let relative = lower[search..]
            .bytes()
            .position(|byte| matches!(byte, b'd' | b'h' | b's'))?;
        let attr_start = search + relative;
        let Some((attr_name, name)) = html_ref_attr_at(lower, attr_start) else {
            search = attr_start + 1;
            continue;
        };
        let Some(tag_start) = lower[..attr_start].rfind('<') else {
            search = attr_start + 1;
            continue;
        };
        if lower[tag_start..].starts_with("<!--") || html_index_inside_comment(lower, tag_start) {
            search = attr_start + 1;
            continue;
        }
        let Some(tag_end) = html_tag_end(html, tag_start) else {
            search = attr_start + 1;
            continue;
        };
        if tag_end <= attr_start || lower[tag_start..attr_start].contains('>') {
            search = attr_start + 1;
            continue;
        }

        let mut index = attr_start + attr_name.len();
        index = skip_ascii_whitespace(lower, index)?;
        if !lower[index..].starts_with('=') {
            search = attr_start + attr_name.len();
            continue;
        }
        index += 1;
        index = skip_ascii_whitespace(lower, index)?;
        let quote = *lower.as_bytes().get(index)?;
        if quote != b'"' && quote != b'\'' {
            search = attr_start + attr_name.len();
            continue;
        }
        let value_start = index + 1;
        let value_end = html.as_bytes()[value_start..]
            .iter()
            .position(|byte| *byte == quote)
            .map(|offset| value_start + offset)?;
        return Some(HtmlAttrRange {
            name,
            tag_start,
            tag_end,
            value_start,
            value_end,
        });
    }
    None
}

fn html_index_inside_comment(lower: &str, index: usize) -> bool {
    let before = &lower[..index.min(lower.len())];
    let Some(comment_start) = before.rfind("<!--") else {
        return false;
    };
    before
        .rfind("-->")
        .is_none_or(|comment_end| comment_end < comment_start)
}

fn html_ref_attr_at(lower: &str, attr_start: usize) -> Option<(&'static str, HtmlAttrName)> {
    [
        ("href", HtmlAttrName::Href),
        ("src", HtmlAttrName::Src),
        ("data", HtmlAttrName::Data),
    ]
    .into_iter()
    .find(|(attr_name, _)| {
        lower[attr_start..].starts_with(attr_name)
            && is_html_attr_name_boundary(lower, attr_start, attr_name.len())
    })
}

fn is_html_attr_name_boundary(lower: &str, attr_start: usize, attr_len: usize) -> bool {
    let before = lower[..attr_start].chars().next_back();
    if before.is_some_and(|ch| !ch.is_ascii_whitespace() && ch != '<') {
        return false;
    }
    let after_index = attr_start + attr_len;
    let after = lower[after_index..].chars().next();
    after.is_some_and(|ch| ch.is_ascii_whitespace() || ch == '=')
}

pub(super) fn path_has_extension(path: &str, extensions: &[&str]) -> bool {
    let extension = path.rsplit_once('.').map(|(_, extension)| extension);
    extension.is_some_and(|extension| {
        extensions
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_reader_labels_from_html() {
        assert_eq!(
            html_document_label(
                r#"<html><head><title>広辞苑 第七版 凡例</title></head><body></body></html>"#
            )
            .as_deref(),
            Some("広辞苑 第七版 凡例")
        );
        assert_eq!(
            html_document_label(
                r#"<html><body><h1><span>有斐閣&nbsp;法律学小辞典</span> 凡例</h1></body></html>"#
            )
            .as_deref(),
            Some("有斐閣 法律学小辞典 凡例")
        );
    }

    #[test]
    fn scans_href_src_and_data_attrs_in_order() {
        let html = r#"<a href = "one.html"><img src = 'two.png' data="three.bin"></a>"#;
        let lower = html.to_ascii_lowercase();
        let first = next_html_href_or_src_attr(html, &lower, 0).unwrap();
        assert_eq!(first.name, HtmlAttrName::Href);
        assert_eq!(&html[first.value_start..first.value_end], "one.html");
        let second = next_html_href_or_src_attr(html, &lower, first.value_end).unwrap();
        assert_eq!(second.name, HtmlAttrName::Src);
        assert_eq!(&html[second.value_start..second.value_end], "two.png");
        let third = next_html_href_or_src_attr(html, &lower, second.value_end).unwrap();
        assert_eq!(third.name, HtmlAttrName::Data);
        assert_eq!(&html[third.value_start..third.value_end], "three.bin");
    }

    #[test]
    fn scans_uppercase_attrs_with_multiline_whitespace() {
        let html = "<A HREF\n=\n\"one.html\"><OBJECT DATA\t=\t'two.bin'></OBJECT></A>";
        let lower = html.to_ascii_lowercase();
        let first = next_html_href_or_src_attr(html, &lower, 0).unwrap();
        assert_eq!(first.name, HtmlAttrName::Href);
        assert_eq!(&html[first.value_start..first.value_end], "one.html");
        let second = next_html_href_or_src_attr(html, &lower, first.value_end).unwrap();
        assert_eq!(second.name, HtmlAttrName::Data);
        assert_eq!(&html[second.value_start..second.value_end], "two.bin");
    }

    #[test]
    fn does_not_scan_prefixed_href_like_attrs() {
        let html = r#"<a data-href="skip.html" href="keep.html"></a>"#;
        let lower = html.to_ascii_lowercase();
        let attr = next_html_href_or_src_attr(html, &lower, 0).unwrap();
        assert_eq!(attr.name, HtmlAttrName::Href);
        assert_eq!(&html[attr.value_start..attr.value_end], "keep.html");
    }

    #[test]
    fn does_not_scan_attrs_inside_html_comments() {
        let html = r#"<a href="keep.html"></a><!--<img src="skip.png">--><img src="after.png">"#;
        let lower = html.to_ascii_lowercase();
        let first = next_html_href_or_src_attr(html, &lower, 0).unwrap();
        assert_eq!(first.name, HtmlAttrName::Href);
        assert_eq!(&html[first.value_start..first.value_end], "keep.html");
        let second = next_html_href_or_src_attr(html, &lower, first.value_end).unwrap();
        assert_eq!(second.name, HtmlAttrName::Src);
        assert_eq!(&html[second.value_start..second.value_end], "after.png");
        assert!(next_html_href_or_src_attr(html, &lower, second.value_end).is_none());
    }

    #[test]
    fn sanitizes_rich_label_html_for_app_chrome() {
        let html = r#"<b>safe</b><script>alert(1)</script><span class="hostile lvcore-subtitle">sub</span><span class="scl_ps hostile">小</span><img class="icon lvcore-gaiji lvcore-gaiji-external" src="lvcore://resource/book/token" onerror="bad()" alt="<A>" title="gaiji"><img src="javascript:alert(1)">"#;
        let sanitized = sanitize_rich_label_html(html);

        assert!(sanitized.contains("<b>safe</b>"));
        assert!(!sanitized.contains("<script"));
        assert!(!sanitized.contains("</script>"));
        assert!(!sanitized.contains("onerror"));
        assert!(!sanitized.contains("javascript:"));
        assert!(!sanitized.contains("hostile"));
        assert!(!sanitized.contains("class=\"icon"));
        assert!(sanitized.contains(r#"<span class="lvcore-subtitle">sub</span>"#));
        assert!(sanitized.contains(r#"<span class="scl_ps">小</span>"#));
        assert!(sanitized.contains(
            r#"<img class="lvcore-gaiji lvcore-gaiji-external" src="lvcore://resource/book/token" alt="&lt;A&gt;" title="gaiji">"#
        ));
    }

    #[test]
    fn label_text_decodes_numeric_entities_before_sanitizing() {
        let html = r#"<span>&#x2051;<b>test</b> &#9733; &amp;#x2605;</span>"#;

        assert_eq!(
            sanitize_rich_label_html(html),
            "<span>⁑<b>test</b> ★ ★</span>"
        );
        assert_eq!(html_label_text(html), "⁑test ★ ★");
    }
}

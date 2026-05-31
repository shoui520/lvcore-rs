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
    html_unescape_minimal(value)
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
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
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .trim()
        .to_owned()
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
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HtmlAttrName {
    Href,
    Src,
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HtmlAttrRange {
    pub(super) name: HtmlAttrName,
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
            value_start,
            value_end,
        });
    }
    None
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
}

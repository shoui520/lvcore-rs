use rusqlite::Connection;
use std::collections::BTreeSet;

use super::LvedSqliteSchema;

pub(crate) fn lved_sqlite_title_from_connection(
    connection: &Connection,
    schema: &LvedSqliteSchema,
) -> Option<String> {
    if !schema.table_has_columns("info", &["name", "body"]) {
        return None;
    }
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    for sql in title_probe_queries() {
        rows.extend(title_probe_rows(connection, &mut seen, sql)?);
        if let Some((score, title)) = best_title_candidate_with_score(&rows)
            && score >= 120
        {
            return Some(title);
        }
    }
    best_title_candidate_with_score(&rows).map(|(_, title)| title)
}

fn title_probe_queries() -> [&'static str; 4] {
    [
        "
        select rowid, name, body from info
        where body is not null and body != ''
          and lower(name) in (
            'index.html', 'index.htm',
            'about.html', 'about.htm',
            'hanrei.html', 'hanrei.htm', 'hanrei_toc.html', 'hanrei_toc.htm',
            'copyright.html', 'copyright.htm',
            'license.html', 'license.htm'
          )
        order by
          case
            when lower(name) = 'index.html' then 0
            when lower(name) = 'index.htm' then 1
            when lower(name) like 'about.%' then 2
            when lower(name) like 'hanrei%' then 3
            when lower(name) like 'copyright.%' then 4
            when lower(name) like 'license.%' then 5
            else 6
          end,
          rowid
        limit 32
        ",
        "
        select rowid, name, body from info
        where body is not null and body != ''
          and (
            lower(name) like '%index%'
            or lower(name) like '%about%'
            or lower(name) like '%hanrei%'
            or lower(name) like '%copyright%'
            or lower(name) like '%license%'
          )
        order by
          case
            when lower(name) like '%index%' then 0
            when lower(name) like '%about%' then 1
            when lower(name) like '%hanrei%' then 2
            when lower(name) like '%copyright%' then 3
            when lower(name) like '%license%' then 4
            else 5
          end,
          rowid
        limit 64
        ",
        "
        select rowid, name, body from info
        where body is not null and body != ''
          and length(body) <= 65536
        order by rowid
        limit 1024
        ",
        "
        select rowid, name, body from info
        where body is not null and body != ''
        order by rowid
        limit 64
        ",
    ]
}

fn title_probe_rows(
    connection: &Connection,
    seen: &mut BTreeSet<i64>,
    sql: &str,
) -> Option<Vec<(i64, String, String)>> {
    let mut statement = connection.prepare(sql).ok()?;
    let mapped = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            ))
        })
        .ok()?;
    let mut rows = Vec::new();
    for row in mapped {
        let Ok((rowid, name, body)) = row else {
            continue;
        };
        if seen.insert(rowid) {
            rows.push((rowid, name, body));
        }
    }
    Some(rows)
}

fn best_title_candidate_with_score(rows: &[(i64, String, String)]) -> Option<(i32, String)> {
    let mut candidates = Vec::<(i32, usize, String)>::new();
    for (index, (_rowid, name, body)) in rows.iter().enumerate() {
        let lower_name = name.to_lowercase();
        let Some(candidate) = (if lower_name.contains("copyright") || lower_name.contains("license")
        {
            lved_copyright_title_candidate(body)
        } else {
            lved_html_title_candidate(body)
        }) else {
            continue;
        };
        let mut score = title_score(&candidate);
        if score >= 100 {
            if lower_name == "index.html" {
                score += 15;
            } else if lower_name.contains("index") {
                score += 10;
            } else if lower_name.contains("menu") {
                score += 8;
            } else if lower_name.contains("about") {
                score += 5;
            } else if lower_name.contains("hanrei") {
                score += 3;
            } else if lower_name.contains("copyright") {
                score += 5;
            } else if lower_name.contains("license") {
                score += 2;
            }
        }
        candidates.push((score, index, candidate));
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    candidates
        .into_iter()
        .next()
        .and_then(|(score, _, title)| (score > 0).then_some((score, title)))
}

pub(crate) fn html_text_lines(fragment: &str) -> Vec<String> {
    let mut text = String::with_capacity(fragment.len());
    let mut in_tag = false;
    let mut tag = String::new();
    let mut skipping_element: Option<String> = None;
    for ch in fragment.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let normalized_tag = tag.trim().to_lowercase();
                let tag_name = normalized_tag
                    .trim_start_matches('/')
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                if matches!(tag_name, "style" | "script" | "rt" | "rp") {
                    if normalized_tag.starts_with('/') {
                        skipping_element = None;
                    } else {
                        skipping_element = Some(tag_name.to_owned());
                    }
                    continue;
                }
                if matches!(tag_name, "br" | "br/" | "p" | "div" | "li" | "tr") {
                    text.push('\n');
                }
            }
            _ if in_tag => tag.push(ch),
            _ if skipping_element.is_some() => {}
            _ => text.push(ch),
        }
    }
    decode_basic_html_entities(&text)
        .lines()
        .map(|line| {
            line.trim_matches(|ch: char| ch.is_whitespace() || "　・●◆".contains(ch))
                .to_owned()
        })
        .filter(|line| !line.is_empty())
        .collect()
}

pub(crate) fn lved_content_title_from_body(fragment: &str) -> Option<String> {
    for tag in ["title", "h1", "h2", "h3"] {
        if let Some(body) = first_html_element_body(fragment, tag, |_| true)
            && let Some(line) = first_lved_content_title_line(body)
        {
            return Some(line);
        }
    }
    if let Some(body) = first_html_element_body(fragment, "div", |tag| {
        html_element_has_class(tag, "midashi")
    }) && let Some(line) = first_lved_content_title_line(body)
    {
        return Some(line);
    }
    first_lved_content_title_line(fragment)
}

fn first_lved_content_title_line(fragment: &str) -> Option<String> {
    html_text_lines(fragment)
        .into_iter()
        .map(|line| line.trim().to_owned())
        .find(|line| !line.is_empty())
}

fn decode_basic_html_entities(value: &str) -> String {
    let mut decoded = value.to_owned();
    for _ in 0..2 {
        let next = decode_basic_html_entities_once(&decoded);
        if next == decoded {
            return decoded;
        }
        decoded = next;
    }
    decoded
}

fn decode_basic_html_entities_once(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut cursor = 0;
    while cursor < value.len() {
        let rest = &value[cursor..];
        if let Some((entity, replacement)) = match_basic_html_entity(rest) {
            decoded.push_str(replacement);
            cursor += entity.len();
            continue;
        }
        if let Some((entity_len, ch)) = match_numeric_html_entity(rest) {
            decoded.push(ch);
            cursor += entity_len;
            continue;
        }
        let Some(ch) = rest.chars().next() else {
            break;
        };
        decoded.push(ch);
        cursor += ch.len_utf8();
    }
    decoded
}

fn match_basic_html_entity(value: &str) -> Option<(&'static str, &'static str)> {
    if value.starts_with("&nbsp;") {
        Some(("&nbsp;", " "))
    } else if value.starts_with("&#160;") {
        Some(("&#160;", " "))
    } else if value.starts_with("&lt;") {
        Some(("&lt;", "<"))
    } else if value.starts_with("&gt;") {
        Some(("&gt;", ">"))
    } else if value.starts_with("&amp;") {
        Some(("&amp;", "&"))
    } else if value.starts_with("&quot;") {
        Some(("&quot;", "\""))
    } else if value.starts_with("&#39;") {
        Some(("&#39;", "'"))
    } else {
        None
    }
}

fn match_numeric_html_entity(value: &str) -> Option<(usize, char)> {
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

pub(crate) fn html_to_text(fragment: &str) -> String {
    html_text_lines(fragment).join(" ")
}

fn lved_copyright_title_candidate(fragment: &str) -> Option<String> {
    if let Some(explicit) = lved_explicit_book_title_candidate(fragment) {
        return Some(explicit);
    }
    let mut candidates = Vec::new();
    for line in html_text_lines(fragment).into_iter().take(24) {
        let text = quoted_title_text(&line).unwrap_or(line);
        if let Some(candidate) = normalize_title_candidate(&text) {
            candidates.push(candidate);
        }
    }
    best_scored_title_candidate(candidates)
}

fn lved_html_title_candidate(fragment: &str) -> Option<String> {
    if let Some(explicit) = lved_explicit_book_title_candidate(fragment) {
        return Some(explicit);
    }
    for tag in ["title", "h1", "h2", "h3"] {
        if let Some(body) = first_html_element_body(fragment, tag, |_| true) {
            for line in html_text_lines(body) {
                if let Some(candidate) = normalize_title_candidate(&line) {
                    return Some(candidate);
                }
            }
        }
    }
    if let Some(body) = first_html_element_body(fragment, "div", |tag| {
        let tag = tag.to_lowercase();
        tag.contains("font-weight") && tag.contains("bold")
    }) {
        for line in html_text_lines(body) {
            if let Some(candidate) = normalize_title_candidate(&line) {
                return Some(candidate);
            }
        }
    }
    html_text_lines(fragment)
        .into_iter()
        .find_map(|line| normalize_title_candidate(&line))
}

fn lved_explicit_book_title_candidate(fragment: &str) -> Option<String> {
    let mut candidates = Vec::new();
    for tag in ["div", "span"] {
        let mut cursor = 0;
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        while let Some(open_start) = find_ascii_case_insensitive_from(fragment, &open, cursor) {
            let Some(header_end) = fragment[open_start..]
                .find('>')
                .map(|offset| open_start + offset)
            else {
                break;
            };
            let body_start = header_end + 1;
            let header = &fragment[open_start..=header_end];
            let lower_header = header.to_lowercase();
            if lower_header.contains("class")
                && (lower_header.contains("book_title")
                    || lower_header.contains("book-title")
                    || lower_header.contains("booktitle")
                    || lower_header.contains("書籍名")
                    || lower_header.contains("辞書名")
                    || lower_header.contains("辞典名"))
                && let Some(close_start) =
                    find_ascii_case_insensitive_from(fragment, &close, body_start)
            {
                for line in html_text_lines(&fragment[body_start..close_start]) {
                    if let Some(candidate) = normalize_title_candidate(&line) {
                        candidates.push(candidate);
                    }
                }
            }
            cursor = body_start;
        }
    }
    best_scored_title_candidate(candidates)
}

fn best_scored_title_candidate(candidates: Vec<String>) -> Option<String> {
    candidates
        .into_iter()
        .map(|candidate| (title_score(&candidate), candidate))
        .max_by(|a, b| a.0.cmp(&b.0))
        .and_then(|(score, candidate)| (score > 0).then_some(candidate))
}

fn quoted_title_text(value: &str) -> Option<String> {
    for (open, close) in [('『', '』'), ('《', '》')] {
        let Some(start) = value.find(open) else {
            continue;
        };
        let content_start = start + open.len_utf8();
        let Some(end) = value[content_start..]
            .find(close)
            .map(|offset| offset + content_start)
        else {
            continue;
        };
        let candidate = value[content_start..end].trim();
        if (2..=80).contains(&candidate.chars().count()) {
            return Some(candidate.to_owned());
        }
    }
    None
}

fn first_html_element_body<'a, P>(fragment: &'a str, tag: &str, predicate: P) -> Option<&'a str>
where
    P: Fn(&str) -> bool,
{
    next_html_element_body(fragment, tag, 0, predicate).map(|(_, body, _)| body)
}

fn next_html_element_body<'a, P>(
    fragment: &'a str,
    tag: &str,
    start: usize,
    predicate: P,
) -> Option<(&'a str, &'a str, usize)>
where
    P: Fn(&str) -> bool,
{
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut cursor = start.min(fragment.len());
    while let Some(open_start) = find_ascii_case_insensitive_from(fragment, &open, cursor) {
        let header_end = fragment[open_start..].find('>')? + open_start;
        let body_start = header_end + 1;
        let close_start = find_ascii_case_insensitive_from(fragment, &close, body_start)?;
        let next_cursor = close_start + close.len();
        let header = &fragment[open_start..=header_end];
        if predicate(header) {
            return Some((header, &fragment[body_start..close_start], next_cursor));
        }
        cursor = next_cursor;
    }
    None
}

fn html_element_has_class(tag: &str, class_name: &str) -> bool {
    let lower = tag.to_ascii_lowercase();
    let Some(class_pos) = lower.find("class") else {
        return false;
    };
    let Some(eq_pos) = lower[class_pos + "class".len()..].find('=') else {
        return false;
    };
    let mut value_start = class_pos + "class".len() + eq_pos + 1;
    while value_start < tag.len() && tag.as_bytes()[value_start].is_ascii_whitespace() {
        value_start += 1;
    }
    let Some(quote) = tag[value_start..].chars().next() else {
        return false;
    };
    if quote != '"' && quote != '\'' {
        return false;
    }
    value_start += quote.len_utf8();
    let Some(value_end) = tag[value_start..].find(quote) else {
        return false;
    };
    tag[value_start..value_start + value_end]
        .split_whitespace()
        .any(|value| value.eq_ignore_ascii_case(class_name))
}

fn find_ascii_case_insensitive_from(haystack: &str, needle: &str, start: usize) -> Option<usize> {
    if needle.is_empty() || start >= haystack.len() {
        return None;
    }
    let haystack_bytes = haystack.as_bytes();
    let needle_bytes = needle.as_bytes();
    haystack_bytes
        .get(start..)?
        .windows(needle_bytes.len())
        .position(|window| window.eq_ignore_ascii_case(needle_bytes))
        .map(|offset| start + offset)
}

pub(crate) fn normalize_title_candidate(value: &str) -> Option<String> {
    let mut value = value
        .split("について")
        .next()
        .unwrap_or(value)
        .trim_matches(|ch: char| ch.is_whitespace() || "　:：-－【】『』《》".contains(ch))
        .to_owned();
    value = remove_inline_book_quote_marks(&value);
    for prefix in ["書名", "書籍名", "辞書名", "辞典名"] {
        if let Some(stripped) = value.strip_prefix(prefix)
            && stripped.chars().count() >= 2
        {
            value = stripped
                .trim_matches(|ch: char| ch.is_whitespace() || "　:：-－【】『』《》".contains(ch))
                .to_owned();
            break;
        }
    }
    for marker in ["&copy;", "©", "Copyright", "copyright", "(C)", "（C）"] {
        if let Some((head, _tail)) = value.split_once(marker) {
            value = head.trim().to_owned();
        }
    }
    for marker in [
        " 凡例",
        "　凡例",
        " 目次",
        "　目次",
        " 付録",
        "　付録",
        " 著作権",
        "　著作権",
        " ●",
        "　●",
    ] {
        if let Some((head, _tail)) = value.split_once(marker) {
            value = head.trim().to_owned();
        }
    }
    if let Some(stripped) = value.strip_prefix("書籍版") {
        value = stripped
            .trim_matches(|ch: char| ch.is_whitespace() || "　:：-－【】『』《》".contains(ch))
            .to_owned();
    }
    if value.ends_with('序')
        && ["辞典", "事典", "辞書", "字典"]
            .iter()
            .any(|marker| value.contains(marker))
    {
        value.pop();
    }
    value = value
        .trim_matches(|ch: char| ch.is_whitespace() || "　:：-－【】『』《》".contains(ch))
        .to_owned();
    let char_count = value.chars().count();
    if value.is_empty() || char_count > 60 {
        return None;
    }
    let generic = [
        "凡例",
        "著作権",
        "著作権について",
        "目次",
        "凡例・著作権・その他",
        "凡例・その他",
        "はじめに",
        "記号一覧",
        "略号・記号一覧表",
        "copyright",
    ];
    if generic.iter().any(|item| value.eq_ignore_ascii_case(item)) {
        return None;
    }
    if value.starts_with("Copyright")
        || value.starts_with("©")
        || value.contains("LogoVista電子辞典")
    {
        return None;
    }
    if value.contains('。')
        || value.contains('．')
        || value.contains("この辞書")
        || value.contains("この辞典")
        || value.contains("教授")
    {
        return None;
    }
    Some(value)
}

fn remove_inline_book_quote_marks(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '『' | '《' => {}
            '』' | '》' => {
                if chars.peek().is_some_and(|next| !next.is_whitespace()) {
                    out.push(' ');
                }
            }
            _ => out.push(ch),
        }
    }
    out.trim().to_owned()
}

pub(crate) fn title_score(value: &str) -> i32 {
    let mut score = 0;
    for keyword in [
        "辞典",
        "事典",
        "辞書",
        "字典",
        "大辞典",
        "広辞苑",
        "大辞林",
        "字通",
        "シソーラス",
        "リーダーズ",
        "ロワイヤル",
        "大百科",
        "百科",
        "現代用語",
        "国語",
        "英和",
        "和英",
        "仏和",
        "和仏",
        "独和",
        "和独",
        "中日",
        "日中",
        "法律",
        "医学",
        "数学",
        "理化学",
        "仏教",
        "世界人名",
        "世界史",
        "日本史",
        "古語",
        "漢語",
        "類語",
        "用語",
        "文例集",
        "Dictionary",
        "Thesaurus",
        "Encyclopedia",
    ] {
        if value.contains(keyword) {
            score += 120;
            break;
        }
    }
    if value.contains('第') && value.contains('版') {
        score += 30;
    }
    if value.starts_with("NEW ") {
        score += 10;
    }
    if value.contains("この辞書") {
        score -= 200;
    }
    for weak in [
        "凡例",
        "索引",
        "一覧",
        "インデックス",
        "目次",
        "使い方",
        "はしがき",
        "編集",
        "著作権",
        "記号",
        "略語",
        "掲載語",
    ] {
        if value.contains(weak) {
            score -= 90;
        }
    }
    if matches!(value, "Index" | "LVED") {
        score -= 200;
    }
    if value.contains("小辞典") && !value.contains('第') {
        score -= 80;
    }
    if value.chars().count() > 50 {
        score -= 20;
    }
    score
}

pub fn render_britannica_html_fragment(fragment: &str) -> String {
    let without_body = strip_outer_body(fragment);
    normalize_britannica_whatday_table(&without_body)
        .trim()
        .to_owned()
}

pub(super) fn plain_text_from_html(fragment: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for ch in fragment.chars() {
        match ch {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    collapse_whitespace(&html_unescape_minimal(&text))
}

fn strip_outer_body(fragment: &str) -> String {
    let mut output = fragment.trim().to_owned();
    let lower = output.to_ascii_lowercase();
    if let Some(start) = lower.find("<body")
        && let Some(end_rel) = lower[start..].find('>')
    {
        output.replace_range(start..start + end_rel + 1, "");
    }
    let lower = output.to_ascii_lowercase();
    if let Some(start) = lower.rfind("</body>") {
        output.replace_range(start..start + "</body>".len(), "");
    }
    output
}

fn normalize_britannica_whatday_table(fragment: &str) -> String {
    let mut output = normalize_colspan_three_to_two(fragment);
    output = remove_britannica_middle_table_cells(&output);
    output
}

fn normalize_colspan_three_to_two(fragment: &str) -> String {
    let mut output = String::with_capacity(fragment.len());
    let mut cursor = 0usize;
    let lower = fragment.to_ascii_lowercase();
    while let Some(relative) = lower[cursor..].find("colspan") {
        let start = cursor + relative;
        output.push_str(&fragment[cursor..start]);
        let Some(eq_relative) = lower[start..].find('=') else {
            output.push_str(&fragment[start..]);
            return output;
        };
        let eq = start + eq_relative;
        output.push_str(&fragment[start..=eq]);
        let mut value_start = eq + 1;
        while value_start < fragment.len() && fragment.as_bytes()[value_start].is_ascii_whitespace()
        {
            output.push(fragment.as_bytes()[value_start] as char);
            value_start += 1;
        }
        let quote = fragment.as_bytes().get(value_start).copied();
        let (value, value_end) = match quote {
            Some(q @ (b'"' | b'\'')) => {
                let inner_start = value_start + 1;
                let Some(end_rel) = fragment.as_bytes()[inner_start..]
                    .iter()
                    .position(|byte| *byte == q)
                else {
                    output.push_str(&fragment[value_start..]);
                    return output;
                };
                (
                    &fragment[inner_start..inner_start + end_rel],
                    inner_start + end_rel + 1,
                )
            }
            _ => {
                let end = fragment.as_bytes()[value_start..]
                    .iter()
                    .position(|byte| byte.is_ascii_whitespace() || *byte == b'>')
                    .map(|offset| value_start + offset)
                    .unwrap_or(fragment.len());
                (&fragment[value_start..end], end)
            }
        };
        if value == "3" {
            match quote {
                Some(b'"') => output.push_str("\"2\""),
                Some(b'\'') => output.push_str("'2'"),
                _ => output.push('2'),
            }
        } else {
            output.push_str(&fragment[value_start..value_end]);
        }
        cursor = value_end;
    }
    output.push_str(&fragment[cursor..]);
    output
}

fn remove_britannica_middle_table_cells(fragment: &str) -> String {
    let mut output = String::with_capacity(fragment.len());
    let lower = fragment.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(row_start_rel) = lower[cursor..].find("<tr") {
        let row_start = cursor + row_start_rel;
        let Some(row_end_rel) = lower[row_start..].find("</tr>") else {
            break;
        };
        let row_end = row_start + row_end_rel + "</tr>".len();
        output.push_str(&fragment[cursor..row_start]);
        let row = &fragment[row_start..row_end];
        output.push_str(&remove_middle_cell_if_three_td(row));
        cursor = row_end;
    }
    output.push_str(&fragment[cursor..]);
    output
}

fn remove_middle_cell_if_three_td(row: &str) -> String {
    let lower = row.to_ascii_lowercase();
    let mut cells = Vec::new();
    let mut cursor = 0usize;
    while let Some(start_rel) = lower[cursor..].find("<td") {
        let start = cursor + start_rel;
        let Some(end_rel) = lower[start..].find("</td>") else {
            break;
        };
        let end = start + end_rel + "</td>".len();
        cells.push((start, end));
        cursor = end;
    }
    if cells.len() != 3 {
        return row.to_owned();
    }
    let mut output = String::with_capacity(row.len());
    output.push_str(&row[..cells[1].0]);
    output.push_str(&row[cells[1].1..]);
    output
}

fn html_unescape_minimal(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

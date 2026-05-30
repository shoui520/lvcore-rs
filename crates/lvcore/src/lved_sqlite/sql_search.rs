use rusqlite::{Connection, Row};

use super::title::html_to_text;
use super::{LvedSearchHit, LvedSqliteSchema, has_column, nonempty_string, sqlite_value_to_string};
use crate::error::{Error, Result};
use crate::search::SearchMode;

pub(super) fn search_lved_sqlite_connection(
    connection: &Connection,
    schema: &LvedSqliteSchema,
    query: &str,
    mode: &SearchMode,
    offset: usize,
    limit: usize,
) -> Result<Vec<LvedSearchHit>> {
    if !schema.table_exists("search") || !schema.table_exists("list") {
        return Ok(Vec::new());
    }
    let search_columns = schema.columns("search");
    let list_columns = schema.columns("list");
    if !has_column(list_columns, "id") || !has_column(list_columns, "refid") {
        return Ok(Vec::new());
    }
    let normalized = normalize_lved_query(query);
    if normalized.is_empty() {
        return Ok(Vec::new());
    }
    let Some((where_clause, parameter)) = lved_search_where(&normalized, mode, search_columns)
    else {
        return Ok(Vec::new());
    };

    let projection = lved_list_projection(list_columns);
    let sql = format!(
        "select l.id, l.refid, {}, {}, {}, {} \
         from search s join list l on l.id = s.rowid where {where_clause} order by l.id limit ? offset ?",
        projection.anchor, projection.title, projection.subtitle, projection.kind
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        (parameter, limit as i64, offset as i64),
        lved_search_hit_from_row,
    )?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

pub(super) fn lved_list_hits_by_id_clause(
    connection: &Connection,
    list_columns: &[String],
    where_clause: &str,
    order_clause: &str,
    id: i64,
    limit: usize,
) -> Result<Vec<LvedSearchHit>> {
    lved_list_hits_by_id_clause_offset(
        connection,
        list_columns,
        where_clause,
        order_clause,
        id,
        limit,
        0,
    )
}

pub(super) fn lved_list_hits_by_id_clause_offset(
    connection: &Connection,
    list_columns: &[String],
    where_clause: &str,
    order_clause: &str,
    id: i64,
    limit: usize,
    offset: usize,
) -> Result<Vec<LvedSearchHit>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let projection = lved_list_projection(list_columns);
    let sql = format!(
        "select l.id, l.refid, {}, {}, {}, {} \
         from list l where {where_clause} order by {order_clause} limit ? offset ?",
        projection.anchor, projection.title, projection.subtitle, projection.kind
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map((id, limit as i64, offset as i64), lved_search_hit_from_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

fn lved_search_hit_from_row(row: &Row<'_>) -> rusqlite::Result<LvedSearchHit> {
    let title_html = sqlite_value_to_string(row.get_ref(3)?)?;
    Ok(LvedSearchHit {
        list_id: row.get(0)?,
        content_id: row.get(1)?,
        anchor: nonempty_string(sqlite_value_to_string(row.get_ref(2)?)?),
        title_text: html_to_text(&title_html),
        title_html,
        subtitle_html: sqlite_value_to_string(row.get_ref(4)?)?,
        list_type: row.get(5)?,
    })
}

fn lved_search_where(
    normalized: &str,
    mode: &SearchMode,
    search_columns: &[String],
) -> Option<(String, String)> {
    match mode {
        SearchMode::Exact => {
            if has_column(search_columns, "filter") {
                Some((
                    "s.filter like ? escape '\\'".to_owned(),
                    format!("%∥{}∥%", escape_sql_like(normalized)),
                ))
            } else if has_column(search_columns, "forward") {
                Some(("s.forward = ?".to_owned(), normalized.to_owned()))
            } else {
                None
            }
        }
        SearchMode::Forward => fts_match("forward", normalized, search_columns, true, false),
        SearchMode::Backward => {
            let reversed = normalized.chars().rev().collect::<String>();
            fts_match("back", &reversed, search_columns, true, false)
        }
        SearchMode::Partial => fts_match("part", normalized, search_columns, false, true),
        SearchMode::FullText => fts_match(
            "fts",
            normalized,
            search_columns,
            false,
            should_split_chars(normalized),
        ),
        SearchMode::Advanced(column) => fts_match(
            column,
            normalized,
            search_columns,
            false,
            should_split_chars(normalized),
        ),
    }
}

pub(super) fn lved_available_search_modes(schema: &LvedSqliteSchema) -> Vec<SearchMode> {
    if !schema.table_exists("search") || !schema.table_exists("list") {
        return Vec::new();
    }
    let search_columns = schema.columns("search");
    let list_columns = schema.columns("list");
    if !has_column(list_columns, "id") || !has_column(list_columns, "refid") {
        return Vec::new();
    }
    let mut modes = Vec::new();
    if has_column(search_columns, "filter") || has_column(search_columns, "forward") {
        modes.push(SearchMode::Exact);
    }
    if has_column(search_columns, "forward") {
        modes.push(SearchMode::Forward);
    }
    if has_column(search_columns, "back") {
        modes.push(SearchMode::Backward);
    }
    if has_column(search_columns, "part") {
        modes.push(SearchMode::Partial);
    }
    if has_column(search_columns, "fts") {
        modes.push(SearchMode::FullText);
    }
    modes.extend(
        search_columns
            .iter()
            .filter(|column| column.starts_with("advanced"))
            .cloned()
            .map(SearchMode::Advanced),
    );
    modes
}

fn fts_match(
    column: &str,
    normalized: &str,
    search_columns: &[String],
    prefix_last: bool,
    split_chars: bool,
) -> Option<(String, String)> {
    if !has_column(search_columns, column) {
        return None;
    }
    let tokens = fts_tokens(normalized, split_chars);
    let terms = tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| {
            let term = fts_term(token, prefix_last && index + 1 == tokens.len());
            (!term.is_empty()).then(|| format!("{column}:{term}"))
        })
        .collect::<Vec<_>>();
    (!terms.is_empty()).then(|| {
        (
            "s.rowid in (select rowid from search where search match ?)".to_owned(),
            terms.join(" "),
        )
    })
}

fn normalize_lved_query(query: &str) -> String {
    query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn fts_tokens(query: &str, split_chars: bool) -> Vec<String> {
    if split_chars {
        return query
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .map(|ch| ch.to_string())
            .collect();
    }
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in query.chars() {
        if ch.is_alphanumeric() || matches!(ch, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}') {
            current.push(ch);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn fts_term(term: &str, prefix: bool) -> String {
    let cleaned = term
        .chars()
        .filter(|ch| !matches!(ch, '"' | '\'' | ':' | '*' | '(' | ')') && !ch.is_whitespace())
        .collect::<String>();
    if cleaned.is_empty() {
        String::new()
    } else if prefix {
        format!("{cleaned}*")
    } else {
        cleaned
    }
}

fn should_split_chars(query: &str) -> bool {
    !query.chars().any(|ch| ch.is_ascii_alphanumeric())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LvedListProjection<'a> {
    pub(super) anchor: &'a str,
    pub(super) title: &'a str,
    pub(super) subtitle: &'a str,
    pub(super) kind: &'a str,
}

pub(super) fn lved_list_projection(columns: &[String]) -> LvedListProjection<'_> {
    let subtitle = if has_column(columns, "titlesub") {
        "l.titlesub"
    } else if has_column(columns, "subtext") {
        "l.subtext"
    } else if has_column(columns, "titleplain") {
        "l.titleplain"
    } else {
        "''"
    };
    LvedListProjection {
        anchor: optional_column_expr(columns, "anchor", "''"),
        title: optional_column_expr(columns, "title", "''"),
        subtitle,
        kind: optional_column_expr(columns, "type", "null"),
    }
}

fn optional_column_expr<'a>(columns: &'a [String], column: &'a str, fallback: &'a str) -> &'a str {
    if has_column(columns, column) {
        match column {
            "anchor" => "l.anchor",
            "title" => "l.title",
            "type" => "l.type",
            _ => fallback,
        }
    } else {
        fallback
    }
}

fn escape_sql_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

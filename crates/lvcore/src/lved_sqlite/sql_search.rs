use rusqlite::types::Value;
use rusqlite::{Connection, Row, params_from_iter};
use std::collections::BTreeMap;

use super::title::html_to_text;
use super::{
    LvedSearchHit, LvedSqliteSchema, has_column, nonempty_string, quote_identifier,
    sqlite_value_to_string,
};
use crate::error::{Error, Result};
use crate::search::SearchMode;

#[derive(Debug, Clone, Copy)]
struct LvedSearchProvider<'a> {
    search_table: &'static str,
    list_table: &'static str,
    search_columns: &'a [String],
    list_columns: &'a [String],
}

pub(super) fn search_lved_sqlite_connection(
    connection: &Connection,
    schema: &LvedSqliteSchema,
    query: &str,
    mode: &SearchMode,
    offset: usize,
    limit: usize,
) -> Result<Vec<LvedSearchHit>> {
    let normalized_variants = lved_query_variants(query);
    if normalized_variants.is_empty() {
        return Ok(Vec::new());
    }
    let Some(provider) = lved_search_provider_for_mode(schema, mode) else {
        return Ok(Vec::new());
    };
    if normalized_variants.len() > 1
        && let Some(match_queries) =
            lved_fts_match_queries(&normalized_variants, mode, provider.search_columns)
    {
        return search_lved_sqlite_fts_variants(
            connection,
            provider,
            &match_queries,
            offset,
            limit,
        );
    }
    // SQLite FTS rejects `OR` over multiple direct `MATCH` clauses, so
    // hiragana/katakana variant searches keep the subquery form. Single-variant
    // searches can use the direct FTS table expression, which avoids
    // materializing broad match sets before the list join.
    let prefer_direct_fts = normalized_variants.len() == 1;
    let mut where_clauses = Vec::new();
    let mut parameters = Vec::new();
    for normalized in normalized_variants {
        if let Some((where_clause, mut variant_parameters)) =
            lved_search_where(&normalized, mode, provider, prefer_direct_fts)
        {
            where_clauses.push(format!("({where_clause})"));
            parameters.append(&mut variant_parameters);
        }
    }
    if where_clauses.is_empty() {
        return Ok(Vec::new());
    }
    let where_clause = where_clauses.join(" or ");

    let projection = lved_list_projection(provider.list_columns);
    let search_table = quote_identifier(provider.search_table);
    let list_table = quote_identifier(provider.list_table);
    let sql = format!(
        "select l.id, l.refid, {}, {}, {}, {} \
         from {search_table} s join {list_table} l on l.id = s.rowid where {where_clause} order by l.id limit ? offset ?",
        projection.anchor, projection.title, projection.subtitle, projection.kind
    );
    let mut statement = connection.prepare(&sql)?;
    let mut sql_parameters = parameters
        .into_iter()
        .map(Value::Text)
        .collect::<Vec<Value>>();
    sql_parameters.push(Value::Integer(limit as i64));
    sql_parameters.push(Value::Integer(offset as i64));
    let rows = statement.query_map(params_from_iter(sql_parameters), lved_search_hit_from_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

fn search_lved_sqlite_fts_variants(
    connection: &Connection,
    provider: LvedSearchProvider<'_>,
    match_queries: &[String],
    offset: usize,
    limit: usize,
) -> Result<Vec<LvedSearchHit>> {
    let fetch_limit = offset.saturating_add(limit);
    if fetch_limit == 0 {
        return Ok(Vec::new());
    }
    let projection = lved_list_projection(provider.list_columns);
    let search_table = quote_identifier(provider.search_table);
    let list_table = quote_identifier(provider.list_table);
    let match_expr = fts_table_match_expr(provider.search_table);
    let sql = format!(
        "select l.id, l.refid, {}, {}, {}, {} \
         from {search_table} join {list_table} l on l.id = {search_table}.rowid where {match_expr} order by l.id limit ?",
        projection.anchor, projection.title, projection.subtitle, projection.kind
    );
    let mut hits_by_list_id = BTreeMap::new();
    for match_query in match_queries {
        let mut statement = connection.prepare(&sql)?;
        let rows =
            statement.query_map((match_query, fetch_limit as i64), lved_search_hit_from_row)?;
        for row in rows {
            let hit = row?;
            hits_by_list_id.entry(hit.list_id).or_insert(hit);
        }
    }
    Ok(hits_by_list_id
        .into_values()
        .skip(offset)
        .take(limit)
        .collect())
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
    provider: LvedSearchProvider<'_>,
    prefer_direct_fts: bool,
) -> Option<(String, Vec<String>)> {
    let search_columns = provider.search_columns;
    match mode {
        SearchMode::Exact => exact_lved_search_where(normalized, provider, prefer_direct_fts),
        SearchMode::Forward => one_parameter_where(fts_match(
            provider.search_table,
            "forward",
            normalized,
            search_columns,
            true,
            false,
            prefer_direct_fts,
        )),
        SearchMode::Backward => {
            let reversed = normalized.chars().rev().collect::<String>();
            one_parameter_where(fts_match(
                provider.search_table,
                "back",
                &reversed,
                search_columns,
                true,
                false,
                prefer_direct_fts,
            ))
        }
        SearchMode::Partial => one_parameter_where(fts_match(
            provider.search_table,
            "part",
            normalized,
            search_columns,
            false,
            true,
            prefer_direct_fts,
        )),
        SearchMode::FullText => one_parameter_where(fts_match(
            provider.search_table,
            "fts",
            normalized,
            search_columns,
            false,
            should_split_chars(normalized),
            prefer_direct_fts,
        )),
        SearchMode::Advanced(column) => one_parameter_where(fts_match(
            provider.search_table,
            column,
            normalized,
            search_columns,
            false,
            should_split_chars(normalized),
            prefer_direct_fts,
        )),
    }
}

fn lved_fts_match_queries(
    normalized_variants: &[String],
    mode: &SearchMode,
    search_columns: &[String],
) -> Option<Vec<String>> {
    let queries = normalized_variants
        .iter()
        .filter_map(|normalized| lved_fts_match_query(normalized, mode, search_columns))
        .collect::<Vec<_>>();
    (!queries.is_empty()).then_some(queries)
}

fn lved_fts_match_query(
    normalized: &str,
    mode: &SearchMode,
    search_columns: &[String],
) -> Option<String> {
    match mode {
        SearchMode::Exact => None,
        SearchMode::Forward => fts_query("forward", normalized, search_columns, true, false),
        SearchMode::Backward => {
            let reversed = normalized.chars().rev().collect::<String>();
            fts_query("back", &reversed, search_columns, true, false)
        }
        SearchMode::Partial => fts_query("part", normalized, search_columns, false, true),
        SearchMode::FullText => fts_query(
            "fts",
            normalized,
            search_columns,
            false,
            should_split_chars(normalized),
        ),
        SearchMode::Advanced(column) => fts_query(
            column,
            normalized,
            search_columns,
            false,
            should_split_chars(normalized),
        ),
    }
}

fn exact_lved_search_where(
    normalized: &str,
    provider: LvedSearchProvider<'_>,
    prefer_direct_fts: bool,
) -> Option<(String, Vec<String>)> {
    let search_columns = provider.search_columns;
    if has_column(search_columns, "filter") {
        let like_parameter = format!("%∥{}∥%", escape_sql_like(normalized));
        if let Some(match_query) = exact_lved_filter_prefilter_query(normalized, search_columns) {
            let where_clause = if prefer_direct_fts {
                format!(
                    "{} and s.filter like ? escape '\\'",
                    fts_table_match_expr(provider.search_table)
                )
            } else {
                let search_table = quote_identifier(provider.search_table);
                format!(
                    "s.rowid in (select rowid from {search_table} where {search_table} match ?) \
                     and s.filter like ? escape '\\'"
                )
            };
            return Some((where_clause, vec![match_query, like_parameter]));
        }
        return Some((
            "s.filter like ? escape '\\'".to_owned(),
            vec![like_parameter],
        ));
    }
    if has_column(search_columns, "forward") {
        return Some(("s.forward = ?".to_owned(), vec![normalized.to_owned()]));
    }
    None
}

fn exact_lved_filter_prefilter_query(
    normalized: &str,
    search_columns: &[String],
) -> Option<String> {
    if !normalized.chars().any(char::is_alphanumeric) {
        return None;
    }
    let short_headword_prefilter =
        normalized.chars().count() <= 2 && normalized.chars().all(char::is_alphanumeric);
    if short_headword_prefilter
        && let Some(query) = fts_query("forward", normalized, search_columns, true, false)
    {
        return Some(query);
    }
    let split_chars = should_split_chars(normalized);
    fts_query("part", normalized, search_columns, false, split_chars).or_else(|| {
        let terms = fts_tokens(normalized, split_chars)
            .into_iter()
            .filter(|token| token.chars().any(char::is_alphanumeric))
            .map(|token| fts_term(&token, false))
            .filter(|term| !term.is_empty())
            .collect::<Vec<_>>();
        (!terms.is_empty()).then(|| terms.join(" "))
    })
}

pub(super) fn lved_available_search_modes(schema: &LvedSqliteSchema) -> Vec<SearchMode> {
    let mut modes = Vec::new();
    if let Some(provider) = lved_primary_search_provider(schema) {
        extend_lved_search_modes(&mut modes, provider.search_columns, true);
    }
    if let Some(provider) = lved_secondary_search_provider(schema) {
        extend_lved_search_modes(&mut modes, provider.search_columns, false);
    }
    modes
}

fn lved_search_provider_for_mode<'a>(
    schema: &'a LvedSqliteSchema,
    mode: &SearchMode,
) -> Option<LvedSearchProvider<'a>> {
    if let Some(provider) = lved_primary_search_provider(schema)
        && lved_search_mode_supported_by_columns(mode, provider.search_columns)
    {
        return Some(provider);
    }
    if let Some(provider) = lved_secondary_search_provider(schema)
        && matches!(mode, SearchMode::Advanced(_))
        && lved_search_mode_supported_by_columns(mode, provider.search_columns)
    {
        return Some(provider);
    }
    None
}

fn lved_primary_search_provider(schema: &LvedSqliteSchema) -> Option<LvedSearchProvider<'_>> {
    lved_search_provider(schema, "search", "list")
}

fn lved_secondary_search_provider(schema: &LvedSqliteSchema) -> Option<LvedSearchProvider<'_>> {
    lved_search_provider(schema, "searchsub", "listsub")
}

fn lved_search_provider<'a>(
    schema: &'a LvedSqliteSchema,
    search_table: &'static str,
    list_table: &'static str,
) -> Option<LvedSearchProvider<'a>> {
    if !schema.table_exists(search_table) || !schema.table_exists(list_table) {
        return None;
    }
    let search_columns = schema.columns(search_table);
    let list_columns = schema.columns(list_table);
    if !has_column(list_columns, "id") || !has_column(list_columns, "refid") {
        return None;
    }
    Some(LvedSearchProvider {
        search_table,
        list_table,
        search_columns,
        list_columns,
    })
}

fn lved_search_mode_supported_by_columns(mode: &SearchMode, search_columns: &[String]) -> bool {
    match mode {
        SearchMode::Exact => {
            has_column(search_columns, "filter") || has_column(search_columns, "forward")
        }
        SearchMode::Forward => has_column(search_columns, "forward"),
        SearchMode::Backward => has_column(search_columns, "back"),
        SearchMode::Partial => has_column(search_columns, "part"),
        SearchMode::FullText => has_column(search_columns, "fts"),
        SearchMode::Advanced(column) => has_column(search_columns, column),
    }
}

fn extend_lved_search_modes(
    modes: &mut Vec<SearchMode>,
    search_columns: &[String],
    include_standard: bool,
) {
    if include_standard {
        push_unique_mode_if(
            modes,
            SearchMode::Exact,
            has_column(search_columns, "filter") || has_column(search_columns, "forward"),
        );
        push_unique_mode_if(
            modes,
            SearchMode::Forward,
            has_column(search_columns, "forward"),
        );
        push_unique_mode_if(
            modes,
            SearchMode::Backward,
            has_column(search_columns, "back"),
        );
        push_unique_mode_if(
            modes,
            SearchMode::Partial,
            has_column(search_columns, "part"),
        );
        push_unique_mode_if(
            modes,
            SearchMode::FullText,
            has_column(search_columns, "fts"),
        );
    }
    for column in search_columns
        .iter()
        .filter(|column| column.starts_with("advanced"))
    {
        push_unique_mode_if(modes, SearchMode::Advanced(column.clone()), true);
    }
}

fn push_unique_mode_if(modes: &mut Vec<SearchMode>, mode: SearchMode, condition: bool) {
    if condition && !modes.contains(&mode) {
        modes.push(mode);
    }
}

fn fts_match(
    search_table: &str,
    column: &str,
    normalized: &str,
    search_columns: &[String],
    prefix_last: bool,
    split_chars: bool,
    prefer_direct_fts: bool,
) -> Option<(String, String)> {
    let query = fts_query(column, normalized, search_columns, prefix_last, split_chars)?;
    let where_clause = if prefer_direct_fts {
        fts_table_match_expr(search_table)
    } else {
        let search_table = quote_identifier(search_table);
        format!("s.rowid in (select rowid from {search_table} where {search_table} match ?)")
    };
    Some((where_clause, query))
}

fn fts_table_match_expr(search_table: &str) -> String {
    let search_table = quote_identifier(search_table);
    format!("{search_table} match ?")
}

fn fts_query(
    column: &str,
    normalized: &str,
    search_columns: &[String],
    prefix_last: bool,
    split_chars: bool,
) -> Option<String> {
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
    (!terms.is_empty()).then(|| terms.join(" "))
}

fn one_parameter_where(value: Option<(String, String)>) -> Option<(String, Vec<String>)> {
    value.map(|(where_clause, parameter)| (where_clause, vec![parameter]))
}

fn normalize_lved_query(query: &str) -> String {
    query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn lved_query_variants(query: &str) -> Vec<String> {
    let normalized = normalize_lved_query(query);
    if normalized.is_empty() {
        return Vec::new();
    }
    let mut variants = vec![normalized.clone()];
    let katakana = hiragana_to_katakana(&normalized);
    if katakana != normalized {
        variants.push(katakana);
    }
    variants
}

fn hiragana_to_katakana(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\u{3041}'..='\u{3096}' => char::from_u32(ch as u32 + 0x60).unwrap_or(ch),
            '\u{309d}' => '\u{30fd}',
            '\u{309e}' => '\u{30fe}',
            _ => ch,
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn search_columns(columns: &[&str]) -> Vec<String> {
        columns.iter().map(|column| (*column).to_owned()).collect()
    }

    fn primary_provider<'a>(columns: &'a [String]) -> LvedSearchProvider<'a> {
        const LIST_COLUMNS: &[String] = &[];
        LvedSearchProvider {
            search_table: "search",
            list_table: "list",
            search_columns: columns,
            list_columns: LIST_COLUMNS,
        }
    }

    #[test]
    fn exact_filter_search_prefilters_ascii_terms_with_fts() {
        let columns = search_columns(&["forward", "back", "part", "fts", "filter"]);

        let (where_clause, parameters) =
            exact_lved_search_where("abacus", primary_provider(&columns), true)
                .expect("exact filter search");

        assert!(where_clause.contains("\"search\" match ?"));
        assert!(where_clause.contains("s.filter like ?"));
        assert_eq!(parameters, vec!["part:abacus", "%∥abacus∥%"]);
    }

    #[test]
    fn exact_filter_search_uses_forward_prefilter_for_short_ascii_headwords() {
        let columns = search_columns(&["forward", "back", "part", "fts", "filter"]);

        let (where_clause, parameters) =
            exact_lved_search_where("a", primary_provider(&columns), true)
                .expect("exact filter search");

        assert!(where_clause.contains("\"search\" match ?"));
        assert!(where_clause.contains("s.filter like ?"));
        assert_eq!(parameters, vec!["forward:a*", "%∥a∥%"]);
    }

    #[test]
    fn exact_filter_search_uses_forward_prefilter_for_two_character_headwords() {
        let columns = search_columns(&["forward", "back", "part", "fts", "filter"]);

        let (where_clause, parameters) =
            exact_lved_search_where("ああ", primary_provider(&columns), true)
                .expect("exact filter search");

        assert!(where_clause.contains("\"search\" match ?"));
        assert!(where_clause.contains("s.filter like ?"));
        assert_eq!(parameters, vec!["forward:ああ*", "%∥ああ∥%"]);
    }

    #[test]
    fn exact_filter_search_prefilters_non_ascii_terms_with_fts() {
        let columns = search_columns(&["forward", "back", "part", "fts", "filter"]);

        let (where_clause, parameters) =
            exact_lved_search_where("あいう", primary_provider(&columns), true)
                .expect("exact filter search");

        assert!(where_clause.contains("\"search\" match ?"));
        assert!(where_clause.contains("s.filter like ?"));
        assert_eq!(parameters, vec!["part:あ part:い part:う", "%∥あいう∥%"]);
    }

    #[test]
    fn exact_filter_search_uses_forward_prefilter_for_short_non_ascii_headwords() {
        let columns = search_columns(&["forward", "back", "part", "fts", "filter"]);

        let (where_clause, parameters) =
            exact_lved_search_where("あ", primary_provider(&columns), true)
                .expect("exact filter search");

        assert!(where_clause.contains("\"search\" match ?"));
        assert!(where_clause.contains("s.filter like ?"));
        assert_eq!(parameters, vec!["forward:あ*", "%∥あ∥%"]);
    }

    #[test]
    fn exact_filter_search_keeps_like_scan_for_symbol_only_terms() {
        let columns = search_columns(&["forward", "back", "part", "fts", "filter"]);

        let (where_clause, parameters) =
            exact_lved_search_where("／", primary_provider(&columns), true)
                .expect("exact filter search");

        assert!(!where_clause.contains("\"search\" match ?"));
        assert_eq!(where_clause, "s.filter like ? escape '\\'");
        assert_eq!(parameters, vec!["%∥／∥%"]);
    }

    #[test]
    fn exact_filter_search_uses_subquery_prefilter_for_variant_or_queries() {
        let columns = search_columns(&["forward", "back", "part", "fts", "filter"]);

        let (where_clause, parameters) =
            exact_lved_search_where("131i", primary_provider(&columns), false)
                .expect("exact filter search");

        assert!(where_clause.contains("select rowid from \"search\" where \"search\" match ?"));
        assert_eq!(parameters, vec!["part:131i", "%∥131i∥%"]);
    }
}

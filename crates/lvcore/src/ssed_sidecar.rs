use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::crypto::{decrypt_logofont_cipher_file_to_path, decrypt_logofont_cipher_prefix};
use crate::diagnostics::Diagnostic;
use crate::error::Result;
use crate::storage::{private_cache_dir, regular_file_inside_root};
use encoding_rs::SHIFT_JIS;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params_from_iter};

const SQLITE_MAGIC: &[u8] = b"SQLite format 3\x00";

mod text;

use text::{normalize_sidecar_search_text, strip_html_tags};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsedSidecarStorage {
    Plain,
    LogoFontCipher,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsedSidecarBodyResolver {
    pub path: PathBuf,
    pub storage: SsedSidecarStorage,
    pub kind: SsedSidecarKind,
    pub table: String,
    pub id_column: String,
    pub id_rule: SsedSidecarIdRule,
    pub title_column: Option<String>,
    pub html_column: Option<String>,
    pub plain_column: Option<String>,
    pub block_column: Option<String>,
    pub offset_column: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsedSidecarMediaResolver {
    pub path: PathBuf,
    pub storage: SsedSidecarStorage,
    pub table: String,
    pub name_column: String,
    pub blob_column: String,
    pub type_column: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsedSidecarRangeResolver {
    pub path: PathBuf,
    pub storage: SsedSidecarStorage,
    pub table: String,
    pub start_block_column: String,
    pub start_offset_column: String,
    pub end_block_column: String,
    pub end_offset_column: String,
    pub title_column: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsedSidecarRangeBound {
    pub end_block: u32,
    pub end_offset: u32,
    pub resolver: SsedSidecarRangeResolver,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsedSidecarMedia {
    pub name: String,
    pub data: Vec<u8>,
    pub media_type: Option<i64>,
    pub resolver: SsedSidecarMediaResolver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsedSidecarKind {
    TContents,
    Honbun,
    MainWordlist,
    AndroidBodyDb,
    GenericBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsedSidecarIdRule {
    DirectColumn,
    RowIdTimesFive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsedSidecarBody {
    pub title: String,
    pub text: String,
    pub html: Option<String>,
    pub resolver: SsedSidecarBodyResolver,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsedSidecarSearchHit {
    pub anchor_id: String,
    pub body: SsedSidecarBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsedSidecarSearchPage {
    pub hits: Vec<SsedSidecarSearchHit>,
    pub matched_count: usize,
    pub exhausted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SsedSidecarLookup {
    Resolved(SsedSidecarBody),
    MissingRow {
        resolver: SsedSidecarBodyResolver,
        query_values: Vec<String>,
        diagnostics: Vec<Diagnostic>,
    },
    NoResolver {
        diagnostics: Vec<Diagnostic>,
    },
}

impl SsedSidecarBodyResolver {
    pub fn source_kind_label(&self) -> &'static str {
        match self.kind {
            SsedSidecarKind::TContents => "t_contents",
            SsedSidecarKind::Honbun => "honbun",
            SsedSidecarKind::MainWordlist => "main_wordlist",
            SsedSidecarKind::AndroidBodyDb => "android_body_db",
            SsedSidecarKind::GenericBody => "sqlite_body",
        }
    }

    fn storage_label(&self) -> &'static str {
        match self.storage {
            SsedSidecarStorage::Plain => "plain",
            SsedSidecarStorage::LogoFontCipher => "logofont_cipher",
        }
    }

    pub fn is_ordered_honbun_renderer_body(&self) -> bool {
        self.kind == SsedSidecarKind::Honbun
            && self.id_rule == SsedSidecarIdRule::DirectColumn
            && self.table.eq_ignore_ascii_case("HONBUN")
            && self.id_column.eq_ignore_ascii_case("ID")
            && self
                .html_column
                .as_deref()
                .is_some_and(|column| column.eq_ignore_ascii_case("Contents_HTML_box"))
    }

    pub fn has_block_offset_body_address(&self) -> bool {
        self.block_column.is_some() && self.offset_column.is_some()
    }
}

impl SsedSidecarIdRule {
    fn label(self) -> &'static str {
        match self {
            Self::DirectColumn => "direct_column",
            Self::RowIdTimesFive => "rowid_times_five",
        }
    }

    fn sql_where_identifier(self, id_column: &str) -> String {
        match self {
            Self::DirectColumn => quote_sql_identifier(id_column),
            Self::RowIdTimesFive => "rowid".to_owned(),
        }
    }
}

pub fn lookup_ssed_dense_sidecar_body(
    root: &Path,
    dict_id_hint: Option<&str>,
    anchor_id: &str,
    resolver_hint: Option<&str>,
) -> Result<SsedSidecarLookup> {
    let resolvers = discover_ssed_sidecar_body_resolvers(root, dict_id_hint)?;
    lookup_ssed_dense_sidecar_body_with_resolvers(&resolvers, anchor_id, resolver_hint)
}

pub fn lookup_ssed_dense_sidecar_body_with_resolvers(
    resolvers: &[SsedSidecarBodyResolver],
    anchor_id: &str,
    resolver_hint: Option<&str>,
) -> Result<SsedSidecarLookup> {
    let candidates = candidate_resolvers(resolvers, resolver_hint);
    if candidates.is_empty() {
        return Ok(SsedSidecarLookup::NoResolver {
            diagnostics: vec![Diagnostic::warning(
                "ssed_dense_sidecar_not_found",
                "dense HONMON anchor was found, but no renderable SQLite sidecar body table was identified",
            )],
        });
    }

    let mut first_missing: Option<SsedSidecarLookup> = None;
    for resolver in &candidates {
        match lookup_resolver_body(resolver, anchor_id)? {
            resolved @ SsedSidecarLookup::Resolved(_) => return Ok(resolved),
            missing @ SsedSidecarLookup::MissingRow { .. } => {
                if first_missing.is_none() {
                    first_missing = Some(missing);
                }
            }
            no_resolver @ SsedSidecarLookup::NoResolver { .. } => {
                if first_missing.is_none() {
                    first_missing = Some(no_resolver);
                }
            }
        }
    }

    if let Some(SsedSidecarLookup::MissingRow {
        resolver,
        query_values,
        mut diagnostics,
    }) = first_missing
    {
        diagnostics.push(
            Diagnostic::info(
                "ssed_dense_sidecar_resolver_exhausted",
                "dense HONMON anchor was not found in any candidate sidecar body table",
            )
            .with_context("resolver_count", candidates.len().to_string()),
        );
        return Ok(SsedSidecarLookup::MissingRow {
            resolver,
            query_values,
            diagnostics,
        });
    }

    Ok(SsedSidecarLookup::NoResolver {
        diagnostics: vec![Diagnostic::warning(
            "ssed_dense_sidecar_not_found",
            "dense HONMON anchor was found, but no renderable SQLite sidecar body table was identified",
        )],
    })
}

pub fn lookup_ssed_ordered_honbun_body_by_row(
    resolvers: &[SsedSidecarBodyResolver],
    row_index: usize,
) -> Result<SsedSidecarLookup> {
    let Some(resolver) = resolvers
        .iter()
        .filter(|resolver| resolver.is_ordered_honbun_renderer_body())
        .min_by_key(|resolver| resolver_priority(resolver))
    else {
        return Ok(SsedSidecarLookup::NoResolver {
            diagnostics: vec![Diagnostic::warning(
                "ssed_ordered_honbun_sidecar_not_found",
                "raw HONMON slot could not be mapped because no ordered HONBUN renderer body table was identified",
            )],
        });
    };
    lookup_ordered_honbun_resolver_body(resolver, row_index)
}

pub fn lookup_ssed_sidecar_body_by_address_with_resolvers(
    resolvers: &[SsedSidecarBodyResolver],
    block: u32,
    offset: u32,
) -> Result<SsedSidecarLookup> {
    const OFFSET_FORWARD_TOLERANCE: u32 = 16;

    let candidates = resolvers
        .iter()
        .filter(|resolver| resolver.has_block_offset_body_address())
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok(SsedSidecarLookup::NoResolver {
            diagnostics: vec![Diagnostic::warning(
                "ssed_address_sidecar_not_found",
                "SSED address could not be mapped because no block/offset sidecar body table was identified",
            )],
        });
    }

    let mut first_missing: Option<SsedSidecarLookup> = None;
    for resolver in &candidates {
        match lookup_block_offset_resolver_body(
            resolver,
            block,
            offset,
            offset.saturating_add(OFFSET_FORWARD_TOLERANCE),
        )? {
            resolved @ SsedSidecarLookup::Resolved(_) => return Ok(resolved),
            missing @ SsedSidecarLookup::MissingRow { .. } => {
                if first_missing.is_none() {
                    first_missing = Some(missing);
                }
            }
            no_resolver @ SsedSidecarLookup::NoResolver { .. } => {
                if first_missing.is_none() {
                    first_missing = Some(no_resolver);
                }
            }
        }
    }

    if let Some(SsedSidecarLookup::MissingRow {
        resolver,
        query_values,
        mut diagnostics,
    }) = first_missing
    {
        diagnostics.push(
            Diagnostic::info(
                "ssed_address_sidecar_resolver_exhausted",
                "SSED address was not found in any candidate block/offset sidecar body table",
            )
            .with_context("resolver_count", candidates.len().to_string()),
        );
        return Ok(SsedSidecarLookup::MissingRow {
            resolver,
            query_values,
            diagnostics,
        });
    }

    Ok(SsedSidecarLookup::NoResolver {
        diagnostics: vec![Diagnostic::warning(
            "ssed_address_sidecar_not_found",
            "SSED address could not be mapped because no block/offset sidecar body table was identified",
        )],
    })
}

pub fn search_ssed_dense_sidecar_bodies_with_resolvers(
    resolvers: &[SsedSidecarBodyResolver],
    query: &str,
    offset: usize,
    limit: usize,
) -> Result<SsedSidecarSearchPage> {
    let needle = normalize_sidecar_search_text(query);
    if needle.is_empty() || limit == 0 {
        return Ok(SsedSidecarSearchPage {
            hits: Vec::new(),
            matched_count: 0,
            exhausted: true,
        });
    }

    if sidecar_sql_prefilter_is_authoritative(query) {
        return search_ssed_dense_sidecar_bodies_prefiltered(
            resolvers, query, &needle, offset, limit,
        );
    }

    if offset == 0 {
        let prefiltered =
            search_ssed_dense_sidecar_bodies_prefiltered(resolvers, query, &needle, 0, limit)?;
        if prefiltered.hits.len() >= limit {
            return Ok(prefiltered);
        }
    }

    let mut hits = Vec::new();
    let mut matched = 0usize;
    for resolver in resolvers {
        let connection = open_sidecar_connection(&resolver.path, resolver.storage)?;
        let mut statement = connection.prepare(&search_sql_for_resolver(resolver))?;
        let rows = statement.query_map([], |row| {
            let anchor_id = anchor_id_from_search_row(resolver, row)?;
            let body = sidecar_body_from_row(resolver, row)?;
            Ok(SsedSidecarSearchHit { anchor_id, body })
        })?;
        for row in rows {
            let hit = row?;
            if !sidecar_search_hit_matches(&hit, &needle) {
                continue;
            }
            if matched < offset {
                matched = matched.saturating_add(1);
                continue;
            }
            hits.push(hit);
            matched = matched.saturating_add(1);
            if hits.len() >= limit {
                return Ok(SsedSidecarSearchPage {
                    hits,
                    matched_count: matched,
                    exhausted: false,
                });
            }
        }
    }
    Ok(SsedSidecarSearchPage {
        hits,
        matched_count: matched,
        exhausted: true,
    })
}

fn search_ssed_dense_sidecar_bodies_prefiltered(
    resolvers: &[SsedSidecarBodyResolver],
    query: &str,
    needle: &str,
    offset: usize,
    limit: usize,
) -> Result<SsedSidecarSearchPage> {
    let pattern = sqlite_like_contains_pattern(query);
    if pattern == "%%" {
        return Ok(SsedSidecarSearchPage {
            hits: Vec::new(),
            matched_count: 0,
            exhausted: true,
        });
    }
    let mut hits = Vec::new();
    let mut matched = 0usize;
    for resolver in resolvers {
        let Some(sql) = search_prefilter_sql_for_resolver(resolver) else {
            continue;
        };
        let connection = open_sidecar_connection(&resolver.path, resolver.storage)?;
        let parameters = vec![pattern.as_str(); sidecar_search_column_count(resolver)];
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(parameters), |row| {
            let anchor_id = anchor_id_from_search_row(resolver, row)?;
            let body = sidecar_body_from_row(resolver, row)?;
            Ok(SsedSidecarSearchHit { anchor_id, body })
        })?;
        for row in rows {
            let hit = row?;
            if !sidecar_search_hit_matches(&hit, needle) {
                continue;
            }
            if matched < offset {
                matched = matched.saturating_add(1);
                continue;
            }
            hits.push(hit);
            matched = matched.saturating_add(1);
            if hits.len() >= limit {
                return Ok(SsedSidecarSearchPage {
                    hits,
                    matched_count: matched,
                    exhausted: false,
                });
            }
        }
    }
    Ok(SsedSidecarSearchPage {
        hits,
        matched_count: matched,
        exhausted: true,
    })
}

fn sidecar_sql_prefilter_is_authoritative(query: &str) -> bool {
    let query = query.trim();
    !query.is_empty()
        && !query.chars().any(char::is_whitespace)
        && !query
            .chars()
            .any(|ch| matches!(ch as u32, 0xff01..=0xff5e | 0x3000))
        && !query.bytes().any(|byte| byte.is_ascii_alphabetic())
}

pub fn discover_ssed_sidecar_body_resolvers(
    root: &Path,
    dict_id_hint: Option<&str>,
) -> Result<Vec<SsedSidecarBodyResolver>> {
    discover_ssed_sidecar_body_resolvers_with_candidates(root, dict_id_hint, &[])
}

pub fn discover_ssed_sidecar_body_resolvers_with_candidates(
    root: &Path,
    dict_id_hint: Option<&str>,
    explicit_candidates: &[PathBuf],
) -> Result<Vec<SsedSidecarBodyResolver>> {
    let mut resolvers = Vec::new();
    for candidate in sidecar_file_candidates(root, dict_id_hint, explicit_candidates)? {
        let Some(storage) = sqlite_storage(&candidate)? else {
            continue;
        };
        let connection = open_sidecar_connection(&candidate, storage)?;
        let tables = sqlite_table_names(&connection)?;
        for table in tables {
            let columns = sqlite_columns(&connection, &table)?;
            let Some(resolver) =
                resolver_for_table(candidate.clone(), storage, &table, &columns, dict_id_hint)
            else {
                continue;
            };
            resolvers.push(resolver);
        }
    }
    resolvers
        .sort_by_key(|resolver| resolver_priority_with_explicit(resolver, explicit_candidates));
    Ok(resolvers)
}

pub fn discover_ssed_sidecar_media_resolvers(
    root: &Path,
    dict_id_hint: Option<&str>,
) -> Result<Vec<SsedSidecarMediaResolver>> {
    let mut resolvers = Vec::new();
    for candidate in sidecar_file_candidates(root, dict_id_hint, &[])? {
        let Some(storage) = sqlite_storage(&candidate)? else {
            continue;
        };
        let connection = open_sidecar_connection(&candidate, storage)?;
        let tables = sqlite_table_names(&connection)?;
        for table in tables {
            let columns = sqlite_columns(&connection, &table)?;
            let Some(resolver) =
                media_resolver_for_table(candidate.clone(), storage, &table, &columns)
            else {
                continue;
            };
            resolvers.push(resolver);
        }
    }
    resolvers.sort_by_key(media_resolver_priority);
    Ok(resolvers)
}

pub fn discover_ssed_sidecar_range_resolvers(
    root: &Path,
    dict_id_hint: Option<&str>,
) -> Result<Vec<SsedSidecarRangeResolver>> {
    discover_ssed_sidecar_range_resolvers_with_candidates(root, dict_id_hint, &[])
}

pub fn discover_ssed_sidecar_range_resolvers_with_candidates(
    root: &Path,
    dict_id_hint: Option<&str>,
    explicit_candidates: &[PathBuf],
) -> Result<Vec<SsedSidecarRangeResolver>> {
    let mut resolvers = Vec::new();
    for candidate in sidecar_file_candidates(root, dict_id_hint, explicit_candidates)? {
        let Some(storage) = sqlite_storage(&candidate)? else {
            continue;
        };
        let connection = open_sidecar_connection(&candidate, storage)?;
        let tables = sqlite_table_names(&connection)?;
        for table in tables {
            let columns = sqlite_columns(&connection, &table)?;
            let Some(resolver) =
                range_resolver_for_table(candidate.clone(), storage, &table, &columns)
            else {
                continue;
            };
            resolvers.push(resolver);
        }
    }
    resolvers.sort_by_key(|resolver| {
        range_resolver_priority_with_explicit(resolver, explicit_candidates)
    });
    Ok(resolvers)
}

pub fn lookup_ssed_sidecar_range_bound_with_resolvers(
    resolvers: &[SsedSidecarRangeResolver],
    block: u32,
    offset: u32,
) -> Result<Option<SsedSidecarRangeBound>> {
    for resolver in resolvers {
        if let Some(bound) = lookup_containing_range_bound(resolver, block, offset)? {
            return Ok(Some(bound));
        }
        if let Some(bound) = lookup_next_range_start_bound(resolver, block, offset)? {
            return Ok(Some(bound));
        }
    }
    Ok(None)
}

pub fn lookup_ssed_sidecar_media(
    resolvers: &[SsedSidecarMediaResolver],
    sidecar_hint: Option<&str>,
    table_hint: Option<&str>,
    media_name: &str,
) -> Result<Option<SsedSidecarMedia>> {
    let query_names = sidecar_media_query_names(media_name);
    for resolver in resolvers
        .iter()
        .filter(|resolver| media_resolver_matches_hint(resolver, sidecar_hint, table_hint))
    {
        let connection = open_sidecar_connection(&resolver.path, resolver.storage)?;
        let mut select_columns = vec![resolver.name_column.clone(), resolver.blob_column.clone()];
        let mut select_expressions = vec![
            quote_sql_identifier(&resolver.name_column),
            quote_sql_identifier(&resolver.blob_column),
        ];
        if let Some(type_column) = &resolver.type_column {
            select_columns.push(type_column.clone());
            select_expressions.push(quote_sql_identifier(type_column));
        }
        let sql = format!(
            "select {} from {} where lower({}) = lower(?) limit 1",
            select_expressions.join(", "),
            quote_sql_identifier(&resolver.table),
            quote_sql_identifier(&resolver.name_column),
        );
        let mut statement = connection.prepare(&sql)?;
        for name in &query_names {
            let row = statement
                .query_row([name.as_str()], |row| {
                    let stored_name = sqlite_value_to_string(row.get_ref(0)?)?;
                    let data = sqlite_value_to_bytes(row.get_ref(1)?)?;
                    let media_type = if resolver.type_column.is_some() {
                        sqlite_value_to_i64(row.get_ref(2)?)
                    } else {
                        None
                    };
                    Ok(SsedSidecarMedia {
                        name: stored_name,
                        data,
                        media_type,
                        resolver: resolver.clone(),
                    })
                })
                .optional()?;
            if let Some(media) = row {
                return Ok(Some(media));
            }
        }
    }
    Ok(None)
}

fn search_sql_for_resolver(resolver: &SsedSidecarBodyResolver) -> String {
    search_select_sql_for_resolver(resolver, None)
}

fn search_prefilter_sql_for_resolver(resolver: &SsedSidecarBodyResolver) -> Option<String> {
    let clauses = sidecar_search_columns(resolver)
        .into_iter()
        .map(|column| format!("{} like ? escape '\\'", quote_sql_identifier(column)))
        .collect::<Vec<_>>();
    if clauses.is_empty() {
        return None;
    }
    Some(search_select_sql_for_resolver(
        resolver,
        Some(&format!(" where {}", clauses.join(" or "))),
    ))
}

fn search_select_sql_for_resolver(
    resolver: &SsedSidecarBodyResolver,
    where_sql: Option<&str>,
) -> String {
    let mut select_columns = vec![resolver.id_column.clone()];
    let mut select_expressions = match resolver.id_rule {
        SsedSidecarIdRule::DirectColumn => vec![quote_sql_identifier(&resolver.id_column)],
        SsedSidecarIdRule::RowIdTimesFive => vec![format!(
            "rowid as {}",
            quote_sql_identifier(&resolver.id_column)
        )],
    };
    for column in [
        resolver.title_column.as_ref(),
        resolver.html_column.as_ref(),
        resolver.plain_column.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if !select_columns.iter().any(|existing| existing == column) {
            select_columns.push(column.clone());
            select_expressions.push(quote_sql_identifier(column));
        }
    }
    let select_sql = select_expressions.join(", ");
    let order_sql = resolver.id_rule.sql_where_identifier(&resolver.id_column);
    let where_sql = where_sql.unwrap_or_default();
    format!(
        "select {select_sql} from {}{where_sql} order by {order_sql}",
        quote_sql_identifier(&resolver.table)
    )
}

fn sidecar_search_columns(resolver: &SsedSidecarBodyResolver) -> Vec<&str> {
    [
        resolver.title_column.as_deref(),
        resolver.html_column.as_deref(),
        resolver.plain_column.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn sidecar_search_column_count(resolver: &SsedSidecarBodyResolver) -> usize {
    sidecar_search_columns(resolver).len()
}

fn sqlite_like_contains_pattern(query: &str) -> String {
    let mut out = String::with_capacity(query.len().saturating_add(2));
    out.push('%');
    for ch in query.trim().chars() {
        match ch {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out.push('%');
    out
}

fn candidate_resolvers<'a>(
    resolvers: &'a [SsedSidecarBodyResolver],
    resolver_hint: Option<&str>,
) -> Vec<&'a SsedSidecarBodyResolver> {
    if let Some(hint) = resolver_hint {
        let hint = hint.casefold();
        let matches = resolvers
            .iter()
            .filter(|resolver| {
                resolver
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().casefold() == hint)
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        if !matches.is_empty() {
            return matches;
        }
    }
    resolvers.iter().collect()
}

fn anchor_id_from_search_row(
    resolver: &SsedSidecarBodyResolver,
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<String> {
    match resolver.id_rule {
        SsedSidecarIdRule::DirectColumn => {
            sqlite_value_to_string(row.get_ref(resolver.id_column.as_str())?)
        }
        SsedSidecarIdRule::RowIdTimesFive => {
            let rowid = match row.get_ref(resolver.id_column.as_str())? {
                ValueRef::Integer(value) => value,
                value => sqlite_value_to_string(value)?.parse::<i64>().unwrap_or(0),
            };
            Ok(rowid.saturating_mul(5).to_string())
        }
    }
}

fn sidecar_search_hit_matches(hit: &SsedSidecarSearchHit, needle: &str) -> bool {
    let mut haystack = String::new();
    haystack.push_str(&hit.body.title);
    haystack.push('\n');
    haystack.push_str(&hit.body.text);
    if let Some(html) = &hit.body.html {
        haystack.push('\n');
        haystack.push_str(&strip_html_tags(html));
    }
    normalize_sidecar_search_text(&haystack).contains(needle)
}

fn lookup_resolver_body(
    resolver: &SsedSidecarBodyResolver,
    anchor_id: &str,
) -> Result<SsedSidecarLookup> {
    let connection = open_sidecar_connection(&resolver.path, resolver.storage)?;
    let (select_sql, _) = body_select_sql_for_resolver(resolver);
    let sql = format!(
        "select {select_sql} from {} where {} = ? limit 1",
        quote_sql_identifier(&resolver.table),
        resolver.id_rule.sql_where_identifier(&resolver.id_column),
    );
    let query_values = anchor_query_values_for_resolver(resolver, anchor_id);
    for value in &query_values {
        let row = match value {
            AnchorQueryValue::Text(value) => connection
                .query_row(&sql, [value.as_str()], |row| {
                    sidecar_body_from_row(resolver, row)
                })
                .optional()?,
            AnchorQueryValue::Integer(value) => connection
                .query_row(&sql, [*value], |row| sidecar_body_from_row(resolver, row))
                .optional()?,
        };
        if let Some(body) = row {
            return Ok(SsedSidecarLookup::Resolved(body));
        }
    }
    let values = query_values
        .into_iter()
        .map(|value| match value {
            AnchorQueryValue::Text(value) => value,
            AnchorQueryValue::Integer(value) => value.to_string(),
        })
        .collect::<Vec<_>>();
    Ok(SsedSidecarLookup::MissingRow {
        resolver: resolver.clone(),
        query_values: values.clone(),
        diagnostics: vec![
            Diagnostic::warning(
                "ssed_dense_sidecar_row_missing",
                "body sidecar did not contain a row for the dense HONMON anchor",
            )
            .with_context("anchor_hash", short_anchor_hash(anchor_id))
            .with_context("query_value_count", values.len().to_string())
            .with_context("sidecar", display_name(&resolver.path))
            .with_context("table", &resolver.table)
            .with_context("id_column", &resolver.id_column),
        ],
    })
}

fn body_select_sql_for_resolver(resolver: &SsedSidecarBodyResolver) -> (String, Vec<String>) {
    let mut select_columns = vec![resolver.id_column.clone()];
    let mut select_expressions = match resolver.id_rule {
        SsedSidecarIdRule::DirectColumn => vec![quote_sql_identifier(&resolver.id_column)],
        SsedSidecarIdRule::RowIdTimesFive => vec![format!(
            "rowid as {}",
            quote_sql_identifier(&resolver.id_column)
        )],
    };
    for column in [
        resolver.title_column.as_ref(),
        resolver.html_column.as_ref(),
        resolver.plain_column.as_ref(),
        resolver.block_column.as_ref(),
        resolver.offset_column.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if !select_columns.iter().any(|existing| existing == column) {
            select_columns.push(column.clone());
            select_expressions.push(quote_sql_identifier(column));
        }
    }
    (select_expressions.join(", "), select_columns)
}

fn lookup_ordered_honbun_resolver_body(
    resolver: &SsedSidecarBodyResolver,
    row_index: usize,
) -> Result<SsedSidecarLookup> {
    let connection = open_sidecar_connection(&resolver.path, resolver.storage)?;
    let mut select_columns = vec![resolver.id_column.clone()];
    let mut select_expressions = vec![quote_sql_identifier(&resolver.id_column)];
    for column in [
        resolver.title_column.as_ref(),
        resolver.html_column.as_ref(),
        resolver.plain_column.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if !select_columns.iter().any(|existing| existing == column) {
            select_columns.push(column.clone());
            select_expressions.push(quote_sql_identifier(column));
        }
    }
    let Ok(row_offset) = i64::try_from(row_index) else {
        return Ok(SsedSidecarLookup::MissingRow {
            resolver: resolver.clone(),
            query_values: vec![row_index.to_string()],
            diagnostics: vec![
                Diagnostic::warning(
                    "ssed_ordered_honbun_row_index_too_large",
                    "raw HONMON slot index is too large for SQLite offset lookup",
                )
                .with_context("row_index", row_index.to_string())
                .with_context("sidecar", display_name(&resolver.path))
                .with_context("table", &resolver.table),
            ],
        });
    };
    let select_sql = select_expressions.join(", ");
    let sql = format!(
        "select {select_sql} from {} order by {} limit 1 offset ?",
        quote_sql_identifier(&resolver.table),
        quote_sql_identifier(&resolver.id_column),
    );
    let row = connection
        .query_row(&sql, [row_offset], |row| {
            sidecar_body_from_row(resolver, row)
        })
        .optional()?;
    if let Some(body) = row {
        return Ok(SsedSidecarLookup::Resolved(body));
    }
    Ok(SsedSidecarLookup::MissingRow {
        resolver: resolver.clone(),
        query_values: vec![row_index.to_string()],
        diagnostics: vec![
            Diagnostic::warning(
                "ssed_ordered_honbun_row_missing",
                "ordered HONBUN renderer sidecar did not contain a row for the raw HONMON slot",
            )
            .with_context("row_index", row_index.to_string())
            .with_context("sidecar", display_name(&resolver.path))
            .with_context("table", &resolver.table)
            .with_context("id_column", &resolver.id_column),
        ],
    })
}

fn lookup_block_offset_resolver_body(
    resolver: &SsedSidecarBodyResolver,
    block: u32,
    offset_start: u32,
    offset_end: u32,
) -> Result<SsedSidecarLookup> {
    let Some(block_column) = &resolver.block_column else {
        return Ok(SsedSidecarLookup::NoResolver {
            diagnostics: vec![Diagnostic::warning(
                "ssed_address_sidecar_block_column_missing",
                "block/offset sidecar resolver is missing a block column",
            )],
        });
    };
    let Some(offset_column) = &resolver.offset_column else {
        return Ok(SsedSidecarLookup::NoResolver {
            diagnostics: vec![Diagnostic::warning(
                "ssed_address_sidecar_offset_column_missing",
                "block/offset sidecar resolver is missing an offset column",
            )],
        });
    };
    let connection = open_sidecar_connection(&resolver.path, resolver.storage)?;
    let (select_sql, _) = body_select_sql_for_resolver(resolver);
    let sql = format!(
        "select {select_sql} from {} where {} = ? and {} >= ? and {} <= ? order by {} asc limit 1",
        quote_sql_identifier(&resolver.table),
        quote_sql_identifier(block_column),
        quote_sql_identifier(offset_column),
        quote_sql_identifier(offset_column),
        quote_sql_identifier(offset_column),
    );
    let row = connection
        .query_row(
            &sql,
            [
                i64::from(block),
                i64::from(offset_start),
                i64::from(offset_end),
            ],
            |row| sidecar_body_from_row(resolver, row),
        )
        .optional()?;
    if let Some(body) = row {
        return Ok(SsedSidecarLookup::Resolved(body));
    }
    Ok(SsedSidecarLookup::MissingRow {
        resolver: resolver.clone(),
        query_values: vec![
            block.to_string(),
            offset_start.to_string(),
            offset_end.to_string(),
        ],
        diagnostics: vec![
            Diagnostic::warning(
                "ssed_address_sidecar_row_missing",
                "block/offset sidecar did not contain a row for the SSED address",
            )
            .with_context("block", block.to_string())
            .with_context("offset_start", offset_start.to_string())
            .with_context("offset_end", offset_end.to_string())
            .with_context("sidecar", display_name(&resolver.path))
            .with_context("table", &resolver.table)
            .with_context("block_column", block_column)
            .with_context("offset_column", offset_column),
        ],
    })
}

fn lookup_containing_range_bound(
    resolver: &SsedSidecarRangeResolver,
    block: u32,
    offset: u32,
) -> Result<Option<SsedSidecarRangeBound>> {
    let connection = open_sidecar_connection(&resolver.path, resolver.storage)?;
    let select_sql = range_select_sql_for_resolver(resolver);
    let sql = format!(
        "select {select_sql} from {} where ({} < ? or ({} = ? and {} <= ?)) and ({} > ? or ({} = ? and {} > ?)) order by {} desc, {} desc limit 1",
        quote_sql_identifier(&resolver.table),
        quote_sql_identifier(&resolver.start_block_column),
        quote_sql_identifier(&resolver.start_block_column),
        quote_sql_identifier(&resolver.start_offset_column),
        quote_sql_identifier(&resolver.end_block_column),
        quote_sql_identifier(&resolver.end_block_column),
        quote_sql_identifier(&resolver.end_offset_column),
        quote_sql_identifier(&resolver.start_block_column),
        quote_sql_identifier(&resolver.start_offset_column),
    );
    Ok(connection
        .query_row(
            &sql,
            [
                i64::from(block),
                i64::from(block),
                i64::from(offset),
                i64::from(block),
                i64::from(block),
                i64::from(offset),
            ],
            |row| range_bound_from_row(resolver, row),
        )
        .optional()?)
}

fn lookup_next_range_start_bound(
    resolver: &SsedSidecarRangeResolver,
    block: u32,
    offset: u32,
) -> Result<Option<SsedSidecarRangeBound>> {
    let connection = open_sidecar_connection(&resolver.path, resolver.storage)?;
    let select_sql = range_select_sql_for_resolver(resolver);
    let sql = format!(
        "select {select_sql} from {} where {} > ? or ({} = ? and {} > ?) order by {} asc, {} asc limit 1",
        quote_sql_identifier(&resolver.table),
        quote_sql_identifier(&resolver.start_block_column),
        quote_sql_identifier(&resolver.start_block_column),
        quote_sql_identifier(&resolver.start_offset_column),
        quote_sql_identifier(&resolver.start_block_column),
        quote_sql_identifier(&resolver.start_offset_column),
    );
    Ok(connection
        .query_row(
            &sql,
            [i64::from(block), i64::from(block), i64::from(offset)],
            |row| {
                let end_block = sqlite_value_to_u32(row.get_ref("lvcore_range_start_block")?)?;
                let end_offset = sqlite_value_to_u32(row.get_ref("lvcore_range_start_offset")?)?;
                Ok(SsedSidecarRangeBound {
                    end_block,
                    end_offset,
                    resolver: resolver.clone(),
                })
            },
        )
        .optional()?)
}

fn range_select_sql_for_resolver(resolver: &SsedSidecarRangeResolver) -> String {
    let mut expressions = vec![
        format!(
            "{} as {}",
            quote_sql_identifier(&resolver.start_block_column),
            quote_sql_identifier("lvcore_range_start_block")
        ),
        format!(
            "{} as {}",
            quote_sql_identifier(&resolver.start_offset_column),
            quote_sql_identifier("lvcore_range_start_offset")
        ),
        format!(
            "{} as {}",
            quote_sql_identifier(&resolver.end_block_column),
            quote_sql_identifier("lvcore_range_end_block")
        ),
        format!(
            "{} as {}",
            quote_sql_identifier(&resolver.end_offset_column),
            quote_sql_identifier("lvcore_range_end_offset")
        ),
    ];
    if let Some(title_column) = &resolver.title_column {
        expressions.push(quote_sql_identifier(title_column));
    }
    expressions.join(", ")
}

fn range_bound_from_row(
    resolver: &SsedSidecarRangeResolver,
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<SsedSidecarRangeBound> {
    Ok(SsedSidecarRangeBound {
        end_block: sqlite_value_to_u32(row.get_ref("lvcore_range_end_block")?)?,
        end_offset: sqlite_value_to_u32(row.get_ref("lvcore_range_end_offset")?)?,
        resolver: resolver.clone(),
    })
}

fn short_anchor_hash(anchor_id: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(anchor_id.as_bytes());
    hex::encode(digest)[..12].to_owned()
}

fn sidecar_body_from_row(
    resolver: &SsedSidecarBodyResolver,
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<SsedSidecarBody> {
    let title = match &resolver.title_column {
        Some(column) => sqlite_value_to_string(row.get_ref(column.as_str())?)?,
        None => String::new(),
    };
    let html = match &resolver.html_column {
        Some(column) => sqlite_value_to_string(row.get_ref(column.as_str())?)?,
        None => String::new(),
    };
    let plain = match &resolver.plain_column {
        Some(column) => sqlite_value_to_string(row.get_ref(column.as_str())?)?,
        None => String::new(),
    };
    let text = if !plain.trim().is_empty() {
        plain.trim().to_owned()
    } else if !html.trim().is_empty() {
        strip_html_tags(&html)
    } else {
        strip_html_tags(&title)
    };
    Ok(SsedSidecarBody {
        title: strip_html_tags(&title),
        text,
        html: (!html.is_empty()).then_some(html),
        resolver: resolver.clone(),
        diagnostics: vec![
            Diagnostic::info(
                "ssed_dense_sidecar_body_resolved",
                "entry body resolved from SSED sidecar database",
            )
            .with_context("sidecar", display_name(&resolver.path))
            .with_context("sidecar_kind", resolver.source_kind_label())
            .with_context("storage", resolver.storage_label())
            .with_context("id_rule", resolver.id_rule.label())
            .with_context("table", &resolver.table)
            .with_context("id_column", &resolver.id_column),
        ],
    })
}

fn resolver_for_table(
    path: PathBuf,
    storage: SsedSidecarStorage,
    table: &str,
    columns: &[String],
    dict_id_hint: Option<&str>,
) -> Option<SsedSidecarBodyResolver> {
    let html_column = find_column(columns, HTML_COLUMN_ALIASES);
    let plain_column = find_column(columns, PLAIN_COLUMN_ALIASES);
    if html_column.is_none() && plain_column.is_none() {
        return None;
    }
    let direct_id_column = find_column(columns, ID_COLUMN_ALIASES);
    let (kind, id_column, id_rule) = if let Some(id_column) = direct_id_column {
        (
            sidecar_kind_for_table(table),
            id_column,
            SsedSidecarIdRule::DirectColumn,
        )
    } else if is_android_rowid_body_table(&path, table, columns, dict_id_hint) {
        (
            SsedSidecarKind::AndroidBodyDb,
            "__lvcore_rowid".to_owned(),
            SsedSidecarIdRule::RowIdTimesFive,
        )
    } else {
        return None;
    };
    Some(SsedSidecarBodyResolver {
        path,
        storage,
        kind,
        table: table.to_owned(),
        id_column,
        id_rule,
        title_column: find_column(columns, TITLE_COLUMN_ALIASES),
        html_column,
        plain_column,
        block_column: find_column(columns, BLOCK_COLUMN_ALIASES),
        offset_column: find_column(columns, OFFSET_COLUMN_ALIASES),
    })
}

fn media_resolver_for_table(
    path: PathBuf,
    storage: SsedSidecarStorage,
    table: &str,
    columns: &[String],
) -> Option<SsedSidecarMediaResolver> {
    let table_name = table.casefold();
    if table_name != "media" && !table_name.contains("media") {
        return None;
    }
    let name_column = find_column(columns, MEDIA_NAME_COLUMN_ALIASES)?;
    let blob_column = find_column(columns, MEDIA_BLOB_COLUMN_ALIASES)?;
    Some(SsedSidecarMediaResolver {
        path,
        storage,
        table: table.to_owned(),
        name_column,
        blob_column,
        type_column: find_column(columns, MEDIA_TYPE_COLUMN_ALIASES),
    })
}

fn range_resolver_for_table(
    path: PathBuf,
    storage: SsedSidecarStorage,
    table: &str,
    columns: &[String],
) -> Option<SsedSidecarRangeResolver> {
    Some(SsedSidecarRangeResolver {
        path,
        storage,
        table: table.to_owned(),
        start_block_column: find_column(columns, START_BLOCK_COLUMN_ALIASES)?,
        start_offset_column: find_column(columns, START_OFFSET_COLUMN_ALIASES)?,
        end_block_column: find_column(columns, END_BLOCK_COLUMN_ALIASES)?,
        end_offset_column: find_column(columns, END_OFFSET_COLUMN_ALIASES)?,
        title_column: find_column(columns, TITLE_COLUMN_ALIASES),
    })
}

fn sidecar_kind_for_table(table: &str) -> SsedSidecarKind {
    let table = table.casefold();
    if table == "t_contents" || table.starts_with("t_contents_") {
        SsedSidecarKind::TContents
    } else if table == "honbun" {
        SsedSidecarKind::Honbun
    } else if table == "main" {
        SsedSidecarKind::MainWordlist
    } else {
        SsedSidecarKind::GenericBody
    }
}

fn resolver_priority(resolver: &SsedSidecarBodyResolver) -> (u8, u8, String, String) {
    let file = display_name(&resolver.path).casefold();
    let sidecar_priority = if file.starts_with("vlpljbl") {
        let suffix = file.trim_start_matches("vlpljbl");
        match suffix {
            "f" | "b" | "h" => 1,
            "m" | "n" | "s" => 4,
            _ => 2,
        }
    } else if resolver.path.extension().is_some() {
        3
    } else {
        0
    };
    let kind_priority = match resolver.kind {
        SsedSidecarKind::TContents => 0,
        SsedSidecarKind::Honbun => 1,
        SsedSidecarKind::MainWordlist => 2,
        SsedSidecarKind::AndroidBodyDb => 2,
        SsedSidecarKind::GenericBody => 3,
    };
    (
        sidecar_priority,
        kind_priority,
        resolver.table.casefold(),
        file,
    )
}

fn resolver_priority_with_explicit(
    resolver: &SsedSidecarBodyResolver,
    explicit_candidates: &[PathBuf],
) -> (u8, u8, u8, String, String) {
    let explicit_priority = if explicit_candidates
        .iter()
        .any(|candidate| candidate == &resolver.path)
    {
        0
    } else {
        1
    };
    let (sidecar_priority, kind_priority, table, file) = resolver_priority(resolver);
    (
        explicit_priority,
        sidecar_priority,
        kind_priority,
        table,
        file,
    )
}

fn media_resolver_priority(resolver: &SsedSidecarMediaResolver) -> (u8, String, String) {
    let file = display_name(&resolver.path).casefold();
    let table_priority = if resolver.table.eq_ignore_ascii_case("media") {
        0
    } else {
        1
    };
    (table_priority, resolver.table.casefold(), file)
}

fn range_resolver_priority(resolver: &SsedSidecarRangeResolver) -> (u8, String, String) {
    let file = display_name(&resolver.path).casefold();
    let table_priority = if resolver.table.eq_ignore_ascii_case(
        resolver
            .path
            .file_stem()
            .map(|stem| stem.to_string_lossy())
            .unwrap_or_default()
            .as_ref(),
    ) {
        0
    } else {
        1
    };
    (table_priority, resolver.table.casefold(), file)
}

fn range_resolver_priority_with_explicit(
    resolver: &SsedSidecarRangeResolver,
    explicit_candidates: &[PathBuf],
) -> (u8, u8, String, String) {
    let explicit_priority = if explicit_candidates
        .iter()
        .any(|candidate| candidate == &resolver.path)
    {
        0
    } else {
        1
    };
    let (table_priority, table, file) = range_resolver_priority(resolver);
    (explicit_priority, table_priority, table, file)
}

fn media_resolver_matches_hint(
    resolver: &SsedSidecarMediaResolver,
    sidecar_hint: Option<&str>,
    table_hint: Option<&str>,
) -> bool {
    if let Some(sidecar_hint) = sidecar_hint {
        let Some(name) = resolver.path.file_name().map(|name| name.to_string_lossy()) else {
            return false;
        };
        if !name.eq_ignore_ascii_case(sidecar_hint) {
            return false;
        }
    }
    if let Some(table_hint) = table_hint
        && !resolver.table.eq_ignore_ascii_case(table_hint)
    {
        return false;
    }
    true
}

fn sidecar_media_query_names(media_name: &str) -> Vec<String> {
    let mut names = Vec::new();
    let normalized = media_name.replace('\\', "/");
    let filename = normalized.rsplit('/').next().unwrap_or(normalized.as_str());
    for candidate in [normalized.as_str(), filename] {
        let candidate = candidate.trim();
        if !candidate.is_empty() {
            push_unique(&mut names, candidate.to_owned());
            let stem = Path::new(candidate)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .trim();
            if !stem.is_empty() && stem != candidate {
                push_unique(&mut names, stem.to_owned());
            }
        }
    }
    names
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&value))
    {
        values.push(value);
    }
}

fn is_android_rowid_body_table(
    path: &Path,
    table: &str,
    columns: &[String],
    dict_id_hint: Option<&str>,
) -> bool {
    if find_column(columns, HTML_COLUMN_ALIASES).is_none() {
        return false;
    }
    if dict_id_hint.is_some_and(|dict_id| table.eq_ignore_ascii_case(dict_id)) {
        return true;
    }
    path.file_stem()
        .map(|stem| table.eq_ignore_ascii_case(&stem.to_string_lossy()))
        .unwrap_or(false)
}

fn sidecar_file_candidates(
    root: &Path,
    dict_id_hint: Option<&str>,
    explicit_candidates: &[PathBuf],
) -> Result<Vec<PathBuf>> {
    let mut candidates = Vec::new();
    if !root.is_dir() {
        return Ok(candidates);
    }
    for path in explicit_candidates {
        if regular_file_inside_root(root, path)?
            && !is_metadata_noise_path(path)
            && path.is_file()
            && !candidates
                .iter()
                .any(|candidate: &PathBuf| candidate == path)
        {
            candidates.push(path.clone());
        }
    }
    let dict_id = dict_id_hint.map(str::casefold);
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if !regular_file_inside_root(root, &path)? || is_metadata_noise_path(&path) {
            continue;
        }
        let Some(name) = path.file_name().map(|name| name.to_string_lossy()) else {
            continue;
        };
        let lower = name.to_lowercase();
        let suffix = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let is_dict_id_payload = dict_id
            .as_ref()
            .is_some_and(|dict_id| suffix.is_empty() && lower.casefold() == *dict_id);
        if lower == "vlpljbl.bin" {
            continue;
        }
        if (lower.starts_with("vlpljbl")
            || matches!(suffix.as_str(), "db" | "sqlite" | "sqlite3" | "sql")
            || is_dict_id_payload)
            && !candidates.iter().any(|candidate| candidate == &path)
        {
            candidates.push(path);
        }
    }
    candidates.sort_by(|a, b| {
        candidate_priority(a, dict_id_hint, explicit_candidates).cmp(&candidate_priority(
            b,
            dict_id_hint,
            explicit_candidates,
        ))
    });
    Ok(candidates)
}

fn candidate_priority(
    path: &Path,
    dict_id_hint: Option<&str>,
    explicit_candidates: &[PathBuf],
) -> (u8, String) {
    let name = display_name(path).casefold();
    if explicit_candidates
        .iter()
        .any(|candidate| candidate == path)
    {
        return (0, name);
    }
    if dict_id_hint.is_some_and(|dict_id| name == dict_id.casefold()) {
        return (1, name);
    }
    if name.starts_with("vlpljbl") {
        let suffix = name.trim_start_matches("vlpljbl");
        if matches!(suffix, "f" | "b" | "h") {
            return (2, name);
        }
        if matches!(suffix, "m" | "n" | "s") {
            return (5, name);
        }
        return (3, name);
    }
    if path.extension().is_some() {
        return (4, name);
    }
    (6, name)
}

fn sqlite_storage(path: &Path) -> Result<Option<SsedSidecarStorage>> {
    let mut file = File::open(path)?;
    let mut prefix = vec![0_u8; 2048];
    let read = file.read(&mut prefix)?;
    prefix.truncate(read);
    if prefix.starts_with(SQLITE_MAGIC) {
        return Ok(Some(SsedSidecarStorage::Plain));
    }
    if let Ok(decrypted) = decrypt_logofont_cipher_prefix(&prefix, 64)
        && decrypted.starts_with(SQLITE_MAGIC)
    {
        return Ok(Some(SsedSidecarStorage::LogoFontCipher));
    }
    Ok(None)
}

fn open_sidecar_connection(path: &Path, storage: SsedSidecarStorage) -> Result<Connection> {
    match storage {
        SsedSidecarStorage::Plain => open_readonly_sqlite(path),
        SsedSidecarStorage::LogoFontCipher => {
            let cache_path = decrypted_sidecar_cache_path(path)?;
            if !cache_path.exists() {
                if let Some(parent) = cache_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let tmp = cache_path.with_extension("tmp");
                decrypt_logofont_cipher_file_to_path(path, &tmp)?;
                fs::rename(tmp, &cache_path)?;
            }
            open_readonly_sqlite(&cache_path)
        }
    }
}

fn decrypted_sidecar_cache_path(path: &Path) -> Result<PathBuf> {
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    hasher.update(path.to_string_lossy().as_bytes());
    if let Ok(metadata) = fs::metadata(path) {
        hasher.update(metadata.len().to_le_bytes());
        if let Ok(modified) = metadata.modified()
            && let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH)
        {
            hasher.update(duration.as_secs().to_le_bytes());
            hasher.update(duration.subsec_nanos().to_le_bytes());
        }
    }
    let digest = hex::encode(hasher.finalize());
    let stem = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "sidecar".into());
    Ok(private_cache_dir("ssed-sidecars")?.join(format!("{stem}-{digest}.sqlite")))
}

fn open_readonly_sqlite(path: &Path) -> Result<Connection> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

fn sqlite_table_names(connection: &Connection) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare("select name from sqlite_master where type in ('table', 'view') order by name")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut tables = Vec::new();
    for row in rows {
        tables.push(row?);
    }
    Ok(tables)
}

fn sqlite_columns(connection: &Connection, table: &str) -> Result<Vec<String>> {
    let mut statement = connection.prepare(&format!(
        "pragma table_info({})",
        quote_sql_identifier(table)
    ))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = Vec::new();
    for row in rows {
        columns.push(row?);
    }
    Ok(columns)
}

fn sqlite_value_to_string(value: ValueRef<'_>) -> rusqlite::Result<String> {
    match value {
        ValueRef::Null => Ok(String::new()),
        ValueRef::Integer(value) => Ok(value.to_string()),
        ValueRef::Real(value) => Ok(value.to_string()),
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => Ok(decode_sqlite_text(bytes)),
    }
}

fn sqlite_value_to_bytes(value: ValueRef<'_>) -> rusqlite::Result<Vec<u8>> {
    match value {
        ValueRef::Null => Ok(Vec::new()),
        ValueRef::Integer(value) => Ok(value.to_string().into_bytes()),
        ValueRef::Real(value) => Ok(value.to_string().into_bytes()),
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => Ok(bytes.to_vec()),
    }
}

fn sqlite_value_to_i64(value: ValueRef<'_>) -> Option<i64> {
    match value {
        ValueRef::Integer(value) => Some(value),
        ValueRef::Real(value) => Some(value as i64),
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => std::str::from_utf8(bytes)
            .ok()
            .and_then(|value| value.trim().parse().ok()),
        ValueRef::Null => None,
    }
}

fn sqlite_value_to_u32(value: ValueRef<'_>) -> rusqlite::Result<u32> {
    let value = match value {
        ValueRef::Integer(value) => value,
        ValueRef::Real(value) => value as i64,
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => std::str::from_utf8(bytes)
            .ok()
            .and_then(|value| value.trim().parse::<i64>().ok())
            .unwrap_or(0),
        ValueRef::Null => 0,
    };
    Ok(u32::try_from(value).unwrap_or(0))
}

fn decode_sqlite_text(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    let (decoded, _encoding, _had_errors) = SHIFT_JIS.decode(bytes);
    decoded.into_owned()
}

fn quote_sql_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn find_column(columns: &[String], aliases: &[&str]) -> Option<String> {
    aliases.iter().find_map(|alias| {
        columns
            .iter()
            .find(|column| column.eq_ignore_ascii_case(alias))
            .cloned()
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AnchorQueryValue {
    Text(String),
    Integer(i64),
}

fn anchor_query_values(anchor_id: &str) -> Vec<AnchorQueryValue> {
    let mut values = vec![AnchorQueryValue::Text(anchor_id.to_owned())];
    let stripped = anchor_id.trim_start_matches('0');
    let stripped = if stripped.is_empty() { "0" } else { stripped };
    if stripped != anchor_id {
        values.push(AnchorQueryValue::Text(stripped.to_owned()));
    }
    if let Ok(value) = stripped.parse::<i64>() {
        values.push(AnchorQueryValue::Integer(value));
    }
    values.dedup();
    values
}

fn anchor_query_values_for_resolver(
    resolver: &SsedSidecarBodyResolver,
    anchor_id: &str,
) -> Vec<AnchorQueryValue> {
    match resolver.id_rule {
        SsedSidecarIdRule::DirectColumn => anchor_query_values(anchor_id),
        SsedSidecarIdRule::RowIdTimesFive => {
            let stripped = anchor_id.trim_start_matches('0');
            let stripped = if stripped.is_empty() { "0" } else { stripped };
            let Ok(value) = stripped.parse::<i64>() else {
                return Vec::new();
            };
            if value <= 0 || value % 5 != 0 {
                return Vec::new();
            }
            vec![AnchorQueryValue::Integer(value / 5)]
        }
    }
}

fn is_metadata_noise_path(path: &Path) -> bool {
    path.file_name()
        .map(|name| {
            let name = name.to_string_lossy();
            name.starts_with("._") || name == ".DS_Store" || name.ends_with('~')
        })
        .unwrap_or(false)
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

trait Casefold {
    fn casefold(&self) -> String;
}

impl Casefold for str {
    fn casefold(&self) -> String {
        self.to_lowercase()
    }
}

impl Casefold for String {
    fn casefold(&self) -> String {
        self.as_str().casefold()
    }
}

const ID_COLUMN_ALIASES: &[&str] = &[
    "ID",
    "No",
    "ItemID",
    "ItemId",
    "DataID",
    "DataId",
    "ContentID",
    "ContentId",
    "contents_id",
    "content_id",
    "row_id",
    "f_DataId",
    "f_dataid",
    "f_data_id",
    "f_array_no",
    "f_contents_id",
    "f_order_id",
    "id",
    "index",
];

const TITLE_COLUMN_ALIASES: &[&str] = &[
    "Title",
    "Heading",
    "Headword",
    "Label",
    "TitleJIS",
    "JIS_Title",
    "Title_UTF8",
    "Title_SJIS",
    "f_Title",
    "f_title",
    "Keyword",
    "Midashi",
    "MidashiJ",
    "f_midasi",
    "f_midashi",
    "f_midashi_hyoki",
    "f_midashi_key",
    "f_abbr",
    "f_fullname",
    "C_text",
    "K_text",
    "J_text",
];

const HTML_COLUMN_ALIASES: &[&str] = &[
    "HTML",
    "Html",
    "body_html",
    "html_body",
    "content_html",
    "Contents_HTML_box",
    "Contents_HTML_list",
    "f_Html",
    "f_html",
    "f_html_text",
    "f_contents",
];

const PLAIN_COLUMN_ALIASES: &[&str] = &[
    "Text",
    "Plain",
    "Body",
    "body_text",
    "plain_text",
    "content_text",
    "f_body",
    "f_Plane",
    "f_plane",
    "f_plain",
    "f_plane_text",
    "h_text",
    "Value",
    "J_text",
    "C_text",
    "K_text",
    "Pinyin",
    "data",
];

const BLOCK_COLUMN_ALIASES: &[&str] = &[
    "Block",
    "block",
    "BodyBlock",
    "body_block",
    "f_Block",
    "f_block",
];

const OFFSET_COLUMN_ALIASES: &[&str] = &[
    "Offset",
    "offset",
    "BodyOffset",
    "body_offset",
    "f_Offset",
    "f_offset",
];

const START_BLOCK_COLUMN_ALIASES: &[&str] = &[
    "Block_s",
    "block_s",
    "StartBlock",
    "start_block",
    "BlockStart",
    "block_start",
];

const START_OFFSET_COLUMN_ALIASES: &[&str] = &[
    "Offset_s",
    "offset_s",
    "StartOffset",
    "start_offset",
    "OffsetStart",
    "offset_start",
];

const END_BLOCK_COLUMN_ALIASES: &[&str] = &[
    "Block_e",
    "block_e",
    "EndBlock",
    "end_block",
    "BlockEnd",
    "block_end",
];

const END_OFFSET_COLUMN_ALIASES: &[&str] = &[
    "Offset_e",
    "offset_e",
    "EndOffset",
    "end_offset",
    "OffsetEnd",
    "offset_end",
];

const MEDIA_NAME_COLUMN_ALIASES: &[&str] = &[
    "name",
    "Name",
    "f_name",
    "file",
    "File",
    "filename",
    "file_name",
    "path",
    "Path",
];

const MEDIA_BLOB_COLUMN_ALIASES: &[&str] = &[
    "main", "Main", "f_main", "blob", "Blob", "f_blob", "data", "Data",
];

const MEDIA_TYPE_COLUMN_ALIASES: &[&str] = &["type", "Type", "f_type", "media_type"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_sql_prefilter_is_authoritative_for_plain_japanese_needles() {
        assert!(sidecar_sql_prefilter_is_authoritative("白水"));
        assert!(sidecar_sql_prefilter_is_authoritative("ブリ"));
    }

    #[test]
    fn sidecar_sql_prefilter_is_not_authoritative_when_rust_normalization_matters() {
        assert!(!sidecar_sql_prefilter_is_authoritative("fulltext"));
        assert!(!sidecar_sql_prefilter_is_authoritative("ＦＵＬＬＴＥＸＴ"));
        assert!(!sidecar_sql_prefilter_is_authoritative("白 水"));
        assert!(!sidecar_sql_prefilter_is_authoritative("　"));
    }
}

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::crypto::{decrypt_logofont_cipher_file_to_path, decrypt_logofont_cipher_prefix};
use crate::diagnostics::Diagnostic;
use crate::error::Result;
use crate::storage::private_cache_dir;
use encoding_rs::SHIFT_JIS;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension};

const SQLITE_MAGIC: &[u8] = b"SQLite format 3\x00";

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

pub fn discover_ssed_sidecar_body_resolvers(
    root: &Path,
    dict_id_hint: Option<&str>,
) -> Result<Vec<SsedSidecarBodyResolver>> {
    let mut resolvers = Vec::new();
    for candidate in sidecar_file_candidates(root, dict_id_hint)? {
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
    resolvers.sort_by_key(resolver_priority);
    Ok(resolvers)
}

fn search_sql_for_resolver(resolver: &SsedSidecarBodyResolver) -> String {
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
    format!(
        "select {select_sql} from {} order by {order_sql}",
        quote_sql_identifier(&resolver.table)
    )
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

fn sidecar_file_candidates(root: &Path, dict_id_hint: Option<&str>) -> Result<Vec<PathBuf>> {
    let mut candidates = Vec::new();
    if !root.is_dir() {
        return Ok(candidates);
    }
    let dict_id = dict_id_hint.map(str::casefold);
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if !path.is_file() || is_metadata_noise_path(&path) {
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
        if lower.starts_with("vlpljbl")
            || matches!(suffix.as_str(), "db" | "sqlite" | "sqlite3" | "sql")
            || is_dict_id_payload
        {
            candidates.push(path);
        }
    }
    candidates.sort_by(|a, b| {
        candidate_priority(a, dict_id_hint).cmp(&candidate_priority(b, dict_id_hint))
    });
    Ok(candidates)
}

fn candidate_priority(path: &Path, dict_id_hint: Option<&str>) -> (u8, String) {
    let name = display_name(path).casefold();
    if dict_id_hint.is_some_and(|dict_id| name == dict_id.casefold()) {
        return (0, name);
    }
    if name.starts_with("vlpljbl") {
        let suffix = name.trim_start_matches("vlpljbl");
        if matches!(suffix, "f" | "b" | "h") {
            return (1, name);
        }
        if matches!(suffix, "m" | "n" | "s") {
            return (4, name);
        }
        return (2, name);
    }
    if path.extension().is_some() {
        return (3, name);
    }
    (5, name)
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

fn strip_html_tags(value: &str) -> String {
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

fn normalize_sidecar_search_text(value: &str) -> String {
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

use super::*;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, params_from_iter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SsedIosSearchResolver {
    path: PathBuf,
    source: SsedIosSearchResolverSource,
    table: String,
    block_column: String,
    offset_column: String,
    search_columns: Vec<String>,
    label_columns: Vec<String>,
    mode_hints: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SsedIosSearchResolverSource {
    DictSearchDb,
    DictFullDb,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SsedIosSearchRow {
    block: u32,
    offset: u32,
    label: String,
    snippet: String,
}

impl SsedIosSearchResolverSource {
    fn sort_key(self) -> u8 {
        match self {
            Self::DictSearchDb => 0,
            Self::DictFullDb => 1,
        }
    }

    fn diagnostic_code(self) -> &'static str {
        match self {
            Self::DictSearchDb => "ssed_ios_dictsearchdb_scan",
            Self::DictFullDb => "ssed_ios_fulldb_search_scan",
        }
    }

    fn diagnostic_message(self) -> &'static str {
        match self {
            Self::DictSearchDb => {
                "SSED search included an iOS DictSearchDB block/offset helper table"
            }
            Self::DictFullDb => "SSED search included an iOS DictFULLDB block/offset body table",
        }
    }
}

impl ReaderBookPackage {
    pub(super) fn search_ssed_ios_search_dbs(&self, query: &SearchQuery) -> Result<SearchPage> {
        if query.limit == 0 {
            return Ok(SearchPage {
                hits: Vec::new(),
                next_cursor: None,
                result_sequence: None,
                diagnostics: Vec::new(),
            });
        }
        let resolvers = self.ssed_ios_search_resolvers()?;
        let mode_resolvers = resolvers
            .iter()
            .filter(|resolver| resolver_applies_to_mode(resolver, &query.mode))
            .collect::<Vec<_>>();
        if mode_resolvers.is_empty() {
            return Ok(SearchPage::deferred(
                "iOS advanced search has no implemented table for this SSED search mode",
            ));
        }
        let has_search_db_resolver = mode_resolvers
            .iter()
            .any(|resolver| resolver.source == SsedIosSearchResolverSource::DictSearchDb);
        let mode_resolvers = mode_resolvers
            .into_iter()
            .filter(|resolver| {
                !has_search_db_resolver
                    || resolver.source == SsedIosSearchResolverSource::DictSearchDb
            })
            .collect::<Vec<_>>();
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(SearchPage {
                hits: Vec::new(),
                next_cursor: None,
                result_sequence: None,
                diagnostics: vec![Diagnostic::error(
                    "ssed_catalog_missing",
                    "iOS DictSearchDB search requires a parsed SSEDINFO catalog",
                )],
            });
        };
        if catalog.honmon().is_none() {
            return Ok(SearchPage {
                hits: Vec::new(),
                next_cursor: None,
                result_sequence: None,
                diagnostics: vec![Diagnostic::error(
                    "ssed_honmon_missing",
                    "iOS DictSearchDB rows point to SSED HONMON block/offset addresses",
                )],
            });
        }
        let page_offset = decode_offset_cursor(query.cursor.as_deref());
        let page_limit = query.limit.saturating_add(1);
        let label_policy = query.label_gaiji_policy();
        let mut hits = Vec::new();
        let mut matched = 0usize;
        let mut diagnostics = Vec::new();
        for resolver in mode_resolvers {
            let rows = search_ios_resolver(resolver, &query.query, page_offset, page_limit)?;
            if rows.is_empty() {
                continue;
            }
            diagnostics.push(
                Diagnostic::info(
                    resolver.source.diagnostic_code(),
                    resolver.source.diagnostic_message(),
                )
                .with_context("sidecar", display_name(&resolver.path))
                .with_context("table", &resolver.table),
            );
            for row in rows {
                if matched < page_offset {
                    matched = matched.saturating_add(1);
                    continue;
                }
                let label = self.ssed_rich_label_with_policy(&row.label, &label_policy);
                let (block, offset) = self.convert_ios_ssed_address(row.block, row.offset)?;
                let Some(target) =
                    self.ssed_target_for_loose_address(block, offset, &mut diagnostics)?
                else {
                    continue;
                };
                let href = target.href();
                let snippet_html = (!row.snippet.trim().is_empty())
                    .then(|| escape_plain_label_html(row.snippet.trim()));
                hits.push(SearchHit {
                    href,
                    book_id: self.book_id_for_hit(),
                    target,
                    title_html: label.html,
                    title_text: label.text,
                    snippet_html,
                    sequence_hint: None,
                    diagnostics: label.diagnostics,
                });
                matched = matched.saturating_add(1);
                if hits.len() >= page_limit {
                    break;
                }
            }
            if hits.len() >= page_limit {
                break;
            }
        }
        let next_cursor =
            (hits.len() > query.limit).then(|| (page_offset + query.limit).to_string());
        hits.truncate(query.limit);
        Ok(SearchPage {
            hits,
            next_cursor,
            result_sequence: None,
            diagnostics,
        })
    }

    pub(super) fn ssed_ios_search_resolvers(&self) -> Result<&[SsedIosSearchResolver]> {
        let resolvers = self.ssed_ios_search_resolvers.get_or_init(|| {
            discover_ios_search_resolvers(
                &self.retained_ios_search_payloads,
                &self.retained_ios_full_db_payloads,
            )
            .map_err(|error| error.to_string())
        });
        match resolvers {
            Ok(resolvers) => Ok(resolvers.as_slice()),
            Err(error) => Err(Error::Driver(error.clone())),
        }
    }
}

fn discover_ios_search_resolvers(
    payloads: &[IosDictSearchPayload],
    full_db_payloads: &[IosDictFullDbPayload],
) -> Result<Vec<SsedIosSearchResolver>> {
    let mut resolvers = Vec::new();
    for payload in payloads {
        if !payload.absolute_path.is_file() {
            continue;
        }
        let connection = open_ios_search_connection(&payload.absolute_path)?;
        for table in sqlite_table_names(&connection)? {
            let columns = sqlite_columns(&connection, &table)?;
            let Some(resolver) = resolver_for_ios_search_table(
                payload.absolute_path.clone(),
                SsedIosSearchResolverSource::DictSearchDb,
                &table,
                &columns,
            ) else {
                continue;
            };
            resolvers.push(resolver);
        }
    }
    for payload in full_db_payloads {
        if !payload.absolute_path.is_file() {
            continue;
        }
        let connection = open_ios_search_connection(&payload.absolute_path)?;
        for table in sqlite_table_names(&connection)? {
            let columns = sqlite_columns(&connection, &table)?;
            let Some(resolver) = resolver_for_ios_search_table(
                payload.absolute_path.clone(),
                SsedIosSearchResolverSource::DictFullDb,
                &table,
                &columns,
            ) else {
                continue;
            };
            resolvers.push(resolver);
        }
    }
    resolvers.sort_by_key(|resolver| {
        (
            display_name(&resolver.path).to_ascii_lowercase(),
            resolver.source.sort_key(),
            resolver.table.to_ascii_lowercase(),
        )
    });
    Ok(resolvers)
}

fn resolver_for_ios_search_table(
    path: PathBuf,
    source: SsedIosSearchResolverSource,
    table: &str,
    columns: &[String],
) -> Option<SsedIosSearchResolver> {
    let (block_column, offset_column) = find_block_offset_pair(columns)?;
    let search_columns = match source {
        SsedIosSearchResolverSource::DictSearchDb => {
            ios_search_columns(columns, &block_column, &offset_column)
        }
        SsedIosSearchResolverSource::DictFullDb => {
            ios_full_db_search_columns(columns, &block_column, &offset_column)
        }
    };
    if search_columns.is_empty() {
        return None;
    }
    let label_columns = ios_label_columns(columns, &search_columns);
    let mode_hints = match source {
        SsedIosSearchResolverSource::DictSearchDb => ios_search_mode_hints(&path, table, columns),
        SsedIosSearchResolverSource::DictFullDb => ios_full_db_mode_hints(columns),
    };
    if mode_hints.is_empty() {
        return None;
    }
    Some(SsedIosSearchResolver {
        path,
        source,
        table: table.to_owned(),
        block_column,
        offset_column,
        search_columns,
        label_columns,
        mode_hints,
    })
}

fn resolver_applies_to_mode(resolver: &SsedIosSearchResolver, mode: &SearchMode) -> bool {
    let SearchMode::Advanced(name) = mode else {
        return false;
    };
    resolver
        .mode_hints
        .contains(&normalize_advanced_mode_name(name))
}

fn search_ios_resolver(
    resolver: &SsedIosSearchResolver,
    query: &str,
    page_offset: usize,
    page_limit: usize,
) -> Result<Vec<SsedIosSearchRow>> {
    let pattern = sqlite_like_contains_pattern(query);
    if pattern == "%%" {
        return Ok(Vec::new());
    }
    let connection = open_ios_search_connection(&resolver.path)?;
    let mut select_columns = vec![
        resolver.block_column.clone(),
        resolver.offset_column.clone(),
    ];
    for column in resolver
        .label_columns
        .iter()
        .chain(resolver.search_columns.iter())
    {
        if !select_columns.iter().any(|existing| existing == column) {
            select_columns.push(column.clone());
        }
    }
    let select_sql = select_columns
        .iter()
        .map(|column| quote_sql_identifier(column))
        .collect::<Vec<_>>()
        .join(", ");
    let where_sql = resolver
        .search_columns
        .iter()
        .map(|column| format!("{} like ? escape '\\'", quote_sql_identifier(column)))
        .collect::<Vec<_>>()
        .join(" or ");
    let sql = format!(
        "select {select_sql} from {} where {where_sql} order by rowid limit ?",
        quote_sql_identifier(&resolver.table),
    );
    let row_limit = page_offset.saturating_add(page_limit).max(page_limit);
    let mut params = vec![pattern.as_str(); resolver.search_columns.len()];
    let row_limit_string = row_limit.to_string();
    params.push(row_limit_string.as_str());
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(params), |row| {
        ios_search_row_from_sqlite_row(resolver, &select_columns, row)
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn ios_search_row_from_sqlite_row(
    resolver: &SsedIosSearchResolver,
    select_columns: &[String],
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<SsedIosSearchRow> {
    let block_index = select_columns
        .iter()
        .position(|column| column == &resolver.block_column)
        .unwrap_or(0);
    let offset_index = select_columns
        .iter()
        .position(|column| column == &resolver.offset_column)
        .unwrap_or(1);
    let block = sqlite_value_to_u32(row.get_ref(block_index)?)?;
    let offset = sqlite_value_to_u32(row.get_ref(offset_index)?)?;
    let mut labels = Vec::new();
    for column in &resolver.label_columns {
        if let Some(index) = select_columns
            .iter()
            .position(|selected| selected == column)
        {
            push_nonempty_unique(
                &mut labels,
                decode_ios_search_text(&sqlite_value_to_string(row.get_ref(index)?)?),
            );
        }
    }
    let mut snippets = Vec::new();
    for column in &resolver.search_columns {
        if let Some(index) = select_columns
            .iter()
            .position(|selected| selected == column)
        {
            push_nonempty_unique(
                &mut snippets,
                decode_ios_search_text(&sqlite_value_to_string(row.get_ref(index)?)?),
            );
        }
    }
    let label = labels
        .into_iter()
        .find(|value| !value.trim().is_empty())
        .or_else(|| snippets.first().cloned())
        .unwrap_or_else(|| format!("{}:{:04x}", block, offset));
    Ok(SsedIosSearchRow {
        block,
        offset,
        label,
        snippet: snippets.join(" / "),
    })
}

fn find_block_offset_pair(columns: &[String]) -> Option<(String, String)> {
    for (block, offset) in [
        ("f_Block_Dic", "f_Offset_Dic"),
        ("Block", "Offset"),
        ("f_block", "f_offset"),
        ("block", "offset"),
        ("full_Block", "full_Offset"),
        ("part_Block", "part_Offset"),
    ] {
        if let (Some(block), Some(offset)) =
            (find_column(columns, block), find_column(columns, offset))
        {
            return Some((block, offset));
        }
    }
    None
}

fn ios_search_columns(columns: &[String], block_column: &str, offset_column: &str) -> Vec<String> {
    columns
        .iter()
        .filter(|column| {
            !column.eq_ignore_ascii_case(block_column)
                && !column.eq_ignore_ascii_case(offset_column)
                && ios_search_column_name_is_textual(column)
        })
        .cloned()
        .collect()
}

fn ios_full_db_search_columns(
    columns: &[String],
    block_column: &str,
    offset_column: &str,
) -> Vec<String> {
    let mut search_columns = Vec::new();
    for alias in ["Body", "BodyText", "Contents", "Content", "Text", "Plain"] {
        if let Some(column) = find_column(columns, alias) {
            push_nonempty_unique(&mut search_columns, column);
        }
    }
    search_columns.retain(|column| {
        !column.eq_ignore_ascii_case(block_column)
            && !column.eq_ignore_ascii_case(offset_column)
            && ios_search_column_name_is_textual(column)
    });
    search_columns
}

fn ios_label_columns(columns: &[String], search_columns: &[String]) -> Vec<String> {
    let mut labels = Vec::new();
    for alias in [
        "Title",
        "Target",
        "Midashi",
        "MidashiJ",
        "Keyword",
        "f_Keyword_Jp",
        "f_Keyword_En",
        "f_exam",
        "f_exam2",
        "Col1",
        "Col2",
        "Col0",
    ] {
        if let Some(column) = find_column(columns, alias) {
            push_nonempty_unique(&mut labels, column);
        }
    }
    if labels.is_empty() {
        labels.extend(search_columns.iter().take(3).cloned());
    }
    labels
}

fn ios_search_mode_hints(path: &Path, table: &str, columns: &[String]) -> BTreeSet<String> {
    let path_name = display_name(path).to_ascii_lowercase();
    let table_name = table.to_ascii_lowercase();
    let mut hints = BTreeSet::new();
    if table_name.contains("example")
        || table_name.contains("exam")
        || table_name == "t_exam"
        || columns
            .iter()
            .any(|column| column.eq_ignore_ascii_case("f_exam"))
    {
        hints.insert("example".to_owned());
    }
    if table_name.contains("idiom")
        || table_name.contains("seiku")
        || path_name.contains("_mi.")
        || path_name.ends_with("_mi.sql")
    {
        hints.insert("phrase".to_owned());
    }
    if table_name == "d_goyo" || table_name == "d_keigo" || table_name == "d_kininaru" {
        hints.insert("sakuin".to_owned());
    }
    if table_name == "koro" {
        hints.insert("gyaku".to_owned());
    }
    hints
}

fn ios_full_db_mode_hints(columns: &[String]) -> BTreeSet<String> {
    let mut hints = BTreeSet::new();
    if ["Body", "BodyText", "Contents", "Content", "Text", "Plain"]
        .iter()
        .any(|alias| find_column(columns, alias).is_some())
    {
        hints.insert("example".to_owned());
    }
    hints
}

fn ios_search_column_name_is_textual(column: &str) -> bool {
    let name = column.to_ascii_lowercase();
    if matches!(
        name.as_str(),
        "no" | "rowid"
            | "col0"
            | "level"
            | "f_order"
            | "f_kaku"
            | "f_bushunai_kaku"
            | "f_bushu_kaku"
            | "f_sstext_id"
            | "f_bushu_id"
            | "f_code"
            | "part_block"
            | "part_offset"
            | "full_block"
            | "full_offset"
    ) {
        return false;
    }
    !(name.contains("block") || name.contains("offset"))
}

fn normalize_advanced_mode_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn decode_ios_search_text(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(bytes) = decode_hex_string(trimmed) {
        let decoded = decode_title_text(&bytes);
        if !decoded.trim().is_empty() {
            return decoded;
        }
    }
    trimmed
        .split(':')
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned()
}

fn decode_hex_string(value: &str) -> Option<Vec<u8>> {
    let compact = value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if compact.len() < 4 || compact.len() % 2 != 0 {
        return None;
    }
    if !compact.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = Vec::with_capacity(compact.len() / 2);
    for index in (0..compact.len()).step_by(2) {
        let byte = u8::from_str_radix(&compact[index..index + 2], 16).ok()?;
        bytes.push(byte);
    }
    Some(bytes)
}

fn open_ios_search_connection(path: &Path) -> Result<Connection> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

fn sqlite_table_names(connection: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut statement = connection.prepare(
        "select name from sqlite_master where type in ('table', 'view') and name not like 'sqlite_%' order by name",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect()
}

fn sqlite_columns(connection: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    let sql = format!("pragma table_info({})", quote_sql_string(table));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect()
}

fn find_column(columns: &[String], alias: &str) -> Option<String> {
    columns
        .iter()
        .find(|column| column.eq_ignore_ascii_case(alias))
        .cloned()
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

fn sqlite_value_to_string(value: ValueRef<'_>) -> rusqlite::Result<String> {
    Ok(match value {
        ValueRef::Null => String::new(),
        ValueRef::Integer(value) => value.to_string(),
        ValueRef::Real(value) => value.to_string(),
        ValueRef::Text(value) => String::from_utf8_lossy(value).into_owned(),
        ValueRef::Blob(value) => {
            if let Ok(text) = std::str::from_utf8(value) {
                text.to_owned()
            } else {
                let (decoded, _encoding, had_errors) = SHIFT_JIS.decode(value);
                if had_errors {
                    String::new()
                } else {
                    decoded.into_owned()
                }
            }
        }
    })
}

fn sqlite_value_to_u32(value: ValueRef<'_>) -> rusqlite::Result<u32> {
    match value {
        ValueRef::Integer(value) => Ok(value.max(0) as u32),
        value => Ok(sqlite_value_to_string(value)?
            .trim()
            .parse::<u32>()
            .unwrap_or(0)),
    }
}

fn quote_sql_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn quote_sql_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn push_nonempty_unique(values: &mut Vec<String>, value: String) {
    let value = value.trim();
    if !value.is_empty() && !values.iter().any(|existing| existing == value) {
        values.push(value.to_owned());
    }
}

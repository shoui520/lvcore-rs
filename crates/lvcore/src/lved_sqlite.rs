use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use encoding_rs::SHIFT_JIS;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::search::SearchMode;

mod discovery;

pub use discovery::{
    android_dictinfo_for_payload, derive_android_lved_sqlcipher_key, discover_lved_key_file,
    infer_lved_dict_code, is_android_lved_sqlcipher_payload, is_lved_payload_name,
    lved_payload_path, parse_android_dictinfo, read_lved_key_file,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LvedKeyFile {
    pub path: PathBuf,
    pub match_kind: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct LvedSqliteStore {
    pub payload_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_file: Option<LvedKeyFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub android_info: Option<AndroidDictInfo>,
    #[serde(skip, default = "default_lved_connection_cache")]
    connection: Arc<Mutex<Option<Connection>>>,
    #[serde(skip, default = "default_lved_tree_index_cache")]
    tree_indexes_cache: Arc<Mutex<Option<Arc<Vec<LvedTreeIndex>>>>>,
    #[serde(skip, default = "default_lved_title_cache")]
    title_cache: Arc<Mutex<Option<Option<String>>>>,
    #[serde(skip, default = "default_lved_schema_cache")]
    schema_cache: Arc<Mutex<Option<Arc<LvedSqliteSchema>>>>,
}

fn default_lved_connection_cache() -> Arc<Mutex<Option<Connection>>> {
    Arc::new(Mutex::new(None))
}

fn default_lved_tree_index_cache() -> Arc<Mutex<Option<Arc<Vec<LvedTreeIndex>>>>> {
    Arc::new(Mutex::new(None))
}

fn default_lved_title_cache() -> Arc<Mutex<Option<Option<String>>>> {
    Arc::new(Mutex::new(None))
}

fn default_lved_schema_cache() -> Arc<Mutex<Option<Arc<LvedSqliteSchema>>>> {
    Arc::new(Mutex::new(None))
}

impl std::fmt::Debug for LvedSqliteStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LvedSqliteStore")
            .field("payload_path", &self.payload_path)
            .field("key_file", &self.key_file)
            .field("android_info", &self.android_info)
            .finish_non_exhaustive()
    }
}

impl PartialEq for LvedSqliteStore {
    fn eq(&self, other: &Self) -> bool {
        self.payload_path == other.payload_path
            && self.key_file == other.key_file
            && self.android_info == other.android_info
    }
}

impl Eq for LvedSqliteStore {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AndroidDictInfo {
    pub dict_id: i64,
    pub dict_code: String,
    pub title: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LvedSqliteSummary {
    pub title: Option<String>,
    pub list_available: bool,
    pub info_available: bool,
    pub tree_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LvedSearchHit {
    pub list_id: i64,
    pub content_id: i64,
    pub anchor: Option<String>,
    pub title_html: String,
    pub title_text: String,
    pub subtitle_html: String,
    pub list_type: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LvedListWindow {
    pub center: LvedSearchHit,
    pub before: Vec<LvedSearchHit>,
    pub after: Vec<LvedSearchHit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LvedInfoPage {
    pub id: i64,
    pub name: String,
    pub title_html: String,
    pub title_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LvedListItem {
    pub list_id: i64,
    pub content_id: i64,
    pub anchor: Option<String>,
    pub title_html: String,
    pub title_text: String,
    pub subtitle_html: String,
    pub list_type: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LvedTreeIndexItem {
    pub source: String,
    pub raw_target: String,
    pub data_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub level: u32,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LvedTreeIndex {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub items: Vec<LvedTreeIndexItem>,
}

#[derive(Debug, Clone)]
struct LvedSqliteSchema {
    tables: BTreeMap<String, Vec<String>>,
}

impl LvedSqliteSchema {
    fn load(connection: &Connection) -> Result<Self> {
        let mut tables = BTreeMap::new();
        for table in sqlite_table_names(connection)? {
            let columns = sqlite_columns(connection, &table)?;
            tables.insert(table.to_lowercase(), columns);
        }
        Ok(Self { tables })
    }

    fn table_exists(&self, table: &str) -> bool {
        self.tables.contains_key(&table.to_lowercase())
    }

    fn columns(&self, table: &str) -> &[String] {
        self.tables
            .get(&table.to_lowercase())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn table_has_columns(&self, table: &str, required: &[&str]) -> bool {
        let columns = self.columns(table);
        required.iter().all(|column| has_column(columns, column))
    }
}

impl LvedSqliteStore {
    pub fn discover(root: &Path) -> Result<Option<Self>> {
        let Some(payload_path) = lved_payload_path(root)? else {
            return Ok(None);
        };
        let key_file = discover_lved_key_file(&payload_path)?;
        let android_info = android_dictinfo_for_payload(&payload_path)?;
        Ok(Some(Self {
            payload_path,
            key_file,
            android_info,
            connection: default_lved_connection_cache(),
            tree_indexes_cache: default_lved_tree_index_cache(),
            title_cache: default_lved_title_cache(),
            schema_cache: default_lved_schema_cache(),
        }))
    }

    pub fn open_readonly(&self) -> Result<Connection> {
        let connection = Connection::open_with_flags(
            &self.payload_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        if let Some(key) = self.sqlcipher_key()? {
            apply_sqlcipher_key(&connection, &key)?;
        }
        validate_sqlite_connection(&connection)?;
        Ok(connection)
    }

    fn with_connection<T>(&self, read: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let mut guard = self
            .connection
            .lock()
            .map_err(|_| Error::Driver("LVED_SQLITE3 connection cache is poisoned".to_owned()))?;
        if guard.is_none() {
            *guard = Some(self.open_readonly()?);
        }
        let connection = guard
            .as_ref()
            .ok_or_else(|| Error::Driver("LVED_SQLITE3 connection cache is empty".to_owned()))?;
        read(connection)
    }

    fn sqlcipher_key(&self) -> Result<Option<String>> {
        if let Some(key_file) = &self.key_file {
            return Ok(Some(read_lved_key_file(&key_file.path)?));
        }
        Ok(self
            .android_info
            .as_ref()
            .map(|info| derive_android_lved_sqlcipher_key(info.dict_id, &info.dict_code)))
    }

    pub fn table_names(&self) -> Result<Vec<String>> {
        self.with_connection(sqlite_table_names)
    }

    pub fn title(&self) -> Result<Option<String>> {
        if let Some(title) = self
            .android_info
            .as_ref()
            .and_then(|info| nonempty_string(info.title.clone()))
        {
            return Ok(Some(title));
        }
        self.with_connection(|connection| self.cached_title(connection))
    }

    pub fn summary(&self) -> Result<LvedSqliteSummary> {
        self.with_connection(|connection| {
            let schema = self.schema(connection)?;
            let title = self.cached_title(connection)?;
            Ok(LvedSqliteSummary {
                title,
                list_available: lved_list_available(connection, &schema)?,
                info_available: lved_info_available(connection, &schema)?,
                tree_available: !self.tree_indexes_arc()?.is_empty(),
            })
        })
    }

    fn schema(&self, connection: &Connection) -> Result<Arc<LvedSqliteSchema>> {
        {
            let cache = self
                .schema_cache
                .lock()
                .map_err(|_| Error::Driver("LVED_SQLITE3 schema cache is poisoned".to_owned()))?;
            if let Some(schema) = cache.as_ref() {
                return Ok(Arc::clone(schema));
            }
        }

        let schema = Arc::new(LvedSqliteSchema::load(connection)?);
        let mut cache = self
            .schema_cache
            .lock()
            .map_err(|_| Error::Driver("LVED_SQLITE3 schema cache is poisoned".to_owned()))?;
        Ok(Arc::clone(cache.get_or_insert(schema)))
    }

    fn cached_title(&self, connection: &Connection) -> Result<Option<String>> {
        if let Some(title) = self
            .android_info
            .as_ref()
            .and_then(|info| nonempty_string(info.title.clone()))
        {
            return Ok(Some(title));
        }
        {
            let cache = self
                .title_cache
                .lock()
                .map_err(|_| Error::Driver("LVED_SQLITE3 title cache is poisoned".to_owned()))?;
            if let Some(title) = cache.as_ref() {
                return Ok(title.clone());
            }
        }

        let schema = self.schema(connection)?;
        let title =
            lved_sqlite_title_from_connection(connection, &schema).or(self.tree_index_title()?);
        let mut cache = self
            .title_cache
            .lock()
            .map_err(|_| Error::Driver("LVED_SQLITE3 title cache is poisoned".to_owned()))?;
        Ok(cache.get_or_insert(title).clone())
    }

    pub fn search_modes(&self) -> Result<Vec<SearchMode>> {
        self.with_connection(|connection| {
            let schema = self.schema(connection)?;
            Ok(lved_available_search_modes(&schema))
        })
    }

    pub fn search(
        &self,
        query: &str,
        mode: &SearchMode,
        limit: usize,
    ) -> Result<Vec<LvedSearchHit>> {
        self.search_page(query, mode, 0, limit)
    }

    pub fn search_page(
        &self,
        query: &str,
        mode: &SearchMode,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<LvedSearchHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.with_connection(|connection| {
            let schema = self.schema(connection)?;
            search_lved_sqlite_connection(connection, &schema, query, mode, offset, limit)
        })
    }

    pub fn content_html(&self, content_id: i64) -> Result<Option<String>> {
        self.with_connection(|connection| {
            let schema = self.schema(connection)?;
            if !schema.table_has_columns("content", &["id", "body"]) {
                return Ok(None);
            }
            let mut statement =
                connection.prepare("select body from content where id = ? limit 1")?;
            let mut rows = statement.query([content_id])?;
            let Some(row) = rows.next()? else {
                return Ok(None);
            };
            Ok(Some(sqlite_value_to_string(row.get_ref(0)?)?))
        })
    }

    pub fn info_html(&self, row_id: i64) -> Result<Option<String>> {
        self.with_connection(|connection| {
            let schema = self.schema(connection)?;
            if !schema.table_has_columns("info", &["id", "body"]) {
                return Ok(None);
            }
            let mut statement =
                connection.prepare("select body from info where id = ? or rowid = ? limit 1")?;
            let mut rows = statement.query((row_id, row_id))?;
            let Some(row) = rows.next()? else {
                return Ok(None);
            };
            Ok(Some(sqlite_value_to_string(row.get_ref(0)?)?))
        })
    }

    pub fn info_html_by_name(&self, name: &str) -> Result<Option<String>> {
        self.with_connection(|connection| {
            let schema = self.schema(connection)?;
            if !schema.table_has_columns("info", &["name", "body"]) {
                return Ok(None);
            }
            let mut statement =
                connection.prepare("select body from info where name = ? limit 1")?;
            let mut rows = statement.query([name])?;
            let Some(row) = rows.next()? else {
                return Ok(None);
            };
            Ok(Some(sqlite_value_to_string(row.get_ref(0)?)?))
        })
    }

    pub fn named_html_by_name(&self, table: &str, name: &str) -> Result<Option<String>> {
        self.with_connection(|connection| {
            let schema = self.schema(connection)?;
            if !is_safe_sqlite_identifier(table)
                || !schema.table_has_columns(table, &["name", "body"])
            {
                return Ok(None);
            }
            let sql = format!(
                "select body from {} where name = ? limit 1",
                quote_identifier(table)
            );
            let mut statement = connection.prepare(&sql)?;
            let mut rows = statement.query([name])?;
            let Some(row) = rows.next()? else {
                return Ok(None);
            };
            Ok(Some(sqlite_value_to_string(row.get_ref(0)?)?))
        })
    }

    pub fn info_pages(&self, limit: usize) -> Result<Vec<LvedInfoPage>> {
        self.info_pages_page(0, limit)
    }

    pub fn info_pages_page(&self, offset: usize, limit: usize) -> Result<Vec<LvedInfoPage>> {
        self.with_connection(|connection| {
            let schema = self.schema(connection)?;
            if limit == 0 || !schema.table_has_columns("info", &["id", "name", "body"]) {
                return Ok(Vec::new());
            }
            let mut statement = connection.prepare(
                "select coalesce(id, rowid), name, body from info \
                 order by coalesce(id, rowid), rowid limit ? offset ?",
            )?;
            let rows = statement.query_map((limit as i64, offset as i64), |row| {
                let name = sqlite_value_to_string(row.get_ref(1)?)?;
                let body = sqlite_value_to_string(row.get_ref(2)?)?;
                let title_text = html_text_lines(&body)
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| name.clone());
                Ok(LvedInfoPage {
                    id: row.get(0)?,
                    name: name.clone(),
                    title_html: title_text.clone(),
                    title_text,
                })
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::from)
        })
    }

    pub fn list_items(&self, limit: usize) -> Result<Vec<LvedListItem>> {
        self.list_items_page(0, limit)
    }

    pub fn list_items_page(&self, offset: usize, limit: usize) -> Result<Vec<LvedListItem>> {
        self.with_connection(|connection| {
            let schema = self.schema(connection)?;
            let list_columns = schema.columns("list");
            if limit == 0 || !has_column(list_columns, "id") || !has_column(list_columns, "refid") {
                return Ok(Vec::new());
            }
            let rows = lved_list_hits_by_id_clause_offset(
                connection,
                list_columns,
                "1 = ?",
                "l.id",
                1,
                limit,
                offset,
            )?;
            Ok(rows.into_iter().map(LvedListItem::from).collect())
        })
    }

    pub fn tree_index_items(&self) -> Result<Vec<LvedTreeIndexItem>> {
        Ok(self
            .tree_indexes_arc()?
            .iter()
            .flat_map(|tree| tree.items.iter().cloned())
            .collect())
    }

    pub fn tree_indexes(&self) -> Result<Vec<LvedTreeIndex>> {
        Ok(self.tree_indexes_arc()?.as_ref().clone())
    }

    fn tree_indexes_arc(&self) -> Result<Arc<Vec<LvedTreeIndex>>> {
        {
            let cache = self.tree_indexes_cache.lock().map_err(|_| {
                Error::Driver("LVED_SQLITE3 tree index cache is poisoned".to_owned())
            })?;
            if let Some(trees) = cache.as_ref() {
                return Ok(Arc::clone(trees));
            }
        }

        let trees = Arc::new(self.load_tree_indexes()?);
        let mut cache = self
            .tree_indexes_cache
            .lock()
            .map_err(|_| Error::Driver("LVED_SQLITE3 tree index cache is poisoned".to_owned()))?;
        Ok(Arc::clone(cache.get_or_insert(trees)))
    }

    fn load_tree_indexes(&self) -> Result<Vec<LvedTreeIndex>> {
        let root = self.payload_path.parent().ok_or_else(|| {
            Error::Driver("LVED_SQLITE3 payload has no parent directory".to_owned())
        })?;
        let mut trees = Vec::new();
        for path in lved_tree_index_candidate_paths(root)? {
            let data = fs::read(&path)?;
            if !is_lved_text_tree_index(&data) {
                continue;
            }
            let source = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let items = parse_lved_tree_index(&data, &source)?;
            let title = items
                .iter()
                .find_map(|row| (row.level == 0).then_some(row.label.clone()))
                .filter(|label| usable_lved_tree_title(label));
            trees.push(LvedTreeIndex {
                source,
                title,
                items,
            });
        }
        Ok(trees)
    }

    pub fn tree_index_paths(&self) -> Result<Vec<PathBuf>> {
        let Some(root) = self.payload_path.parent() else {
            return Ok(Vec::new());
        };
        Ok(self
            .tree_indexes_arc()?
            .iter()
            .map(|tree| root.join(&tree.source))
            .collect())
    }

    pub fn tree_index_path(&self) -> Option<PathBuf> {
        let root = self.payload_path.parent()?;
        self.tree_indexes_arc()
            .ok()?
            .first()
            .map(|tree| root.join(&tree.source))
    }

    pub fn tree_index_title(&self) -> Result<Option<String>> {
        Ok(self
            .tree_indexes_arc()?
            .iter()
            .find_map(|tree| tree.title.clone()))
    }

    pub fn media_blob(&self, store: &str, key: &str) -> Result<Option<Vec<u8>>> {
        self.with_connection(|connection| {
            let table = match store {
                "lved.media" | "media" => "media",
                "lved.mediasub" | "mediasub" => "mediasub",
                _ => return Ok(None),
            };
            let schema = self.schema(connection)?;
            if !schema.table_has_columns(table, &["name", "main"]) {
                return Ok(None);
            }
            let stem = Path::new(key)
                .file_stem()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| key.to_owned());
            let sql = format!(
                "select main from {} where name = ? or name = ? limit 1",
                quote_identifier(table)
            );
            let mut statement = connection.prepare(&sql)?;
            let mut rows = statement.query((key, stem))?;
            let Some(row) = rows.next()? else {
                return Ok(None);
            };
            Ok(Some(sqlite_value_to_bytes(row.get_ref(0)?)?))
        })
    }

    pub fn list_window_for_content(
        &self,
        content_id: i64,
        before: usize,
        after: usize,
    ) -> Result<Option<LvedListWindow>> {
        self.with_connection(|connection| {
            let schema = self.schema(connection)?;
            let list_columns = schema.columns("list");
            if !has_column(list_columns, "id") || !has_column(list_columns, "refid") {
                return Ok(None);
            }
            let Some(center_list_id) = connection
                .query_row(
                    "select id from list where refid = ? order by id limit 1",
                    [content_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
            else {
                return Ok(None);
            };
            let center = lved_list_hits_by_id_clause(
                connection,
                list_columns,
                "l.id = ?",
                "l.id",
                center_list_id,
                1,
            )?
            .into_iter()
            .next();
            let Some(center) = center else {
                return Ok(None);
            };
            let mut before_rows = lved_list_hits_by_id_clause(
                connection,
                list_columns,
                "l.id < ?",
                "l.id desc",
                center_list_id,
                before,
            )?;
            before_rows.reverse();
            let after_rows = lved_list_hits_by_id_clause(
                connection,
                list_columns,
                "l.id > ?",
                "l.id",
                center_list_id,
                after,
            )?;
            Ok(Some(LvedListWindow {
                center,
                before: before_rows,
                after: after_rows,
            }))
        })
    }
}

fn lved_tree_index_candidate_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    push_lved_tree_index_path(&mut paths, root.join("res/tree.idx"));
    push_lved_tree_index_path(&mut paths, root.join("tree.idx"));
    for entry in fs::read_dir(root)?.collect::<std::io::Result<Vec<_>>>()? {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("idx"))
        {
            push_lved_tree_index_path(&mut paths, path);
        }
    }
    let res_dir = root.join("res");
    if res_dir.is_dir() {
        for entry in fs::read_dir(&res_dir)?.collect::<std::io::Result<Vec<_>>>()? {
            let path = entry.path();
            if path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("idx"))
            {
                push_lved_tree_index_path(&mut paths, path);
            }
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn lved_list_available(connection: &Connection, schema: &LvedSqliteSchema) -> Result<bool> {
    let list_columns = schema.columns("list");
    if !has_column(list_columns, "id") || !has_column(list_columns, "refid") {
        return Ok(false);
    }
    connection
        .query_row("select 1 from list limit 1", [], |_| Ok(()))
        .optional()
        .map(|value| value.is_some())
        .map_err(Error::from)
}

fn lved_info_available(connection: &Connection, schema: &LvedSqliteSchema) -> Result<bool> {
    if !schema.table_has_columns("info", &["id", "name", "body"]) {
        return Ok(false);
    }
    connection
        .query_row("select 1 from info limit 1", [], |_| Ok(()))
        .optional()
        .map(|value| value.is_some())
        .map_err(Error::from)
}

fn parse_lved_tree_index(bytes: &[u8], source: &str) -> Result<Vec<LvedTreeIndexItem>> {
    let text = decode_sqlite_text(bytes);
    let mut items = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r').trim_start_matches('\u{feff}');
        if line.trim().is_empty() {
            continue;
        }
        let mut columns = line.splitn(3, '\t');
        let Some(raw_target) = columns.next() else {
            continue;
        };
        let Some((data_id, query)) = parse_lved_tree_target(raw_target) else {
            continue;
        };
        let Some(level) = columns.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let Some(label) = columns.next() else {
            continue;
        };
        items.push(LvedTreeIndexItem {
            source: source.to_owned(),
            raw_target: raw_target.trim().to_owned(),
            data_id,
            query,
            level,
            label: label.to_owned(),
        });
    }
    Ok(items)
}

fn parse_lved_tree_target(value: &str) -> Option<(i64, Option<String>)> {
    let stripped = value.trim();
    let (target, query) = match stripped.split_once('?') {
        Some((target, query)) => (target, Some(query.to_owned())),
        None => (stripped, None),
    };
    if target.is_empty()
        || !target
            .bytes()
            .all(|byte| byte == b'-' || byte.is_ascii_digit())
    {
        return None;
    }
    target.parse::<i64>().ok().map(|value| (value, query))
}

fn is_lved_text_tree_index(bytes: &[u8]) -> bool {
    let Some((_, text)) = decode_retained_text(bytes) else {
        return false;
    };
    for line in text.lines() {
        let line = line.trim_end_matches('\r').trim_start_matches('\u{feff}');
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let Some(first) = parts.next().map(str::trim) else {
            return false;
        };
        let Some(second) = parts.next().map(str::trim) else {
            return false;
        };
        if is_eight_digit_hex(first) && is_eight_digit_hex(second) {
            return false;
        }
        return parse_lved_tree_target(first).is_some() && second.parse::<u32>().is_ok();
    }
    false
}

fn decode_retained_text(bytes: &[u8]) -> Option<(&'static str, String)> {
    if let Ok(value) = std::str::from_utf8(bytes) {
        return Some(("utf-8", value.trim_start_matches('\u{feff}').to_owned()));
    }
    let (decoded, _, had_errors) = SHIFT_JIS.decode(bytes);
    (!had_errors).then(|| ("cp932", decoded.into_owned()))
}

fn is_eight_digit_hex(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn push_lved_tree_index_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_file() && !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn usable_lved_tree_title(label: &str) -> bool {
    let value = html_to_text(label).trim().to_owned();
    !value.is_empty() && !matches!(value.as_str(), "見出し語索引" | "索引" | "目次")
}

impl From<LvedSearchHit> for LvedListItem {
    fn from(value: LvedSearchHit) -> Self {
        Self {
            list_id: value.list_id,
            content_id: value.content_id,
            anchor: value.anchor,
            title_html: value.title_html,
            title_text: value.title_text,
            subtitle_html: value.subtitle_html,
            list_type: value.list_type,
        }
    }
}

pub fn apply_sqlcipher_key(connection: &Connection, key: &str) -> Result<()> {
    connection.pragma_update(None, "key", key)?;
    connection.pragma_update(None, "cipher_compatibility", 4)?;
    Ok(())
}

pub fn sqlite_table_names(connection: &Connection) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare("select name from sqlite_master where type in ('table', 'view') order by name")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

fn search_lved_sqlite_connection(
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

fn lved_list_hits_by_id_clause(
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

fn lved_list_hits_by_id_clause_offset(
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

fn lved_available_search_modes(schema: &LvedSqliteSchema) -> Vec<SearchMode> {
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
struct LvedListProjection<'a> {
    anchor: &'a str,
    title: &'a str,
    subtitle: &'a str,
    kind: &'a str,
}

fn lved_list_projection(columns: &[String]) -> LvedListProjection<'_> {
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

fn nonempty_string(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
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

fn decode_sqlite_text(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if let Ok(value) = std::str::from_utf8(bytes) {
        return value.to_owned();
    }
    let (decoded, _encoding, had_errors) = SHIFT_JIS.decode(bytes);
    if had_errors {
        String::new()
    } else {
        decoded.into_owned()
    }
}

fn validate_sqlite_connection(connection: &Connection) -> Result<()> {
    let _: i64 =
        connection.query_row("select count(*) from sqlite_master", [], |row| row.get(0))?;
    Ok(())
}

fn lved_sqlite_title_from_connection(
    connection: &Connection,
    schema: &LvedSqliteSchema,
) -> Option<String> {
    if !schema.table_has_columns("info", &["name", "body"]) {
        return None;
    }
    let mut statement = connection
        .prepare(
            "
            select name, body from info
            where body is not null and body != ''
            order by
              case
                when lower(name) = 'index.html' then 0
                when lower(name) like '%index%' then 1
                when lower(name) like '%about%' then 2
                when lower(name) like '%hanrei%' then 3
                when lower(name) like '%copyright%' then 4
                when lower(name) like '%license%' then 5
                else 6
              end,
              rowid
            limit 1024
            ",
        )
        .ok()?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        })
        .ok()?;
    let mut candidates = Vec::<(i32, usize, String)>::new();
    for (index, row) in rows.enumerate() {
        let Ok((name, body)) = row else {
            continue;
        };
        let lower_name = name.to_lowercase();
        let Some(candidate) = (if lower_name.contains("copyright") || lower_name.contains("license")
        {
            lved_copyright_title_candidate(&body)
        } else {
            lved_html_title_candidate(&body)
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
        .and_then(|(score, _, title)| (score > 0).then_some(title))
}

fn sqlite_columns(connection: &Connection, table: &str) -> Result<Vec<String>> {
    let Ok(mut statement) =
        connection.prepare(&format!("pragma table_info({})", quote_identifier(table)))
    else {
        return Ok(Vec::new());
    };
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map(|columns| {
            columns
                .into_iter()
                .map(|column| column.to_lowercase())
                .collect()
        })
        .map_err(Error::from)
}

fn has_column(columns: &[String], column: &str) -> bool {
    columns.iter().any(|found| found == &column.to_lowercase())
}

fn html_text_lines(fragment: &str) -> Vec<String> {
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
                if matches!(tag_name, "style" | "script") {
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

fn decode_basic_html_entities(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut cursor = 0;
    while cursor < value.len() {
        let rest = &value[cursor..];
        let Some((entity, replacement)) = match_basic_html_entity(rest) else {
            let Some(ch) = rest.chars().next() else {
                break;
            };
            decoded.push(ch);
            cursor += ch.len_utf8();
            continue;
        };
        decoded.push_str(replacement);
        cursor += entity.len();
    }
    decoded
}

fn match_basic_html_entity(value: &str) -> Option<(&'static str, &'static str)> {
    if value.starts_with("&nbsp;") {
        Some(("&nbsp;", " "))
    } else if value.starts_with("&lt;") {
        Some(("&lt;", "<"))
    } else if value.starts_with("&gt;") {
        Some(("&gt;", ">"))
    } else if value.starts_with("&amp;") {
        Some(("&amp;", "&"))
    } else if value.starts_with("&quot;") {
        Some(("&quot;", "\""))
    } else {
        None
    }
}

fn html_to_text(fragment: &str) -> String {
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

fn normalize_title_candidate(value: &str) -> Option<String> {
    let mut value = value
        .split("について")
        .next()
        .unwrap_or(value)
        .trim_matches(|ch: char| ch.is_whitespace() || "　:：-－【】『』《》".contains(ch))
        .to_owned();
    for marker in ["&copy;", "©", "Copyright", "copyright", "(C)", "（C）"] {
        if let Some((head, _tail)) = value.split_once(marker) {
            value = head.trim().to_owned();
        }
    }
    for marker in [" 凡例", "　凡例", " 目次", "　目次", " ●", "　●"] {
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

fn title_score(value: &str) -> i32 {
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
    if value.ends_with("小辞典") && !value.contains('第') {
        score -= 80;
    }
    if value.chars().count() > 50 {
        score -= 20;
    }
    score
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn is_safe_sqlite_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn normalize_lved_dict_code(value: &str) -> String {
    value
        .trim()
        .strip_prefix("_DCT_")
        .unwrap_or(value.trim())
        .trim_start_matches('.')
        .to_ascii_uppercase()
}

fn files_with_suffix(root: &Path, suffix: &str) -> Result<Vec<PathBuf>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let suffix = suffix.to_lowercase();
    let mut out = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase().ends_with(&suffix))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;

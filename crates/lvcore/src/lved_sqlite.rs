use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use encoding_rs::SHIFT_JIS;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::ios_dictlist::discover_ios_dictlist_info;
use crate::search::SearchMode;
use crate::storage::regular_file_inside_root;

mod discovery;
mod schema;
mod sql_search;
mod title;
mod tree;

use schema::{LvedSqliteSchema, has_column};
#[cfg(test)]
use sql_search::{LvedListProjection, lved_list_projection};
use sql_search::{
    lved_available_search_modes, lved_list_hits_by_id_clause, lved_list_hits_by_id_clause_offset,
    search_lved_sqlite_connection,
};
use title::{html_text_lines, lved_sqlite_title_from_connection};
#[cfg(test)]
use title::{normalize_title_candidate, title_score};
use tree::{
    is_lved_text_tree_index, lved_tree_index_candidate_paths, parse_lved_tree_index,
    usable_lved_tree_title,
};

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
    #[serde(skip, default = "default_lved_tree_index_items_cache")]
    tree_index_items_cache: Arc<Mutex<Option<Arc<Vec<LvedTreeIndexItem>>>>>,
    #[serde(skip, default = "default_lved_title_cache")]
    title_cache: Arc<Mutex<Option<Option<String>>>>,
    #[serde(skip, default = "default_lved_schema_cache")]
    schema_cache: Arc<Mutex<Option<Arc<LvedSqliteSchema>>>>,
    #[serde(skip, default = "default_lved_media_index_cache")]
    media_index_cache: Arc<Mutex<BTreeMap<String, Arc<LvedMediaIndex>>>>,
}

fn default_lved_connection_cache() -> Arc<Mutex<Option<Connection>>> {
    Arc::new(Mutex::new(None))
}

fn default_lved_tree_index_cache() -> Arc<Mutex<Option<Arc<Vec<LvedTreeIndex>>>>> {
    Arc::new(Mutex::new(None))
}

fn default_lved_tree_index_items_cache() -> Arc<Mutex<Option<Arc<Vec<LvedTreeIndexItem>>>>> {
    Arc::new(Mutex::new(None))
}

fn default_lved_title_cache() -> Arc<Mutex<Option<Option<String>>>> {
    Arc::new(Mutex::new(None))
}

fn default_lved_schema_cache() -> Arc<Mutex<Option<Arc<LvedSqliteSchema>>>> {
    Arc::new(Mutex::new(None))
}

fn default_lved_media_index_cache() -> Arc<Mutex<BTreeMap<String, Arc<LvedMediaIndex>>>> {
    Arc::new(Mutex::new(BTreeMap::new()))
}

#[derive(Debug, Clone, Default)]
struct LvedMediaIndex {
    by_name: BTreeMap<String, i64>,
    by_id: BTreeMap<i64, i64>,
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

impl LvedSqliteStore {
    pub fn from_payload_with_derived_key_info(
        payload_path: PathBuf,
        key_info: AndroidDictInfo,
    ) -> Self {
        Self {
            payload_path,
            key_file: None,
            android_info: Some(key_info),
            connection: default_lved_connection_cache(),
            tree_indexes_cache: default_lved_tree_index_cache(),
            tree_index_items_cache: default_lved_tree_index_items_cache(),
            title_cache: default_lved_title_cache(),
            schema_cache: default_lved_schema_cache(),
            media_index_cache: default_lved_media_index_cache(),
        }
    }

    pub fn discover(root: &Path) -> Result<Option<Self>> {
        let Some(payload_path) = lved_payload_path(root)? else {
            return Ok(None);
        };
        let key_file = discover_lved_key_file(&payload_path)?;
        let android_info = if key_file.is_none() {
            ios_lved_sqlcipher_info_for_payload(&payload_path)?
                .or(android_dictinfo_for_payload(&payload_path)?)
        } else {
            None
        };
        Ok(Some(Self {
            payload_path,
            key_file,
            android_info,
            connection: default_lved_connection_cache(),
            tree_indexes_cache: default_lved_tree_index_cache(),
            tree_index_items_cache: default_lved_tree_index_items_cache(),
            title_cache: default_lved_title_cache(),
            schema_cache: default_lved_schema_cache(),
            media_index_cache: default_lved_media_index_cache(),
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

    pub fn cheap_title_hint(&self) -> Option<String> {
        self.android_info
            .as_ref()
            .and_then(|info| nonempty_string(info.title.clone()))
    }

    pub fn summary(&self) -> Result<LvedSqliteSummary> {
        self.with_connection(|connection| {
            let schema = self.schema(connection)?;
            let title = self.cached_title(connection)?;
            Ok(LvedSqliteSummary {
                title,
                list_available: lved_list_available(connection, &schema)?,
                info_available: lved_info_available(connection, &schema)?,
                tree_available: self.tree_index_available()?,
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

    pub fn content_title_text(&self, content_id: i64) -> Result<Option<String>> {
        self.with_connection(|connection| {
            let schema = self.schema(connection)?;
            let list_columns = schema.columns("list");
            if !has_column(list_columns, "id") || !has_column(list_columns, "refid") {
                return Ok(None);
            }
            let title = lved_list_hits_by_id_clause(
                connection,
                list_columns,
                "l.refid = ?",
                "l.id",
                content_id,
                1,
            )?
            .into_iter()
            .find_map(|hit| (!hit.title_text.trim().is_empty()).then_some(hit.title_text));
            Ok(title)
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
        Ok(self.tree_index_items_arc()?.as_ref().clone())
    }

    pub(crate) fn tree_index_items_arc(&self) -> Result<Arc<Vec<LvedTreeIndexItem>>> {
        {
            let cache = self.tree_index_items_cache.lock().map_err(|_| {
                Error::Driver("LVED_SQLITE3 tree index item cache is poisoned".to_owned())
            })?;
            if let Some(items) = cache.as_ref() {
                return Ok(Arc::clone(items));
            }
        }

        let items = Arc::new(
            self.tree_indexes_arc()?
                .iter()
                .flat_map(|tree| tree.items.iter().cloned())
                .collect(),
        );
        let mut cache = self.tree_index_items_cache.lock().map_err(|_| {
            Error::Driver("LVED_SQLITE3 tree index item cache is poisoned".to_owned())
        })?;
        Ok(Arc::clone(cache.get_or_insert(items)))
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

    fn tree_index_available(&self) -> Result<bool> {
        let root = self.payload_path.parent().ok_or_else(|| {
            Error::Driver("LVED_SQLITE3 payload has no parent directory".to_owned())
        })?;
        for path in lved_tree_index_candidate_paths(root)? {
            let data = fs::read(path)?;
            if is_lved_text_tree_index(&data) {
                return Ok(true);
            }
        }
        Ok(false)
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
            let tables: &[&str] = match store {
                "lved.media" | "media" => &["media"],
                "lved.mediasub" | "mediasub" => &["mediasub", "sound", "media"],
                "lved.sound" | "sound" => &["sound", "mediasub", "media"],
                _ => return Ok(None),
            };
            let schema = self.schema(connection)?;
            let names = lved_media_lookup_names(key);
            let ids = lved_media_lookup_ids(key);
            for table in tables {
                if !schema.table_has_columns(table, &["name", "main"]) {
                    continue;
                }
                let name_uses_index =
                    sqlite_equality_lookup_uses_search(connection, table, "name")?;
                if name_uses_index
                    && let Some(bytes) = media_blob_by_indexed_name(connection, table, &names)?
                {
                    return Ok(Some(bytes));
                }
                let has_id = has_column(schema.columns(table), "id");
                let id_uses_index =
                    has_id && sqlite_equality_lookup_uses_search(connection, table, "id")?;
                if id_uses_index
                    && let Some(bytes) = media_blob_by_indexed_id(connection, table, &ids)?
                {
                    return Ok(Some(bytes));
                }
                if name_uses_index && (!has_id || id_uses_index || ids.is_empty()) {
                    continue;
                }
                if let Some(bytes) =
                    self.media_blob_from_index(connection, &schema, table, &names, &ids)?
                {
                    return Ok(Some(bytes));
                }
            }
            Ok(None)
        })
    }

    pub fn media_blob_len(&self, store: &str, key: &str) -> Result<Option<u64>> {
        self.with_connection(|connection| {
            let tables: &[&str] = match store {
                "lved.media" | "media" => &["media"],
                "lved.mediasub" | "mediasub" => &["mediasub", "sound", "media"],
                "lved.sound" | "sound" => &["sound", "mediasub", "media"],
                _ => return Ok(None),
            };
            let schema = self.schema(connection)?;
            let names = lved_media_lookup_names(key);
            let ids = lved_media_lookup_ids(key);
            for table in tables {
                if !schema.table_has_columns(table, &["name", "main"]) {
                    continue;
                }
                let name_uses_index =
                    sqlite_equality_lookup_uses_search(connection, table, "name")?;
                if name_uses_index
                    && let Some(byte_len) =
                        media_blob_len_by_indexed_name(connection, table, &names)?
                {
                    return Ok(Some(byte_len));
                }
                let has_id = has_column(schema.columns(table), "id");
                let id_uses_index =
                    has_id && sqlite_equality_lookup_uses_search(connection, table, "id")?;
                if id_uses_index
                    && let Some(byte_len) = media_blob_len_by_indexed_id(connection, table, &ids)?
                {
                    return Ok(Some(byte_len));
                }
                if name_uses_index && (!has_id || id_uses_index || ids.is_empty()) {
                    continue;
                }
                if let Some(byte_len) =
                    self.media_blob_len_from_index(connection, &schema, table, &names, &ids)?
                {
                    return Ok(Some(byte_len));
                }
            }
            Ok(None)
        })
    }

    fn media_blob_from_index(
        &self,
        connection: &Connection,
        schema: &LvedSqliteSchema,
        table: &str,
        names: &[String],
        ids: &[i64],
    ) -> Result<Option<Vec<u8>>> {
        let index = self.media_index(connection, schema, table)?;
        let mut rowids = Vec::new();
        for name in names {
            if let Some(rowid) = index.by_name.get(name) {
                push_unique_i64(&mut rowids, *rowid);
            }
        }
        for id in ids {
            if let Some(rowid) = index.by_id.get(id) {
                push_unique_i64(&mut rowids, *rowid);
            }
        }
        if rowids.is_empty() {
            return Ok(None);
        }
        let sql = format!(
            "select main from {} where rowid = ? limit 1",
            quote_identifier(table)
        );
        let mut statement = connection.prepare(&sql)?;
        for rowid in rowids {
            if let Some(bytes) = statement
                .query_row([rowid], |row| sqlite_value_to_bytes(row.get_ref(0)?))
                .optional()?
            {
                return Ok(Some(bytes));
            }
        }
        Ok(None)
    }

    fn media_blob_len_from_index(
        &self,
        connection: &Connection,
        schema: &LvedSqliteSchema,
        table: &str,
        names: &[String],
        ids: &[i64],
    ) -> Result<Option<u64>> {
        let index = self.media_index(connection, schema, table)?;
        let mut rowids = Vec::new();
        for name in names {
            if let Some(rowid) = index.by_name.get(name) {
                push_unique_i64(&mut rowids, *rowid);
            }
        }
        for id in ids {
            if let Some(rowid) = index.by_id.get(id) {
                push_unique_i64(&mut rowids, *rowid);
            }
        }
        if rowids.is_empty() {
            return Ok(None);
        }
        let sql = format!(
            "select length(main) from {} where rowid = ? limit 1",
            quote_identifier(table)
        );
        let mut statement = connection.prepare(&sql)?;
        for rowid in rowids {
            if let Some(byte_len) = statement
                .query_row([rowid], |row| row.get::<_, Option<i64>>(0))
                .optional()?
                .flatten()
                .and_then(nonnegative_i64_to_u64)
            {
                return Ok(Some(byte_len));
            }
        }
        Ok(None)
    }

    fn media_index(
        &self,
        connection: &Connection,
        schema: &LvedSqliteSchema,
        table: &str,
    ) -> Result<Arc<LvedMediaIndex>> {
        let cache_key = table.to_lowercase();
        if let Some(index) = self
            .media_index_cache
            .lock()
            .map_err(|_| Error::Driver("LVED_SQLITE3 media index cache is poisoned".to_owned()))?
            .get(&cache_key)
            .cloned()
        {
            return Ok(index);
        }
        let index = Arc::new(load_lved_media_index(connection, schema, table)?);
        self.media_index_cache
            .lock()
            .map_err(|_| Error::Driver("LVED_SQLITE3 media index cache is poisoned".to_owned()))?
            .insert(cache_key, index.clone());
        Ok(index)
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

fn nonempty_string(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
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

fn sqlite_equality_lookup_uses_search(
    connection: &Connection,
    table: &str,
    column: &str,
) -> Result<bool> {
    let sql = format!(
        "explain query plan select main from {} where {} = ? limit 1",
        quote_identifier(table),
        quote_identifier(column)
    );
    let mut statement = connection.prepare(&sql)?;
    let details = statement.query_map(rusqlite::params![rusqlite::types::Null], |row| {
        row.get::<_, String>(3)
    })?;
    for detail in details {
        let detail = detail?.to_ascii_lowercase();
        if detail.contains("search") && !detail.contains("scan") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn media_blob_by_indexed_name(
    connection: &Connection,
    table: &str,
    names: &[String],
) -> Result<Option<Vec<u8>>> {
    let sql = format!(
        "select main from {} where {} = ? limit 1",
        quote_identifier(table),
        quote_identifier("name")
    );
    let mut statement = connection.prepare(&sql)?;
    for name in names {
        if let Some(bytes) = statement
            .query_row([name], |row| sqlite_value_to_bytes(row.get_ref(0)?))
            .optional()?
        {
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

fn media_blob_len_by_indexed_name(
    connection: &Connection,
    table: &str,
    names: &[String],
) -> Result<Option<u64>> {
    let sql = format!(
        "select length(main) from {} where {} = ? limit 1",
        quote_identifier(table),
        quote_identifier("name")
    );
    let mut statement = connection.prepare(&sql)?;
    for name in names {
        if let Some(byte_len) = statement
            .query_row([name], |row| row.get::<_, Option<i64>>(0))
            .optional()?
            .flatten()
            .and_then(nonnegative_i64_to_u64)
        {
            return Ok(Some(byte_len));
        }
    }
    Ok(None)
}

fn media_blob_by_indexed_id(
    connection: &Connection,
    table: &str,
    ids: &[i64],
) -> Result<Option<Vec<u8>>> {
    let sql = format!(
        "select main from {} where {} = ? limit 1",
        quote_identifier(table),
        quote_identifier("id")
    );
    let mut statement = connection.prepare(&sql)?;
    for id in ids {
        if let Some(bytes) = statement
            .query_row([id], |row| sqlite_value_to_bytes(row.get_ref(0)?))
            .optional()?
        {
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

fn media_blob_len_by_indexed_id(
    connection: &Connection,
    table: &str,
    ids: &[i64],
) -> Result<Option<u64>> {
    let sql = format!(
        "select length(main) from {} where {} = ? limit 1",
        quote_identifier(table),
        quote_identifier("id")
    );
    let mut statement = connection.prepare(&sql)?;
    for id in ids {
        if let Some(byte_len) = statement
            .query_row([id], |row| row.get::<_, Option<i64>>(0))
            .optional()?
            .flatten()
            .and_then(nonnegative_i64_to_u64)
        {
            return Ok(Some(byte_len));
        }
    }
    Ok(None)
}

fn nonnegative_i64_to_u64(value: i64) -> Option<u64> {
    u64::try_from(value).ok()
}

fn sqlite_value_to_optional_i64(value: ValueRef<'_>) -> rusqlite::Result<Option<i64>> {
    match value {
        ValueRef::Null => Ok(None),
        ValueRef::Integer(value) => Ok(Some(value)),
        ValueRef::Real(_) => Ok(None),
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
            let text = decode_sqlite_text(bytes);
            Ok(text.trim().parse::<i64>().ok())
        }
    }
}

fn load_lved_media_index(
    connection: &Connection,
    schema: &LvedSqliteSchema,
    table: &str,
) -> Result<LvedMediaIndex> {
    let has_id = has_column(schema.columns(table), "id");
    let id_column = if has_id {
        quote_identifier("id")
    } else {
        "null".to_owned()
    };
    let sql = format!(
        "select rowid, {}, {} from {}",
        id_column,
        quote_identifier("name"),
        quote_identifier(table)
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([], |row| {
        let rowid = row.get::<_, i64>(0)?;
        let id = sqlite_value_to_optional_i64(row.get_ref(1)?)?;
        let name = sqlite_value_to_string(row.get_ref(2)?)?;
        Ok((rowid, id, name))
    })?;
    let mut index = LvedMediaIndex::default();
    for row in rows {
        let (rowid, id, name) = row?;
        if let Some(id) = id {
            index.by_id.entry(id).or_insert(rowid);
        }
        insert_lved_media_name(&mut index, &name, rowid);
    }
    Ok(index)
}

fn insert_lved_media_name(index: &mut LvedMediaIndex, name: &str, rowid: i64) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    index.by_name.entry(name.to_owned()).or_insert(rowid);
    let lowercase = name.to_ascii_lowercase();
    if lowercase != name {
        index.by_name.entry(lowercase).or_insert(rowid);
    }
}

fn lved_media_lookup_names(key: &str) -> Vec<String> {
    let mut names = Vec::new();
    let normalized = key.trim().replace('\\', "/");
    push_unique_string(&mut names, key.trim());
    push_unique_string(&mut names, &normalized);

    let path = Path::new(&normalized);
    if let Some(file_name) = path.file_name().and_then(|value| value.to_str()) {
        push_unique_string(&mut names, file_name);
        if let Some(stem) = Path::new(file_name)
            .file_stem()
            .and_then(|value| value.to_str())
        {
            push_unique_string(&mut names, stem);
            push_common_lved_media_stem_variants(&mut names, stem);
        }
    }
    if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
        push_unique_string(&mut names, stem);
        push_common_lved_media_stem_variants(&mut names, stem);
    }

    let originals = names.clone();
    for value in originals {
        push_unique_string(&mut names, &value.to_ascii_lowercase());
    }
    names
}

fn push_common_lved_media_stem_variants(names: &mut Vec<String>, stem: &str) {
    for suffix in ["_C", "_c", "_W", "_w"] {
        if let Some(base) = stem.strip_suffix(suffix) {
            push_unique_string(names, base);
        }
    }
}

fn lved_media_lookup_ids(key: &str) -> Vec<i64> {
    let normalized = key.trim().replace('\\', "/");
    let mut ids = Vec::new();
    let Some(stem) = Path::new(&normalized)
        .file_stem()
        .and_then(|value| value.to_str())
    else {
        return ids;
    };

    if let Some((_, tail)) = stem.rsplit_once('_')
        && tail.bytes().all(|byte| byte.is_ascii_digit())
        && let Ok(id) = tail.parse::<i64>()
    {
        ids.push(id);
    }

    let bytes = stem.as_bytes();
    if bytes.len() > 2
        && matches!(bytes[0], b'z' | b'Z')
        && bytes[1].is_ascii_alphabetic()
        && bytes[2..].iter().all(u8::is_ascii_digit)
        && let Ok(id) = stem[2..].parse::<i64>()
        && !ids.contains(&id)
    {
        ids.push(id);
    }
    ids
}

fn push_unique_string(values: &mut Vec<String>, value: &str) {
    if !value.is_empty() && !values.iter().any(|existing| existing == value) {
        values.push(value.to_owned());
    }
}

fn ios_lved_sqlcipher_info_for_payload(payload_path: &Path) -> Result<Option<AndroidDictInfo>> {
    let root = payload_path.parent().unwrap_or(payload_path);
    let Some(info) = discover_ios_dictlist_info(root)? else {
        return Ok(None);
    };
    let payload_canonical = fs::canonicalize(payload_path).ok();
    for payload in info.fts_payloads {
        let same_path = payload.absolute_path == payload_path
            || payload_canonical.as_ref().is_some_and(|canonical| {
                fs::canonicalize(&payload.absolute_path).ok().as_ref() == Some(canonical)
            });
        if !same_path {
            continue;
        }
        let Some(dict_id) = payload.dict_id else {
            continue;
        };
        if payload.dict_code.is_empty() {
            continue;
        }
        let title = payload.dictionary_name.unwrap_or_default();
        return Ok(Some(AndroidDictInfo {
            dict_id,
            dict_code: payload.dict_code,
            title: title.clone(),
            name: title,
            fonts: Vec::new(),
        }));
    }
    Ok(None)
}

fn push_unique_i64(values: &mut Vec<i64>, value: i64) {
    if !values.contains(&value) {
        values.push(value);
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
        if regular_file_inside_root(root, &path)?
            && path
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

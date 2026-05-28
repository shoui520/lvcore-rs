use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use encoding_rs::SHIFT_JIS;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::search::SearchMode;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LvedKeyFile {
    pub path: PathBuf,
    pub match_kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LvedSqliteStore {
    pub payload_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_file: Option<LvedKeyFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub android_info: Option<AndroidDictInfo>,
}

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
    pub data_id: i64,
    pub level: u32,
    pub label: String,
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
        let connection = self.open_readonly()?;
        sqlite_table_names(&connection)
    }

    pub fn title(&self) -> Result<Option<String>> {
        if let Some(title) = self
            .android_info
            .as_ref()
            .and_then(|info| nonempty_string(info.title.clone()))
        {
            return Ok(Some(title));
        }
        if let Some(title) = self.tree_index_title()? {
            return Ok(Some(title));
        }
        let connection = self.open_readonly()?;
        Ok(lved_sqlite_title_from_connection(&connection))
    }

    pub fn summary(&self) -> Result<LvedSqliteSummary> {
        let connection = self.open_readonly()?;
        let title = self
            .android_info
            .as_ref()
            .and_then(|info| nonempty_string(info.title.clone()));
        let title = match title {
            Some(title) => Some(title),
            None => self
                .tree_index_title()?
                .or_else(|| lved_sqlite_title_from_connection(&connection)),
        };
        Ok(LvedSqliteSummary {
            title,
            list_available: lved_list_available(&connection)?,
            info_available: lved_info_available(&connection)?,
            tree_available: self.tree_index_path().is_some(),
        })
    }

    pub fn search_modes(&self) -> Result<Vec<SearchMode>> {
        let connection = self.open_readonly()?;
        lved_available_search_modes(&connection)
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
        let connection = self.open_readonly()?;
        search_lved_sqlite_connection(&connection, query, mode, offset, limit)
    }

    pub fn content_html(&self, content_id: i64) -> Result<Option<String>> {
        let connection = self.open_readonly()?;
        if !sqlite_table_exists(&connection, "content")
            || !sqlite_table_has_columns(&connection, "content", &["id", "body"])
        {
            return Ok(None);
        }
        let mut statement = connection.prepare("select body from content where id = ? limit 1")?;
        let mut rows = statement.query([content_id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(sqlite_value_to_string(row.get_ref(0)?)?))
    }

    pub fn info_html(&self, row_id: i64) -> Result<Option<String>> {
        let connection = self.open_readonly()?;
        if !sqlite_table_exists(&connection, "info")
            || !sqlite_table_has_columns(&connection, "info", &["id", "body"])
        {
            return Ok(None);
        }
        let mut statement =
            connection.prepare("select body from info where id = ? or rowid = ? limit 1")?;
        let mut rows = statement.query((row_id, row_id))?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(sqlite_value_to_string(row.get_ref(0)?)?))
    }

    pub fn info_html_by_name(&self, name: &str) -> Result<Option<String>> {
        let connection = self.open_readonly()?;
        if !sqlite_table_exists(&connection, "info")
            || !sqlite_table_has_columns(&connection, "info", &["name", "body"])
        {
            return Ok(None);
        }
        let mut statement = connection.prepare("select body from info where name = ? limit 1")?;
        let mut rows = statement.query([name])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(sqlite_value_to_string(row.get_ref(0)?)?))
    }

    pub fn named_html_by_name(&self, table: &str, name: &str) -> Result<Option<String>> {
        let connection = self.open_readonly()?;
        if !is_safe_sqlite_identifier(table)
            || !sqlite_table_exists(&connection, table)
            || !sqlite_table_has_columns(&connection, table, &["name", "body"])
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
    }

    pub fn info_pages(&self, limit: usize) -> Result<Vec<LvedInfoPage>> {
        self.info_pages_page(0, limit)
    }

    pub fn info_pages_page(&self, offset: usize, limit: usize) -> Result<Vec<LvedInfoPage>> {
        let connection = self.open_readonly()?;
        if limit == 0
            || !sqlite_table_exists(&connection, "info")
            || !sqlite_table_has_columns(&connection, "info", &["id", "name", "body"])
        {
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
    }

    pub fn list_items(&self, limit: usize) -> Result<Vec<LvedListItem>> {
        self.list_items_page(0, limit)
    }

    pub fn list_items_page(&self, offset: usize, limit: usize) -> Result<Vec<LvedListItem>> {
        let connection = self.open_readonly()?;
        let list_columns = sqlite_columns(&connection, "list")?;
        if limit == 0 || !has_column(&list_columns, "id") || !has_column(&list_columns, "refid") {
            return Ok(Vec::new());
        }
        let rows = lved_list_hits_by_id_clause_offset(
            &connection,
            &list_columns,
            "1 = ?",
            "l.id",
            1,
            limit,
            offset,
        )?;
        Ok(rows.into_iter().map(LvedListItem::from).collect())
    }

    pub fn tree_index_items(&self) -> Result<Vec<LvedTreeIndexItem>> {
        let Some(path) = self.tree_index_path() else {
            return Ok(Vec::new());
        };
        parse_lved_tree_index(&fs::read(path)?)
    }

    pub fn tree_index_path(&self) -> Option<PathBuf> {
        let root = self.payload_path.parent()?;
        let candidates = [root.join("res/tree.idx"), root.join("tree.idx")];
        candidates.into_iter().find(|path| path.is_file())
    }

    pub fn tree_index_title(&self) -> Result<Option<String>> {
        let Some(path) = self.tree_index_path() else {
            return Ok(None);
        };
        let rows = parse_lved_tree_index(&fs::read(path)?)?;
        Ok(rows.into_iter().find_map(|row| {
            (row.level == 0)
                .then_some(row.label)
                .filter(|label| usable_lved_tree_title(label))
        }))
    }

    pub fn media_blob(&self, store: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let connection = self.open_readonly()?;
        let table = match store {
            "lved.media" | "media" => "media",
            "lved.mediasub" | "mediasub" => "mediasub",
            _ => return Ok(None),
        };
        if !sqlite_table_exists(&connection, table)
            || !sqlite_table_has_columns(&connection, table, &["name", "main"])
        {
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
    }

    pub fn list_window_for_content(
        &self,
        content_id: i64,
        before: usize,
        after: usize,
    ) -> Result<Option<LvedListWindow>> {
        let connection = self.open_readonly()?;
        let list_columns = sqlite_columns(&connection, "list")?;
        if !has_column(&list_columns, "id") || !has_column(&list_columns, "refid") {
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
            &connection,
            &list_columns,
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
            &connection,
            &list_columns,
            "l.id < ?",
            "l.id desc",
            center_list_id,
            before,
        )?;
        before_rows.reverse();
        let after_rows = lved_list_hits_by_id_clause(
            &connection,
            &list_columns,
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
    }
}

fn lved_list_available(connection: &Connection) -> Result<bool> {
    let list_columns = sqlite_columns(connection, "list")?;
    if !has_column(&list_columns, "id") || !has_column(&list_columns, "refid") {
        return Ok(false);
    }
    connection
        .query_row("select 1 from list limit 1", [], |_| Ok(()))
        .optional()
        .map(|value| value.is_some())
        .map_err(Error::from)
}

fn lved_info_available(connection: &Connection) -> Result<bool> {
    if !sqlite_table_exists(connection, "info")
        || !sqlite_table_has_columns(connection, "info", &["id", "name", "body"])
    {
        return Ok(false);
    }
    connection
        .query_row("select 1 from info limit 1", [], |_| Ok(()))
        .optional()
        .map(|value| value.is_some())
        .map_err(Error::from)
}

fn parse_lved_tree_index(bytes: &[u8]) -> Result<Vec<LvedTreeIndexItem>> {
    let text = decode_sqlite_text(bytes);
    let mut items = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r').trim_start_matches('\u{feff}');
        if line.trim().is_empty() {
            continue;
        }
        let mut columns = line.splitn(3, '\t');
        let Some(data_id) = columns.next().and_then(|value| value.parse::<i64>().ok()) else {
            continue;
        };
        let Some(level) = columns.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let Some(label) = columns.next() else {
            continue;
        };
        items.push(LvedTreeIndexItem {
            data_id,
            level,
            label: label.to_owned(),
        });
    }
    Ok(items)
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

pub fn lved_payload_path(root: &Path) -> Result<Option<PathBuf>> {
    if root.is_file() && is_lved_payload_name(root) {
        return Ok(Some(root.to_path_buf()));
    }
    if !root.is_dir() {
        return Ok(None);
    }
    let main_data = root.join("main.data");
    if main_data.is_file() {
        return Ok(Some(main_data));
    }
    let mut dbc_files = files_with_suffix(root, ".dbc")?;
    dbc_files.sort();
    if let Some(path) = dbc_files.into_iter().next() {
        return Ok(Some(path));
    }
    let mut db_files = fs::read_dir(root)?
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_lved_payload_name(path))
        .collect::<Vec<_>>();
    db_files.sort();
    Ok(db_files.into_iter().next())
}

pub fn is_lved_payload_name(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    name == "main.data" || name.ends_with(".dbc") || is_android_lved_sqlcipher_payload(path)
}

pub fn is_android_lved_sqlcipher_payload(path: &Path) -> bool {
    let Some(extension) = path.extension() else {
        return false;
    };
    if !extension.eq_ignore_ascii_case("db") {
        return false;
    }
    if path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("thumbs.db"))
    {
        return false;
    }
    let Some(stem) = path
        .file_stem()
        .map(|value| normalize_lved_dict_code(&value.to_string_lossy()))
    else {
        return false;
    };
    let Some(parent) = path
        .parent()
        .and_then(|parent| parent.file_name())
        .map(|value| normalize_lved_dict_code(&value.to_string_lossy()))
    else {
        return false;
    };
    if stem.is_empty() || stem != parent {
        return false;
    }
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if metadata.len() == 0 || metadata.len() % 4096 != 0 {
        return false;
    }
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut header = [0_u8; 16];
    if file.read_exact(&mut header).is_err() {
        return false;
    }
    if &header == b"SQLite format 3\0" {
        return false;
    }
    let Some(root) = path.parent() else {
        return false;
    };
    root.join("resource/conf.ini").is_file() || root.join("resource/property.data").is_file()
}

pub fn infer_lved_dict_code(payload_path: &Path) -> Option<String> {
    if is_android_lved_sqlcipher_payload(payload_path) {
        return payload_path
            .file_stem()
            .map(|name| normalize_lved_dict_code(&name.to_string_lossy()));
    }
    if payload_path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("main.data"))
    {
        return payload_path
            .parent()
            .and_then(|parent| parent.file_name())
            .map(|name| normalize_lved_dict_code(&name.to_string_lossy()));
    }
    payload_path
        .file_stem()
        .map(|name| normalize_lved_dict_code(&name.to_string_lossy()))
}

pub fn derive_android_lved_sqlcipher_key(dict_id: i64, dict_code: &str) -> String {
    let code = normalize_lved_dict_code(dict_code);
    let mut chars = code.chars();
    let first = chars.next().unwrap_or_default();
    let last = code.chars().last().unwrap_or(first);
    let key_code = format!("{first}{last}").to_lowercase();
    format!("jlasgoiahoiampvsjhosDHfopj{}{}", key_code, dict_id * 19286)
}

pub fn android_dictinfo_for_payload(path: &Path) -> Result<Option<AndroidDictInfo>> {
    let Some(dict_code) = infer_lved_dict_code(path) else {
        return Ok(None);
    };
    for info_path in discover_android_dictinfo_files(path) {
        let rows = parse_android_dictinfo(&info_path)?;
        if let Some(row) = rows.into_iter().find(|row| row.dict_code == dict_code) {
            return Ok(Some(row));
        }
    }
    Ok(None)
}

pub fn parse_android_dictinfo(path: &Path) -> Result<Vec<AndroidDictInfo>> {
    let xml = fs::read_to_string(path)?;
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);

    let mut rows = Vec::new();
    let mut current = None::<AndroidDictInfoBuilder>;
    let mut current_field = None::<AndroidDictInfoField>;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.name().as_ref() == b"dict" => {
                current = Some(android_dictinfo_builder_from_event(&reader, &event)?);
                current_field = None;
            }
            Ok(Event::Start(event)) if current.is_some() => {
                current_field = AndroidDictInfoField::from_name(event.name().as_ref());
            }
            Ok(Event::Text(text)) => {
                if let (Some(builder), Some(field)) = (&mut current, current_field) {
                    let value = text.xml_content().map_err(|error| {
                        Error::Driver(format!(
                            "Android dictinfo.xml text decode error at byte {}: {error}",
                            reader.buffer_position()
                        ))
                    })?;
                    if !value.trim().is_empty() {
                        builder.push_field(field, value.into_owned());
                    }
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if let (Some(builder), Some(field)) = (&mut current, current_field)
                    && let Some(value) = decode_xml_reference(reference.as_ref())
                {
                    builder.push_field(field, value);
                }
            }
            Ok(Event::End(event)) if event.name().as_ref() == b"dict" => {
                if let Some(row) = current.take().and_then(AndroidDictInfoBuilder::finish) {
                    rows.push(row);
                }
                current_field = None;
            }
            Ok(Event::End(_)) => current_field = None,
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(Error::Driver(format!(
                    "Android dictinfo.xml parse error at byte {}: {error}",
                    reader.buffer_position()
                )));
            }
            _ => {}
        }
    }
    Ok(rows)
}

#[derive(Debug, Default)]
struct AndroidDictInfoBuilder {
    dict_id: Option<i64>,
    dict_code: Option<String>,
    title: Option<String>,
    name: Option<String>,
    fonts: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum AndroidDictInfoField {
    Code,
    Title,
    Name,
    Font,
}

impl AndroidDictInfoField {
    fn from_name(name: &[u8]) -> Option<Self> {
        match name {
            b"code" => Some(Self::Code),
            b"title" => Some(Self::Title),
            b"name" => Some(Self::Name),
            b"font" | b"multi_font" | b"font_bold" => Some(Self::Font),
            _ => None,
        }
    }
}

impl AndroidDictInfoBuilder {
    fn push_field(&mut self, field: AndroidDictInfoField, value: String) {
        match field {
            AndroidDictInfoField::Code => append_android_dictinfo_field(&mut self.dict_code, value),
            AndroidDictInfoField::Title => append_android_dictinfo_field(&mut self.title, value),
            AndroidDictInfoField::Name => append_android_dictinfo_field(&mut self.name, value),
            AndroidDictInfoField::Font => self.fonts.push(value),
        }
    }

    fn finish(self) -> Option<AndroidDictInfo> {
        let dict_id = self.dict_id?;
        let dict_code = normalize_lved_dict_code(&self.dict_code?);
        if dict_code.is_empty() {
            return None;
        }
        let title = self.title.unwrap_or_default();
        let name = self.name.unwrap_or_else(|| title.clone());
        Some(AndroidDictInfo {
            dict_id,
            dict_code,
            title,
            name,
            fonts: self.fonts,
        })
    }
}

fn android_dictinfo_builder_from_event(
    reader: &Reader<&[u8]>,
    event: &BytesStart<'_>,
) -> Result<AndroidDictInfoBuilder> {
    let mut builder = AndroidDictInfoBuilder::default();
    for attr in event.attributes().flatten() {
        match attr.key.as_ref() {
            b"id" => {
                let value = attr
                    .decode_and_unescape_value(reader.decoder())
                    .map_err(|error| Error::Driver(format!("invalid Android dict id: {error}")))?;
                builder.dict_id = value.trim().parse::<i64>().ok();
            }
            b"name" => {
                let value = attr
                    .decode_and_unescape_value(reader.decoder())
                    .map_err(|error| {
                        Error::Driver(format!("invalid Android dict name: {error}"))
                    })?;
                builder.name = Some(value.trim().to_owned());
            }
            _ => {}
        }
    }
    Ok(builder)
}

fn append_android_dictinfo_field(slot: &mut Option<String>, value: String) {
    match slot {
        Some(existing) => existing.push_str(&value),
        None => *slot = Some(value),
    }
}

fn decode_xml_reference(value: &[u8]) -> Option<String> {
    let value = std::str::from_utf8(value).ok()?;
    let decoded = match value {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ if value.starts_with("#x") => {
            let code = u32::from_str_radix(&value[2..], 16).ok()?;
            char::from_u32(code)?
        }
        _ if value.starts_with('#') => {
            let code = value[1..].parse::<u32>().ok()?;
            char::from_u32(code)?
        }
        _ => return None,
    };
    Some(decoded.to_string())
}

fn discover_android_dictinfo_files(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = Vec::<PathBuf>::new();
    let mut current = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    };

    for _ in 0..6 {
        push_android_dictinfo_candidate(&current.join("dictinfo.xml"), &mut out, &mut seen);
        for child_name in ["android viewer", "resources", "res", "xml"] {
            let child = current.join(child_name);
            if child.is_dir() {
                collect_dictinfo_recursive(&child, &mut out, &mut seen);
            }
        }
        let child_names = match fs::read_dir(&current) {
            Ok(entries) => entries
                .filter_map(std::result::Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
                .collect::<Vec<_>>(),
            Err(_) => {
                let Some(parent) = current.parent() else {
                    break;
                };
                current = parent.to_path_buf();
                continue;
            }
        };
        if out.is_empty()
            && child_names
                .iter()
                .any(|name| matches!(name.as_str(), "sqlite" | "ssed"))
        {
            collect_dictinfo_recursive(&current, &mut out, &mut seen);
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent.to_path_buf();
    }

    out.sort();
    out
}

fn collect_dictinfo_recursive(root: &Path, out: &mut Vec<PathBuf>, seen: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_dictinfo_recursive(&path, out, seen);
        } else if path
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("dictinfo.xml"))
        {
            push_android_dictinfo_candidate(&path, out, seen);
        }
    }
}

fn push_android_dictinfo_candidate(path: &Path, out: &mut Vec<PathBuf>, seen: &mut Vec<PathBuf>) {
    if !path.is_file() {
        return;
    }
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if seen.iter().any(|seen_path| seen_path == &resolved) {
        return;
    }
    seen.push(resolved);
    out.push(path.to_path_buf());
}

pub fn discover_lved_key_file(payload_path: &Path) -> Result<Option<LvedKeyFile>> {
    let parent = payload_path.parent().unwrap_or_else(|| Path::new("."));
    let mut candidates = Vec::new();
    if let Some(dict_code) = infer_lved_dict_code(payload_path) {
        candidates.push((
            parent.join(format!("{dict_code}.key")),
            "inferred_dict_code",
        ));
        candidates.push((
            parent.join(format!("{}.key", dict_code.to_lowercase())),
            "inferred_dict_code_lower",
        ));
    }
    if payload_path.extension().is_some() {
        candidates.push((
            payload_path.with_extension(format!(
                "{}.key",
                payload_path
                    .extension()
                    .map(|value| value.to_string_lossy())
                    .unwrap_or_default()
            )),
            "payload_name",
        ));
    }
    if let Some(stem) = payload_path.file_stem() {
        candidates.push((
            parent.join(format!("{}.key", stem.to_string_lossy())),
            "payload_stem",
        ));
    }

    let mut seen = Vec::<PathBuf>::new();
    for (path, match_kind) in candidates {
        let resolved = path.canonicalize().unwrap_or_else(|_| path.clone());
        if seen.iter().any(|item| item == &resolved) {
            continue;
        }
        seen.push(resolved);
        if path.is_file() {
            return Ok(Some(LvedKeyFile {
                path,
                match_kind: match_kind.to_owned(),
            }));
        }
    }

    let mut key_files = files_with_suffix(parent, ".key")?;
    key_files.sort();
    if key_files.len() == 1 {
        return Ok(Some(LvedKeyFile {
            path: key_files.remove(0),
            match_kind: "single_key_in_payload_dir".to_owned(),
        }));
    }
    Ok(None)
}

pub fn read_lved_key_file(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)?.trim().to_owned())
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
    query: &str,
    mode: &SearchMode,
    offset: usize,
    limit: usize,
) -> Result<Vec<LvedSearchHit>> {
    if !sqlite_table_exists(connection, "search") || !sqlite_table_exists(connection, "list") {
        return Ok(Vec::new());
    }
    let search_columns = sqlite_columns(connection, "search")?;
    let list_columns = sqlite_columns(connection, "list")?;
    if !has_column(&list_columns, "id") || !has_column(&list_columns, "refid") {
        return Ok(Vec::new());
    }
    let normalized = normalize_lved_query(query);
    if normalized.is_empty() {
        return Ok(Vec::new());
    }
    let Some((where_clause, parameter)) = lved_search_where(&normalized, mode, &search_columns)
    else {
        return Ok(Vec::new());
    };

    let anchor_column = optional_column_expr(&list_columns, "anchor", "''");
    let title_column = optional_column_expr(&list_columns, "title", "''");
    let subtitle_column = if has_column(&list_columns, "titlesub") {
        "l.titlesub"
    } else if has_column(&list_columns, "subtext") {
        "l.subtext"
    } else if has_column(&list_columns, "titleplain") {
        "l.titleplain"
    } else {
        "''"
    };
    let type_column = optional_column_expr(&list_columns, "type", "null");
    let sql = format!(
        "select l.id, l.refid, {anchor_column}, {title_column}, {subtitle_column}, {type_column} \
         from search s join list l on l.id = s.rowid where {where_clause} order by l.id limit ? offset ?"
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
    let anchor_column = optional_column_expr(list_columns, "anchor", "''");
    let title_column = optional_column_expr(list_columns, "title", "''");
    let subtitle_column = if has_column(list_columns, "titlesub") {
        "l.titlesub"
    } else if has_column(list_columns, "subtext") {
        "l.subtext"
    } else if has_column(list_columns, "titleplain") {
        "l.titleplain"
    } else {
        "''"
    };
    let type_column = optional_column_expr(list_columns, "type", "null");
    let sql = format!(
        "select l.id, l.refid, {anchor_column}, {title_column}, {subtitle_column}, {type_column} \
         from list l where {where_clause} order by {order_clause} limit ? offset ?"
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

fn lved_available_search_modes(connection: &Connection) -> Result<Vec<SearchMode>> {
    if !sqlite_table_exists(connection, "search") || !sqlite_table_exists(connection, "list") {
        return Ok(Vec::new());
    }
    let search_columns = sqlite_columns(connection, "search")?;
    let list_columns = sqlite_columns(connection, "list")?;
    if !has_column(&list_columns, "id") || !has_column(&list_columns, "refid") {
        return Ok(Vec::new());
    }
    let mut modes = Vec::new();
    if has_column(&search_columns, "filter") || has_column(&search_columns, "forward") {
        modes.push(SearchMode::Exact);
    }
    if has_column(&search_columns, "forward") {
        modes.push(SearchMode::Forward);
    }
    if has_column(&search_columns, "back") {
        modes.push(SearchMode::Backward);
    }
    if has_column(&search_columns, "part") {
        modes.push(SearchMode::Partial);
    }
    if has_column(&search_columns, "fts") {
        modes.push(SearchMode::FullText);
    }
    modes.extend(
        search_columns
            .into_iter()
            .filter(|column| column.starts_with("advanced"))
            .map(SearchMode::Advanced),
    );
    Ok(modes)
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

fn lved_sqlite_title_from_connection(connection: &Connection) -> Option<String> {
    if !sqlite_table_exists(connection, "info")
        || !sqlite_table_has_columns(connection, "info", &["name", "body"])
    {
        return None;
    }
    let mut statement = connection
        .prepare(
            "
            select name, body from info
            where body is not null and body != ''
            order by
              case
                when body like '%book_title%' then 0
                when body like '%凡例書籍名%' then 0
                when body like '%著作権表示%' then 0
                when lower(name) like '%copyright%' then 0
                when lower(name) like '%about%' then 0
                when lower(name) like '%hanrei%' then 1
                when lower(name) like '%license%' then 3
                when lower(name) = 'index.html' then 4
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
        let body_text = html_text_lines(&body);
        let mut row_candidates = Vec::new();
        for line in body_text.into_iter().take(24) {
            if let Some(candidate) = normalize_title_candidate(&line) {
                row_candidates.push(candidate);
            }
        }
        for candidate in row_candidates {
            let score = title_score(&candidate, &name);
            if score >= 100 {
                candidates.push((score, index, candidate));
            }
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    candidates.into_iter().next().map(|(_, _, title)| title)
}

fn sqlite_table_exists(connection: &Connection, table: &str) -> bool {
    connection
        .query_row(
            "select 1 from sqlite_master where type in ('table', 'view') and lower(name) = lower(?) limit 1",
            [table],
            |_| Ok(()),
        )
        .is_ok()
}

fn sqlite_table_has_columns(connection: &Connection, table: &str, required: &[&str]) -> bool {
    let Ok(columns) = sqlite_columns(connection, table) else {
        return false;
    };
    required.iter().all(|column| has_column(&columns, column))
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
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .lines()
        .map(|line| {
            line.trim_matches(|ch: char| ch.is_whitespace() || "　・●◆".contains(ch))
                .to_owned()
        })
        .filter(|line| !line.is_empty())
        .collect()
}

fn html_to_text(fragment: &str) -> String {
    html_text_lines(fragment).join(" ")
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

fn title_score(value: &str, source_name: &str) -> i32 {
    let mut score = 0;
    for keyword in [
        "辞典",
        "事典",
        "辞書",
        "字典",
        "大辞典",
        "広辞苑",
        "大辞林",
        "シソーラス",
        "リーダーズ",
        "ロワイヤル",
        "百科",
        "英和",
        "和英",
        "法律",
        "医学",
        "数学",
        "Dictionary",
        "Thesaurus",
        "Encyclopedia",
    ] {
        if value.contains(keyword) {
            score += 120;
            break;
        }
    }
    if value.contains('版') {
        score += 20;
    }
    let source_name = source_name.to_lowercase();
    let source_basename = source_name.rsplit('/').next().unwrap_or(&source_name);
    if source_name.contains("about") || matches!(source_basename, "index.html" | "index.htm") {
        score += 40;
    }
    if source_name.contains("copyright") {
        score += 70;
    }
    if source_name.contains("license") {
        score += 20;
    }
    for weak in [
        "凡例",
        "索引",
        "一覧",
        "インデックス",
        "目次",
        "使い方",
        "著作権",
        "記号",
        "略語",
    ] {
        if value.contains(weak) {
            score -= 90;
        }
    }
    if value.ends_with("小辞典") && !value.contains('第') {
        score -= 80;
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
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn discovers_dict_code_key_file_for_main_data() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("main.data"), b"encrypted").unwrap();
        fs::write(dir.path().join("TEST.key"), b"secret").unwrap();
        let payload = dir.path().join("main.data");

        let key = discover_lved_key_file(&payload).unwrap().unwrap();

        assert_eq!(key.path.file_name().unwrap(), "TEST.key");
        assert_eq!(read_lved_key_file(&key.path).unwrap(), "secret");
    }

    #[test]
    fn discovers_android_lved_payload_and_uses_dictinfo_key() {
        let dir = tempdir().unwrap();
        let package = dir.path().join("SQLite/.TESTDICT");
        fs::create_dir_all(package.join("resource")).unwrap();
        fs::write(package.join("resource/conf.ini"), b"").unwrap();
        fs::create_dir_all(dir.path().join("android viewer/res/xml")).unwrap();
        fs::write(
            dir.path().join("android viewer/res/xml/dictinfo.xml"),
            r#"
            <dictinfo>
              <dict id="750" name="TESTDICT">
                <code>TESTDICT</code>
                <title>Android&#x20;&amp;&#x20;Test Dictionary</title>
                <fonts use="1"><font>ipamp</font></fonts>
              </dict>
            </dictinfo>
            "#,
        )
        .unwrap();
        let payload = package.join("TESTDICT.db");
        let key = derive_android_lved_sqlcipher_key(750, "TESTDICT");
        {
            let connection = Connection::open(&payload).unwrap();
            apply_sqlcipher_key(&connection, &key).unwrap();
            connection
                .execute_batch(
                    "
                    create table info (id integer, type integer, name text primary key, body text, media text);
                    insert into info values (1, 1, 'about.html', '<h1>Wrong fallback title</h1>', '');
                    create table content (id integer primary key, type integer, body text, media text);
                    create table list (id integer primary key, refid integer, type integer, anchor text, title text, titlesub text);
                    create virtual table search using fts4(forward, back, part, fts, advanced1, advanced2, filter);
                    insert into content values (100, 1, '<article>body</article>', '');
                    insert into list values (1, 100, 1, '', '<b>alpha</b>', '');
                    insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                      values (1, 'alpha', 'ahpla', 'alpha', 'alpha body', '', '', '∥alpha∥');
                    ",
                )
                .unwrap();
        }

        assert!(is_lved_payload_name(&payload));
        let store = LvedSqliteStore::discover(&package).unwrap().unwrap();
        assert!(store.key_file.is_none());
        assert_eq!(
            store.android_info.as_ref().map(|info| info.dict_id),
            Some(750)
        );
        assert_eq!(
            store.title().unwrap().as_deref(),
            Some("Android & Test Dictionary")
        );
        assert_eq!(
            store.search("alp", &SearchMode::Forward, 10).unwrap()[0].title_text,
            "alpha"
        );
    }

    #[test]
    fn android_lved_payload_detection_rejects_plaintext_helper_db() {
        let dir = tempdir().unwrap();
        let package = dir.path().join(".HELPER");
        fs::create_dir_all(package.join("resource")).unwrap();
        fs::write(package.join("resource/conf.ini"), b"").unwrap();
        let payload = package.join("HELPER.db");
        Connection::open(&payload).unwrap();

        assert!(!is_lved_payload_name(&payload));
    }

    #[test]
    fn opens_sqlcipher_payload_and_extracts_title() {
        let dir = tempdir().unwrap();
        let payload = dir.path().join("main.data");
        let key = "test-key";
        {
            let connection = Connection::open(&payload).unwrap();
            apply_sqlcipher_key(&connection, key).unwrap();
            connection
                .execute_batch(
                    "
                    create table info (id integer, type integer, name text primary key, body text, media text);
                    insert into info values (1, 1, 'about.html', '<h1>Example Dictionary 第2版</h1>', '');
                    ",
                )
                .unwrap();
        }
        fs::write(dir.path().join("main.key"), key).unwrap();

        let store = LvedSqliteStore::discover(dir.path()).unwrap().unwrap();

        assert_eq!(store.table_names().unwrap(), vec!["info".to_owned()]);
        assert_eq!(
            store.title().unwrap().as_deref(),
            Some("Example Dictionary 第2版")
        );
    }

    #[test]
    fn searches_lved_list_rows_and_preserves_content_html() {
        let dir = tempdir().unwrap();
        let payload = dir.path().join("main.data");
        let key = "test-key";
        {
            let connection = Connection::open(&payload).unwrap();
            apply_sqlcipher_key(&connection, key).unwrap();
            connection
                .execute_batch(
                    "
                    create table info (id integer, type integer, name text primary key, body text, media text);
                    create table content (id integer primary key, type integer, body text, media text);
                    create table mediasub (id integer primary key, name text, type integer, main blob);
                    create table list (
                      id integer primary key,
                      refid integer,
                      type integer,
                      anchor text,
                      title text,
                      titlesub text
                    );
                    create virtual table search using fts4(
                      forward,
                      back,
                      part,
                      fts,
                      advanced1,
                      advanced2,
                      filter
                    );
                    insert into info values (1, 1, 'about.html', '<h1>Example Dictionary 第2版</h1>', '');
                    insert into content values (100, 1, '<article><h1>Alpha</h1><p>body</p></article>', '');
                    insert into content values (101, 1, '<article><h1>Beta</h1></article>', '');
                    insert into content values (102, 1, '<article><h1>Gamma</h1></article>', '');
                    insert into mediasub values (1, '00010033', 5, X'49443303');
                    insert into list values (1, 100, 1, 'body-anchor', '<b>alpha</b>', '<span>subtitle</span>');
                    insert into list values (2, 101, 1, '', '<b>beta</b>', '');
                    insert into list values (3, 102, 1, '', '<b>gamma</b>', '');
                    insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                      values (1, 'alpha', 'ahpla', 'alpha', 'alpha body', 'topic marker', '', '∥alpha∥');
                    ",
                )
                .unwrap();
        }
        fs::write(dir.path().join("main.key"), key).unwrap();
        let store = LvedSqliteStore::discover(dir.path()).unwrap().unwrap();
        assert_eq!(
            store.search_modes().unwrap(),
            vec![
                SearchMode::Exact,
                SearchMode::Forward,
                SearchMode::Backward,
                SearchMode::Partial,
                SearchMode::FullText,
                SearchMode::Advanced("advanced1".to_owned()),
                SearchMode::Advanced("advanced2".to_owned()),
            ]
        );

        let hits = store.search("alp", &SearchMode::Forward, 10).unwrap();

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].content_id, 100);
        let advanced_hits = store
            .search("topic", &SearchMode::Advanced("advanced1".to_owned()), 10)
            .unwrap();
        assert_eq!(advanced_hits.len(), 1);
        assert_eq!(advanced_hits[0].content_id, 100);
        let missing_advanced_hits = store
            .search(
                "topic",
                &SearchMode::Advanced("missing_column".to_owned()),
                10,
            )
            .unwrap();
        assert!(missing_advanced_hits.is_empty());
        assert_eq!(hits[0].anchor.as_deref(), Some("body-anchor"));
        assert_eq!(hits[0].title_text, "alpha");
        assert_eq!(
            store.content_html(100).unwrap().as_deref(),
            Some("<article><h1>Alpha</h1><p>body</p></article>")
        );
        assert_eq!(
            store.info_html(1).unwrap().as_deref(),
            Some("<h1>Example Dictionary 第2版</h1>")
        );
        assert_eq!(
            store.info_pages(10).unwrap()[0].title_text,
            "Example Dictionary 第2版"
        );
        assert_eq!(
            store.media_blob("lved.mediasub", "00010033.mp3").unwrap(),
            Some(b"ID3\x03".to_vec())
        );
        let window = store.list_window_for_content(101, 1, 1).unwrap().unwrap();
        assert_eq!(window.before[0].title_text, "alpha");
        assert_eq!(window.center.title_text, "beta");
        assert_eq!(window.after[0].title_text, "gamma");
    }

    #[test]
    fn title_probe_rejects_common_false_positive_shapes() {
        assert!(normalize_title_candidate("外国語は片仮名で表記した．").is_none());
        assert!(title_score("和英小辞典", "index.html") < 100);
        assert_eq!(
            normalize_title_candidate("『広辞苑 第七版』　　&copy;2018年").as_deref(),
            Some("広辞苑 第七版")
        );
    }

    #[test]
    fn title_probe_prefers_index_title_over_later_bibliography_lines() {
        let index_title = normalize_title_candidate("研究社　類義語使い分け辞典 凡例").unwrap();
        let bibliography =
            normalize_title_candidate("『基礎日本語辞典』　森田良行、角川書店、1991、第 3 版")
                .unwrap();

        assert!(
            title_score(&index_title, "index.html") > title_score(&bibliography, "shuyou.html")
        );
    }

    #[test]
    fn title_probe_finds_late_book_title_and_ignores_index_labels() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "
                create table info (id integer, type integer, name text primary key, body text);
                insert into info values (
                  1, 1, 'index.html',
                  '<div class=\"title\">索引</div><div>和英インデックス</div>'
                );
                ",
            )
            .unwrap();
        for index in 0..300 {
            connection
                .execute(
                    "insert into info values (?, 1, ?, ?)",
                    (
                        index + 2,
                        format!("i{index:03}.html"),
                        format!("<div class=\"title\">CEFR-J ランク {index}</div>"),
                    ),
                )
                .unwrap();
        }
        connection
            .execute(
                "insert into info values (1000, 1, 'h04.html', ?)",
                ["<div class=\"Copyright\"><div class=\"凡例書籍名\">エースクラウン英和辞典 第4版</div></div>"],
            )
            .unwrap();

        assert_eq!(
            lved_sqlite_title_from_connection(&connection).as_deref(),
            Some("エースクラウン英和辞典 第4版")
        );
    }

    #[test]
    fn title_probe_ignores_style_blocks_and_staff_affiliations() {
        assert_eq!(
            html_text_lines(
                r#"<html><head><style>.title { color: red; }</style></head><body><div class="title">新明解国語辞典　第八版</div></body></html>"#
            ),
            vec!["新明解国語辞典　第八版".to_owned()]
        );
        assert!(
            normalize_title_candidate("浅井　昌弘　慶應義塾大学医学部　精神神経科　教授").is_none()
        );
    }
}

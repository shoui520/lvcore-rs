use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use tempfile::TempDir;

use crate::crypto::{decrypt_logofont_cipher_file_to_path, decrypt_logofont_cipher_prefix};
use crate::error::{Error, Result};
use crate::search::SearchMode;

const SQLITE_MAGIC: &[u8] = b"SQLite format 3\0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiviewMenuItem {
    pub label: String,
    pub href: Option<String>,
    pub anchor: Option<String>,
    pub children: Vec<MultiviewMenuItem>,
}

impl MultiviewMenuItem {
    fn new(label: String, href: Option<String>, anchor: Option<String>) -> Self {
        Self {
            label,
            href,
            anchor,
            children: Vec::new(),
        }
    }
}

pub fn parse_menu_data(xml: &str) -> Result<Vec<MultiviewMenuItem>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut roots = Vec::new();
    let mut stack: Vec<MultiviewMenuItem> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.name().as_ref() == b"item" => {
                stack.push(menu_item_from_event(&reader, &event)?);
            }
            Ok(Event::Empty(event)) if event.name().as_ref() == b"item" => {
                push_menu_item(
                    &mut roots,
                    &mut stack,
                    menu_item_from_event(&reader, &event)?,
                );
            }
            Ok(Event::End(event)) if event.name().as_ref() == b"item" => {
                let Some(item) = stack.pop() else {
                    return Err(Error::Driver(
                        "menuData.xml has an unmatched </item>".to_owned(),
                    ));
                };
                push_menu_item(&mut roots, &mut stack, item);
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(Error::Driver(format!(
                    "menuData.xml XML parse error at byte {}: {error}",
                    reader.buffer_position()
                )));
            }
            _ => {}
        }
    }

    if !stack.is_empty() {
        return Err(Error::Driver(
            "menuData.xml ended with unclosed <item> elements".to_owned(),
        ));
    }

    Ok(roots)
}

#[derive(Debug)]
pub struct MultiviewStore {
    payloads: Vec<MultiviewPayloadSource>,
    temp_dir: Mutex<Option<TempDir>>,
    decrypted_paths: Mutex<BTreeMap<PathBuf, PathBuf>>,
    roles: Mutex<BTreeMap<PathBuf, MultiviewPayloadRole>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiviewSearchHit {
    pub href: String,
    pub title_html: String,
    pub title_text: String,
    pub snippet_html: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiviewBody {
    pub title: String,
    pub html: String,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MultiviewPayloadRole {
    ContentSearchBody,
    LawBody,
    HtmlIndex,
    CaseDigestBody,
    LawMetadata,
    SubjectIndex,
    Unclassified,
}

#[derive(Debug)]
struct MultiviewPayloadSource {
    name: String,
    path: PathBuf,
    storage: MultiviewPayloadStorage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MultiviewPayloadStorage {
    PlainSqlite,
    LogoFontCipherSqlite,
}

impl MultiviewStore {
    pub fn discover(root: &Path) -> Result<Option<Self>> {
        let payload_paths = multiview_payload_paths(root)?;
        if payload_paths.is_empty() {
            return Ok(None);
        }
        let mut payloads = Vec::new();
        for payload_path in payload_paths {
            let storage = match multiview_payload_storage(&payload_path)? {
                Some(storage) => storage,
                None => continue,
            };
            payloads.push(MultiviewPayloadSource {
                name: payload_path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| "payload".to_owned()),
                path: payload_path,
                storage,
            });
        }
        if payloads.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            payloads,
            temp_dir: Mutex::new(None),
            decrypted_paths: Mutex::new(BTreeMap::new()),
            roles: Mutex::new(BTreeMap::new()),
        }))
    }

    pub fn search(
        &self,
        query: &str,
        mode: &SearchMode,
        limit: usize,
    ) -> Result<Vec<MultiviewSearchHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let Some(payload) = self.first_payload_by_role(MultiviewPayloadRole::ContentSearchBody)?
        else {
            return Ok(Vec::new());
        };
        let sqlite_path = self.sqlite_path_for_payload(payload)?;
        let connection = open_sqlite(&sqlite_path)?;
        if !sqlite_table_has_columns(
            &connection,
            "t_search",
            &["f_ID", "f_KeyWord", "f_TitleMain", "f_All"],
        )? {
            return Ok(Vec::new());
        }
        let (column, pattern) = match mode {
            SearchMode::Exact => ("f_KeyWord", format!("%§{query}§%")),
            SearchMode::Forward => ("f_KeyWord", format!("%§{query}%")),
            SearchMode::Backward => ("f_KeyWord", format!("%{query}§%")),
            SearchMode::Partial | SearchMode::FullText | SearchMode::Advanced(_) => {
                ("f_All", format!("%{query}%"))
            }
        };
        let operator = "like";
        let sql = format!(
            "select f_ID, f_KeyWord, f_TitleMain, f_All from t_search \
             where {column} {operator} ? order by f_No limit ?"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map((pattern, limit as i64), |row| {
            let id = sqlite_value_to_string(row.get_ref(0)?)?;
            let keyword = sqlite_value_to_string(row.get_ref(1)?)?;
            let title = sqlite_value_to_string(row.get_ref(2)?)?;
            let all = sqlite_value_to_string(row.get_ref(3)?)?;
            let href = if id.chars().all(|ch| ch.is_ascii_digit()) {
                format!("{:06}", id.parse::<i64>().unwrap_or_default())
            } else {
                id
            };
            let title_html = if title.is_empty() {
                keyword.clone()
            } else {
                title
            };
            let title_text = html_to_text(&title_html);
            Ok(MultiviewSearchHit {
                href,
                title_html,
                title_text,
                snippet_html: (!all.is_empty()).then_some(all),
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    pub fn body_for_href(&self, href: &str) -> Result<Option<MultiviewBody>> {
        if let Some(body) = self.content_body_for_href(href)? {
            return Ok(Some(body));
        }
        if let Some(body) = self.law_body_for_href(href)? {
            return Ok(Some(body));
        }
        if let Some(body) = self.html_index_body_for_href(href)? {
            return Ok(Some(body));
        }
        Ok(None)
    }

    fn content_body_for_href(&self, href: &str) -> Result<Option<MultiviewBody>> {
        let Some(payload) = self.first_payload_by_role(MultiviewPayloadRole::ContentSearchBody)?
        else {
            return Ok(None);
        };
        let Ok(content_id) = href.parse::<i64>() else {
            return Ok(None);
        };
        let sqlite_path = self.sqlite_path_for_payload(payload)?;
        let connection = open_sqlite(&sqlite_path)?;
        if !sqlite_table_has_columns(&connection, "t_contents", &["f_ID", "f_Title", "f_Body"])? {
            return Ok(None);
        }
        let row = connection
            .query_row(
                "select f_Title, f_Body from t_contents where f_ID = ? limit 1",
                [content_id],
                |row| {
                    Ok((
                        sqlite_value_to_string(row.get_ref(0)?)?,
                        sqlite_value_to_string(row.get_ref(1)?)?,
                    ))
                },
            )
            .optional()?;
        Ok(row.map(|(title, html)| MultiviewBody {
            title: html_to_text(&title),
            html,
            source: format!("{}:t_contents", payload.name),
        }))
    }

    fn law_body_for_href(&self, href: &str) -> Result<Option<MultiviewBody>> {
        let Some(payload) = self.first_payload_by_role(MultiviewPayloadRole::LawBody)? else {
            return Ok(None);
        };
        let table_hint = href.split('_').next().unwrap_or(href);
        if table_hint.is_empty() {
            return Ok(None);
        }
        let sqlite_path = self.sqlite_path_for_payload(payload)?;
        let connection = open_sqlite(&sqlite_path)?;
        let table = if sqlite_table_exists(&connection, &format!("t_{table_hint}"))? {
            Some(format!("t_{table_hint}"))
        } else {
            table_with_anchor(&connection, href)?
        };
        let Some(table) = table else {
            return Ok(None);
        };
        if !sqlite_table_has_columns(&connection, &table, &["f_text"])? {
            return Ok(None);
        }

        let rows = if sqlite_table_has_columns(&connection, &table, &["f_anchor"])? {
            query_law_rows_by_anchor(&connection, &table, href)?
        } else {
            Vec::new()
        };
        let rows = if rows.is_empty() {
            query_law_rows_by_hore_code(&connection, &table, table_hint)?
        } else {
            rows
        };
        if rows.is_empty() {
            return Ok(None);
        }
        let title = rows
            .iter()
            .find_map(|row| {
                let title = [row.title_no.as_str(), row.title_sub.as_str()]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                (!title.is_empty()).then_some(title)
            })
            .unwrap_or_else(|| href.to_owned());
        let html = rows
            .into_iter()
            .map(|row| row.text)
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Some(MultiviewBody {
            title,
            html,
            source: format!("{}:{table}", payload.name),
        }))
    }

    fn html_index_body_for_href(&self, href: &str) -> Result<Option<MultiviewBody>> {
        let code = href.strip_prefix("index:").unwrap_or(href);
        let Some(payload) = self.first_payload_by_role(MultiviewPayloadRole::HtmlIndex)? else {
            return Ok(None);
        };
        let sqlite_path = self.sqlite_path_for_payload(payload)?;
        let connection = open_sqlite(&sqlite_path)?;
        if !sqlite_table_has_columns(
            &connection,
            "t_index",
            &["f_hore_code", "f_title_no", "f_title_sub", "f_text"],
        )? {
            return Ok(None);
        }
        let rows = query_index_rows(&connection, code)?;
        if rows.is_empty() {
            return Ok(None);
        }
        let title = [rows[0].title_no.as_str(), rows[0].title_sub.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let html = rows
            .into_iter()
            .map(|row| row.text)
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Some(MultiviewBody {
            title: if title.is_empty() {
                code.to_owned()
            } else {
                title
            },
            html,
            source: format!("{}:t_index", payload.name),
        }))
    }

    fn first_payload_by_role(
        &self,
        role: MultiviewPayloadRole,
    ) -> Result<Option<&MultiviewPayloadSource>> {
        for payload in &self.payloads {
            if self.role_for_payload(payload)? == role {
                return Ok(Some(payload));
            }
        }
        Ok(None)
    }

    fn role_for_payload(&self, payload: &MultiviewPayloadSource) -> Result<MultiviewPayloadRole> {
        if let Some(role) = self
            .roles
            .lock()
            .map_err(|_| Error::Driver("multiview role cache was poisoned".to_owned()))?
            .get(&payload.path)
            .copied()
        {
            return Ok(role);
        }
        let sqlite_path = self.sqlite_path_for_payload(payload)?;
        let role = classify_sqlite_payload(&sqlite_path)?;
        self.roles
            .lock()
            .map_err(|_| Error::Driver("multiview role cache was poisoned".to_owned()))?
            .insert(payload.path.clone(), role);
        Ok(role)
    }

    fn sqlite_path_for_payload(&self, payload: &MultiviewPayloadSource) -> Result<PathBuf> {
        match payload.storage {
            MultiviewPayloadStorage::PlainSqlite => Ok(payload.path.clone()),
            MultiviewPayloadStorage::LogoFontCipherSqlite => {
                self.decrypted_sqlite_path(&payload.path)
            }
        }
    }

    fn decrypted_sqlite_path(&self, path: &Path) -> Result<PathBuf> {
        if let Some(existing) = self
            .decrypted_paths
            .lock()
            .map_err(|_| Error::Driver("multiview decrypt cache was poisoned".to_owned()))?
            .get(path)
            .cloned()
        {
            return Ok(existing);
        }
        let temp_root = {
            let mut guard = self
                .temp_dir
                .lock()
                .map_err(|_| Error::Driver("multiview temp cache was poisoned".to_owned()))?;
            if guard.is_none() {
                *guard = Some(
                    tempfile::Builder::new()
                        .prefix("lvcore-multiview-")
                        .tempdir()?,
                );
            }
            guard
                .as_ref()
                .ok_or_else(|| Error::Driver("temporary directory was not created".to_owned()))?
                .path()
                .to_path_buf()
        };
        let output = temp_root.join(
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "payload".to_owned()),
        );
        decrypt_logofont_cipher_file_to_path(path, &output)?;
        self.decrypted_paths
            .lock()
            .map_err(|_| Error::Driver("multiview decrypt cache was poisoned".to_owned()))?
            .insert(path.to_path_buf(), output.clone());
        Ok(output)
    }
}

#[derive(Debug)]
struct LawRow {
    title_no: String,
    title_sub: String,
    text: String,
}

fn menu_item_from_event(
    reader: &Reader<&[u8]>,
    event: &BytesStart<'_>,
) -> Result<MultiviewMenuItem> {
    let mut label = String::new();
    let mut href = None;
    let mut anchor = None;

    for attribute in event.attributes() {
        let attribute = attribute.map_err(|error| {
            Error::Driver(format!(
                "menuData.xml has an invalid attribute at byte {}: {error}",
                reader.buffer_position()
            ))
        })?;
        let value = attribute
            .decode_and_unescape_value(reader.decoder())
            .map_err(|error| {
                Error::Driver(format!(
                    "menuData.xml has an invalid attribute value at byte {}: {error}",
                    reader.buffer_position()
                ))
            })?
            .into_owned();
        match attribute.key.as_ref() {
            b"label" => label = value,
            b"href" => href = nonempty_value(value),
            b"anchor" => anchor = nonempty_value(value),
            _ => {}
        }
    }

    Ok(MultiviewMenuItem::new(label, href, anchor))
}

fn nonempty_value(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn push_menu_item(
    roots: &mut Vec<MultiviewMenuItem>,
    stack: &mut [MultiviewMenuItem],
    item: MultiviewMenuItem,
) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(item);
    } else {
        roots.push(item);
    }
}

fn multiview_payload_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_multiview_payload_name)
        {
            paths.push(path);
        }
    }
    paths.sort_by(|left, right| {
        left.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .cmp(
                &right
                    .file_name()
                    .map(|name| name.to_string_lossy().to_lowercase()),
            )
            .then_with(|| left.cmp(right))
    });
    Ok(paths)
}

fn is_multiview_payload_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    lower.len() == 6
        && bytes[1] == b'l'
        && bytes[2] == b'v'
        && (lower.ends_with("bat") || lower.ends_with("dat"))
}

fn multiview_payload_storage(path: &Path) -> Result<Option<MultiviewPayloadStorage>> {
    let prefix = fs::read(path).map(|bytes| bytes.into_iter().take(4096).collect::<Vec<_>>())?;
    if prefix.starts_with(SQLITE_MAGIC) {
        return Ok(Some(MultiviewPayloadStorage::PlainSqlite));
    }
    if prefix.len() < 16 || !path.metadata()?.len().is_multiple_of(16) {
        return Ok(None);
    }
    let decrypted_prefix = decrypt_logofont_cipher_prefix(&prefix, 4096)?;
    if !decrypted_prefix.starts_with(SQLITE_MAGIC) {
        return Ok(None);
    }
    Ok(Some(MultiviewPayloadStorage::LogoFontCipherSqlite))
}

fn classify_sqlite_payload(path: &Path) -> Result<MultiviewPayloadRole> {
    let connection = open_sqlite(path)?;
    let tables = sqlite_table_names(&connection)?;
    if tables.is_empty() {
        return Ok(MultiviewPayloadRole::Unclassified);
    }
    if sqlite_table_has_columns(&connection, "t_contents", &["f_ID", "f_Title", "f_Body"])?
        && sqlite_table_has_columns(
            &connection,
            "t_search",
            &["f_ID", "f_Anchor", "f_KeyWord", "f_TitleMain", "f_All"],
        )?
    {
        return Ok(MultiviewPayloadRole::ContentSearchBody);
    }
    if sqlite_table_has_columns(&connection, "t_index", &["f_hore_code", "f_text"])? {
        return Ok(MultiviewPayloadRole::HtmlIndex);
    }
    if sqlite_table_exists(&connection, "t_hore")? {
        return Ok(MultiviewPayloadRole::LawMetadata);
    }
    if sqlite_table_has_columns(
        &connection,
        "t_page",
        &["f_name", "f_name_key", "f_name_kana", "f_anchor"],
    )? {
        return Ok(MultiviewPayloadRole::SubjectIndex);
    }
    if sqlite_table_has_columns(&connection, "t_page", &["f_text", "f_text_plane"])? {
        return Ok(MultiviewPayloadRole::CaseDigestBody);
    }
    let mut body_like = 0usize;
    for table in &tables {
        if sqlite_table_has_columns(
            &connection,
            table,
            &["f_hore_code", "f_rec_id", "f_text", "f_text_plane"],
        )? {
            body_like += 1;
        }
    }
    if body_like >= tables.len().div_ceil(2).max(1) {
        return Ok(MultiviewPayloadRole::LawBody);
    }
    Ok(MultiviewPayloadRole::Unclassified)
}

fn open_sqlite(path: &Path) -> Result<Connection> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

fn sqlite_table_names(connection: &Connection) -> Result<Vec<String>> {
    let mut statement =
        connection.prepare("select name from sqlite_master where type='table' order by name")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

fn sqlite_table_exists(connection: &Connection, table: &str) -> Result<bool> {
    let exists = connection
        .query_row(
            "select 1 from sqlite_master where type='table' and lower(name) = lower(?) limit 1",
            [table],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    Ok(exists)
}

fn sqlite_table_has_columns(
    connection: &Connection,
    table: &str,
    columns: &[&str],
) -> Result<bool> {
    if !sqlite_table_exists(connection, table)? {
        return Ok(false);
    }
    let mut statement =
        connection.prepare(&format!("pragma table_info({})", quote_identifier(table)))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let present = rows
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(|column| column.to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    Ok(columns
        .iter()
        .all(|column| present.contains(&column.to_lowercase())))
}

fn table_with_anchor(connection: &Connection, href: &str) -> Result<Option<String>> {
    for table in sqlite_table_names(connection)? {
        if !sqlite_table_has_columns(connection, &table, &["f_anchor"])? {
            continue;
        }
        let sql = format!(
            "select 1 from {} where f_anchor = ? limit 1",
            quote_identifier(&table)
        );
        let found = connection
            .query_row(&sql, [href], |_| Ok(()))
            .optional()?
            .is_some();
        if found {
            return Ok(Some(table));
        }
    }
    Ok(None)
}

fn query_law_rows_by_anchor(
    connection: &Connection,
    table: &str,
    href: &str,
) -> Result<Vec<LawRow>> {
    let sql = format!(
        "select f_title_no, f_title_sub, f_text from {} where f_anchor = ? order by f_rec_id",
        quote_identifier(table)
    );
    query_law_rows(connection, &sql, href)
}

fn query_law_rows_by_hore_code(
    connection: &Connection,
    table: &str,
    hore_code: &str,
) -> Result<Vec<LawRow>> {
    if !sqlite_table_has_columns(connection, table, &["f_hore_code"])? {
        return Ok(Vec::new());
    }
    let sql = format!(
        "select f_title_no, f_title_sub, f_text from {} where f_hore_code = ? order by f_rec_id limit 120",
        quote_identifier(table)
    );
    query_law_rows(connection, &sql, hore_code)
}

fn query_law_rows(connection: &Connection, sql: &str, param: &str) -> Result<Vec<LawRow>> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([param], |row| {
        Ok(LawRow {
            title_no: sqlite_value_to_string(row.get_ref(0)?)?,
            title_sub: sqlite_value_to_string(row.get_ref(1)?)?,
            text: sqlite_value_to_string(row.get_ref(2)?)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

fn query_index_rows(connection: &Connection, code: &str) -> Result<Vec<LawRow>> {
    let mut statement = connection
        .prepare("select f_title_no, f_title_sub, f_text from t_index where f_hore_code = ?")?;
    let rows = statement.query_map([code], |row| {
        Ok(LawRow {
            title_no: sqlite_value_to_string(row.get_ref(0)?)?,
            title_sub: sqlite_value_to_string(row.get_ref(1)?)?,
            text: sqlite_value_to_string(row.get_ref(2)?)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

fn sqlite_value_to_string(value: ValueRef<'_>) -> rusqlite::Result<String> {
    match value {
        ValueRef::Null => Ok(String::new()),
        ValueRef::Integer(value) => Ok(value.to_string()),
        ValueRef::Real(value) => Ok(value.to_string()),
        ValueRef::Text(value) => Ok(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(value) => Ok(String::from_utf8_lossy(value).into_owned()),
    }
}

fn html_to_text(fragment: &str) -> String {
    let mut text = String::with_capacity(fragment.len());
    let mut in_tag = false;
    for ch in fragment.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            _ => text.push(ch),
        }
    }
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .trim()
        .to_owned()
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_menu_data_items() {
        let items = parse_menu_data(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <list>
              <item label="Book" href="">
                <item label="凡例">
                  <item label="まえがき" href="000001" anchor="top"></item>
                </item>
                <item label="五十音順法令一覧" href="50on"></item>
              </item>
            </list>"#,
        )
        .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Book");
        assert_eq!(items[0].href, None);
        assert_eq!(items[0].children[0].label, "凡例");
        assert_eq!(
            items[0].children[0].children[0].href.as_deref(),
            Some("000001")
        );
        assert_eq!(
            items[0].children[0].children[0].anchor.as_deref(),
            Some("top")
        );
        assert_eq!(items[0].children[1].href.as_deref(), Some("50on"));
    }

    #[test]
    fn rejects_unbalanced_menu_data_items() {
        let error = parse_menu_data("<list><item label=\"broken\"></list>").unwrap_err();
        assert!(
            error.to_string().contains("XML parse error") || error.to_string().contains("unclosed")
        );
    }
}

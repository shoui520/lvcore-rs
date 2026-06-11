use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::crypto::{decrypt_logofont_cipher_file_to_path, decrypt_logofont_cipher_prefix};
use crate::error::{Error, Result};
use crate::search::SearchMode;
use crate::storage::{private_cache_dir, regular_file_inside_root};

const SQLITE_MAGIC: &[u8] = b"SQLite format 3\0";

mod menu;

pub use menu::{MultiviewMenuItem, parse_menu_data};

pub struct MultiviewStore {
    payloads: Vec<MultiviewPayloadSource>,
    connections: Mutex<BTreeMap<PathBuf, Connection>>,
    decrypted_paths: Mutex<BTreeMap<PathBuf, PathBuf>>,
    roles: Mutex<BTreeMap<PathBuf, MultiviewPayloadRole>>,
    schemas: Mutex<BTreeMap<PathBuf, Arc<MultiviewSqliteSchema>>>,
}

impl std::fmt::Debug for MultiviewStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiviewStore")
            .field("payloads", &self.payloads)
            .finish_non_exhaustive()
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiviewLawList {
    pub title: String,
    pub items: Vec<MultiviewLawListItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiviewLawListItem {
    pub code: String,
    pub name: String,
    pub kana: String,
    pub kana_initial: String,
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
            connections: Mutex::new(BTreeMap::new()),
            decrypted_paths: Mutex::new(BTreeMap::new()),
            roles: Mutex::new(BTreeMap::new()),
            schemas: Mutex::new(BTreeMap::new()),
        }))
    }

    pub fn search(
        &self,
        query: &str,
        mode: &SearchMode,
        limit: usize,
    ) -> Result<Vec<MultiviewSearchHit>> {
        self.search_page(query, mode, 0, limit)
    }

    pub fn search_page(
        &self,
        query: &str,
        mode: &SearchMode,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<MultiviewSearchHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        if let Some(hits) = self.content_search_page(query, mode, offset, limit)?
            && !hits.is_empty()
        {
            return Ok(hits);
        }
        self.law_search_page(query, mode, offset, limit)
    }

    pub fn has_law_navigation(&self) -> Result<bool> {
        Ok(self
            .first_payload_by_role(MultiviewPayloadRole::LawMetadata)?
            .is_some()
            || self
                .first_payload_by_role(MultiviewPayloadRole::LawBody)?
                .is_some())
    }

    fn content_search_page(
        &self,
        query: &str,
        mode: &SearchMode,
        offset: usize,
        limit: usize,
    ) -> Result<Option<Vec<MultiviewSearchHit>>> {
        let Some(payload) = self.first_payload_by_role(MultiviewPayloadRole::ContentSearchBody)?
        else {
            return Ok(None);
        };
        let sqlite_path = self.sqlite_path_for_payload(payload)?;
        self.with_connection(&sqlite_path, |connection| {
            let schema = self.schema(&sqlite_path, connection)?;
            if !schema.table_has_columns("t_search", &["f_ID", "f_KeyWord", "f_TitleMain", "f_All"])
            {
                return Ok(Some(Vec::new()));
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
                 where {column} {operator} ? order by f_No limit ? offset ?"
            );
            let mut statement = connection.prepare(&sql)?;
            let rows = statement.query_map((pattern, limit as i64, offset as i64), |row| {
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
                .map(Some)
                .map_err(Error::from)
        })
    }

    fn law_search_page(
        &self,
        query: &str,
        mode: &SearchMode,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<MultiviewSearchHit>> {
        if *mode != SearchMode::FullText {
            return self.law_metadata_search_page(query, mode, offset, limit);
        }

        let metadata_count = self.law_metadata_match_count(query, mode)?;
        let mut hits = Vec::new();
        if offset < metadata_count {
            hits.extend(self.law_metadata_search_page(query, mode, offset, limit)?);
        }
        if hits.len() >= limit {
            hits.truncate(limit);
            return Ok(hits);
        }

        let body_offset = offset.saturating_sub(metadata_count);
        let body_limit = limit.saturating_sub(hits.len());
        hits.extend(self.law_body_search_page(query, body_offset, body_limit)?);
        hits.truncate(limit);
        Ok(hits)
    }

    fn law_metadata_match_count(&self, query: &str, mode: &SearchMode) -> Result<usize> {
        if query.trim().is_empty() {
            return Ok(0);
        }
        let Some(payload) = self.first_payload_by_role(MultiviewPayloadRole::LawMetadata)? else {
            return Ok(0);
        };
        let sqlite_path = self.sqlite_path_for_payload(payload)?;
        self.with_connection(&sqlite_path, |connection| {
            let schema = self.schema(&sqlite_path, connection)?;
            let searchable_columns = law_metadata_search_columns(&schema);
            if searchable_columns.is_empty() {
                return Ok(0);
            }
            let (operator, pattern) = sql_match_operator_and_pattern(query, mode);
            let conditions = sql_conditions_for_columns(&searchable_columns, operator);
            let sql = format!("select count(*) from t_hore where {conditions}");
            let params = vec![pattern; searchable_columns.len()];
            let count = connection.query_row(&sql, rusqlite::params_from_iter(params), |row| {
                row.get::<_, i64>(0)
            })?;
            Ok(count.max(0) as usize)
        })
    }

    fn law_metadata_search_page(
        &self,
        query: &str,
        mode: &SearchMode,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<MultiviewSearchHit>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let Some(payload) = self.first_payload_by_role(MultiviewPayloadRole::LawMetadata)? else {
            return Ok(Vec::new());
        };
        let sqlite_path = self.sqlite_path_for_payload(payload)?;
        self.with_connection(&sqlite_path, |connection| {
            let schema = self.schema(&sqlite_path, connection)?;
            let searchable_columns = law_metadata_search_columns(&schema);
            if searchable_columns.is_empty() {
                return Ok(Vec::new());
            }
            let (operator, pattern) = sql_match_operator_and_pattern(query, mode);
            let conditions = sql_conditions_for_columns(&searchable_columns, operator);
            let name_sub = optional_column_expr(&schema, "t_hore", "f_name_sub");
            let abbr1 = optional_column_expr(&schema, "t_hore", "f_abbr1");
            let commonname = optional_column_expr(&schema, "t_hore", "f_commonname");
            let sql = format!(
                "select f_hore_code, f_name, {name_sub}, f_name_kana, {abbr1}, {commonname} \
                 from t_hore where {conditions} \
                 order by f_kana_order, f_name_kana, f_name, f_hore_code limit ? offset ?",
            );
            let mut params = vec![pattern; searchable_columns.len()];
            params.push(limit.to_string());
            params.push(offset.to_string());
            let mut statement = connection.prepare(&sql)?;
            let rows = statement.query_map(rusqlite::params_from_iter(params), |row| {
                let code = sqlite_value_to_string(row.get_ref(0)?)?;
                let name = sqlite_value_to_string(row.get_ref(1)?)?;
                let name_sub = sqlite_value_to_string(row.get_ref(2)?)?;
                let kana = sqlite_value_to_string(row.get_ref(3)?)?;
                let abbr = sqlite_value_to_string(row.get_ref(4)?)?;
                let common = sqlite_value_to_string(row.get_ref(5)?)?;
                let title_html = if name_sub.is_empty() || name_sub == name {
                    name.clone()
                } else {
                    format!("{name}<span class=\"lv-subtitle\">{name_sub}</span>")
                };
                let snippet = [kana, abbr, common]
                    .into_iter()
                    .filter(|part| !part.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join(" / ");
                Ok(MultiviewSearchHit {
                    href: code,
                    title_html,
                    title_text: name,
                    snippet_html: (!snippet.is_empty()).then_some(snippet),
                })
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::from)
        })
    }

    fn law_body_search_page(
        &self,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<MultiviewSearchHit>> {
        if query.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let Some(payload) = self.first_payload_by_role(MultiviewPayloadRole::LawBody)? else {
            return Ok(Vec::new());
        };
        let sqlite_path = self.sqlite_path_for_payload(payload)?;
        self.with_connection(&sqlite_path, |connection| {
            let schema = self.schema(&sqlite_path, connection)?;
            let pattern = format!("%{}%", escape_sql_like(query));
            let mut remaining_offset = offset;
            let mut remaining_limit = limit;
            let mut hits = Vec::new();
            for table in law_body_tables(&schema) {
                if remaining_limit == 0 {
                    break;
                }
                let conditions = "coalesce(f_text_plane, '') like ? escape '\\' \
                    or coalesce(f_text, '') like ? escape '\\'";
                let count_sql = format!(
                    "select count(*) from {} where {conditions}",
                    quote_identifier(&table)
                );
                let count = connection.query_row(
                    &count_sql,
                    rusqlite::params![pattern, pattern],
                    |row| row.get::<_, i64>(0),
                )?;
                let count = count.max(0) as usize;
                if remaining_offset >= count {
                    remaining_offset -= count;
                    continue;
                }

                let select_sql = format!(
                    "select f_hore_code, f_title_no, f_title_sub, f_anchor, f_text_plane \
                     from {} where {conditions} order by f_rec_id limit ? offset ?",
                    quote_identifier(&table)
                );
                let mut statement = connection.prepare(&select_sql)?;
                let rows = statement.query_map(
                    rusqlite::params![
                        pattern,
                        pattern,
                        remaining_limit as i64,
                        remaining_offset as i64
                    ],
                    |row| {
                        let hore_code = sqlite_value_to_string(row.get_ref(0)?)?;
                        let title_no = sqlite_value_to_string(row.get_ref(1)?)?;
                        let title_sub = sqlite_value_to_string(row.get_ref(2)?)?;
                        let anchor = sqlite_value_to_string(row.get_ref(3)?)?;
                        let text_plane = sqlite_value_to_string(row.get_ref(4)?)?;
                        let title_html = [title_no.as_str(), title_sub.as_str()]
                            .into_iter()
                            .filter(|part| !part.trim().is_empty())
                            .collect::<Vec<_>>()
                            .join(" ");
                        let title_html = if title_html.is_empty() {
                            if anchor.is_empty() {
                                hore_code.clone()
                            } else {
                                anchor.clone()
                            }
                        } else {
                            title_html
                        };
                        Ok(MultiviewSearchHit {
                            href: if anchor.is_empty() { hore_code } else { anchor },
                            title_text: html_to_text(&title_html),
                            title_html,
                            snippet_html: plain_snippet(&text_plane, 220),
                        })
                    },
                )?;
                let mut table_hits = rows.collect::<std::result::Result<Vec<_>, _>>()?;
                remaining_limit = remaining_limit.saturating_sub(table_hits.len());
                remaining_offset = 0;
                hits.append(&mut table_hits);
            }
            Ok(hits)
        })
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

    pub fn content_target_for_lved_dataid(
        &self,
        data_id: &str,
    ) -> Result<Option<(String, Option<String>)>> {
        let Ok(parsed_id) = data_id.parse::<i64>() else {
            return Ok(None);
        };
        let Some(payload) = self.first_payload_by_role(MultiviewPayloadRole::ContentSearchBody)?
        else {
            return Ok(None);
        };
        let sqlite_path = self.sqlite_path_for_payload(payload)?;
        self.with_connection(&sqlite_path, |connection| {
            let schema = self.schema(&sqlite_path, connection)?;
            if !schema.table_has_columns("t_contents", &["f_ID"]) {
                return Ok(None);
            }
            if let Some(content_id) = connection
                .query_row(
                    "select f_ID from t_contents where f_ID = ? limit 1",
                    [parsed_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
            {
                return Ok(Some((multiview_content_href_for_id(content_id), None)));
            }
            if !schema.table_has_columns("t_contents", &["f_Body"]) {
                return Ok(None);
            }
            let pattern = format!("%{}%", escape_sql_like(data_id));
            let mut statement = connection.prepare(
                "select f_ID, cast(f_Body as blob) from t_contents \
                 where cast(f_Body as text) like ? escape '\\' order by f_ID limit 64",
            )?;
            let rows = statement.query_map([pattern], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    sqlite_value_to_string(row.get_ref(1)?)?,
                ))
            })?;
            for row in rows {
                let (content_id, body) = row?;
                if multiview_body_has_anchor(&body, data_id) {
                    return Ok(Some((
                        multiview_content_href_for_id(content_id),
                        Some(data_id.to_owned()),
                    )));
                }
            }
            Ok(None)
        })
    }

    pub fn law_list_for_href(&self, href: &str) -> Result<Option<MultiviewLawList>> {
        if !href.eq_ignore_ascii_case("50on") {
            return Ok(None);
        }
        let Some(payload) = self.first_payload_by_role(MultiviewPayloadRole::LawMetadata)? else {
            return Ok(None);
        };
        let sqlite_path = self.sqlite_path_for_payload(payload)?;
        self.with_connection(&sqlite_path, |connection| {
            let schema = self.schema(&sqlite_path, connection)?;
            if !schema.table_has_columns(
                "t_hore",
                &[
                    "f_hore_code",
                    "f_name",
                    "f_name_kana",
                    "f_kana_ini",
                    "f_kana_order",
                ],
            ) {
                return Ok(None);
            }
            let mut statement = connection.prepare(
                "select f_hore_code, f_name, f_name_kana, f_kana_ini \
                 from t_hore \
                 where coalesce(f_hore_code, '') <> '' and coalesce(f_name, '') <> '' \
                 order by f_kana_order, f_name_kana, f_name, f_hore_code",
            )?;
            let rows = statement.query_map([], |row| {
                let code = sqlite_value_to_string(row.get_ref(0)?)?;
                Ok(MultiviewLawListItem {
                    code,
                    name: sqlite_value_to_string(row.get_ref(1)?)?,
                    kana: sqlite_value_to_string(row.get_ref(2)?)?,
                    kana_initial: sqlite_value_to_string(row.get_ref(3)?)?,
                })
            })?;
            let items = rows.collect::<std::result::Result<Vec<_>, _>>()?;
            if items.is_empty() {
                return Ok(None);
            }
            Ok(Some(MultiviewLawList {
                title: "五十音順法令一覧".to_owned(),
                items,
            }))
        })
    }

    fn content_body_for_href(&self, href: &str) -> Result<Option<MultiviewBody>> {
        let Ok(content_id) = href.parse::<i64>() else {
            return Ok(None);
        };
        let Some(payload) = self.first_payload_by_role(MultiviewPayloadRole::ContentSearchBody)?
        else {
            return Ok(None);
        };
        let sqlite_path = self.sqlite_path_for_payload(payload)?;
        self.with_connection(&sqlite_path, |connection| {
            let schema = self.schema(&sqlite_path, connection)?;
            if !schema.table_has_columns("t_contents", &["f_ID", "f_Title", "f_Body"]) {
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
        })
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
        self.with_connection(&sqlite_path, |connection| {
            let schema = self.schema(&sqlite_path, connection)?;
            let hinted_table = format!("t_{table_hint}");
            let table = if let Some(table) = schema.canonical_table(&hinted_table) {
                Some(table.to_owned())
            } else {
                table_with_anchor(connection, &schema, href)?
            };
            let Some(table) = table else {
                return Ok(None);
            };
            if !schema.table_has_columns(&table, &["f_text"]) {
                return Ok(None);
            }

            let rows = if schema.table_has_columns(&table, &["f_anchor"]) {
                query_law_rows_by_anchor(connection, &table, href)?
            } else {
                Vec::new()
            };
            let rows = if rows.is_empty() {
                query_law_rows_by_hore_code(connection, &schema, &table, table_hint)?
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
        })
    }

    fn html_index_body_for_href(&self, href: &str) -> Result<Option<MultiviewBody>> {
        let code = href.strip_prefix("index:").unwrap_or(href);
        let Some(payload) = self.first_payload_by_role(MultiviewPayloadRole::HtmlIndex)? else {
            return Ok(None);
        };
        let sqlite_path = self.sqlite_path_for_payload(payload)?;
        self.with_connection(&sqlite_path, |connection| {
            let schema = self.schema(&sqlite_path, connection)?;
            if !schema.table_has_columns(
                "t_index",
                &["f_hore_code", "f_title_no", "f_title_sub", "f_text"],
            ) {
                return Ok(None);
            }
            let rows = query_index_rows(connection, code)?;
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
        })
    }

    fn first_payload_by_role(
        &self,
        role: MultiviewPayloadRole,
    ) -> Result<Option<&MultiviewPayloadSource>> {
        let payload_count = self.payloads.len();
        for payload in &self.payloads {
            if !payload_may_have_role(&payload.name, payload_count, role) {
                continue;
            }
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
        if let Some(role) = hinted_payload_role(&payload.name, self.payloads.len()) {
            self.roles
                .lock()
                .map_err(|_| Error::Driver("multiview role cache was poisoned".to_owned()))?
                .insert(payload.path.clone(), role);
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

    fn with_connection<T>(
        &self,
        sqlite_path: &Path,
        read: impl FnOnce(&Connection) -> Result<T>,
    ) -> Result<T> {
        let mut guard = self
            .connections
            .lock()
            .map_err(|_| Error::Driver("multiview connection cache was poisoned".to_owned()))?;
        if !guard.contains_key(sqlite_path) {
            guard.insert(sqlite_path.to_path_buf(), open_sqlite(sqlite_path)?);
        }
        let connection = guard
            .get(sqlite_path)
            .ok_or_else(|| Error::Driver("multiview connection cache is empty".to_owned()))?;
        read(connection)
    }

    fn schema(
        &self,
        sqlite_path: &Path,
        connection: &Connection,
    ) -> Result<Arc<MultiviewSqliteSchema>> {
        {
            let schemas = self
                .schemas
                .lock()
                .map_err(|_| Error::Driver("multiview schema cache was poisoned".to_owned()))?;
            if let Some(schema) = schemas.get(sqlite_path) {
                return Ok(Arc::clone(schema));
            }
        }
        let schema = Arc::new(MultiviewSqliteSchema::load(connection)?);
        let mut schemas = self
            .schemas
            .lock()
            .map_err(|_| Error::Driver("multiview schema cache was poisoned".to_owned()))?;
        Ok(Arc::clone(
            schemas.entry(sqlite_path.to_path_buf()).or_insert(schema),
        ))
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
        let output = decrypted_multiview_cache_path(path)?;
        if !output.exists() {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            let tmp = output.with_extension(format!("{}.tmp", std::process::id()));
            decrypt_logofont_cipher_file_to_path(path, &tmp)?;
            fs::rename(tmp, &output)?;
        }
        self.decrypted_paths
            .lock()
            .map_err(|_| Error::Driver("multiview decrypt cache was poisoned".to_owned()))?
            .insert(path.to_path_buf(), output.clone());
        Ok(output)
    }
}

fn decrypted_multiview_cache_path(path: &Path) -> Result<PathBuf> {
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
        .unwrap_or_else(|| "payload".into());
    Ok(private_cache_dir("multiview-payloads")?.join(format!("{stem}-{digest}.sqlite")))
}

fn payload_may_have_role(name: &str, payload_count: usize, role: MultiviewPayloadRole) -> bool {
    let lower = name.to_ascii_lowercase();
    match role {
        MultiviewPayloadRole::ContentSearchBody => payload_count == 1 || lower == "blvdat",
        MultiviewPayloadRole::LawBody => {
            lower.starts_with("blv")
                || lower.starts_with("hlv")
                || hinted_payload_role(name, payload_count).is_none()
        }
        MultiviewPayloadRole::HtmlIndex => {
            lower.starts_with("ilv") || hinted_payload_role(name, payload_count).is_none()
        }
        MultiviewPayloadRole::LawMetadata => {
            lower.starts_with("nlv") || hinted_payload_role(name, payload_count).is_none()
        }
        MultiviewPayloadRole::SubjectIndex => {
            lower.starts_with("jlv") || hinted_payload_role(name, payload_count).is_none()
        }
        MultiviewPayloadRole::CaseDigestBody | MultiviewPayloadRole::Unclassified => true,
    }
}

fn hinted_payload_role(name: &str, payload_count: usize) -> Option<MultiviewPayloadRole> {
    let lower = name.to_ascii_lowercase();
    if payload_count == 1 && lower == "blvdat" {
        return Some(MultiviewPayloadRole::ContentSearchBody);
    }
    if lower == "blvbat" {
        return Some(MultiviewPayloadRole::LawBody);
    }
    if lower == "hlvbat" {
        return Some(MultiviewPayloadRole::CaseDigestBody);
    }
    if lower.starts_with("ilv") {
        return Some(MultiviewPayloadRole::HtmlIndex);
    }
    if lower.starts_with("jlv") {
        return Some(MultiviewPayloadRole::SubjectIndex);
    }
    if lower.starts_with("nlv") {
        return Some(MultiviewPayloadRole::LawMetadata);
    }
    None
}

#[derive(Debug)]
struct LawRow {
    title_no: String,
    title_sub: String,
    text: String,
}

#[derive(Debug)]
struct MultiviewSqliteSchema {
    tables: BTreeMap<String, String>,
    columns: BTreeMap<String, BTreeSet<String>>,
}

impl MultiviewSqliteSchema {
    fn load(connection: &Connection) -> Result<Self> {
        let mut tables = BTreeMap::new();
        let mut columns = BTreeMap::new();
        for table in sqlite_table_names(connection)? {
            let table_key = table.to_ascii_lowercase();
            tables.insert(table_key.clone(), table.clone());
            columns.insert(
                table_key,
                sqlite_columns(connection, &table)?
                    .into_iter()
                    .map(|column| column.to_ascii_lowercase())
                    .collect(),
            );
        }
        Ok(Self { tables, columns })
    }

    fn canonical_table(&self, table: &str) -> Option<&str> {
        self.tables
            .get(&table.to_ascii_lowercase())
            .map(String::as_str)
    }

    fn table_has_columns(&self, table: &str, required: &[&str]) -> bool {
        let Some(columns) = self.columns.get(&table.to_ascii_lowercase()) else {
            return false;
        };
        required
            .iter()
            .all(|column| columns.contains(&column.to_ascii_lowercase()))
    }

    fn table_names(&self) -> impl Iterator<Item = &str> {
        self.tables.values().map(String::as_str)
    }
}

fn multiview_payload_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if regular_file_inside_root(root, &path)?
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
    let prefix = read_file_prefix(path, 4096)?;
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

fn read_file_prefix(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let mut bytes = vec![0; limit];
    let read = file.read(&mut bytes)?;
    bytes.truncate(read);
    Ok(bytes)
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
    if sqlite_table_has_columns(&connection, "t_page", &["f_text", "f_text_plane"])? {
        return Ok(MultiviewPayloadRole::CaseDigestBody);
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

fn sqlite_columns(connection: &Connection, table: &str) -> Result<Vec<String>> {
    let mut statement =
        connection.prepare(&format!("pragma table_info({})", quote_identifier(table)))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

fn table_with_anchor(
    connection: &Connection,
    schema: &MultiviewSqliteSchema,
    href: &str,
) -> Result<Option<String>> {
    for table in schema.table_names() {
        if !schema.table_has_columns(table, &["f_anchor"]) {
            continue;
        }
        let sql = format!(
            "select 1 from {} where f_anchor = ? limit 1",
            quote_identifier(table)
        );
        let found = connection
            .query_row(&sql, [href], |_| Ok(()))
            .optional()?
            .is_some();
        if found {
            return Ok(Some(table.to_owned()));
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
    schema: &MultiviewSqliteSchema,
    table: &str,
    hore_code: &str,
) -> Result<Vec<LawRow>> {
    if !schema.table_has_columns(table, &["f_hore_code"]) {
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

fn law_metadata_search_columns(schema: &MultiviewSqliteSchema) -> Vec<&'static str> {
    if !schema.table_has_columns("t_hore", &["f_hore_code", "f_name", "f_name_kana"]) {
        return Vec::new();
    }
    [
        "f_name",
        "f_name_sub",
        "f_name_kana",
        "f_abbr1",
        "f_abbr1_kana",
        "f_nickname",
        "f_commonname",
        "f_commonname_kana",
        "f_commonname_ex",
        "f_abbr_user",
        "f_abbr_user_kana",
        "f_temp_kana",
    ]
    .into_iter()
    .filter(|column| schema.table_has_columns("t_hore", &[*column]))
    .collect()
}

fn law_body_tables(schema: &MultiviewSqliteSchema) -> Vec<String> {
    schema
        .table_names()
        .filter(|table| {
            schema.table_has_columns(
                table,
                &[
                    "f_hore_code",
                    "f_rec_id",
                    "f_title_no",
                    "f_title_sub",
                    "f_anchor",
                    "f_text",
                    "f_text_plane",
                ],
            )
        })
        .map(str::to_owned)
        .collect()
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

fn plain_snippet(text: &str, max_chars: usize) -> Option<String> {
    let snippet = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect::<String>();
    (!snippet.is_empty()).then_some(snippet)
}

fn multiview_content_href_for_id(content_id: i64) -> String {
    format!("{content_id:06}")
}

fn multiview_body_has_anchor(body: &str, anchor: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(relative_start) = lower[cursor..].find("<a") {
        let start = cursor + relative_start;
        let Some(relative_end) = lower[start..].find('>') else {
            break;
        };
        let end = start + relative_end;
        let tag = &body[start..end];
        if html_tag_quoted_attr_value(tag, "name")
            .or_else(|| html_tag_quoted_attr_value(tag, "id"))
            .is_some_and(|value| value.trim() == anchor)
        {
            return true;
        }
        cursor = end.saturating_add(1);
    }
    false
}

fn html_tag_quoted_attr_value(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let lower_bytes = lower.as_bytes();
    let bytes = tag.as_bytes();
    let mut cursor = 0usize;
    while let Some(relative_start) = lower[cursor..].find(attr) {
        let start = cursor + relative_start;
        let before_is_boundary = start == 0
            || lower_bytes
                .get(start - 1)
                .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'<');
        let after = start + attr.len();
        let after_is_boundary = lower_bytes
            .get(after)
            .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_');
        if !before_is_boundary || !after_is_boundary {
            cursor = after;
            continue;
        }
        let mut index = after;
        while bytes
            .get(index)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            index += 1;
        }
        if bytes.get(index).copied() != Some(b'=') {
            cursor = after;
            continue;
        }
        index += 1;
        while bytes
            .get(index)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            index += 1;
        }
        let quote = bytes.get(index).copied()?;
        if quote != b'\'' && quote != b'"' {
            cursor = after;
            continue;
        }
        let value_start = index + 1;
        let value_end = bytes[value_start..]
            .iter()
            .position(|byte| *byte == quote)
            .map(|relative| value_start + relative)?;
        return Some(tag[value_start..value_end].to_owned());
    }
    None
}

fn sql_match_operator_and_pattern(query: &str, mode: &SearchMode) -> (&'static str, String) {
    match mode {
        SearchMode::Exact => ("=", query.to_owned()),
        SearchMode::Forward => ("like", format!("{}%", escape_sql_like(query))),
        SearchMode::Backward => ("like", format!("%{}", escape_sql_like(query))),
        SearchMode::Partial | SearchMode::FullText | SearchMode::Advanced(_) => {
            ("like", format!("%{}%", escape_sql_like(query)))
        }
    }
}

fn escape_sql_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '%' | '_' | '\\' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn sql_conditions_for_columns(columns: &[&str], operator: &str) -> String {
    columns
        .iter()
        .map(|column| {
            if operator == "=" {
                format!("coalesce({}, '') = ?", quote_identifier(column))
            } else {
                format!(
                    "coalesce({}, '') like ? escape '\\'",
                    quote_identifier(column)
                )
            }
        })
        .collect::<Vec<_>>()
        .join(" or ")
}

fn optional_column_expr(schema: &MultiviewSqliteSchema, table: &str, column: &str) -> String {
    if schema.table_has_columns(table, &[column]) {
        quote_identifier(column)
    } else {
        "''".to_owned()
    }
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

#[cfg(test)]
mod tests;

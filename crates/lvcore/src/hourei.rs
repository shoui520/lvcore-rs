use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use encoding_rs::SHIFT_JIS;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::search::SearchMode;
use crate::storage::{path_stays_inside_root, regular_file_inside_root};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoureiStore {
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoureiCategory {
    pub id: i64,
    pub name: String,
    pub laws: Vec<HoureiLawEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoureiLawEntry {
    pub hore_id: String,
    pub name: String,
    pub name_sub: Option<String>,
    pub abbr1: Option<String>,
    pub category_id: Option<i64>,
    pub kana_order: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoureiSearchHit {
    pub hore_id: String,
    pub title_html: String,
    pub title_text: String,
    pub snippet_html: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoureiLawWindow {
    pub before: Vec<HoureiLawEntry>,
    pub center: HoureiLawEntry,
    pub after: Vec<HoureiLawEntry>,
}

impl HoureiStore {
    pub fn discover(root: &Path) -> Result<Option<Self>> {
        let required = [
            "_DataBase/hore_base.db",
            "_DataBase/hore_search_a.db",
            "_DataBase/horejo_base.db",
        ];
        if required
            .iter()
            .all(|path| regular_file_inside_root(root, &root.join(path)).unwrap_or(false))
        {
            Ok(Some(Self {
                root: root.to_path_buf(),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn categories_with_laws(&self) -> Result<Vec<HoureiCategory>> {
        let connection = self.open_core_db("hore_base.db")?;
        let mut categories = BTreeMap::<i64, HoureiCategory>::new();
        if sqlite_table_has_columns(
            &connection,
            "t_category",
            &["f_category_id", "f_category_name"],
        )? {
            let mut statement = connection.prepare(
                "select f_category_id, f_category_name from t_category order by f_category_id",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    sqlite_value_to_string(row.get_ref(1)?)?,
                ))
            })?;
            for row in rows {
                let (id, name) = row?;
                categories.insert(
                    id,
                    HoureiCategory {
                        id,
                        name,
                        laws: Vec::new(),
                    },
                );
            }
        }

        let mut statement = connection.prepare(
            "select f_hore_id, f_name, f_name_sub, f_abbr1, f_category_id, f_kana_order \
             from t_hore order by f_category_id, f_kana_order, f_hore_id",
        )?;
        let rows = statement.query_map([], hourei_law_entry_from_row)?;
        for law in rows {
            let law = law?;
            let category_id = law.category_id.unwrap_or(-1);
            let category = categories
                .entry(category_id)
                .or_insert_with(|| HoureiCategory {
                    id: category_id,
                    name: if category_id == -1 {
                        "未分類".to_owned()
                    } else {
                        format!("Category {category_id}")
                    },
                    laws: Vec::new(),
                });
            category.laws.push(law);
        }

        Ok(categories.into_values().collect())
    }

    pub fn search(
        &self,
        query: &str,
        mode: &SearchMode,
        limit: usize,
    ) -> Result<Vec<HoureiSearchHit>> {
        self.search_page(query, mode, 0, limit)
    }

    pub fn search_page(
        &self,
        query: &str,
        mode: &SearchMode,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<HoureiSearchHit>> {
        if limit == 0 || query.is_empty() {
            return Ok(Vec::new());
        }
        let connection = self.open_core_db("hore_search_a.db")?;
        if !sqlite_table_has_columns(&connection, "t_hore", &["f_hore_id", "f_name"])? {
            return Ok(Vec::new());
        }
        let columns = sqlite_columns(&connection, "t_hore")?;
        let title_columns = [
            "f_name",
            "f_name_sub",
            "f_abbr1",
            "f_abbr2",
            "f_abbr3",
            "f_abbr4",
            "f_abbr5",
            "f_abbr6",
            "f_abbr7",
        ]
        .into_iter()
        .filter(|column| has_column(&columns, column))
        .collect::<Vec<_>>();
        let mut search_columns = title_columns.clone();
        if has_column(&columns, "f_text_plane") {
            search_columns.push("f_text_plane");
        }
        if search_columns.is_empty() {
            return Ok(Vec::new());
        }

        let (where_clause, params) = hourei_search_where(query, mode, &search_columns);
        let exact_order = title_columns
            .iter()
            .map(|column| format!("{} = ?", quote_identifier(column)))
            .collect::<Vec<_>>()
            .join(" or ");
        let forward_order = title_columns
            .iter()
            .map(|column| format!("{} like ? escape '\\'", quote_identifier(column)))
            .collect::<Vec<_>>()
            .join(" or ");
        let order_prefix = if title_columns.is_empty() {
            String::new()
        } else {
            format!("case when {exact_order} then 0 when {forward_order} then 1 else 2 end, ")
        };
        let mut sql_params = params;
        for _ in &title_columns {
            sql_params.push(query.to_owned());
        }
        for _ in &title_columns {
            sql_params.push(format!("{}%", escape_sql_like(query)));
        }
        sql_params.push(limit.to_string());
        sql_params.push(offset.to_string());

        let sql = format!(
            "select f_hore_id, f_name, f_name_sub, f_abbr1, f_text_plane \
             from t_hore where {where_clause} order by {order_prefix}f_kana_order, f_hore_id limit ? offset ?"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(sql_params.iter()), |row| {
            let hore_id = sqlite_value_to_string(row.get_ref(0)?)?;
            let name = sqlite_value_to_string(row.get_ref(1)?)?;
            let name_sub = sqlite_value_to_string(row.get_ref(2)?)?;
            let abbr1 = sqlite_value_to_string(row.get_ref(3)?)?;
            let text = sqlite_value_to_string(row.get_ref(4)?)?;
            let title_text = hourei_law_label(&name, &name_sub, &abbr1, &hore_id);
            Ok(HoureiSearchHit {
                hore_id,
                title_html: escape_plain_label_html(&title_text),
                title_text,
                snippet_html: nonempty_string(snippet(&text, 180))
                    .map(|value| escape_plain_label_html(&value)),
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    pub fn law_entry(&self, hore_id: &str) -> Result<Option<HoureiLawEntry>> {
        if !is_valid_hourei_law_id(hore_id) {
            return Ok(None);
        }
        let connection = self.open_core_db("hore_base.db")?;
        connection
            .query_row(
                "select f_hore_id, f_name, f_name_sub, f_abbr1, f_category_id, f_kana_order \
                 from t_hore where f_hore_id = ? limit 1",
                [hore_id],
                hourei_law_entry_from_row,
            )
            .optional()
            .map_err(Error::from)
    }

    pub fn law_html(&self, hore_id: &str) -> Result<Option<String>> {
        if !is_valid_hourei_law_id(hore_id) {
            return Ok(None);
        }
        if let Some(path) = self.cached_law_html_path(hore_id)? {
            return Ok(Some(decode_hourei_text(&fs::read(path)?)?));
        }
        let Some(path) = self.law_db_path(hore_id)? else {
            return Ok(None);
        };
        let connection = open_readonly_sqlite(&path)?;
        if !sqlite_table_has_columns(&connection, "t_page", &["f_rec_id", "f_text"])? {
            return Ok(None);
        }
        let mut statement = connection.prepare("select f_text from t_page order by f_rec_id")?;
        let rows = statement.query_map([], |row| sqlite_value_to_string(row.get_ref(0)?))?;
        let fragments = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        if fragments.is_empty() {
            Ok(None)
        } else {
            Ok(Some(fragments.join("\n")))
        }
    }

    pub fn law_window(
        &self,
        hore_id: &str,
        before: usize,
        after: usize,
    ) -> Result<Option<HoureiLawWindow>> {
        if !is_valid_hourei_law_id(hore_id) {
            return Ok(None);
        }
        let connection = self.open_core_db("hore_base.db")?;
        let center_order = connection
            .query_row(
                "select f_kana_order from t_hore where f_hore_id = ? limit 1",
                [hore_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(center_order) = center_order else {
            return Ok(None);
        };
        let center = self.law_entry(hore_id)?.ok_or_else(|| {
            Error::Driver(format!("Hourei law metadata disappeared for {hore_id}"))
        })?;
        let before_rows = self.laws_by_order_clause(
            "f_kana_order < ?",
            "f_kana_order desc, f_hore_id desc",
            center_order,
            before,
        )?;
        let mut before_rows = before_rows;
        before_rows.reverse();
        let after_rows = self.laws_by_order_clause(
            "f_kana_order > ?",
            "f_kana_order, f_hore_id",
            center_order,
            after,
        )?;
        Ok(Some(HoureiLawWindow {
            before: before_rows,
            center,
            after: after_rows,
        }))
    }

    pub fn resource_path_by_reference(&self, raw_ref: &str) -> Result<Option<PathBuf>> {
        let value = html_unescape_minimal(raw_ref).trim().replace('\\', "/");
        if value.is_empty()
            || value.starts_with('#')
            || value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("data:")
            || value.starts_with("mailto:")
            || value.starts_with("javascript:")
        {
            return Ok(None);
        }
        let relative = value.split(['#', '?']).next().unwrap_or("").trim();
        if relative.is_empty() {
            return Ok(None);
        }
        let direct = self.root.join(relative);
        if direct.is_file() {
            return Ok(Some(PathBuf::from(relative)));
        }
        let filename = Path::new(relative)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| relative.to_owned());
        let roots = [
            "_DataBase/image",
            "_KanpouImg",
            "_Manual/image",
            "_Programs",
        ];
        for root in roots {
            if let Some(path) = find_file_by_name(&self.root.join(root), &filename)? {
                return path
                    .strip_prefix(&self.root)
                    .map(|path| Some(path.to_path_buf()))
                    .map_err(|error| Error::Driver(error.to_string()));
            }
        }
        Ok(Some(PathBuf::from(relative)))
    }

    fn open_core_db(&self, name: &str) -> Result<Connection> {
        open_readonly_sqlite(&self.root.join("_DataBase").join(name))
    }

    fn cached_law_html_path(&self, hore_id: &str) -> Result<Option<PathBuf>> {
        let path = self
            .root
            .join("_DataBase")
            .join("HTMLs")
            .join("H")
            .join(format!("{hore_id}_H.html"));
        if path.is_file() && path_stays_inside_root(&self.root, &path)? {
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    fn law_db_path(&self, hore_id: &str) -> Result<Option<PathBuf>> {
        let bytes = hore_id.as_bytes();
        if bytes.len() < 3
            || !bytes[0].is_ascii_digit()
            || !bytes[1].is_ascii_digit()
            || !bytes[2].is_ascii_digit()
        {
            return Ok(None);
        }
        let prefix = match bytes[0] {
            b'1' => "M",
            b'2' => "T",
            b'3' => "S",
            b'4' => "H",
            _ => return Ok(None),
        };
        let year = (bytes[1] - b'0') * 10 + (bytes[2] - b'0');
        let path = self
            .root
            .join("_DataBase")
            .join(format!("{prefix}{year:02}"))
            .join(format!("{hore_id}.db"));
        if path.is_file() && path_stays_inside_root(&self.root, &path)? {
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    fn laws_by_order_clause(
        &self,
        where_clause: &str,
        order_clause: &str,
        order: i64,
        limit: usize,
    ) -> Result<Vec<HoureiLawEntry>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let connection = self.open_core_db("hore_base.db")?;
        let sql = format!(
            "select f_hore_id, f_name, f_name_sub, f_abbr1, f_category_id, f_kana_order \
             from t_hore where {where_clause} order by {order_clause} limit ?"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map((order, limit as i64), hourei_law_entry_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }
}

fn is_valid_hourei_law_id(hore_id: &str) -> bool {
    !hore_id.is_empty() && hore_id.bytes().all(|byte| byte.is_ascii_digit())
}

fn hourei_law_entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HoureiLawEntry> {
    Ok(HoureiLawEntry {
        hore_id: sqlite_value_to_string(row.get_ref(0)?)?,
        name: sqlite_value_to_string(row.get_ref(1)?)?,
        name_sub: nonempty_string(sqlite_value_to_string(row.get_ref(2)?)?),
        abbr1: nonempty_string(sqlite_value_to_string(row.get_ref(3)?)?),
        category_id: optional_i64(row.get_ref(4)?),
        kana_order: optional_i64(row.get_ref(5)?),
    })
}

fn hourei_search_where(query: &str, mode: &SearchMode, columns: &[&str]) -> (String, Vec<String>) {
    let mut params = Vec::new();
    let (operator, pattern) = match mode {
        SearchMode::Exact => ("=", query.to_owned()),
        SearchMode::Forward => ("like", format!("{}%", escape_sql_like(query))),
        SearchMode::Backward => ("like", format!("%{}", escape_sql_like(query))),
        SearchMode::Partial | SearchMode::FullText | SearchMode::Advanced(_) => {
            ("like", format!("%{}%", escape_sql_like(query)))
        }
    };
    let clause = columns
        .iter()
        .map(|column| {
            params.push(pattern.clone());
            if operator == "like" {
                format!("{} like ? escape '\\'", quote_identifier(column))
            } else {
                format!("{} = ?", quote_identifier(column))
            }
        })
        .collect::<Vec<_>>()
        .join(" or ");
    (clause, params)
}

fn open_readonly_sqlite(path: &Path) -> Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(Error::from)
}

fn sqlite_columns(connection: &Connection, table: &str) -> Result<Vec<String>> {
    let mut statement =
        connection.prepare(&format!("pragma table_info({})", quote_identifier(table)))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

fn sqlite_table_has_columns(
    connection: &Connection,
    table: &str,
    required: &[&str],
) -> Result<bool> {
    let columns = sqlite_columns(connection, table)?;
    Ok(required.iter().all(|column| has_column(&columns, column)))
}

fn has_column(columns: &[String], needle: &str) -> bool {
    columns
        .iter()
        .any(|column| column.eq_ignore_ascii_case(needle))
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn sqlite_value_to_string(value: ValueRef<'_>) -> rusqlite::Result<String> {
    match value {
        ValueRef::Null => Ok(String::new()),
        ValueRef::Integer(value) => Ok(value.to_string()),
        ValueRef::Real(value) => Ok(value.to_string()),
        ValueRef::Text(value) => Ok(String::from_utf8_lossy(value).to_string()),
        ValueRef::Blob(value) => Ok(decode_bytes(value)),
    }
}

fn optional_i64(value: ValueRef<'_>) -> Option<i64> {
    match value {
        ValueRef::Null => None,
        ValueRef::Integer(value) => Some(value),
        ValueRef::Text(value) => std::str::from_utf8(value).ok()?.parse().ok(),
        _ => None,
    }
}

fn decode_hourei_text(data: &[u8]) -> Result<String> {
    match std::str::from_utf8(data) {
        Ok(value) => Ok(value.trim_start_matches('\u{feff}').to_owned()),
        Err(_) => Ok(decode_bytes(data)),
    }
}

fn decode_bytes(data: &[u8]) -> String {
    let (decoded, _, _) = SHIFT_JIS.decode(data);
    decoded.into_owned()
}

fn hourei_law_label(name: &str, name_sub: &str, abbr1: &str, fallback: &str) -> String {
    if !name.trim().is_empty() && !name_sub.trim().is_empty() {
        format!("{} {}", name.trim(), name_sub.trim())
    } else if !name.trim().is_empty() {
        name.trim().to_owned()
    } else if !abbr1.trim().is_empty() {
        abbr1.trim().to_owned()
    } else {
        fallback.to_owned()
    }
}

fn nonempty_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn snippet(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

pub fn escape_plain_label_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

pub fn html_unescape_minimal(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

pub fn escape_sql_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn find_file_by_name(root: &Path, filename: &str) -> Result<Option<PathBuf>> {
    let Ok(metadata) = fs::symlink_metadata(root) else {
        return Ok(None);
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(None);
    }
    let canonical_root = fs::canonicalize(root)?;
    let needle = filename.to_lowercase();
    let mut visited = std::collections::BTreeSet::new();
    visited.insert(canonical_root.clone());
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut entries_seen = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        if depth > 32 || entries_seen > 20_000 {
            continue;
        }
        for entry in fs::read_dir(&dir)? {
            entries_seen += 1;
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                let Ok(canonical) = fs::canonicalize(&path) else {
                    continue;
                };
                if canonical.starts_with(&canonical_root) && visited.insert(canonical) {
                    stack.push((path, depth + 1));
                }
            } else if file_type.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().to_lowercase() == needle)
            {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}

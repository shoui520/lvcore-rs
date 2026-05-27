use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

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
}

impl LvedSqliteStore {
    pub fn discover(root: &Path) -> Result<Option<Self>> {
        let Some(payload_path) = lved_payload_path(root)? else {
            return Ok(None);
        };
        let key_file = discover_lved_key_file(&payload_path)?;
        Ok(Some(Self {
            payload_path,
            key_file,
        }))
    }

    pub fn open_readonly(&self) -> Result<Connection> {
        let connection = Connection::open_with_flags(
            &self.payload_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        if let Some(key_file) = &self.key_file {
            let key = read_lved_key_file(&key_file.path)?;
            apply_sqlcipher_key(&connection, &key)?;
        }
        validate_sqlite_connection(&connection)?;
        Ok(connection)
    }

    pub fn table_names(&self) -> Result<Vec<String>> {
        let connection = self.open_readonly()?;
        sqlite_table_names(&connection)
    }

    pub fn title(&self) -> Result<Option<String>> {
        let connection = self.open_readonly()?;
        Ok(lved_sqlite_title_from_connection(&connection))
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
    Ok(dbc_files.into_iter().next())
}

pub fn is_lved_payload_name(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    name == "main.data" || name.ends_with(".dbc")
}

pub fn infer_lved_dict_code(payload_path: &Path) -> Option<String> {
    if payload_path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("main.data"))
    {
        return payload_path
            .parent()
            .and_then(|parent| parent.file_name())
            .map(|name| strip_dct_prefix(&name.to_string_lossy()));
    }
    payload_path
        .file_stem()
        .map(|name| strip_dct_prefix(&name.to_string_lossy()))
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
                when lower(name) like '%about%' then 0
                when lower(name) like '%hanrei%' then 1
                when lower(name) like '%copyright%' then 2
                when lower(name) like '%license%' then 3
                else 6
              end,
              rowid
            limit 128
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
    let Ok(mut statement) =
        connection.prepare(&format!("pragma table_info({})", quote_identifier(table)))
    else {
        return false;
    };
    let Ok(rows) = statement.query_map([], |row| row.get::<_, String>(1)) else {
        return false;
    };
    let columns = rows
        .filter_map(std::result::Result::ok)
        .map(|column| column.to_lowercase())
        .collect::<Vec<_>>();
    required
        .iter()
        .all(|column| columns.iter().any(|found| found == &column.to_lowercase()))
}

fn html_text_lines(fragment: &str) -> Vec<String> {
    let mut text = String::with_capacity(fragment.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for ch in fragment.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let tag_name = tag.trim_start_matches('/').trim().to_lowercase();
                if matches!(tag_name.as_str(), "br" | "br/" | "p" | "div" | "li" | "tr") {
                    text.push('\n');
                }
            }
            _ if in_tag => tag.push(ch),
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
    if value.contains('。') || value.contains('．') || value.contains("この辞書") {
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
    if source_name.contains("copyright") || source_name.contains("license") {
        score -= 20;
    }
    if source_name.contains("index") {
        score -= 120;
    }
    for weak in [
        "凡例",
        "索引",
        "一覧",
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

fn strip_dct_prefix(value: &str) -> String {
    value.strip_prefix("_DCT_").unwrap_or(value).to_owned()
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
    fn title_probe_rejects_common_false_positive_shapes() {
        assert!(normalize_title_candidate("外国語は片仮名で表記した．").is_none());
        assert!(title_score("和英小辞典", "index.html") < 100);
        assert_eq!(
            normalize_title_candidate("『広辞苑 第七版』　　&copy;2018年").as_deref(),
            Some("広辞苑 第七版")
        );
    }
}

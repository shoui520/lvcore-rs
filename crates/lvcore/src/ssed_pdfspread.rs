use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::error::Result;
use crate::storage::regular_file_inside_root;

const FULLWIDTH_DIGITS: [char; 10] = ['０', '１', '２', '３', '４', '５', '６', '７', '８', '９'];
const SPECIAL_RIGHT_PAGES: [u32; 7] = [17, 69, 177, 241, 295, 305, 369];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfSpreadLookup {
    pub page_id: String,
    pub page_number: u32,
    pub side: PdfSpreadSide,
    pub id_right: String,
    pub id_left: Option<String>,
    pub pdf: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfSpreadSide {
    Right,
    Left,
}

impl PdfSpreadSide {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Right => "Right",
            Self::Left => "Left",
        }
    }
}

pub fn normalize_pdfspread_page_id(value: &str) -> Option<String> {
    let digits = page_digits(value)?;
    Some(
        digits
            .into_iter()
            .map(|digit| FULLWIDTH_DIGITS[digit as usize])
            .collect(),
    )
}

pub fn pdfspread_page_number(page_id: &str) -> Option<u32> {
    let digits = page_digits(page_id)?;
    digits.into_iter().try_fold(0_u32, |value, digit| {
        value.checked_mul(10)?.checked_add(digit)
    })
}

pub fn pdfspread_lookup_side(page_id: &str) -> Option<PdfSpreadSide> {
    let page_number = pdfspread_page_number(page_id)?;
    if SPECIAL_RIGHT_PAGES.contains(&page_number) || page_number % 2 == 0 {
        Some(PdfSpreadSide::Right)
    } else {
        Some(PdfSpreadSide::Left)
    }
}

pub fn find_pdfspread_database(root: &Path) -> Result<Option<PathBuf>> {
    let package = if root
        .parent()
        .is_some_and(|parent| regular_file_inside_root(parent, root).unwrap_or(false))
    {
        root.parent().unwrap_or(root)
    } else {
        root
    };
    if !package.is_dir() {
        return Ok(None);
    }

    let mut candidates = fs::read_dir(package)?.collect::<std::io::Result<Vec<_>>>()?;
    candidates.sort_by_key(|entry| entry.path());
    for candidate in candidates {
        let path = candidate.path();
        if !regular_file_inside_root(package, &path)? || is_metadata_noise_path(&path) {
            continue;
        }
        let suffix = path
            .extension()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(suffix.as_str(), "db" | "sqlite" | "sqlite3") {
            continue;
        }
        if sqlite_has_pdfspread(&path)? {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

pub fn sqlite_has_pdfspread(path: &Path) -> Result<bool> {
    let connection =
        match Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(connection) => connection,
            Err(_) => return Ok(false),
        };
    let has_table = connection
        .query_row(
            "select 1 from sqlite_master where type='table' and lower(name)='pdfspread' limit 1",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if !has_table {
        return Ok(false);
    }
    let mut statement = connection.prepare("pragma table_info(\"PDFSpread\")")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let has_id_right = columns
        .iter()
        .any(|column| column.eq_ignore_ascii_case("IDRight"));
    let has_pdf = columns
        .iter()
        .any(|column| column.eq_ignore_ascii_case("PDF"));
    Ok(has_id_right && has_pdf)
}

pub fn lookup_pdfspread(path: &Path, page_id: &str) -> Result<Option<PdfSpreadLookup>> {
    let Some(normalized) = normalize_pdfspread_page_id(page_id) else {
        return Ok(None);
    };
    let Some(side) = pdfspread_lookup_side(&normalized) else {
        return Ok(None);
    };
    let Some(page_number) = pdfspread_page_number(&normalized) else {
        return Ok(None);
    };
    let id_column = match side {
        PdfSpreadSide::Right => "IDRight",
        PdfSpreadSide::Left => "IDLeft",
    };
    let query =
        format!("select IDRight, IDLeft, PDF from PDFSpread where \"{id_column}\" = ?1 limit 1");
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = connection.prepare(&query)?;
    let mut rows = statement.query([normalized.as_str()])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(PdfSpreadLookup {
        page_id: normalized,
        page_number,
        side,
        id_right: row.get::<_, String>(0)?,
        id_left: row.get::<_, Option<String>>(1)?,
        pdf: row.get::<_, Vec<u8>>(2)?,
    }))
}

fn page_digits(value: &str) -> Option<Vec<u32>> {
    let mut window = Vec::new();
    for ch in value.chars() {
        if let Some(digit) = page_digit(ch) {
            window.push(digit);
            if window.len() == 8 {
                return Some(window);
            }
        } else {
            window.clear();
        }
    }
    None
}

fn page_digit(ch: char) -> Option<u32> {
    match ch {
        '0'..='9' => Some(ch as u32 - '0' as u32),
        '０'..='９' => Some(ch as u32 - '０' as u32),
        _ => None,
    }
}

fn is_metadata_noise_path(path: &Path) -> bool {
    let Some(name) = path.file_name().map(|value| value.to_string_lossy()) else {
        return false;
    };
    name.starts_with("._") || name == ".DS_Store"
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn pdfspread_page_ids_normalize_to_fullwidth_digits() {
        assert_eq!(
            normalize_pdfspread_page_id("page 00000017"),
            Some("００００００１７".to_owned())
        );
        assert_eq!(
            normalize_pdfspread_page_id("頁００００００１９"),
            Some("００００００１９".to_owned())
        );
        assert_eq!(normalize_pdfspread_page_id("123"), None);
    }

    #[test]
    fn pdfspread_side_uses_observed_hc03e9_rules() {
        assert_eq!(
            pdfspread_lookup_side("００００００１９"),
            Some(PdfSpreadSide::Left)
        );
        assert_eq!(
            pdfspread_lookup_side("００００００１８"),
            Some(PdfSpreadSide::Right)
        );
        assert_eq!(
            pdfspread_lookup_side("００００００１７"),
            Some(PdfSpreadSide::Right)
        );
    }

    #[cfg(unix)]
    #[test]
    fn pdfspread_discovery_ignores_symlinked_database_escape() {
        let package = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let database = outside.path().join("pdf.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "create table PDFSpread (IDRight text primary key, IDLeft text, PDF blob);",
            )
            .unwrap();
        std::os::unix::fs::symlink(&database, package.path().join("pdf.db")).unwrap();

        assert!(find_pdfspread_database(package.path()).unwrap().is_none());
    }
}

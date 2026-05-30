use std::collections::BTreeMap;

use rusqlite::Connection;

use super::{quote_identifier, sqlite_table_names};
use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub(super) struct LvedSqliteSchema {
    tables: BTreeMap<String, Vec<String>>,
}

impl LvedSqliteSchema {
    pub(super) fn load(connection: &Connection) -> Result<Self> {
        let mut tables = BTreeMap::new();
        for table in sqlite_table_names(connection)? {
            let columns = sqlite_columns(connection, &table)?;
            tables.insert(table.to_lowercase(), columns);
        }
        Ok(Self { tables })
    }

    pub(super) fn table_exists(&self, table: &str) -> bool {
        self.tables.contains_key(&table.to_lowercase())
    }

    pub(super) fn columns(&self, table: &str) -> &[String] {
        self.tables
            .get(&table.to_lowercase())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub(super) fn table_has_columns(&self, table: &str, required: &[&str]) -> bool {
        let columns = self.columns(table);
        required.iter().all(|column| has_column(columns, column))
    }
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

pub(super) fn has_column(columns: &[String], column: &str) -> bool {
    columns.iter().any(|found| found == &column.to_lowercase())
}

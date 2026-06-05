use super::*;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SsedIosAddressConverter {
    mappings: BTreeMap<(u32, u32), (u32, u32)>,
}

impl SsedIosAddressConverter {
    pub(super) fn convert(&self, block: u32, offset: u32) -> (u32, u32) {
        self.mappings
            .get(&(block, offset))
            .copied()
            .unwrap_or((block, offset))
    }
}

impl ReaderBookPackage {
    pub(super) fn convert_ios_ssed_address(&self, block: u32, offset: u32) -> Result<(u32, u32)> {
        let Some(converter) = self.ssed_ios_address_converter()? else {
            return Ok((block, offset));
        };
        Ok(converter.convert(block, offset))
    }

    fn ssed_ios_address_converter(&self) -> Result<Option<&SsedIosAddressConverter>> {
        let converter = self.ssed_ios_address_converter.get_or_init(|| {
            load_ios_address_converter(&self.retained_ios_convert_addr_payloads)
                .map_err(|error| error.to_string())
        });
        match converter {
            Ok(converter) => Ok(converter.as_ref()),
            Err(error) => Err(Error::Driver(error.clone())),
        }
    }
}

fn load_ios_address_converter(
    payloads: &[IosDictConvertAddrPayload],
) -> Result<Option<SsedIosAddressConverter>> {
    let mut mappings = BTreeMap::new();
    for payload in payloads {
        if !payload.absolute_path.is_file() {
            continue;
        }
        let connection = open_converter_connection(&payload.absolute_path)?;
        for table in sqlite_table_names(&connection)? {
            let columns = sqlite_columns(&connection, &table)?;
            if !has_convert_addr_columns(&columns) {
                continue;
            }
            load_converter_table(&connection, &table, &mut mappings)?;
        }
    }
    Ok((!mappings.is_empty()).then_some(SsedIosAddressConverter { mappings }))
}

fn load_converter_table(
    connection: &Connection,
    table: &str,
    mappings: &mut BTreeMap<(u32, u32), (u32, u32)>,
) -> Result<()> {
    let sql = format!(
        "select {}, {}, {}, {} from {}",
        quote_sql_identifier("o_Block"),
        quote_sql_identifier("o_Offset"),
        quote_sql_identifier("n_Block"),
        quote_sql_identifier("n_Offset"),
        quote_sql_identifier(table),
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([], |row| {
        Ok((
            sqlite_value_to_u32(row.get_ref(0)?)?,
            sqlite_value_to_u32(row.get_ref(1)?)?,
            sqlite_value_to_u32(row.get_ref(2)?)?,
            sqlite_value_to_u32(row.get_ref(3)?)?,
        ))
    })?;
    for row in rows {
        let (old_block, old_offset, new_block, new_offset) = row?;
        mappings.insert((old_block, old_offset), (new_block, new_offset));
    }
    Ok(())
}

fn has_convert_addr_columns(columns: &[String]) -> bool {
    ["o_Block", "o_Offset", "n_Block", "n_Offset"]
        .into_iter()
        .all(|expected| {
            columns
                .iter()
                .any(|column| column.eq_ignore_ascii_case(expected))
        })
}

fn open_converter_connection(path: &Path) -> Result<Connection> {
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

fn sqlite_value_to_u32(value: ValueRef<'_>) -> rusqlite::Result<u32> {
    match value {
        ValueRef::Integer(value) => Ok(value.max(0) as u32),
        ValueRef::Text(value) => Ok(String::from_utf8_lossy(value)
            .trim()
            .parse::<u32>()
            .unwrap_or(0)),
        ValueRef::Blob(value) => Ok(String::from_utf8_lossy(value)
            .trim()
            .parse::<u32>()
            .unwrap_or(0)),
        ValueRef::Real(value) => Ok(value.max(0.0) as u32),
        ValueRef::Null => Ok(0),
    }
}

fn quote_sql_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn quote_sql_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

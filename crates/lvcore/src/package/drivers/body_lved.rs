use super::*;

impl ReaderBookPackage {
    pub(super) fn visual_body_for_lved_row(&self, table: &str, row_id: i64) -> Result<VisualBody> {
        if table.eq_ignore_ascii_case("info") {
            return self.visual_body_for_lved_info_row(row_id);
        }
        if !table.eq_ignore_ascii_case("content") {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 target table is not renderable yet".to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "lved_row_table_deferred",
                    format!("LVED_SQLITE3 table {table} is not a renderable content table"),
                )],
            });
        }
        let Some(store) = &self.lved_store else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 content targets require an opened SQLCipher store",
                )],
            });
        };
        let Some(html) = store.content_html(row_id)? else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 content row was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "lved_content_missing",
                    format!("LVED_SQLITE3 content row {row_id} was not found"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html,
            source: BodySourceKind::LvedSqlite,
        })
    }

    fn visual_body_for_lved_info_row(&self, row_id: i64) -> Result<VisualBody> {
        let Some(store) = &self.lved_store else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 info targets require an opened SQLCipher store",
                )],
            });
        };
        let Some(html) = store.info_html(row_id)? else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 info row was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "lved_info_missing",
                    format!("LVED_SQLITE3 info row {row_id} was not found"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html,
            source: BodySourceKind::LvedSqlite,
        })
    }

    pub(super) fn visual_body_for_lved_info_name(&self, name: &str) -> Result<VisualBody> {
        let Some(store) = &self.lved_store else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 info targets require an opened SQLCipher store",
                )],
            });
        };
        let Some(html) = store.info_html_by_name(name)? else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 info page was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "lved_info_missing",
                    format!("LVED_SQLITE3 info page {name} was not found"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html,
            source: BodySourceKind::LvedSqlite,
        })
    }

    pub(super) fn visual_body_for_lved_named_page(
        &self,
        table: &str,
        name: &str,
    ) -> Result<VisualBody> {
        let Some(store) = &self.lved_store else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 named page targets require an opened SQLCipher store",
                )],
            });
        };
        let Some(html) = store.named_html_by_name(table, name)? else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 named page was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "lved_named_page_missing",
                    format!("LVED_SQLITE3 {table} page {name} was not found"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html,
            source: BodySourceKind::LvedSqlite,
        })
    }
}

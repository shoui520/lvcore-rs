use super::*;

impl ReaderBookPackage {
    pub(super) fn visual_body_for_hourei_law(&self, hore_id: &str) -> Result<VisualBody> {
        let Some(store) = &self.hourei_store else {
            return Ok(VisualBody::Unsupported {
                reason: "Hourei store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "hourei_store_missing",
                    "Hourei law targets require an opened Hourei store",
                )],
            });
        };
        let Some(html) = store.law_html(hore_id)? else {
            return Ok(VisualBody::Unsupported {
                reason: "Hourei law body was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "hourei_law_missing",
                    format!("Hourei law {hore_id} was not found in cached HTML or law shard DB"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html,
            source: BodySourceKind::HoureiSqlite,
        })
    }
}

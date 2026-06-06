use super::*;

impl ReaderBookPackage {
    pub(super) fn visual_body_for_multiview_href(
        &self,
        href: &str,
        _anchor: Option<&str>,
    ) -> Result<VisualBody> {
        let Some(store) = &self.multiview_store else {
            return Ok(VisualBody::Unsupported {
                reason: "LVLMultiView store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "multiview_store_missing",
                    "LVLMultiView targets require opened LogoFontCipher SQLite payloads",
                )],
            });
        };
        let Some(body) = store.body_for_href(href)? else {
            return Ok(VisualBody::Unsupported {
                reason: "LVLMultiView target was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "multiview_target_missing",
                    format!("LVLMultiView target {href} was not found in decoded payloads"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html: body.html,
            source: BodySourceKind::LvlMultiViewSqlite,
        })
    }
}

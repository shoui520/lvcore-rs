use super::*;

impl SearchProvider for ReaderBookPackage {
    fn search(&self, query: &SearchQuery) -> Result<SearchPage> {
        let mut page =
            if self.metadata.format_family == FormatFamily::Ssed {
                if self.lved_store.is_some()
                    && self.metadata.search_modes.contains(&query.mode)
                    && self.ssed_catalog.as_ref().is_none_or(|catalog| {
                        !has_decodable_ssed_index_rows(catalog, &self.storage)
                    })
                {
                    self.search_lved_sqlite(query)?
                } else if !self.retained_ios_fts_payloads.is_empty()
                    && self.metadata.search_modes.contains(&query.mode)
                    && self.ssed_catalog.as_ref().is_none_or(|catalog| {
                        !has_decodable_ssed_index_rows(catalog, &self.storage)
                    })
                {
                    SearchPage {
                        hits: Vec::new(),
                        next_cursor: None,
                        result_sequence: None,
                        diagnostics: self.retained_ios_fts_deferred_diagnostics(),
                    }
                } else {
                    self.search_ssed_simple_indexes(query)?
                }
            } else if self.metadata.format_family == FormatFamily::LvedSqlite3 {
                self.search_lved_sqlite(query)?
            } else if self.metadata.format_family == FormatFamily::LvlMultiView {
                self.search_multiview(query)?
            } else if self.metadata.format_family == FormatFamily::Hourei {
                self.search_hourei(query)?
            } else {
                SearchPage::deferred(format!(
                    "{} search provider is not implemented yet",
                    self.metadata.format_label
                ))
            };
        page.attach_result_sequence()?;
        Ok(page)
    }
}

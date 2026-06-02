use super::*;

impl SearchProvider for ReaderBookPackage {
    fn search(&self, query: &SearchQuery) -> Result<SearchPage> {
        if self.metadata.format_family == FormatFamily::Ssed {
            if self.lved_store.is_some()
                && self.metadata.search_modes.contains(&query.mode)
                && self
                    .ssed_catalog
                    .as_ref()
                    .is_none_or(|catalog| !has_decodable_ssed_index_rows(catalog, &self.storage))
            {
                return self.search_lved_sqlite(query);
            }
            if !self.retained_ios_fts_payloads.is_empty()
                && self.metadata.search_modes.contains(&query.mode)
                && self
                    .ssed_catalog
                    .as_ref()
                    .is_none_or(|catalog| !has_decodable_ssed_index_rows(catalog, &self.storage))
            {
                return Ok(SearchPage {
                    hits: Vec::new(),
                    next_cursor: None,
                    diagnostics: self.retained_ios_fts_deferred_diagnostics(),
                });
            }
            return self.search_ssed_simple_indexes(query);
        }
        if self.metadata.format_family == FormatFamily::LvedSqlite3 {
            return self.search_lved_sqlite(query);
        }
        if self.metadata.format_family == FormatFamily::LvlMultiView {
            return self.search_multiview(query);
        }
        if self.metadata.format_family == FormatFamily::Hourei {
            return self.search_hourei(query);
        }
        Ok(SearchPage::deferred(format!(
            "{} search provider is not implemented yet",
            self.metadata.format_label
        )))
    }
}

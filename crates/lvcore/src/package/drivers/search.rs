use super::*;

impl SearchProvider for ReaderBookPackage {
    fn search(&self, query: &SearchQuery) -> Result<SearchPage> {
        let has_decodable_ssed_indexes = self
            .ssed_catalog
            .as_ref()
            .is_some_and(|catalog| has_decodable_ssed_index_rows(catalog, &self.storage));
        let mut page = if self.lved_store.is_some()
            && self.metadata.search_modes.contains(&query.mode)
        {
            self.search_lved_sqlite(query)?
        } else if has_decodable_ssed_indexes {
            self.search_ssed_simple_indexes(query)?
        } else if !self.retained_ios_fts_payloads.is_empty()
            && self.metadata.search_modes.contains(&query.mode)
        {
            SearchPage {
                hits: Vec::new(),
                next_cursor: None,
                result_sequence: None,
                diagnostics: self.retained_ios_fts_deferred_diagnostics(),
            }
        } else if self.metadata.search_modes.contains(&query.mode)
            && self.has_ssed_sizk_surface()?
        {
            self.search_ssed_sizk(query)?
        } else if self.multiview_store.is_some()
            || self.metadata.format_family == FormatFamily::LvlMultiView
        {
            self.search_multiview(query)?
        } else if self.hourei_store.is_some() || self.metadata.format_family == FormatFamily::Hourei
        {
            self.search_hourei(query)?
        } else if self.ssed_catalog.is_some() || self.metadata.format_family == FormatFamily::Ssed {
            self.search_ssed_simple_indexes(query)?
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

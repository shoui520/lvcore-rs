use super::*;

impl SearchProvider for ReaderBookPackage {
    fn search(&self, query: &SearchQuery) -> Result<SearchPage> {
        if self.metadata.format_family == FormatFamily::Ssed {
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

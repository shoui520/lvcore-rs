use super::*;

impl ReaderBookPackage {
    pub(super) fn search_hourei(&self, query: &SearchQuery) -> Result<SearchPage> {
        let Some(store) = &self.hourei_store else {
            return Ok(SearchPage::deferred(
                "Hourei search requires an opened Hourei store",
            ));
        };
        if query.limit == 0 {
            return Ok(SearchPage {
                hits: Vec::new(),
                next_cursor: None,
                diagnostics: Vec::new(),
            });
        }
        let offset = decode_offset_cursor(query.cursor.as_deref());
        let page_limit = query.limit.saturating_add(1);
        let mut raw_hits = store.search_page(&query.query, &query.mode, offset, page_limit)?;
        let next_cursor =
            (raw_hits.len() > query.limit).then(|| (offset + query.limit).to_string());
        raw_hits.truncate(query.limit);
        let hits = raw_hits
            .into_iter()
            .map(|hit| {
                let target = TargetToken::new(&InternalTarget::HoureiLaw {
                    hore_id: hit.hore_id,
                    anchor: None,
                })?;
                let href = target.href();
                Ok(SearchHit {
                    href,
                    book_id: self.metadata.book_id.clone(),
                    target,
                    title_html: hit.title_html,
                    title_text: hit.title_text,
                    snippet_html: hit.snippet_html,
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(SearchPage {
            hits,
            next_cursor,
            diagnostics: Vec::new(),
        })
    }
}

use super::*;

const LVED_UNVERIFIED_OFFSET_CURSOR_PREFIX: &str = "lved-offset-unverified:";

impl ReaderBookPackage {
    pub(super) fn search_lved_sqlite(&self, query: &SearchQuery) -> Result<SearchPage> {
        let Some(store) = &self.lved_store else {
            return Ok(SearchPage::deferred(
                "LVED_SQLITE3 search requires an opened SQLCipher store",
            ));
        };
        if query.limit == 0 {
            return Ok(SearchPage {
                hits: Vec::new(),
                next_cursor: None,
                result_sequence: None,
                diagnostics: Vec::new(),
            });
        }
        if let Some(retained_offset) =
            self.decode_lved_retained_index_cursor(query.cursor.as_deref())
        {
            let page_limit = query.limit.saturating_add(1);
            let mut page =
                self.search_lved_retained_ssed_indexes(query, &[], page_limit, retained_offset)?;
            if page.hits.len() > query.limit {
                page.hits.truncate(query.limit);
            }
            return Ok(page);
        }
        let offset = decode_lved_unverified_offset_cursor(query.cursor.as_deref())
            .unwrap_or_else(|| decode_offset_cursor(query.cursor.as_deref()));
        let defer_offset_overfetch = query.cursor.is_some();
        let page_limit = if defer_offset_overfetch {
            query.limit
        } else {
            query.limit.saturating_add(1)
        };
        let mut raw_hits = store.search_page(&query.query, &query.mode, offset, page_limit)?;
        let mut next_cursor = if raw_hits.len() > query.limit {
            Some((offset + query.limit).to_string())
        } else if defer_offset_overfetch && query.limit > 0 && raw_hits.len() == query.limit {
            Some(encode_lved_unverified_offset_cursor(offset + query.limit))
        } else {
            None
        };
        raw_hits.truncate(query.limit);
        let mut hits = raw_hits
            .into_iter()
            .map(|hit| {
                let target = TargetToken::new(&InternalTarget::LvedRow {
                    table: "content".to_owned(),
                    row_id: hit.content_id,
                    anchor: hit.anchor,
                    query: None,
                })?;
                let href = target.href();
                let title_html = self.normalize_lved_label_html(&hit.title_html)?;
                let snippet_html = if hit.subtitle_html.is_empty() {
                    None
                } else {
                    Some(self.normalize_lved_label_html(&hit.subtitle_html)?)
                };
                Ok(SearchHit {
                    href,
                    book_id: self.metadata.book_id.clone(),
                    target,
                    title_html,
                    title_text: hit.title_text,
                    snippet_html,
                    sequence_hint: None,
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut diagnostics = Vec::new();
        if next_cursor.is_none() && hits.len() < query.limit {
            let retained_limit = query.limit.saturating_sub(hits.len()).saturating_add(1);
            let mut retained_page =
                self.search_lved_retained_ssed_indexes(query, &hits, retained_limit, 0)?;
            next_cursor = retained_page.next_cursor.take();
            diagnostics.extend(retained_page.diagnostics);
            hits.extend(retained_page.hits);
            if hits.len() > query.limit {
                hits.truncate(query.limit);
            }
        }
        Ok(SearchPage {
            hits,
            next_cursor,
            result_sequence: None,
            diagnostics,
        })
    }
}

fn decode_lved_unverified_offset_cursor(cursor: Option<&str>) -> Option<usize> {
    let cursor = cursor?.strip_prefix(LVED_UNVERIFIED_OFFSET_CURSOR_PREFIX)?;
    cursor.parse().ok()
}

fn encode_lved_unverified_offset_cursor(offset: usize) -> String {
    format!("{LVED_UNVERIFIED_OFFSET_CURSOR_PREFIX}{offset}")
}

use super::*;

impl ReaderBookPackage {
    pub(super) fn open_lved_list_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        let Some(store) = &self.lved_store else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 list surface requires an opened SQLCipher store",
                )],
            });
        };
        if limit == 0 {
            return Ok(NavigationSurface::TitleIndexBrowse {
                surface_id: surface_id.to_owned(),
                items: Vec::new(),
                next_cursor: None,
            });
        }
        let offset = decode_offset_cursor(cursor);
        let mut rows = store.list_items_page(offset, limit.saturating_add(1))?;
        let next_cursor = (rows.len() > limit).then(|| (offset + limit).to_string());
        rows.truncate(limit);
        if rows.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "surface_missing",
                    "LVED_SQLITE3 list table did not expose renderable rows",
                )],
            });
        }
        let items = rows
            .into_iter()
            .map(|row| {
                let label_html = self.normalize_lved_label_html(&lved_list_label_html(
                    &row.title_html,
                    &row.subtitle_html,
                ))?;
                let label_text = if row.subtitle_html.is_empty() {
                    row.title_text.clone()
                } else {
                    format!("{} {}", row.title_text, html_label_text(&row.subtitle_html))
                };
                Ok(NavigationItem {
                    href: String::new(),
                    item_id: row.list_id.to_string(),
                    label_html,
                    label_text,
                    target: TargetToken::new(&InternalTarget::LvedRow {
                        table: "content".to_owned(),
                        row_id: row.content_id,
                        anchor: row.anchor,
                        query: None,
                    })?,
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::TitleIndexBrowse {
            surface_id: surface_id.to_owned(),
            items,
            next_cursor,
        })
    }

    pub(super) fn open_lved_info_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        let Some(store) = &self.lved_store else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 info surface requires an opened SQLCipher store",
                )],
            });
        };
        if limit == 0 {
            return Ok(NavigationSurface::InfoPages {
                surface_id: surface_id.to_owned(),
                pages: Vec::new(),
                next_cursor: None,
            });
        }
        let offset = decode_offset_cursor(cursor);
        let mut pages = store.info_pages_page(offset, limit.saturating_add(1))?;
        let next_cursor = (pages.len() > limit).then(|| (offset + limit).to_string());
        pages.truncate(limit);
        if pages.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "surface_missing",
                    "LVED_SQLITE3 info table did not expose renderable pages",
                )],
            });
        }
        let items = pages
            .into_iter()
            .map(|page| {
                Ok(NavigationItem {
                    href: String::new(),
                    item_id: page.name,
                    label_html: sanitize_rich_label_html(&page.title_html),
                    label_text: page.title_text,
                    target: TargetToken::new(&InternalTarget::LvedRow {
                        table: "info".to_owned(),
                        row_id: page.id,
                        anchor: None,
                        query: None,
                    })?,
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::InfoPages {
            surface_id: surface_id.to_owned(),
            pages: items,
            next_cursor,
        })
    }

    pub(super) fn open_lved_named_page_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        let Some(store) = &self.lved_store else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 named page surface requires an opened SQLCipher store",
                )],
            });
        };
        if limit == 0 {
            return Ok(NavigationSurface::InfoPages {
                surface_id: surface_id.to_owned(),
                pages: Vec::new(),
                next_cursor: None,
            });
        }
        let offset = decode_offset_cursor(cursor);
        let mut pages = store.named_pages_page(surface_id, offset, limit.saturating_add(1))?;
        let next_cursor = (pages.len() > limit).then(|| (offset + limit).to_string());
        pages.truncate(limit);
        if pages.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "surface_missing",
                    format!(
                        "LVED_SQLITE3 {surface_id} table did not expose renderable named pages"
                    ),
                )],
            });
        }
        let items = pages
            .into_iter()
            .map(|page| {
                Ok(NavigationItem {
                    href: String::new(),
                    item_id: page.name.clone(),
                    label_html: sanitize_rich_label_html(&page.title_html),
                    label_text: page.title_text,
                    target: TargetToken::new(&InternalTarget::LvedNamedPage {
                        table: surface_id.to_owned(),
                        name: page.name,
                        anchor: None,
                    })?,
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::InfoPages {
            surface_id: surface_id.to_owned(),
            pages: items,
            next_cursor,
        })
    }

    pub(super) fn open_lved_tree_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        let Some(store) = &self.lved_store else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 tree surface requires an opened SQLCipher store",
                )],
            });
        };
        if limit == 0 {
            return Ok(NavigationSurface::HierarchicalTree {
                surface_id: surface_id.to_owned(),
                nodes: Vec::new(),
                next_cursor: None,
            });
        }
        let rows = store.tree_index_items_arc()?;
        if rows.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "surface_missing",
                    "LVED_SQLITE3 tree.idx did not expose navigation rows",
                )],
            });
        }
        let (nodes, next_cursor) = lved_tree_items_to_nodes_page(rows.as_ref(), cursor, limit)?;
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
            next_cursor,
        })
    }
}

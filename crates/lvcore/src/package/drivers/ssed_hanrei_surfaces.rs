use super::*;

impl ReaderBookPackage {
    pub(super) fn open_ssed_hanrei_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        if cursor.is_none()
            && limit > 0
            && let Some(nodes) = self.discover_ssed_hanrei_chm_toc_nodes("HANREI.chm")?
        {
            return Ok(NavigationSurface::HierarchicalTree {
                surface_id: surface_id.to_owned(),
                nodes,
            });
        }
        if limit == 0 {
            return Ok(NavigationSurface::InfoPages {
                surface_id: surface_id.to_owned(),
                pages: Vec::new(),
                next_cursor: None,
            });
        }
        let offset = decode_offset_cursor(cursor);
        let mut pages = self.discover_ssed_hanrei_pages()?;
        if pages.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_hanrei_missing",
                    "SSED HANREI files were not found",
                )],
            });
        }
        let next_cursor = (pages.len() > offset + limit).then(|| (offset + limit).to_string());
        pages = pages.into_iter().skip(offset).take(limit).collect();
        let items = pages
            .into_iter()
            .map(|page| {
                let resource = ResourceToken::new(&page.resource)?;
                Ok(NavigationItem {
                    item_id: page.item_id,
                    label_html: escape_plain_label_html(&page.label),
                    label_text: page.label,
                    target: TargetToken::new(&InternalTarget::Resource {
                        resource,
                        anchor: page.anchor,
                    })?,
                    diagnostics: page.diagnostics,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::InfoPages {
            surface_id: surface_id.to_owned(),
            pages: items,
            next_cursor,
        })
    }

    fn discover_ssed_hanrei_chm_toc_nodes(
        &self,
        chm_path: &str,
    ) -> Result<Option<Vec<NavigationNode>>> {
        if !self.storage.exists(Path::new(chm_path))? {
            return Ok(None);
        }
        let Some(resolved) = self.storage.resolve_casefolded(Path::new(chm_path))? else {
            return Ok(None);
        };
        let Ok(entries) = list_chm_entries(&resolved) else {
            return Ok(None);
        };
        let mut toc_items = Vec::new();
        for entry in &entries {
            if !path_has_extension(&entry.path, &["hhc"]) {
                continue;
            }
            let Ok(bytes) = read_chm_entry(&resolved, &entry.path) else {
                continue;
            };
            let html = decode_package_html_text(&bytes);
            toc_items.extend(parse_chm_hhc_toc(&html));
        }
        if toc_items.is_empty() {
            return Ok(None);
        }
        let nodes = chm_hhc_toc_items_to_nodes(chm_path, &toc_items)?;
        Ok((!nodes.is_empty()).then_some(nodes))
    }
}

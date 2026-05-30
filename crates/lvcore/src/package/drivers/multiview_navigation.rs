use super::*;

impl ReaderBookPackage {
    pub(super) fn open_multiview_menu_surface(
        &self,
        surface_id: &str,
    ) -> Result<NavigationSurface> {
        let bytes = self.storage.read(Path::new("menuData.xml"))?;
        let xml = String::from_utf8(bytes)
            .map_err(|error| Error::Driver(format!("menuData.xml is not valid UTF-8: {error}")))?;
        let items = parse_menu_data(&xml)?;
        let nodes = items
            .iter()
            .enumerate()
            .map(|(index, item)| multiview_menu_item_to_node(item, &index.to_string()))
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
        })
    }

    pub(super) fn multiview_navigation_surface_for_href(
        &self,
        href: &str,
    ) -> Result<Option<(String, NavigationSurface)>> {
        let Some(store) = &self.multiview_store else {
            return Ok(None);
        };
        let Some(list) = store.law_list_for_href(href)? else {
            return Ok(None);
        };
        let title = list.title;
        let items = list
            .items
            .into_iter()
            .map(|item| {
                let target = TargetToken::new(&InternalTarget::MultiviewHref {
                    href: item.code.clone(),
                    anchor: None,
                })?;
                let label_text = if item.kana.is_empty() {
                    item.name
                } else {
                    format!("{} ({})", item.name, item.kana)
                };
                Ok(NavigationItem {
                    item_id: item.code,
                    label_html: escape_plain_label_html(&label_text),
                    label_text,
                    target,
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Some((
            title,
            NavigationSurface::TitleIndexBrowse {
                surface_id: format!("multiview:{href}"),
                items,
                next_cursor: None,
            },
        )))
    }
}

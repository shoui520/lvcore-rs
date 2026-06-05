use super::*;

impl ReaderBookPackage {
    pub(super) fn multiview_menu_surface_files(&self) -> Result<Vec<String>> {
        multiview_menu_files(&self.root)
    }

    pub(super) fn multiview_menu_file_for_surface(&self, surface_id: &str) -> Result<String> {
        let files = self.multiview_menu_surface_files()?;
        let index = multiview_menu_surface_index(surface_id).ok_or_else(|| {
            Error::Driver(format!("{surface_id} is not a MultiView menu surface"))
        })?;
        files
            .get(index)
            .cloned()
            .ok_or_else(|| Error::Driver(format!("MultiView menu surface {surface_id} is missing")))
    }

    pub(super) fn multiview_menu_surface_id_for_href(&self, href: &str) -> Result<Option<String>> {
        Ok(self
            .multiview_menu_surface_files()?
            .iter()
            .position(|candidate| candidate == href)
            .map(multiview_menu_surface_id))
    }

    pub(super) fn open_multiview_menu_surface(
        &self,
        surface_id: &str,
    ) -> Result<NavigationSurface> {
        let menu_file = self.multiview_menu_file_for_surface(surface_id)?;
        let bytes = self.storage.read(Path::new(&menu_file))?;
        let xml = String::from_utf8(bytes)
            .map_err(|error| Error::Driver(format!("{menu_file} is not valid UTF-8: {error}")))?;
        let items = parse_menu_data(&xml)?;
        let nodes = items
            .iter()
            .enumerate()
            .map(|(index, item)| multiview_menu_item_to_node(item, &index.to_string()))
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
            next_cursor: None,
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
                    href: String::new(),
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

pub(super) fn multiview_menu_surface_id(index: usize) -> String {
    if index == 0 {
        "menuData".to_owned()
    } else {
        format!("menuData:{index}")
    }
}

fn multiview_menu_surface_index(surface_id: &str) -> Option<usize> {
    if surface_id == "menuData" {
        return Some(0);
    }
    surface_id.strip_prefix("menuData:")?.parse().ok()
}

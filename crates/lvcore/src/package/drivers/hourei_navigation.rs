use super::*;

const HOUREI_KANA_PANEL_PATH: &str = "_Programs/index_panel.html";
const HOUREI_KANA_PANEL_SURFACE_ID: &str = "kana-panel";
const HOUREI_KANA_SURFACE_PREFIX: &str = "hourei-kana:";

impl ReaderBookPackage {
    pub(super) fn has_hourei_kana_panel(&self) -> Result<bool> {
        self.storage.exists(Path::new(HOUREI_KANA_PANEL_PATH))
    }

    pub(super) fn open_hourei_kana_panel_surface(
        &self,
        surface_id: &str,
    ) -> Result<NavigationSurface> {
        let bytes = self.storage.read(Path::new(HOUREI_KANA_PANEL_PATH))?;
        let html = decode_package_html_text(&bytes);
        let cells = hourei_kana_panel_cells(surface_id, &html)?;
        if cells.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "hourei_kana_panel_empty",
                    "Hourei kana panel HTML did not contain any usable cells",
                )],
            });
        }
        Ok(NavigationSurface::Panel {
            surface_id: surface_id.to_owned(),
            cells,
            next_cursor: None,
        })
    }

    pub(super) fn open_hourei_kana_initial_surface(
        &self,
        surface_id: &str,
        kana_initial: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        let Some(store) = &self.hourei_store else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "hourei_store_missing",
                    "Hourei kana browse requires an opened Hourei store",
                )],
            });
        };
        let offset = decode_offset_cursor(cursor);
        let page_limit = limit.saturating_add(1);
        let mut laws = store.laws_by_kana_initial(kana_initial, offset, page_limit)?;
        let next_cursor = (laws.len() > limit).then(|| (offset + limit).to_string());
        laws.truncate(limit);
        let items = laws
            .into_iter()
            .map(|law| {
                let label = hourei_law_node_label(&law);
                Ok(NavigationItem {
                    href: String::new(),
                    item_id: format!("law:{}", law.hore_id),
                    label_html: escape_hourei_label_html(&label),
                    label_text: label,
                    target: TargetToken::new(&InternalTarget::HoureiLaw {
                        hore_id: law.hore_id,
                        anchor: None,
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

    pub(super) fn open_hourei_law_tree_surface(
        &self,
        surface_id: &str,
    ) -> Result<NavigationSurface> {
        let Some(store) = &self.hourei_store else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "hourei_store_missing",
                    "Hourei law tree requires an opened Hourei store",
                )],
            });
        };
        let categories = store.categories_with_laws()?;
        let nodes = categories
            .into_iter()
            .map(|category| {
                let children = category
                    .laws
                    .into_iter()
                    .map(|law| {
                        let label = hourei_law_node_label(&law);
                        Ok(NavigationNode {
                            href: None,
                            node_id: format!("law:{}", law.hore_id),
                            label_html: escape_hourei_label_html(&label),
                            label_text: label,
                            target: Some(TargetToken::new(&InternalTarget::HoureiLaw {
                                hore_id: law.hore_id,
                                anchor: None,
                            })?),
                            diagnostics: Vec::new(),
                            children: Vec::new(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(NavigationNode {
                    href: None,
                    node_id: format!("category:{}", category.id),
                    label_html: escape_hourei_label_html(&category.name),
                    label_text: category.name,
                    target: None,
                    diagnostics: Vec::new(),
                    children,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
            next_cursor: None,
        })
    }
}

pub(super) fn hourei_kana_surface_id(kana_initial: &str) -> String {
    format!("{HOUREI_KANA_SURFACE_PREFIX}{kana_initial}")
}

pub(super) fn hourei_kana_initial_from_surface_id(surface_id: &str) -> Option<&str> {
    surface_id
        .strip_prefix(HOUREI_KANA_SURFACE_PREFIX)
        .filter(|value| !value.trim().is_empty())
}

fn hourei_kana_panel_cells(panel_id: &str, html: &str) -> Result<Vec<PanelCell>> {
    let lower = html.to_ascii_lowercase();
    let mut cells = Vec::new();
    let mut cursor = 0usize;
    let mut row = 0u32;
    while let Some(relative_start) = lower[cursor..].find("<p") {
        let start = cursor + relative_start;
        let Some(tag_end) = lower[start..].find('>').map(|offset| start + offset + 1) else {
            break;
        };
        let row_tag = &lower[start..tag_end];
        let Some(end) = lower[tag_end..].find("</p>").map(|offset| tag_end + offset) else {
            break;
        };
        if row_tag.contains("cell_line") {
            hourei_kana_panel_row_cells(panel_id, row, &html[tag_end..end], &mut cells)?;
            row = row.saturating_add(1);
        }
        cursor = end + "</p>".len();
    }
    Ok(cells)
}

fn hourei_kana_panel_row_cells(
    panel_id: &str,
    row: u32,
    html: &str,
    cells: &mut Vec<PanelCell>,
) -> Result<()> {
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0usize;
    let mut column = 0u32;
    while let Some(relative_start) = lower[cursor..].find("<a") {
        let start = cursor + relative_start;
        let Some(tag_end) = lower[start..].find('>').map(|offset| start + offset + 1) else {
            break;
        };
        let Some(end) = lower[tag_end..].find("</a>").map(|offset| tag_end + offset) else {
            break;
        };
        let tag = &html[start..tag_end];
        let label_text = html_basic_text(&html[tag_end..end]).trim().to_owned();
        let label_html = escape_plain_label_html(&label_text);
        let target = hourei_kana_panel_href(tag)?.map(|kana_initial| {
            TargetToken::new(&InternalTarget::MenuItem {
                surface_id: hourei_kana_surface_id(&kana_initial),
                item_id: "root".to_owned(),
            })
        });
        cells.push(PanelCell {
            href: None,
            panel_id: panel_id.to_owned(),
            row,
            column,
            label_html,
            label_text,
            target: target.transpose()?,
            diagnostics: Vec::new(),
        });
        column = column.saturating_add(1);
        cursor = end + "</a>".len();
    }
    Ok(())
}

fn hourei_kana_panel_href(tag: &str) -> Result<Option<String>> {
    let lower = tag.to_ascii_lowercase();
    let Some(attr) = next_html_href_or_src_attr(tag, &lower, 0) else {
        return Ok(None);
    };
    let raw_value = &tag[attr.value_start..attr.value_end];
    let value = html_unescape_minimal(raw_value).trim().to_owned();
    Ok(value
        .strip_prefix("lved_ref:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned))
}

pub(super) fn hourei_kana_panel_surface_id() -> &'static str {
    HOUREI_KANA_PANEL_SURFACE_ID
}

use super::html::escape_plain_label_html;
use crate::error::Result;
use crate::multiview::MultiviewMenuItem;
use crate::navigation::{
    HomeSurface, NavigationNode, NavigationStatus, NavigationSurfaceKind, PanelCell,
};
use crate::target::{InternalTarget, TargetToken};

#[derive(Debug, Clone)]
pub(super) struct OrderedSequenceTarget {
    pub(super) target: TargetToken,
    pub(super) title: Option<String>,
}

pub(super) fn lved_list_label_html(title_html: &str, subtitle_html: &str) -> String {
    if subtitle_html.is_empty() {
        title_html.to_owned()
    } else {
        format!(r#"{title_html}<span class="lvcore-subtitle"> {subtitle_html}</span>"#)
    }
}

pub(super) fn home_surface_reader_priority(surface: &HomeSurface) -> (u8, u8) {
    let targetable = surface.target.is_some()
        || surface
            .href
            .as_deref()
            .is_some_and(|href| !href.trim().is_empty());
    let status_group = match (surface.status, targetable) {
        (NavigationStatus::Available, true) => 0,
        (NavigationStatus::Available, false) => 1,
        (NavigationStatus::Empty, _) => 2,
        (NavigationStatus::Deferred, _) => 3,
        (NavigationStatus::Unsupported, _) => 4,
        (NavigationStatus::Missing, _) => 5,
    };
    let kind_group = match surface.kind {
        NavigationSurfaceKind::Menu | NavigationSurfaceKind::ScreenMenu => 0,
        NavigationSurfaceKind::Panel => 1,
        NavigationSurfaceKind::LawTree
        | NavigationSurfaceKind::MultiviewTree
        | NavigationSurfaceKind::LvedTree => 2,
        NavigationSurfaceKind::Hanrei => 3,
        NavigationSurfaceKind::Toc
        | NavigationSurfaceKind::MultiSelector
        | NavigationSurfaceKind::EncyclopediaIndex
        | NavigationSurfaceKind::AuxiliaryIndex => 4,
        NavigationSurfaceKind::TitleIndexBrowse => 5,
        NavigationSurfaceKind::Info => 6,
        NavigationSurfaceKind::SearchFallback => 7,
    };
    (status_group, kind_group)
}

pub(super) fn lved_tree_items_to_nodes(
    rows: &[crate::lved_sqlite::LvedTreeIndexItem],
) -> Result<Vec<NavigationNode>> {
    let mut cursor = 0usize;
    let Some(first) = rows.first() else {
        return Ok(Vec::new());
    };
    lved_tree_level_to_nodes(rows, &mut cursor, first.level)
}

pub(super) fn lved_tree_items_to_nodes_page(
    rows: &[crate::lved_sqlite::LvedTreeIndexItem],
    offset: usize,
    limit: usize,
) -> Result<(Vec<NavigationNode>, Option<String>)> {
    if rows.is_empty() || limit == 0 {
        return Ok((Vec::new(), None));
    }
    let root_level = rows[0].level;
    let mut cursor = 0usize;
    let mut root_index = 0usize;
    let mut nodes = Vec::new();
    let mut next_cursor = None;

    while cursor < rows.len() {
        let start = cursor;
        cursor += 1;
        while cursor < rows.len() && rows[cursor].level > root_level {
            cursor += 1;
        }

        if root_index >= offset {
            if nodes.len() >= limit {
                next_cursor = Some(root_index.to_string());
                break;
            }
            nodes.extend(lved_tree_items_to_nodes(&rows[start..cursor])?);
        }
        root_index += 1;
    }
    if nodes.len() >= limit && cursor < rows.len() {
        next_cursor = Some(root_index.to_string());
    }

    Ok((nodes, next_cursor))
}

fn lved_tree_level_to_nodes(
    rows: &[crate::lved_sqlite::LvedTreeIndexItem],
    cursor: &mut usize,
    level: u32,
) -> Result<Vec<NavigationNode>> {
    let mut nodes = Vec::new();
    while let Some(item) = rows.get(*cursor) {
        if item.level < level {
            break;
        }
        if item.level > level {
            nodes.extend(lved_tree_level_to_nodes(rows, cursor, item.level)?);
            continue;
        }
        let item_index = *cursor;
        *cursor += 1;
        let children = if rows
            .get(*cursor)
            .is_some_and(|next_item| next_item.level > item.level)
        {
            lved_tree_level_to_nodes(rows, cursor, rows[*cursor].level)?
        } else {
            Vec::new()
        };
        let target = if item.data_id > 0 {
            Some(TargetToken::new(&InternalTarget::LvedRow {
                table: "content".to_owned(),
                row_id: item.data_id,
                anchor: None,
                query: item.query.clone(),
            })?)
        } else {
            None
        };
        nodes.push(NavigationNode {
            href: None,
            child_cursor: None,
            node_id: format!("tree:{}:{}", item.data_id, item_index),
            label_html: escape_plain_label_html(&item.label),
            label_text: item.label.clone(),
            target,
            diagnostics: Vec::new(),
            children,
        });
    }
    Ok(nodes)
}

pub(super) fn multiview_menu_item_to_node(
    item: &MultiviewMenuItem,
    node_id: &str,
) -> Result<NavigationNode> {
    let target = multiview_menu_item_target(item)?;
    let children = item
        .children
        .iter()
        .enumerate()
        .map(|(index, child)| multiview_menu_item_to_node(child, &format!("{node_id}.{index}")))
        .collect::<Result<Vec<_>>>()?;
    Ok(NavigationNode {
        href: None,
        child_cursor: None,
        node_id: node_id.to_owned(),
        label_html: escape_plain_label_html(&item.label),
        label_text: item.label.clone(),
        target,
        diagnostics: Vec::new(),
        children,
    })
}

pub(super) fn multiview_menu_items_to_nodes_page(
    items: &[MultiviewMenuItem],
    cursor: Option<&str>,
    limit: usize,
) -> Result<(Vec<NavigationNode>, Option<String>)> {
    if limit == 0 {
        return Ok((Vec::new(), None));
    }
    let page = parse_multiview_menu_page_cursor(cursor)?;
    let (page_items, node_prefix, offset) = match page {
        MultiviewMenuPageCursor::Root { offset } => (items, None, offset),
        MultiviewMenuPageCursor::Children { path, offset } => {
            let Some(children) = multiview_menu_children_at_path(items, &path) else {
                return Ok((Vec::new(), None));
            };
            (children, Some(multiview_menu_path_node_id(&path)), offset)
        }
    };
    let page_items = page_items
        .iter()
        .enumerate()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let full_page_fits = multiview_menu_items_node_count_capped(
        page_items.iter().map(|(_, item)| *item),
        limit.saturating_add(1),
    ) <= limit;
    let nodes = page_items
        .iter()
        .map(|(index, item)| {
            let node_id = match &node_prefix {
                Some(prefix) => format!("{prefix}.{index}"),
                None => index.to_string(),
            };
            if full_page_fits {
                multiview_menu_item_to_node(item, &node_id)
            } else {
                multiview_menu_item_to_lazy_node(item, &node_id)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let next_offset = offset.saturating_add(nodes.len());
    let next_cursor =
        (next_offset < page_items_len_for_cursor(items, cursor)?).then(|| match &node_prefix {
            Some(prefix) => format!("children:{prefix}:{next_offset}"),
            None => next_offset.to_string(),
        });
    Ok((nodes, next_cursor))
}

fn multiview_menu_item_to_lazy_node(
    item: &MultiviewMenuItem,
    node_id: &str,
) -> Result<NavigationNode> {
    let target = multiview_menu_item_target(item)?;
    Ok(NavigationNode {
        href: None,
        child_cursor: (!item.children.is_empty()).then(|| format!("children:{node_id}:0")),
        node_id: node_id.to_owned(),
        label_html: escape_plain_label_html(&item.label),
        label_text: item.label.clone(),
        target,
        diagnostics: Vec::new(),
        children: Vec::new(),
    })
}

fn multiview_menu_item_target(item: &MultiviewMenuItem) -> Result<Option<TargetToken>> {
    let Some(href) = item.href.as_ref() else {
        return Ok(None);
    };
    if is_multiview_menu_navigation_command_href(href) {
        return Ok(None);
    }
    TargetToken::new(&InternalTarget::MultiviewHref {
        href: href.clone(),
        anchor: item.anchor.clone(),
    })
    .map(Some)
}

fn is_multiview_menu_navigation_command_href(href: &str) -> bool {
    href.eq_ignore_ascii_case("index")
}

enum MultiviewMenuPageCursor {
    Root { offset: usize },
    Children { path: Vec<usize>, offset: usize },
}

fn parse_multiview_menu_page_cursor(cursor: Option<&str>) -> Result<MultiviewMenuPageCursor> {
    let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(MultiviewMenuPageCursor::Root { offset: 0 });
    };
    if let Some(rest) = cursor.strip_prefix("children:") {
        let Some((path, offset)) = rest.rsplit_once(':') else {
            return Err(crate::error::Error::Driver(format!(
                "invalid MultiView child cursor: {cursor}"
            )));
        };
        let path = parse_multiview_menu_node_path(path)?;
        let offset = offset.parse::<usize>().map_err(|error| {
            crate::error::Error::Driver(format!(
                "invalid MultiView child cursor offset {offset}: {error}"
            ))
        })?;
        return Ok(MultiviewMenuPageCursor::Children { path, offset });
    }
    let offset = cursor.parse::<usize>().map_err(|error| {
        crate::error::Error::Driver(format!("invalid MultiView root cursor {cursor}: {error}"))
    })?;
    Ok(MultiviewMenuPageCursor::Root { offset })
}

fn parse_multiview_menu_node_path(value: &str) -> Result<Vec<usize>> {
    if value.trim().is_empty() {
        return Err(crate::error::Error::Driver(
            "empty MultiView child cursor path".to_owned(),
        ));
    }
    value
        .split('.')
        .map(|part| {
            part.parse::<usize>().map_err(|error| {
                crate::error::Error::Driver(format!(
                    "invalid MultiView child cursor path segment {part}: {error}"
                ))
            })
        })
        .collect()
}

fn multiview_menu_path_node_id(path: &[usize]) -> String {
    path.iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn multiview_menu_children_at_path<'a>(
    items: &'a [MultiviewMenuItem],
    path: &[usize],
) -> Option<&'a [MultiviewMenuItem]> {
    let mut current = items;
    for index in path {
        current = current.get(*index)?.children.as_slice();
    }
    Some(current)
}

fn page_items_len_for_cursor(items: &[MultiviewMenuItem], cursor: Option<&str>) -> Result<usize> {
    match parse_multiview_menu_page_cursor(cursor)? {
        MultiviewMenuPageCursor::Root { .. } => Ok(items.len()),
        MultiviewMenuPageCursor::Children { path, .. } => {
            Ok(multiview_menu_children_at_path(items, &path)
                .map(<[MultiviewMenuItem]>::len)
                .unwrap_or(0))
        }
    }
}

fn multiview_menu_items_node_count_capped<'a>(
    items: impl IntoIterator<Item = &'a MultiviewMenuItem>,
    cap: usize,
) -> usize {
    let mut count = 0usize;
    for item in items {
        count = count.saturating_add(multiview_menu_item_node_count_capped(
            item,
            cap.saturating_sub(count),
        ));
        if count > cap {
            return count;
        }
    }
    count
}

fn multiview_menu_item_node_count_capped(item: &MultiviewMenuItem, cap: usize) -> usize {
    let mut count = 1usize;
    if count > cap {
        return count;
    }
    for child in &item.children {
        count = count.saturating_add(multiview_menu_item_node_count_capped(
            child,
            cap.saturating_sub(count),
        ));
        if count > cap {
            return count;
        }
    }
    count
}

pub(super) fn navigation_node_mut_at_path<'a>(
    nodes: &'a mut [NavigationNode],
    path: &[usize],
) -> Option<&'a mut NavigationNode> {
    let (&first, rest) = path.split_first()?;
    let mut node = nodes.get_mut(first)?;
    for index in rest {
        node = node.children.get_mut(*index)?;
    }
    Some(node)
}

pub(super) fn collect_multiview_menu_ordered_targets(
    items: &[MultiviewMenuItem],
    out: &mut Vec<TargetToken>,
) -> Result<()> {
    for item in items {
        if let Some(target) = multiview_menu_item_target(item)? {
            out.push(target);
        }
        collect_multiview_menu_ordered_targets(&item.children, out)?;
    }
    Ok(())
}

pub(super) fn collect_panel_cell_ordered_targets(
    cells: &[PanelCell],
    out: &mut Vec<OrderedSequenceTarget>,
) {
    for cell in cells {
        if let Some(target) = &cell.target {
            out.push(OrderedSequenceTarget {
                target: target.clone(),
                title: Some(cell.label_text.clone()),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface(
        surface_id: &str,
        kind: NavigationSurfaceKind,
        status: NavigationStatus,
        href: Option<&str>,
    ) -> HomeSurface {
        HomeSurface {
            href: href.map(str::to_owned),
            surface_id: surface_id.to_owned(),
            kind,
            status,
            title_html: surface_id.to_owned(),
            title_text: surface_id.to_owned(),
            target: None,
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn home_surface_priority_treats_href_only_surfaces_as_targetable() {
        let href_only_panel = surface(
            "panels",
            NavigationSurfaceKind::Panel,
            NavigationStatus::Available,
            Some("lvcore://target/panel"),
        );
        let targetless_menu = surface(
            "menu",
            NavigationSurfaceKind::Menu,
            NavigationStatus::Available,
            None,
        );

        assert!(
            home_surface_reader_priority(&href_only_panel)
                < home_surface_reader_priority(&targetless_menu)
        );
    }
}

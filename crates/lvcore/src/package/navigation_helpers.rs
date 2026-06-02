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
    let status_group = match (surface.status, surface.target.is_some()) {
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
    let target = item
        .href
        .as_ref()
        .map(|href| {
            TargetToken::new(&InternalTarget::MultiviewHref {
                href: href.clone(),
                anchor: item.anchor.clone(),
            })
        })
        .transpose()?;
    let children = item
        .children
        .iter()
        .enumerate()
        .map(|(index, child)| multiview_menu_item_to_node(child, &format!("{node_id}.{index}")))
        .collect::<Result<Vec<_>>>()?;
    Ok(NavigationNode {
        href: None,
        node_id: node_id.to_owned(),
        label_html: escape_plain_label_html(&item.label),
        label_text: item.label.clone(),
        target,
        diagnostics: Vec::new(),
        children,
    })
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

pub(super) fn collect_navigation_node_targets(
    nodes: &[NavigationNode],
    out: &mut Vec<TargetToken>,
) {
    for node in nodes {
        if let Some(target) = &node.target {
            out.push(target.clone());
        }
        collect_navigation_node_targets(&node.children, out);
    }
}

pub(super) fn collect_navigation_node_ordered_targets(
    nodes: &[NavigationNode],
    out: &mut Vec<OrderedSequenceTarget>,
) {
    for node in nodes {
        if let Some(target) = &node.target {
            out.push(OrderedSequenceTarget {
                target: target.clone(),
                title: Some(node.label_text.clone()),
            });
        }
        collect_navigation_node_ordered_targets(&node.children, out);
    }
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

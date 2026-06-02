use super::*;

#[derive(Debug, Clone)]
pub(super) struct SsedHanreiPage {
    pub(super) item_id: String,
    pub(super) label: String,
    pub(super) resource: InternalResource,
    pub(super) anchor: Option<String>,
    pub(super) diagnostics: Vec<Diagnostic>,
}

pub(super) fn read_path_inside_loose_root(
    package_root: &Path,
    root_name: &str,
    resolved: &Path,
) -> Result<Vec<u8>> {
    let Some(root) = find_loose_media_root(package_root, root_name)? else {
        return Err(Error::Driver(format!(
            "loose SSED resource root not found: {root_name}"
        )));
    };
    if !path_stays_inside_root(&root, resolved)? {
        return Err(Error::Driver(format!(
            "loose SSED resource path is outside its root: {root_name}"
        )));
    }
    Ok(fs::read(resolved)?)
}

pub(super) fn read_path_inside_resolved_parent(resolved: &Path, label: &str) -> Result<Vec<u8>> {
    let Some(root) = resolved.parent() else {
        return Err(Error::Driver(format!(
            "{label} resource path has no parent directory"
        )));
    };
    if !path_stays_inside_root(root, resolved)? {
        return Err(Error::Driver(format!(
            "{label} resource path is outside its resolved root"
        )));
    }
    Ok(fs::read(resolved)?)
}

pub(super) fn ssed_menu_records_to_nodes_from(
    package: &ReaderBookPackage,
    records: &[SsedMenuRecord],
    base_index: usize,
    diagnostics: &mut Vec<Diagnostic>,
    gaiji_policy: &GaijiPolicy,
) -> Result<Vec<NavigationNode>> {
    let mut roots = Vec::new();
    let mut path = Vec::<usize>::new();

    for (index, record) in records.iter().enumerate() {
        let global_index = base_index + index;
        let nodes =
            ssed_menu_record_nodes(package, record, global_index, diagnostics, gaiji_policy)?;
        let depth = record.depth.max(1);
        for node in nodes {
            attach_ssed_menu_node(
                &mut roots,
                &mut path,
                node,
                depth,
                global_index,
                diagnostics,
            );
        }
    }

    Ok(roots)
}

fn ssed_menu_record_nodes(
    package: &ReaderBookPackage,
    record: &SsedMenuRecord,
    global_index: usize,
    diagnostics: &mut Vec<Diagnostic>,
    gaiji_policy: &GaijiPolicy,
) -> Result<Vec<NavigationNode>> {
    let target_links = record
        .links
        .iter()
        .enumerate()
        .filter(|(_, link)| {
            link.destination
                .as_ref()
                .is_some_and(|destination| !destination.is_null())
                && !link.label.trim().is_empty()
        })
        .collect::<Vec<_>>();
    if target_links.len() > 1 {
        let mut nodes = Vec::new();
        for (link_index, link) in target_links {
            let Some(destination) = link.destination.as_ref() else {
                continue;
            };
            let Some(target) = ssed_menu_destination_target(package, destination, diagnostics)?
            else {
                continue;
            };
            let label = ssed_menu_link_display_label(&link.label);
            if label.is_empty() {
                continue;
            }
            let rich_label = package.ssed_rich_label_with_policy(&label, gaiji_policy);
            nodes.push(NavigationNode {
                href: None,
                node_id: format!("ssed-menu:{global_index}:link:{link_index}"),
                label_html: rich_label.html,
                label_text: rich_label.text,
                target: Some(target),
                diagnostics: rich_label.diagnostics,
                children: Vec::new(),
            });
        }
        return Ok(nodes);
    }

    let label = record.label();
    if label.is_empty() {
        return Ok(Vec::new());
    }
    let target = ssed_menu_record_target(package, record, diagnostics)?;
    let rich_label = package.ssed_rich_label_with_policy(label, gaiji_policy);
    Ok(vec![NavigationNode {
        href: None,
        node_id: format!("ssed-menu:{global_index}"),
        label_html: rich_label.html,
        label_text: rich_label.text,
        target,
        diagnostics: rich_label.diagnostics,
        children: Vec::new(),
    }])
}

fn attach_ssed_menu_node(
    roots: &mut Vec<NavigationNode>,
    path: &mut Vec<usize>,
    node: NavigationNode,
    depth: usize,
    global_index: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    while path.len() >= depth {
        path.pop();
    }
    if path.is_empty() {
        roots.push(node);
        path.push(roots.len() - 1);
    } else if let Some(parent) = navigation_node_mut_at_path(roots, path) {
        parent.children.push(node);
        path.push(parent.children.len() - 1);
    } else {
        diagnostics.push(Diagnostic::warning(
            "ssed_navigation_tree_depth_invalid",
            format!("could not attach MENU/TOC row {global_index} at depth {depth}"),
        ));
        roots.push(node);
        path.clear();
        path.push(roots.len() - 1);
    }
}

fn ssed_menu_link_display_label(label: &str) -> String {
    label
        .split(['■', '§'])
        .next()
        .unwrap_or(label)
        .trim()
        .to_owned()
}

pub(super) fn ssed_multi_selector_records_to_nodes(
    package: &ReaderBookPackage,
    descriptor_name: &str,
    record_index: u16,
    records: &[SsedMenuRecord],
    gaiji_policy: &GaijiPolicy,
) -> Result<Vec<NavigationNode>> {
    let mut roots = Vec::new();
    let mut path = Vec::<usize>::new();

    for (index, record) in records.iter().enumerate() {
        let label = record.label();
        if label.is_empty() {
            continue;
        }
        let rich_label = package.ssed_rich_label_with_policy(label, gaiji_policy);
        let node = NavigationNode {
            href: None,
            node_id: format!("multi:{descriptor_name}:record:{record_index}:selector:{index}"),
            label_html: rich_label.html,
            label_text: rich_label.text.clone(),
            target: Some(TargetToken::new(&InternalTarget::TitleIndexItem {
                surface_id: ssed_multi_record_surface_id(
                    descriptor_name,
                    record_index,
                    Some(label),
                ),
                item_id: "root".to_owned(),
            })?),
            diagnostics: rich_label.diagnostics,
            children: Vec::new(),
        };
        let depth = record.depth.max(1);
        while path.len() >= depth {
            path.pop();
        }
        if path.is_empty() {
            roots.push(node);
            path.push(roots.len() - 1);
        } else if let Some(parent) = navigation_node_mut_at_path(&mut roots, &path) {
            parent.children.push(node);
            path.push(parent.children.len() - 1);
        } else {
            roots.push(node);
            path.clear();
            path.push(roots.len() - 1);
        }
    }

    Ok(roots)
}

pub(super) fn ssed_encyclopedia_rows_to_nodes(
    package: &ReaderBookPackage,
    rows: &[SsedEncyclopediaRow],
    diagnostics: &mut Vec<Diagnostic>,
    gaiji_policy: &GaijiPolicy,
) -> Result<Vec<NavigationNode>> {
    let mut roots = Vec::new();
    let mut path = Vec::<usize>::new();

    for (index, row) in rows.iter().enumerate() {
        let rich_label = package.ssed_rich_label_with_policy(&row.label, gaiji_policy);
        let node = NavigationNode {
            href: None,
            node_id: format!("encyclopedia:{}:{index}", row.index),
            label_html: rich_label.html,
            label_text: rich_label.text,
            target: ssed_encyclopedia_row_target(package, row, diagnostics)?,
            diagnostics: rich_label.diagnostics,
            children: Vec::new(),
        };
        let depth = row.depth as usize;
        while path.len() > depth {
            path.pop();
        }
        if path.is_empty() {
            roots.push(node);
            path.push(roots.len() - 1);
        } else if let Some(parent) = navigation_node_mut_at_path(&mut roots, &path) {
            parent.children.push(node);
            path.push(parent.children.len() - 1);
        } else {
            diagnostics.push(Diagnostic::warning(
                "ssed_encyclopedia_tree_depth_invalid",
                format!("could not attach encyclop.idx row {index} at depth {depth}"),
            ));
            roots.push(node);
            path.clear();
            path.push(roots.len() - 1);
        }
    }

    Ok(roots)
}

fn ssed_encyclopedia_row_target(
    package: &ReaderBookPackage,
    row: &SsedEncyclopediaRow,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<TargetToken>> {
    if !row.has_target() {
        return Ok(None);
    }
    let Some(catalog) = &package.ssed_catalog else {
        diagnostics.push(Diagnostic::warning(
            "ssed_encyclopedia_catalog_missing",
            format!(
                "encyclop.idx row {} points to {:08x}:{:04x}, but no SSED catalog is available",
                row.index, row.block, row.offset
            ),
        ));
        return Ok(None);
    };
    let Some(component) = catalog.component_for_address(row.block) else {
        diagnostics.push(Diagnostic::warning(
            "ssed_encyclopedia_target_unresolved",
            format!(
                "encyclop.idx row {} points outside declared components: {:08x}:{:04x}",
                row.index, row.block, row.offset
            ),
        ));
        return Ok(None);
    };
    Ok(Some(TargetToken::new(&InternalTarget::SsedAddress {
        component: component.filename.clone(),
        block: row.block,
        offset: row.offset,
    })?))
}

pub(super) fn ssed_aux_index_rows_to_nodes(
    package: &ReaderBookPackage,
    rows: &[SsedAuxIndexRow],
    diagnostics: &mut Vec<Diagnostic>,
    gaiji_policy: &GaijiPolicy,
) -> Result<Vec<NavigationNode>> {
    let mut roots = Vec::new();
    let mut path = Vec::<usize>::new();

    for (index, row) in rows.iter().enumerate() {
        let rich_label = package.ssed_rich_label_with_policy(&row.label, gaiji_policy);
        let next_target_row = nearest_higher_aux_target_row(rows, row);
        let mut node_diagnostics = rich_label.diagnostics;
        let target =
            ssed_aux_index_row_target(package, row, next_target_row, &mut node_diagnostics)?;
        let node = NavigationNode {
            href: None,
            node_id: format!("aux-index:{}:{index}", row.line_number),
            label_html: rich_label.html,
            label_text: rich_label.text,
            target,
            diagnostics: node_diagnostics,
            children: Vec::new(),
        };
        let depth = row.depth.max(1) as usize;
        while path.len() >= depth {
            path.pop();
        }
        if path.is_empty() {
            roots.push(node);
            path.push(roots.len() - 1);
        } else if let Some(parent) = navigation_node_mut_at_path(&mut roots, &path) {
            parent.children.push(node);
            path.push(parent.children.len() - 1);
        } else {
            diagnostics.push(Diagnostic::warning(
                "ssed_auxiliary_index_tree_depth_invalid",
                format!("could not attach auxiliary index row {index} at depth {depth}"),
            ));
            roots.push(node);
            path.clear();
            path.push(roots.len() - 1);
        }
    }

    Ok(roots)
}

pub(super) fn ssed_aux_index_rows_to_flat_nodes(
    package: &ReaderBookPackage,
    rows: &[SsedAuxIndexRow],
    _diagnostics: &mut Vec<Diagnostic>,
    gaiji_policy: &GaijiPolicy,
) -> Result<Vec<NavigationNode>> {
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let rich_label = package.ssed_rich_label_with_policy(&row.label, gaiji_policy);
            let next_target_row = nearest_higher_aux_target_row(rows, row);
            let mut node_diagnostics = rich_label.diagnostics;
            let target =
                ssed_aux_index_row_target(package, row, next_target_row, &mut node_diagnostics)?;
            Ok(NavigationNode {
                href: None,
                node_id: format!("aux-index:{}:{index}", row.line_number),
                label_html: rich_label.html,
                label_text: rich_label.text,
                target,
                diagnostics: node_diagnostics,
                children: Vec::new(),
            })
        })
        .collect()
}

fn ssed_aux_index_row_target(
    package: &ReaderBookPackage,
    row: &SsedAuxIndexRow,
    next_target_row: Option<&SsedAuxIndexRow>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<TargetToken>> {
    if !row.has_target() {
        return Ok(None);
    }
    if let Some(selector) = row.virtual_selector() {
        if !package.storage.exists(Path::new("Panels.xml"))? {
            diagnostics.push(
                Diagnostic::info(
                    "ssed_auxiliary_index_virtual_selector_without_panels",
                    format!(
                        "auxiliary index row {} points to virtual selector {selector}, but no Panels.xml is present",
                        row.line_number
                    ),
                )
                .with_context("panel_id", &selector),
            );
            return Ok(None);
        }
        diagnostics.push(
            Diagnostic::info(
                "ssed_auxiliary_index_virtual_selector",
                format!(
                    "auxiliary index row {} points to virtual selector {selector}; routing through panel {selector}",
                    row.line_number
                ),
            )
            .with_context("panel_id", &selector),
        );
        return Ok(Some(TargetToken::new(&InternalTarget::PanelCell {
            panel_id: selector,
            row: 0,
            column: 0,
        })?));
    }
    let pointer = SsedIndexPointer {
        block: row.block,
        offset: row.offset,
    };
    let end = next_target_row.map(|next| SsedIndexPointer {
        block: next.block,
        offset: next.offset,
    });
    match package.ssed_target_for_index_pointer_with_bound(pointer, end)? {
        Ok(target) => Ok(Some(target)),
        Err(diagnostic) => {
            diagnostics.push(diagnostic);
            Ok(None)
        }
    }
}

fn nearest_higher_aux_target_row<'a>(
    rows: &'a [SsedAuxIndexRow],
    row: &SsedAuxIndexRow,
) -> Option<&'a SsedAuxIndexRow> {
    rows.iter()
        .filter(|candidate| candidate.has_target() && candidate.virtual_selector().is_none())
        .filter(|candidate| (candidate.block, candidate.offset) > (row.block, row.offset))
        .min_by_key(|candidate| (candidate.block, candidate.offset))
}

fn ssed_menu_record_target(
    package: &ReaderBookPackage,
    record: &SsedMenuRecord,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<TargetToken>> {
    let Some(destination) = record
        .links
        .iter()
        .filter_map(|link| link.destination.as_ref())
        .find(|destination| !destination.is_null())
    else {
        return Ok(None);
    };
    ssed_menu_destination_target(package, destination, diagnostics)
}

fn ssed_menu_destination_target(
    package: &ReaderBookPackage,
    destination: &SsedMenuDestination,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<TargetToken>> {
    let Some(catalog) = &package.ssed_catalog else {
        diagnostics.push(Diagnostic::error(
            "ssed_catalog_missing",
            "SSED menu destination cannot be resolved without a catalog",
        ));
        return Ok(None);
    };
    let Some(component) = catalog.component_for_address(destination.block) else {
        diagnostics.push(Diagnostic::warning(
            "ssed_navigation_target_unresolved",
            format!(
                "MENU/TOC target block {} offset {} is outside declared components",
                destination.block, destination.offset
            ),
        ));
        return Ok(None);
    };
    if component
        .relative_offset(destination.block, destination.offset)
        .is_none()
    {
        diagnostics.push(Diagnostic::warning(
            "ssed_navigation_target_invalid",
            format!(
                "{} does not contain MENU/TOC target block {} offset {}",
                component.filename, destination.block, destination.offset
            ),
        ));
        return Ok(None);
    }
    if component.role != SsedComponentRole::Honmon {
        diagnostics.push(
            Diagnostic::info(
                "ssed_navigation_non_body_target_deferred",
                format!(
                    "MENU/TOC target points to {} ({:?}); non-body navigation routing is deferred",
                    component.filename, component.role
                ),
            )
            .with_context("component", &component.filename),
        );
        return Ok(None);
    }
    Ok(Some(TargetToken::new(&InternalTarget::SsedAddress {
        component: component.filename.clone(),
        block: destination.block,
        offset: destination.offset,
    })?))
}

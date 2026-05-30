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

pub(super) fn ssed_menu_records_to_nodes(
    package: &ReaderBookPackage,
    records: &[SsedMenuRecord],
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<NavigationNode>> {
    let mut roots = Vec::new();
    let mut path = Vec::<usize>::new();

    for (index, record) in records.iter().enumerate() {
        let label = record.label();
        if label.is_empty() {
            continue;
        }
        let target = ssed_menu_record_target(package, record, diagnostics)?;
        let rich_label = package.ssed_rich_label(label);
        let node = NavigationNode {
            node_id: format!("ssed-menu:{index}"),
            label_html: rich_label.html,
            label_text: rich_label.text,
            target,
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
            diagnostics.push(Diagnostic::warning(
                "ssed_navigation_tree_depth_invalid",
                format!("could not attach MENU/TOC row {index} at depth {depth}"),
            ));
            roots.push(node);
            path.clear();
            path.push(roots.len() - 1);
        }
    }

    Ok(roots)
}

pub(super) fn ssed_multi_selector_records_to_nodes(
    package: &ReaderBookPackage,
    descriptor_name: &str,
    record_index: u16,
    records: &[SsedMenuRecord],
) -> Result<Vec<NavigationNode>> {
    let mut roots = Vec::new();
    let mut path = Vec::<usize>::new();

    for (index, record) in records.iter().enumerate() {
        let label = record.label();
        if label.is_empty() {
            continue;
        }
        let rich_label = package.ssed_rich_label(label);
        let node = NavigationNode {
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
) -> Result<Vec<NavigationNode>> {
    let mut roots = Vec::new();
    let mut path = Vec::<usize>::new();

    for (index, row) in rows.iter().enumerate() {
        let rich_label = package.ssed_rich_label(&row.label);
        let node = NavigationNode {
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
) -> Result<Vec<NavigationNode>> {
    let mut roots = Vec::new();
    let mut path = Vec::<usize>::new();

    for (index, row) in rows.iter().enumerate() {
        let rich_label = package.ssed_rich_label(&row.label);
        let node = NavigationNode {
            node_id: format!("aux-index:{}:{index}", row.line_number),
            label_html: rich_label.html,
            label_text: rich_label.text,
            target: ssed_aux_index_row_target(package, row, diagnostics)?,
            diagnostics: rich_label.diagnostics,
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

fn ssed_aux_index_row_target(
    package: &ReaderBookPackage,
    row: &SsedAuxIndexRow,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<TargetToken>> {
    if !row.has_target() {
        return Ok(None);
    }
    if let Some(selector) = row.virtual_selector() {
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
    let Some(catalog) = &package.ssed_catalog else {
        diagnostics.push(Diagnostic::warning(
            "ssed_auxiliary_index_catalog_missing",
            format!(
                "auxiliary index row {} points to {:08x}:{:04x}, but no SSED catalog is available",
                row.line_number, row.block, row.offset
            ),
        ));
        return Ok(None);
    };
    let Some(component) = catalog.component_for_address(row.block) else {
        diagnostics.push(Diagnostic::warning(
            "ssed_auxiliary_index_target_unresolved",
            format!(
                "auxiliary index row {} points outside declared components: {:08x}:{:04x}",
                row.line_number, row.block, row.offset
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

pub(super) struct SsedMultiSurfaceId {
    pub(super) descriptor: String,
    pub(super) record_index: Option<u16>,
    pub(super) filter: Option<String>,
}

pub(super) fn parse_ssed_multi_surface_id(surface_id: &str) -> Option<SsedMultiSurfaceId> {
    let rest = surface_id.strip_prefix("multi:")?;
    let parts = rest.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [descriptor] if !descriptor.is_empty() => Some(SsedMultiSurfaceId {
            descriptor: (*descriptor).to_owned(),
            record_index: None,
            filter: None,
        }),
        [descriptor, "record", record_index] if !descriptor.is_empty() => {
            Some(SsedMultiSurfaceId {
                descriptor: (*descriptor).to_owned(),
                record_index: record_index.parse().ok(),
                filter: None,
            })
            .filter(|parsed| parsed.record_index.is_some())
        }
        [descriptor, "record", record_index, "filter", filter_hex] if !descriptor.is_empty() => {
            let filter = hex::decode(filter_hex)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())?;
            Some(SsedMultiSurfaceId {
                descriptor: (*descriptor).to_owned(),
                record_index: Some(record_index.parse().ok()?),
                filter: Some(filter),
            })
        }
        _ => None,
    }
}

pub(super) fn ssed_multi_root_surface_id(descriptor_name: &str) -> String {
    format!("multi:{descriptor_name}")
}

pub(super) fn ssed_multi_record_surface_id(
    descriptor_name: &str,
    record_index: u16,
    filter: Option<&str>,
) -> String {
    match filter {
        Some(filter) => format!(
            "multi:{descriptor_name}:record:{record_index}:filter:{}",
            hex::encode(filter.as_bytes())
        ),
        None => format!("multi:{descriptor_name}:record:{record_index}"),
    }
}

pub(super) fn ssed_multi_record_menu_ref(
    record: &SsedMultiRecord,
) -> Option<&SsedMultiComponentRef> {
    record
        .refs
        .iter()
        .find(|reference| reference.component_type == 0x01)
}

pub(super) fn ssed_multi_record_index_ref(
    record: &SsedMultiRecord,
) -> Option<&SsedMultiComponentRef> {
    record
        .refs
        .iter()
        .find(|reference| is_supported_index_type(reference.component_type))
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

pub(super) fn ssed_panel_inline_cell_to_navigation_cell(
    package: &ReaderBookPackage,
    cell: &SsedPanelInlineCell,
) -> Result<PanelCell> {
    let target = if !cell.ref_id.is_empty() {
        Some(TargetToken::new(&InternalTarget::PanelCell {
            panel_id: cell.ref_id.clone(),
            row: 0,
            column: 0,
        })?)
    } else {
        None
    };
    let rich_label = package.ssed_rich_label(&cell.label);
    Ok(PanelCell {
        panel_id: cell.panel_id.clone(),
        row: cell.row.unwrap_or(cell.cell_index),
        column: cell.column.unwrap_or(0),
        label_html: rich_label.html,
        label_text: rich_label.text,
        target,
        diagnostics: rich_label.diagnostics,
    })
}

pub(super) fn ssed_panel_bin_record_to_navigation_cell(
    package: &ReaderBookPackage,
    data_ref: &SsedPanelDataRef,
    record: &SsedPanelBinRecord,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<PanelCell> {
    let rich_label = package.ssed_rich_label(&record.text);
    Ok(PanelCell {
        panel_id: data_ref.panel_id.clone(),
        row: record.index,
        column: 0,
        label_html: rich_label.html,
        label_text: rich_label.text,
        target: ssed_panel_record_target(package, record, diagnostics)?,
        diagnostics: rich_label.diagnostics,
    })
}

fn ssed_panel_record_target(
    package: &ReaderBookPackage,
    record: &SsedPanelBinRecord,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<TargetToken>> {
    if record.block == 0 && record.offset == 0 {
        return Ok(None);
    }
    let Some(catalog) = &package.ssed_catalog else {
        diagnostics.push(Diagnostic::error(
            "ssed_catalog_missing",
            "Panel BIN target cannot be resolved without a catalog",
        ));
        return Ok(None);
    };
    let Some(component) = catalog.component_for_address(record.block) else {
        diagnostics.push(Diagnostic::warning(
            "ssed_panel_target_unresolved",
            format!(
                "Panel target block {} offset {} is outside declared components",
                record.block, record.offset
            ),
        ));
        return Ok(None);
    };
    if component
        .relative_offset(record.block, record.offset)
        .is_none()
    {
        diagnostics.push(Diagnostic::warning(
            "ssed_panel_target_invalid",
            format!(
                "{} does not contain Panel target block {} offset {}",
                component.filename, record.block, record.offset
            ),
        ));
        return Ok(None);
    }
    if component.role != SsedComponentRole::Honmon {
        diagnostics.push(
            Diagnostic::info(
                "ssed_panel_non_body_target_deferred",
                format!(
                    "Panel target points to {} ({:?}); non-body panel routing is deferred",
                    component.filename, component.role
                ),
            )
            .with_context("component", &component.filename),
        );
        return Ok(None);
    }
    Ok(Some(TargetToken::new(&InternalTarget::SsedAddress {
        component: component.filename.clone(),
        block: record.block,
        offset: record.offset,
    })?))
}

pub(super) fn hourei_law_node_label(entry: &crate::hourei::HoureiLawEntry) -> String {
    if let Some(name_sub) = &entry.name_sub
        && !name_sub.trim().is_empty()
    {
        return format!("{} {}", entry.name, name_sub);
    }
    if !entry.name.trim().is_empty() {
        return entry.name.clone();
    }
    if let Some(abbr1) = &entry.abbr1
        && !abbr1.trim().is_empty()
    {
        return abbr1.clone();
    }
    entry.hore_id.clone()
}

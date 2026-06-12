use super::*;

#[derive(Debug, Clone)]
pub(super) struct SsedHanreiPage {
    pub(super) item_id: String,
    pub(super) label: String,
    pub(super) resource: InternalResource,
    pub(super) anchor: Option<String>,
    pub(super) diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
pub(super) struct SsedMenuNodePage {
    pub(super) nodes: Vec<NavigationNode>,
    pub(super) next_cursor: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SsedMenuNodeCursor {
    pub(super) record_offset: usize,
    pub(super) link_offset: usize,
}

pub(super) struct SsedMenuNodePageRequest<'a> {
    pub(super) package: &'a ReaderBookPackage,
    pub(super) records: &'a [SsedMenuRecord],
    pub(super) base_index: usize,
    pub(super) initial_link_offset: usize,
    pub(super) limit: usize,
    pub(super) parsed_next_cursor: Option<String>,
    pub(super) gaiji_policy: &'a GaijiPolicy,
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

pub(super) fn decode_ssed_menu_node_cursor(cursor: Option<&str>) -> SsedMenuNodeCursor {
    let Some(cursor) = cursor.map(str::trim).filter(|cursor| !cursor.is_empty()) else {
        return SsedMenuNodeCursor {
            record_offset: 0,
            link_offset: 0,
        };
    };
    if let Some(rest) = cursor.strip_prefix("link:") {
        let mut parts = rest.split(':');
        if let (Some(record), Some(link), None) = (parts.next(), parts.next(), parts.next()) {
            return SsedMenuNodeCursor {
                record_offset: record.parse::<usize>().unwrap_or(0),
                link_offset: link.parse::<usize>().unwrap_or(0),
            };
        }
    }
    SsedMenuNodeCursor {
        record_offset: cursor.parse::<usize>().unwrap_or(0),
        link_offset: 0,
    }
}

pub(super) fn ssed_menu_records_to_nodes_page_from(
    request: SsedMenuNodePageRequest<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<SsedMenuNodePage> {
    let mut roots = Vec::new();
    let mut path = Vec::<usize>::new();
    let mut emitted = 0usize;
    let mut next_cursor = None;
    let destination_index = SsedMenuDestinationIndex::new(request.records);

    for (index, record) in request.records.iter().enumerate() {
        if emitted >= request.limit {
            next_cursor = Some((request.base_index + index).to_string());
            break;
        }
        let global_index = request.base_index + index;
        let link_offset = if index == 0 {
            request.initial_link_offset
        } else {
            0
        };
        let page = ssed_menu_record_nodes_page(
            SsedMenuRecordNodeRequest {
                package: request.package,
                global_index,
                link_offset,
                limit: request.limit.saturating_sub(emitted),
                gaiji_policy: request.gaiji_policy,
                destination_index: &destination_index,
            },
            record,
            diagnostics,
        )?;
        if let Some(next_link_offset) = page.next_link_offset {
            next_cursor = Some(format!("link:{global_index}:{next_link_offset}"));
        }
        let depth = record.depth.max(1);
        for node in page.nodes {
            attach_ssed_menu_node(
                &mut roots,
                &mut path,
                node,
                depth,
                global_index,
                diagnostics,
            );
            emitted = emitted.saturating_add(1);
        }
        if next_cursor.is_some() {
            break;
        }
    }

    Ok(SsedMenuNodePage {
        nodes: roots,
        next_cursor: next_cursor.or(request.parsed_next_cursor),
    })
}

#[derive(Debug)]
struct SsedMenuRecordNodePage {
    nodes: Vec<NavigationNode>,
    next_link_offset: Option<usize>,
}

struct SsedMenuRecordNodeRequest<'a> {
    package: &'a ReaderBookPackage,
    global_index: usize,
    link_offset: usize,
    limit: usize,
    gaiji_policy: &'a GaijiPolicy,
    destination_index: &'a SsedMenuDestinationIndex<'a>,
}

fn ssed_menu_record_nodes_page(
    request: SsedMenuRecordNodeRequest<'_>,
    record: &SsedMenuRecord,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<SsedMenuRecordNodePage> {
    if request.limit == 0 {
        return Ok(SsedMenuRecordNodePage {
            nodes: Vec::new(),
            next_link_offset: None,
        });
    }
    let target_link_count = record
        .links
        .iter()
        .filter(|link| ssed_menu_link_is_visible_target(link))
        .count();
    if target_link_count > 1 {
        let mut nodes = Vec::new();
        let mut consumed_links = 0usize;
        let mut next_link_offset = None;
        for (position, (link_index, link)) in record
            .links
            .iter()
            .enumerate()
            .filter(|(_, link)| ssed_menu_link_is_visible_target(link))
            .enumerate()
        {
            if position < request.link_offset {
                continue;
            }
            if nodes.len() >= request.limit {
                next_link_offset = Some(request.link_offset.saturating_add(consumed_links));
                break;
            }
            consumed_links = consumed_links.saturating_add(1);
            let Some(destination) = link.destination.as_ref() else {
                continue;
            };
            let next_destination = request.destination_index.next_after(destination);
            let Some(target) = ssed_menu_destination_target(
                request.package,
                destination,
                next_destination,
                diagnostics,
            )?
            else {
                continue;
            };
            let label = ssed_menu_link_display_label(&link.label);
            if label.is_empty() {
                continue;
            }
            let rich_label = request
                .package
                .ssed_rich_label_with_policy(&label, request.gaiji_policy);
            nodes.push(NavigationNode {
                href: None,
                child_cursor: None,
                node_id: format!("ssed-menu:{}:link:{link_index}", request.global_index),
                label_html: rich_label.html,
                label_text: rich_label.text,
                target: Some(target),
                diagnostics: rich_label.diagnostics,
                children: Vec::new(),
            });
        }
        return Ok(SsedMenuRecordNodePage {
            nodes,
            next_link_offset,
        });
    }

    let label = record.label();
    if label.is_empty() {
        return Ok(SsedMenuRecordNodePage {
            nodes: Vec::new(),
            next_link_offset: None,
        });
    }
    let target = ssed_menu_record_target_with_destination_index(
        request.package,
        record,
        request.destination_index,
        diagnostics,
    )?;
    let rich_label = request
        .package
        .ssed_rich_label_with_policy(label, request.gaiji_policy);
    Ok(SsedMenuRecordNodePage {
        nodes: vec![NavigationNode {
            href: None,
            child_cursor: None,
            node_id: format!("ssed-menu:{}", request.global_index),
            label_html: rich_label.html,
            label_text: rich_label.text,
            target,
            diagnostics: rich_label.diagnostics,
            children: Vec::new(),
        }],
        next_link_offset: None,
    })
}

fn ssed_menu_link_is_visible_target(link: &SsedMenuLink) -> bool {
    link.destination
        .as_ref()
        .is_some_and(|destination| !destination.is_null())
        && !link.label.trim().is_empty()
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

pub(in crate::package::drivers) fn ssed_menu_link_display_label(label: &str) -> String {
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
            child_cursor: None,
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
            child_cursor: None,
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
    visible_rows: &[SsedAuxIndexRow],
    all_rows: &[SsedAuxIndexRow],
    diagnostics: &mut Vec<Diagnostic>,
    gaiji_policy: &GaijiPolicy,
) -> Result<Vec<NavigationNode>> {
    let mut roots = Vec::new();
    let mut path = Vec::<usize>::new();

    for (index, row) in visible_rows.iter().enumerate() {
        let rich_label = package.ssed_rich_label_with_policy(&row.label, gaiji_policy);
        let next_target_row = nearest_higher_aux_target_row(all_rows, row);
        let mut node_diagnostics = rich_label.diagnostics;
        let target =
            ssed_aux_index_row_target(package, row, next_target_row, &mut node_diagnostics)?;
        let node = NavigationNode {
            href: None,
            child_cursor: None,
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
    visible_rows: &[SsedAuxIndexRow],
    all_rows: &[SsedAuxIndexRow],
    _diagnostics: &mut Vec<Diagnostic>,
    gaiji_policy: &GaijiPolicy,
) -> Result<Vec<NavigationNode>> {
    visible_rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let rich_label = package.ssed_rich_label_with_policy(&row.label, gaiji_policy);
            let next_target_row = nearest_higher_aux_target_row(all_rows, row);
            let mut node_diagnostics = rich_label.diagnostics;
            let target =
                ssed_aux_index_row_target(package, row, next_target_row, &mut node_diagnostics)?;
            Ok(NavigationNode {
                href: None,
                child_cursor: None,
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

pub(in crate::package::drivers) fn ssed_aux_index_row_target(
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
    if let Some(catalog) = &package.ssed_catalog
        && let Some(component) = catalog.component_for_address(row.block)
        && component.relative_offset(row.block, row.offset).is_some()
        && component.role != SsedComponentRole::Honmon
    {
        return ssed_component_address_navigation_target(
            SsedComponentNavigationTargetRequest {
                package,
                component,
                block: row.block,
                offset: row.offset,
                next: next_target_row.map(|next| (next.block, next.offset)),
                diagnostic_code: "ssed_auxiliary_index_non_body_target_deferred",
                source_label: "Auxiliary index",
            },
            diagnostics,
        );
    }
    if let Some(catalog) = &package.ssed_catalog
        && let Some(component) = catalog.component_for_address(row.block)
        && component.role == SsedComponentRole::Honmon
        && let Some(diagnostic) = ssed_aux_honmon_target_non_renderable_diagnostic(
            package,
            component,
            row,
            next_target_row,
        )?
    {
        diagnostics.push(diagnostic);
        return Ok(None);
    }
    let pointer = if let Some(catalog) = &package.ssed_catalog {
        if let Some(component) = catalog.component_for_address(row.block) {
            ssed_aux_honmon_render_pointer(package, component, row, next_target_row)?
                .unwrap_or(pointer)
        } else {
            pointer
        }
    } else {
        pointer
    };
    match package.ssed_target_for_index_pointer_with_bound(pointer, end)? {
        Ok(target) => Ok(Some(target)),
        Err(diagnostic) => {
            diagnostics.push(diagnostic);
            Ok(None)
        }
    }
}

const SSED_AUX_HONMON_ENTRY_MARKER_FORWARD_SCAN_LIMIT: usize = 4096;

fn ssed_aux_honmon_render_pointer(
    package: &ReaderBookPackage,
    component: &SsedComponent,
    row: &SsedAuxIndexRow,
    next_target_row: Option<&SsedAuxIndexRow>,
) -> Result<Option<SsedIndexPointer>> {
    if component.role != SsedComponentRole::Honmon {
        return Ok(None);
    }
    let Some(component_offset) = component.relative_offset(row.block, row.offset) else {
        return Ok(None);
    };
    let Some(path) = package.resolve_readable_ssed_component_path(component)? else {
        return Ok(None);
    };
    let mut reader = SsedDataFile::open(path)?;
    let Ok(start) = usize::try_from(component_offset) else {
        return Ok(None);
    };
    let bound = next_target_row
        .and_then(|next| {
            package
                .ssed_catalog
                .as_ref()
                .and_then(|catalog| catalog.component_for_address(next.block))
                .filter(|next_component| next_component.filename == component.filename)
                .and_then(|next_component| next_component.relative_offset(next.block, next.offset))
        })
        .and_then(|offset| usize::try_from(offset).ok());
    let match_texts = ssed_aux_label_target_match_texts(&row.label);
    let Some(marker_offset) =
        ssed_aux_matching_entry_marker_offset(package, &mut reader, start, bound, &match_texts)?
    else {
        return Ok(None);
    };
    ssed_component_offset_to_pointer(component, marker_offset).map(Some)
}

fn ssed_aux_matching_entry_marker_offset(
    package: &ReaderBookPackage,
    reader: &mut SsedDataFile,
    start: usize,
    bound: Option<usize>,
    match_texts: &[String],
) -> Result<Option<usize>> {
    if match_texts.is_empty() {
        return Ok(None);
    }
    let expanded_size = reader.header().expanded_size();
    if start >= expanded_size {
        return Ok(None);
    }
    let scan_end = bound
        .unwrap_or_else(|| start.saturating_add(SSED_AUX_HONMON_ENTRY_MARKER_FORWARD_SCAN_LIMIT))
        .min(start.saturating_add(SSED_AUX_HONMON_ENTRY_MARKER_FORWARD_SCAN_LIMIT))
        .min(expanded_size);
    if scan_end <= start.saturating_add(1) {
        return Ok(None);
    }
    let data = reader.read_range(start, scan_end - start)?;
    let Some(last_marker_start) = data.len().checked_sub(SSED_ENTRY_MARKER.len()) else {
        return Ok(None);
    };
    for marker_pos in 1..=last_marker_start {
        if data.get(marker_pos..marker_pos + SSED_ENTRY_MARKER.len())
            != Some(SSED_ENTRY_MARKER.as_slice())
        {
            continue;
        }
        let entry_start = if marker_pos >= 2 && data[marker_pos - 2..marker_pos] == [0x1f, 0x02] {
            start + marker_pos - 2
        } else {
            start + marker_pos
        };
        if entry_start < start || entry_start >= scan_end {
            continue;
        }
        let title_len = 512.min(expanded_size.saturating_sub(entry_start));
        let title_data = reader.read_range(entry_start, title_len)?;
        let title = decode_title_text_with_gaiji_filter(&title_data, |identity| {
            package.gaiji_unicode_map.contains_key(identity)
        });
        let title_key = normalize_search_match_text(&title);
        if match_texts.iter().any(|candidate| candidate == &title_key) {
            return Ok(Some(entry_start));
        }
    }
    Ok(None)
}

fn ssed_aux_label_target_match_texts(label: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    push_unique_ssed_aux_label_target_match_text(&mut candidates, label);
    let stripped = label.trim_start_matches(is_ssed_aux_label_decoration);
    if stripped != label {
        push_unique_ssed_aux_label_target_match_text(&mut candidates, stripped);
    }
    candidates
}

fn push_unique_ssed_aux_label_target_match_text(candidates: &mut Vec<String>, label: &str) {
    let normalized = normalize_search_match_text(label.trim());
    if !normalized.is_empty() && !candidates.contains(&normalized) {
        candidates.push(normalized);
    }
}

fn is_ssed_aux_label_decoration(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '■' | '□'
                | '▲'
                | '△'
                | '▼'
                | '▽'
                | '◆'
                | '◇'
                | '●'
                | '○'
                | '◎'
                | '§'
                | '・'
                | '-'
                | '－'
                | '–'
                | '—'
                | '▶'
                | '▷'
        )
}

fn ssed_component_offset_to_pointer(
    component: &SsedComponent,
    component_offset: usize,
) -> Result<SsedIndexPointer> {
    let block_delta = u32::try_from(component_offset / BLOCK_SIZE as usize)
        .map_err(|_| Error::Driver("SSED component offset block delta is too large".to_owned()))?;
    let offset = u32::try_from(component_offset % BLOCK_SIZE as usize)
        .map_err(|_| Error::Driver("SSED component offset does not fit in u32".to_owned()))?;
    Ok(SsedIndexPointer {
        block: component.start_block.saturating_add(block_delta),
        offset,
    })
}

fn ssed_aux_honmon_target_non_renderable_diagnostic(
    package: &ReaderBookPackage,
    component: &SsedComponent,
    row: &SsedAuxIndexRow,
    next_target_row: Option<&SsedAuxIndexRow>,
) -> Result<Option<Diagnostic>> {
    const MIN_AUX_HONMON_RANGE_BYTES: u64 = 8;

    let Some(component_offset) = component.relative_offset(row.block, row.offset) else {
        return Ok(None);
    };
    let Some(path) = package.resolve_readable_ssed_component_path(component)? else {
        return Ok(None);
    };
    let mut reader = SsedDataFile::open(path)?;
    let Ok(component_offset_usize) = usize::try_from(component_offset) else {
        return Ok(Some(
            Diagnostic::info(
                "ssed_auxiliary_index_body_target_non_renderable",
                format!(
                    "auxiliary index row {} points to a HONMON offset too large to validate",
                    row.line_number
                ),
            )
            .with_context("component", &component.filename),
        ));
    };

    if let Some(next) = next_target_row
        && let Some(next_component) = package
            .ssed_catalog
            .as_ref()
            .and_then(|catalog| catalog.component_for_address(next.block))
        && next_component
            .filename
            .eq_ignore_ascii_case(&component.filename)
        && let Some(next_offset) = next_component.relative_offset(next.block, next.offset)
    {
        let stream_start =
            ssed_aux_honmon_stream_start_offset(&mut reader, component_offset_usize)?;
        if next_offset > u64::try_from(stream_start).unwrap_or(u64::MAX) {
            let range_len = next_offset - u64::try_from(stream_start).unwrap_or(0);
            if range_len < MIN_AUX_HONMON_RANGE_BYTES {
                return Ok(Some(
                    Diagnostic::info(
                        "ssed_auxiliary_index_body_target_non_renderable",
                        format!(
                            "auxiliary index row {} points into a {range_len} byte HONMON range, which is not a renderable entry body",
                            row.line_number
                        ),
                    )
                    .with_context("component", &component.filename)
                    .with_context("block", row.block.to_string())
                    .with_context("offset", row.offset.to_string()),
                ));
            }
        }
    }

    if ssed_aux_honmon_offset_is_inside_entry_marker(&mut reader, component_offset_usize)? {
        return Ok(Some(
            Diagnostic::info(
                "ssed_auxiliary_index_body_target_non_renderable",
                format!(
                    "auxiliary index row {} points inside an SSED entry marker/control header",
                    row.line_number
                ),
            )
            .with_context("component", &component.filename)
            .with_context("block", row.block.to_string())
            .with_context("offset", row.offset.to_string()),
        ));
    }

    Ok(None)
}

fn ssed_aux_honmon_stream_start_offset(
    reader: &mut SsedDataFile,
    component_offset: usize,
) -> Result<usize> {
    if component_offset >= 2 {
        let prefix_offset = component_offset - 2;
        let data = reader.read_range(prefix_offset, SSED_ENTRY_MARKER.len() + 2)?;
        if data.starts_with(&[0x1f, 0x02])
            && data
                .get(2..2 + SSED_ENTRY_MARKER.len())
                .is_some_and(|marker| marker == SSED_ENTRY_MARKER)
        {
            return Ok(prefix_offset);
        }
    }
    Ok(component_offset)
}

fn ssed_aux_honmon_offset_is_inside_entry_marker(
    reader: &mut SsedDataFile,
    component_offset: usize,
) -> Result<bool> {
    let scan_start = component_offset.saturating_sub(SSED_ENTRY_MARKER.len() + 2);
    let data = reader.read_range(scan_start, SSED_ENTRY_MARKER.len() * 3)?;
    for relative_start in 0..data.len().min(SSED_ENTRY_MARKER.len() + 2) {
        let absolute_start = scan_start + relative_start;
        let Some(marker_len) = ssed_entry_marker_len_in_slice(&data[relative_start..]) else {
            continue;
        };
        if component_offset > absolute_start
            && component_offset < absolute_start.saturating_add(marker_len)
            && !(marker_len == SSED_ENTRY_MARKER.len() + 2
                && component_offset == absolute_start + 2)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn ssed_entry_marker_len_in_slice(data: &[u8]) -> Option<usize> {
    if data.starts_with(&[0x1f, 0x02])
        && data
            .get(2..2 + SSED_ENTRY_MARKER.len())
            .is_some_and(|marker| marker == SSED_ENTRY_MARKER)
    {
        return Some(SSED_ENTRY_MARKER.len() + 2);
    }
    data.starts_with(&SSED_ENTRY_MARKER)
        .then_some(SSED_ENTRY_MARKER.len())
}

pub(in crate::package::drivers) fn nearest_higher_aux_target_row<'a>(
    rows: &'a [SsedAuxIndexRow],
    row: &SsedAuxIndexRow,
) -> Option<&'a SsedAuxIndexRow> {
    rows.iter()
        .filter(|candidate| candidate.has_target() && candidate.virtual_selector().is_none())
        .filter(|candidate| (candidate.block, candidate.offset) > (row.block, row.offset))
        .min_by_key(|candidate| (candidate.block, candidate.offset))
}

pub(in crate::package::drivers) fn ssed_menu_record_target(
    package: &ReaderBookPackage,
    records: &[SsedMenuRecord],
    record: &SsedMenuRecord,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<TargetToken>> {
    let destination_index = SsedMenuDestinationIndex::new(records);
    ssed_menu_record_target_with_destination_index(package, record, &destination_index, diagnostics)
}

fn ssed_menu_record_target_with_destination_index(
    package: &ReaderBookPackage,
    record: &SsedMenuRecord,
    destination_index: &SsedMenuDestinationIndex<'_>,
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
    let next_destination = destination_index.next_after(destination);
    ssed_menu_destination_target(package, destination, next_destination, diagnostics)
}

pub(in crate::package::drivers) fn ssed_menu_destination_target(
    package: &ReaderBookPackage,
    destination: &SsedMenuDestination,
    next_destination: Option<&SsedMenuDestination>,
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
        return ssed_component_address_navigation_target(
            SsedComponentNavigationTargetRequest {
                package,
                component,
                block: destination.block,
                offset: destination.offset,
                next: next_destination.map(|next| (next.block, next.offset)),
                diagnostic_code: "ssed_navigation_non_body_target_deferred",
                source_label: "MENU/TOC",
            },
            diagnostics,
        );
    }
    let target = ssed_honmon_address_target(
        package,
        component.filename.clone(),
        destination.block,
        destination.offset,
        next_destination.map(|next| (next.block, next.offset)),
    )?;
    Ok(Some(target))
}

pub(in crate::package::drivers) fn nearest_higher_menu_destination<'a>(
    records: &'a [SsedMenuRecord],
    destination: &SsedMenuDestination,
) -> Option<&'a SsedMenuDestination> {
    let destination_index = SsedMenuDestinationIndex::new(records);
    destination_index.next_after(destination)
}

struct SsedMenuDestinationIndex<'a> {
    destinations: Vec<&'a SsedMenuDestination>,
}

impl<'a> SsedMenuDestinationIndex<'a> {
    fn new(records: &'a [SsedMenuRecord]) -> Self {
        let mut destinations = records
            .iter()
            .flat_map(|record| record.links.iter())
            .filter_map(|link| link.destination.as_ref())
            .filter(|destination| !destination.is_null())
            .collect::<Vec<_>>();
        destinations.sort_by_key(|destination| (destination.block, destination.offset));
        destinations.dedup_by_key(|destination| (destination.block, destination.offset));
        Self { destinations }
    }

    fn next_after(&self, destination: &SsedMenuDestination) -> Option<&'a SsedMenuDestination> {
        let key = (destination.block, destination.offset);
        let index = self
            .destinations
            .partition_point(|candidate| (candidate.block, candidate.offset) <= key);
        self.destinations.get(index).copied()
    }
}

pub(in crate::package::drivers) fn ssed_honmon_address_target(
    package: &ReaderBookPackage,
    component: String,
    block: u32,
    offset: u32,
    next: Option<(u32, u32)>,
) -> Result<TargetToken> {
    let bounded = next
        .filter(|(end_block, end_offset)| (*end_block, *end_offset) > (block, offset))
        .and_then(|(end_block, end_offset)| {
            let catalog = package.ssed_catalog.as_ref()?;
            let end_component = catalog.component_for_address(end_block)?;
            end_component
                .filename
                .eq_ignore_ascii_case(&component)
                .then_some((end_block, end_offset))
        });
    let target = if let Some((end_block, end_offset)) = bounded {
        InternalTarget::SsedBoundedAddress {
            component,
            block,
            offset,
            end_block,
            end_offset,
        }
    } else {
        InternalTarget::SsedAddress {
            component,
            block,
            offset,
        }
    };
    TargetToken::new(&target)
}

pub(in crate::package::drivers) fn ssed_component_address_navigation_target(
    request: SsedComponentNavigationTargetRequest<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<TargetToken>> {
    match request.component.role {
        SsedComponentRole::Menu => {
            let surface_id = ssed_direct_navigation_surface_id_for_component(request.component);
            return Ok(Some(TargetToken::new(&InternalTarget::MenuItem {
                surface_id,
                item_id: ssed_menu_address_item_id(request.block, request.offset),
            })?));
        }
        SsedComponentRole::Toc => {
            let surface_id = ssed_direct_navigation_surface_id_for_component(request.component);
            return Ok(Some(ssed_direct_navigation_target_for_component(
                request.component,
                surface_id,
                ssed_menu_address_item_id(request.block, request.offset),
            )?));
        }
        _ => {}
    }

    let resource = match request.component.role {
        SsedComponentRole::Colscr => InternalResource::SsedComponentAddress {
            component: request.component.filename.clone(),
            block: request.block,
            offset: request.offset,
            resource_kind: ResourceKind::Colscr,
        },
        SsedComponentRole::MonoScr => InternalResource::SsedComponentAddress {
            component: request.component.filename.clone(),
            block: request.block,
            offset: request.offset,
            resource_kind: ResourceKind::Image,
        },
        SsedComponentRole::PcmData => ssed_pcmdata_navigation_resource(
            request.package,
            request.component,
            request.block,
            request.offset,
            request.next,
        ),
        _ => {
            diagnostics.push(
                Diagnostic::info(
                    request.diagnostic_code,
                    format!(
                        "{} target points to {} ({:?}); non-body navigation routing is deferred",
                        request.source_label, request.component.filename, request.component.role
                    ),
                )
                .with_context("component", &request.component.filename),
            );
            return Ok(None);
        }
    };
    let resource = ResourceToken::new(&resource)?;
    Ok(Some(TargetToken::new(&InternalTarget::Resource {
        resource,
        anchor: None,
    })?))
}

pub(in crate::package::drivers) fn ssed_menu_address_item_id(block: u32, offset: u32) -> String {
    format!("addr:{block}:{offset}")
}

pub(in crate::package::drivers) fn ssed_direct_navigation_components(
    catalog: &SsedCatalog,
) -> Vec<&SsedComponent> {
    catalog
        .components
        .iter()
        .filter(|component| {
            component.has_positive_range()
                && matches!(
                    component.role,
                    SsedComponentRole::Menu | SsedComponentRole::Toc
                )
                && !ssed_component_name_is_multi_selector_child(&component.filename)
        })
        .collect()
}

pub(in crate::package::drivers) fn ssed_direct_navigation_surface_id_for_component(
    component: &SsedComponent,
) -> String {
    let upper = component.filename.to_ascii_uppercase();
    match upper.as_str() {
        "MENU.DIC" => "menu".to_owned(),
        "TOC.DIC" => "toc".to_owned(),
        _ => format!("ssed-nav:{}", hex::encode(component.filename.as_bytes())),
    }
}

pub(in crate::package::drivers) fn ssed_direct_navigation_component_name_from_surface_id(
    surface_id: &str,
) -> Option<String> {
    match surface_id {
        "menu" => Some("MENU.DIC".to_owned()),
        "toc" => Some("TOC.DIC".to_owned()),
        _ => surface_id
            .strip_prefix("ssed-nav:")
            .and_then(|encoded| hex::decode(encoded).ok())
            .and_then(|bytes| String::from_utf8(bytes).ok()),
    }
}

pub(in crate::package::drivers) fn ssed_direct_navigation_kind_for_component(
    component: &SsedComponent,
) -> NavigationSurfaceKind {
    let upper = component.filename.to_ascii_uppercase();
    if upper.starts_with("MENU") {
        return NavigationSurfaceKind::Menu;
    }
    if upper.starts_with("TOC") {
        return NavigationSurfaceKind::Toc;
    }
    match component.role {
        SsedComponentRole::Toc => NavigationSurfaceKind::Toc,
        _ => NavigationSurfaceKind::Menu,
    }
}

pub(in crate::package::drivers) fn ssed_direct_navigation_target_for_component(
    component: &SsedComponent,
    surface_id: String,
    item_id: String,
) -> Result<TargetToken> {
    match ssed_direct_navigation_kind_for_component(component) {
        NavigationSurfaceKind::Toc => TargetToken::new(&InternalTarget::TocItem {
            surface_id,
            item_id,
        }),
        _ => TargetToken::new(&InternalTarget::MenuItem {
            surface_id,
            item_id,
        }),
    }
}

fn ssed_component_name_is_multi_selector_child(filename: &str) -> bool {
    let upper = filename.to_ascii_uppercase();
    upper.starts_with("MUL") && upper.contains('_')
}

pub(in crate::package::drivers) struct SsedComponentNavigationTargetRequest<'a> {
    pub(in crate::package::drivers) package: &'a ReaderBookPackage,
    pub(in crate::package::drivers) component: &'a SsedComponent,
    pub(in crate::package::drivers) block: u32,
    pub(in crate::package::drivers) offset: u32,
    pub(in crate::package::drivers) next: Option<(u32, u32)>,
    pub(in crate::package::drivers) diagnostic_code: &'static str,
    pub(in crate::package::drivers) source_label: &'static str,
}

fn ssed_pcmdata_navigation_resource(
    package: &ReaderBookPackage,
    component: &SsedComponent,
    block: u32,
    offset: u32,
    next: Option<(u32, u32)>,
) -> InternalResource {
    if let Some((end_block, end_offset)) =
        ssed_navigation_range_end(package, component, block, offset, next)
    {
        InternalResource::SsedPcmDataRange {
            component: component.filename.clone(),
            start_block: block,
            start_offset: offset,
            end_block,
            end_offset,
        }
    } else {
        InternalResource::SsedComponentAddress {
            component: component.filename.clone(),
            block,
            offset,
            resource_kind: ResourceKind::PcmData,
        }
    }
}

fn ssed_navigation_range_end(
    package: &ReaderBookPackage,
    component: &SsedComponent,
    block: u32,
    offset: u32,
    next: Option<(u32, u32)>,
) -> Option<(u32, u32)> {
    let (next_block, next_offset) = next?;
    if (next_block, next_offset) <= (block, offset) {
        return None;
    }
    let catalog = package.ssed_catalog.as_ref()?;
    let next_component = catalog.component_for_address(next_block)?;
    if !next_component
        .filename
        .eq_ignore_ascii_case(&component.filename)
    {
        return None;
    }
    previous_ssed_address(next_block, next_offset).filter(|end| *end >= (block, offset))
}

fn previous_ssed_address(block: u32, offset: u32) -> Option<(u32, u32)> {
    if offset > 0 {
        Some((block, offset - 1))
    } else if block > 0 {
        Some((block - 1, BLOCK_SIZE - 1))
    } else {
        None
    }
}

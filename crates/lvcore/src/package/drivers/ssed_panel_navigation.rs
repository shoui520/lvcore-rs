use super::*;
use crate::package::drivers::ssed_navigation::{
    SsedComponentNavigationTargetRequest, ssed_component_address_navigation_target,
    ssed_honmon_address_target,
};

pub(super) fn ssed_panel_inline_cell_to_navigation_cell(
    package: &ReaderBookPackage,
    cell: &SsedPanelInlineCell,
    known_panel_ids: &BTreeSet<String>,
    gaiji_policy: &GaijiPolicy,
) -> Result<PanelCell> {
    let rich_label = package.ssed_rich_label_with_policy(&cell.label, gaiji_policy);
    let mut diagnostics = rich_label.diagnostics;
    let target = if let Some(address) = parse_lved_address(&cell.action_verb) {
        ssed_panel_address_target(
            package,
            address.block,
            address.offset,
            None,
            &mut diagnostics,
        )?
    } else if let Some(panel_id) = panel_ref_from_action_or_ref(cell, known_panel_ids) {
        Some(TargetToken::new(&InternalTarget::PanelCell {
            panel_id: panel_id.to_owned(),
            row: 0,
            column: 0,
        })?)
    } else if let Some(block) = cell.target_block {
        ssed_panel_address_target(
            package,
            block,
            cell.target_offset.unwrap_or(0),
            None,
            &mut diagnostics,
        )?
    } else {
        None
    };
    Ok(PanelCell {
        href: None,
        panel_id: cell.panel_id.clone(),
        row: cell.row.unwrap_or(cell.cell_index),
        column: cell.column.unwrap_or(0),
        label_html: rich_label.html,
        label_text: rich_label.text,
        target,
        diagnostics,
    })
}

fn panel_ref_from_action_or_ref<'a>(
    cell: &'a SsedPanelInlineCell,
    known_panel_ids: &BTreeSet<String>,
) -> Option<&'a str> {
    let action_ref = panel_ref_from_action(&cell.action_verb);
    let cell_ref = (!cell.ref_id.trim().is_empty()).then_some(cell.ref_id.as_str());
    match (action_ref, cell_ref) {
        (Some(action), Some(_reference)) if known_panel_ids.contains(action) => Some(action),
        (Some(_action), Some(reference)) if known_panel_ids.contains(reference) => Some(reference),
        (Some(action), _) => Some(action),
        (None, Some(reference)) => Some(reference),
        (None, None) => None,
    }
}

fn panel_ref_from_action(action_verb: &str) -> Option<&str> {
    let action = action_verb.trim();
    let prefix = "lved.panel:";
    action
        .get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .and_then(|_| action.get(prefix.len()..))
        .map(str::trim)
        .filter(|panel_id| !panel_id.is_empty())
}

pub(super) fn ssed_panel_bin_record_to_navigation_cell(
    package: &ReaderBookPackage,
    data_ref: &SsedPanelDataRef,
    record: &SsedPanelBinRecord,
    next_record: Option<&SsedPanelBinRecord>,
    diagnostics: &mut Vec<Diagnostic>,
    gaiji_policy: &GaijiPolicy,
) -> Result<PanelCell> {
    let rich_label = package.ssed_rich_label_with_policy(&record.text, gaiji_policy);
    Ok(PanelCell {
        href: None,
        panel_id: data_ref.panel_id.clone(),
        row: record.index,
        column: 0,
        label_html: rich_label.html,
        label_text: rich_label.text,
        target: ssed_panel_record_target(package, record, next_record, diagnostics)?,
        diagnostics: rich_label.diagnostics,
    })
}

fn ssed_panel_record_target(
    package: &ReaderBookPackage,
    record: &SsedPanelBinRecord,
    next_record: Option<&SsedPanelBinRecord>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<TargetToken>> {
    ssed_panel_address_target(
        package,
        record.block,
        record.offset,
        next_record.map(|next| (next.block, next.offset)),
        diagnostics,
    )
}

fn ssed_panel_address_target(
    package: &ReaderBookPackage,
    block: u32,
    offset: u32,
    next: Option<(u32, u32)>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<TargetToken>> {
    if block == 0 && offset == 0 {
        return Ok(None);
    }
    let Some(catalog) = &package.ssed_catalog else {
        diagnostics.push(Diagnostic::error(
            "ssed_catalog_missing",
            "Panel BIN target cannot be resolved without a catalog",
        ));
        return Ok(None);
    };
    let Some(component) = catalog.component_for_address(block) else {
        diagnostics.push(Diagnostic::warning(
            "ssed_panel_target_unresolved",
            format!(
                "Panel target block {} offset {} is outside declared components",
                block, offset
            ),
        ));
        return Ok(None);
    };
    if component.relative_offset(block, offset).is_none() {
        diagnostics.push(Diagnostic::warning(
            "ssed_panel_target_invalid",
            format!(
                "{} does not contain Panel target block {} offset {}",
                component.filename, block, offset
            ),
        ));
        return Ok(None);
    }
    if component.role != SsedComponentRole::Honmon {
        return ssed_component_address_navigation_target(
            SsedComponentNavigationTargetRequest {
                package,
                component,
                block,
                offset,
                next,
                diagnostic_code: "ssed_panel_non_body_target_deferred",
                source_label: "Panel",
            },
            diagnostics,
        );
    }
    Ok(Some(ssed_honmon_address_target(
        package,
        component.filename.clone(),
        block,
        offset,
        next,
    )?))
}

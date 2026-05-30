use super::*;

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

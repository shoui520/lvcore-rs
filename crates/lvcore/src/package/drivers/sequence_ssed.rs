use super::*;

impl ReaderBookPackage {
    pub(super) fn resolve_ssed_title_index_window(
        &self,
        target: &TargetToken,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let InternalTarget::SsedAddress {
            component,
            block,
            offset,
        } = target.decode()?
        else {
            return Ok(None);
        };

        let mut rows = Vec::new();
        let mut diagnostics = self.scan_ssed_simple_index_rows(None, |row| {
            rows.push(row);
            Ok(true)
        })?;
        if rows.is_empty() {
            diagnostics.push(Diagnostic::info(
                "sequence_deferred",
                "SSED title/index order is unavailable for this target",
            ));
            return Ok(Some(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics,
            }));
        }

        let center_index = rows.iter().position(|row| {
            row.body.block == block
                && row.body.offset == offset
                && self
                    .ssed_component_for_index_pointer(row.body)
                    .is_some_and(|row_component| row_component.eq_ignore_ascii_case(&component))
        });
        let Some(center_index) = center_index else {
            diagnostics.push(Diagnostic::info(
                "sequence_target_not_in_title_index",
                "target is not present in the simple SSED title/index order",
            ));
            return Ok(Some(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics,
            }));
        };

        let mut center = self.render_target(target, options)?;
        let center_label = self.ssed_index_row_label(&rows[center_index]);
        center.title = Some(center_label.text);
        center.diagnostics.extend(center_label.diagnostics);
        let before_start = center_index.saturating_sub(before);
        let after_end = rows
            .len()
            .min(center_index.saturating_add(after).saturating_add(1));

        let mut before_views = Vec::new();
        for row in &rows[before_start..center_index] {
            if let Some(view) = self.render_ssed_index_row(row, options, &mut diagnostics)? {
                before_views.push(view);
            }
        }
        let mut after_views = Vec::new();
        for row in &rows[center_index + 1..after_end] {
            if let Some(view) = self.render_ssed_index_row(row, options, &mut diagnostics)? {
                after_views.push(view);
            }
        }

        Ok(Some(TargetWindow {
            center,
            before: before_views,
            after: after_views,
            diagnostics,
        }))
    }

    fn render_ssed_index_row(
        &self,
        row: &SsedIndexRow,
        options: &RenderOptions,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Option<ResolvedTargetView>> {
        let target = match self.ssed_target_for_index_pointer(row.body)? {
            Ok(target) => target,
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                return Ok(None);
            }
        };
        let mut view = self.render_target(&target, options)?;
        let label = self.ssed_index_row_label(row);
        view.title = Some(label.text);
        view.diagnostics.extend(label.diagnostics);
        Ok(Some(view))
    }

    pub(super) fn resolve_ssed_menu_window(
        &self,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let Some(SequenceHint::MenuOrder { value: surface_id }) = sequence_hint else {
            return Ok(None);
        };
        let surface = self.open_surface(surface_id)?;
        let nodes = match surface {
            NavigationSurface::SimpleMenu { nodes, .. }
            | NavigationSurface::HierarchicalTree { nodes, .. } => nodes,
            _ => {
                return Ok(Some(TargetWindow {
                    center: self.render_target(target, options)?,
                    before: Vec::new(),
                    after: Vec::new(),
                    diagnostics: vec![Diagnostic::info(
                        "sequence_surface_not_ordered",
                        format!("{surface_id} is not an ordered SSED navigation surface"),
                    )],
                }));
            }
        };
        let mut ordered = Vec::new();
        collect_navigation_node_ordered_targets(&nodes, &mut ordered);
        Ok(Some(self.resolve_ordered_target_window(
            target,
            &ordered,
            before,
            after,
            options,
            Diagnostic::info(
                "sequence_target_not_in_ssed_menu",
                "target is not present in the requested SSED MENU/TOC order",
            ),
        )?))
    }

    pub(super) fn resolve_ssed_panel_window(
        &self,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let Some(SequenceHint::PanelOrder { value: panel_id }) = sequence_hint else {
            return Ok(None);
        };
        let surface_id = if panel_id == "panels" || panel_id.starts_with("panels:") {
            panel_id.clone()
        } else {
            format!("panels:{panel_id}")
        };
        let surface = self.open_surface(&surface_id)?;
        let NavigationSurface::Panel { cells, .. } = surface else {
            return Ok(Some(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![Diagnostic::info(
                    "sequence_surface_not_ordered",
                    format!("{surface_id} is not an SSED panel surface"),
                )],
            }));
        };
        let mut ordered = Vec::new();
        collect_panel_cell_ordered_targets(&cells, &mut ordered);
        Ok(Some(self.resolve_ordered_target_window(
            target,
            &ordered,
            before,
            after,
            options,
            Diagnostic::info(
                "sequence_target_not_in_ssed_panel",
                "target is not present in the requested SSED panel order",
            ),
        )?))
    }
}

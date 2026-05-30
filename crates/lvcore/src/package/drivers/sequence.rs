use super::*;

impl ReaderBookPackage {
    fn resolve_ssed_title_index_window(
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

    fn resolve_ssed_menu_window(
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

    fn resolve_ssed_panel_window(
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

    fn resolve_ordered_target_window(
        &self,
        target: &TargetToken,
        ordered: &[OrderedSequenceTarget],
        before: usize,
        after: usize,
        options: &RenderOptions,
        not_found_diagnostic: Diagnostic,
    ) -> Result<TargetWindow> {
        let Some(center_index) = ordered
            .iter()
            .position(|candidate| &candidate.target == target)
        else {
            return Ok(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![not_found_diagnostic],
            });
        };

        let mut center = self.render_target(target, options)?;
        if let Some(title) = &ordered[center_index].title {
            center.title = Some(title.clone());
        }

        let before_start = center_index.saturating_sub(before);
        let before_views = ordered[before_start..center_index]
            .iter()
            .map(|item| self.render_ordered_sequence_target(item, options))
            .collect::<Result<Vec<_>>>()?;
        let after_end = (center_index + 1 + after).min(ordered.len());
        let after_views = ordered[center_index + 1..after_end]
            .iter()
            .map(|item| self.render_ordered_sequence_target(item, options))
            .collect::<Result<Vec<_>>>()?;

        Ok(TargetWindow {
            center,
            before: before_views,
            after: after_views,
            diagnostics: Vec::new(),
        })
    }

    fn render_ordered_sequence_target(
        &self,
        item: &OrderedSequenceTarget,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let mut view = self.render_target(&item.target, options)?;
        if let Some(title) = &item.title {
            view.title = Some(title.clone());
        }
        Ok(view)
    }

    fn resolve_lved_list_window(
        &self,
        target: &TargetToken,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let InternalTarget::LvedRow {
            table,
            row_id,
            anchor: _,
            query: _,
        } = target.decode()?
        else {
            return Ok(None);
        };
        if !table.eq_ignore_ascii_case("content") {
            return Ok(None);
        }
        let Some(store) = &self.lved_store else {
            return Ok(None);
        };
        let Some(window) = store.list_window_for_content(row_id, before, after)? else {
            return Ok(Some(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![Diagnostic::info(
                    "sequence_target_not_in_lved_list",
                    "target is not present in the LVED list order",
                )],
            }));
        };

        let mut center = self.render_target(target, options)?;
        center.title = Some(window.center.title_text);
        let before = window
            .before
            .iter()
            .map(|hit| self.render_lved_list_hit(hit, options))
            .collect::<Result<Vec<_>>>()?;
        let after = window
            .after
            .iter()
            .map(|hit| self.render_lved_list_hit(hit, options))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(TargetWindow {
            center,
            before,
            after,
            diagnostics: Vec::new(),
        }))
    }

    fn render_lved_list_hit(
        &self,
        hit: &crate::lved_sqlite::LvedSearchHit,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let target = TargetToken::new(&InternalTarget::LvedRow {
            table: "content".to_owned(),
            row_id: hit.content_id,
            anchor: hit.anchor.clone(),
            query: None,
        })?;
        let mut view = self.render_target(&target, options)?;
        view.title = Some(hit.title_text.clone());
        Ok(view)
    }

    fn resolve_lved_tree_window(
        &self,
        target: &TargetToken,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let InternalTarget::LvedRow {
            table,
            row_id,
            anchor: _,
            query: _,
        } = target.decode()?
        else {
            return Ok(None);
        };
        if !table.eq_ignore_ascii_case("content") {
            return Ok(None);
        }
        let Some(store) = &self.lved_store else {
            return Ok(None);
        };
        let rows = store
            .tree_index_items()?
            .into_iter()
            .filter(|row| row.data_id >= 0)
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return Ok(None);
        }
        let Some(center_index) = rows.iter().position(|row| row.data_id == row_id) else {
            return Ok(Some(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![Diagnostic::info(
                    "sequence_target_not_in_lved_tree",
                    "target is not present in the LVED tree.idx order",
                )],
            }));
        };
        let mut center = self.render_lved_tree_item(&rows[center_index], options)?;
        center.title = Some(rows[center_index].label.clone());
        let before_start = center_index.saturating_sub(before);
        let before_views = rows[before_start..center_index]
            .iter()
            .map(|row| self.render_lved_tree_item(row, options))
            .collect::<Result<Vec<_>>>()?;
        let after_end = (center_index + 1 + after).min(rows.len());
        let after_views = rows[center_index + 1..after_end]
            .iter()
            .map(|row| self.render_lved_tree_item(row, options))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(TargetWindow {
            center,
            before: before_views,
            after: after_views,
            diagnostics: Vec::new(),
        }))
    }

    fn render_lved_tree_item(
        &self,
        item: &crate::lved_sqlite::LvedTreeIndexItem,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let target = TargetToken::new(&InternalTarget::LvedRow {
            table: "content".to_owned(),
            row_id: item.data_id,
            anchor: None,
            query: item.query.clone(),
        })?;
        let mut view = self.render_target(&target, options)?;
        view.title = Some(item.label.clone());
        Ok(view)
    }

    fn resolve_multiview_menu_window(
        &self,
        target: &TargetToken,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let InternalTarget::MultiviewHref { href, anchor } = target.decode()? else {
            return Ok(None);
        };
        let surface = self.open_multiview_menu_surface("menuData")?;
        let NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
            return Ok(None);
        };
        let mut ordered = Vec::new();
        collect_navigation_node_targets(&nodes, &mut ordered);
        let Some(center_index) = ordered.iter().position(|candidate| {
            matches!(
                candidate.decode(),
                Ok(InternalTarget::MultiviewHref {
                    href: candidate_href,
                    anchor: candidate_anchor,
                }) if candidate_href == href && candidate_anchor == anchor
            )
        }) else {
            return Ok(None);
        };
        let before_start = center_index.saturating_sub(before);
        let before_views = ordered[before_start..center_index]
            .iter()
            .map(|token| self.render_target(token, options))
            .collect::<Result<Vec<_>>>()?;
        let after_end = (center_index + 1 + after).min(ordered.len());
        let after_views = ordered[center_index + 1..after_end]
            .iter()
            .map(|token| self.render_target(token, options))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(TargetWindow {
            center: self.render_target(target, options)?,
            before: before_views,
            after: after_views,
            diagnostics: Vec::new(),
        }))
    }

    fn resolve_hourei_law_window(
        &self,
        target: &TargetToken,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let InternalTarget::HoureiLaw { hore_id, .. } = target.decode()? else {
            return Ok(None);
        };
        let Some(store) = &self.hourei_store else {
            return Ok(None);
        };
        let Some(window) = store.law_window(&hore_id, before, after)? else {
            return Ok(Some(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![Diagnostic::info(
                    "sequence_target_not_in_hourei_law_order",
                    "target is not present in the Hourei kana-order law list",
                )],
            }));
        };
        let mut center = self.render_target(target, options)?;
        center.title = Some(hourei_law_node_label(&window.center));
        let before = window
            .before
            .into_iter()
            .map(|entry| self.render_hourei_law_entry(&entry, options))
            .collect::<Result<Vec<_>>>()?;
        let after = window
            .after
            .into_iter()
            .map(|entry| self.render_hourei_law_entry(&entry, options))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(TargetWindow {
            center,
            before,
            after,
            diagnostics: Vec::new(),
        }))
    }

    fn render_hourei_law_entry(
        &self,
        entry: &crate::hourei::HoureiLawEntry,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let target = TargetToken::new(&InternalTarget::HoureiLaw {
            hore_id: entry.hore_id.clone(),
            anchor: None,
        })?;
        let mut view = self.render_target(&target, options)?;
        view.title = Some(hourei_law_node_label(entry));
        Ok(view)
    }
}

impl SequenceProvider for ReaderBookPackage {
    fn resolve_target_window(
        &self,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<TargetWindow> {
        if self.metadata.format_family == FormatFamily::Ssed
            && sequence_hint.is_none_or(|hint| {
                matches!(
                    hint,
                    SequenceHint::TitleIndexOrder { .. } | SequenceHint::BodyOrder
                )
            })
            && let Some(window) =
                self.resolve_ssed_title_index_window(target, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::Ssed
            && matches!(sequence_hint, Some(SequenceHint::MenuOrder { .. }))
            && let Some(window) =
                self.resolve_ssed_menu_window(target, sequence_hint, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::Ssed
            && matches!(sequence_hint, Some(SequenceHint::PanelOrder { .. }))
            && let Some(window) =
                self.resolve_ssed_panel_window(target, sequence_hint, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::LvedSqlite3
            && matches!(sequence_hint, Some(SequenceHint::LvedTreeOrder))
            && let Some(window) = self.resolve_lved_tree_window(target, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::LvedSqlite3
            && sequence_hint.is_none_or(|hint| {
                matches!(hint, SequenceHint::LvedListOrder | SequenceHint::BodyOrder)
            })
            && let Some(window) = self.resolve_lved_list_window(target, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::LvlMultiView
            && sequence_hint.is_none_or(|hint| {
                matches!(
                    hint,
                    SequenceHint::MultiviewTreeOrder | SequenceHint::BodyOrder
                )
            })
            && let Some(window) =
                self.resolve_multiview_menu_window(target, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::Hourei
            && sequence_hint.is_none_or(|hint| {
                matches!(
                    hint,
                    SequenceHint::HoureiLawArticleOrder | SequenceHint::BodyOrder
                )
            })
            && let Some(window) = self.resolve_hourei_law_window(target, before, after, options)?
        {
            return Ok(window);
        }
        Ok(TargetWindow {
            center: self.render_target(target, options)?,
            before: Vec::new(),
            after: Vec::new(),
            diagnostics: vec![Diagnostic::info(
                "sequence_deferred",
                "sequence provider is not implemented yet",
            )],
        })
    }
}

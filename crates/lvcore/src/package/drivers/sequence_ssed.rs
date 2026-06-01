use super::sequence::sequence_targets_match;
use super::*;
use std::collections::VecDeque;

const SSED_SEQUENCE_SURFACE_PAGE_LIMIT: usize = 128;
const SSED_SEQUENCE_SURFACE_MAX_PAGES: usize = 4096;

impl ReaderBookPackage {
    pub(super) fn resolve_ssed_title_index_window(
        &self,
        target: &TargetToken,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let (component, block, offset) = match target.decode()? {
            InternalTarget::SsedAddress {
                component,
                block,
                offset,
            }
            | InternalTarget::SsedBoundedAddress {
                component,
                block,
                offset,
                ..
            }
            | InternalTarget::SsedIndexAddress {
                component,
                block,
                offset,
                ..
            } => (component, block, offset),
            _ => return Ok(None),
        };

        let skip_backward_rows = self.ssed_has_forward_browse_index();
        let mut scanned_any = false;
        let mut before_rows = VecDeque::<SsedIndexRow>::with_capacity(before);
        let mut center_row = None::<SsedIndexRow>;
        let mut tail_rows = Vec::<SsedIndexRow>::with_capacity(after.saturating_add(1));
        let mut diagnostics = self.scan_ssed_simple_index_rows_with_filters(
            None,
            |component| {
                !(skip_backward_rows && ssed_index_component_name_is_backward(&component.filename))
            },
            |_, _| true,
            |row| {
                scanned_any = true;
                let Some(row_component) = self.ssed_component_for_index_pointer(row.body) else {
                    return Ok(true);
                };
                let row_matches = row.body.block == block
                    && row.body.offset == offset
                    && row_component.eq_ignore_ascii_case(&component);
                if center_row.is_some() {
                    tail_rows.push(row);
                    return Ok(tail_rows.len() <= after);
                }
                if row_matches {
                    center_row = Some(row);
                    return Ok(true);
                }
                if before > 0 {
                    if before_rows.len() >= before {
                        before_rows.pop_front();
                    }
                    before_rows.push_back(row);
                }
                Ok(true)
            },
        )?;
        if !scanned_any {
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

        let Some(center_row) = center_row else {
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

        let mut context_rows = Vec::with_capacity(
            before_rows
                .len()
                .saturating_add(1)
                .saturating_add(tail_rows.len()),
        );
        context_rows.extend(before_rows.iter().cloned());
        let center_index = context_rows.len();
        context_rows.push(center_row.clone());
        context_rows.extend(tail_rows.iter().cloned());

        let center = match self.render_ssed_index_row(
            &center_row,
            next_distinct_index_row(&context_rows, center_index),
            options,
            &mut diagnostics,
        )? {
            Some(view) => view,
            None => self.render_target(target, options)?,
        };

        let mut before_views = Vec::new();
        for index in 0..center_index {
            if let Some(view) = self.render_ssed_index_row(
                &context_rows[index],
                next_distinct_index_row(&context_rows, index),
                options,
                &mut diagnostics,
            )? {
                before_views.push(view);
            }
        }
        let mut after_views = Vec::new();
        for index in center_index + 1..context_rows.len().min(center_index + 1 + after) {
            if let Some(view) = self.render_ssed_index_row(
                &context_rows[index],
                next_distinct_index_row(&context_rows, index),
                options,
                &mut diagnostics,
            )? {
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
        next_row: Option<&SsedIndexRow>,
        options: &RenderOptions,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Option<ResolvedTargetView>> {
        let target = match self.ssed_target_for_index_row(row, next_row)? {
            Ok(target) => target,
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                return Ok(None);
            }
        };
        let mut view = self.render_target(&target, options)?;
        let label = self.ssed_index_row_label_with_policy(row, &options.gaiji_policy);
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
        let label_options = LabelOptions {
            gaiji_policy: options.gaiji_policy.clone(),
        };
        let mut ordered = Vec::new();
        let mut diagnostics = Vec::new();
        let mut cursor = None::<String>;
        let mut reached_page_limit = false;
        for page_index in 0..SSED_SEQUENCE_SURFACE_MAX_PAGES {
            let surface = self.open_surface_page_with_options(
                surface_id,
                cursor.as_deref(),
                SSED_SEQUENCE_SURFACE_PAGE_LIMIT,
                &label_options,
            )?;
            let next_cursor = match surface {
                NavigationSurface::SimpleMenu {
                    nodes, next_cursor, ..
                }
                | NavigationSurface::HierarchicalTree {
                    nodes, next_cursor, ..
                } => {
                    collect_navigation_node_ordered_targets(&nodes, &mut ordered);
                    next_cursor
                }
                NavigationSurface::Deferred {
                    diagnostics: surface_diagnostics,
                    ..
                } => {
                    diagnostics.extend(surface_diagnostics);
                    break;
                }
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
            if ordered
                .iter()
                .position(|candidate| sequence_targets_match(&candidate.target, target))
                .is_some_and(|center_index| ordered.len() > center_index.saturating_add(after))
            {
                break;
            }
            let Some(next_cursor) = next_cursor else {
                break;
            };
            if page_index + 1 >= SSED_SEQUENCE_SURFACE_MAX_PAGES {
                reached_page_limit = true;
                break;
            }
            if cursor.as_deref() == Some(next_cursor.as_str()) {
                diagnostics.push(Diagnostic::warning(
                    "sequence_surface_cursor_stalled",
                    format!("{surface_id} returned a repeated pagination cursor"),
                ));
                break;
            }
            cursor = Some(next_cursor);
        }
        if reached_page_limit {
            diagnostics.push(Diagnostic::warning(
                "sequence_surface_page_limit_reached",
                format!("{surface_id} sequence lookup stopped at the page limit"),
            ));
        }
        let mut window = self.resolve_ordered_target_window(
            target,
            &ordered,
            before,
            after,
            options,
            Diagnostic::info(
                "sequence_target_not_in_ssed_menu",
                "target is not present in the requested SSED MENU/TOC order",
            ),
        )?;
        window.diagnostics.extend(diagnostics);
        Ok(Some(window))
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
        let surface = self.open_surface_with_options(
            &surface_id,
            &LabelOptions {
                gaiji_policy: options.gaiji_policy.clone(),
            },
        )?;
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
fn next_distinct_index_row(rows: &[SsedIndexRow], index: usize) -> Option<&SsedIndexRow> {
    let row = rows.get(index)?;
    rows.iter()
        .skip(index + 1)
        .find(|next| next.body != row.body)
}

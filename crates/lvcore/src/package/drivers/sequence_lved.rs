use super::*;

impl ReaderBookPackage {
    pub(super) fn resolve_lved_list_window(
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

    pub(super) fn resolve_lved_tree_window(
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
        let rows = store.tree_index_items_arc()?;
        let rows = rows
            .iter()
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
        let mut center = self.render_lved_tree_item(rows[center_index], options)?;
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
}

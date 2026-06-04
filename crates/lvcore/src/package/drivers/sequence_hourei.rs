use super::hourei_navigation::{hourei_kana_initial_from_surface_id, hourei_kana_panel_surface_id};
use super::*;

impl ReaderBookPackage {
    pub(super) fn resolve_hourei_law_window(
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

    pub(super) fn resolve_hourei_kana_panel_window(
        &self,
        target: &TargetToken,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let InternalTarget::MenuItem {
            surface_id,
            item_id,
        } = target.decode()?
        else {
            return Ok(None);
        };
        if item_id != "root" || hourei_kana_initial_from_surface_id(&surface_id).is_none() {
            return Ok(None);
        }

        let surface = self.open_hourei_kana_panel_surface(hourei_kana_panel_surface_id())?;
        let NavigationSurface::Panel { cells, .. } = surface else {
            return Ok(Some(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![Diagnostic::info(
                    "sequence_surface_not_ordered",
                    "Hourei kana panel is not available as an ordered panel surface",
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
                "sequence_target_not_in_hourei_kana_panel",
                "target is not present in the Hourei kana panel order",
            ),
        )?))
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

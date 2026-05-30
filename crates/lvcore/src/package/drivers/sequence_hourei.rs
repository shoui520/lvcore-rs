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

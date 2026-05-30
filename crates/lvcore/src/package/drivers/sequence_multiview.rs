use super::*;

impl ReaderBookPackage {
    pub(super) fn resolve_multiview_menu_window(
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
}

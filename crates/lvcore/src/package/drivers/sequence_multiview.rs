use super::*;

impl ReaderBookPackage {
    pub(super) fn resolve_multiview_list_window(
        &self,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let Some(href) = multiview_list_href_from_sequence_hint(sequence_hint) else {
            return Ok(None);
        };
        let Some((_title, surface)) = self.multiview_navigation_surface_for_href(href)? else {
            return Ok(None);
        };
        let NavigationSurface::TitleIndexBrowse { items, .. } = surface else {
            return Ok(None);
        };
        let ordered = items
            .into_iter()
            .map(|item| OrderedSequenceTarget {
                target: item.target,
                title: Some(item.label_text),
            })
            .collect::<Vec<_>>();
        Ok(Some(self.resolve_ordered_target_window(
            target,
            &ordered,
            before,
            after,
            options,
            Diagnostic::info(
                "sequence_target_not_in_multiview_list",
                "target is not present in the MultiView list order",
            ),
        )?))
    }

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
        let mut ordered = Vec::new();
        for menu_file in self.multiview_menu_surface_files()? {
            let bytes = self.storage.read(Path::new(&menu_file))?;
            let xml = String::from_utf8(bytes).map_err(|error| {
                Error::Driver(format!("{menu_file} is not valid UTF-8: {error}"))
            })?;
            let items = parse_menu_data(&xml)?;
            collect_multiview_menu_ordered_targets(&items, &mut ordered)?;
        }
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

fn multiview_list_href_from_sequence_hint(sequence_hint: Option<&SequenceHint>) -> Option<&str> {
    let Some(SequenceHint::TitleIndexOrder { value, .. }) = sequence_hint else {
        return None;
    };
    value
        .strip_prefix("multiview:")
        .filter(|href| !href.trim().is_empty())
}

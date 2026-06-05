use super::hourei_navigation::hourei_kana_panel_surface_id;
use super::*;

impl SequenceProvider for ReaderBookPackage {
    fn resolve_target_window(
        &self,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<TargetWindow> {
        if let Some(SequenceHint::SearchResults { value }) = sequence_hint
            && let Some(window) =
                self.resolve_search_results_window(target, value, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::Ssed
            && sequence_hint.is_none_or(|hint| {
                matches!(
                    hint,
                    SequenceHint::TitleIndexOrder { .. } | SequenceHint::BodyOrder
                )
            })
            && let Some(window) =
                self.resolve_ssed_title_index_window(target, sequence_hint, before, after, options)?
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
        if self.lved_store.is_some()
            && sequence_hint.is_some_and(is_lved_tree_sequence_hint)
            && let Some(window) = self.resolve_lved_tree_window(target, before, after, options)?
        {
            return Ok(window);
        }
        if self.lved_store.is_some()
            && sequence_hint.is_none_or(is_lved_list_sequence_hint)
            && let Some(window) = self.resolve_lved_list_window(target, before, after, options)?
        {
            return Ok(window);
        }
        if self.metadata.format_family == FormatFamily::LvlMultiView
            && matches!(
                sequence_hint,
                Some(SequenceHint::TitleIndexOrder { value, .. }) if value.starts_with("multiview:")
            )
            && let Some(window) =
                self.resolve_multiview_list_window(target, sequence_hint, before, after, options)?
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
        if self.metadata.format_family == FormatFamily::Hourei
            && matches!(
                sequence_hint,
                Some(SequenceHint::PanelOrder { value }) if value == hourei_kana_panel_surface_id()
            )
            && let Some(window) =
                self.resolve_hourei_kana_panel_window(target, before, after, options)?
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

fn is_lved_tree_sequence_hint(hint: &SequenceHint) -> bool {
    matches!(hint, SequenceHint::LvedTreeOrder)
        || matches!(
            hint,
            SequenceHint::TitleIndexOrder { value, .. } if value == "lved-tree"
        )
}

fn is_lved_list_sequence_hint(hint: &SequenceHint) -> bool {
    matches!(hint, SequenceHint::LvedListOrder | SequenceHint::BodyOrder)
        || matches!(
            hint,
            SequenceHint::TitleIndexOrder { value, .. } if value == "lved-list"
        )
}

impl ReaderBookPackage {
    pub(super) fn resolve_search_results_window(
        &self,
        target: &TargetToken,
        value: &str,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let sequence = match SearchResultSequence::decode(value) {
            Ok(sequence) => sequence,
            Err(error) => {
                return Ok(Some(TargetWindow {
                    center: self.render_target(target, options)?,
                    before: Vec::new(),
                    after: Vec::new(),
                    diagnostics: vec![Diagnostic::warning(
                        "search_results_sequence_invalid",
                        error.to_string(),
                    )],
                }));
            }
        };
        let ordered = sequence
            .targets
            .into_iter()
            .map(|item| OrderedSequenceTarget {
                target: item.target,
                title: item.title,
            })
            .collect::<Vec<_>>();
        Ok(Some(self.resolve_ordered_target_window(
            target,
            &ordered,
            before,
            after,
            options,
            Diagnostic::info(
                "sequence_target_not_in_search_results",
                "target is not present in the provided search-result order",
            ),
        )?))
    }

    pub(super) fn resolve_ordered_target_window(
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
            .position(|candidate| sequence_targets_match(&candidate.target, target))
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
}

pub(super) fn sequence_targets_match(candidate: &TargetToken, target: &TargetToken) -> bool {
    candidate == target
        || match (candidate.decode(), target.decode()) {
            (Ok(candidate), Ok(target)) => internal_sequence_targets_match(&candidate, &target),
            _ => false,
        }
}

fn internal_sequence_targets_match(candidate: &InternalTarget, target: &InternalTarget) -> bool {
    ssed_sequence_address(candidate)
        .zip(ssed_sequence_address(target))
        .is_some_and(|(candidate, target)| candidate == target)
}

fn ssed_sequence_address(target: &InternalTarget) -> Option<(&str, u32, u32)> {
    match target {
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
        } => Some((component.as_str(), *block, *offset)),
        _ => None,
    }
}

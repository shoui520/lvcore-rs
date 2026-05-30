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

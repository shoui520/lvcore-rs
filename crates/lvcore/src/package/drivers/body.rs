use super::*;

impl ReaderBookPackage {
    pub(super) fn resolved_kind_for_body_target(
        &self,
        target: &TargetToken,
    ) -> Result<ResolvedTargetKind> {
        match target.decode()? {
            InternalTarget::LvedRow { table, .. } if table.eq_ignore_ascii_case("info") => {
                Ok(ResolvedTargetKind::InfoPage)
            }
            InternalTarget::LvedInfoPage { .. } => Ok(ResolvedTargetKind::InfoPage),
            InternalTarget::LvedNamedPage { .. } => Ok(ResolvedTargetKind::InfoPage),
            InternalTarget::SsedAuxRecord { .. } => Ok(ResolvedTargetKind::InfoPage),
            InternalTarget::HoureiLaw { .. } => Ok(ResolvedTargetKind::LawArticle),
            _ => Ok(ResolvedTargetKind::EntryBody),
        }
    }

    pub(super) fn title_for_body_target(&self, target: &TargetToken) -> Result<Option<String>> {
        match target.decode()? {
            InternalTarget::HoureiLaw { hore_id, .. } => {
                let Some(store) = &self.hourei_store else {
                    return Ok(None);
                };
                Ok(store
                    .law_entry(&hore_id)?
                    .map(|entry| hourei_law_node_label(&entry)))
            }
            InternalTarget::SsedAuxRecord { source, key, .. }
                if source == BRITANNICA_CHRONOLOGY_SOURCE_ID =>
            {
                Ok(lookup_britannica_chronology_record(&self.root, &key)?
                    .map(|record| record.title()))
            }
            _ => Ok(None),
        }
    }
}

impl BodyProvider for ReaderBookPackage {
    fn visual_body_for_target(&self, token: &TargetToken) -> Result<VisualBody> {
        match token.decode()? {
            InternalTarget::SsedDenseAnchor {
                anchor,
                resolver_hint,
            } => self.visual_body_for_ssed_dense_anchor(&anchor, resolver_hint.as_deref()),
            InternalTarget::SsedAddress {
                component,
                block,
                offset,
            } => self.visual_body_for_ssed_address(&component, block, offset),
            InternalTarget::SsedIndexAddress {
                component,
                block,
                offset,
                index_component,
            } => {
                self.visual_body_for_ssed_index_address(&component, block, offset, &index_component)
            }
            InternalTarget::SsedBoundedAddress {
                component,
                block,
                offset,
                end_block,
                end_offset,
            } => self.visual_body_for_ssed_bounded_address(
                &component, block, offset, end_block, end_offset,
            ),
            InternalTarget::SsedAuxRecord { source, key, .. }
                if source == BRITANNICA_CHRONOLOGY_SOURCE_ID =>
            {
                self.visual_body_for_britannica_chronology_record(&key)
            }
            InternalTarget::LvedRow {
                table,
                row_id,
                anchor: _,
                query: _,
            } => self.visual_body_for_lved_row(&table, row_id),
            InternalTarget::LvedInfoPage { name, anchor: _ } => {
                self.visual_body_for_lved_info_name(&name)
            }
            InternalTarget::LvedNamedPage {
                table,
                name,
                anchor: _,
            } => self.visual_body_for_lved_named_page(&table, &name),
            InternalTarget::MultiviewHref { href, anchor } => {
                self.visual_body_for_multiview_href(&href, anchor.as_deref())
            }
            InternalTarget::HoureiLaw { hore_id, anchor: _ } => {
                self.visual_body_for_hourei_law(&hore_id)
            }
            _ => Ok(VisualBody::Unsupported {
                reason: "body provider deferred".to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "body_deferred",
                    "body provider is not implemented for this target",
                )],
            }),
        }
    }
}

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
            InternalTarget::SsedIosHtmlPage { .. } => Ok(ResolvedTargetKind::InfoPage),
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
            InternalTarget::SsedAuxRecord { source, key, .. }
                if source == super::ssed_sizk_surfaces::SSED_SIZK_SOURCE_ID =>
            {
                self.title_for_ssed_sizk_record(&key)
            }
            InternalTarget::SsedIosHtmlPage {
                source_id, index, ..
            } => Ok(self
                .ssed_ios_html_list_item(&source_id, index)?
                .map(|item| item.label_text)),
            InternalTarget::SsedDenseAnchor {
                anchor,
                resolver_hint,
            } => self.ssed_sidecar_title_for_dense_anchor(&anchor, resolver_hint.as_deref()),
            InternalTarget::SsedAddress {
                component,
                block,
                offset,
            }
            | InternalTarget::SsedIndexAddress {
                component,
                block,
                offset,
                index_component: _,
            }
            | InternalTarget::SsedBoundedAddress {
                component,
                block,
                offset,
                end_block: _,
                end_offset: _,
            } => self.ssed_title_for_address_target(&component, block, offset),
            InternalTarget::LvedRow { table, row_id, .. }
                if table.eq_ignore_ascii_case("content") =>
            {
                let Some(store) = &self.lved_store else {
                    return Ok(None);
                };
                store.content_title_text(row_id)
            }
            InternalTarget::LvedRow { table, row_id, .. } if table.eq_ignore_ascii_case("info") => {
                let Some(store) = &self.lved_store else {
                    return Ok(None);
                };
                store.info_title_text(row_id)
            }
            InternalTarget::LvedInfoPage { name, .. } => {
                let Some(store) = &self.lved_store else {
                    return Ok(None);
                };
                store.info_title_text_by_name(&name)
            }
            InternalTarget::LvedNamedPage { table, name, .. } => {
                let Some(store) = &self.lved_store else {
                    return Ok(None);
                };
                store.named_title_text_by_name(&table, &name)
            }
            InternalTarget::MultiviewHref { href, anchor: _ } => {
                let Some(store) = &self.multiview_store else {
                    return Ok(None);
                };
                Ok(store.body_for_href(&href)?.map(|body| body.title))
            }
            _ => Ok(None),
        }
    }

    fn ssed_sidecar_title_for_dense_anchor(
        &self,
        anchor: &str,
        resolver_hint: Option<&str>,
    ) -> Result<Option<String>> {
        match lookup_ssed_dense_sidecar_body_with_resolvers(
            self.ssed_sidecar_body_resolvers()?,
            anchor,
            resolver_hint,
        )? {
            SsedSidecarLookup::Resolved(body) if !body.title.trim().is_empty() => {
                Ok(Some(body.title))
            }
            _ => Ok(None),
        }
    }

    fn ssed_title_for_address_target(
        &self,
        requested_component: &str,
        block: u32,
        offset: u32,
    ) -> Result<Option<String>> {
        let (block, offset) = self.convert_ios_ssed_address(block, offset)?;
        let Some(catalog) = &self.ssed_catalog else {
            return self.ssed_sidecar_title_for_address(block, offset);
        };
        let Some(component) = catalog
            .component_named(requested_component)
            .or_else(|| catalog.component_for_address(block))
        else {
            return self.ssed_sidecar_title_for_address(block, offset);
        };
        let Some(component_offset) = component.relative_offset(block, offset) else {
            return self.ssed_sidecar_title_for_address(block, offset);
        };
        if component.role == SsedComponentRole::Honmon {
            if let Some(anchor_id) = self.ssed_dense_anchor_at_component_offset(
                component,
                usize::try_from(component_offset).unwrap_or(usize::MAX),
            )? && let Some(title) = self.ssed_sidecar_title_for_dense_anchor(&anchor_id, None)?
            {
                return Ok(Some(title));
            }
            if let Some(title) =
                self.ssed_ordered_honbun_title_at_component_offset(component, component_offset)?
            {
                return Ok(Some(title));
            }
        }
        self.ssed_sidecar_title_for_address(block, offset)
    }

    fn ssed_ordered_honbun_title_at_component_offset(
        &self,
        component: &SsedComponent,
        component_offset: u64,
    ) -> Result<Option<String>> {
        if !self
            .ssed_sidecar_body_resolvers()?
            .iter()
            .any(SsedSidecarBodyResolver::is_ordered_honbun_renderer_body)
        {
            return Ok(None);
        }
        let Some(row_index) =
            self.ssed_entry_slice_row_index_at_component_offset(component, component_offset)?
        else {
            return Ok(None);
        };
        match lookup_ssed_ordered_honbun_body_by_row(
            self.ssed_sidecar_body_resolvers()?,
            row_index,
        )? {
            SsedSidecarLookup::Resolved(body) if !body.title.trim().is_empty() => {
                Ok(Some(body.title))
            }
            _ => Ok(None),
        }
    }

    fn ssed_sidecar_title_for_address(&self, block: u32, offset: u32) -> Result<Option<String>> {
        match lookup_ssed_sidecar_body_by_address_with_resolvers(
            self.ssed_sidecar_body_resolvers()?,
            block,
            offset,
        )? {
            SsedSidecarLookup::Resolved(body) if !body.title.trim().is_empty() => {
                Ok(Some(body.title))
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
            InternalTarget::SsedAuxRecord { source, key, .. }
                if source == super::ssed_sizk_surfaces::SSED_SIZK_SOURCE_ID =>
            {
                self.visual_body_for_ssed_sizk_record(&key)
            }
            InternalTarget::SsedIosHtmlPage {
                source_id, index, ..
            } => self.visual_body_for_ssed_ios_html_page(&source_id, index),
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

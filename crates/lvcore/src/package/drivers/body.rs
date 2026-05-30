use super::*;

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

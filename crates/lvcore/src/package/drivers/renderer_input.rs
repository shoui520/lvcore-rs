use super::*;

impl ReaderBookPackage {
    fn renderer_input_from_visual_body(
        &self,
        target: TargetToken,
        body: VisualBody,
    ) -> Result<RendererInput> {
        match body {
            VisualBody::PreservedHtml { html, source } => Ok(RendererInput::PreservedHtml {
                target,
                html,
                source,
            }),
            VisualBody::SsedStream {
                component,
                offset,
                length,
            } => {
                let (resources, mut diagnostics) =
                    self.ssed_stream_renderer_resources(&component, offset, length)?;
                let hc_profile = hc_renderer_profile(&self.storage)?;
                let profile_hint = hc_profile
                    .as_ref()
                    .map(|profile| profile.profile_id.clone());
                diagnostics.insert(
                    0,
                    Diagnostic::info(
                        "hc_renderer_input_ready",
                        "SSED stream was resolved as input for an HC/profile renderer",
                    ),
                );
                Ok(RendererInput::HcSsedStream {
                    target,
                    component,
                    offset,
                    length,
                    profile_hint,
                    hc_profile,
                    resources,
                    diagnostics,
                })
            }
            VisualBody::SemanticFallback { text } => {
                Ok(RendererInput::SemanticFallback { target, text })
            }
            VisualBody::Unsupported {
                reason,
                diagnostics,
            } => Ok(RendererInput::Unsupported {
                target,
                reason,
                diagnostics,
            }),
        }
    }
}

impl RendererInputProvider for ReaderBookPackage {
    fn renderer_input_for_target(&self, token: &TargetToken) -> Result<RendererInput> {
        let body = self.visual_body_for_target(token)?;
        self.renderer_input_from_visual_body(token.clone(), body)
    }
}

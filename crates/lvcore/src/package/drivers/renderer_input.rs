use super::*;

impl RendererInputProvider for ReaderBookPackage {
    fn renderer_input_for_target(&self, token: &TargetToken) -> Result<RendererInput> {
        let body = self.visual_body_for_target(token)?;
        self.renderer_input_from_visual_body(token.clone(), body)
    }
}

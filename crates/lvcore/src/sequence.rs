use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::error::Result;
use crate::render::{RenderOptions, ResolvedTargetView};
use crate::target::TargetToken;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SequenceHint {
    TitleIndexOrder { value: String },
    SearchResults { value: String },
    BodyOrder,
    MenuOrder { value: String },
    PanelOrder { value: String },
    LvedListOrder,
    LvedTreeOrder,
    HoureiLawArticleOrder,
    MultiviewTreeOrder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetWindow {
    pub center: ResolvedTargetView,
    pub before: Vec<ResolvedTargetView>,
    pub after: Vec<ResolvedTargetView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

pub trait SequenceProvider: Send + Sync {
    fn resolve_target_window(
        &self,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<TargetWindow>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_hint_has_frontend_safe_tagged_json_shape() {
        assert_eq!(
            serde_json::to_value(SequenceHint::TitleIndexOrder {
                value: "title-index".to_owned()
            })
            .unwrap(),
            serde_json::json!({ "kind": "title_index_order", "value": "title-index" })
        );
        assert_eq!(
            serde_json::to_value(SequenceHint::BodyOrder).unwrap(),
            serde_json::json!({ "kind": "body_order" })
        );
        assert_eq!(
            serde_json::from_value::<SequenceHint>(serde_json::json!({
                "kind": "panel_order",
                "value": "01010000"
            }))
            .unwrap(),
            SequenceHint::PanelOrder {
                value: "01010000".to_owned()
            }
        );
    }
}

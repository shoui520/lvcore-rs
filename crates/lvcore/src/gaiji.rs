use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::resources::ResourceRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GaijiSourcePreference {
    Unicode,
    ExternalResource,
    Ga16Bitmap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GaijiPolicy {
    pub priority: Vec<GaijiSourcePreference>,
}

impl Default for GaijiPolicy {
    fn default() -> Self {
        Self {
            priority: vec![
                GaijiSourcePreference::Unicode,
                GaijiSourcePreference::ExternalResource,
                GaijiSourcePreference::Ga16Bitmap,
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GaijiResolution {
    pub identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unicode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceRef>,
    pub nonliteral_marker: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

pub trait GaijiProvider: Send + Sync {
    fn resolve_gaiji(&self, identity: &str, policy: &GaijiPolicy) -> GaijiResolution;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaiji_policy_can_reorder_sources() {
        let policy = GaijiPolicy {
            priority: vec![
                GaijiSourcePreference::Ga16Bitmap,
                GaijiSourcePreference::ExternalResource,
                GaijiSourcePreference::Unicode,
            ],
        };
        assert_eq!(policy.priority[0], GaijiSourcePreference::Ga16Bitmap);
    }
}

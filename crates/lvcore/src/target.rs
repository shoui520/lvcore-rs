use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::error::{Error, Result};
use crate::resources::ResourceToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    SsedAddress,
    SsedDenseAnchor,
    SsedAuxRecord,
    SsedIosHtmlPage,
    LvedRow,
    LvedInfoPage,
    LvedNamedPage,
    LvedCrossBook,
    LvedAddress,
    LvedViewerHook,
    HoureiLaw,
    MultiviewHref,
    MenuItem,
    TocItem,
    TitleIndexItem,
    PanelCell,
    Resource,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InternalTarget {
    SsedAddress {
        component: String,
        block: u32,
        offset: u32,
    },
    SsedBoundedAddress {
        component: String,
        block: u32,
        offset: u32,
        end_block: u32,
        end_offset: u32,
    },
    SsedIndexAddress {
        component: String,
        block: u32,
        offset: u32,
        index_component: String,
    },
    SsedDenseAnchor {
        anchor: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolver_hint: Option<String>,
    },
    SsedAuxRecord {
        source: String,
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<String>,
    },
    SsedIosHtmlPage {
        source_id: String,
        index: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<String>,
    },
    LvedRow {
        table: String,
        row_id: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
    },
    LvedInfoPage {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<String>,
    },
    LvedNamedPage {
        table: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<String>,
    },
    LvedCrossBook {
        link_kind: String,
        dict_code: String,
        content_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<String>,
    },
    LvedAddress {
        block: u32,
        offset: u32,
        raw: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<String>,
    },
    LvedViewerHook {
        hook: String,
        value: String,
    },
    HoureiLaw {
        hore_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<String>,
    },
    MultiviewHref {
        href: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<String>,
    },
    MenuItem {
        surface_id: String,
        item_id: String,
    },
    TocItem {
        surface_id: String,
        item_id: String,
    },
    TitleIndexItem {
        surface_id: String,
        item_id: String,
    },
    PanelCell {
        panel_id: String,
        row: u32,
        column: u32,
    },
    Resource {
        resource: ResourceToken,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<String>,
    },
    Unsupported {
        reason: String,
    },
}

impl InternalTarget {
    pub fn kind(&self) -> TargetKind {
        match self {
            Self::SsedAddress { .. }
            | Self::SsedBoundedAddress { .. }
            | Self::SsedIndexAddress { .. } => TargetKind::SsedAddress,
            Self::SsedDenseAnchor { .. } => TargetKind::SsedDenseAnchor,
            Self::SsedAuxRecord { .. } => TargetKind::SsedAuxRecord,
            Self::SsedIosHtmlPage { .. } => TargetKind::SsedIosHtmlPage,
            Self::LvedRow { .. } => TargetKind::LvedRow,
            Self::LvedInfoPage { .. } => TargetKind::LvedInfoPage,
            Self::LvedNamedPage { .. } => TargetKind::LvedNamedPage,
            Self::LvedCrossBook { .. } => TargetKind::LvedCrossBook,
            Self::LvedAddress { .. } => TargetKind::LvedAddress,
            Self::LvedViewerHook { .. } => TargetKind::LvedViewerHook,
            Self::HoureiLaw { .. } => TargetKind::HoureiLaw,
            Self::MultiviewHref { .. } => TargetKind::MultiviewHref,
            Self::MenuItem { .. } => TargetKind::MenuItem,
            Self::TocItem { .. } => TargetKind::TocItem,
            Self::TitleIndexItem { .. } => TargetKind::TitleIndexItem,
            Self::PanelCell { .. } => TargetKind::PanelCell,
            Self::Resource { .. } => TargetKind::Resource,
            Self::Unsupported { .. } => TargetKind::Unsupported,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
/// Opaque, round-trippable frontend handle for LogoVista targets.
///
/// The token is transport data, not authority. Callers must resolve it through
/// the owning `BookPackage`/`BookLibrary`, because those sinks validate that
/// decoded internals actually exist in the opened book before reading bodies or
/// resources.
pub struct TargetToken(String);

impl TargetToken {
    pub fn from_opaque(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn new(target: &InternalTarget) -> Result<Self> {
        let envelope = TargetEnvelope {
            version: 1,
            target: target.clone(),
        };
        let bytes = serde_json::to_vec(&envelope)?;
        Ok(Self(URL_SAFE_NO_PAD.encode(bytes)))
    }

    pub fn decode(&self) -> Result<InternalTarget> {
        let bytes = URL_SAFE_NO_PAD
            .decode(&self.0)
            .map_err(|_| Error::InvalidTargetToken)?;
        let envelope: TargetEnvelope =
            serde_json::from_slice(&bytes).map_err(|_| Error::InvalidTargetToken)?;
        if envelope.version != 1 {
            return Err(Error::InvalidTargetToken);
        }
        Ok(envelope.target)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn href(&self) -> String {
        format!("lvcore://target/{}", self.0)
    }
}

impl std::fmt::Display for TargetToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<&InternalTarget> for TargetToken {
    type Error = Error;

    fn try_from(value: &InternalTarget) -> Result<Self> {
        Self::new(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TargetEnvelope {
    version: u8,
    target: InternalTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetLink {
    pub token: TargetToken,
    pub href: String,
    pub label: String,
    pub kind: TargetKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
}

impl TargetLink {
    pub fn new(label: impl Into<String>, target: &InternalTarget) -> Result<Self> {
        let token = TargetToken::new(target)?;
        Ok(Self {
            href: token.href(),
            token,
            label: label.into(),
            kind: target.kind(),
            diagnostics: Vec::new(),
            attributes: BTreeMap::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_round_trips_dense_honmon_target() {
        let target = InternalTarget::SsedDenseAnchor {
            anchor: "00100050".to_owned(),
            resolver_hint: Some("vlpljbl".to_owned()),
        };
        let token = TargetToken::new(&target).unwrap();
        assert_eq!(token.decode().unwrap(), target);
        assert!(!token.as_str().contains("00100050"));
        assert_eq!(token.href(), format!("lvcore://target/{}", token.as_str()));
    }
}

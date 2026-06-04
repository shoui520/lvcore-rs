use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::error::Result;
use crate::target::TargetToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodySourceKind {
    HonmonStream,
    HonmonAnchorDereference,
    SidecarHtml,
    SidecarText,
    BritannicaChronologySqlite,
    RendererDatabase,
    LvedSqlite,
    LvlMultiViewSqlite,
    HoureiSqlite,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VisualBody {
    PreservedHtml {
        html: String,
        source: BodySourceKind,
    },
    SsedStream {
        component: String,
        offset: u64,
        length: Option<u64>,
    },
    SemanticFallback {
        text: String,
    },
    Unsupported {
        reason: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        diagnostics: Vec<Diagnostic>,
    },
}

pub trait BodyProvider: Send + Sync {
    fn visual_body_for_target(&self, token: &TargetToken) -> Result<VisualBody>;
}

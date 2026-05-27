use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::error::Result;
use crate::gaiji::GaijiPolicy;
use crate::resources::ResourceRef;
use crate::target::{TargetLink, TargetToken};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderMode {
    Native,
    GenericHtml,
    BasicText,
    Debug,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderOptions {
    pub mode: RenderMode,
    pub gaiji_policy: GaijiPolicy,
    pub include_debug_trace: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            mode: RenderMode::Native,
            gaiji_policy: GaijiPolicy::default(),
            include_debug_trace: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderCapability {
    Html,
    Css,
    Javascript,
    MathJax,
    VerticalText,
    Audio,
    Images,
    Panels,
    DeferredHook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedTargetKind {
    EntryBody,
    NavigationSurface,
    PanelSurface,
    HanreiPage,
    InfoPage,
    LawArticle,
    MediaResource,
    SearchResults,
    Unsupported,
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedTargetView {
    pub kind: ResolvedTargetKind,
    pub target: TargetToken,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_html: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic_text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<ResourceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<TargetLink>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<RenderCapability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug_trace: Option<String>,
}

impl ResolvedTargetView {
    pub fn deferred(target: TargetToken, title: impl Into<String>, diagnostic: Diagnostic) -> Self {
        Self {
            kind: ResolvedTargetKind::Deferred,
            target,
            title: Some(title.into()),
            display_html: None,
            basic_text: None,
            resources: Vec::new(),
            links: Vec::new(),
            capabilities: Vec::new(),
            diagnostics: vec![diagnostic],
            debug_trace: None,
        }
    }

    pub fn unsupported(
        target: TargetToken,
        title: impl Into<String>,
        diagnostic: Diagnostic,
    ) -> Self {
        Self {
            kind: ResolvedTargetKind::Unsupported,
            target,
            title: Some(title.into()),
            display_html: None,
            basic_text: None,
            resources: Vec::new(),
            links: Vec::new(),
            capabilities: Vec::new(),
            diagnostics: vec![diagnostic],
            debug_trace: None,
        }
    }
}

pub trait RendererProvider: Send + Sync {
    fn render_target(
        &self,
        token: &TargetToken,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView>;
}

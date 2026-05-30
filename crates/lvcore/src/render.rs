use serde::{Deserialize, Serialize};

use crate::body::BodySourceKind;
use crate::diagnostics::Diagnostic;
use crate::error::Result;
use crate::gaiji::GaijiPolicy;
use crate::navigation::NavigationSurface;
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
    Video,
    Images,
    Panels,
    HcRenderInput,
    DeferredHook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RendererInputKind {
    HcSsedStream,
    PreservedHtml,
    SemanticFallback,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HcRendererProfileSource {
    HcDll,
    ExinfoHtmlDll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HcRendererProfileStatus {
    InputOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HcRendererProfile {
    pub profile_id: String,
    pub source: HcRendererProfileSource,
    pub status: HcRendererProfileStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dll_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dll_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RendererInput {
    HcSsedStream {
        target: TargetToken,
        component: String,
        offset: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        length: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile_hint: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hc_profile: Option<HcRendererProfile>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        resources: Vec<ResourceRef>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        diagnostics: Vec<Diagnostic>,
    },
    PreservedHtml {
        target: TargetToken,
        html: String,
        source: BodySourceKind,
    },
    SemanticFallback {
        target: TargetToken,
        text: String,
    },
    Unsupported {
        target: TargetToken,
        reason: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        diagnostics: Vec<Diagnostic>,
    },
}

impl RendererInput {
    pub fn kind(&self) -> RendererInputKind {
        match self {
            Self::HcSsedStream { .. } => RendererInputKind::HcSsedStream,
            Self::PreservedHtml { .. } => RendererInputKind::PreservedHtml,
            Self::SemanticFallback { .. } => RendererInputKind::SemanticFallback,
            Self::Unsupported { .. } => RendererInputKind::Unsupported,
        }
    }

    pub fn target(&self) -> &TargetToken {
        match self {
            Self::HcSsedStream { target, .. }
            | Self::PreservedHtml { target, .. }
            | Self::SemanticFallback { target, .. }
            | Self::Unsupported { target, .. } => target,
        }
    }
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
    /// Reader-ready HTML after lvcore has normalized package links/resources.
    ///
    /// Package content is still dictionary-authored HTML. Dedicated reader
    /// frontends should render it inside a constrained webview/document sandbox,
    /// not mix it into privileged application chrome.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_html: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_anchor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<NavigationSurface>,
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
            scroll_anchor: None,
            surface: None,
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
            scroll_anchor: None,
            surface: None,
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

pub trait RendererInputProvider: Send + Sync {
    fn renderer_input_for_target(&self, token: &TargetToken) -> Result<RendererInput>;
}

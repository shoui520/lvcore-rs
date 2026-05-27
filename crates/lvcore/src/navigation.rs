use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::error::Result;
use crate::target::TargetToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationSurfaceKind {
    Menu,
    Toc,
    TitleIndexBrowse,
    Panel,
    Hanrei,
    Info,
    SearchFallback,
    LawTree,
    MultiviewTree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationStatus {
    Available,
    Unsupported,
    Missing,
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeSurface {
    pub surface_id: String,
    pub kind: NavigationSurfaceKind,
    pub status: NavigationStatus,
    pub title_html: String,
    pub title_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetToken>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NavigationSurface {
    SimpleMenu {
        surface_id: String,
        nodes: Vec<NavigationNode>,
    },
    TitleIndexBrowse {
        surface_id: String,
        items: Vec<NavigationItem>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_cursor: Option<String>,
    },
    Panel {
        surface_id: String,
        cells: Vec<PanelCell>,
    },
    HierarchicalTree {
        surface_id: String,
        nodes: Vec<NavigationNode>,
    },
    InfoPages {
        surface_id: String,
        pages: Vec<NavigationItem>,
    },
    FallbackSearch {
        surface_id: String,
    },
    Deferred {
        surface_id: String,
        diagnostics: Vec<Diagnostic>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationNode {
    pub node_id: String,
    pub label_html: String,
    pub label_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetToken>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NavigationNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationItem {
    pub item_id: String,
    pub label_html: String,
    pub label_text: String,
    pub target: TargetToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelCell {
    pub panel_id: String,
    pub row: u32,
    pub column: u32,
    pub label_html: String,
    pub label_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetToken>,
}

pub trait NavigationProvider: Send + Sync {
    fn home_surfaces(&self) -> Result<Vec<HomeSurface>>;
    fn open_surface(&self, surface_id: &str) -> Result<NavigationSurface>;
}

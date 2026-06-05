use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::diagnostics::Diagnostic;
use crate::error::Result;
use crate::gaiji::GaijiPolicy;
use crate::resources::ResourceRef;
use crate::sequence::SequenceHint;
use crate::target::TargetToken;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelOptions {
    #[serde(default)]
    pub gaiji_policy: GaijiPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationSurfaceKind {
    Menu,
    ScreenMenu,
    EncyclopediaIndex,
    AuxiliaryIndex,
    Toc,
    TitleIndexBrowse,
    MultiSelector,
    Panel,
    Hanrei,
    Info,
    SearchFallback,
    LvedTree,
    LawTree,
    MultiviewTree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationStatus {
    Available,
    Unsupported,
    Missing,
    Empty,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NavigationSurface {
    SimpleMenu {
        surface_id: String,
        nodes: Vec<NavigationNode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_cursor: Option<String>,
    },
    ScreenMenu {
        surface_id: String,
        screens: Vec<ScreenMenuScreen>,
        stats: BTreeMap<String, u32>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        diagnostics: Vec<Diagnostic>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_cursor: Option<String>,
    },
    HierarchicalTree {
        surface_id: String,
        nodes: Vec<NavigationNode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_cursor: Option<String>,
    },
    InfoPages {
        surface_id: String,
        pages: Vec<NavigationItem>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_cursor: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NavigationNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenMenuScreen {
    pub screen_id: String,
    pub screen_index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hotspots: Vec<ScreenMenuHotspot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenMenuHotspot {
    pub hotspot_id: String,
    pub rect: ScreenMenuRect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetToken>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenMenuRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationItem {
    pub item_id: String,
    pub label_html: String,
    pub label_text: String,
    pub target: TargetToken,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub href: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationTarget {
    pub surface_id: String,
    pub source_id: String,
    pub label_html: String,
    pub label_text: String,
    pub target: TargetToken,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub href: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence_hint: Option<SequenceHint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

impl NavigationSurface {
    pub fn surface_id(&self) -> &str {
        match self {
            Self::SimpleMenu { surface_id, .. }
            | Self::ScreenMenu { surface_id, .. }
            | Self::TitleIndexBrowse { surface_id, .. }
            | Self::Panel { surface_id, .. }
            | Self::HierarchicalTree { surface_id, .. }
            | Self::InfoPages { surface_id, .. }
            | Self::FallbackSearch { surface_id }
            | Self::Deferred { surface_id, .. } => surface_id,
        }
    }

    pub fn is_deferred(&self) -> bool {
        matches!(self, Self::Deferred { .. })
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::Deferred { diagnostics, .. } => diagnostics,
            Self::ScreenMenu { diagnostics, .. } => diagnostics,
            _ => &[],
        }
    }

    pub fn sequence_hint(&self) -> Option<SequenceHint> {
        match self {
            Self::TitleIndexBrowse { surface_id, .. } if surface_id == "lved-list" => {
                Some(SequenceHint::LvedListOrder)
            }
            Self::TitleIndexBrowse { surface_id, .. } => Some(SequenceHint::TitleIndexOrder {
                value: surface_id.clone(),
                cursor: None,
            }),
            Self::SimpleMenu { surface_id, .. } => Some(SequenceHint::MenuOrder {
                value: surface_id.clone(),
                cursor: None,
            }),
            Self::HierarchicalTree { surface_id, .. } if surface_id == "lved-tree" => {
                Some(SequenceHint::LvedTreeOrder)
            }
            Self::HierarchicalTree { surface_id, .. } if surface_id == "menuData" => {
                Some(SequenceHint::MultiviewTreeOrder)
            }
            Self::HierarchicalTree { surface_id, .. } if surface_id == "law-tree" => {
                Some(SequenceHint::HoureiLawArticleOrder)
            }
            Self::HierarchicalTree { surface_id, .. } => Some(SequenceHint::MenuOrder {
                value: surface_id.clone(),
                cursor: None,
            }),
            Self::Panel { surface_id, .. } => Some(SequenceHint::PanelOrder {
                value: surface_id.clone(),
            }),
            Self::ScreenMenu { .. }
            | Self::InfoPages { .. }
            | Self::FallbackSearch { .. }
            | Self::Deferred { .. } => None,
        }
    }

    pub fn actionable_targets(&self) -> Vec<NavigationTarget> {
        let sequence_hint = self.sequence_hint();
        match self {
            Self::SimpleMenu {
                surface_id, nodes, ..
            }
            | Self::HierarchicalTree {
                surface_id, nodes, ..
            } => {
                let mut targets = Vec::new();
                collect_node_targets(surface_id, nodes, sequence_hint.as_ref(), &mut targets);
                targets
            }
            Self::ScreenMenu {
                surface_id,
                screens,
                ..
            } => screens
                .iter()
                .flat_map(|screen| {
                    screen.hotspots.iter().filter_map(|hotspot| {
                        hotspot.target.as_ref().map(|target| NavigationTarget {
                            href: target.href(),
                            surface_id: surface_id.clone(),
                            source_id: format!("{}:{}", screen.screen_id, hotspot.hotspot_id),
                            label_html: hotspot.hotspot_id.clone(),
                            label_text: hotspot.hotspot_id.clone(),
                            target: target.clone(),
                            sequence_hint: sequence_hint.clone(),
                            diagnostics: hotspot.diagnostics.clone(),
                        })
                    })
                })
                .collect(),
            Self::TitleIndexBrowse {
                surface_id, items, ..
            } => items
                .iter()
                .map(|item| NavigationTarget {
                    href: item.target.href(),
                    surface_id: surface_id.clone(),
                    source_id: item.item_id.clone(),
                    label_html: item.label_html.clone(),
                    label_text: item.label_text.clone(),
                    target: item.target.clone(),
                    sequence_hint: title_index_item_sequence_hint(
                        sequence_hint.as_ref(),
                        &item.item_id,
                    ),
                    diagnostics: item.diagnostics.clone(),
                })
                .collect(),
            Self::Panel {
                surface_id, cells, ..
            } => cells
                .iter()
                .filter_map(|cell| {
                    cell.target.as_ref().map(|target| NavigationTarget {
                        href: target.href(),
                        surface_id: surface_id.clone(),
                        source_id: format!("{}:{}:{}", cell.panel_id, cell.row, cell.column),
                        label_html: cell.label_html.clone(),
                        label_text: cell.label_text.clone(),
                        target: target.clone(),
                        sequence_hint: sequence_hint.clone(),
                        diagnostics: cell.diagnostics.clone(),
                    })
                })
                .collect(),
            Self::InfoPages {
                surface_id, pages, ..
            } => pages
                .iter()
                .map(|page| NavigationTarget {
                    href: page.target.href(),
                    surface_id: surface_id.clone(),
                    source_id: page.item_id.clone(),
                    label_html: page.label_html.clone(),
                    label_text: page.label_text.clone(),
                    target: page.target.clone(),
                    sequence_hint: sequence_hint.clone(),
                    diagnostics: page.diagnostics.clone(),
                })
                .collect(),
            Self::FallbackSearch { .. } | Self::Deferred { .. } => Vec::new(),
        }
    }

    pub fn has_actionable_targets(&self) -> bool {
        match self {
            Self::SimpleMenu { nodes, .. } | Self::HierarchicalTree { nodes, .. } => {
                nodes_have_target(nodes)
            }
            Self::ScreenMenu { screens, .. } => screens.iter().any(|screen| {
                screen
                    .hotspots
                    .iter()
                    .any(|hotspot| hotspot.target.is_some())
            }),
            Self::TitleIndexBrowse { items, .. } => !items.is_empty(),
            Self::Panel { cells, .. } => cells.iter().any(|cell| cell.target.is_some()),
            Self::InfoPages { pages, .. } => !pages.is_empty(),
            Self::FallbackSearch { .. } | Self::Deferred { .. } => false,
        }
    }
}

pub trait NavigationProvider: Send + Sync {
    fn home_surfaces(&self) -> Result<Vec<HomeSurface>>;
    fn open_surface(&self, surface_id: &str) -> Result<NavigationSurface>;
    fn open_surface_page(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        let _ = cursor;
        let _ = limit;
        self.open_surface(surface_id)
    }

    fn open_surface_with_options(
        &self,
        surface_id: &str,
        options: &LabelOptions,
    ) -> Result<NavigationSurface> {
        let _ = options;
        self.open_surface(surface_id)
    }

    fn open_surface_page_with_options(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
        options: &LabelOptions,
    ) -> Result<NavigationSurface> {
        let _ = options;
        self.open_surface_page(surface_id, cursor, limit)
    }
}

fn collect_node_targets(
    surface_id: &str,
    nodes: &[NavigationNode],
    sequence_hint: Option<&SequenceHint>,
    targets: &mut Vec<NavigationTarget>,
) {
    for node in nodes {
        if let Some(target) = &node.target {
            let sequence_hint = node_sequence_hint(sequence_hint, &node.node_id);
            targets.push(NavigationTarget {
                href: target.href(),
                surface_id: surface_id.to_owned(),
                source_id: node.node_id.clone(),
                label_html: node.label_html.clone(),
                label_text: node.label_text.clone(),
                target: target.clone(),
                sequence_hint,
                diagnostics: node.diagnostics.clone(),
            });
        }
        collect_node_targets(surface_id, &node.children, sequence_hint, targets);
    }
}

fn node_sequence_hint(sequence_hint: Option<&SequenceHint>, node_id: &str) -> Option<SequenceHint> {
    match sequence_hint {
        Some(SequenceHint::MenuOrder { value, .. }) => Some(SequenceHint::MenuOrder {
            value: value.clone(),
            cursor: ssed_menu_node_cursor(node_id),
        }),
        Some(hint) => Some(hint.clone()),
        None => None,
    }
}

fn title_index_item_sequence_hint(
    sequence_hint: Option<&SequenceHint>,
    item_id: &str,
) -> Option<SequenceHint> {
    match sequence_hint {
        Some(SequenceHint::TitleIndexOrder { value, .. }) => Some(SequenceHint::TitleIndexOrder {
            value: value.clone(),
            cursor: Some(item_id.to_owned()),
        }),
        Some(hint) => Some(hint.clone()),
        None => None,
    }
}

fn ssed_menu_node_cursor(node_id: &str) -> Option<String> {
    let rest = node_id.strip_prefix("ssed-menu:")?;
    let mut parts = rest.split(':');
    let record = parts.next()?.parse::<usize>().ok()?;
    match (parts.next(), parts.next(), parts.next()) {
        (None, None, None) => Some(record.to_string()),
        (Some("link"), Some(link), None) => {
            let link = link.parse::<usize>().ok()?;
            Some(format!("link:{record}:{link}"))
        }
        _ => None,
    }
}

fn nodes_have_target(nodes: &[NavigationNode]) -> bool {
    nodes
        .iter()
        .any(|node| node.target.is_some() || nodes_have_target(&node.children))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::{InternalTarget, TargetToken};

    fn token(label: &str) -> TargetToken {
        TargetToken::new(&InternalTarget::Unsupported {
            reason: label.to_owned(),
        })
        .unwrap()
    }

    #[test]
    fn navigation_surface_actionable_targets_skip_targetless_folders() {
        let surface = NavigationSurface::HierarchicalTree {
            surface_id: "menuData".to_owned(),
            nodes: vec![NavigationNode {
                href: None,
                node_id: "root".to_owned(),
                label_html: "Root".to_owned(),
                label_text: "Root".to_owned(),
                target: None,
                diagnostics: Vec::new(),
                children: vec![NavigationNode {
                    href: None,
                    node_id: "child".to_owned(),
                    label_html: "Child".to_owned(),
                    label_text: "Child".to_owned(),
                    target: Some(token("child")),
                    diagnostics: Vec::new(),
                    children: Vec::new(),
                }],
            }],
            next_cursor: None,
        };

        let targets = surface.actionable_targets();
        assert_eq!(surface.surface_id(), "menuData");
        assert!(surface.has_actionable_targets());
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].href, targets[0].target.href());
        assert_eq!(targets[0].source_id, "child");
        assert_eq!(targets[0].label_text, "Child");
        assert_eq!(
            targets[0].sequence_hint,
            Some(SequenceHint::MultiviewTreeOrder)
        );
    }

    #[test]
    fn navigation_surfaces_expose_backend_sequence_hints() {
        let lved = NavigationSurface::TitleIndexBrowse {
            surface_id: "lved-list".to_owned(),
            items: vec![NavigationItem {
                href: String::new(),
                item_id: "1".to_owned(),
                label_html: "alpha".to_owned(),
                label_text: "alpha".to_owned(),
                target: token("alpha"),
                diagnostics: Vec::new(),
            }],
            next_cursor: None,
        };
        assert_eq!(lved.sequence_hint(), Some(SequenceHint::LvedListOrder));
        assert_eq!(
            lved.actionable_targets()[0].sequence_hint,
            Some(SequenceHint::LvedListOrder)
        );
        assert_eq!(
            lved.actionable_targets()[0].href,
            lved.actionable_targets()[0].target.href()
        );

        let title = NavigationSurface::TitleIndexBrowse {
            surface_id: "FHTITLE".to_owned(),
            items: Vec::new(),
            next_cursor: None,
        };
        assert_eq!(
            title.sequence_hint(),
            Some(SequenceHint::TitleIndexOrder {
                value: "FHTITLE".to_owned(),
                cursor: None
            })
        );

        let menu = NavigationSurface::SimpleMenu {
            surface_id: "menu".to_owned(),
            nodes: Vec::new(),
            next_cursor: None,
        };
        assert_eq!(
            menu.sequence_hint(),
            Some(SequenceHint::MenuOrder {
                value: "menu".to_owned(),
                cursor: None
            })
        );

        let panel = NavigationSurface::Panel {
            surface_id: "panels:01010000".to_owned(),
            cells: Vec::new(),
            next_cursor: None,
        };
        assert_eq!(
            panel.sequence_hint(),
            Some(SequenceHint::PanelOrder {
                value: "panels:01010000".to_owned()
            })
        );
    }

    #[test]
    fn deferred_navigation_surface_is_diagnostic_only() {
        let surface = NavigationSurface::Deferred {
            surface_id: "menu".to_owned(),
            diagnostics: vec![Diagnostic::info("navigation_deferred", "not decoded")],
        };

        assert!(surface.is_deferred());
        assert!(!surface.has_actionable_targets());
        assert_eq!(surface.diagnostics()[0].code, "navigation_deferred");
        assert!(surface.actionable_targets().is_empty());
    }
}

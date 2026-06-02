use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::error::{Error, Result};
use crate::navigation::{HomeSurface, NavigationNode, NavigationSurface};
use crate::package::BookId;
use crate::render::{RendererInput, ResolvedTargetView};
use crate::resources::{ResourceRef, ResourceToken};
use crate::search::SearchPage;
use crate::sequence::TargetWindow;
use crate::target::TargetToken;

pub(super) fn scope_target_window_resource_hrefs(
    book_id: &BookId,
    mut window: TargetWindow,
) -> TargetWindow {
    scope_view_resource_hrefs(book_id, &mut window.center);
    for view in &mut window.before {
        scope_view_resource_hrefs(book_id, view);
    }
    for view in &mut window.after {
        scope_view_resource_hrefs(book_id, view);
    }
    window
}

pub(super) fn scope_home_surfaces_resource_hrefs(book_id: &BookId, surfaces: &mut [HomeSurface]) {
    for surface in surfaces {
        if let Some(target) = &surface.target {
            surface.href = Some(target.href());
        }
        surface.title_html = scope_resource_hrefs_in_html(book_id, &surface.title_html);
    }
}

pub(super) fn scope_navigation_surface_resource_hrefs(
    book_id: &BookId,
    surface: &mut NavigationSurface,
) {
    match surface {
        NavigationSurface::SimpleMenu { nodes, .. }
        | NavigationSurface::HierarchicalTree { nodes, .. } => {
            scope_navigation_node_resource_hrefs(book_id, nodes);
        }
        NavigationSurface::TitleIndexBrowse { items, .. } => {
            for item in items {
                item.href = item.target.href();
                item.label_html = scope_resource_hrefs_in_html(book_id, &item.label_html);
            }
        }
        NavigationSurface::Panel { cells, .. } => {
            for cell in cells {
                if let Some(target) = &cell.target {
                    cell.href = Some(target.href());
                }
                cell.label_html = scope_resource_hrefs_in_html(book_id, &cell.label_html);
            }
        }
        NavigationSurface::ScreenMenu { screens, .. } => {
            for screen in screens {
                if let Some(resource) = &mut screen.background {
                    scope_resource_ref_href(book_id, resource);
                }
                for hotspot in &mut screen.hotspots {
                    if let Some(target) = &hotspot.target {
                        hotspot.href = Some(target.href());
                    }
                }
            }
        }
        NavigationSurface::InfoPages { pages, .. } => {
            for page in pages {
                page.href = page.target.href();
                page.label_html = scope_resource_hrefs_in_html(book_id, &page.label_html);
            }
        }
        NavigationSurface::FallbackSearch { .. } | NavigationSurface::Deferred { .. } => {}
    }
}

fn scope_navigation_node_resource_hrefs(book_id: &BookId, nodes: &mut [NavigationNode]) {
    for node in nodes {
        if let Some(target) = &node.target {
            node.href = Some(target.href());
        }
        node.label_html = scope_resource_hrefs_in_html(book_id, &node.label_html);
        scope_navigation_node_resource_hrefs(book_id, &mut node.children);
    }
}

pub(super) fn scope_view_resource_hrefs(book_id: &BookId, view: &mut ResolvedTargetView) {
    view.href = view.target.href();
    if let Some(surface) = &mut view.surface {
        scope_navigation_surface_resource_hrefs(book_id, surface);
    }
    let Some(display_html) = &mut view.display_html else {
        for resource in &mut view.resources {
            scope_resource_ref_href(book_id, resource);
        }
        return;
    };
    for resource in &mut view.resources {
        scope_resource_ref_href(book_id, resource);
    }
    *display_html = scope_resource_hrefs_in_html(book_id, display_html);
}

pub(super) fn scope_renderer_input_resource_hrefs(book_id: &BookId, input: &mut RendererInput) {
    if let RendererInput::HcSsedStream { resources, .. } = input {
        for resource in resources {
            scope_resource_ref_href(book_id, resource);
        }
    }
}

pub(super) fn scope_resource_ref_href(book_id: &BookId, resource: &mut ResourceRef) {
    if resource.href.is_some() {
        resource.href = Some(scoped_resource_href(book_id, &resource.token));
    }
}

fn scoped_resource_href(book_id: &BookId, token: &ResourceToken) -> String {
    format!(
        "lvcore://resource/{}/{}",
        URL_SAFE_NO_PAD.encode(book_id.0.as_bytes()),
        token.as_str()
    )
}

pub(super) fn scope_search_page_resource_hrefs(book_id: &BookId, page: &mut SearchPage) {
    for hit in &mut page.hits {
        hit.href = hit.target.href();
        hit.title_html = scope_resource_hrefs_in_html(book_id, &hit.title_html);
        if let Some(snippet_html) = &mut hit.snippet_html {
            *snippet_html = scope_resource_hrefs_in_html(book_id, snippet_html);
        }
    }
}

fn scope_resource_hrefs_in_html(book_id: &BookId, html: &str) -> String {
    const PREFIX: &str = "lvcore://resource/";
    let mut output = String::with_capacity(html.len());
    let mut cursor = 0;
    while let Some(relative_start) = html[cursor..].find(PREFIX) {
        let start = cursor + relative_start;
        output.push_str(&html[cursor..start]);
        output.push_str(PREFIX);
        let value_start = start + PREFIX.len();
        let value_end = html[value_start..]
            .find(is_resource_href_delimiter)
            .map(|offset| value_start + offset)
            .unwrap_or(html.len());
        let value = &html[value_start..value_end];
        if value.is_empty() || value.contains('/') {
            output.push_str(value);
        } else {
            output.push_str(&URL_SAFE_NO_PAD.encode(book_id.0.as_bytes()));
            output.push('/');
            output.push_str(value);
        }
        cursor = value_end;
    }
    output.push_str(&html[cursor..]);
    output
}

fn is_resource_href_delimiter(value: char) -> bool {
    value.is_whitespace() || matches!(value, '"' | '\'' | '<' | '>' | ')' | '(' | '?' | '#')
}

pub(super) fn parse_scoped_resource_href(href: &str) -> Result<(BookId, ResourceToken)> {
    let Some(rest) = href.strip_prefix("lvcore://resource/") else {
        return Err(Error::InvalidResourceHref);
    };
    let rest = rest
        .split_once(['?', '#'])
        .map(|(target, _)| target)
        .unwrap_or(rest);
    let mut parts = rest.split('/');
    let Some(book_scope) = parts.next().filter(|value| !value.is_empty()) else {
        return Err(Error::InvalidResourceHref);
    };
    let Some(resource_token) = parts.next().filter(|value| !value.is_empty()) else {
        return Err(Error::InvalidResourceHref);
    };
    if parts.next().is_some() {
        return Err(Error::InvalidResourceHref);
    }
    let book_id_bytes = URL_SAFE_NO_PAD
        .decode(book_scope)
        .map_err(|_| Error::InvalidResourceHref)?;
    let book_id = String::from_utf8(book_id_bytes).map_err(|_| Error::InvalidResourceHref)?;
    Ok((BookId(book_id), ResourceToken::from_opaque(resource_token)))
}

pub(super) fn parse_target_href(href: &str) -> Result<TargetToken> {
    let Some(rest) = href.strip_prefix("lvcore://target/") else {
        return Err(Error::InvalidTargetHref);
    };
    let token = rest
        .split_once(['?', '#'])
        .map(|(target, _)| target)
        .unwrap_or(rest);
    if token.is_empty() || token.contains('/') {
        return Err(Error::InvalidTargetHref);
    }
    let token = TargetToken::from_opaque(token);
    token.decode()?;
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Diagnostic;
    use crate::navigation::{
        HomeSurface, NavigationItem, NavigationNode, NavigationStatus, NavigationSurface,
        NavigationSurfaceKind, PanelCell, ScreenMenuHotspot, ScreenMenuRect, ScreenMenuScreen,
    };
    use crate::render::{ResolvedTargetKind, ResolvedTargetView};
    use crate::resources::ResourceKind;
    use crate::search::{SearchHit, SearchPage};
    use crate::target::InternalTarget;

    fn target(label: &str) -> TargetToken {
        TargetToken::new(&InternalTarget::Unsupported {
            reason: label.to_owned(),
        })
        .unwrap()
    }

    #[test]
    fn scopes_resource_href_without_swallowing_query_or_fragment() {
        let scoped = scope_resource_hrefs_in_html(
            &BookId("DAIJIRN4".to_owned()),
            r#"<img src="lvcore://resource/token123?variant=small#fig1">"#,
        );
        assert!(scoped.contains("lvcore://resource/REFJSklSTjQ/token123?variant=small#fig1"));
    }

    #[test]
    fn parses_scoped_resource_href_with_query_or_fragment_suffix() {
        let (book_id, token) =
            parse_scoped_resource_href("lvcore://resource/REFJSklSTjQ/token123?variant=small#fig1")
                .unwrap();
        assert_eq!(book_id, BookId("DAIJIRN4".to_owned()));
        assert_eq!(token, ResourceToken::from_opaque("token123"));

        let (book_id, token) =
            parse_scoped_resource_href("lvcore://resource/REFJSklSTjQ/token123#fig1").unwrap();
        assert_eq!(book_id, BookId("DAIJIRN4".to_owned()));
        assert_eq!(token, ResourceToken::from_opaque("token123"));
    }

    #[test]
    fn parses_target_href_with_query_or_fragment_suffix() {
        let token = TargetToken::new(&crate::target::InternalTarget::Unsupported {
            reason: "target".to_owned(),
        })
        .unwrap();
        let parsed =
            parse_target_href(&format!("lvcore://target/{}?x=1#frag", token.as_str())).unwrap();
        assert_eq!(parsed, token);
    }

    #[test]
    fn rejects_invalid_target_hrefs() {
        assert!(matches!(
            parse_target_href("lvcore://resource/book/token"),
            Err(Error::InvalidTargetHref)
        ));
        assert!(matches!(
            parse_target_href("lvcore://target/"),
            Err(Error::InvalidTargetHref)
        ));
        assert!(matches!(
            parse_target_href("lvcore://target/not/a/token"),
            Err(Error::InvalidTargetHref)
        ));
    }

    #[test]
    fn scopes_hc_renderer_input_resource_refs_without_touching_target() {
        let target = crate::target::TargetToken::from_opaque("target-token");
        let resource = ResourceToken::from_opaque("resource-token");
        let mut input = RendererInput::HcSsedStream {
            target: target.clone(),
            component: "HONMON.DIC".to_owned(),
            offset: 0,
            length: Some(16),
            profile_hint: Some("HC0158".to_owned()),
            hc_profile: None,
            resources: vec![ResourceRef {
                token: resource,
                kind: ResourceKind::Image,
                label: None,
                href: Some("lvcore://resource/resource-token".to_owned()),
                mime_type: Some("image/svg+xml".to_owned()),
                diagnostics: Vec::new(),
            }],
            diagnostics: Vec::new(),
        };

        scope_renderer_input_resource_hrefs(&BookId("SSED:ARCHSIC4".to_owned()), &mut input);

        let RendererInput::HcSsedStream {
            target: scoped_target,
            resources,
            ..
        } = input
        else {
            panic!("input kind should not change");
        };
        assert_eq!(scoped_target, target);
        assert_eq!(
            resources[0].href.as_deref(),
            Some("lvcore://resource/U1NFRDpBUkNIU0lDNA/resource-token")
        );
    }

    #[test]
    fn populates_public_target_hrefs_for_search_hits() {
        let target = target("search-hit");
        let mut page = SearchPage {
            hits: vec![SearchHit {
                href: String::new(),
                book_id: BookId("SSED:TEST".to_owned()),
                target: target.clone(),
                title_html: "見出し".to_owned(),
                title_text: "見出し".to_owned(),
                snippet_html: None,
                diagnostics: Vec::new(),
            }],
            next_cursor: None,
            result_sequence: None,
            diagnostics: Vec::new(),
        };

        scope_search_page_resource_hrefs(&BookId("SSED:TEST".to_owned()), &mut page);

        assert_eq!(page.hits[0].href, target.href());
    }

    #[test]
    fn populates_public_target_hrefs_for_home_and_navigation_surfaces() {
        let home_target = target("home");
        let node_target = target("node");
        let item_target = target("item");
        let cell_target = target("cell");
        let hotspot_target = target("hotspot");
        let href_only_home = "lvcore://target/home-href-only?surface=menu".to_owned();
        let href_only_node = "lvcore://target/node-href-only?source=backend".to_owned();
        let href_only_cell = "lvcore://target/cell-href-only?source=backend".to_owned();
        let href_only_hotspot = "lvcore://target/hotspot-href-only?source=backend".to_owned();
        let mut homes = vec![
            HomeSurface {
                href: None,
                surface_id: "menu".to_owned(),
                kind: NavigationSurfaceKind::Menu,
                status: NavigationStatus::Available,
                title_html: "Menu".to_owned(),
                title_text: "Menu".to_owned(),
                target: Some(home_target.clone()),
                diagnostics: Vec::new(),
            },
            HomeSurface {
                href: Some(href_only_home.clone()),
                surface_id: "href-only-menu".to_owned(),
                kind: NavigationSurfaceKind::Menu,
                status: NavigationStatus::Available,
                title_html: "Href Menu".to_owned(),
                title_text: "Href Menu".to_owned(),
                target: None,
                diagnostics: Vec::new(),
            },
        ];
        let mut menu = NavigationSurface::SimpleMenu {
            surface_id: "menu".to_owned(),
            nodes: vec![NavigationNode {
                href: None,
                node_id: "node".to_owned(),
                label_html: "Node".to_owned(),
                label_text: "Node".to_owned(),
                target: Some(node_target.clone()),
                diagnostics: Vec::new(),
                children: vec![NavigationNode {
                    href: Some(href_only_node.clone()),
                    node_id: "href-only-node".to_owned(),
                    label_html: "Href Node".to_owned(),
                    label_text: "Href Node".to_owned(),
                    target: None,
                    diagnostics: Vec::new(),
                    children: Vec::new(),
                }],
            }],
            next_cursor: None,
        };
        let mut browse = NavigationSurface::TitleIndexBrowse {
            surface_id: "title".to_owned(),
            items: vec![NavigationItem {
                href: String::new(),
                item_id: "item".to_owned(),
                label_html: "Item".to_owned(),
                label_text: "Item".to_owned(),
                target: item_target.clone(),
                diagnostics: Vec::new(),
            }],
            next_cursor: None,
        };
        let mut panel = NavigationSurface::Panel {
            surface_id: "panel".to_owned(),
            cells: vec![
                PanelCell {
                    href: None,
                    panel_id: "panel".to_owned(),
                    row: 1,
                    column: 2,
                    label_html: "Cell".to_owned(),
                    label_text: "Cell".to_owned(),
                    target: Some(cell_target.clone()),
                    diagnostics: Vec::new(),
                },
                PanelCell {
                    href: Some(href_only_cell.clone()),
                    panel_id: "panel".to_owned(),
                    row: 2,
                    column: 3,
                    label_html: "Href Cell".to_owned(),
                    label_text: "Href Cell".to_owned(),
                    target: None,
                    diagnostics: Vec::new(),
                },
            ],
        };
        let mut screen = NavigationSurface::ScreenMenu {
            surface_id: "screen".to_owned(),
            screens: vec![ScreenMenuScreen {
                screen_id: "screen-1".to_owned(),
                screen_index: 0,
                width: None,
                height: None,
                background: None,
                hotspots: vec![
                    ScreenMenuHotspot {
                        href: None,
                        hotspot_id: "hotspot".to_owned(),
                        rect: ScreenMenuRect {
                            x: 0,
                            y: 0,
                            width: 10,
                            height: 10,
                        },
                        target: Some(hotspot_target.clone()),
                        target_kind: None,
                        diagnostics: Vec::new(),
                    },
                    ScreenMenuHotspot {
                        href: Some(href_only_hotspot.clone()),
                        hotspot_id: "href-only-hotspot".to_owned(),
                        rect: ScreenMenuRect {
                            x: 1,
                            y: 1,
                            width: 10,
                            height: 10,
                        },
                        target: None,
                        target_kind: None,
                        diagnostics: Vec::new(),
                    },
                ],
                diagnostics: Vec::new(),
            }],
            stats: Default::default(),
            diagnostics: Vec::new(),
        };

        let book_id = BookId("SSED:TEST".to_owned());
        scope_home_surfaces_resource_hrefs(&book_id, &mut homes);
        scope_navigation_surface_resource_hrefs(&book_id, &mut menu);
        scope_navigation_surface_resource_hrefs(&book_id, &mut browse);
        scope_navigation_surface_resource_hrefs(&book_id, &mut panel);
        scope_navigation_surface_resource_hrefs(&book_id, &mut screen);

        assert_eq!(homes[0].href.as_deref(), Some(home_target.href().as_str()));
        assert_eq!(homes[1].href.as_deref(), Some(href_only_home.as_str()));
        let NavigationSurface::SimpleMenu { nodes, .. } = menu else {
            panic!("expected menu");
        };
        assert_eq!(nodes[0].href.as_deref(), Some(node_target.href().as_str()));
        assert_eq!(
            nodes[0].children[0].href.as_deref(),
            Some(href_only_node.as_str())
        );
        let NavigationSurface::TitleIndexBrowse { items, .. } = browse else {
            panic!("expected title browse");
        };
        assert_eq!(items[0].href, item_target.href());
        let NavigationSurface::Panel { cells, .. } = panel else {
            panic!("expected panel");
        };
        assert_eq!(cells[0].href.as_deref(), Some(cell_target.href().as_str()));
        assert_eq!(cells[1].href.as_deref(), Some(href_only_cell.as_str()));
        let NavigationSurface::ScreenMenu { screens, .. } = screen else {
            panic!("expected screen menu");
        };
        assert_eq!(
            screens[0].hotspots[0].href.as_deref(),
            Some(hotspot_target.href().as_str())
        );
        assert_eq!(
            screens[0].hotspots[1].href.as_deref(),
            Some(href_only_hotspot.as_str())
        );
    }

    #[test]
    fn populates_public_target_href_for_resolved_views() {
        let target = target("view");
        let mut view = ResolvedTargetView {
            href: String::new(),
            kind: ResolvedTargetKind::EntryBody,
            target: target.clone(),
            title: Some("Entry".to_owned()),
            display_html: None,
            basic_text: None,
            scroll_anchor: None,
            surface: None,
            resources: Vec::new(),
            links: Vec::new(),
            capabilities: Vec::new(),
            diagnostics: vec![Diagnostic::info("test", "test")],
            debug_trace: None,
        };

        scope_view_resource_hrefs(&BookId("SSED:TEST".to_owned()), &mut view);

        assert_eq!(view.href, target.href());
    }
}

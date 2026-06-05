use super::common::*;

#[test]
fn multiview_book_id_uses_package_code_without_windows_folder_wrapper() {
    let dir = tempdir().unwrap();
    let package_root = dir.path().join("_DCT_MOROKU26");
    fs::create_dir_all(&package_root).unwrap();
    fs::write(
        package_root.join("menuData.xml"),
        r#"<list><item label="模範六法" href=""/></list>"#,
    )
    .unwrap();
    fs::write(package_root.join("blvdat"), b"payload").unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    let metadata = package.metadata();

    assert_eq!(metadata.format_family, FormatFamily::LvlMultiView);
    assert!(
        metadata.book_id.0.starts_with("LVLMultiView:MOROKU26:"),
        "{}",
        metadata.book_id.0
    );
    assert!(
        !metadata.book_id.0.contains("_DCT_"),
        "{}",
        metadata.book_id.0
    );
}

#[test]
fn multiview_preserved_html_packages_do_not_advertise_deferred_rendering() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("menuData.xml"),
        r#"<list><item label="Book"><item label="前" href="000001" /></item></list>"#,
    )
    .unwrap();
    write_minimal_multiview_content_fixture(&dir.path().join("blvdat"));

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let metadata = package.metadata();

    assert_eq!(metadata.format_family, FormatFamily::LvlMultiView);
    assert!(metadata.capabilities.contains(&Capability::PreservedHtml));
    assert!(
        !metadata
            .capabilities
            .contains(&Capability::DeferredRendering),
        "LVLMultiView bodies are preserved HTML inputs; deferred rendering is a diagnostic state, not a positive format capability"
    );
}

#[test]
fn multiview_law_navigation_capability_is_payload_based() {
    let simple = tempdir().unwrap();
    fs::write(
        simple.path().join("menuData.xml"),
        r#"<list><item label="Book"><item label="前" href="000001" /></item></list>"#,
    )
    .unwrap();
    write_minimal_multiview_content_fixture(&simple.path().join("blvdat"));
    let simple_package = DriverRegistry::default().open_best(simple.path()).unwrap();
    assert_eq!(
        simple_package.metadata().format_family,
        FormatFamily::LvlMultiView
    );
    assert!(
        !simple_package
            .metadata()
            .capabilities
            .contains(&Capability::LawNavigation),
        "non-law MultiView dictionaries must not look like law books to the frontend"
    );

    let law = tempdir().unwrap();
    write_minimal_multiview_law_fixture(law.path());
    fs::write(
        law.path().join("menuData.xml"),
        r#"<list><item label="法令"><item label="五十音順法令一覧" href="list:kana:み" /></item></list>"#,
    )
    .unwrap();
    let law_package = DriverRegistry::default().open_best(law.path()).unwrap();
    assert_eq!(
        law_package.metadata().format_family,
        FormatFamily::LvlMultiView
    );
    assert!(
        law_package
            .metadata()
            .capabilities
            .contains(&Capability::LawNavigation),
        "law MultiView payloads should advertise law navigation"
    );
}

#[test]
fn multiview_menu_data_opens_as_hierarchical_tree() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("menuData.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
        <list>
          <item label="模範六法" href="">
            <item label="憲法編" href="">
              <item genre="A1" index="" label="日本国憲法" href="A010">
                <item label="前文部" href="A010_ZEN" anchor="top"></item>
              </item>
            </item>
          </item>
        </list>"#,
    )
    .unwrap();
    fs::write(dir.path().join("blvdat"), b"payload").unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let surface = package.open_surface("menuData").unwrap();
    let lvcore::NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
        panic!("menuData should open as a MultiView tree");
    };

    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].label_text, "模範六法");
    assert!(nodes[0].target.is_none());
    let law = &nodes[0].children[0].children[0];
    assert_eq!(law.label_text, "日本国憲法");
    assert_eq!(
        law.target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::MultiviewHref {
            href: "A010".to_owned(),
            anchor: None,
        }
    );
    let preface = &law.children[0];
    assert_eq!(
        preface.target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::MultiviewHref {
            href: "A010_ZEN".to_owned(),
            anchor: Some("top".to_owned()),
        }
    );
}

#[test]
fn multiview_menu_and_search_targets_resolve_to_preserved_body_html() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("menuData.xml"),
        r#"<list><item label="Book"><item label="前" href="000001" /><item label="中" href="000002" /><item label="後" href="000003" /></item></list>"#,
    )
    .unwrap();
    fs::create_dir(dir.path().join("Templates")).unwrap();
    fs::write(dir.path().join("Templates/pic.png"), b"png").unwrap();
    write_minimal_multiview_content_fixture(&dir.path().join("blvdat"));

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let surface = package.open_surface("menuData").unwrap();
    let lvcore::NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
        panic!("menuData should open as a MultiView tree");
    };
    let target = nodes[0].children[0].target.clone().unwrap();
    let view = package
        .render_target(&target, &RenderOptions::default())
        .unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::EntryBody);
    let html = view.display_html.as_deref().unwrap();
    assert!(html.contains("<article><h1>まえがき</h1><p>body</p>"));
    assert!(html.contains(r#"<a href="lvcore://target/"#));
    assert!(html.contains(r#"<img src="lvcore://resource/"#));
    assert!(html.contains(r#"src="javascript:bad()""#));
    assert!(html.contains(r#"data="file:///tmp/outside.bin""#));
    assert!(html.contains(r#"src="lvcore://resource/already-normalized""#));
    assert_eq!(view.links.len(), 1);
    assert_eq!(view.resources.len(), 1);
    assert!(
        view.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "resource_missing")
    );
    let basic_view = package
        .render_target(
            &target,
            &RenderOptions {
                mode: RenderMode::BasicText,
                ..RenderOptions::default()
            },
        )
        .unwrap();
    assert!(basic_view.display_html.is_none());
    assert_eq!(
        basic_view.basic_text.as_deref(),
        Some("まえがき\nbody\nnext")
    );
    assert!(basic_view.resources.is_empty());
    assert!(basic_view.links.is_empty());

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "まえ".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "まえがき");
    assert_eq!(page.hits[0].href, page.hits[0].target.href());
    assert_eq!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::MultiviewHref {
            href: "000001".to_owned(),
            anchor: None,
        }
    );
    let input = package
        .renderer_input_for_target(&page.hits[0].target)
        .unwrap();
    let RendererInput::PreservedHtml { source, html, .. } = input else {
        panic!("MultiView body must stay preserved HTML before rendering normalization");
    };
    assert_eq!(source, BodySourceKind::LvlMultiViewSqlite);
    assert!(html.contains("<article><h1>まえがき</h1><p>body</p>"));

    let first = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Partial,
            query: "body".to_owned(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(first.hits[0].title_text, "まえがき");
    assert_eq!(first.next_cursor.as_deref(), Some("1"));
    let second = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Partial,
            query: "body".to_owned(),
            cursor: first.next_cursor,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(second.hits[0].title_text, "本文");
    assert!(second.next_cursor.is_none());

    let middle = nodes[0].children[1].target.clone().unwrap();
    let window = package
        .resolve_target_window(&middle, None, 1, 1, &RenderOptions::default())
        .unwrap();
    assert_eq!(window.before.len(), 1);
    assert_eq!(window.after.len(), 1);
    assert!(
        window.before[0]
            .display_html
            .as_deref()
            .unwrap()
            .contains("<h1>まえがき</h1>")
    );
    assert!(
        window.after[0]
            .display_html
            .as_deref()
            .unwrap()
            .contains("<h1>あとがき</h1>")
    );
}

#[test]
fn multiview_search_labels_are_sanitized_for_app_chrome() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("menuData.xml"),
        r#"<list><item label="Book"><item label="前" href="000001" /></item></list>"#,
    )
    .unwrap();
    write_minimal_multiview_content_fixture(&dir.path().join("blvdat"));
    {
        let connection = Connection::open(dir.path().join("blvdat")).unwrap();
        connection
            .execute(
                "update t_search set f_TitleMain = ?1, f_All = ?2 where f_No = 1",
                (
                    r#"<b>まえがき</b><script>alert(1)</script><img src="javascript:bad()" onerror="bad()">"#,
                    r#"<em>snippet</em><script>alert(1)</script><span class="hostile lvcore-subtitle">safe subtitle</span>"#,
                ),
            )
            .unwrap();
    }

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "まえ".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    let hit = &page.hits[0];
    assert!(hit.title_html.contains("<b>まえがき</b>"));
    assert!(!hit.title_html.contains("<script"));
    assert!(!hit.title_html.contains("javascript:"));
    assert!(!hit.title_html.contains("onerror"));
    let snippet = hit.snippet_html.as_deref().unwrap();
    assert!(snippet.contains("<em>snippet</em>"));
    assert!(snippet.contains(r#"<span class="lvcore-subtitle">safe subtitle</span>"#));
    assert!(!snippet.contains("<script"));
    assert!(!snippet.contains("hostile"));
}

#[test]
fn multiview_law_list_targets_resolve_to_navigation_and_law_bodies() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("menuData.xml"),
        r#"<list>
          <item label="模範六法" href="">
            <item label="五十音順法令一覧" href="50on" />
            <item label="◎日本国憲法" href="111S21K1" />
          </item>
        </list>"#,
    )
    .unwrap();
    write_minimal_multiview_law_fixture(dir.path());

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let surface = package.open_surface("menuData").unwrap();
    let NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
        panic!("menuData should open as a MultiView tree");
    };
    let list_target = nodes[0].children[0].target.clone().unwrap();
    let list_view = package
        .render_target(&list_target, &RenderOptions::default())
        .unwrap();
    assert_eq!(list_view.kind, ResolvedTargetKind::NavigationSurface);
    let NavigationSurface::TitleIndexBrowse { items, .. } = list_view.surface.as_ref().unwrap()
    else {
        panic!("50on should resolve to a law title/index browse surface");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].item_id, "111S21K1");
    assert_eq!(items[0].label_text, "日本国憲法 (にほんこくけんぽう)");

    let actionable = list_view.surface.as_ref().unwrap().actionable_targets();
    assert_eq!(actionable.len(), 2);
    assert_eq!(
        actionable[0].sequence_hint,
        Some(lvcore::SequenceHint::TitleIndexOrder {
            value: "multiview:50on".to_owned(),
            cursor: Some("111S21K1".to_owned()),
        })
    );
    let hinted_window = package
        .resolve_target_window(
            &actionable[0].target,
            actionable[0].sequence_hint.as_ref(),
            0,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(hinted_window.center.kind, ResolvedTargetKind::EntryBody);
    assert_eq!(hinted_window.after.len(), 1);
    assert_eq!(
        hinted_window.after[0].title.as_deref(),
        Some("民法 (みんぽう)")
    );
    assert!(
        hinted_window
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "sequence_deferred")
    );

    let law_view = package
        .render_target(&items[0].target, &RenderOptions::default())
        .unwrap();
    assert_eq!(law_view.kind, ResolvedTargetKind::EntryBody);
    assert!(
        law_view
            .display_html
            .as_deref()
            .unwrap()
            .contains("日本国憲法本文")
    );

    let direct_law_view = package
        .render_target(
            nodes[0].children[1].target.as_ref().unwrap(),
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(direct_law_view.kind, ResolvedTargetKind::EntryBody);
    assert!(
        direct_law_view
            .display_html
            .as_deref()
            .unwrap()
            .contains("日本国憲法本文")
    );

    let window = package
        .resolve_target_window(
            &items[0].target,
            Some(&lvcore::SequenceHint::MultiviewTreeOrder),
            1,
            0,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(window.center.kind, ResolvedTargetKind::EntryBody);
    assert_eq!(window.before.len(), 1);
    assert_eq!(window.before[0].kind, ResolvedTargetKind::NavigationSurface);
    assert!(
        window
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "sequence_deferred")
    );

    let body_order_window = package
        .resolve_target_window(
            &items[0].target,
            Some(&lvcore::SequenceHint::BodyOrder),
            1,
            0,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(body_order_window.center.kind, ResolvedTargetKind::EntryBody);
    assert_eq!(body_order_window.before.len(), 1);
    assert!(
        body_order_window
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "sequence_deferred")
    );
}

#[test]
fn hourei_book_id_uses_stable_product_identity_not_folder_name() {
    let dir = tempdir().unwrap();
    let package_root = dir.path().join("user_named_hourei_folder");
    fs::create_dir_all(&package_root).unwrap();
    write_minimal_hourei_fixture(&package_root);

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    let metadata = package.metadata();

    assert_eq!(metadata.format_family, FormatFamily::Hourei);
    assert!(
        metadata
            .book_id
            .0
            .starts_with("Hourei:LOGOVISTA_HOUREI_PROFESSIONAL:"),
        "{}",
        metadata.book_id.0
    );
    assert!(
        !metadata.book_id.0.contains("user_named_hourei_folder"),
        "{}",
        metadata.book_id.0
    );
}

#[test]
fn hourei_preserved_html_packages_do_not_advertise_deferred_rendering() {
    let dir = tempdir().unwrap();
    write_minimal_hourei_fixture(dir.path());

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let metadata = package.metadata();

    assert_eq!(metadata.format_family, FormatFamily::Hourei);
    assert!(metadata.capabilities.contains(&Capability::PreservedHtml));
    assert!(
        !metadata
            .capabilities
            .contains(&Capability::DeferredRendering),
        "Hourei cached/law bodies are preserved HTML inputs; deferred rendering is a diagnostic state, not a positive format capability"
    );
}

#[test]
fn hourei_law_tree_search_body_links_and_sequence_are_backend_owned() {
    let dir = tempdir().unwrap();
    write_minimal_hourei_fixture(dir.path());

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert_eq!(package.metadata().format_family, FormatFamily::Hourei);
    let surfaces = package.home_surfaces().unwrap();
    assert!(surfaces.iter().any(|surface| {
        surface.surface_id == "kana-panel"
            && surface.kind == NavigationSurfaceKind::Panel
            && surface.status == NavigationStatus::Available
            && surface.target.is_some()
    }));
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::LawTree
            && surface.status == NavigationStatus::Available
            && surface.target.is_some()
    }));
    for surface in surfaces.iter().filter(|surface| surface.target.is_some()) {
        let expected_href = surface.target.as_ref().unwrap().href();
        assert_eq!(surface.href.as_deref(), Some(expected_href.as_str()));
    }

    let kana_surface = package.open_surface("kana-panel").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = kana_surface else {
        panic!("Hourei kana panel should open as a panel surface");
    };
    assert_eq!(cells[0].label_text, "");
    assert!(cells[0].target.is_none());
    assert_eq!(cells[1].label_text, "み");
    let kana_panel_target = cells[1].target.as_ref().unwrap().clone();
    let kana_panel_href = kana_panel_target.href();
    assert_eq!(cells[1].href.as_deref(), Some(kana_panel_href.as_str()));
    assert_eq!(
        kana_panel_target.decode().unwrap(),
        InternalTarget::MenuItem {
            surface_id: "hourei-kana:み".to_owned(),
            item_id: "root".to_owned(),
        }
    );
    let kana_browse = package
        .open_surface_page("hourei-kana:み", None, 10)
        .unwrap();
    let kana_actionable = kana_browse.actionable_targets();
    assert_eq!(kana_actionable.len(), 1);
    assert_eq!(
        kana_actionable[0].sequence_hint,
        Some(lvcore::SequenceHint::TitleIndexOrder {
            value: "hourei-kana:み".to_owned(),
            cursor: Some("law:401000000000000001".to_owned()),
        })
    );
    let lvcore::NavigationSurface::TitleIndexBrowse { items, .. } = &kana_browse else {
        panic!("Hourei kana initial should open as a title/index browse surface");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label_text, "民法");
    assert_eq!(items[0].href, items[0].target.href());

    let surface = package.open_surface("law-tree").unwrap();
    let lvcore::NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
        panic!("Hourei law tree should open as a hierarchical tree");
    };
    assert_eq!(nodes[0].label_text, "民事");
    assert_eq!(nodes[0].children.len(), 2);
    assert_eq!(nodes[0].children[0].label_text, "民法");
    let node_href = nodes[0].children[0].target.as_ref().unwrap().href();
    assert_eq!(
        nodes[0].children[0].href.as_deref(),
        Some(node_href.as_str())
    );
    assert_eq!(
        nodes[0].children[0]
            .target
            .as_ref()
            .unwrap()
            .decode()
            .unwrap(),
        InternalTarget::HoureiLaw {
            hore_id: "401000000000000001".to_owned(),
            anchor: None,
        }
    );

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "民".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "民法");
    assert_eq!(page.hits[0].href, page.hits[0].target.href());

    let body_only_forward = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "本文".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert!(body_only_forward.hits.is_empty());

    let body_only_partial = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Partial,
            query: "本文".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert!(body_only_partial.hits.is_empty());

    let body_fulltext = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::FullText,
            query: "本文".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(body_fulltext.hits.len(), 2);
    assert!(body_fulltext.result_sequence.is_some());
    assert!(matches!(
        body_fulltext.hits[0].sequence_hint,
        Some(lvcore::SequenceHint::SearchResults { .. })
    ));
    let package_search_window = package
        .resolve_target_window(
            &body_fulltext.hits[0].target,
            body_fulltext.hits[0].sequence_hint.as_ref(),
            0,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(package_search_window.center.title.as_deref(), Some("民法"));
    assert_eq!(package_search_window.after.len(), 1);
    assert_eq!(
        package_search_window.after[0].title.as_deref(),
        Some("商法")
    );
    assert!(
        package_search_window
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "sequence_deferred")
    );

    let first = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::FullText,
            query: "本文".to_owned(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(first.hits[0].title_text, "民法");
    assert_eq!(first.next_cursor.as_deref(), Some("1"));
    let second = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::FullText,
            query: "本文".to_owned(),
            cursor: first.next_cursor,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(second.hits[0].title_text, "商法");
    assert!(second.next_cursor.is_none());

    let view = package
        .render_target(&page.hits[0].target, &RenderOptions::default())
        .unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::LawArticle);
    assert_eq!(view.title.as_deref(), Some("民法"));
    let html = view.display_html.as_deref().unwrap();
    assert!(html.contains("<div class=\"header\">民法</div>"));
    assert!(html.contains("lvcore://target/"));
    assert!(html.contains("lvcore://resource/"));
    assert!(!html.contains("lved_ref&1:"));
    assert!(!html.contains("lved_ref:み"));
    assert_eq!(view.links.len(), 2);
    assert_eq!(
        view.links[0].token.decode().unwrap(),
        InternalTarget::HoureiLaw {
            hore_id: "401000000000000002".to_owned(),
            anchor: Some("A2".to_owned()),
        }
    );
    assert_eq!(
        view.links[1].token.decode().unwrap(),
        InternalTarget::MenuItem {
            surface_id: "hourei-kana:み".to_owned(),
            item_id: "root".to_owned(),
        }
    );
    let linked_kana_view = package
        .render_target(&view.links[1].token, &RenderOptions::default())
        .unwrap();
    assert_eq!(linked_kana_view.kind, ResolvedTargetKind::NavigationSurface);
    assert!(matches!(
        linked_kana_view.surface,
        Some(lvcore::NavigationSurface::TitleIndexBrowse { .. })
    ));
    let kana_window = package
        .resolve_target_window(
            &kana_panel_target,
            Some(&lvcore::SequenceHint::PanelOrder {
                value: "kana-panel".to_owned(),
            }),
            0,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(
        kana_window.center.kind,
        ResolvedTargetKind::NavigationSurface
    );
    assert_eq!(kana_window.center.title.as_deref(), Some("み"));
    assert_eq!(kana_window.after.len(), 1);
    assert_eq!(kana_window.after[0].title.as_deref(), Some("し"));
    assert!(
        kana_window
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "sequence_deferred")
    );
    assert_eq!(view.resources.len(), 1);
    assert_eq!(view.resources[0].kind, ResourceKind::Image);
    assert_eq!(
        package.read_resource(&view.resources[0].token).unwrap(),
        b"png".to_vec()
    );

    let window = package
        .resolve_target_window(
            &page.hits[0].target,
            Some(&lvcore::SequenceHint::HoureiLawArticleOrder),
            0,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(window.after.len(), 1);
    assert!(
        window.after[0]
            .display_html
            .as_deref()
            .unwrap()
            .contains("商法本文")
    );

    let body_order = package
        .resolve_target_window(
            &page.hits[0].target,
            Some(&lvcore::SequenceHint::BodyOrder),
            0,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(body_order.after.len(), 1);
    assert!(
        body_order
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "sequence_deferred")
    );

    let kana_entry_window = package
        .resolve_target_window(
            &kana_actionable[0].target,
            kana_actionable[0].sequence_hint.as_ref(),
            0,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(
        kana_entry_window.center.kind,
        ResolvedTargetKind::LawArticle
    );
    assert_eq!(kana_entry_window.center.title.as_deref(), Some("民法"));
    assert_eq!(kana_entry_window.after.len(), 1);
    assert_eq!(kana_entry_window.after[0].title.as_deref(), Some("商法"));
    assert!(
        kana_entry_window
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "sequence_deferred")
    );
}

#[test]
fn hourei_rejects_traversal_law_target_ids_before_body_lookup() {
    let root = tempdir().unwrap();
    let package_root = root.path().join("book");
    fs::create_dir_all(&package_root).unwrap();
    write_minimal_hourei_fixture(&package_root);
    fs::write(root.path().join("escape_H.html"), "<div>outside</div>").unwrap();
    fs::write(root.path().join("escape.db"), b"not a package db").unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    for hore_id in ["../../../../escape", "401/../../../../escape"] {
        let token = TargetToken::new(&InternalTarget::HoureiLaw {
            hore_id: hore_id.to_owned(),
            anchor: None,
        })
        .unwrap();
        let view = package
            .render_target(&token, &RenderOptions::default())
            .unwrap();
        assert_eq!(view.kind, ResolvedTargetKind::Unsupported);
        assert!(view.display_html.is_none());
        assert!(!view.display_html.unwrap_or_default().contains("outside"));
    }
}

#[test]
fn hourei_rejects_numeric_law_target_ids_not_owned_by_book() {
    let root = tempdir().unwrap();
    let package_root = root.path().join("book");
    fs::create_dir_all(&package_root).unwrap();
    write_minimal_hourei_fixture(&package_root);

    let unknown_id = "401000000000000999";
    fs::write(
        package_root
            .join("_DataBase")
            .join("HTMLs/H")
            .join(format!("{unknown_id}_H.html")),
        "<div>unknown cached body</div>",
    )
    .unwrap();
    let unknown_db = package_root
        .join("_DataBase")
        .join("H01")
        .join(format!("{unknown_id}.db"));
    let connection = Connection::open(&unknown_db).unwrap();
    connection
        .execute_batch(
            "create table t_page (f_rec_id integer primary key, f_text text);
             insert into t_page values (1, '<div>unknown shard body</div>');",
        )
        .unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    let token = TargetToken::new(&InternalTarget::HoureiLaw {
        hore_id: unknown_id.to_owned(),
        anchor: None,
    })
    .unwrap();
    let view = package
        .render_target(&token, &RenderOptions::default())
        .unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::Unsupported);
    assert!(view.display_html.is_none());
    assert!(
        view.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "hourei_law_missing")
    );
}

#[cfg(unix)]
#[test]
fn hourei_rejects_cached_law_html_symlink_escape() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    let package_root = root.path().join("book");
    fs::create_dir_all(&package_root).unwrap();
    write_minimal_hourei_fixture(&package_root);
    let outside = root.path().join("outside.html");
    fs::write(&outside, "<div>outside</div>").unwrap();
    fs::remove_file(
        package_root
            .join("_DataBase")
            .join("HTMLs/H/401000000000000001_H.html"),
    )
    .unwrap();
    symlink(
        &outside,
        package_root
            .join("_DataBase")
            .join("HTMLs/H/401000000000000001_H.html"),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    let token = TargetToken::new(&InternalTarget::HoureiLaw {
        hore_id: "401000000000000001".to_owned(),
        anchor: None,
    })
    .unwrap();
    let view = package
        .render_target(&token, &RenderOptions::default())
        .unwrap();
    assert!(view.display_html.is_none());
    assert!(
        view.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "hourei_law_missing")
    );
    assert!(!view.display_html.unwrap_or_default().contains("outside"));
}

#[cfg(unix)]
#[test]
fn hourei_resource_search_skips_symlinked_directories() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    let package_root = root.path().join("book");
    fs::create_dir_all(&package_root).unwrap();
    write_minimal_hourei_fixture(&package_root);
    let outside = root.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("law.png"), b"outside").unwrap();
    fs::remove_file(package_root.join("_DataBase/image/law.png")).unwrap();
    symlink(&outside, package_root.join("_DataBase/image/linked")).unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    let token = TargetToken::new(&InternalTarget::HoureiLaw {
        hore_id: "401000000000000001".to_owned(),
        anchor: None,
    })
    .unwrap();
    let view = package
        .render_target(&token, &RenderOptions::default())
        .unwrap();
    assert!(
        view.display_html
            .as_deref()
            .unwrap()
            .contains("lvcore://resource/")
    );
    assert_eq!(view.resources.len(), 1);
    assert!(view.resources[0].href.is_none());
    assert!(
        view.resources[0]
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "resource_missing")
    );
}

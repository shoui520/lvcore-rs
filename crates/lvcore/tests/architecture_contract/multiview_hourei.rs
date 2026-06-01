use super::common::*;

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
    assert_eq!(view.links.len(), 1);
    assert_eq!(view.resources.len(), 1);
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
fn hourei_law_tree_search_body_links_and_sequence_are_backend_owned() {
    let dir = tempdir().unwrap();
    write_minimal_hourei_fixture(dir.path());

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert_eq!(package.metadata().format_family, FormatFamily::Hourei);
    let surfaces = package.home_surfaces().unwrap();
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::LawTree
            && surface.status == NavigationStatus::Available
            && surface.target.is_some()
    }));

    let surface = package.open_surface("law-tree").unwrap();
    let lvcore::NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
        panic!("Hourei law tree should open as a hierarchical tree");
    };
    assert_eq!(nodes[0].label_text, "民事");
    assert_eq!(nodes[0].children.len(), 2);
    assert_eq!(nodes[0].children[0].label_text, "民法");
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

    let first = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Partial,
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
            mode: SearchMode::Partial,
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
    assert_eq!(view.links.len(), 1);
    assert_eq!(
        view.links[0].token.decode().unwrap(),
        InternalTarget::HoureiLaw {
            hore_id: "401000000000000002".to_owned(),
            anchor: Some("A2".to_owned()),
        }
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

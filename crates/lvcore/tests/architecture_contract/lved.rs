use super::common::*;

#[test]
fn lved_list_surface_is_cursor_paged_by_backend() {
    let dir = tempdir().unwrap();
    write_minimal_lved_sqlite_fixture(dir.path());
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let first = package.open_surface_page("lved-list", None, 1).unwrap();
    let NavigationSurface::TitleIndexBrowse {
        items, next_cursor, ..
    } = first
    else {
        panic!("expected paged LVED list surface");
    };
    assert_eq!(items[0].label_text, "alpha");
    assert_eq!(next_cursor.as_deref(), Some("1"));

    let second = package
        .open_surface_page("lved-list", next_cursor.as_deref(), 1)
        .unwrap();
    let NavigationSurface::TitleIndexBrowse {
        items, next_cursor, ..
    } = second
    else {
        panic!("expected second LVED list page");
    };
    assert_eq!(items[0].label_text, "beta");
    assert!(next_cursor.is_none());
}

#[test]
fn lved_search_is_cursor_paged_by_backend() {
    let dir = tempdir().unwrap();
    write_minimal_lved_sqlite_fixture(dir.path());
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let first = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "shared".to_owned(),
            cursor: None,
            limit: 1,
        })
        .unwrap();
    assert_eq!(first.hits[0].title_text, "alpha");
    assert_eq!(first.next_cursor.as_deref(), Some("1"));

    let second = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "shared".to_owned(),
            cursor: first.next_cursor,
            limit: 1,
        })
        .unwrap();
    assert_eq!(second.hits[0].title_text, "beta");
    assert!(second.next_cursor.is_none());
}

#[test]
fn lved_advanced_search_mode_uses_named_search_column() {
    let dir = tempdir().unwrap();
    write_minimal_lved_sqlite_fixture(dir.path());
    {
        let connection = Connection::open(dir.path().join("main.data")).unwrap();
        connection.pragma_update(None, "key", "test-key").unwrap();
        connection
            .pragma_update(None, "cipher_compatibility", 4)
            .unwrap();
        connection
            .execute(
                "update search set advanced1 = 'domain marker' where rowid = 1",
                [],
            )
            .unwrap();
    }
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert_eq!(
        package.metadata().search_modes,
        vec![
            SearchMode::Exact,
            SearchMode::Forward,
            SearchMode::Backward,
            SearchMode::Partial,
            SearchMode::FullText,
            SearchMode::Advanced("advanced1".to_owned()),
            SearchMode::Advanced("advanced2".to_owned()),
        ]
    );

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Advanced("advanced1".to_owned()),
            query: "domain".to_owned(),
            cursor: None,
            limit: 10,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "alpha");
    assert!(page.diagnostics.is_empty());
}

#[test]
fn lved_tree_idx_opens_as_navigation_tree_and_targets_content_rows() {
    let dir = tempdir().unwrap();
    write_minimal_lved_sqlite_fixture(dir.path());

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert_eq!(
        package.metadata().title.as_deref(),
        Some("Example Dictionary 第2版")
    );
    let surfaces = package.home_surfaces().unwrap();
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::LvedTree
            && surface.status == NavigationStatus::Available
            && surface.target.is_some()
    }));

    let surface = package.open_surface("lved-tree").unwrap();
    let lvcore::NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
        panic!("LVED tree.idx should open as a hierarchical tree");
    };
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].label_text, "Example Dictionary");
    assert!(nodes[0].target.is_none());
    assert_eq!(nodes[0].children[0].label_text, "Browse");
    assert!(nodes[0].children[0].target.is_none());
    let alpha = &nodes[0].children[0].children[0];
    assert_eq!(alpha.label_text, "Alpha");
    assert_eq!(
        alpha.target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::LvedRow {
            table: "content".to_owned(),
            row_id: 100,
            anchor: None,
            query: None,
        }
    );

    let view = package
        .render_target(alpha.target.as_ref().unwrap(), &RenderOptions::default())
        .unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::EntryBody);
    assert!(
        view.display_html
            .as_deref()
            .unwrap()
            .contains("<article><h1>Alpha</h1>")
    );
    let window = package
        .resolve_target_window(
            alpha.target.as_ref().unwrap(),
            Some(&lvcore::SequenceHint::LvedTreeOrder),
            0,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(window.center.title.as_deref(), Some("Alpha"));
    assert_eq!(window.after.len(), 1);
    assert_eq!(window.after[0].title.as_deref(), Some("Beta"));

    let body_order = package
        .resolve_target_window(
            alpha.target.as_ref().unwrap(),
            Some(&lvcore::SequenceHint::BodyOrder),
            0,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(body_order.center.title.as_deref(), Some("alpha"));
    assert_eq!(body_order.after.len(), 1);
    assert_eq!(body_order.after[0].title.as_deref(), Some("beta"));
    assert!(
        body_order
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "sequence_deferred")
    );

    let info_surface = package.open_surface("info").unwrap();
    let NavigationSurface::InfoPages { pages, .. } = info_surface else {
        panic!("LVED info should open as info pages");
    };
    let null_id_page = pages
        .iter()
        .find(|page| page.item_id == "null-id.html")
        .expect("expected NULL-id info row to use rowid-backed target");
    let null_id_view = package
        .render_target(&null_id_page.target, &RenderOptions::default())
        .unwrap();
    assert_eq!(null_id_view.kind, ResolvedTargetKind::InfoPage);
    assert_eq!(
        null_id_view.display_html.as_deref(),
        Some("<h1>Null id info</h1>")
    );
}

#[test]
fn lved_retained_product_idx_opens_as_navigation_tree() {
    let dir = tempdir().unwrap();
    write_minimal_lved_sqlite_fixture(dir.path());
    fs::remove_file(dir.path().join("res/tree.idx")).unwrap();
    fs::write(
        dir.path().join("res/ibio5_2.idx"),
        "\u{feff}-127\t0\tBiology Table\r\n102?key=visible\t1\tGamma\r\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("000002C5.idx"),
        b"00000000\t0000ffff\t\tRank A\r\n",
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert_eq!(
        package.metadata().title.as_deref(),
        Some("Example Dictionary 第2版")
    );
    assert!(package.home_surfaces().unwrap().iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::LvedTree
            && surface.status == NavigationStatus::Available
    }));
    let surface = package.open_surface("lved-tree").unwrap();
    let NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
        panic!("LVED retained product .idx should open as a hierarchical tree");
    };
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].label_text, "Biology Table");
    let gamma = &nodes[0].children[0];
    assert_eq!(gamma.label_text, "Gamma");
    assert_eq!(
        gamma.target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::LvedRow {
            table: "content".to_owned(),
            row_id: 102,
            anchor: None,
            query: Some("key=visible".to_owned()),
        }
    );
    assert!(nodes.iter().all(|node| node.label_text != "Rank A"));
}

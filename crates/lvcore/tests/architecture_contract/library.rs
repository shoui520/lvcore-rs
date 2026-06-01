use super::common::*;

#[test]
fn library_opens_discovered_package_roots_for_frontend_library_import() {
    let root = tempdir().unwrap();
    let first = root.path().join("FirstBook");
    let second = root.path().join("SecondBook");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    write_minimal_lved_sqlite_fixture(&first);
    write_minimal_lved_sqlite_fixture(&second);

    let registry = DriverRegistry::default();
    let mut library = BookLibrary::new();
    let opened = library
        .open_discovered_paths(
            [root.path()],
            &registry,
            PackageDiscoveryOptions { max: Some(1) },
        )
        .unwrap();

    assert_eq!(opened.len(), 1);
    assert_eq!(library.len(), 1);
    assert_eq!(library.metadata_snapshot().len(), 1);
}

#[test]
fn library_tolerant_import_reports_opened_books_without_aborting() {
    let root = tempdir().unwrap();
    let first = root.path().join("FirstBook");
    let second = root.path().join("SecondBook");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    write_minimal_lved_sqlite_fixture(&first);
    write_minimal_lved_sqlite_fixture(&second);

    let registry = DriverRegistry::default();
    let mut library = BookLibrary::new();
    let report = library.try_open_discovered_paths(
        [root.path()],
        &registry,
        PackageDiscoveryOptions::default(),
    );

    assert_eq!(report.opened.len(), 2);
    assert!(report.diagnostics.is_empty());
    assert_eq!(library.len(), 2);

    let import_result = library.import_result(report);
    assert_eq!(import_result.book_count, 2);
    assert_eq!(import_result.books.len(), 2);
    assert_eq!(import_result.opened_book_ids.len(), 2);
    assert!(import_result.import_diagnostics.is_empty());
}

#[test]
fn library_import_deduplicates_identical_book_ids() {
    let root = tempdir().unwrap();
    let first = root.path().join("FirstCopy");
    let second = root.path().join("SecondCopy");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    fs::write(first.join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(second.join("DICT.IDX"), ssedinfo_fixture()).unwrap();

    let registry = DriverRegistry::default();
    let mut library = BookLibrary::new();
    let report = library.try_open_discovered_paths(
        [root.path()],
        &registry,
        PackageDiscoveryOptions::default(),
    );

    assert_eq!(report.opened.len(), 1);
    assert_eq!(library.len(), 1);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "library_duplicate_book_skipped")
    );

    let import_result = library.import_result(report);
    assert_eq!(import_result.book_count, 1);
    assert_eq!(import_result.books.len(), 1);
    assert_eq!(import_result.opened_book_ids.len(), 1);
    assert_eq!(import_result.import_diagnostics.len(), 1);
}

#[test]
fn library_routes_all_book_search_without_unhandled_exceptions() {
    let ssed = tempdir().unwrap();
    fs::write(ssed.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();

    let lved = tempdir().unwrap();
    write_minimal_lved_sqlite_fixture(lved.path());

    let registry = DriverRegistry::default();
    let mut library = BookLibrary::new();
    let ssed_id = library.open_path(ssed.path(), &registry).unwrap();
    let lved_id = library.open_path(lved.path(), &registry).unwrap();
    assert_eq!(library.len(), 2);
    assert!(
        library
            .metadata()
            .iter()
            .any(|metadata| metadata.book_id == ssed_id)
    );
    assert!(
        library
            .metadata()
            .iter()
            .any(|metadata| metadata.book_id == lved_id)
    );
    let snapshot = library.metadata_snapshot();
    assert_eq!(snapshot.len(), 2);
    assert!(snapshot.iter().any(|metadata| metadata.book_id == ssed_id));
    assert!(snapshot.iter().any(|metadata| metadata.book_id == lved_id));

    let page = library
        .search(&SearchQuery {
            scope: SearchScope::AllBooks,
            mode: SearchMode::Forward,
            query: "test".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert!(page.hits.is_empty());
    assert!(!page.diagnostics.is_empty());
    assert!(
        page.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.context.contains_key("book_id"))
    );
}

#[test]
fn library_scopes_resource_hrefs_in_search_result_labels() {
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
                "update list set title = '<img src=\"pic.png\"><b>alpha</b>' where id = 1",
                [],
            )
            .unwrap();
    }
    fs::write(dir.path().join("res/pic.png"), b"png").unwrap();

    let registry = DriverRegistry::default();
    let mut library = BookLibrary::new();
    let book_id = library.open_path(dir.path(), &registry).unwrap();
    let page = library
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook { book_id },
            mode: SearchMode::Forward,
            query: "alpha".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    let hit = page.hits.first().expect("expected alpha search hit");
    assert_eq!(hit.title_text, "alpha");
    let href_start = hit
        .title_html
        .find("lvcore://resource/")
        .expect("search label should include rewritten resource href");
    let href = &hit.title_html[href_start..]
        .split('"')
        .next()
        .expect("resource href should be quoted");
    let scoped_suffix = href.strip_prefix("lvcore://resource/").unwrap();
    assert_eq!(
        scoped_suffix.split('/').count(),
        2,
        "library search result resource href should include book scope and resource token"
    );
    assert_eq!(
        library.read_scoped_resource_href(href).unwrap(),
        b"png".to_vec()
    );
    let resource = library.resolve_scoped_resource_href(href).unwrap();
    assert_eq!(resource.mime_type.as_deref(), Some("image/png"));
    assert_eq!(resource.href.as_deref(), Some(*href));
}

#[test]
fn library_scopes_resource_hrefs_in_navigation_surface_labels() {
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
                "update list set title = '<img src=\"pic.png\"><b>alpha</b>' where id = 1",
                [],
            )
            .unwrap();
    }
    fs::write(dir.path().join("res/pic.png"), b"png").unwrap();

    let registry = DriverRegistry::default();
    let mut library = BookLibrary::new();
    let book_id = library.open_path(dir.path(), &registry).unwrap();
    let surface = library
        .open_surface_page(&book_id, "lved-list", None, 1)
        .unwrap();

    let NavigationSurface::TitleIndexBrowse { ref items, .. } = surface else {
        panic!("expected LVED list title/index surface");
    };
    let item = items.first().expect("expected first list item");
    assert_eq!(item.label_text, "alpha");
    let href_start = item
        .label_html
        .find("lvcore://resource/")
        .expect("navigation label should include rewritten resource href");
    let href = &item.label_html[href_start..]
        .split('"')
        .next()
        .expect("resource href should be quoted");
    let scoped_suffix = href.strip_prefix("lvcore://resource/").unwrap();
    assert_eq!(
        scoped_suffix.split('/').count(),
        2,
        "library navigation label resource href should include book scope and resource token"
    );
    assert_eq!(
        library.read_scoped_resource_href(href).unwrap(),
        b"png".to_vec()
    );
    assert_eq!(surface.actionable_targets()[0].label_html, item.label_html);
}

#[test]
fn library_scopes_resource_hrefs_in_navigation_target_views() {
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
                "update list set title = '<img src=\"pic.png\"><b>alpha</b>' where id = 1",
                [],
            )
            .unwrap();
    }
    fs::write(dir.path().join("res/pic.png"), b"png").unwrap();

    let registry = DriverRegistry::default();
    let mut library = BookLibrary::new();
    let book_id = library.open_path(dir.path(), &registry).unwrap();
    let target = TargetToken::new(&InternalTarget::TitleIndexItem {
        surface_id: "lved-list".to_owned(),
        item_id: "root".to_owned(),
    })
    .unwrap();
    let routed = library
        .render_target_routed(&book_id, &target, &RenderOptions::default())
        .unwrap();

    let NavigationSurface::TitleIndexBrowse { items, .. } =
        routed.view.surface.as_ref().expect("expected surface view")
    else {
        panic!("expected LVED list title/index surface");
    };
    let label_html = &items[0].label_html;
    let href_start = label_html
        .find("lvcore://resource/")
        .expect("navigation target view label should include rewritten resource href");
    let href = &label_html[href_start..]
        .split('"')
        .next()
        .expect("resource href should be quoted");
    let scoped_suffix = href.strip_prefix("lvcore://resource/").unwrap();
    assert_eq!(
        scoped_suffix.split('/').count(),
        2,
        "library navigation target view resource href should include book scope and resource token"
    );
    assert_eq!(
        library.read_scoped_resource_href(href).unwrap(),
        b"png".to_vec()
    );
}

#[test]
fn library_scopes_resource_hrefs_in_local_entry_views_and_windows() {
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
                "update content set body = '<article><img src=\"pic.png\">Alpha body</article>' where id = 100",
                [],
            )
            .unwrap();
        connection
            .execute(
                "update content set body = '<article><img src=\"pic.png\">Beta body</article>' where id = 105",
                [],
            )
            .unwrap();
    }
    fs::write(dir.path().join("res/pic.png"), b"png").unwrap();

    let registry = DriverRegistry::default();
    let mut library = BookLibrary::new();
    let book_id = library.open_path(dir.path(), &registry).unwrap();
    let target = TargetToken::new(&InternalTarget::LvedRow {
        table: "content".to_owned(),
        row_id: 100,
        anchor: None,
        query: None,
    })
    .unwrap();

    let view = library
        .render_target(&book_id, &target, &RenderOptions::default())
        .unwrap();
    let html = view.display_html.as_deref().unwrap();
    let href_start = html
        .find("lvcore://resource/")
        .expect("local render_target HTML should include rewritten resource href");
    let href = &html[href_start..]
        .split('"')
        .next()
        .expect("resource href should be quoted");
    assert_eq!(
        href.strip_prefix("lvcore://resource/")
            .unwrap()
            .split('/')
            .count(),
        2
    );
    assert_eq!(
        library.read_scoped_resource_href(href).unwrap(),
        b"png".to_vec()
    );

    let window = library
        .resolve_target_window(
            &book_id,
            &target,
            Some(&lvcore::SequenceHint::LvedListOrder),
            0,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert!(
        window
            .center
            .display_html
            .as_deref()
            .unwrap()
            .contains(href),
        "local target window center should be scoped like direct render_target"
    );
    assert!(
        window.after[0]
            .display_html
            .as_deref()
            .unwrap()
            .contains("lvcore://resource/")
    );
    let resolved = library
        .resolve_resource(&book_id, &view.resources[0].token)
        .unwrap();
    assert_eq!(
        resolved
            .href
            .as_deref()
            .unwrap()
            .strip_prefix("lvcore://resource/")
            .unwrap()
            .split('/')
            .count(),
        2
    );
}

#[test]
fn library_selected_book_search_uses_backend_cursor_pagination() {
    let first = tempdir().unwrap();
    fs::write(first.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        first.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        first.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture("shared", 1, 2, 13, 0)),
    )
    .unwrap();

    let second = tempdir().unwrap();
    fs::write(second.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        second.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"beta\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        second.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture("shared", 1, 2, 13, 0)),
    )
    .unwrap();

    let registry = DriverRegistry::default();
    let mut library = BookLibrary::new();
    let first_id = library.open_path(first.path(), &registry).unwrap();
    let second_id = library.open_path(second.path(), &registry).unwrap();

    let first_page = library
        .search(&SearchQuery {
            scope: SearchScope::SelectedBooks {
                book_ids: vec![first_id.clone(), second_id.clone()],
            },
            mode: SearchMode::Exact,
            query: "shared".to_owned(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(first_page.hits.len(), 1);
    assert_eq!(first_page.hits[0].title_text, "alpha");
    assert!(first_page.next_cursor.is_some());
    assert!(
        first_page
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "search_cursor_deferred")
    );

    let second_page = library
        .search(&SearchQuery {
            scope: SearchScope::SelectedBooks {
                book_ids: vec![first_id, second_id],
            },
            mode: SearchMode::Exact,
            query: "shared".to_owned(),
            cursor: first_page.next_cursor,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(second_page.hits.len(), 1);
    assert_eq!(second_page.hits[0].title_text, "beta");
    assert!(second_page.next_cursor.is_none());
}

#[test]
fn library_reports_missing_selected_books_as_diagnostics() {
    let registry = DriverRegistry::default();
    let mut library = BookLibrary::new();
    let ssed = tempdir().unwrap();
    fs::write(ssed.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    let ssed_id = library.open_path(ssed.path(), &registry).unwrap();

    let page = library
        .search(&SearchQuery {
            scope: SearchScope::SelectedBooks {
                book_ids: vec![ssed_id, lvcore::BookId("missing-book".to_owned())],
            },
            mode: SearchMode::Exact,
            query: "test".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert!(
        page.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "book_missing")
    );
}

#[test]
fn library_delegates_reader_operations_by_book_id() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"abcdef"),
    )
    .unwrap();
    fs::write(dir.path().join("MENU.DIC"), b"").unwrap();
    fs::create_dir(dir.path().join("Templates")).unwrap();
    fs::write(dir.path().join("Templates/B123.SVG"), b"<svg/>").unwrap();

    let registry = DriverRegistry::default();
    let mut library = BookLibrary::new();
    let book_id = library.open_path(dir.path(), &registry).unwrap();
    let metadata = library
        .metadata()
        .into_iter()
        .find(|metadata| metadata.book_id == book_id)
        .unwrap();
    assert!(book_id.0.starts_with("SSED:"));
    assert!(
        book_id.0.starts_with("SSED:DICT:"),
        "library book ids should use the SSED catalog identity, not the temp/package folder: {}",
        book_id.0
    );
    assert!(book_id.0.ends_with(&metadata.root_fingerprint[..12]));

    let surfaces = library.home_surfaces(&book_id).unwrap();
    assert!(
        surfaces
            .iter()
            .any(|surface| surface.kind == NavigationSurfaceKind::Menu)
    );

    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();
    let view = library
        .render_target(&book_id, &target, &RenderOptions::default())
        .unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::EntryBody);
    assert!(view.display_html.is_some());
    assert!(matches!(
        library
            .renderer_input_for_target(&book_id, &target)
            .unwrap(),
        RendererInput::HcSsedStream { .. }
    ));

    let window = library
        .resolve_target_window(&book_id, &target, None, 1, 1, &RenderOptions::default())
        .unwrap();
    assert_eq!(window.center.target, target);

    let resource = ResourceToken::new(&InternalResource::PackageFile {
        path: "templates/b123.svg".to_owned(),
        resource_kind: ResourceKind::Template,
    })
    .unwrap();
    assert!(
        library
            .resolve_resource(&book_id, &resource)
            .unwrap()
            .href
            .is_some()
    );
    assert_eq!(
        library.read_resource(&book_id, &resource).unwrap(),
        b"<svg/>"
    );
}

#[test]
fn library_routes_lved_cross_book_targets_through_loaded_book_aliases() {
    let root = tempdir().unwrap();
    let source_dir = root.path().join("_DCT_SOURCE");
    let destination_dir = root.path().join("_DCT_BUREI");
    fs::create_dir(&source_dir).unwrap();
    fs::create_dir(&destination_dir).unwrap();
    write_lved_cross_book_source_fixture(&source_dir);
    write_minimal_lved_sqlite_fixture(&destination_dir);
    {
        let connection = Connection::open(destination_dir.join("main.data")).unwrap();
        connection.pragma_update(None, "key", "test-key").unwrap();
        connection
            .pragma_update(None, "cipher_compatibility", 4)
            .unwrap();
        connection
            .execute(
                "update content set body = '<article><h1>Alpha</h1><p>Tree body</p><img src=\"pic.png\"></article>' where id = 100",
                [],
            )
            .unwrap();
    }
    fs::write(destination_dir.join("res/pic.png"), b"png").unwrap();

    let registry = DriverRegistry::default();
    let mut library = BookLibrary::new();
    let source_book_id = library.open_path(&source_dir, &registry).unwrap();
    let destination_book_id = library.open_path(&destination_dir, &registry).unwrap();

    let source_target = TargetToken::new(&InternalTarget::LvedRow {
        table: "content".to_owned(),
        row_id: 10,
        anchor: None,
        query: None,
    })
    .unwrap();
    let source_view = library
        .render_target(&source_book_id, &source_target, &RenderOptions::default())
        .unwrap();
    let cross_book_link = source_view
        .links
        .iter()
        .find(|link| link.kind == TargetKind::LvedCrossBook)
        .expect("source entry should expose a typed cross-book LVED link");

    let routed = library
        .render_target_routed(
            &source_book_id,
            &cross_book_link.token,
            &RenderOptions::default(),
        )
        .unwrap();

    assert_eq!(routed.book_id, destination_book_id);
    assert_eq!(routed.view.kind, ResolvedTargetKind::EntryBody);
    assert_eq!(routed.view.scroll_anchor.as_deref(), Some("dest"));
    assert!(
        routed
            .view
            .display_html
            .as_deref()
            .unwrap()
            .contains("<article><h1>Alpha</h1><p>Tree body</p>")
    );
    assert!(
        routed
            .view
            .display_html
            .as_deref()
            .unwrap()
            .contains("lvcore://resource/")
    );
    assert!(
        routed.view.resources.len() == 1,
        "destination resource should be retained in routed views"
    );
    let resource_href = routed.view.resources[0].href.as_deref().unwrap();
    let scoped_suffix = resource_href.strip_prefix("lvcore://resource/").unwrap();
    assert_eq!(
        scoped_suffix.split('/').count(),
        2,
        "routed resource href should include book scope and resource token"
    );
    assert!(
        routed
            .view
            .display_html
            .as_deref()
            .unwrap()
            .contains(resource_href)
    );
    assert_eq!(
        library.read_scoped_resource_href(resource_href).unwrap(),
        b"png".to_vec()
    );
    assert!(matches!(
        routed.view.target.decode().unwrap(),
        InternalTarget::LvedRow {
            table,
            row_id: 100,
            anchor: Some(anchor),
            query: None
        } if table == "content" && anchor == "dest"
    ));
    assert!(
        routed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "lved_cross_book_routed")
    );

    let window = library
        .resolve_target_window_routed(
            &source_book_id,
            &cross_book_link.token,
            Some(&lvcore::SequenceHint::LvedListOrder),
            0,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(window.book_id, destination_book_id);
    assert_eq!(window.window.center.scroll_anchor.as_deref(), Some("dest"));
    assert_eq!(window.window.after.len(), 1);
    assert_eq!(window.window.after[0].title.as_deref(), Some("beta"));
    assert!(
        window
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "lved_cross_book_routed")
    );
}

#[test]
fn library_rejects_invalid_scoped_resource_hrefs() {
    let library = BookLibrary::new();
    assert!(matches!(
        library.read_scoped_resource_href("lvcore://resource/not-scoped"),
        Err(lvcore::Error::InvalidResourceHref)
    ));
    assert!(matches!(
        library.read_scoped_resource_href("https://example.test/resource"),
        Err(lvcore::Error::InvalidResourceHref)
    ));
    assert!(matches!(
        library.resolve_scoped_resource_href("lvcore://resource/not-scoped"),
        Err(lvcore::Error::InvalidResourceHref)
    ));
}

#[test]
fn library_reports_lved_cross_book_targets_when_destination_is_not_open() {
    let root = tempdir().unwrap();
    let source_dir = root.path().join("_DCT_SOURCE");
    fs::create_dir(&source_dir).unwrap();
    write_lved_cross_book_source_fixture(&source_dir);

    let registry = DriverRegistry::default();
    let mut library = BookLibrary::new();
    let source_book_id = library.open_path(&source_dir, &registry).unwrap();

    let source_target = TargetToken::new(&InternalTarget::LvedRow {
        table: "content".to_owned(),
        row_id: 10,
        anchor: None,
        query: None,
    })
    .unwrap();
    let source_view = library
        .render_target(&source_book_id, &source_target, &RenderOptions::default())
        .unwrap();
    let cross_book_link = source_view
        .links
        .iter()
        .find(|link| link.kind == TargetKind::LvedCrossBook)
        .expect("source entry should expose a typed cross-book LVED link");

    let routed = library
        .render_target_routed(
            &source_book_id,
            &cross_book_link.token,
            &RenderOptions::default(),
        )
        .unwrap();

    assert_eq!(routed.book_id, source_book_id);
    assert_eq!(routed.view.kind, ResolvedTargetKind::Unsupported);
    assert!(
        routed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "lved_cross_book_destination_missing")
    );
    assert!(
        routed
            .view
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "lved_cross_book_destination_missing")
    );

    let window = library
        .resolve_target_window_routed(
            &source_book_id,
            &cross_book_link.token,
            Some(&lvcore::SequenceHint::LvedListOrder),
            1,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(window.book_id, source_book_id);
    assert_eq!(window.window.center.kind, ResolvedTargetKind::Unsupported);
    assert!(window.window.before.is_empty());
    assert!(window.window.after.is_empty());
}

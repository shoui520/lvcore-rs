use super::common::*;

#[test]
fn ios_ssed_shell_with_known_retained_fts_dbc_opens_lved_search_payload() {
    let root = tempdir().unwrap();
    let package_root = root.path().join("OXFPEU4");
    fs::create_dir(&package_root).unwrap();
    fs::write(package_root.join("OXFPEU4.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        package_root.join("HONMON.DIC"),
        sseddata_literal_fixture(b"anchor"),
    )
    .unwrap();
    let payload = package_root.join("OXFPEU4.dbc");
    let key = lvcore::lved_sqlite::derive_android_lved_sqlcipher_key(750, "OXFPEU4");
    {
        let connection = Connection::open(&payload).unwrap();
        lvcore::lved_sqlite::apply_sqlcipher_key(&connection, &key).unwrap();
        connection
            .execute_batch(
                "
                create table info (id integer, type integer, name text primary key, body text, media text);
                insert into info values (1, 1, 'about.html', '<h1>About OXFPEU4</h1>', '');
                create table content (id integer primary key, type integer, body text, media text);
                insert into content values (100, 1, '<article><h1>Alpha</h1><p>retained body</p></article>', '');
                create table list (id integer primary key, refid integer, type integer, anchor text, title text, titlesub text);
                insert into list values (1, 100, 1, '', '<b>alpha</b>', '');
                create virtual table search using fts4(forward, back, part, fts, advanced1, advanced2, filter);
                insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                  values (1, 'alpha', 'ahpla', 'shared alpha', 'alpha body', '', '', '∥alpha∥');
                ",
            )
            .unwrap();
    }
    fs::write(
        root.path().join("DictList.plist"),
        br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>ItemArray</key><array><dict>
    <key>DictFolder</key><string>OXFPEU4</string>
    <key>DictName</key><string>iOS Retained Search Fixture</string>
    <key>DictFtsDB</key><string>OXFPEU4/OXFPEU4.dbc</string>
  </dict></array>
  <key>StatusArray</key><array><dict>
    <key>SearchMethod</key><array>
      <dict><key>key</key><string>Forward</string><key>use</key><true/></dict>
      <dict><key>key</key><string>Backward</string><key>use</key><true/></dict>
      <dict><key>key</key><string>Literal</string><key>use</key><true/></dict>
      <dict><key>key</key><string>Part</string><key>use</key><true/></dict>
      <dict><key>key</key><string>All</string><key>use</key><true/></dict>
    </array>
  </dict></array>
</dict></plist>"#,
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    assert_eq!(package.metadata().format_family, FormatFamily::Ssed);
    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::NativeSearch)
    );
    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::PreservedHtml)
    );
    for mode in [
        SearchMode::Exact,
        SearchMode::Forward,
        SearchMode::Backward,
        SearchMode::Partial,
        SearchMode::FullText,
    ] {
        assert!(package.metadata().search_modes.contains(&mode));
    }
    let surfaces = package.home_surfaces().unwrap();
    assert!(surfaces.iter().any(|surface| {
        surface.surface_id == "lved-list" && surface.status == NavigationStatus::Available
    }));
    assert!(
        surfaces
            .iter()
            .all(|surface| surface.surface_id != "ios-retained-fts"),
        "known retained iOS .dbc payload should not be exposed as deferred"
    );

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "alp".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "alpha");
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::LvedRow {
            table,
            row_id: 100,
            ..
        } if table == "content"
    ));
    let view = package
        .render_target(&page.hits[0].target, &RenderOptions::default())
        .unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::EntryBody);
    assert!(
        view.display_html
            .as_deref()
            .unwrap()
            .contains("retained body")
    );
}

#[test]
fn ios_ssed_shell_with_unknown_retained_fts_dbc_reports_deferred_search_payload() {
    let root = tempdir().unwrap();
    let package_root = root.path().join("DICT");
    fs::create_dir(&package_root).unwrap();
    fs::write(package_root.join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        package_root.join("HONMON.DIC"),
        sseddata_literal_fixture(b"anchor"),
    )
    .unwrap();
    fs::write(
        package_root.join("DICT.dbc"),
        b"encrypted retained database",
    )
    .unwrap();
    fs::write(
        root.path().join("DictList.plist"),
        br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>ItemArray</key><array><dict>
    <key>DictName</key><string>iOS Retained Search Fixture</string>
    <key>DictFtsDB</key><string>DICT/DICT.dbc</string>
  </dict></array>
  <key>StatusArray</key><array><dict>
    <key>SearchMethod</key><array>
      <dict><key>key</key><string>Forward</string><key>use</key><true/></dict>
      <dict><key>key</key><string>Backward</string><key>use</key><true/></dict>
      <dict><key>key</key><string>Literal</string><key>use</key><true/></dict>
      <dict><key>key</key><string>Part</string><key>use</key><true/></dict>
      <dict><key>key</key><string>All</string><key>use</key><true/></dict>
      <dict><key>key</key><string>Example</string><key>use</key><false/></dict>
    </array>
  </dict></array>
</dict></plist>"#,
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    assert_eq!(
        package.metadata().search_modes,
        vec![
            SearchMode::Exact,
            SearchMode::Forward,
            SearchMode::Backward,
            SearchMode::Partial,
            SearchMode::FullText,
        ]
    );
    assert!(
        !package
            .metadata()
            .capabilities
            .contains(&Capability::NativeSearch),
        "the encrypted retained iOS .dbc must not be advertised as implemented native search"
    );
    let surfaces = package.home_surfaces().unwrap();
    let retained = surfaces
        .iter()
        .find(|surface| surface.surface_id == "ios-retained-fts")
        .expect("retained iOS FTS payload should be visible as a deferred home surface");
    assert_eq!(retained.status, NavigationStatus::Deferred);
    assert!(retained.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ios_retained_fts_deferred"
            && diagnostic
                .context
                .get("path")
                .is_some_and(|path| path == "DICT/DICT.dbc")
            && diagnostic
                .context
                .get("dict_code")
                .is_some_and(|code| code == "DICT")
            && diagnostic
                .context
                .get("exists")
                .is_some_and(|value| value == "true")
    }));

    let opened = package.open_surface("ios-retained-fts").unwrap();
    let NavigationSurface::Deferred { diagnostics, .. } = opened else {
        panic!("retained iOS FTS surface should open as diagnostic-only deferred surface");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ios_retained_fts_deferred")
    );

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::FullText,
            query: "alpha".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert!(page.hits.is_empty());
    assert!(
        page.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ios_retained_fts_deferred"),
        "search must fail honestly until the retained .dbc decryption path is understood"
    );
}

#[test]
fn ssed_home_surfaces_are_capability_based() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture("alpha", 10, 2, 13, 0)),
    )
    .unwrap();
    fs::write(
        dir.path().join("MENU.DIC"),
        sseddata_literal_fixture(&menu_stream_fixture(10, 2)),
    )
    .unwrap();
    fs::create_dir(dir.path().join("Panel")).unwrap();
    fs::write(
        dir.path().join("Panels.xml"),
        r#"<panels>
  <panel index="01000000" paneltype="menu" count_x="2">
    <title>五十音</title>
    <data><cell action_verb="lved.panel:01010000" ref="01010000">あ</cell></data>
  </panel>
  <panel index="01010000" paneltype="contents">
    <title>あ</title>
    <data type="bin" filename="Panel\All-A.bin" />
  </panel>
</panels>"#,
    )
    .unwrap();
    fs::write(dir.path().join("Panel/All-A.bin"), panel_bin_fixture(10, 2)).unwrap();
    fs::write(dir.path().join("HANREI.chm"), b"chm").unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let metadata = package.metadata();
    assert_eq!(metadata.format_family, FormatFamily::Ssed);
    assert!(
        metadata.book_id.0.starts_with("SSED:DICT:"),
        "SSED book identity should be based on the catalog .idx stem, not the package folder: {}",
        metadata.book_id.0
    );
    assert!(metadata.capabilities.contains(&Capability::HcRenderInput));
    assert!(metadata.capabilities.contains(&Capability::NativeSearch));
    assert!(metadata.capabilities.contains(&Capability::Hanrei));
    assert!(metadata.capabilities.contains(&Capability::Panels));
    assert!(
        !metadata.capabilities.contains(&Capability::FullTextSearch),
        "SSED fulltext must not be advertised without a supported HONMON payload"
    );
    assert_eq!(
        metadata.search_modes,
        vec![
            SearchMode::Exact,
            SearchMode::Forward,
            SearchMode::Backward,
            SearchMode::Partial,
        ]
    );

    let surfaces = package.home_surfaces().unwrap();
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::Menu && surface.status == NavigationStatus::Available
    }));
    let search_home_surface = surfaces
        .iter()
        .find(|surface| surface.kind == NavigationSurfaceKind::SearchFallback)
        .unwrap();
    assert_eq!(search_home_surface.status, NavigationStatus::Available);
    assert!(search_home_surface.target.is_none());
    assert!(matches!(
        package.open_surface("search").unwrap(),
        NavigationSurface::FallbackSearch { .. }
    ));
    let menu_home_target = surfaces
        .iter()
        .find(|surface| surface.kind == NavigationSurfaceKind::Menu)
        .and_then(|surface| surface.target.clone())
        .unwrap();
    let menu_home_view = package
        .render_target(&menu_home_target, &RenderOptions::default())
        .unwrap();
    assert_eq!(menu_home_view.kind, ResolvedTargetKind::NavigationSurface);
    assert!(matches!(
        menu_home_view.surface.as_ref().unwrap(),
        lvcore::NavigationSurface::SimpleMenu { nodes, .. } if nodes.len() == 1
    ));
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::Panel
            && surface.status == NavigationStatus::Available
    }));
    let hanrei_surface = surfaces
        .iter()
        .find(|surface| surface.kind == NavigationSurfaceKind::Hanrei)
        .unwrap();
    assert_eq!(hanrei_surface.status, NavigationStatus::Available);
    assert!(
        hanrei_surface
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_hanrei_chm_deferred")
    );
    let menu_surface = package.open_surface("menu").unwrap();
    let lvcore::NavigationSurface::SimpleMenu { nodes, .. } = menu_surface else {
        panic!("SSED MENU should decode to a simple menu surface");
    };
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].label_text, "あ");
    assert!(matches!(
        nodes[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 10,
            offset: 2,
        } if component == "HONMON.DIC"
    ));
    let panel_surface = package.open_surface("panels").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = panel_surface else {
        panic!("SSED Panels should decode to a panel surface");
    };
    assert_eq!(cells.len(), 1);
    assert!(matches!(
        cells[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::PanelCell { panel_id, .. } if panel_id == "01010000"
    ));
    let panel_view = package
        .render_target(cells[0].target.as_ref().unwrap(), &RenderOptions::default())
        .unwrap();
    assert_eq!(panel_view.kind, ResolvedTargetKind::PanelSurface);
    assert!(matches!(
        panel_view.surface.as_ref().unwrap(),
        lvcore::NavigationSurface::Panel { cells, .. } if cells.len() == 1
    ));
    let child_panel = package.open_surface("panels:01010000").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = child_panel else {
        panic!("SSED child Panel should decode to a panel surface");
    };
    assert_eq!(cells.len(), 1);
    assert!(matches!(
        cells[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 10,
            offset: 2,
        } if component == "HONMON.DIC"
    ));
}

#[test]
fn ssed_menu_surfaces_are_pageable() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"body"),
    )
    .unwrap();
    fs::write(
        dir.path().join("MENU.DIC"),
        sseddata_literal_fixture(&menu_stream_fixture_rows(&[
            ([0x24, 0x22], 10, 0),
            ([0x24, 0x24], 10, 2),
            ([0x24, 0x26], 10, 4),
        ])),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let first = package.open_surface_page("menu", None, 2).unwrap();
    let NavigationSurface::SimpleMenu {
        nodes, next_cursor, ..
    } = first
    else {
        panic!("SSED MENU should decode to a pageable simple menu surface");
    };
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].node_id, "ssed-menu:0");
    assert_eq!(nodes[1].node_id, "ssed-menu:1");
    assert_eq!(next_cursor.as_deref(), Some("2"));

    let second = package
        .open_surface_page("menu", next_cursor.as_deref(), 2)
        .unwrap();
    let NavigationSurface::SimpleMenu {
        nodes, next_cursor, ..
    } = second
    else {
        panic!("second SSED MENU page should decode to a simple menu surface");
    };
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].node_id, "ssed-menu:2");
    assert_eq!(nodes[0].label_text, "う");
    assert!(next_cursor.is_none());
}

#[test]
fn ssed_missing_declared_indexes_do_not_advertise_search_or_title_browse() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert!(
        !package
            .metadata()
            .capabilities
            .contains(&Capability::NativeSearch),
        "declared index components without payload files must not become native search"
    );
    assert!(
        !package
            .metadata()
            .capabilities
            .contains(&Capability::TitleIndexBrowse),
        "declared index components without payload files must not become title browse"
    );
    assert!(package.metadata().search_modes.is_empty());
    assert!(!package.home_surfaces().unwrap().iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::TitleIndexBrowse
            && surface.status == NavigationStatus::Available
    }));
}

#[test]
fn ssed_empty_placeholder_indexes_do_not_advertise_search_or_title_browse() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"placeholder"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&vec![0; 2048]),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert!(
        !package
            .metadata()
            .capabilities
            .contains(&Capability::NativeSearch),
        "placeholder index payloads without decodable target rows must not become native search"
    );
    assert!(
        !package
            .metadata()
            .capabilities
            .contains(&Capability::TitleIndexBrowse),
        "placeholder index payloads without decodable target rows must not become title browse"
    );
    assert!(package.metadata().search_modes.is_empty());
    assert!(!package.home_surfaces().unwrap().iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::TitleIndexBrowse
            && surface.status == NavigationStatus::Available
    }));
}

#[test]
fn ssed_android_wrapped_index_title_and_menu_payloads_are_supported() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("MENU.DIC"),
        android_wrapped_sseddata_fixture(sseddata_literal_fixture(&menu_stream_fixture(10, 2))),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        android_wrapped_sseddata_fixture(sseddata_literal_fixture(b"keyless\x1f\x0a")),
    )
    .unwrap();
    let mut page = vec![0u8; 2048];
    page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    page[2..4].copy_from_slice(&1u16.to_be_bytes());
    page[4..8].copy_from_slice(&1u32.to_be_bytes());
    page[8..10].copy_from_slice(&14u16.to_be_bytes());
    page[11..15].copy_from_slice(&13u32.to_be_bytes());
    page[15..17].copy_from_slice(&0u16.to_be_bytes());
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        android_wrapped_sseddata_fixture(sseddata_literal_fixture(&page)),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::NativeSearch),
        "Android-wrapped index SSEDDATA is a supported SSED index payload"
    );
    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::TitleIndexBrowse),
        "Android-wrapped index SSEDDATA should advertise title/index browse"
    );
    assert!(
        package.metadata().capabilities.contains(&Capability::Menu),
        "Android-wrapped MENU.DIC should advertise a menu when it decodes to rows"
    );
    let NavigationSurface::SimpleMenu { nodes, .. } = package.open_surface("menu").unwrap() else {
        panic!("Android-wrapped MENU.DIC should open as a simple menu");
    };
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].label_text, "あ");
    let NavigationSurface::TitleIndexBrowse { items, .. } =
        package.open_surface("title-index").unwrap()
    else {
        panic!("Android-wrapped title/index files should open");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label_text, "keyless");
}

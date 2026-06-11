use super::*;
use crate::DriverRegistry;

#[test]
fn detects_lved_sqlite3_by_main_data_and_key() {
    let dir = tempdir().unwrap();
    write_lved_search_fixture(dir.path());

    let detected = LvedSqliteDriver.detect(dir.path()).unwrap().unwrap();
    assert_eq!(detected.format_family, FormatFamily::LvedSqlite3);
    assert!(
        detected
            .evidence
            .iter()
            .any(|item| item.starts_with("key_file:"))
    );
}

#[test]
fn lved_key_file_detection_does_not_decrypt_payload_for_title() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("main.data"), b"not sqlitecipher yet").unwrap();
    fs::write(dir.path().join("main.key"), "test-key").unwrap();

    let detected = LvedSqliteDriver.detect(dir.path()).unwrap().unwrap();

    assert_eq!(detected.format_family, FormatFamily::LvedSqlite3);
    assert!(detected.title.is_none());
    assert!(
        detected
            .evidence
            .iter()
            .any(|item| item.starts_with("key_file:"))
    );
    assert!(LvedSqliteDriver.open(dir.path()).is_err());
}

#[test]
fn explicit_ios_dbc_payload_detects_as_lved_even_with_retained_ssed_idx() {
    let dir = tempdir().unwrap();
    let package = dir.path().join("OXFPEU4");
    fs::create_dir_all(&package).unwrap();
    fs::write(
        dir.path().join("DictList.plist"),
        br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>ItemArray</key><array><dict>
    <key>DictFolder</key><string>OXFPEU4</string>
    <key>DictName</key><string>Oxford Test Dictionary</string>
    <key>DictFtsDB</key><string>OXFPEU4/OXFPEU4.dbc</string>
  </dict></array>
</dict></plist>"#,
    )
    .unwrap();
    write_retained_ssedinfo_idx(&package, "OXFPEU4.IDX");
    let payload = package.join("OXFPEU4.dbc");
    let key = crate::lved_sqlite::derive_android_lved_sqlcipher_key(750, "OXFPEU4");
    {
        let connection = Connection::open(&payload).unwrap();
        apply_sqlcipher_key(&connection, &key).unwrap();
        connection
            .execute_batch(
                "
                create table info (id integer, type integer, name text primary key, body text, media text);
                insert into info values (1, 1, 'about.html', '<h1>Fallback</h1>', '');
                create table content (id integer primary key, type integer, body text, media text);
                create table list (id integer primary key, refid integer, type integer, anchor text, title text, titlesub text);
                create virtual table search using fts4(forward, back, part, fts, filter);
                insert into content values (100, 1, '<article>body</article>', '');
                insert into list values (1, 100, 1, '', '<b>alpha</b>', '');
                insert into search(rowid, forward, back, part, fts, filter)
                  values (1, 'alpha', 'ahpla', 'alpha', 'alpha body', '∥alpha∥');
                ",
            )
            .unwrap();
    }

    let detected = DriverRegistry::default().detect(&payload).unwrap();

    assert_eq!(detected[0].format_family, FormatFamily::LvedSqlite3);
    assert_eq!(detected[0].title.as_deref(), Some("Oxford Test Dictionary"));
    assert!(
        detected
            .iter()
            .any(|row| row.format_family == FormatFamily::Ssed)
    );

    let detected_package = DriverRegistry::default().detect(&package).unwrap();

    assert_eq!(detected_package[0].format_family, FormatFamily::LvedSqlite3);
    assert_eq!(
        detected_package[0].title.as_deref(),
        Some("Oxford Test Dictionary")
    );
    assert!(
        detected_package
            .iter()
            .any(|row| row.format_family == FormatFamily::Ssed)
    );
}

#[test]
fn retained_ssed_components_are_capabilities_not_family_gated() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::BodyRows);
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::LvedSqlite3,
            confidence: 98,
            title: Some("Mixed package".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );

    let surfaces = package.home_surfaces().unwrap();
    assert!(surfaces.iter().any(|surface| {
        surface.surface_id == "title-index"
            && surface.kind == NavigationSurfaceKind::TitleIndexBrowse
            && surface.status == NavigationStatus::Available
    }));

    let surface = package.open_surface("title-index").unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = surface else {
        panic!("expected retained SSED title/index surface");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].label_text, "alpha");

    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "あ".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "alpha");

    let window = package
        .resolve_target_window(
            &items[0].target,
            Some(&SequenceHint::TitleIndexOrder {
                value: "title-index".to_owned(),
                cursor: None,
            }),
            0,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(window.center.title.as_deref(), Some("alpha"));
    assert_eq!(window.after.len(), 1);
    assert_eq!(window.after[0].title.as_deref(), Some("beta"));
}

#[test]
fn lved_retained_loose_sseddata_index_matches_are_deferred_block_offsets() {
    let dir = tempdir().unwrap();
    write_lved_search_fixture(dir.path());
    let index_page =
        simple_index_page_for_test(&[(&body_jis("ateb"), 2, 0), (&body_jis("tsohg"), 999, 0)]);
    fs::write(
        dir.path().join("BHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&[&index_page], 200, 200),
    )
    .unwrap();
    fs::write(
        dir.path().join("BKINDEX.DIC"),
        fixture_sseddata_literal_chunks(&[&index_page], 201, 201),
    )
    .unwrap();

    let package = LvedSqliteDriver.open(dir.path()).unwrap();
    let diagnostics = &package.metadata().diagnostics;
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| { diagnostic.code != "retained_ssed_component_deferred" })
    );

    let surfaces = package.home_surfaces().unwrap();

    assert!(surfaces.iter().any(|surface| {
        surface.surface_id == "lved-list"
            && surface.kind == NavigationSurfaceKind::TitleIndexBrowse
            && surface.status == NavigationStatus::Available
    }));
    assert!(
        !surfaces
            .iter()
            .any(|surface| surface.surface_id == "title-index"),
        "retained loose indexes without an SSED catalog must not expose fake SSED browsing"
    );
    assert!(
        !surfaces
            .iter()
            .any(|surface| surface.surface_id == "retained-ssed-components"),
        "retained component evidence belongs in metadata diagnostics, not user navigation"
    );

    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
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

    let retained_page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Backward,
            query: "eta".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert!(retained_page.hits.is_empty());
    let deferred = retained_page
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "lved_retained_ssed_index_match_deferred")
        .expect("retained SSED index matches should preserve block/offset evidence as deferred");
    assert_eq!(
        deferred.context.get("body_address").map(String::as_str),
        Some("00000002:0000")
    );
}

#[test]
fn lved_cataloged_nonreader_retained_components_are_not_deferred_noise() {
    let dir = tempdir().unwrap();
    write_lved_search_fixture(dir.path());
    write_retained_ssedinfo_idx(dir.path(), "RETAINED.IDX");
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[b"\x1f\x0a retained body"], 100, 100),
    )
    .unwrap();

    let package = LvedSqliteDriver.open(dir.path()).unwrap();

    assert_eq!(package.metadata().format_family, FormatFamily::LvedSqlite3);
    assert!(
        package
            .metadata()
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "retained_ssed_component_deferred"),
        "catalog-backed retained components should not be reported as unresolved loose components"
    );

    let surfaces = package.home_surfaces().unwrap();
    assert!(
        !surfaces
            .iter()
            .any(|surface| surface.surface_id == "title-index"),
        "a retained catalog with no decodable index rows must not expose a fake SSED surface"
    );
    assert!(surfaces.iter().any(|surface| {
        surface.surface_id == "lved-list"
            && surface.kind == NavigationSurfaceKind::TitleIndexBrowse
            && surface.status == NavigationStatus::Available
    }));
}

#[test]
fn lved_adopts_reader_reachable_retained_ssed_catalog_as_secondary_surface() {
    let dir = tempdir().unwrap();
    write_lved_search_fixture(dir.path());
    write_retained_ssed_catalog_with_title_index(dir.path(), "MIXED.IDX");

    let package = LvedSqliteDriver.open(dir.path()).unwrap();

    assert_eq!(package.metadata().format_family, FormatFamily::LvedSqlite3);
    assert!(
        package
            .metadata()
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "retained_ssed_component_deferred")
    );
    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::HcRenderInput)
    );

    let surfaces = package.home_surfaces().unwrap();
    assert!(surfaces.iter().any(|surface| {
        surface.surface_id == "lved-list" && surface.status == NavigationStatus::Available
    }));
    assert!(surfaces.iter().any(|surface| {
        surface.surface_id == "title-index" && surface.status == NavigationStatus::Available
    }));
    let lved_position = surfaces
        .iter()
        .position(|surface| surface.surface_id == "lved-list")
        .unwrap();
    let retained_position = surfaces
        .iter()
        .position(|surface| surface.surface_id == "title-index")
        .unwrap();
    assert!(
        lved_position < retained_position,
        "LVED-native navigation must remain primary when retained SSED is secondary"
    );

    let surface = package.open_surface_page("title-index", None, 10).unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = surface else {
        panic!("retained SSED title/index should open as a secondary browse surface");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label_text, "retained alpha");

    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "alpha".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(page.hits.len(), 1);
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::LvedRow { table, .. } if table == "content"
    ));
}

#[test]
fn lved_tree_surface_lazily_pages_large_child_branches() {
    let dir = tempdir().unwrap();
    write_lved_search_fixture(dir.path());
    let mut tree = String::from("0\t0\tRoot\n");
    for index in 0..24 {
        let data_id = match index {
            0 => 100,
            1 => 101,
            _ => 102,
        };
        tree.push_str(&format!("{data_id}\t1\tChild {index}\n"));
    }
    fs::write(dir.path().join("tree.idx"), tree).unwrap();

    let package = LvedSqliteDriver.open(dir.path()).unwrap();
    let root_surface = package.open_surface_page("lved-tree", None, 4).unwrap();
    let NavigationSurface::HierarchicalTree { nodes, .. } = root_surface else {
        panic!("expected LVED tree surface");
    };
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].label_text, "Root");
    assert_eq!(nodes[0].child_cursor.as_deref(), Some("children:0:0"));
    assert!(nodes[0].children.is_empty());

    let child_surface = package
        .open_surface_page("lved-tree", nodes[0].child_cursor.as_deref(), 4)
        .unwrap();
    let NavigationSurface::HierarchicalTree {
        nodes, next_cursor, ..
    } = child_surface
    else {
        panic!("expected LVED tree child page");
    };
    assert_eq!(nodes.len(), 4);
    assert_eq!(nodes[0].label_text, "Child 0");
    assert!(nodes[0].target.is_some());
    assert_eq!(next_cursor.as_deref(), Some("children:0:4"));
}

#[cfg(unix)]
#[test]
fn lved_detection_ignores_symlinked_payload_escape() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    write_lved_search_fixture(outside.path());
    symlink(
        outside.path().join("main.data"),
        dir.path().join("main.data"),
    )
    .unwrap();
    fs::write(dir.path().join("main.key"), "test-key").unwrap();

    assert!(LvedSqliteDriver.detect(dir.path()).unwrap().is_none());
}

#[cfg(unix)]
#[test]
fn lved_key_discovery_ignores_symlinked_key_escape() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let payload = dir.path().join("main.data");
    fs::write(&payload, b"payload").unwrap();
    fs::write(outside.path().join("main.key"), "outside-key").unwrap();
    symlink(outside.path().join("main.key"), dir.path().join("main.key")).unwrap();

    assert!(
        crate::lved_sqlite::discover_lved_key_file(&payload)
            .unwrap()
            .is_none()
    );
}

fn write_retained_ssedinfo_idx(root: &Path, filename: &str) {
    let mut data = vec![0u8; 0x80 + 0x30];
    data[..SSEDINFO_MAGIC.len()].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Retained SSED";
    data[0x0c] = title.len() as u8;
    data[0x0d..0x0d + title.len()].copy_from_slice(title);
    data[0x4d] = 1;
    let rec = &mut data[0x80..0x80 + 0x30];
    rec[3] = 0x00;
    rec[4..8].copy_from_slice(&100_u32.to_be_bytes());
    rec[8..12].copy_from_slice(&100_u32.to_be_bytes());
    rec[0x10] = b"HONMON.DIC".len() as u8;
    rec[0x11..0x11 + b"HONMON.DIC".len()].copy_from_slice(b"HONMON.DIC");
    fs::write(root.join(filename), data).unwrap();
}

fn write_retained_ssed_catalog_with_title_index(root: &Path, filename: &str) {
    let mut data = vec![0u8; 0x80 + 0x90];
    data[..SSEDINFO_MAGIC.len()].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Retained SSED";
    data[0x0c] = title.len() as u8;
    data[0x0d..0x0d + title.len()].copy_from_slice(title);
    data[0x4d] = 3;
    write_retained_ssedinfo_record(&mut data[0x80..0x80 + 0x30], 0x00, 100, 100, "HONMON.DIC");
    write_retained_ssedinfo_record(
        &mut data[0x80 + 0x30..0x80 + 0x60],
        0x03,
        300,
        300,
        "FHTITLE.DIC",
    );
    write_retained_ssedinfo_record(
        &mut data[0x80 + 0x60..0x80 + 0x90],
        0x91,
        200,
        200,
        "FHINDEX.DIC",
    );
    fs::write(root.join(filename), data).unwrap();

    let mut body = Vec::new();
    body.extend_from_slice(&SSED_ENTRY_MARKER);
    body.extend_from_slice(&body_jis("retained body"));
    fs::write(
        root.join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&body], 100, 100),
    )
    .unwrap();
    fs::write(
        root.join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&[b"retained alpha\x1f\x0a"], 300, 300),
    )
    .unwrap();

    let mut index_page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    index_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    index_page[2..4].copy_from_slice(&1u16.to_be_bytes());
    let mut pos = 4usize;
    write_simple_index_row(&mut index_page, &mut pos, &body_jis("あ"), 100, 0, 300, 0);
    fs::write(
        root.join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&[&index_page], 200, 200),
    )
    .unwrap();
}

fn write_retained_ssedinfo_record(
    rec: &mut [u8],
    component_type: u8,
    start: u32,
    end: u32,
    filename: &str,
) {
    rec[3] = component_type;
    rec[4..8].copy_from_slice(&start.to_be_bytes());
    rec[8..12].copy_from_slice(&end.to_be_bytes());
    rec[0x10] = filename.len() as u8;
    rec[0x11..0x11 + filename.len()].copy_from_slice(filename.as_bytes());
}

#[test]
fn lved_search_hits_resolve_to_preserved_content_html() {
    let dir = tempdir().unwrap();
    write_lved_search_fixture(dir.path());
    let package = LvedSqliteDriver.open(dir.path()).unwrap();
    let surfaces = package.home_surfaces().unwrap();
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::TitleIndexBrowse
            && surface.surface_id == "lved-list"
            && surface.status == NavigationStatus::Available
            && surface.target.is_some()
    }));
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::Info
            && surface.status == NavigationStatus::Available
            && surface.target.is_some()
    }));
    let info_home_target = surfaces
        .iter()
        .find(|surface| surface.kind == NavigationSurfaceKind::Info)
        .and_then(|surface| surface.target.as_ref())
        .unwrap();
    let info_home_view = package
        .render_target(info_home_target, &RenderOptions::default())
        .unwrap();
    assert_eq!(info_home_view.kind, ResolvedTargetKind::InfoPage);
    assert!(matches!(
        info_home_view.surface.as_ref().unwrap(),
        NavigationSurface::InfoPages { .. }
    ));
    let list_surface = package.open_surface("lved-list").unwrap();
    let list_items = match list_surface {
        NavigationSurface::TitleIndexBrowse { items, .. } => items,
        _ => panic!("expected LVED list title/index surface"),
    };
    assert_eq!(list_items.len(), 3);
    assert_eq!(list_items[0].label_text, "alpha subtitle");
    assert!(list_items[0].label_html.contains("lvcore://resource/"));
    assert!(!list_items[0].label_html.contains("src=\"AC6E.svg\""));
    assert!(matches!(
        list_items[0].target.decode().unwrap(),
        InternalTarget::LvedRow {
            table,
            row_id: 100,
            anchor: Some(anchor),
            query: None
        } if table == "content" && anchor == "body-anchor"
    ));
    let info_surface = package.open_surface("info").unwrap();
    let info_target = match info_surface {
        NavigationSurface::InfoPages { pages, .. } => pages[0].target.clone(),
        _ => panic!("expected LVED info pages surface"),
    };
    let info_view = package
        .render_target(&info_target, &RenderOptions::default())
        .unwrap();
    assert_eq!(info_view.kind, ResolvedTargetKind::InfoPage);
    assert_eq!(
        info_view.display_html.as_deref(),
        Some("<h1>Example Dictionary 第2版</h1>")
    );
    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
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
    assert!(page.hits[0].title_html.contains("lvcore://resource/"));
    assert!(!page.hits[0].title_html.contains("src=\"AC6E.svg\""));
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::LvedRow {
            table,
            row_id: 100,
            anchor: Some(_),
            query: None
        } if table == "content"
    ));

    let view = package
        .render_target(&page.hits[0].target, &RenderOptions::default())
        .unwrap();

    assert_eq!(view.kind, ResolvedTargetKind::EntryBody);
    assert_eq!(view.title.as_deref(), Some("Alpha"));
    let html = view.display_html.as_deref().unwrap();
    assert!(html.contains("<article><h1>Alpha</h1><p>body</p>"));
    assert!(html.contains("lvcore://resource/"));
    assert!(html.contains("lvcore://target/"));
    assert!(!html.contains("lved.dataid:101"));
    assert!(!html.contains("lved.info:help.html"));
    assert_eq!(view.links.len(), 2);
    assert!(view.links.iter().all(|link| {
        link.href == format!("lvcore://target/{}", link.token.as_str()) && html.contains(&link.href)
    }));
    assert!(view.links.iter().any(|link| matches!(
        link.token.decode().unwrap(),
        InternalTarget::LvedRow {
            table,
            row_id: 101,
            anchor: Some(anchor),
            query: None
        } if table == "content" && anchor == "jump"
    )));
    let help_token = view
        .links
        .iter()
        .find_map(|link| match link.token.decode().unwrap() {
            InternalTarget::LvedInfoPage {
                name,
                anchor: Some(anchor),
            } if name == "help.html" && anchor == "top" => Some(link.token.clone()),
            _ => None,
        })
        .expect("expected lved.info link to be routed through TargetToken");
    let help_view = package
        .render_target(&help_token, &RenderOptions::default())
        .unwrap();
    assert_eq!(help_view.kind, ResolvedTargetKind::InfoPage);
    assert_eq!(help_view.display_html.as_deref(), Some("<h1>Help</h1>"));
    assert_eq!(view.resources.len(), 2);
    assert!(view.capabilities.contains(&RenderCapability::Html));
    assert!(view.capabilities.contains(&RenderCapability::Images));
    assert!(view.capabilities.contains(&RenderCapability::Audio));
    assert!(
        view.resources
            .iter()
            .any(|resource| resource.kind == ResourceKind::Image)
    );
    assert!(
        view.resources
            .iter()
            .any(|resource| resource.kind == ResourceKind::Audio)
    );
    let audio = view
        .resources
        .iter()
        .find(|resource| resource.kind == ResourceKind::Audio)
        .unwrap();
    assert_eq!(audio.mime_type.as_deref(), Some("audio/mpeg"));
    assert_eq!(audio.byte_len, Some(4));
    assert_eq!(
        package.read_resource(&audio.token).unwrap(),
        b"ID3\x03".to_vec()
    );
    let image = view
        .resources
        .iter()
        .find(|resource| resource.kind == ResourceKind::Image)
        .unwrap();
    assert_eq!(image.mime_type.as_deref(), Some("image/svg+xml"));
    assert_eq!(image.byte_len, Some(6));
    assert_eq!(
        package.read_resource(&image.token).unwrap(),
        b"<svg/>".to_vec()
    );

    let window = package
        .resolve_target_window(
            &page.hits[0].target,
            Some(&SequenceHint::LvedListOrder),
            0,
            2,
            &RenderOptions::default(),
        )
        .unwrap();
    assert!(window.before.is_empty());
    assert_eq!(window.after.len(), 2);
    assert_eq!(window.after[0].title.as_deref(), Some("beta"));
    assert_eq!(window.after[1].title.as_deref(), Some("gamma"));

    let search_result_sequence = SearchResultSequence::new(
        list_items
            .into_iter()
            .map(|item| crate::sequence::SearchResultSequenceTarget {
                book_id: None,
                target: item.target,
                title: Some(item.label_text),
            })
            .collect(),
    )
    .unwrap()
    .encode()
    .unwrap();
    let search_window = package
        .resolve_target_window(
            &window.after[0].target,
            Some(&SequenceHint::SearchResults {
                value: search_result_sequence,
            }),
            1,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(search_window.before.len(), 1);
    assert_eq!(search_window.center.title.as_deref(), Some("beta"));
    assert_eq!(search_window.after.len(), 1);
    assert_eq!(search_window.after[0].title.as_deref(), Some("gamma"));

    let foreign_sequence =
        SearchResultSequence::new(vec![crate::sequence::SearchResultSequenceTarget {
            book_id: Some(BookId("LVED_SQLITE3:OTHER".to_owned())),
            target: window.after[0].target.clone(),
            title: Some("foreign beta".to_owned()),
        }])
        .unwrap()
        .encode()
        .unwrap();
    let foreign_window = package
        .resolve_target_window(
            &window.after[0].target,
            Some(&SequenceHint::SearchResults {
                value: foreign_sequence,
            }),
            1,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert!(foreign_window.before.is_empty());
    assert!(foreign_window.after.is_empty());
    assert!(
        foreign_window
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "search_results_sequence_book_mismatch")
    );
}

#[test]
fn lved_search_and_list_labels_are_sanitized_for_app_chrome() {
    let dir = tempdir().unwrap();
    write_lved_search_fixture(dir.path());
    {
        let connection = Connection::open(dir.path().join("main.data")).unwrap();
        apply_sqlcipher_key(&connection, "test-key").unwrap();
        connection
            .execute(
                "update list set title = ?1, titlesub = ?2 where id = 1",
                (
                    r#"<img class="icon lvcore-gaiji" src="AC6E.svg" onerror="bad()"><b>alpha</b><span class="scl_ps hostile">小</span><script>alert(1)</script>"#,
                    r#"<span class="hostile lvcore-subtitle">subtitle</span><img src="javascript:bad()">"#,
                ),
            )
            .unwrap();
    }

    let package = LvedSqliteDriver.open(dir.path()).unwrap();
    let list_surface = package.open_surface("lved-list").unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = list_surface else {
        panic!("expected LVED list title/index surface");
    };
    let list_html = &items[0].label_html;
    assert!(list_html.contains("<b>alpha</b>"));
    assert!(list_html.contains("lvcore://resource/"));
    assert!(!list_html.contains("<script"));
    assert!(!list_html.contains("onerror"));
    assert!(!list_html.contains("javascript:"));
    assert!(!list_html.contains("class=\"icon"));
    assert!(!list_html.contains("hostile"));
    assert!(list_html.contains(r#"<span class="lvcore-subtitle">"#));
    assert!(list_html.contains(r#"<span class="scl_ps">小</span>"#));

    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "alp".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    let hit_html = &page.hits[0].title_html;
    assert!(hit_html.contains("<b>alpha</b>"));
    assert!(hit_html.contains("lvcore://resource/"));
    assert!(!hit_html.contains("<script"));
    assert!(!hit_html.contains("onerror"));
    assert!(!hit_html.contains("javascript:"));
    assert!(!hit_html.contains("class=\"icon"));
    assert!(!hit_html.contains("hostile"));
    assert!(hit_html.contains(r#"<span class="scl_ps">小</span>"#));
}

#[test]
fn lved_search_normalizes_hiragana_queries_to_observed_katakana_keys() {
    let dir = tempdir().unwrap();
    write_lved_search_fixture(dir.path());
    {
        let connection = Connection::open(dir.path().join("main.data")).unwrap();
        apply_sqlcipher_key(&connection, "test-key").unwrap();
        connection
            .execute_batch(
                r#"
                insert into content values (103, 1, '<article><h1>あいかわらず</h1></article>', '');
                insert into list values (4, 103, 1, '', 'あいかわらず【相変わらず】', '');
                insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                  values (
                    4,
                    'アイカワラズ 相変ワラズ',
                    'ズラワ変相 ズラワカイア',
                    'ア イ カ ワ ラ ズ ∥ 相 変 ワ ラ ズ',
                    'ア イ カ ワ ラ ズ 【 相 変 ワ ラ ズ 】',
                    '',
                    '',
                    '∥アイカワラズ∥相変ワラズ∥'
                  );
                "#,
            )
            .unwrap();
    }

    let package = LvedSqliteDriver.open(dir.path()).unwrap();
    for mode in [
        SearchMode::Exact,
        SearchMode::Forward,
        SearchMode::Backward,
        SearchMode::Partial,
        SearchMode::FullText,
    ] {
        let query = match mode {
            SearchMode::Exact => "あいかわらず",
            SearchMode::Backward => "わらず",
            _ => "あい",
        };
        let page = package
            .search(&SearchQuery {
                scope: crate::search::SearchScope::CurrentBook {
                    book_id: package.metadata().book_id.clone(),
                },
                mode: mode.clone(),
                query: query.to_owned(),
                cursor: None,
                limit: 10,
                gaiji_policy: None,
            })
            .unwrap();
        assert!(
            page.hits
                .iter()
                .any(|hit| hit.title_text == "あいかわらず【相変わらず】"),
            "hiragana query {query:?} did not match katakana LVED key in mode {mode:?}"
        );
    }
}

#[test]
fn render_modes_are_explicit_for_preserved_lved_html() {
    let dir = tempdir().unwrap();
    write_lved_search_fixture(dir.path());
    let package = LvedSqliteDriver.open(dir.path()).unwrap();
    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "alp".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    let target = &page.hits[0].target;

    let basic = package
        .render_target(
            target,
            &RenderOptions {
                mode: RenderMode::BasicText,
                ..RenderOptions::default()
            },
        )
        .unwrap();
    assert!(basic.display_html.is_none());
    assert!(basic.basic_text.as_deref().unwrap().contains("Alpha"));
    assert!(basic.resources.is_empty());
    assert!(basic.links.is_empty());

    let generic = package
        .render_target(
            target,
            &RenderOptions {
                mode: RenderMode::GenericHtml,
                ..RenderOptions::default()
            },
        )
        .unwrap();
    let generic_html = generic.display_html.as_deref().unwrap();
    assert!(!generic_html.contains("lvcore://target/"));
    assert!(!generic_html.contains("lvcore://resource/"));
    assert!(generic_html.contains("#lvcore-target-"));
    assert!(generic_html.contains("data:image/svg+xml;base64,"));
    assert!(
        generic
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "generic_html_resources_inlined")
    );
    assert!(
        generic
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "generic_html_targets_fragmentized")
    );

    let debug = package
        .render_target(
            target,
            &RenderOptions {
                mode: RenderMode::Debug,
                ..RenderOptions::default()
            },
        )
        .unwrap();
    let debug_trace = debug.debug_trace.as_deref().unwrap();
    assert!(debug_trace.contains(r#""mode":"debug""#));
    assert!(debug_trace.contains(r#""has_display_html":true"#));
}

#[test]
fn visual_capabilities_are_derived_from_html_and_resources() {
    let target = TargetToken::new(&InternalTarget::Unsupported {
        reason: "synthetic".to_owned(),
    })
    .unwrap();
    let resource = ResourceToken::new(&InternalResource::PackageFile {
        path: "sound.mp3".to_owned(),
        resource_kind: ResourceKind::Audio,
    })
    .unwrap();
    let view = finalize_resolved_view(
        ResolvedTargetView {
            href: String::new(),
            kind: ResolvedTargetKind::EntryBody,
            target,
            title: None,
            display_html: Some(
                r#"<p>\(x+1\)</p><link rel="stylesheet" href="style.css">"#.to_owned(),
            ),
            basic_text: None,
            scroll_anchor: None,
            surface: None,
            resources: vec![ResourceRef {
                token: resource,
                kind: ResourceKind::Audio,
                label: None,
                href: None,
                mime_type: Some("audio/mpeg".to_owned()),
                byte_len: Some(3),
                diagnostics: Vec::new(),
            }],
            links: Vec::new(),
            capabilities: Vec::new(),
            diagnostics: Vec::new(),
            debug_trace: None,
        },
        &RenderOptions::default(),
    );

    assert!(view.capabilities.contains(&RenderCapability::Html));
    assert!(view.capabilities.contains(&RenderCapability::Css));
    assert!(view.capabilities.contains(&RenderCapability::MathJax));
    assert!(view.capabilities.contains(&RenderCapability::Audio));
}

#[test]
fn lved_protocol_router_preserves_observed_non_entry_hooks() {
    let dir = tempdir().unwrap();
    let payload = dir.path().join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        apply_sqlcipher_key(&connection, key).unwrap();
        connection
            .execute_batch(
                r#"
                create table content (id integer primary key, type integer, body text, media text);
                create table list (id integer primary key, refid integer, type integer, anchor text, title text, titlesub text);
                create table media (id integer primary key, name text, type integer, main blob);
                create table info (id integer primary key, name text, body text);
                create table binran (id integer primary key, name text, body text);
                insert into content values (
                  200,
                  1,
                  '<article>
                    <a href="lved.dataid.result:201#detail">result</a>
                    <a href="lved.dataid202#legacy">legacy</a>
                    <a href="lved.dataid.dict.STEDABBR:300#cross">dict</a>
                    <a href="lved.contentlink:BUREI.400#note">contentlink</a>
                    <a href="lved.binran:usage.html#top">binran</a>
                    <a href="lved.dict.GENIUSE6:pictlink.picture.html#map">picture dict</a>
                    <a href="lved.addr=00029154:0042">addr</a>
                    <a href="lved.bookmark:C001">bookmark</a>
                    <a href="050000/0000">relative appendix</a>
                    <a href="lved.imag00001234:0567:0000002c">image address</a>
                    <img src="lved.image:fig01.png">
                    <a href="lved.pdf:manual.pdf">pdf</a>
                    <script src="./MathJax/MathJax.js"></script>
                  </article>',
                  ''
                );
                insert into content values (201, 1, '<article>result detail</article>', '');
                insert into content values (202, 1, '<article>legacy detail</article>', '');
                insert into list values (1, 200, 1, '', 'router', '');
                insert into media values (1, 'fig01', 4, X'89504E470D0A1A0A');
                insert into media values (2, 'manual', 6, X'255044462D312E37');
                insert into info values (1, 'picture.html', '<h1>Picture</h1>');
                insert into binran values (1, 'usage.html', '<h1>Binran</h1>');
                "#,
            )
            .unwrap();
    }
    fs::write(dir.path().join("main.key"), key).unwrap();

    let package = LvedSqliteDriver.open(dir.path()).unwrap();
    let surfaces = package.home_surfaces().unwrap();
    assert!(surfaces.iter().any(|surface| {
        surface.surface_id == "binran"
            && surface.kind == NavigationSurfaceKind::Info
            && surface.status == NavigationStatus::Available
            && surface.target.is_some()
    }));
    let binran_surface = package.open_surface("binran").unwrap();
    let NavigationSurface::InfoPages { pages, .. } = binran_surface else {
        panic!("expected binran named page surface");
    };
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].label_text, "Binran");
    assert!(matches!(
        pages[0].target.decode().unwrap(),
        InternalTarget::LvedNamedPage {
            table,
            name,
            anchor: None,
        } if table == "binran" && name == "usage.html"
    ));

    let target = TargetToken::new(&InternalTarget::LvedRow {
        table: "content".to_owned(),
        row_id: 200,
        anchor: None,
        query: None,
    })
    .unwrap();
    let view = package
        .render_target(&target, &RenderOptions::default())
        .unwrap();
    let html = view.display_html.as_deref().unwrap();

    for raw in [
        "lved.dataid.result:",
        "lved.dataid202",
        "lved.dataid.dict.",
        "lved.contentlink:",
        "lved.binran:",
        "lved.dict.",
        "lved.addr=",
        "lved.bookmark:",
        "050000/0000",
        "lved.imag00001234:0567:0000002c",
        "lved.image:",
        "lved.pdf:",
    ] {
        assert!(!html.contains(raw), "{raw} leaked through normalized HTML");
    }
    assert_eq!(
        view.resources
            .iter()
            .map(|resource| resource.kind)
            .collect::<Vec<_>>(),
        vec![ResourceKind::Image, ResourceKind::Pdf]
    );
    assert_eq!(
        view.links.iter().map(|link| link.kind).collect::<Vec<_>>(),
        vec![
            TargetKind::LvedRow,
            TargetKind::LvedRow,
            TargetKind::LvedCrossBook,
            TargetKind::LvedCrossBook,
            TargetKind::LvedNamedPage,
            TargetKind::LvedInfoPage,
            TargetKind::LvedAddress,
            TargetKind::LvedViewerHook,
            TargetKind::LvedViewerHook,
            TargetKind::LvedViewerHook,
        ]
    );

    let binran = view
        .links
        .iter()
        .find(|link| link.kind == TargetKind::LvedNamedPage)
        .unwrap();
    let binran_view = package
        .render_target(&binran.token, &RenderOptions::default())
        .unwrap();
    assert_eq!(binran_view.kind, ResolvedTargetKind::InfoPage);
    assert_eq!(binran_view.display_html.as_deref(), Some("<h1>Binran</h1>"));

    let picture = view
        .links
        .iter()
        .find(|link| link.label == "lved.dict.GENIUSE6:pictlink.picture.html#map")
        .unwrap();
    assert!(matches!(
        picture.token.decode().unwrap(),
        InternalTarget::LvedInfoPage { name, anchor }
            if name == "picture.html" && anchor.as_deref() == Some("map")
    ));
    let picture_view = package
        .render_target(&picture.token, &RenderOptions::default())
        .unwrap();
    assert_eq!(picture_view.kind, ResolvedTargetKind::InfoPage);
    assert_eq!(picture_view.scroll_anchor.as_deref(), Some("map"));
    assert_eq!(
        picture_view.display_html.as_deref(),
        Some("<h1>Picture</h1>")
    );

    let cross = view
        .links
        .iter()
        .find(|link| link.kind == TargetKind::LvedCrossBook)
        .unwrap();
    let cross_view = package
        .render_target(&cross.token, &RenderOptions::default())
        .unwrap();
    assert_eq!(cross_view.kind, ResolvedTargetKind::Unsupported);
    assert!(
        cross_view
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "lved_cross_book_deferred")
    );

    let address = view
        .links
        .iter()
        .find(|link| link.kind == TargetKind::LvedAddress)
        .unwrap();
    assert!(matches!(
        address.token.decode().unwrap(),
        InternalTarget::LvedAddress {
            block: 0x0002_9154,
            offset: 0x0042,
            ..
        }
    ));
    let address_view = package
        .render_target(&address.token, &RenderOptions::default())
        .unwrap();
    assert_eq!(address_view.kind, ResolvedTargetKind::Unsupported);
    assert!(
        address_view
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "lved_address_deferred")
    );

    let relative = view
        .links
        .iter()
        .find(|link| link.label == "050000/0000")
        .unwrap();
    assert!(matches!(
        relative.token.decode().unwrap(),
        InternalTarget::LvedViewerHook { hook, value }
            if hook == "relative-appendix" && value == "050000/0000"
    ));
    assert!(
        relative
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "lved_relative_viewer_hook_deferred" })
    );
    let relative_view = package
        .render_target(&relative.token, &RenderOptions::default())
        .unwrap();
    assert_eq!(relative_view.kind, ResolvedTargetKind::Unsupported);
    assert!(
        relative_view
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "lved_viewer_hook_deferred")
    );

    let image_address = view
        .links
        .iter()
        .find(|link| link.label == "lved.imag00001234:0567:0000002c")
        .unwrap();
    assert!(matches!(
        image_address.token.decode().unwrap(),
        InternalTarget::LvedViewerHook { hook, value }
            if hook == "image-address" && value == "lved.imag00001234:0567:0000002c"
    ));
    assert!(
        image_address
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "lved_image_address_hook_deferred" })
    );
}

#[test]
fn lved_addr_links_resolve_to_nearby_content_anchors() {
    let dir = tempdir().unwrap();
    let payload = dir.path().join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        apply_sqlcipher_key(&connection, key).unwrap();
        connection
            .execute_batch(
                r#"
                create table content (id integer primary key, type integer, body, media text);
                create table list (id integer primary key, refid integer, type integer, anchor text, title text, titlesub text);
                insert into content values (
                  42,
                  1,
                  '<article>
                    <a href="lved.addr00000001:0004">addr resolved</a>
                    <a href="lved.addr00001234:0567">addr unresolved</a>
                  </article>',
                  ''
                );
                insert into content values (
                  99,
                  1,
                  cast('<article><a name="000000010020"></a><p>Addr target</p></article>' as blob),
                  ''
                );
                insert into list values (1, 42, 1, '', 'source', '');
                insert into list values (2, 99, 1, '', 'target', '');
                "#,
            )
            .unwrap();
    }
    fs::write(dir.path().join("main.key"), key).unwrap();

    let package = LvedSqliteDriver.open(dir.path()).unwrap();
    let source = TargetToken::new(&InternalTarget::LvedRow {
        table: "content".to_owned(),
        row_id: 42,
        anchor: None,
        query: None,
    })
    .unwrap();
    let view = package
        .render_target(&source, &RenderOptions::default())
        .unwrap();
    let html = view.display_html.as_deref().unwrap();
    assert!(!html.contains("lved.addr"));

    let resolved = view
        .links
        .iter()
        .find(|link| link.label == "lved.addr00000001:0004")
        .unwrap();
    assert_eq!(resolved.kind, TargetKind::LvedRow);
    assert_eq!(
        resolved
            .attributes
            .get("lved_original_href")
            .map(String::as_str),
        Some("lved.addr00000001:0004")
    );
    assert_eq!(
        resolved
            .attributes
            .get("lved_addr_delta")
            .map(String::as_str),
        Some("28")
    );
    assert!(matches!(
        resolved.token.decode().unwrap(),
        InternalTarget::LvedRow {
            table,
            row_id: 99,
            anchor: Some(anchor),
            query: None,
        } if table == "content" && anchor == "000000010020"
    ));
    let target_view = package
        .render_target(&resolved.token, &RenderOptions::default())
        .unwrap();
    assert_eq!(target_view.kind, ResolvedTargetKind::EntryBody);
    assert_eq!(target_view.scroll_anchor.as_deref(), Some("000000010020"));
    assert!(
        target_view
            .display_html
            .as_deref()
            .is_some_and(|body| body.contains("Addr target"))
    );

    let unresolved = view
        .links
        .iter()
        .find(|link| link.label == "lved.addr00001234:0567")
        .unwrap();
    assert_eq!(unresolved.kind, TargetKind::LvedAddress);
    assert!(matches!(
        unresolved.token.decode().unwrap(),
        InternalTarget::LvedAddress {
            block: 0x0000_1234,
            offset: 0x0567,
            ..
        }
    ));
    assert!(
        unresolved
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "lved_address_deferred")
    );
}

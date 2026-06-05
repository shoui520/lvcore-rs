use super::common::*;

#[test]
fn ssed_simple_title_index_surface_resolves_entry_targets() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture("alpha", 1, 2, 13, 0)),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let surface = package.open_surface("title-index").unwrap();
    let targets = surface.actionable_targets();
    assert_eq!(targets.len(), 1);
    assert!(
        matches!(
            targets[0].sequence_hint.as_ref(),
            Some(lvcore::SequenceHint::TitleIndexOrder {
                value,
                cursor: Some(cursor),
            }) if value == "title-index" && cursor == "FHINDEX.DIC:0"
        ),
        "title/index items should carry a cursor for fast continuous windows"
    );
    let lvcore::NavigationSurface::TitleIndexBrowse { items, .. } = surface else {
        panic!("title-index should open as a title/index browse surface");
    };

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label_text, "alpha");
    assert_eq!(
        items[0].target.decode().unwrap(),
        InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 1,
            offset: 2,
        }
    );
}

#[test]
fn title_index_surfaces_are_cursor_paged_by_backend() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0abeta\x1f\x0agamma\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("alpha", 1, 2, 13, 0),
            ("beta", 1, 4, 13, 7),
            ("gamma", 1, 6, 13, 12),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let first = package.open_surface_page("title-index", None, 2).unwrap();
    let NavigationSurface::TitleIndexBrowse {
        items, next_cursor, ..
    } = first
    else {
        panic!("expected paged SSED title/index browse");
    };
    assert_eq!(
        items
            .iter()
            .map(|item| item.label_text.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
    assert_eq!(
        items[0].target.decode().unwrap(),
        InternalTarget::SsedBoundedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 1,
            offset: 2,
            end_block: 1,
            end_offset: 4,
        }
    );
    assert_eq!(next_cursor.as_deref(), Some("2"));

    let second = package
        .open_surface_page("title-index", next_cursor.as_deref(), 2)
        .unwrap();
    let NavigationSurface::TitleIndexBrowse {
        items, next_cursor, ..
    } = second
    else {
        panic!("expected second SSED title/index page");
    };
    assert_eq!(items[0].label_text, "gamma");
    assert!(next_cursor.is_none());
}

#[test]
fn title_index_browse_does_not_apply_backward_body_bounds() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0abeta\x1f\x0agamma\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("alpha", 1, 2, 13, 0),
            ("beta", 1, 8, 13, 7),
            ("gamma", 1, 4, 13, 12),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let surface = package.open_surface("title-index").unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = surface else {
        panic!("expected SSED title/index browse");
    };

    assert_eq!(
        items[0].target.decode().unwrap(),
        InternalTarget::SsedBoundedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 1,
            offset: 2,
            end_block: 1,
            end_offset: 8,
        }
    );
    assert_eq!(
        items[1].target.decode().unwrap(),
        InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 1,
            offset: 8,
        }
    );
}

#[test]
fn title_index_browse_does_not_apply_huge_sparse_body_bounds() {
    let dir = tempdir().unwrap();
    let record_start = 0x80;
    let mut catalog = vec![0u8; record_start + 3 * 0x30];
    catalog[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Sparse Bounds Fixture";
    catalog[0x0c] = title.len() as u8;
    catalog[0x0d..0x0d + title.len()].copy_from_slice(title);
    catalog[0x4d] = 3;
    write_record(
        &mut catalog[record_start..record_start + 0x30],
        0x00,
        1,
        400,
        "HONMON.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x30..record_start + 0x60],
        0x05,
        13,
        14,
        "FHTITLE.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x60..record_start + 0x90],
        0x91,
        15,
        15,
        "FHINDEX.DIC",
    );
    fs::write(dir.path().join("DICT.IDX"), catalog).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0abeta\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("alpha", 1, 2, 13, 0),
            ("beta", 300, 0, 13, 7),
        ])),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let surface = package.open_surface("title-index").unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = surface else {
        panic!("expected SSED title/index browse");
    };

    assert_eq!(
        items[0].target.decode().unwrap(),
        InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 1,
            offset: 2,
        }
    );
}

#[test]
fn title_index_browse_prefers_forward_rows_over_backward_search_rows() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_forward_and_backward_indexes(),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0abeta\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("BHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0abeta\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("alpha", 1, 2, 13, 0),
            ("beta", 1, 4, 13, 7),
        ])),
    )
    .unwrap();
    fs::write(
        dir.path().join("BHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("ahpla", 1, 2, 15, 0),
            ("ateb", 1, 4, 15, 7),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let surface = package.open_surface("title-index").unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = surface else {
        panic!("expected SSED title/index browse");
    };
    assert_eq!(
        items
            .iter()
            .map(|item| item.label_text.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta"]
    );

    let backward = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Backward,
            query: "ha".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(backward.hits.len(), 1);
    assert_eq!(backward.hits[0].title_text, "alpha");
}

#[test]
fn ssed_search_hits_render_with_index_body_boundaries() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"abcdef"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0aalpine\x1f\x0abeta\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("alpha", 1, 2, 13, 0),
            ("alpine", 1, 4, 13, 7),
            ("beta", 1, 6, 13, 14),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "al".to_owned(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedIndexAddress {
            component: "HONMON.DIC".to_owned(),
            block: 1,
            offset: 2,
            index_component: "FHINDEX.DIC".to_owned(),
        }
    );
    let input = package
        .renderer_input_for_target(&page.hits[0].target)
        .unwrap();
    let RendererInput::HcSsedStream { length, .. } = input else {
        panic!("search hit should resolve to SSED renderer input");
    };
    assert_eq!(length, Some(2));
    assert_eq!(page.next_cursor.as_deref(), Some("1"));
}

#[test]
fn title_index_browse_skips_backward_components_before_opening_them() {
    let dir = tempdir().unwrap();
    let record_start = 0x80;
    let mut catalog = vec![0u8; record_start + 5 * 0x30];
    catalog[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Backward First Fixture";
    catalog[0x0c] = title.len() as u8;
    catalog[0x0d..0x0d + title.len()].copy_from_slice(title);
    catalog[0x4d] = 5;
    write_record(
        &mut catalog[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        "HONMON.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x30..record_start + 0x60],
        0x07,
        15,
        16,
        "BHTITLE.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x60..record_start + 0x90],
        0x71,
        18,
        18,
        "BHINDEX.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x90..record_start + 0xc0],
        0x05,
        13,
        14,
        "FHTITLE.DIC",
    );
    write_record(
        &mut catalog[record_start + 0xc0..record_start + 0xf0],
        0x91,
        17,
        17,
        "FHINDEX.DIC",
    );

    fs::write(dir.path().join("DICT.IDX"), catalog).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture("alpha", 1, 2, 13, 0)),
    )
    .unwrap();
    fs::write(dir.path().join("BHINDEX.DIC"), b"not an SSEDDATA stream").unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let surface = package.open_surface("title-index").unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = surface else {
        panic!("expected SSED title/index browse");
    };

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label_text, "alpha");
    let window = package
        .resolve_target_window(
            &items[0].target,
            Some(&lvcore::SequenceHint::TitleIndexOrder {
                value: "title-index".to_owned(),
                cursor: None,
            }),
            0,
            0,
            &RenderOptions::default(),
        )
        .unwrap();
    assert!(
        !window
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_index_component_decode_failed")
    );
}

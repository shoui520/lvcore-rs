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
        })
        .unwrap();
    assert_eq!(backward.hits.len(), 1);
    assert_eq!(backward.hits[0].title_text, "alpha");
}

use std::fs;
use std::path::Path;

use lvcore::{
    BookLibrary, Capability, DriverRegistry, FormatFamily, GaijiPolicy, GaijiSourcePreference,
    InternalResource, InternalTarget, NavigationStatus, NavigationSurfaceKind, RenderOptions,
    ResolvedTargetKind, ResourceKind, ResourceToken, SSEDDATA_MAGIC, SSEDINFO_MAGIC, SearchMode,
    SearchQuery, SearchScope, StorageBackend, TargetToken, VisualBody,
};
use tempfile::tempdir;

#[test]
fn driver_registry_detects_first_class_families() {
    let lved = tempdir().unwrap();
    fs::write(lved.path().join("main.data"), b"encrypted").unwrap();
    fs::write(lved.path().join("dict.key"), b"key").unwrap();

    let multiview = tempdir().unwrap();
    fs::write(multiview.path().join("menuData.xml"), b"<menu/>").unwrap();
    fs::write(multiview.path().join("blvdat"), b"payload").unwrap();

    let hourei = tempdir().unwrap();
    fs::create_dir_all(hourei.path().join("_DataBase")).unwrap();
    fs::write(hourei.path().join("_DataBase/hore_base.db"), b"").unwrap();
    fs::write(hourei.path().join("_DataBase/hore_search_a.db"), b"").unwrap();
    fs::write(hourei.path().join("_DataBase/horejo_base.db"), b"").unwrap();

    let registry = DriverRegistry::default();
    assert_eq!(
        registry.detect(lved.path()).unwrap()[0].format_family,
        FormatFamily::LvedSqlite3
    );
    assert_eq!(
        registry.detect(multiview.path()).unwrap()[0].format_family,
        FormatFamily::LvlMultiView
    );
    assert_eq!(
        registry.detect(hourei.path()).unwrap()[0].format_family,
        FormatFamily::Hourei
    );
}

#[test]
fn library_routes_all_book_search_without_unhandled_exceptions() {
    let ssed = tempdir().unwrap();
    fs::write(ssed.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();

    let lved = tempdir().unwrap();
    fs::write(lved.path().join("main.data"), b"encrypted").unwrap();
    fs::write(lved.path().join("dict.key"), b"key").unwrap();

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

    let page = library
        .search(&SearchQuery {
            scope: SearchScope::AllBooks,
            mode: SearchMode::Forward,
            query: "test".to_owned(),
            cursor: None,
            limit: 10,
        })
        .unwrap();
    assert!(page.hits.is_empty());
    assert_eq!(page.diagnostics.len(), 2);
    assert!(
        page.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.context.contains_key("book_id"))
    );
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
            scope: SearchScope::SelectedBooks(vec![
                ssed_id,
                lvcore::BookId("missing-book".to_owned()),
            ]),
            mode: SearchMode::Exact,
            query: "test".to_owned(),
            cursor: None,
            limit: 10,
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
    assert_eq!(view.kind, ResolvedTargetKind::Deferred);

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
fn target_tokens_are_frontend_safe_and_round_trippable() {
    let target = InternalTarget::LvedRow {
        table: "content".to_owned(),
        row_id: 42,
        anchor: Some("main".to_owned()),
    };
    let token = TargetToken::new(&target).unwrap();
    assert_eq!(token.decode().unwrap(), target);
}

#[test]
fn ssed_home_surfaces_are_capability_based() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(dir.path().join("MENU.DIC"), b"").unwrap();
    fs::write(dir.path().join("Panels.xml"), b"").unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let metadata = package.metadata();
    assert_eq!(metadata.format_family, FormatFamily::Ssed);
    assert!(metadata.capabilities.contains(&Capability::HcRenderInput));

    let surfaces = package.home_surfaces().unwrap();
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::Menu && surface.status == NavigationStatus::Available
    }));
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::Panel
            && surface.status == NavigationStatus::Available
    }));
}

#[test]
fn dense_honmon_targets_do_not_render_as_raw_numeric_anchors() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = TargetToken::new(&InternalTarget::SsedDenseAnchor {
        anchor: "00100050".to_owned(),
        resolver_hint: Some("vlpljbl".to_owned()),
    })
    .unwrap();

    let body = package.visual_body_for_target(&token).unwrap();
    assert!(matches!(body, VisualBody::Unsupported { .. }));
    assert!(!serde_json::to_string(&body).unwrap().contains("00100050"));

    let view = package
        .render_target(&token, &RenderOptions::default())
        .unwrap();
    assert!(view.display_html.is_none());
    assert_eq!(view.kind, ResolvedTargetKind::Unsupported);
    assert!(
        view.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "dense_honmon_dereference_required")
    );
}

#[test]
fn continuous_view_api_returns_typed_deferred_window() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();

    let window = package
        .resolve_target_window(&token, None, 2, 3, &RenderOptions::default())
        .unwrap();
    assert!(window.before.is_empty());
    assert!(window.after.is_empty());
    assert!(
        window
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "sequence_deferred")
    );
}

#[test]
fn ssed_address_targets_resolve_through_catalog_components() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"abcdef"),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "honmon.dic".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();

    let body = package.visual_body_for_target(&token).unwrap();
    assert_eq!(
        body,
        VisualBody::SsedStream {
            component: "HONMON.DIC".to_owned(),
            offset: 2,
            length: None,
        }
    );
}

#[test]
fn render_target_uses_resolved_visual_body_contract() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"abcdef"),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();
    let options = RenderOptions {
        include_debug_trace: true,
        ..RenderOptions::default()
    };

    let view = package.render_target(&token, &options).unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::Deferred);
    assert!(view.display_html.is_none());
    assert!(
        view.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "hc_render_deferred")
    );
    let debug_trace = view.debug_trace.as_deref().unwrap_or_default();
    assert!(debug_trace.contains("HONMON.DIC"));
    assert!(debug_trace.contains("\"offset\":2"));
}

#[test]
fn ssed_address_targets_report_missing_declared_components() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();

    let body = package.visual_body_for_target(&token).unwrap();
    let VisualBody::Unsupported { diagnostics, .. } = body else {
        panic!("missing component must not produce renderable body");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_component_file_missing")
    );
}

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
fn ssed_simple_index_search_returns_title_backed_hits() {
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

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
            mode: SearchMode::Forward,
            query: "alp".to_owned(),
            cursor: None,
            limit: 10,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "alpha");
    assert_eq!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 1,
            offset: 2,
        }
    );
}

#[test]
fn ssed_simple_index_search_does_not_limit_candidates_before_filtering() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"beta\x1f\x0aalpha\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("beta", 1, 2, 13, 0),
            ("alpha", 1, 4, 13, 6),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
            mode: SearchMode::Exact,
            query: "alpha".to_owned(),
            cursor: None,
            limit: 1,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "alpha");
    assert_eq!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 1,
            offset: 4,
        }
    );
}

#[test]
fn ssed_simple_index_targets_preserve_declared_honmon_component_name() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_honmon("HONMON.DIN"),
    )
    .unwrap();
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
    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
            mode: SearchMode::Exact,
            query: "alpha".to_owned(),
            cursor: None,
            limit: 10,
        })
        .unwrap();

    assert_eq!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedAddress {
            component: "HONMON.DIN".to_owned(),
            block: 1,
            offset: 2,
        }
    );
}

#[test]
fn ssed_title_index_sequence_returns_before_and_after_views() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"0123456789"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0abeta\x1f\x0agamma\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("alpha", 1, 0, 13, 0),
            ("beta", 1, 2, 13, 7),
            ("gamma", 1, 4, 13, 13),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();

    let window = package
        .resolve_target_window(
            &target,
            Some(&lvcore::SequenceHint::TitleIndexOrder(
                "title-index".to_owned(),
            )),
            1,
            1,
            &RenderOptions::default(),
        )
        .unwrap();

    assert_eq!(window.center.title.as_deref(), Some("beta"));
    assert_eq!(window.before.len(), 1);
    assert_eq!(window.before[0].title.as_deref(), Some("alpha"));
    assert_eq!(window.after.len(), 1);
    assert_eq!(window.after[0].title.as_deref(), Some("gamma"));
    assert!(
        !window
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "sequence_deferred")
    );
}

#[test]
fn gaiji_policy_is_backend_owned_and_reorderable() {
    let policy = GaijiPolicy {
        priority: vec![
            GaijiSourcePreference::ExternalResource,
            GaijiSourcePreference::Unicode,
            GaijiSourcePreference::Ga16Bitmap,
        ],
    };
    assert_eq!(policy.priority[0], GaijiSourcePreference::ExternalResource);
}

#[test]
fn ssed_detection_uses_actual_idx_magic() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("fake.idx"), b"not-ssed").unwrap();
    assert!(
        DriverRegistry::default()
            .detect(dir.path())
            .unwrap()
            .is_empty()
    );

    fs::write(dir.path().join("real.IDX"), ssedinfo_fixture()).unwrap();
    assert_eq!(
        DriverRegistry::default().detect(dir.path()).unwrap()[0].format_family,
        FormatFamily::Ssed
    );
}

#[test]
fn payload_file_detection_opens_parent_package() {
    let dir = tempdir().unwrap();
    let payload = dir.path().join("main.data");
    fs::write(&payload, b"encrypted").unwrap();
    fs::write(dir.path().join("dict.key"), b"key").unwrap();

    let package = DriverRegistry::default().open_best(&payload).unwrap();
    assert_eq!(package.metadata().format_family, FormatFamily::LvedSqlite3);
    assert_eq!(package.root(), dir.path());
}

#[test]
fn casefolded_paths_find_real_casing() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("HONMON.DIN"), b"body").unwrap();
    let storage = lvcore::DirectoryStorage::new(dir.path());
    let resolved = storage
        .resolve_casefolded(Path::new("honmon.din"))
        .unwrap()
        .unwrap();
    assert_eq!(resolved.file_name().unwrap(), "HONMON.DIN");
}

#[test]
fn package_file_resources_resolve_and_read_with_preserved_casing() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::create_dir(dir.path().join("Templates")).unwrap();
    fs::write(dir.path().join("Templates/B123.SVG"), b"<svg/>").unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = ResourceToken::new(&InternalResource::PackageFile {
        path: "templates/b123.svg".to_owned(),
        resource_kind: ResourceKind::Template,
    })
    .unwrap();

    let resource = package.resolve_resource(&token).unwrap();
    assert_eq!(resource.kind, ResourceKind::Template);
    assert_eq!(resource.label.as_deref(), Some("B123.SVG"));
    assert!(
        resource
            .href
            .as_deref()
            .unwrap_or_default()
            .starts_with("lvcore://resource/")
    );
    assert!(resource.diagnostics.is_empty());
    assert_eq!(package.read_resource(&token).unwrap(), b"<svg/>");
}

#[test]
fn resource_targets_render_as_media_resource_views() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::create_dir(dir.path().join("Templates")).unwrap();
    fs::write(dir.path().join("Templates/B123.SVG"), b"<svg/>").unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let resource = ResourceToken::new(&InternalResource::PackageFile {
        path: "Templates/B123.SVG".to_owned(),
        resource_kind: ResourceKind::Template,
    })
    .unwrap();
    let target = TargetToken::new(&InternalTarget::Resource { resource }).unwrap();

    let view = package
        .render_target(&target, &RenderOptions::default())
        .unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::MediaResource);
    assert_eq!(view.resources.len(), 1);
    assert_eq!(view.resources[0].kind, ResourceKind::Template);
    assert!(view.resources[0].href.is_some());
}

fn ssedinfo_fixture() -> Vec<u8> {
    ssedinfo_fixture_with_honmon("HONMON.DIC")
}

fn ssedinfo_fixture_with_honmon(honmon_filename: &str) -> Vec<u8> {
    let record_start = 0x80;
    let mut data = vec![0u8; record_start + 5 * 0x30];
    data[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Fixture Book";
    data[0x0c] = title.len() as u8;
    data[0x0d..0x0d + title.len()].copy_from_slice(title);
    data[0x4d] = 5;
    write_record(
        &mut data[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        honmon_filename,
    );
    write_record(
        &mut data[record_start + 0x30..record_start + 0x60],
        0x01,
        11,
        12,
        "MENU.DIC",
    );
    write_record(
        &mut data[record_start + 0x60..record_start + 0x90],
        0x05,
        13,
        14,
        "FHTITLE.DIC",
    );
    write_record(
        &mut data[record_start + 0x90..record_start + 0xc0],
        0x91,
        15,
        16,
        "FHINDEX.DIC",
    );
    write_record(
        &mut data[record_start + 0xc0..record_start + 0xf0],
        0xf2,
        17,
        18,
        "GA16HALF",
    );
    data
}

fn write_record(rec: &mut [u8], component_type: u8, start: u32, end: u32, filename: &str) {
    rec[3] = component_type;
    rec[4..8].copy_from_slice(&start.to_be_bytes());
    rec[8..12].copy_from_slice(&end.to_be_bytes());
    rec[0x10] = filename.len() as u8;
    rec[0x11..0x11 + filename.len()].copy_from_slice(filename.as_bytes());
}

fn sseddata_literal_fixture(literals: &[u8]) -> Vec<u8> {
    let chunk_offset = 0x44usize;
    let mut data = vec![0u8; chunk_offset];
    data[..8].copy_from_slice(SSEDDATA_MAGIC);
    data[0x0f] = 1;
    data[0x16..0x18].copy_from_slice(&1u16.to_be_bytes());
    data[0x18..0x1c].copy_from_slice(&1u32.to_be_bytes());
    data[0x1c..0x20].copy_from_slice(&1u32.to_be_bytes());
    data[0x40..0x44].copy_from_slice(&(chunk_offset as u32).to_be_bytes());
    data.extend_from_slice(&[0, 0]);
    data.extend_from_slice(&(literals.len() as u16).to_be_bytes());
    data.push(0);
    for literal in literals {
        data.extend_from_slice(&[0, 0, *literal]);
    }
    data
}

fn simple_index_fixture(
    key: &str,
    body_block: u32,
    body_offset: u16,
    title_block: u32,
    title_offset: u16,
) -> Vec<u8> {
    simple_index_fixture_rows(&[(key, body_block, body_offset, title_block, title_offset)])
}

fn simple_index_fixture_rows(rows: &[(&str, u32, u16, u32, u16)]) -> Vec<u8> {
    let mut page = vec![0u8; 2048];
    page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    page[2..4].copy_from_slice(&(rows.len() as u16).to_be_bytes());
    let mut pos = 4usize;
    for (key, body_block, body_offset, title_block, title_offset) in rows {
        let key = jis_fullwidth_ascii_key(key);
        page[pos] = key.len() as u8;
        pos += 1;
        page[pos..pos + key.len()].copy_from_slice(&key);
        pos += key.len();
        page[pos..pos + 4].copy_from_slice(&body_block.to_be_bytes());
        page[pos + 4..pos + 6].copy_from_slice(&body_offset.to_be_bytes());
        page[pos + 6..pos + 10].copy_from_slice(&title_block.to_be_bytes());
        page[pos + 10..pos + 12].copy_from_slice(&title_offset.to_be_bytes());
        pos += 12;
    }
    page
}

fn jis_fullwidth_ascii_key(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for byte in text.bytes() {
        if (0x21..=0x7e).contains(&byte) {
            out.extend_from_slice(&[0x23, byte]);
        } else if byte == b' ' {
            out.extend_from_slice(&[0x21, 0x21]);
        } else {
            out.push(byte);
        }
    }
    out
}

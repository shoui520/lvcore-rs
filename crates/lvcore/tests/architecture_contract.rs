use std::fs;
use std::io::Write;
use std::path::Path;

use lvcore::{
    BodySourceKind, BookLibrary, Capability, DriverRegistry, FormatFamily, GaijiPolicy,
    GaijiSourcePreference, InternalResource, InternalTarget, NavigationStatus, NavigationSurface,
    NavigationSurfaceKind, RenderMode, RenderOptions, RendererInput, ResolvedTargetKind,
    ResourceKind, ResourceToken, SSEDDATA_MAGIC, SSEDINFO_MAGIC, SearchMode, SearchQuery,
    SearchScope, StorageBackend, TargetKind, TargetToken, VisualBody,
};
use rusqlite::Connection;
use tempfile::tempdir;
use zip::unstable::write::FileOptionsExt;
use zip::write::{SimpleFileOptions, ZipWriter};

#[test]
fn driver_registry_detects_first_class_families() {
    let lved = tempdir().unwrap();
    write_minimal_lved_sqlite_fixture(lved.path());

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
fn multiview_container_wins_over_retained_ssed_facade() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(dir.path().join("menuData.xml"), b"<menu/>").unwrap();
    fs::write(dir.path().join("blvdat"), b"payload").unwrap();

    let registry = DriverRegistry::default();
    assert_eq!(
        registry.detect(dir.path()).unwrap()[0].format_family,
        FormatFamily::LvlMultiView
    );
    assert_eq!(
        registry
            .open_best(dir.path())
            .unwrap()
            .metadata()
            .format_family,
        FormatFamily::LvlMultiView
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
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
            mode: SearchMode::Forward,
            query: "まえ".to_owned(),
            cursor: None,
            limit: 10,
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
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
            mode: SearchMode::Partial,
            query: "body".to_owned(),
            cursor: None,
            limit: 1,
        })
        .unwrap();
    assert_eq!(first.hits[0].title_text, "まえがき");
    assert_eq!(first.next_cursor.as_deref(), Some("1"));
    let second = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
            mode: SearchMode::Partial,
            query: "body".to_owned(),
            cursor: first.next_cursor,
            limit: 1,
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
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
            mode: SearchMode::Forward,
            query: "民".to_owned(),
            cursor: None,
            limit: 10,
        })
        .unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "民法");

    let first = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
            mode: SearchMode::Partial,
            query: "本文".to_owned(),
            cursor: None,
            limit: 1,
        })
        .unwrap();
    assert_eq!(first.hits[0].title_text, "民法");
    assert_eq!(first.next_cursor.as_deref(), Some("1"));
    let second = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
            mode: SearchMode::Partial,
            query: "本文".to_owned(),
            cursor: first.next_cursor,
            limit: 1,
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
            scope: SearchScope::SelectedBooks(vec![first_id.clone(), second_id.clone()]),
            mode: SearchMode::Exact,
            query: "shared".to_owned(),
            cursor: None,
            limit: 1,
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
            scope: SearchScope::SelectedBooks(vec![first_id, second_id]),
            mode: SearchMode::Exact,
            query: "shared".to_owned(),
            cursor: first_page.next_cursor,
            limit: 1,
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
    let metadata = library
        .metadata()
        .into_iter()
        .find(|metadata| metadata.book_id == book_id)
        .unwrap();
    assert!(book_id.0.starts_with("SSED:"));
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
    assert_eq!(view.kind, ResolvedTargetKind::Deferred);
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
    assert!(metadata.capabilities.contains(&Capability::HcRenderInput));
    assert!(metadata.capabilities.contains(&Capability::NativeSearch));
    assert!(metadata.capabilities.contains(&Capability::Hanrei));
    assert!(metadata.capabilities.contains(&Capability::Panels));
    assert!(
        !metadata.capabilities.contains(&Capability::FullTextSearch),
        "SSED fulltext must not be advertised until a real provider exists"
    );

    let surfaces = package.home_surfaces().unwrap();
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::Menu && surface.status == NavigationStatus::Available
    }));
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
fn ssed_hanrei_surface_lists_chm_and_mac_help_pages() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("hanrei.html"),
        b"<html><head><title>Root</title></head><frameset><frame src=\"BOOK_HELP.localized/menu.html\"></frameset></html>",
    )
    .unwrap();
    fs::write(dir.path().join("HANREI.chm"), b"chm").unwrap();
    fs::create_dir_all(dir.path().join("HANREI/sub")).unwrap();
    fs::write(
        dir.path().join("HANREI/index.html"),
        b"<html><body><a href=\"about.html#overview\">Folder index</a></body></html>",
    )
    .unwrap();
    fs::write(
        dir.path().join("HANREI/about.html"),
        b"<html><body><h1 id=\"overview\">Folder about</h1><img src=\"pic.png\"></body></html>",
    )
    .unwrap();
    fs::write(
        dir.path().join("HANREI/sub/detail.html"),
        b"<html><body>Folder detail</body></html>",
    )
    .unwrap();
    fs::write(dir.path().join("HANREI/pic.png"), b"png").unwrap();
    fs::create_dir_all(dir.path().join("BOOK_HELP.localized/contents/image")).unwrap();
    fs::write(
        dir.path().join("BOOK_HELP.localized/menu.html"),
        b"<html><body><a href=\"contents/hanrei.html#usage\">Usage</a></body></html>",
    )
    .unwrap();
    fs::write(
        dir.path().join("BOOK_HELP.localized/top.html"),
        b"<html><body>top</body></html>",
    )
    .unwrap();
    fs::write(
        dir.path().join("BOOK_HELP.localized/contents/hanrei.html"),
        b"<html><head><link rel=\"stylesheet\" href=\"../contents.css\"></head><body><a name=\"usage\"></a><img src=\"image/B123.png\">Usage</body></html>",
    )
    .unwrap();
    fs::write(
        dir.path()
            .join("BOOK_HELP.localized/contents/copyright.html"),
        b"<html><body>copyright</body></html>",
    )
    .unwrap();
    fs::write(
        dir.path().join("BOOK_HELP.localized/contents.css"),
        b"body{}",
    )
    .unwrap();
    fs::write(
        dir.path()
            .join("BOOK_HELP.localized/contents/image/B123.png"),
        b"png",
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let surfaces = package.home_surfaces().unwrap();
    let hanrei_home = surfaces
        .iter()
        .find(|surface| surface.kind == NavigationSurfaceKind::Hanrei)
        .unwrap();
    assert_eq!(hanrei_home.status, NavigationStatus::Available);
    assert!(hanrei_home.target.is_some());
    assert!(
        hanrei_home
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_hanrei_chm_deferred")
    );

    let surface = package.open_surface("hanrei").unwrap();
    let NavigationSurface::InfoPages { pages, .. } = surface else {
        panic!("SSED HANREI should open as info pages");
    };
    assert!(
        pages
            .iter()
            .any(|page| page.item_id == "hanrei.html" && page.label_text == "Root")
    );
    assert!(pages.iter().any(|page| {
        page.item_id == "HANREI.chm"
            && page
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "ssed_hanrei_chm_deferred")
    }));
    assert!(
        pages
            .iter()
            .any(|page| page.item_id == "HANREI/about.html" && page.label_text == "Folder about")
    );
    assert!(
        pages
            .iter()
            .any(|page| page.item_id == "HANREI/sub/detail.html")
    );
    let folder_index = pages
        .iter()
        .find(|page| page.item_id == "HANREI/index.html")
        .unwrap();
    let folder_index_view = package
        .render_target(&folder_index.target, &RenderOptions::default())
        .unwrap();
    assert_eq!(folder_index_view.kind, ResolvedTargetKind::HanreiPage);
    assert_eq!(
        folder_index_view.links[0].token.decode().unwrap(),
        InternalTarget::Resource {
            resource: ResourceToken::new(&InternalResource::PackageFile {
                path: "HANREI/about.html".to_owned(),
                resource_kind: ResourceKind::Html,
            })
            .unwrap(),
            anchor: Some("overview".to_owned()),
        }
    );
    let mac_hanrei = pages
        .iter()
        .find(|page| page.item_id == "BOOK_HELP.localized/contents/hanrei.html")
        .unwrap();
    assert_eq!(mac_hanrei.label_text, "Mac help: 凡例");
    assert!(matches!(
        mac_hanrei.target.decode().unwrap(),
        InternalTarget::Resource { .. }
    ));

    let view = package
        .render_target(&mac_hanrei.target, &RenderOptions::default())
        .unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::HanreiPage);
    let html = view.display_html.as_deref().unwrap();
    assert!(html.contains("Usage"));
    assert!(html.contains("lvcore://resource/"));
    assert!(!html.contains("../contents.css"));
    assert!(!html.contains("image/B123.png"));
    assert_eq!(view.resources.len(), 2);
    assert_eq!(view.links.len(), 0);
}

#[test]
fn ssed_hanrei_capability_detects_mac_help_bundle_without_root_html() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::create_dir_all(dir.path().join("BOOK_HELP.localized/contents")).unwrap();
    fs::write(
        dir.path().join("BOOK_HELP.localized/contents/hanrei.html"),
        b"<html><body>Mac help only</body></html>",
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::Hanrei)
    );
    let surfaces = package.home_surfaces().unwrap();
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::Hanrei
            && surface.status == NavigationStatus::Available
    }));
    let surface = package.open_surface("hanrei").unwrap();
    let NavigationSurface::InfoPages { pages, .. } = surface else {
        panic!("Mac-only SSED HANREI should open as info pages");
    };
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].item_id, "BOOK_HELP.localized/contents/hanrei.html");
}

#[test]
fn ssed_empty_hanrei_folder_is_not_a_capability() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::create_dir_all(dir.path().join("HANREI/image")).unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert!(
        !package
            .metadata()
            .capabilities
            .contains(&Capability::Hanrei)
    );
    assert!(
        !package
            .home_surfaces()
            .unwrap()
            .iter()
            .any(|surface| surface.kind == NavigationSurfaceKind::Hanrei)
    );
}

#[test]
fn package_html_resource_targets_decode_cp932_and_rewrite_html_links() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::create_dir_all(dir.path().join("HELP/sub")).unwrap();
    fs::write(
        dir.path().join("HELP/index.html"),
        [
            b"<html><head><title>".as_slice(),
            &[0x96, 0x7b, 0x95, 0xb6],
            b"</title></head><body><a href=\"sub/page.html#x\">next</a><img src=\"pic.png\"></body></html>",
        ]
        .concat(),
    )
    .unwrap();
    fs::write(dir.path().join("HELP/sub/page.html"), b"<html>sub</html>").unwrap();
    fs::write(dir.path().join("HELP/pic.png"), b"png").unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let resource = ResourceToken::new(&InternalResource::PackageFile {
        path: "HELP/index.html".to_owned(),
        resource_kind: ResourceKind::Html,
    })
    .unwrap();
    let target = TargetToken::new(&InternalTarget::Resource {
        resource,
        anchor: None,
    })
    .unwrap();

    let view = package
        .render_target(&target, &RenderOptions::default())
        .unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::InfoPage);
    let html = view.display_html.as_deref().unwrap();
    assert!(html.contains("本文"));
    assert!(html.contains("lvcore://target/"));
    assert!(!html.contains("#x"));
    assert!(html.contains("lvcore://resource/"));
    assert!(!html.contains("sub/page.html"));
    assert!(!html.contains("pic.png"));
    assert_eq!(view.links.len(), 1);
    assert_eq!(
        view.links[0].token.decode().unwrap(),
        InternalTarget::Resource {
            resource: ResourceToken::new(&InternalResource::PackageFile {
                path: "HELP/sub/page.html".to_owned(),
                resource_kind: ResourceKind::Html,
            })
            .unwrap(),
            anchor: Some("x".to_owned()),
        }
    );
    assert_eq!(view.resources.len(), 1);
    let linked_view = package
        .render_target(&view.links[0].token, &RenderOptions::default())
        .unwrap();
    assert_eq!(linked_view.kind, ResolvedTargetKind::InfoPage);
    assert_eq!(linked_view.scroll_anchor.as_deref(), Some("x"));

    let basic = package
        .render_target(
            &target,
            &RenderOptions {
                mode: RenderMode::BasicText,
                ..RenderOptions::default()
            },
        )
        .unwrap();
    assert_eq!(basic.basic_text.as_deref(), Some("本文next"));
    assert!(basic.display_html.is_none());
}

#[test]
fn ssed_menu_and_panel_targets_support_continuous_view_windows() {
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
    fs::create_dir(dir.path().join("Panel")).unwrap();
    fs::write(
        dir.path().join("Panels.xml"),
        r#"<panels>
  <panel index="01010000" paneltype="contents">
    <title>あ</title>
    <data type="bin" filename="Panel\All-A.bin" />
  </panel>
</panels>"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("Panel/All-A.bin"),
        panel_bin_fixture_rows(&[
            (10, 0, [0x24, 0x22]),
            (10, 2, [0x24, 0x24]),
            (10, 4, [0x24, 0x26]),
        ]),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 10,
        offset: 2,
    })
    .unwrap();

    let menu_window = package
        .resolve_target_window(
            &target,
            Some(&lvcore::SequenceHint::MenuOrder("menu".to_owned())),
            1,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(menu_window.center.title.as_deref(), Some("い"));
    assert_eq!(menu_window.before.len(), 1);
    assert_eq!(menu_window.after.len(), 1);
    assert_eq!(ssed_view_offset(&menu_window.before[0]), Some((10, 0)));
    assert_eq!(ssed_view_offset(&menu_window.after[0]), Some((10, 4)));
    assert!(menu_window.diagnostics.is_empty());

    let panel_window = package
        .resolve_target_window(
            &target,
            Some(&lvcore::SequenceHint::PanelOrder("01010000".to_owned())),
            1,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(panel_window.center.title.as_deref(), Some("い"));
    assert_eq!(panel_window.before.len(), 1);
    assert_eq!(panel_window.after.len(), 1);
    assert_eq!(ssed_view_offset(&panel_window.before[0]), Some((10, 0)));
    assert_eq!(ssed_view_offset(&panel_window.after[0]), Some((10, 4)));
    assert!(panel_window.diagnostics.is_empty());
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

    let input = package.renderer_input_for_target(&token).unwrap();
    assert!(matches!(input, RendererInput::Unsupported { .. }));
    assert!(!serde_json::to_string(&input).unwrap().contains("00100050"));

    let view = package
        .render_target(&token, &RenderOptions::default())
        .unwrap();
    assert!(view.display_html.is_none());
    assert_eq!(view.kind, ResolvedTargetKind::Unsupported);
    assert!(
        view.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_dense_sidecar_not_found")
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
    fs::write(dir.path().join("HC0158.dll"), b"").unwrap();
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

    let input = package.renderer_input_for_target(&token).unwrap();
    assert_eq!(input.target(), &token);
    assert_eq!(input.kind(), lvcore::RendererInputKind::HcSsedStream);
    let RendererInput::HcSsedStream {
        component,
        offset,
        length,
        profile_hint,
        diagnostics,
        ..
    } = input
    else {
        panic!("SSED stream must become explicit HC renderer input");
    };
    assert_eq!(component, "HONMON.DIC");
    assert_eq!(offset, 2);
    assert_eq!(length, None);
    assert_eq!(profile_hint.as_deref(), Some("HC0158"));
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "hc_renderer_input_ready")
    );

    let view = package.render_target(&token, &options).unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::Deferred);
    assert!(view.display_html.is_none());
    assert!(
        view.capabilities
            .contains(&lvcore::RenderCapability::HcRenderInput)
    );
    assert!(
        view.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "hc_render_deferred")
    );
    let debug_trace = view.debug_trace.as_deref().unwrap_or_default();
    assert!(debug_trace.contains("HONMON.DIC"));
    assert!(debug_trace.contains("\"offset\":2"));
    assert!(debug_trace.contains("HC0158"));
}

#[test]
fn ssed_honmon_targets_accept_mac_extensionless_payload_alias() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_honmon("HONMON.DIN"),
    )
    .unwrap();
    fs::write(
        dir.path().join("HONMON"),
        sseddata_literal_fixture(b"0123456789"),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIN".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();

    let input = package.renderer_input_for_target(&target).unwrap();
    let RendererInput::HcSsedStream {
        component, offset, ..
    } = input
    else {
        panic!("extensionless Mac HONMON should still produce HC SSED renderer input");
    };
    assert_eq!(component, "HONMON.DIN");
    assert_eq!(offset, 2);
}

#[test]
fn ssed_honmon_targets_accept_mac_zipcrypto_payload_wrapper() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("0000012e.idx"),
        ssedinfo_fixture_with_honmon("HONMON.DIN"),
    )
    .unwrap();
    write_zipcrypto_honmon_wrapper(
        &dir.path().join("HONMON"),
        "HONMON.DIN",
        b"casKet0000012e",
        &sseddata_literal_fixture(b"0123456789"),
    );

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIN".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();

    let input = package.renderer_input_for_target(&target).unwrap();
    let RendererInput::HcSsedStream {
        component, offset, ..
    } = input
    else {
        panic!("Mac HONMON ZipCrypto wrapper should produce HC SSED renderer input");
    };
    assert_eq!(component, "HONMON.DIN");
    assert_eq!(offset, 2);
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
fn ssed_search_and_navigation_labels_resolve_gaiji_markers() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(dir.path().join("DICT.uni"), uni_fixture()).unwrap();
    fs::write(dir.path().join("GA16HALF"), ga16_fixture(0xA121, 8)).unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"0123456789"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha <zB123> zA128 zB999\x1f\x0a"),
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
    let hit = &page.hits[0];
    assert_eq!(hit.title_text, "alpha 一 〓 〓");
    assert!(hit.title_html.contains("alpha 一 "));
    assert!(hit.title_html.contains("lvcore://resource/"));
    assert!(hit.title_html.contains(r#"data-gaiji="B999""#));
    assert!(!hit.title_html.contains("<zB123>"));
    assert!(!hit.title_html.contains("zA128"));
    assert!(
        hit.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "gaiji_unresolved")
    );

    let surface = package.open_surface("title-index").unwrap();
    let lvcore::NavigationSurface::TitleIndexBrowse { items, .. } = surface else {
        panic!("title-index should open as a title/index browse surface");
    };
    assert_eq!(items[0].label_text, "alpha 一 〓 〓");
    assert!(items[0].label_html.contains("lvcore://resource/"));
    assert!(items[0].label_html.contains(r#"data-gaiji="B999""#));
    assert!(
        items[0]
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "gaiji_unresolved")
    );

    let window = package
        .resolve_target_window(
            &hit.target,
            Some(&lvcore::SequenceHint::TitleIndexOrder(
                "title-index".to_owned(),
            )),
            0,
            0,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(window.center.title.as_deref(), Some("alpha 一 〓 〓"));
}

#[test]
fn ssed_simple_index_search_supports_backward_matching() {
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

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
            mode: SearchMode::Backward,
            query: "ta".to_owned(),
            cursor: None,
            limit: 10,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "beta");
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
fn ssed_tagged_index_search_supports_grouped_rows_across_pages() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_index_type(0x90),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"child title\x1f\x0a"),
    )
    .unwrap();
    let index = [
        leaf_page_fixture(&[tagged_group_record("parent", 2)]),
        leaf_page_fixture(&[tagged_target_record("child", 1, 2, 13, 0)]),
    ]
    .concat();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&index),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
            mode: SearchMode::Exact,
            query: "parent".to_owned(),
            cursor: None,
            limit: 10,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "child title");
    assert_eq!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 1,
            offset: 2,
        }
    );
    assert!(
        page.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "ssed_index_variant_deferred")
    );
}

#[test]
fn ssed_keyword_and_cross_reference_indexes_resolve_grouped_body_targets() {
    for (component_type, target_tag) in [(0x80, 0xb0), (0x81, 0xc0)] {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("DICT.IDX"),
            ssedinfo_fixture_with_index_type(component_type),
        )
        .unwrap();
        fs::write(
            dir.path().join("FHTITLE.DIC"),
            sseddata_literal_fixture(b"group title\x1f\x0a"),
        )
        .unwrap();
        let index = leaf_page_fixture(&[
            title_group_record("group", 13, 0, 1),
            compact_body_target_record(target_tag, 1, 6),
        ]);
        fs::write(
            dir.path().join("FHINDEX.DIC"),
            sseddata_literal_fixture(&index),
        )
        .unwrap();
        let package = DriverRegistry::default().open_best(dir.path()).unwrap();

        let page = package
            .search(&SearchQuery {
                scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
                mode: SearchMode::Exact,
                query: "group".to_owned(),
                cursor: None,
                limit: 10,
            })
            .unwrap();

        assert_eq!(page.hits.len(), 1, "component type {component_type:02x}");
        assert_eq!(page.hits[0].title_text, "group title");
        assert_eq!(
            page.hits[0].target.decode().unwrap(),
            InternalTarget::SsedAddress {
                component: "HONMON.DIC".to_owned(),
                block: 1,
                offset: 6,
            }
        );
    }
}

#[test]
fn ssed_body_only_and_multi_selector_indexes_resolve_targets() {
    for (component_type, index) in [
        (
            0x60,
            leaf_page_fixture(&[body_only_simple_record("body", 1, 8)]),
        ),
        (
            0x30,
            leaf_page_fixture(&[
                tagged_group_record("bodytag", 1),
                tagged_target_body_only_record("child", 1, 10),
            ]),
        ),
        (
            0xa1,
            leaf_page_fixture(&[
                multi_group_record("multi", 1),
                multi_target_record(1, 12, 13, 0),
            ]),
        ),
    ] {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("DICT.IDX"),
            ssedinfo_fixture_with_index_type(component_type),
        )
        .unwrap();
        fs::write(
            dir.path().join("FHTITLE.DIC"),
            sseddata_literal_fixture(b"multi title\x1f\x0a"),
        )
        .unwrap();
        fs::write(
            dir.path().join("FHINDEX.DIC"),
            sseddata_literal_fixture(&index),
        )
        .unwrap();
        let package = DriverRegistry::default().open_best(dir.path()).unwrap();
        let query = if component_type == 0x30 {
            "bodytag"
        } else if component_type == 0xa1 {
            "multi"
        } else {
            "body"
        };

        let page = package
            .search(&SearchQuery {
                scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
                mode: SearchMode::Exact,
                query: query.to_owned(),
                cursor: None,
                limit: 10,
            })
            .unwrap();

        assert_eq!(page.hits.len(), 1, "component type {component_type:02x}");
        let (_, expected_offset) = match component_type {
            0x60 => ("body", 8),
            0x30 => ("bodytag", 10),
            0xa1 => ("multi", 12),
            _ => unreachable!(),
        };
        assert_eq!(
            page.hits[0].target.decode().unwrap(),
            InternalTarget::SsedAddress {
                component: "HONMON.DIC".to_owned(),
                block: 1,
                offset: expected_offset,
            }
        );
    }
}

#[test]
fn ssed_keyless_pointer_table_simple_leaf_is_supported() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"keyless\x1f\x0a"),
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
        sseddata_literal_fixture(&page),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let surface = package.open_surface("title-index").unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = surface else {
        panic!("title-index should open as a title/index browse surface");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label_text, "keyless");
    assert_eq!(
        items[0].target.decode().unwrap(),
        InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 1,
            offset: 14,
        }
    );
}

#[test]
fn ssed_exact_search_uses_internal_page_tree_for_simple_indexes() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_index_type_and_blocks(0x91, 3),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0azeta\x1f\x0a"),
    )
    .unwrap();
    let index = [
        internal_page_fixture(&[("m", 16), ("\u{10ffff}", 17)]),
        simple_index_fixture_rows(&[("alpha", 1, 2, 13, 0)]),
        simple_index_fixture_rows(&[("zeta", 1, 4, 13, 7)]),
    ]
    .concat();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&index),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
            mode: SearchMode::Exact,
            query: "zeta".to_owned(),
            cursor: None,
            limit: 10,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "zeta");
    assert!(
        page.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "ssed_index_internal_page_deferred")
    );
}

#[test]
fn ssed_simple_index_search_uses_cursor_pagination() {
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

    let first = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
            mode: SearchMode::Partial,
            query: "a".to_owned(),
            cursor: None,
            limit: 2,
        })
        .unwrap();
    assert_eq!(
        first
            .hits
            .iter()
            .map(|hit| hit.title_text.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
    assert_eq!(first.next_cursor.as_deref(), Some("2"));

    let second = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
            mode: SearchMode::Partial,
            query: "a".to_owned(),
            cursor: first.next_cursor,
            limit: 2,
        })
        .unwrap();
    assert_eq!(second.hits[0].title_text, "gamma");
    assert!(second.next_cursor.is_none());
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

    let body_order = package
        .resolve_target_window(
            &target,
            Some(&lvcore::SequenceHint::BodyOrder),
            1,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(body_order.center.title.as_deref(), Some("beta"));
    assert_eq!(body_order.before[0].title.as_deref(), Some("alpha"));
    assert_eq!(body_order.after[0].title.as_deref(), Some("gamma"));
    assert!(
        body_order
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "sequence_deferred")
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
fn ssed_gaiji_resolution_honors_policy_and_keeps_fallbacks() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(dir.path().join("DICT.uni"), uni_fixture()).unwrap();
    fs::write(dir.path().join("GA16HALF"), ga16_fixture(0xA121, 8)).unwrap();
    fs::write(dir.path().join("GA16FULL"), ga16_fixture(0xB121, 3)).unwrap();
    fs::create_dir(dir.path().join("Templates")).unwrap();
    fs::write(dir.path().join("Templates/B123.SVG"), b"<svg/>").unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let default = package.resolve_gaiji("B123", &GaijiPolicy::default());
    assert_eq!(default.identity, "B123");
    assert_eq!(
        default.preferred_source,
        Some(GaijiSourcePreference::Unicode)
    );
    assert_eq!(default.unicode.as_deref(), Some("一"));
    assert_eq!(
        default.resource.as_ref().unwrap().kind,
        ResourceKind::Template
    );

    let image_first = package.resolve_gaiji(
        "<zB123>",
        &GaijiPolicy {
            priority: vec![
                GaijiSourcePreference::ExternalResource,
                GaijiSourcePreference::Unicode,
                GaijiSourcePreference::Ga16Bitmap,
            ],
        },
    );
    assert_eq!(
        image_first.preferred_source,
        Some(GaijiSourcePreference::ExternalResource)
    );
    assert_eq!(image_first.unicode.as_deref(), Some("一"));
    assert_eq!(
        image_first.resource.as_ref().unwrap().label.as_deref(),
        Some("B123.SVG")
    );
    let bitmap_first = package.resolve_gaiji(
        "B123",
        &GaijiPolicy {
            priority: vec![
                GaijiSourcePreference::Ga16Bitmap,
                GaijiSourcePreference::ExternalResource,
                GaijiSourcePreference::Unicode,
            ],
        },
    );
    assert_eq!(
        bitmap_first.preferred_source,
        Some(GaijiSourcePreference::Ga16Bitmap)
    );
    assert_eq!(
        bitmap_first.resource.as_ref().unwrap().label.as_deref(),
        Some("GA16FULL")
    );

    let bitmap = package.resolve_gaiji("A128", &GaijiPolicy::default());
    assert_eq!(
        bitmap.preferred_source,
        Some(GaijiSourcePreference::Ga16Bitmap)
    );
    assert!(bitmap.unicode.is_none());
    assert_eq!(
        bitmap.resource.as_ref().unwrap().label.as_deref(),
        Some("GA16HALF")
    );

    let unresolved = package.resolve_gaiji("B999", &GaijiPolicy::default());
    assert_eq!(
        unresolved.preferred_source,
        Some(GaijiSourcePreference::Unresolved)
    );
    assert!(unresolved.resource.is_none());
    assert!(unresolved.unicode.is_none());
    assert!(
        unresolved
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "gaiji_unresolved")
    );
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
    write_minimal_lved_sqlite_fixture(dir.path());
    let payload = dir.path().join("main.data");

    let package = DriverRegistry::default().open_best(&payload).unwrap();
    assert_eq!(package.metadata().format_family, FormatFamily::LvedSqlite3);
    assert_eq!(package.root(), dir.path());
}

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
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
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
            scope: SearchScope::CurrentBook(package.metadata().book_id.clone()),
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
fn lved_tree_idx_opens_as_navigation_tree_and_targets_content_rows() {
    let dir = tempdir().unwrap();
    write_minimal_lved_sqlite_fixture(dir.path());

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert_eq!(
        package.metadata().title.as_deref(),
        Some("Example Dictionary")
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
fn library_routes_lved_cross_book_targets_through_loaded_book_aliases() {
    let root = tempdir().unwrap();
    let source_dir = root.path().join("_DCT_SOURCE");
    let destination_dir = root.path().join("_DCT_BUREI");
    fs::create_dir(&source_dir).unwrap();
    fs::create_dir(&destination_dir).unwrap();
    write_lved_cross_book_source_fixture(&source_dir);
    write_minimal_lved_sqlite_fixture(&destination_dir);

    let registry = DriverRegistry::default();
    let mut library = BookLibrary::new();
    let source_book_id = library.open_path(&source_dir, &registry).unwrap();
    let destination_book_id = library.open_path(&destination_dir, &registry).unwrap();

    let source_target = TargetToken::new(&InternalTarget::LvedRow {
        table: "content".to_owned(),
        row_id: 10,
        anchor: None,
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
            .contains("<article><h1>Alpha</h1><p>Tree body</p></article>")
    );
    assert!(matches!(
        routed.view.target.decode().unwrap(),
        InternalTarget::LvedRow {
            table,
            row_id: 100,
            anchor: Some(anchor)
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
    let target = TargetToken::new(&InternalTarget::Resource {
        resource,
        anchor: None,
    })
    .unwrap();

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

fn ssedinfo_fixture_with_index_type(index_type: u8) -> Vec<u8> {
    ssedinfo_fixture_with_honmon_index_type_and_blocks("HONMON.DIC", index_type, 2)
}

fn ssedinfo_fixture_with_index_type_and_blocks(index_type: u8, index_blocks: u32) -> Vec<u8> {
    ssedinfo_fixture_with_honmon_index_type_and_blocks("HONMON.DIC", index_type, index_blocks)
}

fn write_minimal_lved_sqlite_fixture(root: &Path) {
    let payload = root.join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        connection.pragma_update(None, "key", key).unwrap();
        connection
            .pragma_update(None, "cipher_compatibility", 4)
            .unwrap();
        connection
            .execute_batch(
                "
                create table info (id integer, type integer, name text primary key, body text, media text);
                insert into info values (1, 1, 'about.html', '<h1>Example Dictionary 第2版</h1>', '');
                insert into info values (null, 1, 'null-id.html', '<h1>Null id info</h1>', '');
                create table content (id integer primary key, type integer, body text, media text);
                insert into content values (100, 1, '<article><h1>Alpha</h1><p>Tree body</p></article>', '');
                insert into content values (105, 1, '<article><h1>Beta</h1><p>Tree body</p></article>', '');
                create table list (id integer primary key, refid integer, type integer, anchor text, title text, titlesub text);
                insert into list values (1, 100, 1, '', '<b>alpha</b>', '');
                insert into list values (2, 105, 1, '', '<b>beta</b>', '');
                create virtual table search using fts4(forward, back, part, fts, advanced1, advanced2, filter);
                insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                  values (1, 'alpha', 'ahpla', 'shared alpha', 'alpha body', '', '', '∥shared∥');
                insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                  values (2, 'beta', 'ateb', 'shared beta', 'beta body', '', '', '∥shared∥');
                ",
            )
            .unwrap();
    }
    fs::create_dir(root.join("res")).unwrap();
    fs::write(
        root.join("res/tree.idx"),
        "\u{feff}0\t0\tExample Dictionary\r\n0\t1\tBrowse\r\n100\t2\tAlpha\r\n105\t2\tBeta\r\n",
    )
    .unwrap();
    fs::write(root.join("main.key"), key).unwrap();
}

fn write_lved_cross_book_source_fixture(root: &Path) {
    let payload = root.join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        connection.pragma_update(None, "key", key).unwrap();
        connection
            .pragma_update(None, "cipher_compatibility", 4)
            .unwrap();
        connection
            .execute_batch(
                "
                create table content (id integer primary key, type integer, body text, media text);
                insert into content values (
                  10,
                  1,
                  '<article><h1>Source</h1><a href=\"lved.contentlink:BUREI.100#dest\">target</a></article>',
                  ''
                );
                create table list (id integer primary key, refid integer, type integer, anchor text, title text, titlesub text);
                insert into list values (1, 10, 1, '', 'source', '');
                ",
            )
            .unwrap();
    }
    fs::write(root.join("main.key"), key).unwrap();
}

fn write_minimal_multiview_content_fixture(path: &Path) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            r#"
            create table t_contents (
              f_ID integer primary key,
              f_Title text,
              f_Body text
            );
            create table t_search (
              f_No integer primary key,
              f_ID integer,
              f_Anchor integer,
              f_KeyWord text,
              f_MainFlag integer,
              f_Level integer,
              f_TitleMain text,
              f_All text
            );
            insert into t_contents values
              (1, '<b>まえがき</b>', '<article><h1>まえがき</h1><p>body</p><a href="lved_ref:entry:000002">next</a><img src="pic.png"></article>');
            insert into t_contents values
              (2, '<b>本文</b>', '<article><h1>本文</h1><p>body</p></article>');
            insert into t_contents values
              (3, '<b>あとがき</b>', '<article><h1>あとがき</h1><p>body</p></article>');
            insert into t_search values
              (1, 1, 1, '§まえがき§', 1, 0, '<b>まえがき</b>', 'まえがき body');
            insert into t_search values
              (2, 2, 1, '§本文§', 1, 0, '<b>本文</b>', '本文 body');
            "#,
        )
        .unwrap();
}

fn write_minimal_multiview_law_fixture(root: &Path) {
    let law = Connection::open(root.join("blvbat")).unwrap();
    law.execute_batch(
        r#"
        create table t_page (
          f_hore_code text,
          f_rec_id integer,
          f_rec_type integer,
          f_title_no text,
          f_title_sub text,
          f_anchor text,
          f_text text,
          f_text_plane text,
          f_text_count integer,
          f_text_plane_count integer
        );
        create table t_111S21K1 (
          f_hore_code text,
          f_rec_id integer,
          f_rec_type integer,
          f_title_no text,
          f_title_sub text,
          f_anchor text,
          f_text text,
          f_text_plane text,
          f_text_count integer,
          f_text_plane_count integer
        );
        insert into t_111S21K1 values
          ('111S21K1', 10000, 0, '見出し', '', '111S21K1_TITLE',
           '<div class="header">日本国憲法本文</div>', '日本国憲法本文', 0, 0);
        insert into t_111S21K1 values
          ('111S21K1', 20000, 0, '公布文・前文', '', '111S21K1_ZEN',
           '<div class="zenbun">前文</div>', '前文', 0, 0);
        "#,
    )
    .unwrap();

    let metadata = Connection::open(root.join("nlvbat")).unwrap();
    metadata
        .execute_batch(
            r#"
            create table t_hore (
              f_hore_code text,
              f_hore_id integer,
              f_pub_era integer,
              f_pub_year integer,
              f_pub_no integer,
              f_pub_date date,
              f_pub_desc string,
              f_name string,
              f_name_sub text,
              f_name_kana text,
              f_kana_ini string,
              f_kana_order integer,
              f_abbr1 string,
              f_abbr1_kana text,
              f_nickname text,
              f_commonname text,
              f_commonname_kana text,
              f_commonname_ex text,
              f_category_id text
            );
            insert into t_hore values
              ('111S21K1', 1, 0, 0, 0, '', '', '日本国憲法', '', 'にほんこくけんぽう', 'に', 1, '', '', '', '', '', '', '1');
            insert into t_hore values
              ('22M1', 2, 0, 0, 0, '', '', '民法', '', 'みんぽう', 'み', 2, '', '', '', '', '', '', '3');
            "#,
        )
        .unwrap();
}

fn write_minimal_hourei_fixture(root: &Path) {
    let database = root.join("_DataBase");
    fs::create_dir_all(database.join("HTMLs/H")).unwrap();
    fs::create_dir_all(database.join("image")).unwrap();
    fs::create_dir_all(database.join("H01")).unwrap();
    fs::write(database.join("image/law.png"), b"png").unwrap();

    for name in ["hore_base.db", "hore_search_a.db"] {
        let connection = Connection::open(database.join(name)).unwrap();
        connection
            .execute_batch(
                r#"
                create table t_category (f_category_id integer, f_category_name string);
                create table t_hore (
                  f_hore_id integer,
                  f_name string,
                  f_name_sub string,
                  f_abbr1 string,
                  f_abbr2 string,
                  f_abbr3 string,
                  f_abbr4 string,
                  f_abbr5 string,
                  f_abbr6 string,
                  f_abbr7 string,
                  f_category_id integer,
                  f_kana_order integer,
                  f_text_plane text
                );
                insert into t_category values (10, '民事');
                insert into t_hore values
                  (401000000000000001, '民法', '', '', '', '', '', '', '', '', 10, 1, '民法本文'),
                  (401000000000000002, '商法', '', '', '', '', '', '', '', '', 10, 2, '商法本文');
                "#,
            )
            .unwrap();
    }
    Connection::open(database.join("horejo_base.db")).unwrap();
    fs::write(
        database.join("HTMLs/H/401000000000000001_H.html"),
        r#"<div class="header">民法</div><a href="lved_mark&&A1">mark</a><a href="lved_ref&1:401000000000000002&A2">商法</a><img src="law.png">"#,
    )
    .unwrap();
    let shard = Connection::open(database.join("H01/401000000000000002.db")).unwrap();
    shard
        .execute_batch(
            r#"
            create table t_page (f_rec_id integer, f_text text);
            insert into t_page values (1, '<div>商法本文</div>');
            "#,
        )
        .unwrap();
}

fn ssedinfo_fixture_with_honmon(honmon_filename: &str) -> Vec<u8> {
    ssedinfo_fixture_with_honmon_index_type_and_blocks(honmon_filename, 0x91, 2)
}

fn ssedinfo_fixture_with_honmon_index_type_and_blocks(
    honmon_filename: &str,
    index_type: u8,
    index_blocks: u32,
) -> Vec<u8> {
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
        index_type,
        15,
        15 + index_blocks.saturating_sub(1),
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
    let block_count = literals.len().div_ceil(2048).max(1);
    let mut data = vec![0u8; chunk_offset];
    data[..8].copy_from_slice(SSEDDATA_MAGIC);
    data[0x0f] = 1;
    data[0x16..0x18].copy_from_slice(&1u16.to_be_bytes());
    data[0x18..0x1c].copy_from_slice(&1u32.to_be_bytes());
    data[0x1c..0x20].copy_from_slice(&(block_count as u32).to_be_bytes());
    data[0x40..0x44].copy_from_slice(&(chunk_offset as u32).to_be_bytes());
    data.extend_from_slice(&[0, 0]);
    data.extend_from_slice(&(literals.len() as u16).to_be_bytes());
    data.push(0);
    for literal in literals {
        data.extend_from_slice(&[0, 0, *literal]);
    }
    data
}

fn write_zipcrypto_honmon_wrapper(path: &Path, member_name: &str, password: &[u8], payload: &[u8]) {
    let file = fs::File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().with_deprecated_encryption(password);
    zip.start_file(member_name, options).unwrap();
    zip.write_all(payload).unwrap();
    zip.finish().unwrap();
}

fn menu_stream_fixture(block: u32, offset: u16) -> Vec<u8> {
    menu_stream_fixture_rows(&[([0x24, 0x22], block, offset)])
}

fn menu_stream_fixture_rows(rows: &[([u8; 2], u32, u16)]) -> Vec<u8> {
    let mut data = Vec::new();
    for (label, block, offset) in rows {
        data.extend_from_slice(&[0x1f, 0x09, 0x00, 0x01]);
        data.extend_from_slice(&[0x1f, 0x42]);
        data.extend_from_slice(label);
        data.extend_from_slice(&[0x1f, 0x62]);
        data.extend_from_slice(&bcd_u32(*block));
        data.extend_from_slice(&bcd_u16(*offset));
        data.extend_from_slice(&[0x1f, 0x0a]);
    }
    data
}

fn panel_bin_fixture(block: u32, offset: u32) -> Vec<u8> {
    panel_bin_fixture_rows(&[(block, offset, [0x24, 0x22])])
}

fn panel_bin_fixture_rows(rows: &[(u32, u32, [u8; 2])]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    data.extend_from_slice(&4u32.to_le_bytes());
    for (block, offset, label) in rows {
        data.extend_from_slice(&block.to_le_bytes());
        data.extend_from_slice(&offset.to_le_bytes());
        data.extend_from_slice(label);
        data.extend_from_slice(&[0x00, 0x00]);
    }
    data
}

fn uni_fixture() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"Ver2  ");
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(&1u32.to_be_bytes());
    data.extend_from_slice(&[
        0xB1, 0x23, 0x00, 0x00, 0x4E, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    data
}

fn ga16_fixture(start_code: u16, count: u16) -> Vec<u8> {
    let mut data = vec![0u8; 14];
    data[8] = 16;
    data[9] = 16;
    data[10..12].copy_from_slice(&start_code.to_be_bytes());
    data[12..14].copy_from_slice(&count.to_be_bytes());
    data
}

fn ssed_view_offset(view: &lvcore::ResolvedTargetView) -> Option<(u32, u32)> {
    match view.target.decode().ok()? {
        InternalTarget::SsedAddress { block, offset, .. } => Some((block, offset)),
        _ => None,
    }
}

fn bcd_u32(value: u32) -> [u8; 4] {
    let digits = format!("{value:08}");
    let bytes = digits.as_bytes();
    [
        (bytes[0] - b'0') << 4 | (bytes[1] - b'0'),
        (bytes[2] - b'0') << 4 | (bytes[3] - b'0'),
        (bytes[4] - b'0') << 4 | (bytes[5] - b'0'),
        (bytes[6] - b'0') << 4 | (bytes[7] - b'0'),
    ]
}

fn bcd_u16(value: u16) -> [u8; 2] {
    let digits = format!("{value:04}");
    let bytes = digits.as_bytes();
    [
        (bytes[0] - b'0') << 4 | (bytes[1] - b'0'),
        (bytes[2] - b'0') << 4 | (bytes[3] - b'0'),
    ]
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

fn leaf_page_fixture(records: &[Vec<u8>]) -> Vec<u8> {
    let mut page = vec![0u8; 2048];
    page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    page[2..4].copy_from_slice(&(records.len() as u16).to_be_bytes());
    let mut pos = 4usize;
    for record in records {
        page[pos..pos + record.len()].copy_from_slice(record);
        pos += record.len();
    }
    page
}

fn internal_page_fixture(rows: &[(&str, u32)]) -> Vec<u8> {
    let mut page = vec![0u8; 2048];
    let key_len = 2usize;
    page[0..2].copy_from_slice(&(key_len as u16).to_be_bytes());
    page[2..4].copy_from_slice(&(rows.len() as u16).to_be_bytes());
    let mut pos = 4usize;
    for (key, child_block) in rows {
        let raw_key = if *key == "\u{10ffff}" {
            vec![0xff; key_len]
        } else {
            let mut key = jis_fullwidth_ascii_key(key);
            key.resize(key_len, 0);
            key
        };
        page[pos..pos + key_len].copy_from_slice(&raw_key[..key_len]);
        pos += key_len;
        page[pos..pos + 4].copy_from_slice(&child_block.to_be_bytes());
        pos += 4;
    }
    page
}

fn body_only_simple_record(key: &str, body_block: u32, body_offset: u16) -> Vec<u8> {
    let key = jis_fullwidth_ascii_key(key);
    let mut out = Vec::new();
    out.push(key.len() as u8);
    out.extend_from_slice(&key);
    out.extend_from_slice(&body_block.to_be_bytes());
    out.extend_from_slice(&body_offset.to_be_bytes());
    out
}

fn tagged_group_record(key: &str, count: u16) -> Vec<u8> {
    let key = jis_fullwidth_ascii_key(key);
    let mut out = vec![0x80, key.len() as u8];
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&key);
    out
}

fn tagged_target_record(
    key: &str,
    body_block: u32,
    body_offset: u16,
    title_block: u32,
    title_offset: u16,
) -> Vec<u8> {
    let key = jis_fullwidth_ascii_key(key);
    let mut out = vec![0xc0, key.len() as u8];
    out.extend_from_slice(&key);
    out.extend_from_slice(&body_block.to_be_bytes());
    out.extend_from_slice(&body_offset.to_be_bytes());
    out.extend_from_slice(&title_block.to_be_bytes());
    out.extend_from_slice(&title_offset.to_be_bytes());
    out
}

fn tagged_target_body_only_record(key: &str, body_block: u32, body_offset: u16) -> Vec<u8> {
    let key = jis_fullwidth_ascii_key(key);
    let mut out = vec![0xc0, key.len() as u8];
    out.extend_from_slice(&key);
    out.extend_from_slice(&body_block.to_be_bytes());
    out.extend_from_slice(&body_offset.to_be_bytes());
    out
}

fn title_group_record(key: &str, title_block: u32, title_offset: u16, count: u32) -> Vec<u8> {
    let key = jis_fullwidth_ascii_key(key);
    let mut out = vec![0x80, key.len() as u8];
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&key);
    out.extend_from_slice(&title_block.to_be_bytes());
    out.extend_from_slice(&title_offset.to_be_bytes());
    out
}

fn compact_body_target_record(tag: u8, body_block: u32, body_offset: u16) -> Vec<u8> {
    let mut out = vec![tag];
    out.extend_from_slice(&body_block.to_be_bytes());
    out.extend_from_slice(&body_offset.to_be_bytes());
    out
}

fn multi_group_record(key: &str, count: u32) -> Vec<u8> {
    let key = jis_fullwidth_ascii_key(key);
    let mut out = vec![0x80, key.len() as u8];
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&key);
    out
}

fn multi_target_record(
    body_block: u32,
    body_offset: u16,
    title_block: u32,
    title_offset: u16,
) -> Vec<u8> {
    let mut out = vec![0xc0];
    out.extend_from_slice(&body_block.to_be_bytes());
    out.extend_from_slice(&body_offset.to_be_bytes());
    out.extend_from_slice(&title_block.to_be_bytes());
    out.extend_from_slice(&title_offset.to_be_bytes());
    out
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

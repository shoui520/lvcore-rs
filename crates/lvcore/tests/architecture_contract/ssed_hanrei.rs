use super::common::*;

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
            b"</title></head><body><a href = \"sub/page.html#x\">next</a><img src = \"pic.png\"></body></html>",
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

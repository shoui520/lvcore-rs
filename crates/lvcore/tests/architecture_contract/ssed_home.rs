use super::common::*;

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

use super::common::*;

#[test]
fn ssed_panels_can_read_package_adjacent_panel_sidecar_directory() {
    let root = tempdir().unwrap();
    let package_root = root.path().join("DICT");
    let sibling_panel_root = root.path().join("DICT_Panel");
    fs::create_dir(&package_root).unwrap();
    fs::create_dir(&sibling_panel_root).unwrap();
    fs::write(package_root.join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        package_root.join("Panels.xml"),
        r#"<panels>
  <panel index="01000000" paneltype="menu" count_x="1">
    <title>五十音</title>
    <data><cell ref="01010000">あ</cell></data>
  </panel>
  <panel index="01010000" paneltype="contents">
    <title>あ</title>
    <data type="bin" filename="Panel\All-A.bin" />
  </panel>
</panels>"#,
    )
    .unwrap();
    fs::write(
        sibling_panel_root.join("All-A.bin"),
        panel_bin_fixture(10, 2),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    let child_panel = package.open_surface("panels:01010000").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = child_panel else {
        panic!("SSED child Panel should decode from the sibling sidecar directory");
    };
    assert_eq!(cells.len(), 1);
    assert!(cells[0].diagnostics.is_empty());
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
fn ssed_missing_declared_menu_does_not_hide_panel_home_surface() {
    let root = tempdir().unwrap();
    let package_root = root.path().join("IWKOKU7N");
    let sibling_panel_root = root.path().join("IWKOKU7N_Panel");
    fs::create_dir(&package_root).unwrap();
    fs::create_dir(&sibling_panel_root).unwrap();
    fs::write(package_root.join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        package_root.join("EXINFO.INI"),
        b"[GENERAL]\nIDXCOUNT=1\nIDXNAME0=Index\nIDXINFO0=0000015E.IDX\n",
    )
    .unwrap();
    fs::write(
        package_root.join("0000015E.IDX"),
        b"00000000\t00000000\tRoot\n00000010\t00000002\t\tChild\n",
    )
    .unwrap();
    fs::write(
        package_root.join("Panels.xml"),
        r#"<panels>
  <panel index="01000000" paneltype="menu" count_x="1">
    <title>五十音</title>
    <data><cell ref="01010000">あ</cell></data>
  </panel>
  <panel index="01010000" paneltype="contents">
    <title>あ</title>
    <data type="bin" filename="Panel\All-A.bin" />
  </panel>
</panels>"#,
    )
    .unwrap();
    fs::write(
        sibling_panel_root.join("All-A.bin"),
        panel_bin_fixture(10, 2),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    assert!(
        !package.metadata().capabilities.contains(&Capability::Menu),
        "a catalog-declared MENU.DIC without a payload must not become a reader capability"
    );
    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::Panels)
    );
    let surfaces = package.home_surfaces().unwrap();
    assert!(
        !surfaces
            .iter()
            .any(|surface| surface.kind == NavigationSurfaceKind::Menu),
        "missing MENU.DIC should not be exposed as an actionable home surface"
    );
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::Panel
            && surface.status == NavigationStatus::Available
    }));
    let panel_index = surfaces
        .iter()
        .position(|surface| surface.surface_id == "panels")
        .expect("panel surface should be advertised");
    let aux_index = surfaces
        .iter()
        .position(|surface| surface.surface_id == "aux-index:0")
        .expect("auxiliary index surface should be advertised");
    assert!(
        panel_index < aux_index,
        "Panels are a native book-home surface and should sort before auxiliary indexes"
    );
    let child_panel = package.open_surface("panels:01010000").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = child_panel else {
        panic!("Panels remain the native navigation surface when MENU.DIC is absent");
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
fn ssed_mac_panels_plist_opens_like_xml_panels() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("Panels.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>panel</key><dict>
    <key>00000000</key><dict>
      <key>paneltype</key><string>menu</string>
      <key>title</key><string>トップ</string>
      <key>count_x</key><integer>2</integer>
      <key>data</key><array><dict><key>cell</key><array>
        <dict><key>ref</key><string>10000000</string><key>text</key><string>すべて</string></dict>
      </array></dict></array>
    </dict>
    <key>10000000</key><dict>
      <key>paneltype</key><string>contents</string>
      <key>title</key><string>すべて</string>
      <key>data</key><array><dict>
        <key>filename</key><string>Panel/All-A.bin</string>
        <key>type</key><string>bin</string>
      </dict></array>
    </dict>
  </dict>
</dict></plist>"#,
    )
    .unwrap();
    fs::create_dir(dir.path().join("Panel")).unwrap();
    fs::write(dir.path().join("Panel/All-A.bin"), panel_bin_fixture(10, 2)).unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::Panels)
    );
    let root_panel = package.open_surface("panels").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = root_panel else {
        panic!("Mac Panels.plist root should decode as a panel surface");
    };
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].label_text, "すべて");
    assert!(matches!(
        cells[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::PanelCell { panel_id, .. } if panel_id == "10000000"
    ));

    let child_panel = package.open_surface("panels:10000000").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = child_panel else {
        panic!("Mac Panels.plist child should decode BIN rows");
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
fn ssed_ios_mobile_menu_plist_exposes_direct_and_bin_panel_targets() {
    let root = tempdir().unwrap();
    let package_root = root.path().join("DICT");
    fs::create_dir(&package_root).unwrap();
    fs::create_dir(root.path().join("bin")).unwrap();
    fs::write(package_root.join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        root.path().join("menu.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><array>
  <dict>
    <key>item</key><string>直接</string>
    <key>block</key><integer>10</integer>
    <key>offset</key><integer>2</integer>
    <key>child</key><array/>
  </dict>
  <dict>
    <key>item</key><string>五十音</string>
    <key>block</key><integer>0</integer>
    <key>offset</key><integer>0</integer>
    <key>path</key><string>All-A</string>
  </dict>
</array></plist>"#,
    )
    .unwrap();
    fs::write(root.path().join("bin/All-A.bin"), panel_bin_fixture(10, 4)).unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::Panels),
        "parent mobile menu.plist should advertise the Panel capability"
    );
    let home = package.home_surfaces().unwrap();
    assert!(home.iter().any(|surface| {
        surface.surface_id == "panels" && surface.status == NavigationStatus::Available
    }));

    let root_panel = package.open_surface("panels").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = root_panel else {
        panic!("mobile menu.plist should decode to a panel surface");
    };
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0].label_text, "直接");
    assert!(matches!(
        cells[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 10,
            offset: 2,
        } if component == "HONMON.DIC"
    ));
    assert_eq!(cells[1].label_text, "五十音");
    let InternalTarget::PanelCell { panel_id, .. } =
        cells[1].target.as_ref().unwrap().decode().unwrap()
    else {
        panic!("path-backed mobile menu item should point to a child panel");
    };

    let child_panel = package.open_surface(&format!("panels:{panel_id}")).unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = child_panel else {
        panic!("path-backed mobile menu item should decode parent bin rows");
    };
    assert_eq!(cells.len(), 1);
    assert!(matches!(
        cells[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 10,
            offset: 4,
        } if component == "HONMON.DIC"
    ));
}

#[test]
fn empty_ssed_menu_is_not_exposed_as_targetable_home_surface() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("MENU.DIC"),
        sseddata_literal_fixture(b"\x1f\x03"),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert!(!package.metadata().capabilities.contains(&Capability::Menu));
    let surfaces = package.home_surfaces().unwrap();
    let menu_surface = surfaces
        .iter()
        .find(|surface| surface.kind == NavigationSurfaceKind::Menu)
        .expect("declared MENU.DIC should still be reported as a surface");
    assert_eq!(menu_surface.status, NavigationStatus::Empty);
    assert!(menu_surface.target.is_none());
    assert!(
        menu_surface
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_navigation_empty_sentinel")
    );

    let opened = package.open_surface("menu").unwrap();
    let NavigationSurface::Deferred { diagnostics, .. } = opened else {
        panic!("empty MENU.DIC should open as diagnostic-only navigation");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_navigation_empty_sentinel")
    );
}

#[test]
fn multiblock_ssed_menu_without_rows_is_not_advertised_available() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("MENU.DIC"),
        sseddata_literal_fixture(&vec![0; 4096]),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert!(
        !package.metadata().capabilities.contains(&Capability::Menu),
        "multi-block MENU.DIC size alone must not advertise a menu capability"
    );
    let surfaces = package.home_surfaces().unwrap();
    let menu_surface = surfaces
        .iter()
        .find(|surface| surface.kind == NavigationSurfaceKind::Menu)
        .expect("declared readable MENU.DIC should still be visible as a diagnosed surface");
    assert_eq!(menu_surface.status, NavigationStatus::Empty);
    assert!(menu_surface.target.is_none());
    assert!(
        menu_surface
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_navigation_empty")
    );
}

#[test]
fn ssed_menu_rows_with_many_links_expand_to_entry_nodes() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"body"),
    )
    .unwrap();

    fn push_halfwidth_ascii(data: &mut Vec<u8>, text: &str) {
        data.extend_from_slice(&[0x1f, 0x04]);
        data.extend_from_slice(&jis_fullwidth_ascii_key(text));
        data.extend_from_slice(&[0x1f, 0x05]);
    }

    let mut menu = Vec::new();
    menu.extend_from_slice(&[0x1f, 0x09, 0x00, 0x01]);
    menu.extend_from_slice(&[0x1f, 0x42]);
    push_halfwidth_ascii(&mut menu, "alpha");
    menu.extend_from_slice(&[0x22, 0x23]);
    push_halfwidth_ascii(&mut menu, "search-key");
    menu.extend_from_slice(&[0x1f, 0x62]);
    menu.extend_from_slice(&bcd_u32(10));
    menu.extend_from_slice(&bcd_u16(2));
    menu.extend_from_slice(&[0x1f, 0x42]);
    push_halfwidth_ascii(&mut menu, "beta");
    menu.extend_from_slice(&[0x22, 0x23]);
    push_halfwidth_ascii(&mut menu, "other-key");
    menu.extend_from_slice(&[0x1f, 0x62]);
    menu.extend_from_slice(&bcd_u32(10));
    menu.extend_from_slice(&bcd_u16(4));
    menu.extend_from_slice(&[0x1f, 0x0a]);
    fs::write(dir.path().join("MENU.DIC"), sseddata_literal_fixture(&menu)).unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let NavigationSurface::SimpleMenu { nodes, .. } = package.open_surface("menu").unwrap() else {
        panic!("SSED MENU should decode to a simple menu surface");
    };

    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].label_text, "alpha");
    assert_eq!(nodes[1].label_text, "beta");
    assert!(matches!(
        nodes[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 10,
            offset: 2,
        } if component == "HONMON.DIC"
    ));
    assert!(matches!(
        nodes[1].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 10,
            offset: 4,
        } if component == "HONMON.DIC"
    ));
}

#[test]
fn ssed_multi_descriptor_exposes_selector_navigation_without_fake_menu() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_multi_selector(),
    )
    .unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"body"),
    )
    .unwrap();
    fs::write(
        dir.path().join("MULTI1.DIC"),
        sseddata_literal_fixture(&multi_descriptor_fixture()),
    )
    .unwrap();
    fs::write(
        dir.path().join("MUL1_1_1.DIC"),
        sseddata_literal_fixture(&selector_menu_fixture(&["CAT", "DOG"])),
    )
    .unwrap();
    let mut titles = b"alpha title\x1f\x0a".to_vec();
    titles.resize(32, 0);
    titles.extend_from_slice(b"beta title\x1f\x0a");
    fs::write(
        dir.path().join("MUL1_1_2.DIC"),
        sseddata_literal_fixture(&titles),
    )
    .unwrap();
    fs::write(
        dir.path().join("MUL1_1_3.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("CAT", 1, 8, 22, 0),
            ("DOG", 1, 12, 22, 32),
        ])),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    assert!(!package.metadata().capabilities.contains(&Capability::Menu));
    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::MultiSelector)
    );
    let surfaces = package.home_surfaces().unwrap();
    assert!(
        !surfaces
            .iter()
            .any(|surface| surface.kind == NavigationSurfaceKind::Menu)
    );
    let multi_home = surfaces
        .iter()
        .find(|surface| surface.kind == NavigationSurfaceKind::MultiSelector)
        .expect("MULTI descriptor should be a first-class selector surface");
    assert_eq!(multi_home.status, NavigationStatus::Available);

    let root = package.open_surface("multi:MULTI1.DIC").unwrap();
    let NavigationSurface::HierarchicalTree { nodes, .. } = root else {
        panic!("MULTI root should open as selector tree");
    };
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].label_text, "CLASS");
    assert_eq!(nodes[0].children.len(), 2);
    assert_eq!(nodes[0].children[0].label_text, "CAT");
    let target = nodes[0].children[0]
        .target
        .as_ref()
        .expect("selector child should open a filtered title/index browse");
    let InternalTarget::TitleIndexItem { surface_id, .. } = target.decode().unwrap() else {
        panic!("selector child should target a title-index surface");
    };

    let filtered = package.open_surface_page(&surface_id, None, 10).unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = filtered else {
        panic!("selector child should resolve to title/index items");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label_text, "alpha title");
    assert_eq!(
        items[0].target.decode().unwrap(),
        InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 1,
            offset: 8,
        }
    );
}

#[test]
fn ssed_multi_descriptor_resolves_embedded_selector_components() {
    let dir = tempdir().unwrap();
    let record_start = 0x80;
    let mut info = vec![0u8; record_start + 2 * 0x30];
    info[..8].copy_from_slice(SSEDINFO_MAGIC);
    info[0x4d] = 2;
    write_record(
        &mut info[record_start..record_start + 0x30],
        0x00,
        1,
        1,
        "HONMON.DIC",
    );
    write_record(
        &mut info[record_start + 0x30..record_start + 0x60],
        0xff,
        20,
        23,
        "MULTI1.DIC",
    );
    fs::write(dir.path().join("DICT.IDX"), info).unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture_at(1, b"body"),
    )
    .unwrap();

    let mut multi = vec![0u8; 4 * 2048];
    let descriptor = multi_descriptor_fixture();
    multi[..descriptor.len()].copy_from_slice(&descriptor);
    let menu = selector_menu_fixture(&["CAT"]);
    multi[2048..2048 + menu.len()].copy_from_slice(&menu);
    let mut titles = b"alpha title\x1f\x0a".to_vec();
    titles.resize(2048, 0);
    multi[4096..4096 + titles.len()].copy_from_slice(&titles);
    let index = simple_index_fixture_rows(&[("CAT", 1, 0, 22, 0)]);
    multi[6144..6144 + index.len()].copy_from_slice(&index);
    fs::write(
        dir.path().join("MULTI1.DIC"),
        sseddata_literal_fixture_at(20, &multi),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let root = package.open_surface("multi:MULTI1.DIC").unwrap();
    let NavigationSurface::HierarchicalTree { nodes, .. } = root else {
        panic!("embedded MULTI root should open as selector tree");
    };
    assert_eq!(nodes[0].children.len(), 1);
    assert_eq!(nodes[0].children[0].label_text, "CAT");

    let target = nodes[0].children[0]
        .target
        .as_ref()
        .expect("embedded selector child should open filtered title/index browse");
    let InternalTarget::TitleIndexItem { surface_id, .. } = target.decode().unwrap() else {
        panic!("selector child should target a title-index surface");
    };
    let filtered = package.open_surface_page(&surface_id, None, 10).unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = filtered else {
        panic!("embedded selector child should resolve to title/index items");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label_text, "alpha title");
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
            Some(&lvcore::SequenceHint::MenuOrder {
                value: "menu".to_owned(),
            }),
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
            Some(&lvcore::SequenceHint::PanelOrder {
                value: "01010000".to_owned(),
            }),
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
fn ssed_menu_continuous_view_pages_through_large_menu_surfaces() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"body"),
    )
    .unwrap();
    let rows = (0..180u16)
        .map(|index| ([0x24, 0x22], 10u32, index * 2))
        .collect::<Vec<_>>();
    fs::write(
        dir.path().join("MENU.DIC"),
        sseddata_literal_fixture(&menu_stream_fixture_rows(&rows)),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 10,
        offset: 300,
    })
    .unwrap();

    let menu_window = package
        .resolve_target_window(
            &target,
            Some(&lvcore::SequenceHint::MenuOrder {
                value: "menu".to_owned(),
            }),
            1,
            1,
            &RenderOptions::default(),
        )
        .unwrap();

    assert!(menu_window.diagnostics.is_empty());
    assert_eq!(ssed_view_offset(&menu_window.center), Some((10, 300)));
    assert_eq!(menu_window.before.len(), 1);
    assert_eq!(menu_window.after.len(), 1);
    assert_eq!(ssed_view_offset(&menu_window.before[0]), Some((10, 298)));
    assert_eq!(ssed_view_offset(&menu_window.after[0]), Some((10, 302)));
}

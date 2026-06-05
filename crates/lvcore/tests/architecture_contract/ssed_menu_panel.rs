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
fn ssed_panel_bin_compressed_gaiji_labels_resolve_to_sibling_templates() {
    let root = tempdir().unwrap();
    let package_root = root.path().join("DICT");
    let sibling_panel_root = root.path().join("DICT_Panel");
    let sibling_templates_root = root.path().join("DICT_Templates");
    fs::create_dir(&package_root).unwrap();
    fs::create_dir(&sibling_panel_root).unwrap();
    fs::create_dir(&sibling_templates_root).unwrap();
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
        panel_bin_fixture_rows(&[(10, 2, [0xb1, 0x54]), (10, 4, [0xb2, 0x54])]),
    )
    .unwrap();
    fs::write(sibling_templates_root.join("B540.png"), b"png").unwrap();
    fs::write(sibling_templates_root.join("B541.png"), b"png").unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    let child_panel = package.open_surface("panels:01010000").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = child_panel else {
        panic!("SSED child Panel should decode from the sibling sidecar directory");
    };

    assert_eq!(cells.len(), 2);
    assert!(cells[0].diagnostics.is_empty());
    assert!(cells[1].diagnostics.is_empty());
    assert!(cells[0].label_html.contains("lvcore-gaiji-external"));
    assert!(cells[0].label_html.contains(r#"title="B540""#));
    assert!(cells[1].label_html.contains("lvcore-gaiji-external"));
    assert!(cells[1].label_html.contains(r#"title="B541""#));
    assert!(!cells[0].label_html.contains("zB54"));
    assert!(!cells[1].label_html.contains("zB54"));
}

#[test]
fn ssed_missing_aggregate_panel_bin_synthesizes_from_available_content_bins() {
    let root = tempdir().unwrap();
    let package_root = root.path().join("DICT");
    fs::create_dir(&package_root).unwrap();
    fs::create_dir(package_root.join("Panel")).unwrap();
    fs::write(package_root.join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        package_root.join("Panels.xml"),
        r#"<panels>
  <panel index="00000000" paneltype="menu" count_x="2">
    <title>索引</title>
    <data>
      <cell ref="10000000">すべて</cell>
      <cell ref="20100000">あ</cell>
    </data>
  </panel>
  <panel index="10000000" paneltype="contents">
    <title>すべて</title>
    <data type="bin" filename="Panel\DICT_all.bin" />
  </panel>
  <panel index="20100000" paneltype="contents">
    <title>あ</title>
    <data type="bin" filename="Panel\DICT_a.bin" />
  </panel>
  <panel index="20200000" paneltype="contents">
    <title>い</title>
    <data type="bin" filename="Panel\DICT_i.bin" />
  </panel>
</panels>"#,
    )
    .unwrap();
    fs::write(
        package_root.join("Panel/DICT_a.bin"),
        panel_bin_fixture(10, 2),
    )
    .unwrap();
    fs::write(
        package_root.join("Panel/DICT_i.bin"),
        panel_bin_fixture(10, 4),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    let all_panel = package.open_surface("panels:10000000").unwrap();

    let lvcore::NavigationSurface::Panel { cells, .. } = all_panel else {
        panic!("missing aggregate Panel BIN should synthesize from available content BIN rows");
    };
    assert_eq!(cells.len(), 2);
    assert!(
        cells.iter().all(|cell| !cell
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "ssed_panel_bin_missing" })),
        "the missing aggregate BIN is replaced by available content bins"
    );
    assert!(matches!(
        cells[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 10,
            offset: 2,
        } if component == "HONMON.DIC"
    ));
    assert!(matches!(
        cells[1].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 10,
            offset: 4,
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
fn ssed_panels_honor_exinfo_panelxml_metadata_name() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("EXINFO.INI"),
        b"[GENERAL]\nPANELXML=CustomPanels_win.xml\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("CustomPanels_win.xml"),
        r#"<panels>
  <panel index="01000000" paneltype="menu" count_x="1">
    <title>Custom</title>
    <data><cell ref="01010000">あ</cell></data>
  </panel>
</panels>"#,
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::Panels),
        "EXINFO PANELXML should advertise Panels even when Panels.xml is absent"
    );

    let panel = package.open_surface("panels").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = panel else {
        panic!("EXINFO PANELXML should open as an SSED Panel surface");
    };
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].label_text, "あ");
}

#[test]
fn ssed_panels_honor_exinfo_rosqlname_when_it_names_panel_metadata() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("EXINFO.INI"),
        b"[GENERAL]\nROSQLNAME=CustomPanels_win.xml\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("CustomPanels_win.xml"),
        r#"<panels>
  <panel index="01000000" paneltype="menu" count_x="1">
    <title>Custom</title>
    <data><cell ref="01010000">い</cell></data>
  </panel>
</panels>"#,
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::Panels),
        "EXINFO ROSQLNAME should advertise Panels when it names XML/plist Panel metadata"
    );

    let panel = package.open_surface("panels").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = panel else {
        panic!("EXINFO ROSQLNAME XML should open as an SSED Panel surface");
    };
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].label_text, "い");
}

#[test]
fn ssed_panels_ignore_exinfo_rosqlname_database_values() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("EXINFO.INI"),
        b"[GENERAL]\nROSQLNAME=RendererBody.db\n",
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert!(
        !package
            .metadata()
            .capabilities
            .contains(&Capability::Panels),
        "EXINFO ROSQLNAME database values are sidecar hooks, not Panel metadata"
    );

    let panel = package.open_surface("panels").unwrap();
    let lvcore::NavigationSurface::Deferred { diagnostics, .. } = panel else {
        panic!("database ROSQLNAME without Panel metadata should not open as a panel surface");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_panels_missing")
    );
}

#[test]
fn ssed_panel_inline_action_verbs_resolve_addresses_and_panel_refs() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("Panels.xml"),
        r#"<panels>
  <panel index="01000000" paneltype="menu" count_x="2">
    <title>Root</title>
    <data>
      <cell action_verb="lved.addr0000000A:0002">直接</cell>
      <cell action_verb="lved.panel:01010000">子</cell>
    </data>
  </panel>
</panels>"#,
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let panel = package.open_surface("panels").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = panel else {
        panic!("inline Panel action verbs should open as a panel surface");
    };
    assert_eq!(cells.len(), 2);
    assert!(matches!(
        cells[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 10,
            offset: 2,
        } if component == "HONMON.DIC"
    ));
    assert!(matches!(
        cells[1].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::PanelCell { panel_id, .. } if panel_id == "01010000"
    ));
}

#[test]
fn ssed_panel_inline_action_panel_ref_falls_back_to_declared_ref_when_action_id_is_absent() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::create_dir(dir.path().join("Panel")).unwrap();
    fs::write(
        dir.path().join("Panels.xml"),
        r#"<panels>
  <panel index="02000000" paneltype="menu" count_x="1">
    <title>五十音</title>
    <data>
      <cell action_verb="lved.panel:01010000" ref="02010000">あ</cell>
    </data>
  </panel>
  <panel index="02010000" paneltype="contents">
    <title>あ</title>
    <data type="bin" filename="Panel\All-A.bin" />
  </panel>
</panels>"#,
    )
    .unwrap();
    fs::write(dir.path().join("Panel/All-A.bin"), panel_bin_fixture(10, 2)).unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let root_panel = package.open_surface("panels").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = root_panel else {
        panic!("root Panel should open");
    };
    assert_eq!(cells.len(), 1);
    assert!(matches!(
        cells[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::PanelCell { panel_id, .. } if panel_id == "02010000"
    ));

    let child_panel = package.open_surface("panels:02010000").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = child_panel else {
        panic!("declared ref Panel should open");
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
fn ssed_panel_bin_lookup_accepts_extensionless_names_in_panel_directory() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::create_dir(dir.path().join("Panel")).unwrap();
    fs::write(
        dir.path().join("Panels.xml"),
        r#"<panels>
  <panel index="01010000" paneltype="contents">
    <title>All</title>
    <data type="bin" filename="All-A" />
  </panel>
</panels>"#,
    )
    .unwrap();
    fs::write(dir.path().join("Panel/All-A.bin"), panel_bin_fixture(10, 2)).unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let panel = package.open_surface("panels:01010000").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = panel else {
        panic!("extensionless Panel BIN reference should decode as a panel surface");
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
fn ssed_panel_external_html_data_targets_package_html_resource() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::create_dir(dir.path().join("Templates")).unwrap();
    fs::write(
        dir.path().join("Templates/01010000-ffff.html"),
        b"<html><body>panel html</body></html>",
    )
    .unwrap();
    fs::write(
        dir.path().join("Panels.xml"),
        r#"<panels>
  <panel index="01010000" paneltype="contents">
    <title>HTML</title>
    <data type="html" filename="01010000-ffff.html" />
  </panel>
</panels>"#,
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let panel = package.open_surface("panels:01010000").unwrap();
    let NavigationSurface::Panel { cells, .. } = panel else {
        panic!("external Panel HTML should be exposed as a clickable Panel cell");
    };
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].label_text, "HTML");
    assert!(
        !cells[0]
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_panel_bin_missing")
    );
    let target = cells[0].target.as_ref().unwrap().decode().unwrap();
    let InternalTarget::Resource { resource, .. } = target else {
        panic!("external Panel HTML should target a resource");
    };
    let InternalResource::PackageFile {
        path,
        resource_kind,
    } = resource.decode().unwrap()
    else {
        panic!("external Panel HTML should use a package-file resource");
    };
    assert_eq!(path, "Templates/01010000-ffff.html");
    assert_eq!(resource_kind, ResourceKind::Html);
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
fn ssed_ios_mobile_menu_plist_opens_nested_child_panel_without_flattening_root() {
    let root = tempdir().unwrap();
    let package_root = root.path().join("DICT");
    fs::create_dir(&package_root).unwrap();
    fs::write(package_root.join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        root.path().join("menu.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><array>
  <dict>
    <key>item</key><string>親</string>
    <key>child</key><array>
      <dict>
        <key>item</key><string>子</string>
        <key>block</key><integer>10</integer>
        <key>offset</key><integer>6</integer>
      </dict>
    </array>
  </dict>
</array></plist>"#,
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    let root_panel = package.open_surface("panels").unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = root_panel else {
        panic!("mobile root menu should decode to a panel surface");
    };
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].label_text, "親");
    let InternalTarget::PanelCell { panel_id, .. } =
        cells[0].target.as_ref().unwrap().decode().unwrap()
    else {
        panic!("nested mobile menu item should point to a child panel");
    };

    let child_panel = package.open_surface(&format!("panels:{panel_id}")).unwrap();
    let lvcore::NavigationSurface::Panel { cells, .. } = child_panel else {
        panic!("mobile child menu should decode to a panel surface");
    };
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].label_text, "子");
    assert!(matches!(
        cells[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 10,
            offset: 6,
        } if component == "HONMON.DIC"
    ));
}

#[test]
fn ssed_ios_extra_plist_surfaces_are_first_class_navigation() {
    let root = tempdir().unwrap();
    let package_root = root.path().join("DICT");
    fs::create_dir(&package_root).unwrap();
    fs::create_dir(root.path().join("bin")).unwrap();
    fs::create_dir_all(package_root.join("OTHER/_images")).unwrap();
    fs::write(package_root.join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(package_root.join("OTHER/_images/a825.png"), b"png").unwrap();
    fs::write(
        root.path().join("Gaiji.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>a35b</key><string>n</string>
</dict></plist>"#,
    )
    .unwrap();
    fs::write(
        root.path().join("bin/DICT_getDataStrA.bin"),
        panel_bin_fixture(10, 4),
    )
    .unwrap();
    fs::write(
        root.path().join("indexSearch.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><array>
  <dict>
    <key>title</key><string>Foreign Phrases</string>
    <key>block</key><integer>0</integer>
    <key>offset</key><integer>0</integer>
    <key>child</key><array>
      <dict>
        <key>item</key><string>A</string>
        <key>block</key><real>10.0</real>
        <key>offset</key><integer>2</integer>
        <key>child</key><array/>
      </dict>
    </array>
  </dict>
  <dict>
    <key>title</key><string>Usage</string>
    <key>block</key><integer>0</integer>
    <key>offset</key><integer>0</integer>
    <key>child</key><array>
      <dict>
        <key>item</key><string>文語</string>
        <key>path</key><string>DICT_getDataStrA.bin</string>
        <key>block</key><integer>0</integer>
        <key>offset</key><integer>0</integer>
        <key>child</key><array/>
      </dict>
    </array>
  </dict>
</array></plist>"#,
    )
    .unwrap();
    fs::write(
        root.path().join("HTMLList.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><array>
  <dict>
    <key>name</key><array/>
    <key>block</key><integer>10</integer>
    <key>offset</key><integer>8</integer>
    <key>htmlData</key><string>&lt;div class="midashi"&gt;発音記号表&lt;/div&gt;&lt;div&gt;&lt;a href="lved.addr0000000a:0002"&gt;A&lt;/a&gt;&lt;img src="a825.png"&gt;&lt;img src="a35b.png"&gt;&lt;/div&gt;</string>
  </dict>
</array></plist>"#,
    )
    .unwrap();
    fs::write(
        root.path().join("tableList.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><array>
  <dict>
    <key>name</key><string>United States</string>
    <key>block</key><real>10.0</real>
    <key>offset</key><integer>6</integer>
  </dict>
</array></plist>"#,
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    let home = package.home_surfaces().unwrap();
    assert!(home.iter().any(|surface| {
        surface.surface_id == "ios-plist:indexSearch.plist"
            && surface.kind == NavigationSurfaceKind::Panel
            && surface.status == NavigationStatus::Available
    }));
    assert!(home.iter().any(|surface| {
        surface.surface_id == "ios-html-list:HTMLList.plist"
            && surface.kind == NavigationSurfaceKind::Info
            && surface.status == NavigationStatus::Available
    }));
    assert!(home.iter().any(|surface| {
        surface.surface_id == "ios-table-list:tableList.plist"
            && surface.kind == NavigationSurfaceKind::TitleIndexBrowse
            && surface.status == NavigationStatus::Available
    }));

    let root_panel = package.open_surface("ios-plist:indexSearch.plist").unwrap();
    let NavigationSurface::Panel { cells, .. } = root_panel else {
        panic!("indexSearch.plist should open as a panel-style surface");
    };
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0].label_text, "Foreign Phrases");
    assert!(matches!(
        cells[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::MenuItem { surface_id, .. }
            if surface_id == "ios-plist:indexSearch.plist:root.0000"
    ));

    let child_panel = package
        .open_surface("ios-plist:indexSearch.plist:root.0000")
        .unwrap();
    let NavigationSurface::Panel { cells, .. } = child_panel else {
        panic!("indexSearch.plist child should open as a panel-style surface");
    };
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].label_text, "A");
    assert!(matches!(
        cells[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 10,
            offset: 2,
        } if component == "HONMON.DIC"
    ));

    let bin_panel = package
        .open_surface("ios-plist:indexSearch.plist:root.0001")
        .unwrap();
    let NavigationSurface::Panel { cells, .. } = bin_panel else {
        panic!("indexSearch child should open as a panel-style surface");
    };
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].label_text, "文語");
    assert!(matches!(
        cells[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::MenuItem { surface_id, .. }
            if surface_id == "ios-plist:indexSearch.plist:root.0001.0000"
    ));
    let bin_leaf = package
        .open_surface("ios-plist:indexSearch.plist:root.0001.0000")
        .unwrap();
    let NavigationSurface::Panel { cells, .. } = bin_leaf else {
        panic!("path-backed indexSearch leaf should decode its BIN");
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

    let html_surface = package
        .open_surface("ios-html-list:HTMLList.plist")
        .unwrap();
    let NavigationSurface::InfoPages { pages, .. } = html_surface else {
        panic!("HTMLList.plist should expose info pages");
    };
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].label_text, "発音記号表");
    assert!(matches!(
        pages[0].target.decode().unwrap(),
        InternalTarget::SsedIosHtmlPage {
            source_id,
            index: 0,
            ..
        } if source_id == "HTMLList.plist"
    ));
    let rendered = package
        .render_target(&pages[0].target, &RenderOptions::default())
        .unwrap();
    assert_eq!(rendered.kind, ResolvedTargetKind::InfoPage);
    let html = rendered.display_html.unwrap();
    assert!(html.contains("lvcore://target/"));
    assert!(html.contains("lvcore://resource/"));
    assert!(html.contains(r#"class="lvcore-gaiji lvcore-gaiji-ios-plist""#));
    assert!(html.contains(r#"data-gaiji="A35B">n</span>"#));
    assert_eq!(rendered.links.len(), 1);
    assert_eq!(rendered.resources.len(), 1);
    assert!(
        !rendered.diagnostics.iter().any(|diagnostic| diagnostic.code
            == "ssed_sidecar_direct_resource_missing"
            && diagnostic.message.contains("a35b.png"))
    );

    let table_surface = package
        .open_surface("ios-table-list:tableList.plist")
        .unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = table_surface else {
        panic!("tableList.plist should expose title/index rows");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label_text, "United States");
    assert!(matches!(
        items[0].target.decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 10,
            offset: 6,
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
        InternalTarget::SsedBoundedAddress {
            component,
            block: 10,
            offset: 2,
            end_block: 10,
            end_offset: 4,
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

    let NavigationSurface::SimpleMenu {
        nodes, next_cursor, ..
    } = package.open_surface_page("menu", None, 1).unwrap()
    else {
        panic!("SSED MENU page should decode to a simple menu surface");
    };
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].label_text, "alpha");
    assert_eq!(next_cursor.as_deref(), Some("link:0:1"));

    let NavigationSurface::SimpleMenu {
        nodes, next_cursor, ..
    } = package
        .open_surface_page("menu", next_cursor.as_deref(), 1)
        .unwrap()
    else {
        panic!("SSED MENU link cursor should decode to the next link node");
    };
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].label_text, "beta");
    assert!(next_cursor.is_none());
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
    let NavigationSurface::SimpleMenu { nodes, .. } = package.open_surface("menu").unwrap() else {
        panic!("SSED MENU should decode to a simple menu surface");
    };
    assert!(matches!(
        nodes[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedBoundedAddress {
            component,
            block: 10,
            offset: 0,
            end_block: 10,
            end_offset: 2,
        } if component == "HONMON.DIC"
    ));
    assert!(matches!(
        nodes[1].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedBoundedAddress {
            component,
            block: 10,
            offset: 2,
            end_block: 10,
            end_offset: 4,
        } if component == "HONMON.DIC"
    ));

    let NavigationSurface::Panel { cells, .. } = package.open_surface("panels:01010000").unwrap()
    else {
        panic!("SSED Panel should decode to panel cells");
    };
    assert!(matches!(
        cells[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedBoundedAddress {
            component,
            block: 10,
            offset: 0,
            end_block: 10,
            end_offset: 2,
        } if component == "HONMON.DIC"
    ));
    assert!(matches!(
        cells[1].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedBoundedAddress {
            component,
            block: 10,
            offset: 2,
            end_block: 10,
            end_offset: 4,
        } if component == "HONMON.DIC"
    ));

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
                cursor: None,
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
fn ssed_menu_media_component_targets_resolve_as_resource_views() {
    let dir = tempdir().unwrap();
    let bmp = b"BMmenu";
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_menu_and_colscr(),
    )
    .unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture_at(1, b"body"),
    )
    .unwrap();
    fs::write(
        dir.path().join("MENU.DIC"),
        sseddata_literal_fixture_at(11, &menu_stream_fixture_rows(&[([0x24, 0x22], 20, 0)])),
    )
    .unwrap();
    fs::write(
        dir.path().join("COLSCR.DIC"),
        sseddata_literal_fixture_at(20, &colscr_record_fixture(bmp)),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let NavigationSurface::SimpleMenu { nodes, .. } = package.open_surface("menu").unwrap() else {
        panic!("SSED MENU should decode to a simple menu surface");
    };
    let target = nodes[0].target.as_ref().unwrap();
    assert!(matches!(
        target.decode().unwrap(),
        InternalTarget::Resource {
            resource,
            anchor: None,
        } if matches!(
            resource.decode().unwrap(),
            InternalResource::SsedComponentAddress {
                component,
                block: 20,
                offset: 0,
                resource_kind: ResourceKind::Colscr,
            } if component == "COLSCR.DIC"
        )
    ));

    let view = package
        .render_target(target, &RenderOptions::default())
        .unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::MediaResource);
    assert_eq!(view.resources.len(), 1);
    assert_eq!(view.resources[0].kind, ResourceKind::Colscr);
    assert_eq!(
        package.read_resource(&view.resources[0].token).unwrap(),
        bmp
    );
}

#[test]
fn ssed_panel_media_component_targets_resolve_as_resource_views() {
    let dir = tempdir().unwrap();
    let bmp = b"BMpanel";
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_menu_and_colscr(),
    )
    .unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture_at(1, b"body"),
    )
    .unwrap();
    fs::write(
        dir.path().join("COLSCR.DIC"),
        sseddata_literal_fixture_at(20, &colscr_record_fixture(bmp)),
    )
    .unwrap();
    fs::create_dir(dir.path().join("Panel")).unwrap();
    fs::write(
        dir.path().join("Panels.xml"),
        r#"<panels>
  <panel index="01010000" paneltype="contents">
    <title>図版</title>
    <data type="bin" filename="Panel\Images.bin" />
  </panel>
</panels>"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("Panel/Images.bin"),
        panel_bin_fixture(20, 0),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let NavigationSurface::Panel { cells, .. } = package.open_surface("panels:01010000").unwrap()
    else {
        panic!("SSED Panel should decode to panel cells");
    };
    let target = cells[0].target.as_ref().unwrap();
    assert!(matches!(
        target.decode().unwrap(),
        InternalTarget::Resource {
            resource,
            anchor: None,
        } if matches!(
            resource.decode().unwrap(),
            InternalResource::SsedComponentAddress {
                component,
                block: 20,
                offset: 0,
                resource_kind: ResourceKind::Colscr,
            } if component == "COLSCR.DIC"
        )
    ));

    let view = package
        .render_target(target, &RenderOptions::default())
        .unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::MediaResource);
    assert_eq!(view.resources.len(), 1);
    assert_eq!(view.resources[0].kind, ResourceKind::Colscr);
    assert_eq!(
        package.read_resource(&view.resources[0].token).unwrap(),
        bmp
    );
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
                cursor: None,
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

    let page = package.open_surface_page("menu", Some("150"), 10).unwrap();
    let targets = page.actionable_targets();
    assert!(matches!(
        targets[0].sequence_hint.as_ref(),
        Some(lvcore::SequenceHint::MenuOrder {
            value,
            cursor: Some(cursor),
        }) if value == "menu" && cursor == "150"
    ));
    let hinted_window = package
        .resolve_target_window(
            &targets[0].target,
            targets[0].sequence_hint.as_ref(),
            1,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert!(hinted_window.diagnostics.is_empty());
    assert_eq!(ssed_view_offset(&hinted_window.center), Some((10, 300)));
    assert_eq!(ssed_view_offset(&hinted_window.before[0]), Some((10, 298)));
    assert_eq!(ssed_view_offset(&hinted_window.after[0]), Some((10, 302)));
}

#[test]
fn ssed_menu_continuous_view_uses_visible_page_before_full_direct_parse() {
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
    let mut menu = menu_stream_fixture_rows(&rows);
    menu.extend_from_slice(&[0x1f, 0x77]);
    fs::write(dir.path().join("MENU.DIC"), sseddata_literal_fixture(&menu)).unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 10,
        offset: 0,
    })
    .unwrap();

    let menu_window = package
        .resolve_target_window(
            &target,
            Some(&lvcore::SequenceHint::MenuOrder {
                value: "menu".to_owned(),
                cursor: None,
            }),
            0,
            1,
            &RenderOptions::default(),
        )
        .unwrap();

    assert_eq!(ssed_view_offset(&menu_window.center), Some((10, 0)));
    assert_eq!(menu_window.after.len(), 1);
    assert_eq!(ssed_view_offset(&menu_window.after[0]), Some((10, 2)));
    assert!(
        menu_window
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "ssed_navigation_unknown_controls"),
        "first-page menu windows should not parse unrelated tail controls"
    );
}

#[test]
fn ssed_panel_surfaces_are_cursor_paged_and_sequence_can_find_later_cells() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"body"),
    )
    .unwrap();
    fs::create_dir(dir.path().join("Panel")).unwrap();
    fs::write(
        dir.path().join("Panels.xml"),
        r#"<panels>
  <panel index="01010000" paneltype="contents">
    <title>All</title>
    <data type="bin" filename="Panel\All-A.bin" />
  </panel>
</panels>"#,
    )
    .unwrap();
    let rows = (0..130u32)
        .map(|index| (10, index * 2, [0x24, 0x22]))
        .collect::<Vec<_>>();
    fs::write(
        dir.path().join("Panel/All-A.bin"),
        panel_bin_fixture_rows(&rows),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let first_page = package
        .open_surface_page("panels:01010000", None, 100)
        .unwrap();
    let NavigationSurface::Panel {
        cells, next_cursor, ..
    } = first_page
    else {
        panic!("SSED Panel should decode as a paged panel surface");
    };
    assert_eq!(cells.len(), 100);
    assert_eq!(next_cursor.as_deref(), Some("100"));
    assert_eq!(ssed_panel_cell_offset(&cells[0]), Some((10, 0)));
    assert_eq!(ssed_panel_cell_offset(&cells[99]), Some((10, 198)));

    let second_page = package
        .open_surface_page("panels:01010000", next_cursor.as_deref(), 100)
        .unwrap();
    let NavigationSurface::Panel {
        cells, next_cursor, ..
    } = second_page
    else {
        panic!("SSED Panel second page should decode as a panel surface");
    };
    assert_eq!(cells.len(), 30);
    assert!(next_cursor.is_none());
    assert_eq!(ssed_panel_cell_offset(&cells[0]), Some((10, 200)));

    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 10,
        offset: 240,
    })
    .unwrap();
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
    assert!(panel_window.diagnostics.is_empty());
    assert_eq!(ssed_view_offset(&panel_window.before[0]), Some((10, 238)));
    assert_eq!(ssed_view_offset(&panel_window.center), Some((10, 240)));
    assert_eq!(ssed_view_offset(&panel_window.after[0]), Some((10, 242)));
}

fn ssed_panel_cell_offset(cell: &lvcore::navigation::PanelCell) -> Option<(u32, u32)> {
    match cell.target.as_ref()?.decode().ok()? {
        InternalTarget::SsedAddress { block, offset, .. }
        | InternalTarget::SsedBoundedAddress { block, offset, .. } => Some((block, offset)),
        _ => None,
    }
}

fn ssedinfo_fixture_with_menu_and_colscr() -> Vec<u8> {
    let record_start = 0x80;
    let mut data = vec![0u8; record_start + 3 * 0x30];
    data[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Media Navigation Fixture";
    data[0x0c] = title.len() as u8;
    data[0x0d..0x0d + title.len()].copy_from_slice(title);
    data[0x4d] = 3;
    write_record(
        &mut data[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        "HONMON.DIC",
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
        0xd2,
        20,
        20,
        "COLSCR.DIC",
    );
    data
}

fn colscr_record_fixture(payload: &[u8]) -> Vec<u8> {
    let mut record = b"data".to_vec();
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.extend_from_slice(payload);
    record
}

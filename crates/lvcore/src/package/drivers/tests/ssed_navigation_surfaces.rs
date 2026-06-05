use std::fs;

use super::*;

#[test]
fn ssed_screen_menu_surface_exposes_backgrounds_and_hotspot_targets() {
    let dir = tempdir().unwrap();
    let mut screen_menu = Vec::new();
    screen_menu.extend_from_slice(&[0x1f, 0x4c, 0x00, 0x00]);
    screen_menu.extend_from_slice(&screen_menu_image_control(800, 600, 200, 0));
    screen_menu.extend_from_slice(&screen_menu_hotspot_control(10, 20, 30, 40, 100, 0));
    screen_menu.extend_from_slice(&[0x1f, 0x6c]);
    fs::write(
        dir.path().join("SCRMENU.DIC"),
        fixture_sseddata_literal_chunks(&[&screen_menu], 50, 50),
    )
    .unwrap();
    let bmp = b"BMscreen";
    let mut colscr_record = Vec::new();
    colscr_record.extend_from_slice(b"data");
    colscr_record.extend_from_slice(&(bmp.len() as u32).to_le_bytes());
    colscr_record.extend_from_slice(bmp);
    fs::write(
        dir.path().join("COLSCR.DIC"),
        fixture_sseddata_literal_chunks(&[&colscr_record], 200, 200),
    )
    .unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[b"body"], 100, 100),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Screen".to_owned(),
        components: vec![
            SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0x10,
                start_block: 50,
                end_block: 50,
                data: [0; 4],
                filename: "SCRMENU.DIC".to_owned(),
                role: SsedComponentRole::ScreenMenu,
            },
            SsedComponent {
                index: 1,
                multi: 0,
                component_type: 0xd2,
                start_block: 200,
                end_block: 200,
                data: [0; 4],
                filename: "COLSCR.DIC".to_owned(),
                role: SsedComponentRole::Colscr,
            },
            SsedComponent {
                index: 2,
                multi: 0,
                component_type: 0x00,
                start_block: 100,
                end_block: 100,
                data: [0; 4],
                filename: "HONMON.DIC".to_owned(),
                role: SsedComponentRole::Honmon,
            },
        ],
        layout: crate::ssed::SsedInfoLayout {
            component_count_offset: 0,
            record_start: 0,
            record_size: 0x30,
            component_count: 3,
            trailing_bytes: 0,
        },
    };
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Screen".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );

    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::ScreenMenu)
    );
    assert!(package.home_surfaces().unwrap().iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::ScreenMenu
            && surface.status == NavigationStatus::Available
    }));
    let surface = package.open_surface("screen-menu").unwrap();
    let NavigationSurface::ScreenMenu { screens, stats, .. } = surface else {
        panic!("expected screen-menu surface");
    };
    assert_eq!(stats["screens"], 1);
    assert_eq!(screens[0].width, Some(800));
    assert_eq!(screens[0].height, Some(600));
    let background = screens[0].background.as_ref().unwrap();
    assert_eq!(background.kind, ResourceKind::Colscr);
    assert_eq!(package.read_resource(&background.token).unwrap(), bmp);
    assert!(matches!(
        screens[0].hotspots[0].target.as_ref().unwrap().decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 100,
            offset: 0
        } if component == "HONMON.DIC"
    ));
}

#[test]
fn ssed_encyclopedia_index_opens_as_navigation_tree() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("encyclop.idx"),
        cp932(
            "#LVEDBRSR encyclopedia#Ver.1.0 2008.01.07\t\t\n\
                 #図・写真\t\t\n\
                 00000000\t00000000\t図・写真\t\t\n\
                 00000000\t00000000\t\t動物\t\n\
                 000059f9\t000006dc\t\t\t哺乳類\n",
        ),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "KOJIEN6".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 0x5900,
            end_block: 0x5a00,
            data: [0; 4],
            filename: "HONMON.DIC".to_owned(),
            role: SsedComponentRole::Honmon,
        }],
        layout: crate::ssed::SsedInfoLayout {
            component_count_offset: 0,
            record_start: 0,
            record_size: 0x30,
            component_count: 1,
            trailing_bytes: 0,
        },
    };
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("KOJIEN6".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );

    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::EncyclopediaIndex)
    );
    assert!(package.home_surfaces().unwrap().iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::EncyclopediaIndex
            && surface.status == NavigationStatus::Available
    }));
    let surface = package.open_surface("encyclopedia").unwrap();
    let NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
        panic!("expected encyclopedia navigation tree");
    };
    assert_eq!(nodes[0].label_text, "図・写真");
    assert_eq!(nodes[0].children[0].label_text, "動物");
    let target = nodes[0].children[0].children[0]
        .target
        .as_ref()
        .unwrap()
        .decode()
        .unwrap();
    assert!(matches!(
        target,
        InternalTarget::SsedAddress {
            component,
            block: 0x59f9,
            offset: 0x06dc
        } if component == "HONMON.DIC"
    ));
}

#[cfg(unix)]
#[test]
fn ssed_encyclopedia_index_symlink_escape_is_deferred() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(
        outside.path().join("encyclop.idx"),
        cp932(
            "#LVEDBRSR encyclopedia#Ver.1.0 2008.01.07\t\t\n\
                 #Outside\t\t\n\
                 00000000\t00000000\tOutside\t\t\n",
        ),
    )
    .unwrap();
    symlink(
        outside.path().join("encyclop.idx"),
        dir.path().join("encyclop.idx"),
    )
    .unwrap();

    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Sample".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores::default(),
    );

    let surface = package.open_surface("encyclopedia").unwrap();
    let NavigationSurface::Deferred {
        surface_id,
        diagnostics,
    } = surface
    else {
        panic!("expected symlinked encyclop.idx to be deferred");
    };
    assert_eq!(surface_id, "encyclopedia");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ssed_encyclopedia_index_missing"
            || diagnostic.code == "ssed_encyclopedia_index_read_failed"
    }));
}

#[test]
fn ssed_exinfo_auxiliary_index_opens_as_navigation_tree() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("EXINFO.INI"),
        cp932("[GENERAL]\nIDXCOUNT=1\nIDXNAME0=分野\nIDXINFO0=0000015E.IDX\n"),
    )
    .unwrap();
    fs::write(
        dir.path().join("0000015E.IDX"),
        cp932(
            "00000000\t00000000\t大辞林 第四版\n\
                 00005221\t00000722\t\t季語\n\
                 00005221\t000007C2\t\t\t春\n\
                 00005221\t00000750\t\t\t冬\n\
                 10000000\t0000FFFF\t\t西和ABC順\n\
                 01000000\t0000FFFF\t\t五十音\n",
        ),
    )
    .unwrap();
    fs::write(
        dir.path().join("Panels.xml"),
        r#"<?xml version="1.0"?><panels version="1.0"></panels>"#,
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "DAIJIRIN".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 0x5221,
            end_block: 0x5230,
            data: [0; 4],
            filename: "HONMON.DIC".to_owned(),
            role: SsedComponentRole::Honmon,
        }],
        layout: crate::ssed::SsedInfoLayout {
            component_count_offset: 0,
            record_start: 0,
            record_size: 0x30,
            component_count: 1,
            trailing_bytes: 0,
        },
    };
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("DAIJIRIN".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );

    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::AuxiliaryIndex)
    );
    let home = package.home_surfaces().unwrap();
    assert!(home.iter().any(|surface| {
        surface.surface_id == "aux-index:0"
            && surface.kind == NavigationSurfaceKind::AuxiliaryIndex
            && surface.title_text == "分野"
    }));
    let surface = package.open_surface("aux-index:0").unwrap();
    let NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
        panic!("expected auxiliary navigation tree");
    };
    assert_eq!(nodes[0].label_text, "大辞林 第四版");
    assert_eq!(nodes[0].children[0].label_text, "季語");
    let target = nodes[0].children[0]
        .target
        .as_ref()
        .unwrap()
        .decode()
        .unwrap();
    assert!(matches!(
        target,
        InternalTarget::SsedBoundedAddress {
            component,
            block: 0x5221,
            offset: 0x0722,
            end_block: 0x5221,
            end_offset: 0x0750
        } if component == "HONMON.DIC"
    ));
    let target = nodes[0].children[0].children[0]
        .target
        .as_ref()
        .unwrap()
        .decode()
        .unwrap();
    assert!(matches!(
        target,
        InternalTarget::SsedAddress {
            component,
            block: 0x5221,
            offset: 0x07c2
        } if component == "HONMON.DIC"
    ));
    let panel_target = nodes[0].children[1]
        .target
        .as_ref()
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(
        panel_target,
        InternalTarget::PanelCell {
            panel_id: "10000000".to_owned(),
            row: 0,
            column: 0,
        }
    );
    let panel_target = nodes[0].children[2]
        .target
        .as_ref()
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(
        panel_target,
        InternalTarget::PanelCell {
            panel_id: "01000000".to_owned(),
            row: 0,
            column: 0,
        }
    );
    let center = nodes[0].children[0].children[0]
        .target
        .as_ref()
        .unwrap()
        .clone();
    let window = package
        .resolve_target_window(
            &center,
            Some(&SequenceHint::MenuOrder {
                value: "aux-index:0".to_owned(),
                cursor: None,
            }),
            1,
            0,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(window.before.len(), 1);
    assert_eq!(window.before[0].title.as_deref(), Some("季語"));

    let first_page = package.open_surface_page("aux-index:0", None, 2).unwrap();
    let NavigationSurface::HierarchicalTree {
        nodes, next_cursor, ..
    } = first_page
    else {
        panic!("expected paged auxiliary navigation tree");
    };
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].label_text, "大辞林 第四版");
    assert_eq!(nodes[0].children[0].label_text, "季語");
    assert_eq!(next_cursor.as_deref(), Some("2"));

    let second_page = package
        .open_surface_page("aux-index:0", next_cursor.as_deref(), 2)
        .unwrap();
    let NavigationSurface::HierarchicalTree {
        nodes, next_cursor, ..
    } = second_page
    else {
        panic!("expected second paged auxiliary navigation tree");
    };
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].label_text, "春");
    assert_eq!(nodes[1].label_text, "冬");
    assert_eq!(next_cursor.as_deref(), Some("4"));
}

#[test]
fn ssed_auxiliary_index_defers_honmon_targets_inside_entry_marker_controls() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("EXINFO.INI"),
        cp932("[GENERAL]\nIDXINFO=000002BC.idx\nIDXTITLE=付録\n"),
    )
    .unwrap();
    fs::write(
        dir.path().join("000002BC.idx"),
        cp932(
            "00000000\t00000000\tRoot\n\
                 00000002\t00000002\t\tMarker child\n\
                 00000002\t00000004\t\tPayload child\n",
        ),
    )
    .unwrap();
    let body = [
        0x1f, 0x02, 0x1f, 0x09, 0x00, 0x01, 0x1f, 0x41, 0x00, 0x01, 0x1f, 0x61, 0x1f, 0x0a,
    ];
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&body], 2, 2),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Aux marker".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 2,
            end_block: 2,
            data: [0; 4],
            filename: "HONMON.DIC".to_owned(),
            role: SsedComponentRole::Honmon,
        }],
        layout: crate::ssed::SsedInfoLayout {
            component_count_offset: 0,
            record_start: 0,
            record_size: 0x30,
            component_count: 1,
            trailing_bytes: 0,
        },
    };
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Aux marker".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );

    let surface = package.open_surface("aux-index:0").unwrap();
    let NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
        panic!("expected auxiliary navigation tree");
    };
    let marker_child = &nodes[0].children[0];
    let payload_child = &nodes[0].children[1];
    assert!(marker_child.target.is_none());
    assert!(payload_child.target.is_none());
    assert!(
        marker_child
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "ssed_auxiliary_index_body_target_deferred" })
    );
    assert!(
        payload_child
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "ssed_auxiliary_index_body_target_deferred" })
    );
}

#[cfg(unix)]
#[test]
fn ssed_adjacent_panel_symlink_escape_is_not_advertised() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let root = dir.path().join("_DCT_SAMPLE");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("Panels.xml"),
        r#"<?xml version="1.0"?>
<panels>
  <panel index="01000000" paneltype="contents">
    <title>五十音</title>
    <data type="bin" filename="Panel\All-A.bin" />
  </panel>
</panels>"#,
    )
    .unwrap();
    let outside = dir.path().join("outside-panel");
    fs::create_dir(&outside).unwrap();
    let panel_bin = (1u32)
        .to_le_bytes()
        .into_iter()
        .chain((4u32).to_le_bytes())
        .chain((3u32).to_le_bytes())
        .chain((0x20u32).to_le_bytes())
        .chain([0x24, 0x22, 0, 0])
        .collect::<Vec<_>>();
    fs::write(outside.join("All-A.bin"), panel_bin).unwrap();
    symlink(&outside, dir.path().join("_DCT_SAMPLE_Panel")).unwrap();
    let package = ReaderBookPackage::new(
        &root,
        DetectedPackage {
            root: root.clone(),
            format_family: FormatFamily::Ssed,
            confidence: 80,
            title: Some("Panels".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores::default(),
    );

    let surface = package.open_surface("panels:01000000").unwrap();

    let NavigationSurface::Deferred { diagnostics, .. } = surface else {
        panic!("expected deferred surface when adjacent Panel root is a symlink");
    };
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ssed_panel_bin_missing"
            && diagnostic.message.contains("Panel\\All-A.bin")
    }));
}

#[test]
fn ssed_numeric_auxiliary_index_opens_without_exinfo() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("0000015f.idx"),
        cp932(
            "00000000\t00000000\tRoot\n\
                 00005221\t00000722\t\tChild\n\
                 00000001\t0000ffff\t\tPanel selector without panel metadata\n",
        ),
    )
    .unwrap();
    fs::write(dir.path().join("00000001.idx"), SSEDINFO_MAGIC).unwrap();
    let catalog = SsedCatalog {
        title: "Numeric".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 0x5221,
            end_block: 0x5230,
            data: [0; 4],
            filename: "HONMON.DIC".to_owned(),
            role: SsedComponentRole::Honmon,
        }],
        layout: crate::ssed::SsedInfoLayout {
            component_count_offset: 0,
            record_start: 0,
            record_size: 0x30,
            component_count: 1,
            trailing_bytes: 0,
        },
    };
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Numeric".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );

    let home = package.home_surfaces().unwrap();
    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::AuxiliaryIndex)
    );
    assert!(home.iter().any(|surface| {
        surface.surface_id == "numeric-aux:0000015f.idx"
            && surface.kind == NavigationSurfaceKind::AuxiliaryIndex
    }));
    assert!(
        !home
            .iter()
            .any(|surface| surface.surface_id == "numeric-aux:00000001.idx")
    );

    let surface = package.open_surface("numeric-aux:0000015f.idx").unwrap();
    let NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
        panic!("expected numeric auxiliary navigation tree");
    };
    assert_eq!(nodes[0].children.len(), 2);
    let target = nodes[0].children[0]
        .target
        .as_ref()
        .unwrap()
        .decode()
        .unwrap();
    assert!(matches!(
        target,
        InternalTarget::SsedAddress {
            component,
            block: 0x5221,
            offset: 0x0722
        } if component == "HONMON.DIC"
    ));
    assert!(nodes[0].children[1].target.is_none());
    assert!(nodes[0].children[1].diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ssed_auxiliary_index_virtual_selector_without_panels"
    }));
}

#[test]
fn ssed_auxiliary_index_routes_menu_component_targets_as_menu_items() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("0000015f.idx"),
        cp932(
            "00000000\t00000000\tRoot\n\
                 00007539\t00000606\t\tA\n",
        ),
    )
    .unwrap();
    fs::write(dir.path().join("00000001.idx"), SSEDINFO_MAGIC).unwrap();
    let catalog = SsedCatalog {
        title: "Aux Menu".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 0x7539,
            end_block: 0x7602,
            data: [0; 4],
            filename: "MENU.DIC".to_owned(),
            role: SsedComponentRole::Menu,
        }],
        layout: crate::ssed::SsedInfoLayout {
            component_count_offset: 0,
            record_start: 0,
            record_size: 0x30,
            component_count: 1,
            trailing_bytes: 0,
        },
    };
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Aux Menu".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );

    let surface = package.open_surface("numeric-aux:0000015f.idx").unwrap();
    let NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
        panic!("expected numeric auxiliary navigation tree");
    };
    let target = nodes[0].children[0]
        .target
        .as_ref()
        .unwrap()
        .decode()
        .unwrap();
    assert!(matches!(
        target,
        InternalTarget::MenuItem {
            surface_id,
            item_id,
        } if surface_id == "menu" && item_id == format!("addr:{}:{}", 0x7539, 0x0606)
    ));
}

#[test]
fn ssed_ios_table_list_window_uses_plist_order() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[b"alpha body", b"beta body", b"gamma body"], 100, 102),
    )
    .unwrap();
    fs::write(
        dir.path().join("tableList.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<array>
  <dict><key>name</key><string>Alpha</string><key>block</key><integer>100</integer><key>offset</key><integer>0</integer></dict>
  <dict><key>name</key><string>Beta</string><key>block</key><integer>101</integer><key>offset</key><integer>0</integer></dict>
  <dict><key>name</key><string>Gamma</string><key>block</key><integer>102</integer><key>offset</key><integer>0</integer></dict>
</array>
</plist>
"#,
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "iOS table list".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 100,
            end_block: 102,
            data: [0; 4],
            filename: "HONMON.DIC".to_owned(),
            role: SsedComponentRole::Honmon,
        }],
        layout: crate::ssed::SsedInfoLayout {
            component_count_offset: 0,
            record_start: 0,
            record_size: 0x30,
            component_count: 1,
            trailing_bytes: 0,
        },
    };
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("iOS table list".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );

    let surface = package
        .open_surface_page("ios-table-list:tableList.plist", None, 10)
        .unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = &surface else {
        panic!("expected iOS table-list title/index browse surface");
    };
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].label_text, "Alpha");
    assert_eq!(items[1].label_text, "Beta");
    assert_eq!(items[2].label_text, "Gamma");

    let targets = surface.actionable_targets();
    let beta = targets
        .iter()
        .find(|target| target.label_text == "Beta")
        .unwrap();
    assert_eq!(
        beta.sequence_hint,
        Some(SequenceHint::TitleIndexOrder {
            value: "ios-table-list:tableList.plist".to_owned(),
            cursor: Some("1".to_owned()),
        })
    );
    let window = package
        .resolve_target_window(
            &beta.target,
            beta.sequence_hint.as_ref(),
            1,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(window.center.title.as_deref(), Some("Beta"));
    assert_eq!(window.before.len(), 1);
    assert_eq!(window.before[0].title.as_deref(), Some("Alpha"));
    assert_eq!(window.after.len(), 1);
    assert_eq!(window.after[0].title.as_deref(), Some("Gamma"));
    assert!(!window.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "sequence_target_not_in_title_index"
            || diagnostic.code == "sequence_deferred"
    }));
}

#[cfg(unix)]
#[test]
fn ssed_numeric_auxiliary_index_ignores_symlinked_escape() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(
        outside.path().join("00000160.idx"),
        cp932(
            "00000000\t00000000\tOutside\n\
                 00005221\t00000722\t\tEscaped\n",
        ),
    )
    .unwrap();
    symlink(
        outside.path().join("00000160.idx"),
        dir.path().join("00000160.idx"),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Numeric".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 0x5221,
            end_block: 0x5230,
            data: [0; 4],
            filename: "HONMON.DIC".to_owned(),
            role: SsedComponentRole::Honmon,
        }],
        layout: crate::ssed::SsedInfoLayout {
            component_count_offset: 0,
            record_start: 0,
            record_size: 0x30,
            component_count: 1,
            trailing_bytes: 0,
        },
    };
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Numeric".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );

    assert!(
        !package
            .metadata()
            .capabilities
            .contains(&Capability::AuxiliaryIndex)
    );
    assert!(
        !package
            .home_surfaces()
            .unwrap()
            .iter()
            .any(|surface| surface.surface_id == "numeric-aux:00000160.idx")
    );
}

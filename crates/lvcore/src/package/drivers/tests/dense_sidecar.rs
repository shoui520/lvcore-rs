use super::*;

#[test]
fn dense_honmon_body_is_not_exposed_as_numeric_text() {
    let dir = tempdir().unwrap();
    let catalog = SsedCatalog {
        title: String::new(),
        components: Vec::new(),
        layout: crate::ssed::SsedInfoLayout {
            component_count_offset: 0,
            record_start: 0,
            record_size: 0x30,
            component_count: 0,
            trailing_bytes: 0,
        },
    };
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 1,
            title: None,
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores::default(),
    );
    let token = TargetToken::new(&InternalTarget::SsedDenseAnchor {
        anchor: "00100050".to_owned(),
        resolver_hint: Some("vlpljbl".to_owned()),
    })
    .unwrap();
    let body = package.visual_body_for_target(&token).unwrap();
    let text = serde_json::to_string(&body).unwrap();
    assert!(!text.contains("00100050"));
    assert!(matches!(body, VisualBody::Unsupported { .. }));
}

#[test]
fn parses_observed_styled_dense_anchor_records() {
    let mut record = Vec::new();
    record.extend_from_slice(&SSED_ENTRY_MARKER);
    record.extend_from_slice(&[0x1f, 0x41, 0x01, 0x60, 0x1f, 0x04]);
    record.extend_from_slice(&body_jis("00000005"));
    record.extend_from_slice(&[0x1f, 0x05, 0x1f, 0x61, 0x1f, 0x0a]);

    assert_eq!(
        parse_observed_ssed_dense_anchor_id(&record),
        Some("00000005".to_owned())
    );
}

#[test]
fn android_ssed_body_database_uses_rowid_times_five_anchor_rule() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_dense_sidecar_fixture(
        dir.path(),
        DenseSidecarFixture::AndroidRowidTimesFiveBodyRows,
    );
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("DENSE".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 32,
    })
    .unwrap();

    let body = package.visual_body_for_target(&target).unwrap();

    assert_eq!(
        body,
        VisualBody::PreservedHtml {
            html: "<div>android beta html</div>".to_owned(),
            source: BodySourceKind::SidecarHtml,
        }
    );
}

#[test]
fn dense_honmon_address_target_resolves_sidecar_html() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::BodyRows);
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Dense".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 32,
    })
    .unwrap();

    let body = package.visual_body_for_target(&target).unwrap();

    assert_eq!(
        body,
        VisualBody::PreservedHtml {
            html: "<div>beta sidecar html</div>".to_owned(),
            source: BodySourceKind::RendererDatabase,
        }
    );
    let view = package
        .render_target(&target, &RenderOptions::default())
        .unwrap();
    assert_eq!(
        view.display_html.as_deref(),
        Some("<div>beta sidecar html</div>")
    );
}

#[test]
fn dense_sidecar_lved_dataid_links_route_to_ssed_dense_targets() {
    let dir = tempdir().unwrap();
    let package_root = dir.path().join("Book");
    fs::create_dir(&package_root).unwrap();
    fs::create_dir(dir.path().join("img")).unwrap();
    fs::create_dir_all(package_root.join("OTHER/image")).unwrap();
    fs::create_dir_all(package_root.join("HANREI/img")).unwrap();
    fs::write(package_root.join("OTHER/image/b129.png"), b"png-bytes").unwrap();
    fs::write(package_root.join("HANREI/img/b159_M.png"), b"hanrei-png").unwrap();
    fs::write(dir.path().join("img/KG003173.svg"), b"<svg/>").unwrap();
    fs::write(dir.path().join("img/Furoku0.pdf"), b"%PDF").unwrap();
    let catalog =
        write_ssed_dense_sidecar_fixture(&package_root, DenseSidecarFixture::BodyRowsWithLvedLinks);
    let package = ReaderBookPackage::new(
        &package_root,
        DetectedPackage {
            root: package_root.clone(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Dense".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, &package_root),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 32,
    })
    .unwrap();

    let view = package
        .render_target(&target, &RenderOptions::default())
        .unwrap();
    let html = view.display_html.as_deref().unwrap();

    assert!(!html.contains("lved.dataid:"));
    assert!(!html.contains("lved.addr="));
    assert!(!html.contains("src=\"b129.png\""));
    assert!(!html.contains("src=\"furoku01_01.jpg\""));
    assert!(!html.contains("data = \"KG003173.svg\""));
    assert!(html.contains("lvcore://target/"));
    assert!(html.contains("lvcore://resource/"));
    assert_eq!(view.links.len(), 3);
    assert_eq!(view.resources.len(), 5);
    assert!(view.links.iter().any(|link| matches!(
        link.token.decode().unwrap(),
        InternalTarget::SsedDenseAnchor { anchor, resolver_hint: None } if anchor == "00000001"
    )));
    assert!(view.links.iter().any(|link| matches!(
        link.token.decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 100,
            offset: 0,
        } if component == "HONMON.DIC"
    )));
    let self_link = view
        .links
        .iter()
        .find(|link| {
            matches!(
                link.token.decode().unwrap(),
                InternalTarget::SsedDenseAnchor { anchor, resolver_hint: None } if anchor == "00000002"
            )
        })
        .expect("expected self dense-anchor link");
    assert_eq!(
        self_link.attributes.get("html_anchor").map(String::as_str),
        Some("spot")
    );
    assert!(view.resources.iter().any(|resource| {
        resource.label.as_deref() == Some("b129.png")
            && resource.kind == ResourceKind::Image
            && resource.href.is_some()
    }));
    assert!(view.resources.iter().any(|resource| {
        resource.label.as_deref() == Some("KG003173.svg")
            && resource.kind == ResourceKind::Image
            && resource.href.is_some()
    }));
    assert!(view.resources.iter().any(|resource| {
        resource.label.as_deref() == Some("b159_M.png")
            && resource.kind == ResourceKind::Image
            && resource.href.is_some()
    }));
    assert!(view.resources.iter().any(|resource| {
        resource.label.as_deref() == Some("sidecar_pic.png")
            && resource.kind == ResourceKind::Image
            && resource.href.is_some()
    }));
    assert!(view.resources.iter().any(|resource| {
        resource.label.as_deref() == Some("Furoku0.pdf")
            && resource.kind == ResourceKind::Pdf
            && resource.href.is_some()
    }));
    let mut resource_bytes = view
        .resources
        .iter()
        .map(|resource| package.read_resource(&resource.token).unwrap())
        .collect::<Vec<_>>();
    resource_bytes.sort();
    assert_eq!(
        resource_bytes,
        vec![
            b"%PDF".to_vec(),
            b"<svg/>".to_vec(),
            b"hanrei-png".to_vec(),
            b"png-bytes".to_vec(),
            b"\xff\xd8\xff\xe0".to_vec(),
        ]
    );

    let alpha_dense_link = view
        .links
        .iter()
        .find(|link| {
            matches!(
                link.token.decode().unwrap(),
                InternalTarget::SsedDenseAnchor { anchor, resolver_hint: None } if anchor == "00000001"
            )
        })
        .expect("expected alpha dense-anchor link");
    let linked_view = package
        .render_target(&alpha_dense_link.token, &RenderOptions::default())
        .unwrap();
    assert_eq!(
        linked_view.display_html.as_deref(),
        Some("<div>alpha linked sidecar html</div>")
    );
}

#[test]
fn dense_honmon_ordered_honbun_sidecar_resolves_by_entry_slice_order() {
    let dir = tempdir().unwrap();
    let catalog =
        write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::OrderedHonbunRows);
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Ordered Dense".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let beta_offset = u32::try_from(
        ordered_honbun_entry_record("alpha", &["alpha yomi"])
            .len()
            .saturating_add(8),
    )
    .unwrap();
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: beta_offset,
    })
    .unwrap();

    let body = package.visual_body_for_target(&target).unwrap();

    assert_eq!(
        body,
        VisualBody::PreservedHtml {
            html: "<div>ordered beta html</div>".to_owned(),
            source: BodySourceKind::RendererDatabase,
        }
    );
}

#[test]
fn ssed_address_sidecar_resolves_block_offset_plain_body() {
    let dir = tempdir().unwrap();
    let body = {
        let mut body = Vec::new();
        body.extend_from_slice(&[0x1f, 0x09, 0x00, 0x02]);
        body.extend_from_slice(&body_jis("raw body anchor"));
        body
    };
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&body], 100, 100),
    )
    .unwrap();
    let connection = Connection::open(dir.path().join("GENIUSEB.sql")).unwrap();
    connection
        .execute_batch(
            "
            create table GENIUSEB_1 (
              No integer primary key,
              Block integer,
              Offset integer,
              Title text,
              Body text,
              TitleJIS text
            );
            insert into GENIUSEB_1 values (
              35098,
              100,
              4,
              'correlation coefficient',
              'address sidecar body
second line',
              'correlation coefficient'
            );
            ",
        )
        .unwrap();
    let catalog = SsedCatalog {
        title: "iOS sidecar".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 100,
            end_block: 100,
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
            title: Some("iOS sidecar".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 0,
    })
    .unwrap();

    let body = package.visual_body_for_target(&target).unwrap();

    assert_eq!(
        body,
        VisualBody::PreservedHtml {
            html: "<div class=\"lvcore-sidecar-text\">address sidecar body<br>second line</div>"
                .to_owned(),
            source: BodySourceKind::SidecarText,
        }
    );
    let input = package.renderer_input_for_target(&target).unwrap();
    assert!(matches!(
        input,
        RendererInput::PreservedHtml {
            source: BodySourceKind::SidecarText,
            ..
        }
    ));
    let view = package
        .render_target(&target, &RenderOptions::default())
        .unwrap();
    assert_eq!(
        view.display_html.as_deref(),
        Some("<div class=\"lvcore-sidecar-text\">address sidecar body<br>second line</div>")
    );
}

#[test]
fn ios_dictlist_declared_fulldb_is_preferred_for_block_offset_body() {
    let dir = tempdir().unwrap();
    let body = {
        let mut body = Vec::new();
        body.extend_from_slice(&[0x1f, 0x09, 0x00, 0x02]);
        body.extend_from_slice(&body_jis("raw body anchor"));
        body
    };
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&body], 100, 100),
    )
    .unwrap();
    write_block_offset_body_db(
        dir.path().join("A_HELPER.sql"),
        "HELPER_1",
        "wrong helper body",
    );
    let declared = dir.path().join("Z_DECLARED.sql");
    write_block_offset_body_db(declared.clone(), "DECLARED_1", "declared DictFULLDB body");
    let catalog = SsedCatalog {
        title: "iOS sidecar".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 100,
            end_block: 100,
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
            title: Some("iOS sidecar".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            retained_ios_dictlist: Some(crate::ios_dictlist::IosDictListInfo {
                fts_payloads: Vec::new(),
                full_db_payloads: vec![crate::ios_dictlist::IosDictFullDbPayload {
                    relative_path: "DICT/Z_DECLARED.sql".to_owned(),
                    absolute_path: declared,
                    dict_code: "DICT".to_owned(),
                    dictionary_name: Some("iOS DictFULLDB".to_owned()),
                }],
                search_payloads: Vec::new(),
                convert_addr_payloads: Vec::new(),
                search_modes: Vec::new(),
            }),
            ..Default::default()
        },
    );
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 0,
    })
    .unwrap();

    let body = package.visual_body_for_target(&target).unwrap();

    assert_eq!(
        body,
        VisualBody::PreservedHtml {
            html: "<div class=\"lvcore-sidecar-text\">declared DictFULLDB body</div>".to_owned(),
            source: BodySourceKind::SidecarText,
        }
    );
}

#[test]
fn ios_dictsearchdb_advanced_example_search_returns_ssed_address_target() {
    let dir = tempdir().unwrap();
    let search_db = dir.path().join("DICT_Search.sql");
    let connection = Connection::open(&search_db).unwrap();
    connection
        .execute_batch(
            "
            create table D_Example (
              No integer primary key,
              Block integer,
              Offset integer,
              Keyword text,
              Midashi text,
              Title text
            );
            insert into D_Example values (
              1,
              100,
              32,
              'loan example phrase',
              'ignored midashi',
              '1F042361236223631F05'
            );
            ",
        )
        .unwrap();
    let catalog = SsedCatalog {
        title: "iOS search sidecar".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 100,
            end_block: 100,
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
            title: Some("iOS search sidecar".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            retained_ios_dictlist: Some(crate::ios_dictlist::IosDictListInfo {
                fts_payloads: Vec::new(),
                full_db_payloads: Vec::new(),
                search_payloads: vec![crate::ios_dictlist::IosDictSearchPayload {
                    relative_path: "DICT/DICT_Search.sql".to_owned(),
                    absolute_path: search_db,
                    dict_code: "DICT".to_owned(),
                    dictionary_name: Some("iOS SearchDB".to_owned()),
                }],
                convert_addr_payloads: Vec::new(),
                search_modes: vec![SearchMode::Advanced("example".to_owned())],
            }),
            search_modes: vec![SearchMode::Advanced("example".to_owned())],
            ..Default::default()
        },
    );

    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Advanced("example".to_owned()),
            query: "loan".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "abc");
    assert!(
        page.hits[0]
            .snippet_html
            .as_deref()
            .is_some_and(|snippet| snippet.contains("loan example phrase"))
    );
    assert!(page.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ssed_ios_dictsearchdb_scan"
            && diagnostic
                .context
                .get("table")
                .is_some_and(|value| value == "D_Example")
    }));
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedAddress { component, block: 100, offset: 32 }
            if component == "HONMON.DIC"
    ));
}

#[test]
fn ios_fulldb_advanced_example_search_returns_ssed_address_target() {
    let dir = tempdir().unwrap();
    let full_db = dir.path().join("DICT_Full.sql");
    let connection = Connection::open(&full_db).unwrap();
    connection
        .execute_batch(
            "
            create table DICT_1 (
              No integer primary key,
              Block integer,
              Offset integer,
              Title text,
              Body text,
              TitleJIS text
            );
            insert into DICT_1 values (
              1,
              100,
              48,
              'full title',
              'loan example phrase from DictFULLDB',
              '1F042361236223631F05'
            );
            ",
        )
        .unwrap();
    let catalog = SsedCatalog {
        title: "iOS full DB search sidecar".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 100,
            end_block: 100,
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
            title: Some("iOS full DB search sidecar".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            retained_ios_dictlist: Some(crate::ios_dictlist::IosDictListInfo {
                fts_payloads: Vec::new(),
                full_db_payloads: vec![crate::ios_dictlist::IosDictFullDbPayload {
                    relative_path: "DICT/DICT_Full.sql".to_owned(),
                    absolute_path: full_db,
                    dict_code: "DICT".to_owned(),
                    dictionary_name: Some("iOS DictFULLDB".to_owned()),
                }],
                search_payloads: Vec::new(),
                convert_addr_payloads: Vec::new(),
                search_modes: vec![SearchMode::Advanced("example".to_owned())],
            }),
            search_modes: vec![SearchMode::Advanced("example".to_owned())],
            ..Default::default()
        },
    );

    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Advanced("example".to_owned()),
            query: "loan".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "full title");
    assert!(
        page.hits[0]
            .snippet_html
            .as_deref()
            .is_some_and(|snippet| snippet.contains("loan example phrase from DictFULLDB"))
    );
    assert!(page.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ssed_ios_fulldb_search_scan"
            && diagnostic
                .context
                .get("table")
                .is_some_and(|value| value == "DICT_1")
    }));
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedAddress { component, block: 100, offset: 48 }
            if component == "HONMON.DIC"
    ));
}

#[test]
fn ios_dictsearchdb_example_resolver_suppresses_fulldb_fallback() {
    let dir = tempdir().unwrap();
    let search_db = dir.path().join("DICT_Search.sql");
    let search_connection = Connection::open(&search_db).unwrap();
    search_connection
        .execute_batch(
            "
            create table D_Example (
              No integer primary key,
              Block integer,
              Offset integer,
              Keyword text,
              Midashi text,
              Title text
            );
            insert into D_Example values (
              1,
              100,
              32,
              'indexed example phrase',
              'indexed midashi',
              'indexed title'
            );
            ",
        )
        .unwrap();
    let full_db = dir.path().join("DICT_Full.sql");
    let full_connection = Connection::open(&full_db).unwrap();
    full_connection
        .execute_batch(
            "
            create table DICT_1 (
              No integer primary key,
              Block integer,
              Offset integer,
              Title text,
              Body text,
              TitleJIS text
            );
            insert into DICT_1 values (
              1,
              100,
              48,
              'full title',
              'full db only phrase',
              ''
            );
            ",
        )
        .unwrap();
    let catalog = SsedCatalog {
        title: "iOS search precedence".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 100,
            end_block: 100,
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
            title: Some("iOS search precedence".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            retained_ios_dictlist: Some(crate::ios_dictlist::IosDictListInfo {
                fts_payloads: Vec::new(),
                full_db_payloads: vec![crate::ios_dictlist::IosDictFullDbPayload {
                    relative_path: "DICT/DICT_Full.sql".to_owned(),
                    absolute_path: full_db,
                    dict_code: "DICT".to_owned(),
                    dictionary_name: Some("iOS DictFULLDB".to_owned()),
                }],
                search_payloads: vec![crate::ios_dictlist::IosDictSearchPayload {
                    relative_path: "DICT/DICT_Search.sql".to_owned(),
                    absolute_path: search_db,
                    dict_code: "DICT".to_owned(),
                    dictionary_name: Some("iOS SearchDB".to_owned()),
                }],
                convert_addr_payloads: Vec::new(),
                search_modes: vec![SearchMode::Advanced("example".to_owned())],
            }),
            search_modes: vec![SearchMode::Advanced("example".to_owned())],
            ..Default::default()
        },
    );

    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Advanced("example".to_owned()),
            query: "full db only".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert!(page.hits.is_empty());
    assert!(
        !page
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_ios_fulldb_search_scan")
    );
}

#[test]
fn ios_dictsearchdb_address_only_phrase_search_scans_body_windows() {
    let dir = tempdir().unwrap();
    let mut body = Vec::new();
    body.extend_from_slice(&[0x1f, 0x09, 0x00, 0x01]);
    body.extend_from_slice(&body_jis("loan phrase body"));
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&body], 100, 100),
    )
    .unwrap();
    let search_db = dir.path().join("DICT_Search.sql");
    let connection = Connection::open(&search_db).unwrap();
    connection
        .execute_batch(
            "
            create table DICT_seiku (
              Block integer,
              Offset integer
            );
            insert into DICT_seiku values (100, 0);
            ",
        )
        .unwrap();
    let catalog = SsedCatalog {
        title: "iOS phrase addresses".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 100,
            end_block: 100,
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
            title: Some("iOS phrase addresses".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            retained_ios_dictlist: Some(crate::ios_dictlist::IosDictListInfo {
                fts_payloads: Vec::new(),
                full_db_payloads: Vec::new(),
                search_payloads: vec![crate::ios_dictlist::IosDictSearchPayload {
                    relative_path: "DICT/DICT_Search.sql".to_owned(),
                    absolute_path: search_db,
                    dict_code: "DICT".to_owned(),
                    dictionary_name: Some("iOS SearchDB".to_owned()),
                }],
                convert_addr_payloads: Vec::new(),
                search_modes: vec![SearchMode::Advanced("phrase".to_owned())],
            }),
            search_modes: vec![SearchMode::Advanced("phrase".to_owned())],
            ..Default::default()
        },
    );

    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Advanced("phrase".to_owned()),
            query: "loan".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert!(page.hits[0].title_text.contains("loan phrase body"));
    assert!(
        page.hits[0]
            .snippet_html
            .as_deref()
            .is_some_and(|snippet| snippet.contains("loan phrase body"))
    );
    assert!(page.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ssed_ios_address_only_body_scan"
            && diagnostic
                .context
                .get("table")
                .is_some_and(|value| value == "DICT_seiku")
    }));
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 100,
            offset: 0,
        } if component == "HONMON.DIC"
    ));
}

#[test]
fn ios_dictsearchdb_hits_return_direct_ssed_address_targets() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::BodyRows);
    let search_db = dir.path().join("DICT_Search.sql");
    let connection = Connection::open(&search_db).unwrap();
    connection
        .execute_batch(
            "
            create table D_Example (
              No integer primary key,
              Block integer,
              Offset integer,
              Keyword text,
              Title text
            );
            insert into D_Example values (1, 100, 0, 'alpha example', 'alpha');
            ",
        )
        .unwrap();
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("iOS search sidecar".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            retained_ios_dictlist: Some(crate::ios_dictlist::IosDictListInfo {
                fts_payloads: Vec::new(),
                full_db_payloads: Vec::new(),
                search_payloads: vec![crate::ios_dictlist::IosDictSearchPayload {
                    relative_path: "DICT/DICT_Search.sql".to_owned(),
                    absolute_path: search_db,
                    dict_code: "DICT".to_owned(),
                    dictionary_name: Some("iOS SearchDB".to_owned()),
                }],
                convert_addr_payloads: Vec::new(),
                search_modes: vec![SearchMode::Advanced("example".to_owned())],
            }),
            search_modes: vec![SearchMode::Advanced("example".to_owned())],
            ..Default::default()
        },
    );

    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Advanced("example".to_owned()),
            query: "alpha".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 100,
            offset: 0,
        } if component == "HONMON.DIC"
    ));
}

#[test]
fn ios_convert_addr_db_canonicalizes_direct_ssed_address_targets() {
    let dir = tempdir().unwrap();
    let body = {
        let mut body = Vec::new();
        body.extend_from_slice(&[0x1f, 0x09, 0x00, 0x02]);
        body.extend_from_slice(&body_jis("raw converted body"));
        body
    };
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&body], 100, 100),
    )
    .unwrap();
    write_block_offset_body_db(
        dir.path().join("DICT_Full.sql"),
        "DICT_1",
        "converted address body",
    );
    let full_db = dir.path().join("DICT_Full.sql");
    let full_connection = Connection::open(&full_db).unwrap();
    full_connection
        .execute("update DICT_1 set Offset = 4 where Block = 100", [])
        .unwrap();
    let convert_db = dir.path().join("DICT_on.sql");
    let convert_connection = Connection::open(&convert_db).unwrap();
    convert_connection
        .execute_batch(
            "
            create table DICT (
              o_Block text,
              o_Offset text,
              n_Block text,
              n_Offset text
            );
            insert into DICT values ('100', '64', '100', '4');
            ",
        )
        .unwrap();
    let catalog = SsedCatalog {
        title: "iOS converted address".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 100,
            end_block: 100,
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
            title: Some("iOS converted address".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            retained_ios_dictlist: Some(crate::ios_dictlist::IosDictListInfo {
                fts_payloads: Vec::new(),
                full_db_payloads: vec![crate::ios_dictlist::IosDictFullDbPayload {
                    relative_path: "DICT/DICT_Full.sql".to_owned(),
                    absolute_path: full_db,
                    dict_code: "DICT".to_owned(),
                    dictionary_name: Some("iOS FullDB".to_owned()),
                }],
                search_payloads: Vec::new(),
                convert_addr_payloads: vec![crate::ios_dictlist::IosDictConvertAddrPayload {
                    relative_path: "DICT/DICT_on.sql".to_owned(),
                    absolute_path: convert_db,
                    dict_code: "DICT".to_owned(),
                    dictionary_name: Some("iOS ConvertAddr".to_owned()),
                }],
                search_modes: Vec::new(),
            }),
            ..Default::default()
        },
    );
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 64,
    })
    .unwrap();

    let body = package.visual_body_for_target(&target).unwrap();

    assert_eq!(
        body,
        VisualBody::PreservedHtml {
            html: "<div class=\"lvcore-sidecar-text\">converted address body</div>".to_owned(),
            source: BodySourceKind::SidecarText,
        }
    );
}

#[test]
fn ios_dictsearchdb_hits_emit_converted_ssed_address_targets() {
    let dir = tempdir().unwrap();
    let catalog = SsedCatalog {
        title: "iOS converted search".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 100,
            end_block: 100,
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
    let search_db = dir.path().join("DICT_Search.sql");
    let search_connection = Connection::open(&search_db).unwrap();
    search_connection
        .execute_batch(
            "
            create table D_Example (
              No integer primary key,
              Block integer,
              Offset integer,
              Keyword text,
              Title text
            );
            insert into D_Example values (1, 100, 64, 'converted search', 'converted');
            ",
        )
        .unwrap();
    let convert_db = dir.path().join("DICT_on.sql");
    let convert_connection = Connection::open(&convert_db).unwrap();
    convert_connection
        .execute_batch(
            "
            create table DICT (
              o_Block text,
              o_Offset text,
              n_Block text,
              n_Offset text
            );
            insert into DICT values ('100', '64', '100', '4');
            ",
        )
        .unwrap();
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("iOS converted search".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            retained_ios_dictlist: Some(crate::ios_dictlist::IosDictListInfo {
                fts_payloads: Vec::new(),
                full_db_payloads: Vec::new(),
                search_payloads: vec![crate::ios_dictlist::IosDictSearchPayload {
                    relative_path: "DICT/DICT_Search.sql".to_owned(),
                    absolute_path: search_db,
                    dict_code: "DICT".to_owned(),
                    dictionary_name: Some("iOS SearchDB".to_owned()),
                }],
                convert_addr_payloads: vec![crate::ios_dictlist::IosDictConvertAddrPayload {
                    relative_path: "DICT/DICT_on.sql".to_owned(),
                    absolute_path: convert_db,
                    dict_code: "DICT".to_owned(),
                    dictionary_name: Some("iOS ConvertAddr".to_owned()),
                }],
                search_modes: vec![SearchMode::Advanced("example".to_owned())],
            }),
            search_modes: vec![SearchMode::Advanced("example".to_owned())],
            ..Default::default()
        },
    );

    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Advanced("example".to_owned()),
            query: "converted".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedAddress { component, block: 100, offset: 4 }
            if component == "HONMON.DIC"
    ));
}

#[test]
fn dense_honmon_search_hit_target_resolves_sidecar_html() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::BodyRows);
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Dense".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "い".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "beta");
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedAddress { .. } | InternalTarget::SsedIndexAddress { .. }
    ));
    let body = package
        .visual_body_for_target(&page.hits[0].target)
        .unwrap();
    assert!(matches!(
        body,
        VisualBody::PreservedHtml {
            source: BodySourceKind::RendererDatabase,
            ..
        }
    ));
}

#[test]
fn dense_honmon_fulltext_searches_sidecar_body() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::BodyRows);
    let search_modes = ssed_search_modes(&catalog, dir.path());
    assert!(search_modes.contains(&SearchMode::FullText));
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Dense".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            search_modes,
            ..Default::default()
        },
    );

    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::FullText,
            query: "beta sidecar body".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "beta");
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedDenseAnchor { anchor, .. } if anchor == "2"
    ));
    assert!(
        page.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_sidecar_scan")
    );
    assert!(page.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ssed_fulltext_honmon_scan_skipped_sidecar_backed"
    }));
    assert!(
        !page
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_body_window_scan")
    );
    let view = package
        .render_target(&page.hits[0].target, &RenderOptions::default())
        .unwrap();
    assert_eq!(
        view.display_html.as_deref(),
        Some("<div>beta sidecar html</div>")
    );
}

#[test]
fn dense_honmon_fulltext_decodes_entity_title_labels() {
    let dir = tempdir().unwrap();
    let catalog =
        write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::EntityTitleRows);
    let search_modes = ssed_search_modes(&catalog, dir.path());
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Dense".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            search_modes,
            ..Default::default()
        },
    );

    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::FullText,
            query: "entity sidecar body".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "à *abaisser");
    assert!(page.hits[0].title_html.contains("à *abaisser"));
    assert!(!page.hits[0].title_text.contains("&#x"));
    assert!(!page.hits[0].title_html.contains("&#x"));
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedDenseAnchor { anchor, .. } if anchor == "2"
    ));
}

#[test]
fn title_only_sidecar_does_not_block_dense_body_sidecar() {
    let dir = tempdir().unwrap();
    let catalog =
        write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::TitleOnlyThenBodyRows);
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Dense".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 0,
    })
    .unwrap();

    let body = package.visual_body_for_target(&target).unwrap();

    assert_eq!(
        body,
        VisualBody::PreservedHtml {
            html: "<div>alpha sidecar html</div>".to_owned(),
            source: BodySourceKind::RendererDatabase,
        }
    );
}

#[test]
fn sharded_t_contents_sidecar_tables_are_all_considered() {
    let dir = tempdir().unwrap();
    let catalog =
        write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::ShardedTContentsBodyRows);
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Dense".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 32,
    })
    .unwrap();

    let body = package.visual_body_for_target(&target).unwrap();

    assert_eq!(
        body,
        VisualBody::PreservedHtml {
            html: "<div>beta sharded html</div>".to_owned(),
            source: BodySourceKind::RendererDatabase,
        }
    );
}

#[test]
fn dense_sidecar_decodes_utf8_and_cp932_blob_text() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::BlobBodyRows);
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Dense".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let beta = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 32,
    })
    .unwrap();

    let body = package.visual_body_for_target(&beta).unwrap();

    assert_eq!(
        body,
        VisualBody::PreservedHtml {
            html: "<div>ベータ html</div>".to_owned(),
            source: BodySourceKind::RendererDatabase,
        }
    );
    assert!(!serde_json::to_string(&body).unwrap().contains("b'"));
}

#[test]
fn dense_sidecar_missing_row_is_unsupported_without_anchor_leak() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::MissingBetaRow);
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Dense".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 32,
    })
    .unwrap();

    let body = package.visual_body_for_target(&target).unwrap();
    let json = serde_json::to_string(&body).unwrap();

    assert!(matches!(body, VisualBody::Unsupported { .. }));
    assert!(!json.contains("00000002"));
    assert!(json.contains("ssed_dense_sidecar_row_missing"));
}

#[cfg(unix)]
#[test]
fn dense_sidecar_discovery_ignores_symlinked_sqlite_escape() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let catalog =
        write_ssed_dense_sidecar_fixture(root.path(), DenseSidecarFixture::MissingBetaRow);
    write_dense_body_db(outside.path().join("body.db"), true, true, false);
    fs::remove_file(root.path().join("body.db")).unwrap();
    symlink(outside.path().join("body.db"), root.path().join("body.db")).unwrap();

    let package = ReaderBookPackage::new(
        root.path(),
        DetectedPackage {
            root: root.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Dense".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, root.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 32,
    })
    .unwrap();

    let body = package.visual_body_for_target(&target).unwrap();
    let json = serde_json::to_string(&body).unwrap();
    assert!(!json.contains("beta sidecar html"));
    assert!(!matches!(body, VisualBody::PreservedHtml { .. }));
}

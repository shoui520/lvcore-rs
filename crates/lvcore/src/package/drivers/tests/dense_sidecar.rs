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
    let view = package
        .render_target(&page.hits[0].target, &RenderOptions::default())
        .unwrap();
    assert_eq!(
        view.display_html.as_deref(),
        Some("<div>beta sidecar html</div>")
    );
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

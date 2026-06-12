use std::fs;

use rusqlite::Connection;

use super::*;

#[test]
fn ssed_hc_renderer_input_carries_stream_resource_refs() {
    let dir = tempdir().unwrap();
    let pcm_chunks = pcmdata_wave_chunks_for_test(1, b"\x80\x81\x82");
    let mut figure_payload = vec![0_u8; 17];
    figure_payload.extend_from_slice(&[0x80, 0x80, 0x7f, 0x00]);
    let mut honmon = Vec::new();
    honmon.extend_from_slice(&SSED_ENTRY_MARKER);
    honmon.extend_from_slice(&[
        0x1f, 0x4a, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
        0x00, 0x00, 0x34,
    ]);
    honmon.extend_from_slice(&[
        0x1f, 0x44, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x09,
    ]);
    honmon.extend_from_slice(&[0x1f, 0x64, 0x00, 0x00, 0x12, 0x00, 0x00, 0x17]);
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
    )
    .unwrap();
    fs::write(
        dir.path().join("PCMDATA.DIC"),
        fixture_sseddata_literal_chunks(&[&pcm_chunks], 500, 500),
    )
    .unwrap();
    fs::write(
        dir.path().join("FIGURE.DIC"),
        fixture_sseddata_literal_chunks(&[&figure_payload], 1200, 1200),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Renderer resources".to_owned(),
        components: vec![
            SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0x00,
                start_block: 100,
                end_block: 100,
                data: [0; 4],
                filename: "HONMON.DIC".to_owned(),
                role: SsedComponentRole::Honmon,
            },
            SsedComponent {
                index: 1,
                multi: 0,
                component_type: 0xd8,
                start_block: 500,
                end_block: 500,
                data: [0; 4],
                filename: "PCMDATA.DIC".to_owned(),
                role: SsedComponentRole::PcmData,
            },
            SsedComponent {
                index: 2,
                multi: 0,
                component_type: 0xd0,
                start_block: 1200,
                end_block: 1200,
                data: [0; 4],
                filename: "FIGURE.DIC".to_owned(),
                role: SsedComponentRole::Figure,
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
            confidence: 80,
            title: Some("Renderer resources".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 0,
    })
    .unwrap();

    let input = package.renderer_input_for_target(&token).unwrap();
    let RendererInput::HcSsedStream {
        resources,
        diagnostics,
        ..
    } = input
    else {
        panic!("SSED address should produce HC renderer input");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "hc_renderer_input_ready")
    );
    assert!(resources.iter().any(|resource| {
        resource.kind == ResourceKind::PcmData && resource.mime_type.as_deref() == Some("audio/wav")
    }));
    assert!(resources.iter().any(|resource| {
        resource.kind == ResourceKind::Image
            && resource.label.as_deref() == Some("FIGURE.DIC:00001200:0017:9x2")
    }));

    let view = package
        .render_target(&token, &RenderOptions::default())
        .unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::EntryBody);
    let html = view.display_html.as_deref().unwrap_or_default();
    assert!(html.contains("<audio"));
    assert!(html.contains("lvcore://resource/"));
    assert_eq!(view.resources.len(), resources.len());
    assert!(view.capabilities.contains(&RenderCapability::HcRenderInput));
    assert!(view.capabilities.contains(&RenderCapability::Images));
    assert!(view.capabilities.contains(&RenderCapability::Audio));
    assert!(
        view.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "hc_render_common_html_fallback")
    );
}

#[test]
fn ssed_common_html_links_normalize_packed_honmon_block_addresses() {
    let dir = tempdir().unwrap();
    let mut honmon = Vec::new();
    honmon.extend_from_slice(&SSED_ENTRY_MARKER);
    honmon.extend_from_slice(&body_jis("前"));
    honmon.extend_from_slice(&[
        0x1f, 0x44, 0xaa, 0xbb, 0xcc, 0xdd, 0x00, 0x64, 0x00, 0x00, 0x00, 0x64,
    ]);
    honmon.extend_from_slice(&body_jis("リンク"));
    honmon.extend_from_slice(&[0x1f, 0x64, 0, 0, 0, 0, 0, 0]);
    honmon.extend_from_slice(&body_jis("後"));
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Packed link block".to_owned(),
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
            confidence: 80,
            title: Some("Packed link block".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 0,
    })
    .unwrap();

    let view = package
        .render_target(&token, &RenderOptions::default())
        .unwrap();

    assert_eq!(view.links.len(), 1);
    assert!(matches!(
        view.links[0].token.decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 100,
            offset: 100,
        } if component == "HONMON.DIC"
    ));
    assert!(
        view.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "ssed_loose_address_unresolved")
    );
    let html = view.display_html.as_deref().unwrap_or_default();
    assert!(html.contains("href=\"lvcore://target/"));
    assert!(!html.contains("href=\"lvaddr://06553600/0100\""));
}

#[test]
fn ssed_basic_text_uses_logovista_gaiji_placeholders_for_unresolved_stream_pairs() {
    let dir = tempdir().unwrap();
    let mut honmon = Vec::new();
    honmon.extend_from_slice(&SSED_ENTRY_MARKER);
    honmon.extend_from_slice(&body_jis("前"));
    honmon.extend_from_slice(&[0xa1, 0x40, 0xb1, 0x23]);
    honmon.extend_from_slice(&body_jis("後"));
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Renderer gaiji placeholders".to_owned(),
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
            confidence: 80,
            title: Some("Renderer gaiji placeholders".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 0,
    })
    .unwrap();

    let view = package
        .render_target(
            &token,
            &RenderOptions {
                mode: RenderMode::BasicText,
                ..RenderOptions::default()
            },
        )
        .unwrap();

    assert_eq!(view.basic_text.as_deref(), Some("前<hA140><zB123>後"));
    assert!(
        view.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "hc_basic_text_gaiji_placeholders")
    );
}

#[test]
fn ssed_hc03e9_pdfspread_resource_is_exposed_from_page_anchor() {
    let dir = tempdir().unwrap();
    let page_anchor = [
        0x23, 0x30, 0x23, 0x30, 0x23, 0x30, 0x23, 0x30, 0x23, 0x30, 0x23, 0x30, 0x23, 0x31, 0x23,
        0x37,
    ];
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&page_anchor], 100, 100),
    )
    .unwrap();
    let connection = Connection::open(dir.path().join("HKRKIKHY2.db")).unwrap();
    connection
            .execute_batch(
                r#"
                create table PDFSpread (IDRight text primary key, IDLeft text, PDF blob);
                insert into PDFSpread values ('００００００１７', '００００００１６', X'255044462d706466737072656164');
                "#,
            )
            .unwrap();
    drop(connection);
    fs::write(dir.path().join("._HKRKIKHY2.db"), b"metadata").unwrap();
    let catalog = SsedCatalog {
        title: "PDFSpread".to_owned(),
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
            confidence: 80,
            title: Some("PDFSpread".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
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

    let input = package.renderer_input_for_target(&target).unwrap();
    let RendererInput::HcSsedStream { resources, .. } = input else {
        panic!("SSED address should produce HC renderer input");
    };
    let pdf = resources
        .iter()
        .find(|resource| resource.kind == ResourceKind::Pdf)
        .expect("PDFSpread resource should be exposed");

    assert_eq!(pdf.label.as_deref(), Some("PDFSpread/００００００１７"));
    assert_eq!(pdf.mime_type.as_deref(), Some("application/pdf"));
    assert_eq!(
        package.read_resource(&pdf.token).unwrap(),
        b"%PDF-pdfspread"
    );
}

#[test]
fn ssed_hc_profile_hint_uses_exinfo_htmldll_without_binary() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[b"body"], 100, 100),
    )
    .unwrap();
    fs::write(
        dir.path().join("EXINFO.INI"),
        b"[GENERAL]\r\nHTMLDLL=HC03E9.dll\r\n",
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "EXINFO".to_owned(),
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
            confidence: 80,
            title: Some("EXINFO".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
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

    let input = package.renderer_input_for_target(&target).unwrap();
    let RendererInput::HcSsedStream {
        profile_hint,
        hc_profile,
        ..
    } = input
    else {
        panic!("SSED address should produce HC renderer input");
    };
    assert_eq!(profile_hint.as_deref(), Some("HC03E9"));
    let hc_profile = hc_profile.expect("EXINFO HTMLDLL should become HC profile metadata");
    assert_eq!(hc_profile.profile_id, "HC03E9");
    assert_eq!(hc_profile.source, HcRendererProfileSource::ExinfoHtmlDll);
    assert_eq!(hc_profile.status, HcRendererProfileStatus::InputOnly);
    assert_eq!(hc_profile.dll_sha256, None);
    assert_eq!(hc_profile.dll_size, None);
}

#[test]
fn ssed_hc_renderer_input_uses_marker_entry_length_for_resource_scan() {
    let dir = tempdir().unwrap();
    let first_pcm = pcmdata_wave_chunks_for_test(1, b"\x80");
    let second_pcm = pcmdata_wave_chunks_for_test(1, b"\x81");
    let first_audio =
        pcmdata_range_control_for_test(500, 0, 500, u32::try_from(first_pcm.len() - 1).unwrap());
    let second_audio =
        pcmdata_range_control_for_test(501, 0, 501, u32::try_from(second_pcm.len() - 1).unwrap());
    let mut honmon = Vec::new();
    honmon.extend_from_slice(&SSED_ENTRY_MARKER);
    honmon.extend_from_slice(b"first");
    honmon.extend_from_slice(&first_audio);
    let second_entry_offset = honmon.len();
    honmon.extend_from_slice(&SSED_ENTRY_MARKER);
    honmon.extend_from_slice(b"second");
    honmon.extend_from_slice(&second_audio);

    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
    )
    .unwrap();
    fs::write(
        dir.path().join("PCMDATA.DIC"),
        fixture_sseddata_literal_chunks(&[&first_pcm, &second_pcm], 500, 501),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Bounded renderer scan".to_owned(),
        components: vec![
            SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0x00,
                start_block: 100,
                end_block: 100,
                data: [0; 4],
                filename: "HONMON.DIC".to_owned(),
                role: SsedComponentRole::Honmon,
            },
            SsedComponent {
                index: 1,
                multi: 0,
                component_type: 0xd8,
                start_block: 500,
                end_block: 501,
                data: [0; 4],
                filename: "PCMDATA.DIC".to_owned(),
                role: SsedComponentRole::PcmData,
            },
        ],
        layout: crate::ssed::SsedInfoLayout {
            component_count_offset: 0,
            record_start: 0,
            record_size: 0x30,
            component_count: 2,
            trailing_bytes: 0,
        },
    };
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 80,
            title: Some("Bounded renderer scan".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 0,
    })
    .unwrap();

    let input = package.renderer_input_for_target(&token).unwrap();
    let RendererInput::HcSsedStream {
        length,
        resources,
        diagnostics,
        ..
    } = input
    else {
        panic!("SSED address should produce HC renderer input");
    };
    assert_eq!(length, Some(second_entry_offset as u64));
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "ssed_renderer_resource_scan_bounded")
    );
    assert_eq!(resources.len(), 1);
    let expected_label = format!(
        "PCMDATA.DIC:00000500:0000-00000500:{:04}",
        first_pcm.len() - 1
    );
    assert_eq!(resources[0].label.as_deref(), Some(expected_label.as_str()));
}

#[test]
fn ssed_hc_renderer_input_uses_sidecar_range_bound_for_sparse_index_targets() {
    let dir = tempdir().unwrap();
    let honmon = vec![b'x'; 512];
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
    )
    .unwrap();
    let connection = Connection::open(dir.path().join("GENIUSEB.db")).unwrap();
    connection
        .execute_batch(
            "
            create table GENIUSEB (
              Block_s integer,
              Offset_s integer,
              Block_e integer,
              Offset_e integer,
              Title text,
              JIS_Title text
            );
            insert into GENIUSEB values (100, 40, 100, 80, 'next nearby range', '');
            ",
        )
        .unwrap();
    let catalog = SsedCatalog {
        title: "Sparse range".to_owned(),
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
            confidence: 80,
            title: Some("Sparse range".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let target = TargetToken::new(&InternalTarget::SsedBoundedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 10,
        end_block: 100,
        end_offset: 400,
    })
    .unwrap();

    let input = package.renderer_input_for_target(&target).unwrap();
    let RendererInput::HcSsedStream { length, .. } = input else {
        panic!("SSED bounded address should produce HC renderer input");
    };

    assert_eq!(length, Some(30));
}

#[test]
fn ssed_hc_renderer_input_uses_local_boundary_for_marker_variants() {
    let dir = tempdir().unwrap();
    let mut honmon = Vec::new();
    honmon.extend_from_slice(&[0x1f, 0x09, 0x00, 0x02]);
    honmon.extend_from_slice(b"first");
    let second_entry_offset = honmon.len();
    honmon.extend_from_slice(&[0x1f, 0x09, 0x00, 0x02]);
    honmon.extend_from_slice(b"second");
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Local boundaries".to_owned(),
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
            confidence: 80,
            title: Some("Local boundaries".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 0,
    })
    .unwrap();

    let input = package.renderer_input_for_target(&token).unwrap();
    let RendererInput::HcSsedStream { length, .. } = input else {
        panic!("SSED address should produce HC renderer input");
    };
    assert_eq!(length, Some(second_entry_offset as u64));
}

#[test]
fn ssed_hc_renderer_input_uses_local_boundary_for_0101_marker_variants() {
    let dir = tempdir().unwrap();
    let mut honmon = Vec::new();
    honmon.extend_from_slice(&[0x1f, 0x09, 0x01, 0x01]);
    honmon.extend_from_slice(b"head");
    honmon.extend_from_slice(&[0x1f, 0x09, 0x01, 0x03]);
    honmon.extend_from_slice(b"section");
    let second_entry_offset = honmon.len();
    honmon.extend_from_slice(&SSED_ENTRY_MARKER);
    honmon.extend_from_slice(b"next");
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Local 0101 boundaries".to_owned(),
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
            confidence: 80,
            title: Some("Local 0101 boundaries".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 0,
    })
    .unwrap();

    let input = package.renderer_input_for_target(&token).unwrap();
    let RendererInput::HcSsedStream { length, .. } = input else {
        panic!("SSED address should produce HC renderer input");
    };
    assert_eq!(length, Some(second_entry_offset as u64));
}

#[test]
fn ssed_hc_renderer_input_uses_metadata_record_close_boundary() {
    let dir = tempdir().unwrap();
    let mut honmon = Vec::new();
    honmon.extend_from_slice(&[0x1f, 0x09, 0x99, 0x99]);
    honmon.extend_from_slice(&[0x1f, 0xe2, 0x00, 0x02, 0x23, 0x31, 0x23, 0x32, 0x1f, 0xe3]);
    honmon.extend_from_slice(&[0x1f, 0x09, 0x99, 0x99]);
    honmon.extend_from_slice(&[0x1f, 0xe2, 0x00, 0x02, 0x23, 0x33, 0x23, 0x34, 0x1f, 0xe3]);
    honmon.extend_from_slice(&[0x1f, 0x09, 0x00, 0x03]);
    honmon.extend_from_slice(b"first");
    honmon.extend_from_slice(&[0x1f, 0x09, 0x00, 0x02, 0x1f, 0x41, 0x01, 0x60]);
    honmon.extend_from_slice(b"child");
    honmon.extend_from_slice(&[0x1f, 0x61, 0x1f, 0x0a]);
    let second_entry_offset = honmon.len();
    honmon.extend_from_slice(&[0x1f, 0x09, 0x99, 0x99]);
    honmon.extend_from_slice(&[0x1f, 0xe2, 0x00, 0x02, 0x23, 0x35, 0x23, 0x36, 0x1f, 0xe3]);
    honmon.extend_from_slice(b"second");
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Metadata record boundaries".to_owned(),
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
            confidence: 80,
            title: Some("Metadata record boundaries".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 0,
    })
    .unwrap();

    let input = package.renderer_input_for_target(&token).unwrap();
    let RendererInput::HcSsedStream { length, .. } = input else {
        panic!("SSED address should produce HC renderer input");
    };
    assert_eq!(length, Some(second_entry_offset as u64));
}

#[test]
fn ssed_hc_renderer_input_does_not_scan_index_for_markerless_stream_length() {
    let dir = tempdir().unwrap();
    let second_pcm = pcmdata_wave_chunks_for_test(1, b"\x81");
    let second_audio =
        pcmdata_range_control_for_test(500, 0, 500, u32::try_from(second_pcm.len() - 1).unwrap());
    let mut honmon = Vec::new();
    honmon.extend_from_slice(b"first");
    let second_entry_offset = honmon.len();
    honmon.extend_from_slice(b"second");
    honmon.extend_from_slice(&second_audio);
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
    )
    .unwrap();
    fs::write(
        dir.path().join("PCMDATA.DIC"),
        fixture_sseddata_literal_chunks(&[&second_pcm], 500, 500),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(
            &[&simple_index_page_for_test(&[
                (&[0x24, 0x22], 100, 0),
                (
                    &[0x24, 0x24],
                    100,
                    u16::try_from(second_entry_offset).unwrap(),
                ),
            ])],
            200,
            200,
        ),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Markerless stream".to_owned(),
        components: vec![
            SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0x00,
                start_block: 100,
                end_block: 100,
                data: [0; 4],
                filename: "HONMON.DIC".to_owned(),
                role: SsedComponentRole::Honmon,
            },
            SsedComponent {
                index: 1,
                multi: 0,
                component_type: 0x71,
                start_block: 200,
                end_block: 200,
                data: [0; 4],
                filename: "FHINDEX.DIC".to_owned(),
                role: SsedComponentRole::Index,
            },
            SsedComponent {
                index: 2,
                multi: 0,
                component_type: 0xd8,
                start_block: 500,
                end_block: 500,
                data: [0; 4],
                filename: "PCMDATA.DIC".to_owned(),
                role: SsedComponentRole::PcmData,
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
            confidence: 80,
            title: Some("Markerless stream".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 0,
    })
    .unwrap();

    let input = package.renderer_input_for_target(&token).unwrap();
    let RendererInput::HcSsedStream {
        length,
        resources,
        diagnostics,
        ..
    } = input
    else {
        panic!("SSED address should produce HC renderer input");
    };
    assert_eq!(length, None);
    assert!(resources.is_empty());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "ssed_renderer_resource_scan_deferred" })
    );
}

#[test]
fn ssed_index_address_uses_own_index_component_for_body_bound() {
    let dir = tempdir().unwrap();
    let honmon = b"first-second-third".to_vec();
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(
            &[&simple_index_page_for_test(&[
                (&[0x24, 0x22], 100, 0),
                (&[0x24, 0x24], 100, 5),
            ])],
            200,
            200,
        ),
    )
    .unwrap();
    fs::write(
        dir.path().join("FKINDEX.DIC"),
        fixture_sseddata_literal_chunks(
            &[&simple_index_page_for_test(&[
                (&[0x25, 0x22], 100, 0),
                (&[0x25, 0x24], 100, 12),
            ])],
            300,
            300,
        ),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Index-specific body bounds".to_owned(),
        components: vec![
            SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0x00,
                start_block: 100,
                end_block: 100,
                data: [0; 4],
                filename: "HONMON.DIC".to_owned(),
                role: SsedComponentRole::Honmon,
            },
            SsedComponent {
                index: 1,
                multi: 0,
                component_type: 0x71,
                start_block: 200,
                end_block: 200,
                data: [0; 4],
                filename: "FHINDEX.DIC".to_owned(),
                role: SsedComponentRole::Index,
            },
            SsedComponent {
                index: 2,
                multi: 0,
                component_type: 0x71,
                start_block: 300,
                end_block: 300,
                data: [0; 4],
                filename: "FKINDEX.DIC".to_owned(),
                role: SsedComponentRole::Index,
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
            confidence: 80,
            title: Some("Index-specific body bounds".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let token = TargetToken::new(&InternalTarget::SsedIndexAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 0,
        index_component: "FKINDEX.DIC".to_owned(),
    })
    .unwrap();

    let input = package.renderer_input_for_target(&token).unwrap();
    let RendererInput::HcSsedStream { length, .. } = input else {
        panic!("SSED index address should produce HC renderer input");
    };

    assert_eq!(length, Some(12));
}

#[test]
fn ssed_hc_renderer_input_preserves_prefixed_entry_marker_start() {
    let dir = tempdir().unwrap();
    let mut honmon = Vec::new();
    honmon.extend_from_slice(&[0x1f, 0x02]);
    honmon.extend_from_slice(&SSED_ENTRY_MARKER);
    honmon.extend_from_slice(b"first");
    let second_entry_offset = honmon.len();
    honmon.extend_from_slice(&SSED_ENTRY_MARKER);
    honmon.extend_from_slice(b"second");
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Prefixed marker".to_owned(),
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
            confidence: 80,
            title: Some("Prefixed marker".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 100,
        offset: 2,
    })
    .unwrap();

    let input = package.renderer_input_for_target(&token).unwrap();
    let RendererInput::HcSsedStream { offset, length, .. } = input else {
        panic!("SSED address should produce HC renderer input");
    };
    assert_eq!(offset, 0);
    assert_eq!(length, Some(second_entry_offset as u64));
}

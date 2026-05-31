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
    assert_eq!(view.kind, ResolvedTargetKind::Deferred);
    assert_eq!(view.resources.len(), resources.len());
    assert!(view.capabilities.contains(&RenderCapability::HcRenderInput));
    assert!(view.capabilities.contains(&RenderCapability::Images));
    assert!(view.capabilities.contains(&RenderCapability::Audio));
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
fn ssed_hc_renderer_input_uses_index_boundary_for_marker_variants() {
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
        title: "Index boundaries".to_owned(),
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
            title: Some("Index boundaries".to_owned()),
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

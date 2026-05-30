use std::fs;

use super::*;

#[test]
fn ssed_pcmdata_address_uses_loose_pcmu_audio_when_component_is_absent() {
    let dir = tempdir().unwrap();
    let package_root = dir.path().join("_DCT_SAMPLE");
    let pcmu_root = dir.path().join("_DCT_SAMPLE_PCM_U");
    fs::create_dir(&package_root).unwrap();
    fs::create_dir(&pcmu_root).unwrap();
    fs::write(pcmu_root.join("WaveFile.map"), b"00000001 269094\n").unwrap();
    fs::write(
        pcmu_root.join("00000001"),
        encrypt_logofont_cipher_for_test(b"ID3\x03\x00\x00sample mp3 bytes"),
    )
    .unwrap();

    let package = ReaderBookPackage::new(
        &package_root,
        DetectedPackage {
            root: package_root.clone(),
            format_family: FormatFamily::Ssed,
            confidence: 80,
            title: Some("Sample".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores::default(),
    );
    let token = ResourceToken::new(&InternalResource::SsedComponentAddress {
        component: "PCMDATA.DIC".to_owned(),
        block: 269094,
        offset: 0,
        resource_kind: ResourceKind::PcmData,
    })
    .unwrap();

    let resource = package.resolve_resource(&token).unwrap();
    assert_eq!(resource.kind, ResourceKind::PcmData);
    assert_eq!(resource.label.as_deref(), Some("_PCM_U/00000001"));
    assert_eq!(resource.mime_type.as_deref(), Some("audio/mpeg"));
    assert!(resource.href.is_some());
    assert!(resource.diagnostics.is_empty());
    assert_eq!(
        package.read_resource(&token).unwrap(),
        b"ID3\x03\x00\x00sample mp3 bytes"
    );
}

#[test]
fn ssed_pcmdata_range_reads_portable_wave_audio() {
    let dir = tempdir().unwrap();
    let pcm_chunks = pcmdata_wave_chunks_for_test(1, b"\x80\x81\x82");
    fs::write(
        dir.path().join("PCMDATA.DIC"),
        fixture_sseddata_literal_chunks(&[&pcm_chunks], 500, 500),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Pcm".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0xd8,
            start_block: 500,
            end_block: 500,
            data: [0; 4],
            filename: "PCMDATA.DIC".to_owned(),
            role: SsedComponentRole::PcmData,
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
            title: Some("Pcm".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let token = ResourceToken::new(&InternalResource::SsedPcmDataRange {
        component: "PCMDATA.DIC".to_owned(),
        start_block: 500,
        start_offset: 0,
        end_block: 500,
        end_offset: u32::try_from(pcm_chunks.len() - 1).unwrap(),
    })
    .unwrap();

    let resource = package.resolve_resource(&token).unwrap();
    assert_eq!(resource.kind, ResourceKind::PcmData);
    assert_eq!(resource.mime_type.as_deref(), Some("audio/wav"));
    assert!(resource.href.is_some());
    let audio = package.read_resource(&token).unwrap();
    assert!(audio.starts_with(b"RIFF"));
    assert!(audio.ends_with(b"\x80\x81\x82"));
}

#[test]
fn monoscr_component_address_reads_png_bitmap_cell() {
    let dir = tempdir().unwrap();
    let mut bitmap = vec![0_u8; MONOSCR_BITMAP_BYTES];
    bitmap[0] = 0x80;
    fs::write(
        dir.path().join("MONOSCR.DIC"),
        fixture_sseddata_literal_chunks(&[&bitmap], 400, 400),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Mono".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0xd0,
            start_block: 400,
            end_block: 400,
            data: [0; 4],
            filename: "MONOSCR.DIC".to_owned(),
            role: SsedComponentRole::MonoScr,
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
            title: Some("Mono".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let token = ResourceToken::new(&InternalResource::SsedComponentAddress {
        component: "MONOSCR.DIC".to_owned(),
        block: 400,
        offset: 0,
        resource_kind: ResourceKind::Image,
    })
    .unwrap();

    let resource = package.resolve_resource(&token).unwrap();
    assert_eq!(resource.kind, ResourceKind::Image);
    assert_eq!(resource.mime_type.as_deref(), Some("image/png"));
    assert!(resource.href.is_some());
    let png = package.read_resource(&token).unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[test]
fn figure_resource_reads_variable_bitmap_png() {
    let dir = tempdir().unwrap();
    let mut payload = vec![0_u8; 17];
    payload.extend_from_slice(&[0x80, 0x80, 0x7f, 0x00]);
    fs::write(
        dir.path().join("FIGURE.DIC"),
        fixture_sseddata_literal_chunks(&[&payload], 1200, 1200),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Figure".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0xd0,
            start_block: 1200,
            end_block: 1200,
            data: [0; 4],
            filename: "FIGURE.DIC".to_owned(),
            role: SsedComponentRole::Figure,
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
            title: Some("Figure".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );
    let token = ResourceToken::new(&InternalResource::SsedFigure {
        component: "FIGURE.DIC".to_owned(),
        block: 1200,
        offset: 17,
        width: 9,
        height: 2,
    })
    .unwrap();

    let resource = package.resolve_resource(&token).unwrap();
    assert_eq!(resource.kind, ResourceKind::Image);
    assert_eq!(resource.mime_type.as_deref(), Some("image/png"));
    assert_eq!(
        resource.label.as_deref(),
        Some("FIGURE.DIC:00001200:0017:9x2")
    );
    let png = package.read_resource(&token).unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
}

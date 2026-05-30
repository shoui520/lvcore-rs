use std::fs;

use aes::Aes128;
use aes::cipher::{BlockEncrypt, KeyInit};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

use crate::lved_sqlite::apply_sqlcipher_key;
use crate::render::{HcRendererProfileSource, HcRendererProfileStatus, RenderCapability};
use crate::ssed::SSEDINFO_MAGIC;
use crate::target::TargetKind;

use super::super::PackageDriver;
use super::super::capabilities::ssed_search_modes;
use super::super::ssed_detection::ssed_capabilities;
use super::*;

mod dense_sidecar;
mod detection;
mod fulltext;
mod lved;
mod ssed_loose_resources;

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
                 10000000\t0000FFFF\t\t西和ABC順\n\
                 01000000\t0000FFFF\t\t五十音\n",
        ),
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
            }),
            1,
            0,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(window.before.len(), 1);
    assert_eq!(window.before[0].title.as_deref(), Some("季語"));
}

#[test]
fn ssed_numeric_auxiliary_index_opens_without_exinfo() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("0000015f.idx"),
        cp932(
            "00000000\t00000000\tRoot\n\
                 00005221\t00000722\t\tChild\n",
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
}

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

#[test]
fn ssed_hc_renderer_input_carries_stream_resource_refs() {
    let dir = tempdir().unwrap();
    let pcm_chunks = pcmdata_wave_chunks_for_test(1, b"\x80\x81\x82");
    let mut figure_payload = vec![0_u8; 17];
    figure_payload.extend_from_slice(&[0x80, 0x80, 0x7f, 0x00]);
    let mut honmon = Vec::new();
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

enum DenseSidecarFixture {
    BodyRows,
    AndroidRowidTimesFiveBodyRows,
    TitleOnlyThenBodyRows,
    ShardedTContentsBodyRows,
    BlobBodyRows,
    MissingBetaRow,
}

fn write_ssed_dense_sidecar_fixture(root: &Path, fixture: DenseSidecarFixture) -> SsedCatalog {
    let mut body = Vec::new();
    let (alpha_anchor, beta_anchor) = match fixture {
        DenseSidecarFixture::AndroidRowidTimesFiveBodyRows => ("00000005", "00000010"),
        _ => ("00000001", "00000002"),
    };
    body.extend_from_slice(&dense_anchor_record(alpha_anchor));
    body.extend_from_slice(&dense_anchor_record(beta_anchor));
    fs::write(
        root.join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&body], 100, 100),
    )
    .unwrap();

    let mut titles = Vec::new();
    let alpha_title_offset = 0u16;
    titles.extend_from_slice(b"alpha\x1f\x0a");
    let beta_title_offset = u16::try_from(titles.len()).unwrap();
    titles.extend_from_slice(b"beta\x1f\x0a");
    fs::write(
        root.join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&[&titles], 300, 300),
    )
    .unwrap();

    let mut index_page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    index_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    index_page[2..4].copy_from_slice(&2u16.to_be_bytes());
    let mut pos = 4usize;
    write_simple_index_row(
        &mut index_page,
        &mut pos,
        &body_jis("あ"),
        100,
        0,
        300,
        alpha_title_offset,
    );
    write_simple_index_row(
        &mut index_page,
        &mut pos,
        &body_jis("い"),
        100,
        32,
        300,
        beta_title_offset,
    );
    fs::write(
        root.join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&[&index_page], 200, 200),
    )
    .unwrap();

    match fixture {
        DenseSidecarFixture::BodyRows => {
            write_dense_body_db(root.join("body.db"), true, true, false);
        }
        DenseSidecarFixture::AndroidRowidTimesFiveBodyRows => {
            write_android_body_db(root.join("DENSE.db"), "DENSE");
        }
        DenseSidecarFixture::TitleOnlyThenBodyRows => {
            let connection = Connection::open(root.join("a-title-only.db")).unwrap();
            connection
                .execute_batch(
                    "
                        create table t_contents (f_DataId integer primary key, f_Title text);
                        insert into t_contents values (1, 'alpha title only');
                        ",
                )
                .unwrap();
            write_dense_body_db(root.join("body.db"), true, true, false);
        }
        DenseSidecarFixture::ShardedTContentsBodyRows => {
            write_sharded_t_contents_body_db(root.join("body.db"));
        }
        DenseSidecarFixture::BlobBodyRows => {
            write_dense_body_db(root.join("body.db"), true, true, true);
        }
        DenseSidecarFixture::MissingBetaRow => {
            write_dense_body_db(root.join("body.db"), true, false, false);
        }
    }

    SsedCatalog {
        title: "Dense".to_owned(),
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
                component_type: 0x03,
                start_block: 300,
                end_block: 300,
                data: [0; 4],
                filename: "FHTITLE.DIC".to_owned(),
                role: SsedComponentRole::Title,
            },
            SsedComponent {
                index: 2,
                multi: 0,
                component_type: 0x91,
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
            component_count: 3,
            trailing_bytes: 0,
        },
    }
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

fn dense_anchor_record(anchor: &str) -> Vec<u8> {
    let mut record = Vec::new();
    record.extend_from_slice(&[0x1f, 0x09, 0x00, 0x01, 0x1f, 0x41]);
    record.extend_from_slice(&body_jis(anchor));
    record.extend_from_slice(&[0x1f, 0x61, 0x1f, 0x0a]);
    record.resize(32, 0);
    record
}

fn write_simple_index_row(
    page: &mut [u8],
    pos: &mut usize,
    key: &[u8],
    body_block: u32,
    body_offset: u16,
    title_block: u32,
    title_offset: u16,
) {
    page[*pos] = u8::try_from(key.len()).unwrap();
    *pos += 1;
    page[*pos..*pos + key.len()].copy_from_slice(key);
    *pos += key.len();
    page[*pos..*pos + 4].copy_from_slice(&body_block.to_be_bytes());
    page[*pos + 4..*pos + 6].copy_from_slice(&body_offset.to_be_bytes());
    page[*pos + 6..*pos + 10].copy_from_slice(&title_block.to_be_bytes());
    page[*pos + 10..*pos + 12].copy_from_slice(&title_offset.to_be_bytes());
    *pos += 12;
}

fn write_dense_body_db(path: PathBuf, alpha: bool, beta: bool, blob: bool) {
    let connection = Connection::open(path).unwrap();
    connection
            .execute_batch(
                "create table t_contents (f_DataId integer primary key, f_Title blob, f_Html blob, f_Plane blob);",
            )
            .unwrap();
    if alpha {
        connection
            .execute(
                "insert into t_contents values (?, ?, ?, ?)",
                (
                    1,
                    "alpha".as_bytes(),
                    "<div>alpha sidecar html</div>".as_bytes(),
                    "alpha sidecar body".as_bytes(),
                ),
            )
            .unwrap();
    }
    if beta {
        if blob {
            connection
                .execute(
                    "insert into t_contents values (?, ?, ?, ?)",
                    (
                        2,
                        cp932("ベータ"),
                        cp932("<div>ベータ html</div>"),
                        cp932("ベータ body"),
                    ),
                )
                .unwrap();
        } else {
            connection
                .execute(
                    "insert into t_contents values (?, ?, ?, ?)",
                    (
                        2,
                        "beta".as_bytes(),
                        "<div>beta sidecar html</div>".as_bytes(),
                        "beta sidecar body".as_bytes(),
                    ),
                )
                .unwrap();
        }
    }
}

fn write_sharded_t_contents_body_db(path: PathBuf) {
    let connection = Connection::open(path).unwrap();
    connection
            .execute_batch(
                "
                create table t_contents_1 (f_DataId text primary key, f_Title text, f_Html text);
                create table t_contents_2 (f_DataId text primary key, f_Title text, f_Html text);
                insert into t_contents_2 values ('00000002', 'beta', '<div>beta sharded html</div>');
                ",
            )
            .unwrap();
}

fn write_android_body_db(path: PathBuf, table: &str) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(&format!(
            "create table {} (Html text);",
            quote_fixture_sql_identifier(table)
        ))
        .unwrap();
    connection
        .execute(
            &format!(
                "insert into {} (Html) values (?), (?)",
                quote_fixture_sql_identifier(table)
            ),
            (
                "<div>android alpha html</div>",
                "<div>android beta html</div>",
            ),
        )
        .unwrap();
}

fn quote_fixture_sql_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn cp932(value: &str) -> Vec<u8> {
    let (encoded, _encoding, _had_errors) = SHIFT_JIS.encode(value);
    encoded.into_owned()
}

fn body_jis(value: &str) -> Vec<u8> {
    value
        .chars()
        .flat_map(|ch| {
            let body_ch = if (0x20..=0x7e).contains(&(ch as u32)) {
                if ch == ' ' {
                    '\u{3000}'
                } else {
                    char::from_u32(ch as u32 + 0xfee0).unwrap_or(ch)
                }
            } else {
                ch
            };
            cp932(&body_ch.to_string())
                .chunks(2)
                .next()
                .and_then(sjis_pair_to_jis_pair)
                .unwrap_or_default()
        })
        .collect()
}

fn sjis_pair_to_jis_pair(sjis: &[u8]) -> Option<Vec<u8>> {
    if sjis.len() != 2 {
        return None;
    }
    let lead = sjis[0];
    let trail = sjis[1];
    let row_base = if (0x81..=0x9f).contains(&lead) {
        (lead - 0x81) * 2
    } else if (0xe0..=0xef).contains(&lead) {
        (lead - 0xc1) * 2
    } else {
        return None;
    };
    let (row, cell) = if (0x9f..=0xfc).contains(&trail) {
        (row_base + 1, trail - 0x9f)
    } else if (0x40..=0xfc).contains(&trail) && trail != 0x7f {
        let adjusted = if trail >= 0x80 { trail - 1 } else { trail };
        (row_base, adjusted - 0x40)
    } else {
        return None;
    };
    let first = row + 0x21;
    let second = cell + 0x21;
    ((0x21..=0x7e).contains(&first) && (0x21..=0x7e).contains(&second)).then(|| vec![first, second])
}

fn screen_menu_image_control(width: u32, height: u32, block: u32, offset: u32) -> Vec<u8> {
    let mut payload = vec![0u8; 20];
    payload[0] = 0x1f;
    payload[1] = 0x4d;
    payload[10..12].copy_from_slice(&bcd_word(width));
    payload[12..14].copy_from_slice(&bcd_word(height));
    payload[14..18].copy_from_slice(&bcd_u32(block));
    payload[18..20].copy_from_slice(&bcd_word(offset));
    payload
}

fn screen_menu_hotspot_control(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    block: u32,
    offset: u32,
) -> Vec<u8> {
    let mut payload = vec![0u8; 36];
    payload[0] = 0x1f;
    payload[1] = 0x4f;
    payload[8..10].copy_from_slice(&bcd_word(x));
    payload[10..12].copy_from_slice(&bcd_word(y));
    payload[12..14].copy_from_slice(&bcd_word(width));
    payload[14..16].copy_from_slice(&bcd_word(height));
    payload[28..32].copy_from_slice(&bcd_u32(block));
    payload[32..34].copy_from_slice(&bcd_word(offset));
    payload
}

fn bcd_word(value: u32) -> [u8; 2] {
    let s = format!("{value:04}");
    [
        ((s.as_bytes()[0] - b'0') << 4) | (s.as_bytes()[1] - b'0'),
        ((s.as_bytes()[2] - b'0') << 4) | (s.as_bytes()[3] - b'0'),
    ]
}

fn bcd_u32(value: u32) -> [u8; 4] {
    let s = format!("{value:08}");
    [
        ((s.as_bytes()[0] - b'0') << 4) | (s.as_bytes()[1] - b'0'),
        ((s.as_bytes()[2] - b'0') << 4) | (s.as_bytes()[3] - b'0'),
        ((s.as_bytes()[4] - b'0') << 4) | (s.as_bytes()[5] - b'0'),
        ((s.as_bytes()[6] - b'0') << 4) | (s.as_bytes()[7] - b'0'),
    ]
}

fn encrypt_logofont_cipher_for_test(data: &[u8]) -> Vec<u8> {
    let digest = Sha256::digest(b"LogoFontCipher");
    let key = &digest[..16];
    let mut previous = [0_u8; 16];
    previous.copy_from_slice(&digest[16..32]);
    let cipher = Aes128::new_from_slice(key).unwrap();
    let mut padded = data.to_vec();
    let padding = 16 - (padded.len() % 16);
    padded.extend(std::iter::repeat_n(padding as u8, padding));
    let mut encrypted = Vec::with_capacity(padded.len());
    for chunk in padded.chunks_exact(16) {
        let mut block = [0_u8; 16];
        for index in 0..16 {
            block[index] = chunk[index] ^ previous[index];
        }
        let mut block = aes::Block::from(block);
        cipher.encrypt_block(&mut block);
        previous.copy_from_slice(&block);
        encrypted.extend_from_slice(&block);
    }
    encrypted
}

fn pcmdata_wave_chunks_for_test(format_tag: u16, data: &[u8]) -> Vec<u8> {
    let mut fmt_payload = Vec::new();
    fmt_payload.extend_from_slice(&format_tag.to_le_bytes());
    fmt_payload.extend_from_slice(&1_u16.to_le_bytes());
    fmt_payload.extend_from_slice(&8000_u32.to_le_bytes());
    fmt_payload.extend_from_slice(&8000_u32.to_le_bytes());
    fmt_payload.extend_from_slice(&1_u16.to_le_bytes());
    fmt_payload.extend_from_slice(&8_u16.to_le_bytes());

    let mut chunks = Vec::new();
    chunks.extend_from_slice(b"fmt ");
    chunks.extend_from_slice(&(fmt_payload.len() as u32).to_le_bytes());
    chunks.extend_from_slice(&fmt_payload);
    chunks.extend_from_slice(b"data");
    chunks.extend_from_slice(&(data.len() as u32).to_le_bytes());
    chunks.extend_from_slice(data);
    chunks
}

fn pcmdata_range_control_for_test(
    start_block: u32,
    start_offset: u32,
    end_block: u32,
    end_offset: u32,
) -> Vec<u8> {
    let mut control = vec![0x1f, 0x4a, 0x00, 0x01, 0x00, 0x00];
    control.extend_from_slice(&bcd_decimal_for_test(start_block, 4));
    control.extend_from_slice(&bcd_decimal_for_test(start_offset, 2));
    control.extend_from_slice(&bcd_decimal_for_test(end_block, 4));
    control.extend_from_slice(&bcd_decimal_for_test(end_offset, 2));
    control
}

fn simple_index_page_for_test(rows: &[(&[u8], u32, u16)]) -> Vec<u8> {
    let mut page = vec![0_u8; crate::ssed::BLOCK_SIZE as usize];
    page[0..2].copy_from_slice(&0xc000_u16.to_be_bytes());
    page[2..4].copy_from_slice(&(rows.len() as u16).to_be_bytes());
    let mut pos = 4usize;
    for (key, block, offset) in rows {
        page[pos] = key.len() as u8;
        pos += 1;
        page[pos..pos + key.len()].copy_from_slice(key);
        pos += key.len();
        page[pos..pos + 4].copy_from_slice(&block.to_be_bytes());
        pos += 4;
        page[pos..pos + 2].copy_from_slice(&offset.to_be_bytes());
        pos += 2;
        page[pos..pos + 4].copy_from_slice(&0_u32.to_be_bytes());
        pos += 4;
        page[pos..pos + 2].copy_from_slice(&0_u16.to_be_bytes());
        pos += 2;
    }
    page
}

fn bcd_decimal_for_test(mut value: u32, bytes: usize) -> Vec<u8> {
    let mut out = vec![0_u8; bytes];
    for byte in out.iter_mut().rev() {
        let low = value % 10;
        value /= 10;
        let high = value % 10;
        value /= 10;
        *byte = ((high as u8) << 4) | low as u8;
    }
    out
}

fn fixture_sseddata_literal_chunks(chunks: &[&[u8]], start_block: u32, end_block: u32) -> Vec<u8> {
    let chunk_count = chunks.len();
    let first_chunk_offset = 0x40 + chunk_count * 4;
    let mut data = vec![0u8; first_chunk_offset];
    data[..8].copy_from_slice(SSEDDATA_MAGIC);
    data[0x0f] = 1;
    data[0x16..0x18].copy_from_slice(&(chunk_count as u16).to_be_bytes());
    data[0x18..0x1c].copy_from_slice(&start_block.to_be_bytes());
    data[0x1c..0x20].copy_from_slice(&end_block.to_be_bytes());

    let mut compressed_chunks = Vec::with_capacity(chunk_count);
    let mut next_offset = first_chunk_offset;
    for (index, chunk) in chunks.iter().enumerate() {
        data[0x40 + index * 4..0x44 + index * 4]
            .copy_from_slice(&(next_offset as u32).to_be_bytes());
        let compressed = fixture_sseddata_literal_chunk(chunk);
        next_offset += compressed.len();
        compressed_chunks.push(compressed);
    }
    for compressed in compressed_chunks {
        data.extend_from_slice(&compressed);
    }
    data
}

fn fixture_sseddata_literal_chunk(literals: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&[0, 0]);
    chunk.extend_from_slice(&(literals.len() as u16).to_be_bytes());
    chunk.push(0);
    for literal in literals {
        chunk.extend_from_slice(&[0, 0, *literal]);
    }
    chunk
}

fn write_lved_search_fixture(root: &Path) {
    let payload = root.join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        apply_sqlcipher_key(&connection, key).unwrap();
        connection
                .execute_batch(
                    "
                    create table info (id integer, type integer, name text primary key, body text, media text);
                    insert into info values (1, 1, 'about.html', '<h1>Example Dictionary 第2版</h1>', '');
                    insert into info values (2, 1, 'help.html', '<h1>Help</h1>', '');
                    create table content (id integer primary key, type integer, body text, media text);
                    create table media (id integer primary key, name text, type integer, main blob);
                    create table mediasub (id integer primary key, name text, type integer, main blob);
                    create table list (
                      id integer primary key,
                      refid integer,
                      type integer,
                      anchor text,
                      title text,
                      titlesub text
                    );
                    create virtual table search using fts4(
                      forward,
                      back,
                      part,
                      fts,
                      advanced1,
                      advanced2,
                      filter
                    );
                    insert into content values (100, 1, '<article><h1>Alpha</h1><p>body</p><object class=\"icon\" data=\"AC6E.svg\"></object><a href=\"lved.media.sound:00010033.mp3\">sound</a><a href=\"lved.dataid:101#jump\">next</a><a href=\"lved.info:help.html#top\">help</a></article>', '');
                    insert into content values (101, 1, '<article><h1>Beta</h1></article>', '');
                    insert into content values (102, 1, '<article><h1>Gamma</h1></article>', '');
                    insert into media values (1, 'AC6E', 4, X'3C7376672F3E');
                    insert into mediasub values (1, '00010033', 5, X'49443303');
                    insert into list values (1, 100, 1, 'body-anchor', '<img class=\"icon\" src=\"AC6E.svg\"><b>alpha</b>', '<span>subtitle</span>');
                    insert into list values (2, 101, 1, '', '<b>beta</b>', '');
                    insert into list values (3, 102, 1, '', '<b>gamma</b>', '');
                    insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                      values (1, 'alpha', 'ahpla', 'alpha', 'alpha body', '', '', '∥alpha∥');
                    ",
                )
                .unwrap();
    }
    fs::write(root.join("main.key"), key).unwrap();
}

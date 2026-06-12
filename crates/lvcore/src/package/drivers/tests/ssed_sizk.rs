use std::fs;

use super::*;

#[test]
fn ssed_sizk_read_aloud_surface_renders_playback_with_audio_resource() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("HTMLs")).unwrap();
    fs::create_dir(dir.path().join("Templates")).unwrap();
    fs::write(
        dir.path().join("EXINFO.INI"),
        cp932("[GENERAL]\nMP3NAME=shizuku.mp3\n"),
    )
    .unwrap();
    fs::write(
        dir.path().join("HTMLs").join("b121.html"),
        cp932(
            "<html><head><link rel=\"stylesheet\" type=\"text/css\" href=\"&cssPath;\"></head>\
             <body><h1><!--&IND0004;--></h1><p><!--&IND0008;--></p><img src=\"haikei.png\"></body></html>",
        ),
    )
    .unwrap();
    fs::write(dir.path().join("Templates").join("haikei.png"), b"png").unwrap();
    fs::write(dir.path().join("Templates").join("00000190.css"), b"h1{}").unwrap();
    fs::write(dir.path().join("shizuku.mp3"), b"ID3").unwrap();
    fs::write(
        dir.path().join("shizuku_honbun.txt"),
        utf16le_bom("line one\nline two\n"),
    )
    .unwrap();
    fs::write(
        dir.path().join("shizuku_time.txt"),
        utf16le_bom("1000\n00:02.500\n"),
    )
    .unwrap();

    let mut honmon = Vec::new();
    honmon.extend_from_slice(&sizk_entry(
        "b121",
        &[("0004", "Work"), ("0008", "Summary")],
    ));
    honmon.extend_from_slice(&sizk_entry("b122", &[("0011", "Author")]));
    honmon.extend_from_slice(&sizk_entry("b123", &[("0021", "Narrator")]));
    honmon.extend_from_slice(&sizk_entry(
        "b124",
        &[("0004", "Work"), ("0005", "Reading")],
    ));
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&honmon], 100, 100),
    )
    .unwrap();

    let catalog = SsedCatalog {
        title: "SIZK".to_owned(),
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
            title: Some("SIZK".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );

    assert!(package.home_surfaces().unwrap().iter().any(|surface| {
        surface.surface_id == super::super::ssed_sizk_surfaces::SSED_SIZK_SURFACE_ID
            && surface.kind == NavigationSurfaceKind::Info
            && surface.status == NavigationStatus::Available
    }));

    let surface = package
        .open_surface(super::super::ssed_sizk_surfaces::SSED_SIZK_SURFACE_ID)
        .unwrap();
    let NavigationSurface::InfoPages { pages, .. } = surface else {
        panic!("expected SIZK info pages");
    };
    assert_eq!(pages.len(), 4);
    assert_eq!(pages[0].label_text, "Overview: Ｗｏｒｋ");
    assert_eq!(pages[3].label_text, "Playback: Ｗｏｒｋ");

    let overview = package
        .render_target(&pages[0].target, &RenderOptions::default())
        .unwrap();
    assert!(overview.display_html.as_deref().is_some_and(
        |html| html.contains("<h1>Ｗｏｒｋ</h1>") && html.contains("lvcore://resource/")
    ));
    assert!(overview.display_html.as_deref().is_some_and(|html| {
        html.contains("<style type=\"text/css\">") && !html.contains("&cssPath;")
    }));
    assert_eq!(overview.resources[0].kind, ResourceKind::Image);
    assert_eq!(
        package.read_resource(&overview.resources[0].token).unwrap(),
        b"png"
    );

    let playback = package
        .render_target(&pages[3].target, &RenderOptions::default())
        .unwrap();
    let html = playback.display_html.as_deref().unwrap();
    assert!(html.contains("<audio controls"));
    assert!(html.contains("data-time-ms=\"1000\""));
    assert!(html.contains("data-time-ms=\"2500\""));
    assert!(html.contains("line two"));
    let audio = playback
        .resources
        .iter()
        .find(|resource| resource.kind == ResourceKind::Audio)
        .expect("playback should expose shizuku.mp3");
    assert_eq!(audio.mime_type.as_deref(), Some("audio/mpeg"));
    assert_eq!(package.read_resource(&audio.token).unwrap(), b"ID3");
}

fn sizk_entry(template_code: &str, sections: &[(&str, &str)]) -> Vec<u8> {
    let mut entry = Vec::new();
    entry.extend_from_slice(&SSED_ENTRY_MARKER);
    let code = u16::from_str_radix(template_code, 16).unwrap();
    entry.extend_from_slice(&code.to_be_bytes());
    for (section, text) in sections {
        let code = u16::from_str_radix(section, 16).unwrap();
        entry.extend_from_slice(&[0x1f, 0x09]);
        entry.extend_from_slice(&code.to_be_bytes());
        entry.extend_from_slice(&body_jis(text));
        entry.extend_from_slice(&[0x1f, 0x0a]);
    }
    entry
}

fn utf16le_bom(value: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xfe];
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

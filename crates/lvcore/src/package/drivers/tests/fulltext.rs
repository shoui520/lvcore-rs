use std::fs;
use std::path::Path;

use super::*;

#[test]
fn ssed_fulltext_searches_honmon_body_windows() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_fulltext_fixture(dir.path());
    let search_modes = ssed_search_modes(&catalog, dir.path());
    assert!(search_modes.contains(&SearchMode::FullText));
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Synthetic fulltext".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            search_modes,
            ..Default::default()
        },
    );
    assert!(
        package
            .metadata()
            .capabilities
            .contains(&Capability::FullTextSearch)
    );
    assert!(
        package
            .metadata()
            .search_modes
            .contains(&SearchMode::FullText)
    );

    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::FullText,
            query: "window needle".to_owned(),
            cursor: None,
            limit: 10,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "本文見出し");
    assert!(
        page.hits[0]
            .snippet_html
            .as_deref()
            .is_some_and(|snippet| snippet.contains("window needle"))
    );
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 100,
            offset: 0
        } if component == "HONMON.DIC"
    ));
    assert!(
        page.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_body_window_scan")
    );
}

#[test]
fn ssed_fulltext_matches_fullwidth_ascii_body_text() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_fulltext_fixture(dir.path());
    let search_modes = ssed_search_modes(&catalog, dir.path());
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Synthetic fulltext".to_owned()),
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
            query: "fullwidth".to_owned(),
            cursor: None,
            limit: 10,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
}

#[test]
fn ssed_fulltext_metadata_requires_supported_honmon_payload() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_fulltext_fixture(dir.path());
    fs::write(dir.path().join("HONMON.DIC"), b"not an SSED payload").unwrap();

    let capabilities = ssed_capabilities(&catalog, dir.path());
    let search_modes = ssed_search_modes(&catalog, dir.path());

    assert!(!capabilities.contains(&Capability::FullTextSearch));
    assert!(!search_modes.contains(&SearchMode::FullText));
    assert!(search_modes.contains(&SearchMode::Exact));
}

fn write_ssed_fulltext_fixture(root: &Path) -> SsedCatalog {
    let mut body = Vec::new();
    body.extend_from_slice(&[0x1f, 0x09, 0x00, 0x01, 0x1f, 0x41]);
    body.extend_from_slice(&body_jis(
        "この本文 has a window needle and ＦＵＬＬＷＩＤＴＨ text.",
    ));
    body.extend_from_slice(&[0x1f, 0x61, 0x1f, 0x0a]);
    fs::write(
        root.join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&body], 100, 100),
    )
    .unwrap();

    let title = cp932("本文見出し");
    fs::write(
        root.join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&[&title], 300, 300),
    )
    .unwrap();

    let mut index_page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    index_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    index_page[2..4].copy_from_slice(&1u16.to_be_bytes());
    index_page[4] = 2;
    index_page[5..7].copy_from_slice(&[0x24, 0x22]);
    index_page[7..11].copy_from_slice(&100u32.to_be_bytes());
    index_page[11..13].copy_from_slice(&0u16.to_be_bytes());
    index_page[13..17].copy_from_slice(&300u32.to_be_bytes());
    index_page[17..19].copy_from_slice(&0u16.to_be_bytes());
    fs::write(
        root.join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&[&index_page], 200, 200),
    )
    .unwrap();

    SsedCatalog {
        title: "Synthetic fulltext".to_owned(),
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
            component_count: 3,
            trailing_bytes: 0,
        },
    }
}

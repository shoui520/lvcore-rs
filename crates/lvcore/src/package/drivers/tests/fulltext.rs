use std::fs;
use std::path::Path;

use super::*;

#[test]
fn ssed_fulltext_prefetches_initial_honmon_body_rows() {
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
            gaiji_policy: None,
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
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_row_driven_body_prefetch")
    );
    assert!(
        !page
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_body_window_scan")
    );
}

#[test]
fn ssed_fulltext_row_cursor_keeps_lookahead_hit() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_fulltext_multi_body_fixture(dir.path());
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
    let query_page = |cursor: Option<String>| {
        package
            .search(&SearchQuery {
                scope: crate::search::SearchScope::CurrentBook {
                    book_id: package.metadata().book_id.clone(),
                },
                mode: SearchMode::FullText,
                query: "shared row needle".to_owned(),
                cursor,
                limit: 1,
                gaiji_policy: None,
            })
            .unwrap()
    };

    let first = query_page(None);
    assert_eq!(first.hits.len(), 1);
    assert_eq!(first.hits[0].title_text, "row one");
    assert_eq!(first.next_cursor.as_deref(), Some("row:1"));

    let second = query_page(first.next_cursor.clone());
    assert_eq!(second.hits.len(), 1);
    assert_eq!(second.hits[0].title_text, "row two");
    assert_eq!(second.next_cursor.as_deref(), Some("row:2"));

    let third = query_page(second.next_cursor.clone());
    assert_eq!(third.hits.len(), 1);
    assert_eq!(third.hits[0].title_text, "row three");
    assert_eq!(third.next_cursor, None);
}

#[test]
fn ssed_fulltext_searches_native_title_labels_before_body_rows() {
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
            query: "本文".to_owned(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "本文見出し");
    assert_eq!(page.next_cursor.as_deref(), Some("body:0"));
    assert!(
        page.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_title_index_prepass")
    );
    assert!(
        !page
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_row_driven_body_prefetch")
    );

    let continuation = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::FullText,
            query: "本文".to_owned(),
            cursor: page.next_cursor.clone(),
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(continuation.hits.len(), 1);
    assert!(
        continuation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_body_direct_scan")
    );
    assert!(
        continuation
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "ssed_fulltext_body_window_scan")
    );
}

#[test]
fn ssed_fulltext_searches_partial_native_title_labels_before_body_rows() {
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
            query: "見出".to_owned(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "本文見出し");
    assert_eq!(page.next_cursor.as_deref(), Some("body:0"));
    assert!(
        page.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_title_index_prepass")
    );
    assert!(
        !page
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_row_driven_body_prefetch")
    );
}

#[test]
fn ssed_fulltext_title_prepass_uses_sidecar_start_cursor_when_sidecar_hits_exist() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_fulltext_sidecar_start_fixture(dir.path());
    let search_modes = ssed_search_modes(&catalog, dir.path());
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Synthetic sidecar start".to_owned()),
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
            query: "01".to_owned(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "01");
    assert_eq!(page.next_cursor.as_deref(), Some("sidecar-body-start"));
    assert!(
        page.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_title_index_prepass")
    );

    let continuation = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::FullText,
            query: "01".to_owned(),
            cursor: page.next_cursor.clone(),
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(continuation.hits.len(), 1);
    assert_eq!(continuation.hits[0].title_text, "sidecar-only");
    assert!(
        continuation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_sidecar_scan")
    );
    assert!(
        continuation
            .next_cursor
            .as_deref()
            .is_some_and(|cursor| cursor.starts_with("sidecar-body-row:"))
    );
}

#[test]
fn ssed_fulltext_partial_title_prepass_returns_physical_continuation_cursor() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_fulltext_multi_page_title_fixture(dir.path());
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
            query: "tail".to_owned(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "本文見出し");
    assert!(
        page.next_cursor
            .as_deref()
            .is_some_and(|cursor| cursor.starts_with("title:ssed-partial-index-offset:2:")),
        "unexpected cursor: {:?}",
        page.next_cursor
    );
    assert!(
        !page
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_row_driven_body_prefetch")
    );

    let continuation = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::FullText,
            query: "tail".to_owned(),
            cursor: page.next_cursor.clone(),
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();
    assert!(
        continuation
            .hits
            .first()
            .is_none_or(|hit| hit.title_text != "本文見出し")
    );
}

#[test]
fn ssed_fulltext_searches_late_nonprefix_title_before_body_scan() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_fulltext_late_nonprefix_title_fixture(dir.path());
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
            query: "1計".to_owned(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "０－１計画法");
    let title_cursor = page.next_cursor.clone().unwrap();
    assert!(title_cursor.starts_with("title-nonprefix-unverified:"));
    assert!(
        page.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_partial_nonprefix_title_prepass")
    );
    assert!(
        !page
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_index_empty_physical_pages_skipped")
    );
    assert!(
        !page
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_body_direct_scan")
    );
    assert!(
        !page
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_row_driven_body_prefetch")
    );

    let continuation = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::FullText,
            query: "1計".to_owned(),
            cursor: Some(title_cursor.clone()),
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();

    assert!(
        continuation
            .hits
            .iter()
            .all(|hit| hit.title_text != "０－１計画法")
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
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
}

#[test]
fn ssed_fulltext_prefetches_non_ascii_body_query() {
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
            query: "検索語".to_owned(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "本文見出し");
    assert!(
        page.hits[0]
            .snippet_html
            .as_deref()
            .is_some_and(|snippet| snippet.contains("検索語"))
    );
    assert!(
        page.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_row_driven_body_prefetch")
    );
    assert!(
        !page
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_body_window_scan")
    );
}

#[test]
fn ssed_fulltext_prefetch_skips_out_of_catalog_index_body_pointers() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_fulltext_fixture(dir.path());
    let mut index_page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    index_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    index_page[2..4].copy_from_slice(&2u16.to_be_bytes());
    let mut pos = 4usize;
    for (key, body_block, body_offset) in [(b"\x24\x22", 0x20_0000u32, 0u16), (b"\x24\x23", 100, 0)]
    {
        index_page[pos] = key.len() as u8;
        pos += 1;
        index_page[pos..pos + key.len()].copy_from_slice(key);
        pos += key.len();
        index_page[pos..pos + 4].copy_from_slice(&body_block.to_be_bytes());
        index_page[pos + 4..pos + 6].copy_from_slice(&body_offset.to_be_bytes());
        index_page[pos + 6..pos + 10].copy_from_slice(&300u32.to_be_bytes());
        index_page[pos + 10..pos + 12].copy_from_slice(&0u16.to_be_bytes());
        pos += 12;
    }
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&[&index_page], 200, 200),
    )
    .unwrap();
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
            query: "検索語".to_owned(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert!(
        page.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "ssed_fulltext_body_component_missing"),
        "out-of-catalog index body pointers are sentinel/internal rows, not missing body components"
    );
}

#[test]
fn ssed_native_search_skips_out_of_catalog_index_body_pointers() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_fulltext_fixture(dir.path());
    let mut index_page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    index_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    index_page[2..4].copy_from_slice(&1u16.to_be_bytes());
    index_page[4] = 2;
    index_page[5..7].copy_from_slice(&[0x24, 0x22]);
    index_page[7..11].copy_from_slice(&0x20_0000u32.to_be_bytes());
    index_page[11..13].copy_from_slice(&0u16.to_be_bytes());
    index_page[13..17].copy_from_slice(&300u32.to_be_bytes());
    index_page[17..19].copy_from_slice(&0u16.to_be_bytes());
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&[&index_page], 200, 200),
    )
    .unwrap();
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
            mode: SearchMode::Exact,
            query: "あ".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert!(page.hits.is_empty());
    assert!(
        page.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "ssed_index_body_component_missing"),
        "out-of-catalog index body pointers are sentinel/internal rows, not missing body components"
    );
}

#[test]
fn ssed_fulltext_direct_body_scan_finds_late_body_hit() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_fulltext_late_body_fixture(dir.path());
    let search_modes = ssed_search_modes(&catalog, dir.path());
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Synthetic late fulltext".to_owned()),
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
            query: "曙光".to_owned(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert!(page.hits[0].title_text.contains("曙光"));
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedAddress { component, .. } if component == "HONMON.DIC"
    ));
    assert!(
        page.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_body_direct_scan")
    );
    assert!(
        page.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "ssed_fulltext_body_window_scan")
    );

    let view = package
        .render_target(&page.hits[0].target, &RenderOptions::default())
        .unwrap();
    assert!(
        view.basic_text
            .as_deref()
            .is_some_and(|text| text.contains("曙光"))
    );
}

#[test]
fn ssed_fulltext_body_cursor_uses_bounded_honmon_scan() {
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
            query: "検索語".to_owned(),
            cursor: Some("body:0".to_owned()),
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert!(
        page.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_body_direct_scan")
    );
    assert!(
        page.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "ssed_fulltext_body_window_scan")
    );
    assert!(
        !page
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_row_driven_body_prefetch")
    );
}

#[test]
fn ssed_fulltext_native_body_scan_uses_physical_continuation_cursor() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_fulltext_physical_cursor_fixture(dir.path());
    let search_modes = ssed_search_modes(&catalog, dir.path());
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Synthetic fulltext cursor".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            search_modes,
            ..Default::default()
        },
    );
    let query_page = |cursor: Option<String>| {
        package
            .search(&SearchQuery {
                scope: crate::search::SearchScope::CurrentBook {
                    book_id: package.metadata().book_id.clone(),
                },
                mode: SearchMode::FullText,
                query: "cursor needle".to_owned(),
                cursor,
                limit: 1,
                gaiji_policy: None,
            })
            .unwrap()
    };

    let first = query_page(None);
    assert_eq!(first.hits.len(), 1);
    assert_eq!(first.hits[0].title_text, "row0530");
    assert!(
        first
            .next_cursor
            .as_deref()
            .is_some_and(|cursor| cursor.starts_with("body-offset:")),
        "unexpected cursor: {:?}",
        first.next_cursor
    );
    assert!(
        first
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_body_window_scan")
    );

    let second = query_page(first.next_cursor.clone());
    assert_eq!(second.hits.len(), 1);
    assert_eq!(second.hits[0].title_text, "row0531");
    assert_eq!(second.next_cursor, None);
    assert!(
        second
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_body_cursor_scan")
    );
    assert!(
        !second
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_body_window_scan")
    );
}

#[test]
fn ssed_fulltext_searches_britannica_chronology_before_honmon_scan() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_fulltext_fixture(dir.path());
    let connection = rusqlite::Connection::open(dir.path().join("BriSynthetic.db")).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE D_InternationalChronology (
                INC_Code VARCHAR(20) NOT NULL UNIQUE,
                INC_Type_Code VARCHAR(100),
                INC_Type_Name VARCHAR(200),
                Year INTEGER,
                Month INTEGER,
                Day INTEGER,
                Sub_Disp_Order INTEGER,
                Jpn_Year VARCHAR(20),
                Value TEXT,
                PRIMARY KEY(INC_Code)
            );
            INSERT INTO D_InternationalChronology
                (INC_Code, INC_Type_Code, INC_Type_Name, Year, Month, Day, Sub_Disp_Order, Jpn_Year, Value)
            VALUES
                ('166', 'WOR', '世界史', 43, 0, 0, 10, '',
                 '＃＃Ｓ00000064:0000ブリタニアＥ＃＃，ローマの属州となる');
            "#,
        )
        .unwrap();
    let search_modes = ssed_search_modes(&catalog, dir.path());
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Synthetic Britannica".to_owned()),
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
            query: "ブリ".to_owned(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "43 世界史");
    assert!(
        page.hits[0]
            .snippet_html
            .as_deref()
            .is_some_and(|snippet| snippet.contains("ブリタニア"))
    );
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedAuxRecord { source, key, .. }
            if source == BRITANNICA_CHRONOLOGY_SOURCE_ID && key == "166"
    ));
    assert!(
        page.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_britannica_chronology_scan")
    );
    assert!(
        !page
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_fulltext_body_window_scan")
    );

    let view = package
        .render_target(&page.hits[0].target, &RenderOptions::default())
        .unwrap();
    let html = view.display_html.unwrap();
    assert!(html.contains("lvcore://target/"));
    assert!(!html.contains("＃＃Ｓ"));
}

#[test]
fn ssed_fulltext_chronology_cursor_counts_prior_sidecar_hits() {
    let dir = tempdir().unwrap();
    let catalog = write_ssed_dense_sidecar_fixture(dir.path(), DenseSidecarFixture::BodyRows);
    let connection = rusqlite::Connection::open(dir.path().join("BriSynthetic.db")).unwrap();
    connection
        .execute_batch(
            r#"
            CREATE TABLE D_InternationalChronology (
                INC_Code VARCHAR(20) NOT NULL UNIQUE,
                INC_Type_Code VARCHAR(100),
                INC_Type_Name VARCHAR(200),
                Year INTEGER,
                Month INTEGER,
                Day INTEGER,
                Sub_Disp_Order INTEGER,
                Jpn_Year VARCHAR(20),
                Value TEXT,
                PRIMARY KEY(INC_Code)
            );
            INSERT INTO D_InternationalChronology
                (INC_Code, INC_Type_Code, INC_Type_Name, Year, Month, Day, Sub_Disp_Order, Jpn_Year, Value)
            VALUES
                ('166', 'WOR', '世界史', 43, 0, 0, 10, '',
                 'beta sidecar body chronology first'),
                ('167', 'WOR', '世界史', 44, 0, 0, 20, '',
                 'beta sidecar body chronology second');
            "#,
        )
        .unwrap();
    let search_modes = ssed_search_modes(&catalog, dir.path());
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Synthetic Britannica".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            search_modes,
            ..Default::default()
        },
    );

    let first = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::FullText,
            query: "beta sidecar body".to_owned(),
            cursor: None,
            limit: 2,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(first.hits.len(), 2);
    assert!(matches!(
        first.hits[0].target.decode().unwrap(),
        InternalTarget::SsedDenseAnchor { anchor, .. } if anchor == "2"
    ));
    assert!(matches!(
        first.hits[1].target.decode().unwrap(),
        InternalTarget::SsedAuxRecord { key, .. } if key == "166"
    ));
    assert_eq!(first.next_cursor.as_deref(), Some("chronology:1"));

    let second = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::FullText,
            query: "beta sidecar body".to_owned(),
            cursor: first.next_cursor.clone(),
            limit: 2,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(second.hits.len(), 1);
    assert!(matches!(
        second.hits[0].target.decode().unwrap(),
        InternalTarget::SsedAuxRecord { key, .. } if key == "167"
    ));
    assert_eq!(second.next_cursor, None);
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
        "この本文 has a window needle and ＦＵＬＬＷＩＤＴＨ text. 検索語もあります。",
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
    let mut pos = 4usize;
    write_simple_index_row(
        &mut index_page,
        &mut pos,
        &body_jis("本文見出し"),
        100,
        0,
        300,
        0,
    );
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

fn write_ssed_fulltext_sidecar_start_fixture(root: &Path) -> SsedCatalog {
    let mut body = Vec::new();
    body.extend_from_slice(&[0x1f, 0x09, 0x00, 0x01, 0x1f, 0x41]);
    body.extend_from_slice(&body_jis("native body without matching digits"));
    body.extend_from_slice(&[0x1f, 0x61, 0x1f, 0x0a]);
    fs::write(
        root.join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&body], 100, 100),
    )
    .unwrap();

    fs::write(
        root.join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&[b"01"], 300, 300),
    )
    .unwrap();

    let mut index_page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    index_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    index_page[2..4].copy_from_slice(&1u16.to_be_bytes());
    let mut pos = 4usize;
    write_simple_index_row(&mut index_page, &mut pos, b"01", 100, 0, 300, 0);
    fs::write(
        root.join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&[&index_page], 200, 200),
    )
    .unwrap();

    let connection = rusqlite::Connection::open(root.join("body.db")).unwrap();
    connection
        .execute_batch(
            "
            create table t_contents (
              f_DataId integer primary key,
              f_Title text,
              f_Html text,
              f_Plane text
            );
            insert into t_contents values (
              1,
              'sidecar-only',
              '<div>01 sidecar html</div>',
              '01 sidecar body'
            );
            ",
        )
        .unwrap();

    SsedCatalog {
        title: "Synthetic sidecar start".to_owned(),
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

fn write_ssed_fulltext_multi_body_fixture(root: &Path) -> SsedCatalog {
    let mut body = Vec::new();
    let mut body_offsets = Vec::new();
    for text in [
        "first shared row needle body",
        "second shared row needle body",
        "third shared row needle body",
    ] {
        body_offsets.push(u16::try_from(body.len()).unwrap());
        body.extend_from_slice(&[0x1f, 0x09, 0x00, 0x01, 0x1f, 0x41]);
        body.extend_from_slice(&body_jis(text));
        body.extend_from_slice(&[0x1f, 0x61, 0x1f, 0x0a]);
    }
    fs::write(
        root.join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&body], 100, 100),
    )
    .unwrap();

    let mut titles = Vec::new();
    let mut title_offsets = Vec::new();
    for title in ["row one", "row two", "row three"] {
        title_offsets.push(u16::try_from(titles.len()).unwrap());
        titles.extend_from_slice(&cp932(title));
        titles.extend_from_slice(&[0x1f, 0x0a]);
    }
    fs::write(
        root.join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&[&titles], 300, 300),
    )
    .unwrap();

    let mut index_page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    index_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    index_page[2..4].copy_from_slice(&3u16.to_be_bytes());
    let mut pos = 4usize;
    for ((key, body_offset), title_offset) in ["row one", "row two", "row three"]
        .into_iter()
        .zip(body_offsets)
        .zip(title_offsets)
    {
        write_simple_index_row(
            &mut index_page,
            &mut pos,
            &body_jis(key),
            100,
            body_offset,
            300,
            title_offset,
        );
    }
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

fn write_ssed_fulltext_late_body_fixture(root: &Path) -> SsedCatalog {
    let mut body_pages = Vec::new();
    let mut rows = Vec::new();
    for index in 0..520u32 {
        let mut body = Vec::new();
        body.extend_from_slice(&SSED_ENTRY_MARKER);
        let text = if index == 519 {
            "late direct 曙光 body"
        } else {
            "irrelevant native fulltext body"
        };
        body.extend_from_slice(&body_jis(text));
        body.extend_from_slice(&[0x1f, 0x0a]);
        body.resize(crate::ssed::BLOCK_SIZE as usize, 0);
        body_pages.push(body);
        rows.push((body_jis(&format!("row{index:04}")), 100 + index));
    }
    let body_chunks = body_pages
        .chunks(16)
        .map(|pages| {
            let mut chunk = Vec::new();
            for page in pages {
                chunk.extend_from_slice(page);
            }
            chunk
        })
        .collect::<Vec<_>>();
    let body_chunk_refs = body_chunks.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let body_end_block = 100 + u32::try_from(body_pages.len()).unwrap() - 1;
    fs::write(
        root.join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&body_chunk_refs, 100, body_end_block),
    )
    .unwrap();

    let title = cp932("late body title");
    fs::write(
        root.join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&[&title], 300, 300),
    )
    .unwrap();

    let mut index_pages = Vec::new();
    for chunk in rows.chunks(64) {
        let mut page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
        page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        page[2..4].copy_from_slice(&u16::try_from(chunk.len()).unwrap().to_be_bytes());
        let mut pos = 4usize;
        for (key, body_block) in chunk {
            write_simple_index_row(&mut page, &mut pos, key, *body_block, 0, 300, 0);
        }
        index_pages.push(page);
    }
    let index_chunks = index_pages
        .chunks(16)
        .map(|pages| {
            let mut chunk = Vec::new();
            for page in pages {
                chunk.extend_from_slice(page);
            }
            chunk
        })
        .collect::<Vec<_>>();
    let index_chunk_refs = index_chunks.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let index_end_block = 200 + u32::try_from(index_pages.len()).unwrap() - 1;
    fs::write(
        root.join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&index_chunk_refs, 200, index_end_block),
    )
    .unwrap();

    SsedCatalog {
        title: "Synthetic late fulltext".to_owned(),
        components: vec![
            SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0x00,
                start_block: 100,
                end_block: body_end_block,
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
                end_block: index_end_block,
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

fn write_ssed_fulltext_physical_cursor_fixture(root: &Path) -> SsedCatalog {
    let mut body_pages = Vec::new();
    let mut titles = Vec::new();
    let mut rows = Vec::new();
    for index in 0..532u32 {
        let (title, body_text) = match index {
            530 => (
                "late body one".to_owned(),
                "first cursor needle body".to_owned(),
            ),
            531 => (
                "late body two".to_owned(),
                "second cursor needle body".to_owned(),
            ),
            _ => (
                format!("filler body {index:03}"),
                format!("ordinary filler body {index:03}"),
            ),
        };
        let mut body = Vec::new();
        body.extend_from_slice(&[0x1f, 0x09, 0x00, 0x01, 0x1f, 0x41]);
        body.extend_from_slice(&body_jis(&body_text));
        body.extend_from_slice(&[0x1f, 0x61, 0x1f, 0x0a]);
        body.resize(crate::ssed::BLOCK_SIZE as usize, 0);
        body_pages.push(body);

        let title_offset = u16::try_from(titles.len()).unwrap();
        titles.extend_from_slice(&cp932(&title));
        titles.extend_from_slice(&[0x1f, 0x0a]);

        rows.push((
            body_jis(&format!("row{index:04}")),
            100 + index,
            title_offset,
        ));
    }
    let body_chunks = body_pages
        .chunks(16)
        .map(|pages| {
            let mut chunk = Vec::new();
            for page in pages {
                chunk.extend_from_slice(page);
            }
            chunk
        })
        .collect::<Vec<_>>();
    let body_chunk_refs = body_chunks.iter().map(Vec::as_slice).collect::<Vec<_>>();
    fs::write(
        root.join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&body_chunk_refs, 100, 631),
    )
    .unwrap();
    fs::write(
        root.join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&[&titles], 700, 700),
    )
    .unwrap();

    let mut index_pages = Vec::new();
    for chunk in rows.chunks(64) {
        let mut page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
        page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        page[2..4].copy_from_slice(&(chunk.len() as u16).to_be_bytes());
        let mut pos = 4usize;
        for (key, body_block, title_offset) in chunk {
            write_simple_index_row(&mut page, &mut pos, key, *body_block, 0, 700, *title_offset);
        }
        index_pages.push(page);
    }
    let index_chunks = index_pages
        .chunks(16)
        .map(|pages| {
            let mut chunk = Vec::new();
            for page in pages {
                chunk.extend_from_slice(page);
            }
            chunk
        })
        .collect::<Vec<_>>();
    let index_page_refs = index_chunks.iter().map(Vec::as_slice).collect::<Vec<_>>();
    fs::write(
        root.join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&index_page_refs, 200, 208),
    )
    .unwrap();

    SsedCatalog {
        title: "Synthetic fulltext cursor".to_owned(),
        components: vec![
            SsedComponent {
                index: 0,
                multi: 0,
                component_type: 0x00,
                start_block: 100,
                end_block: 631,
                data: [0; 4],
                filename: "HONMON.DIC".to_owned(),
                role: SsedComponentRole::Honmon,
            },
            SsedComponent {
                index: 1,
                multi: 0,
                component_type: 0x03,
                start_block: 700,
                end_block: 700,
                data: [0; 4],
                filename: "FHTITLE.DIC".to_owned(),
                role: SsedComponentRole::Title,
            },
            SsedComponent {
                index: 2,
                multi: 0,
                component_type: 0x71,
                start_block: 200,
                end_block: 208,
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

fn write_ssed_fulltext_late_nonprefix_title_fixture(root: &Path) -> SsedCatalog {
    let mut body_one = Vec::new();
    body_one.extend_from_slice(&[0x1f, 0x09, 0x00, 0x01, 0x1f, 0x41]);
    body_one.extend_from_slice(&body_jis("body without the first title-only query"));
    body_one.extend_from_slice(&[0x1f, 0x61, 0x1f, 0x0a]);
    let mut body_two = Vec::new();
    body_two.extend_from_slice(&[0x1f, 0x09, 0x00, 0x01, 0x1f, 0x41]);
    body_two.extend_from_slice(&body_jis("body without the second title-only query"));
    body_two.extend_from_slice(&[0x1f, 0x61, 0x1f, 0x0a]);
    fs::write(
        root.join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&body_one, &body_two], 100, 101),
    )
    .unwrap();

    let title_one = body_jis("0-1計画法");
    let title_two = body_jis("追加1計画法");
    fs::write(
        root.join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&[&title_one, &title_two], 300, 301),
    )
    .unwrap();

    const FIRST_INDEX_PAGE_COUNT: usize = 1000;
    const FIRST_TITLE_PAGE_INDEX: usize = 946;
    let mut first_pages = Vec::with_capacity(FIRST_INDEX_PAGE_COUNT);
    for page_index in 0..FIRST_INDEX_PAGE_COUNT {
        let mut page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
        page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        if page_index == FIRST_TITLE_PAGE_INDEX {
            page[2..4].copy_from_slice(&1u16.to_be_bytes());
            let mut pos = 4usize;
            write_simple_index_row(&mut page, &mut pos, &body_jis("0-1計画法"), 100, 0, 300, 0);
        } else {
            page[2..4].copy_from_slice(&0u16.to_be_bytes());
        }
        first_pages.push(page);
    }
    let first_index_chunks: Vec<Vec<u8>> = first_pages
        .chunks(16)
        .map(|chunk_pages| {
            let mut chunk = Vec::new();
            for page in chunk_pages {
                chunk.extend_from_slice(page);
            }
            chunk
        })
        .collect();
    let first_index_chunk_refs: Vec<&[u8]> = first_index_chunks.iter().map(Vec::as_slice).collect();
    let first_index_end_block = 200 + u32::try_from(FIRST_INDEX_PAGE_COUNT).unwrap() - 1;
    fs::write(
        root.join("FKINDEX.DIC"),
        fixture_sseddata_literal_chunks(&first_index_chunk_refs, 200, first_index_end_block),
    )
    .unwrap();

    let mut duplicate_page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    duplicate_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    duplicate_page[2..4].copy_from_slice(&1u16.to_be_bytes());
    let mut duplicate_pos = 4usize;
    write_simple_index_row(
        &mut duplicate_page,
        &mut duplicate_pos,
        &body_jis("0-1計画法"),
        100,
        0,
        300,
        0,
    );
    let mut distinct_page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    distinct_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    distinct_page[2..4].copy_from_slice(&1u16.to_be_bytes());
    let mut distinct_pos = 4usize;
    write_simple_index_row(
        &mut distinct_page,
        &mut distinct_pos,
        &body_jis("追加1計画法"),
        101,
        0,
        301,
        0,
    );
    fs::write(
        root.join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&[&duplicate_page, &distinct_page], 1200, 1201),
    )
    .unwrap();

    const EMPTY_INDEX_PAGE_COUNT: usize = 1300;
    let empty_index_page = {
        let mut page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
        page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        page[2..4].copy_from_slice(&0u16.to_be_bytes());
        page
    };
    let empty_pages = vec![empty_index_page; EMPTY_INDEX_PAGE_COUNT];
    let empty_index_chunks: Vec<Vec<u8>> = empty_pages
        .chunks(16)
        .map(|chunk_pages| {
            let mut chunk = Vec::new();
            for page in chunk_pages {
                chunk.extend_from_slice(page);
            }
            chunk
        })
        .collect();
    let empty_index_chunk_refs: Vec<&[u8]> = empty_index_chunks.iter().map(Vec::as_slice).collect();
    let empty_index_end_block = 1300 + u32::try_from(EMPTY_INDEX_PAGE_COUNT).unwrap() - 1;
    fs::write(
        root.join("EXINDEX.DIC"),
        fixture_sseddata_literal_chunks(&empty_index_chunk_refs, 1300, empty_index_end_block),
    )
    .unwrap();

    SsedCatalog {
        title: "Synthetic late nonprefix fulltext title".to_owned(),
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
                end_block: first_index_end_block,
                data: [0; 4],
                filename: "FKINDEX.DIC".to_owned(),
                role: SsedComponentRole::Index,
            },
            SsedComponent {
                index: 3,
                multi: 0,
                component_type: 0x71,
                start_block: 1200,
                end_block: 1201,
                data: [0; 4],
                filename: "FHINDEX.DIC".to_owned(),
                role: SsedComponentRole::Index,
            },
            SsedComponent {
                index: 4,
                multi: 0,
                component_type: 0x71,
                start_block: 1300,
                end_block: empty_index_end_block,
                data: [0; 4],
                filename: "EXINDEX.DIC".to_owned(),
                role: SsedComponentRole::Index,
            },
        ],
        layout: crate::ssed::SsedInfoLayout {
            component_count_offset: 0,
            record_start: 0,
            record_size: 0x30,
            component_count: 5,
            trailing_bytes: 0,
        },
    }
}

fn write_ssed_fulltext_multi_page_title_fixture(root: &Path) -> SsedCatalog {
    let mut body = Vec::new();
    body.extend_from_slice(&[0x1f, 0x09, 0x00, 0x01, 0x1f, 0x41]);
    body.extend_from_slice(&body_jis("この本文 has a separate body needle."));
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

    let mut pages = Vec::new();
    let mut first_page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    first_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    first_page[2..4].copy_from_slice(&1u16.to_be_bytes());
    let mut pos = 4usize;
    write_simple_index_row(&mut first_page, &mut pos, b"prefix_tail", 100, 0, 300, 0);
    pages.push(first_page);
    for _ in 0..2050 {
        let mut page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
        page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        page[2..4].copy_from_slice(&0u16.to_be_bytes());
        pages.push(page);
    }
    let index_chunks: Vec<Vec<u8>> = pages
        .chunks(16)
        .map(|chunk_pages| {
            let mut chunk = Vec::new();
            for page in chunk_pages {
                chunk.extend_from_slice(page);
            }
            chunk
        })
        .collect();
    let index_chunk_refs: Vec<&[u8]> = index_chunks.iter().map(Vec::as_slice).collect();
    fs::write(
        root.join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&index_chunk_refs, 200, 2250),
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
                end_block: 2250,
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

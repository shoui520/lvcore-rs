use super::common::*;

fn assert_ssed_address_target(token: &TargetToken, component: &str, block: u32, offset: u32) {
    match token.decode().unwrap() {
        InternalTarget::SsedAddress {
            component: actual_component,
            block: actual_block,
            offset: actual_offset,
        }
        | InternalTarget::SsedIndexAddress {
            component: actual_component,
            block: actual_block,
            offset: actual_offset,
            ..
        } => {
            assert_eq!(actual_component, component);
            assert_eq!(actual_block, block);
            assert_eq!(actual_offset, offset);
        }
        other => panic!("expected SSED address-like target, got {other:?}"),
    }
}

#[test]
fn ssed_simple_index_search_returns_title_backed_hits() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture("alpha", 1, 2, 13, 0)),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert_eq!(
        package.metadata().search_modes,
        vec![
            SearchMode::Exact,
            SearchMode::Forward,
            SearchMode::Backward,
            SearchMode::Partial,
        ]
    );

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "alp".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "alpha");
    assert_eq!(page.hits[0].href, page.hits[0].target.href());
    assert_ssed_address_target(&page.hits[0].target, "HONMON.DIC", 1, 2);
}

#[test]
fn ssed_search_falls_back_to_visible_title_label_when_index_key_differs() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"quaker\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture("-keresse", 1, 2, 13, 0)),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let exact = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "quaker".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(exact.hits.len(), 1);
    assert_eq!(exact.hits[0].title_text, "quaker");

    let forward = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "qua".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(forward.hits.len(), 1);
    assert_eq!(forward.hits[0].title_text, "quaker");
}

#[test]
fn ssed_exact_search_rejects_hidden_key_match_when_visible_title_is_broader() {
    let dir = tempdir().unwrap();
    let record_start = 0x80;
    let mut catalog = vec![0u8; record_start + 3 * 0x30];
    catalog[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Cross Reference Exact";
    catalog[0x0c] = title.len() as u8;
    catalog[0x0d..0x0d + title.len()].copy_from_slice(title);
    catalog[0x4d] = 3;
    write_record(
        &mut catalog[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        "HONMON.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x30..record_start + 0x60],
        0x05,
        13,
        13,
        "FHTITLE.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x60..record_start + 0x90],
        0x81,
        14,
        14,
        "CRINDEX.DIC",
    );
    fs::write(dir.path().join("DICT.IDX"), catalog).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"doghook\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"0123456789"),
    )
    .unwrap();
    fs::write(
        dir.path().join("CRINDEX.DIC"),
        sseddata_literal_fixture(&leaf_page_fixture(&[
            title_group_record("dog", 13, 0, 1),
            compact_body_target_record(0xc0, 1, 2),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let exact = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "dog".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert!(
        exact.hits.is_empty(),
        "exact search must not show a broader visible title from a hidden/native index key"
    );

    let visible_exact = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "doghook".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(visible_exact.hits.len(), 1);
    assert_eq!(visible_exact.hits[0].title_text, "doghook");
}

#[test]
fn ssed_exact_visible_title_fallback_matches_headword_segment_before_reading() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    let mut title = body_jis("嗚呼  アア〔一般難読〕");
    title.extend_from_slice(&[0x1f, 0x0a]);
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(&title),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&leaf_page_fixture(&[simple_japanese_index_record(
            "ああ", 1, 2, 13, 0,
        )])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "嗚呼".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "嗚呼　　アア〔一般難読〕");
}

#[test]
fn ssed_search_hit_titles_strip_observed_display_only_markers() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    let mut title = body_jis("¶100円硬貨 ＜えん１【円】＞■search-alt§other-alt");
    title.extend_from_slice(&[0x1f, 0x0a]);
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(&title),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture("100", 1, 2, 13, 0)),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "100".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "１００円硬貨　＜えん１【円】＞");
    assert_eq!(page.hits[0].title_html, "１００円硬貨　＜えん１【円】＞");
}

#[test]
fn ssed_exact_search_ignores_observed_index_disambiguation_suffixes() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_raw_key_fixture_rows(&[
            (&[0x44, 0x39, 0x26, 0x24], 1, 2, 1, 2), // 長 + JIS Greek delta
            (&[0x3f, 0x37, 0x21, 0x29], 1, 4, 1, 4), // 新 + JIS fullwidth question mark
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let long = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "長".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(long.hits.len(), 1);
    assert_eq!(long.hits[0].title_text, "長");

    let shin = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "新".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(shin.hits.len(), 1);
    assert_eq!(shin.hits[0].title_text, "新");
}

#[test]
fn ssed_visible_title_label_fallback_avoids_empty_first_page_for_deep_matches() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_index_type_and_blocks(0x91, 4),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0atarget\x1f\x0a"),
    )
    .unwrap();

    let internal = internal_page_fixture(&[("a", 16), ("b", 17), ("c", 18)]);
    let mut index = Vec::new();
    index.extend_from_slice(&internal);
    let mut body_offset = 1u16;
    for page_index in 0..3 {
        let mut rows = Vec::new();
        for row_index in 0..100 {
            let title_offset = if page_index == 2 && row_index == 70 {
                7
            } else {
                0
            };
            rows.push(("x", 1, body_offset, 13, title_offset));
            body_offset = body_offset.saturating_add(1);
        }
        index.extend_from_slice(&simple_index_fixture_rows(&rows));
    }
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&index),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let first = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "target".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(first.hits.len(), 1);
    assert_eq!(first.hits[0].title_text, "target");
    assert!(
        first
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "ssed_title_label_search_fallback_no_hit_limited")
    );
}

#[test]
fn ssed_simple_index_search_matches_katakana_query_against_hiragana_key() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    let mut title = body_jis("アカウント");
    title.extend_from_slice(&[0x1f, 0x0a]);
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(&title),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&leaf_page_fixture(&[simple_japanese_index_record(
            "あかうんと",
            1,
            2,
            13,
            0,
        )])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let forward = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "アカ".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(forward.hits.len(), 1);
    assert_eq!(forward.hits[0].title_text, "アカウント");

    let exact = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "アカウント".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(exact.hits.len(), 1);
    assert_eq!(exact.hits[0].title_text, "アカウント");
}

fn simple_japanese_index_record(
    key: &str,
    body_block: u32,
    body_offset: u16,
    title_block: u32,
    title_offset: u16,
) -> Vec<u8> {
    let key = body_jis(key);
    let mut out = Vec::new();
    out.push(key.len() as u8);
    out.extend_from_slice(&key);
    out.extend_from_slice(&body_block.to_be_bytes());
    out.extend_from_slice(&body_offset.to_be_bytes());
    out.extend_from_slice(&title_block.to_be_bytes());
    out.extend_from_slice(&title_offset.to_be_bytes());
    out
}

#[test]
fn ssed_missing_optional_index_component_is_info_not_warning() {
    let dir = tempdir().unwrap();
    let record_start = 0x80;
    let mut catalog = vec![0u8; record_start + 4 * 0x30];
    catalog[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Missing Optional Index";
    catalog[0x0c] = title.len() as u8;
    catalog[0x0d..0x0d + title.len()].copy_from_slice(title);
    catalog[0x4d] = 4;
    write_record(
        &mut catalog[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        "HONMON.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x30..record_start + 0x60],
        0x05,
        13,
        14,
        "FHTITLE.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x60..record_start + 0x90],
        0x91,
        15,
        15,
        "FHINDEX.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x90..record_start + 0xc0],
        0x92,
        16,
        16,
        "KWINDEX.DIC",
    );
    fs::write(dir.path().join("DICT.IDX"), catalog).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture("alpha", 1, 2, 13, 0)),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Partial,
            query: "nomatch".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    let missing = page
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "ssed_index_component_missing")
        .expect("missing declared optional index component should be reported");
    assert_eq!(missing.severity, lvcore::DiagnosticSeverity::Info);
}

#[test]
fn ssed_ascii_exact_miss_does_not_linear_scan_optional_indexes() {
    let dir = tempdir().unwrap();
    let record_start = 0x80;
    let mut catalog = vec![0u8; record_start + 4 * 0x30];
    catalog[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"ASCII Exact Miss";
    catalog[0x0c] = title.len() as u8;
    catalog[0x0d..0x0d + title.len()].copy_from_slice(title);
    catalog[0x4d] = 4;
    write_record(
        &mut catalog[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        "HONMON.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x30..record_start + 0x60],
        0x05,
        13,
        14,
        "FHTITLE.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x60..record_start + 0x90],
        0x91,
        15,
        15,
        "FHINDEX.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x90..record_start + 0xc0],
        0x92,
        16,
        16,
        "KWINDEX.DIC",
    );
    fs::write(dir.path().join("DICT.IDX"), catalog).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture("alpha", 1, 2, 13, 0)),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "missing".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert!(page.hits.is_empty());
    assert!(
        page.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "ssed_index_component_missing"),
        "ASCII exact miss should not fall back to a full linear scan"
    );
}

#[test]
fn ssed_exact_search_prefilter_fallback_recovers_secondary_index_rows() {
    let dir = tempdir().unwrap();
    let record_start = 0x80;
    let mut catalog = vec![0u8; record_start + 5 * 0x30];
    catalog[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Secondary Index Fallback";
    catalog[0x0c] = title.len() as u8;
    catalog[0x0d..0x0d + title.len()].copy_from_slice(title);
    catalog[0x4d] = 5;
    write_record(
        &mut catalog[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        "HONMON.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x30..record_start + 0x60],
        0x05,
        13,
        14,
        "FHTITLE.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x60..record_start + 0x90],
        0x06,
        15,
        16,
        "FKTITLE.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x90..record_start + 0xc0],
        0x91,
        17,
        17,
        "FHINDEX.DIC",
    );
    write_record(
        &mut catalog[record_start + 0xc0..record_start + 0xf0],
        0x91,
        18,
        19,
        "FKINDEX.DIC",
    );
    fs::write(dir.path().join("DICT.IDX"), catalog).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"zeta\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FKTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[("zeta", 1, 4, 13, 0)])),
    )
    .unwrap();
    let fk_index = [
        internal_page_fixture(&[("\u{10ffff}", 1)]),
        simple_index_fixture_rows(&[("alpha", 1, 2, 15, 0)]),
    ]
    .concat();
    fs::write(
        dir.path().join("FKINDEX.DIC"),
        sseddata_literal_fixture(&fk_index),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "alpha".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "alpha");
    assert_ssed_address_target(&page.hits[0].target, "HONMON.DIC", 1, 2);
}

#[test]
fn ssed_exact_search_uses_bounded_secondary_tagged_indexes() {
    let dir = tempdir().unwrap();
    let record_start = 0x80;
    let mut catalog = vec![0u8; record_start + 5 * 0x30];
    catalog[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Secondary Tagged Index";
    catalog[0x0c] = title.len() as u8;
    catalog[0x0d..0x0d + title.len()].copy_from_slice(title);
    catalog[0x4d] = 5;
    write_record(
        &mut catalog[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        "HONMON.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x30..record_start + 0x60],
        0x05,
        13,
        13,
        "FHTITLE.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x60..record_start + 0x90],
        0x91,
        14,
        14,
        "FHINDEX.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x90..record_start + 0xc0],
        0x04,
        15,
        15,
        "FKTITLE.DIC",
    );
    write_record(
        &mut catalog[record_start + 0xc0..record_start + 0xf0],
        0x90,
        16,
        16,
        "FKINDEX.DIC",
    );
    fs::write(dir.path().join("DICT.IDX"), catalog).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"zeta\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[("zeta", 1, 4, 13, 0)])),
    )
    .unwrap();
    fs::write(
        dir.path().join("FKTITLE.DIC"),
        sseddata_literal_fixture(b"parent title\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FKINDEX.DIC"),
        sseddata_literal_fixture(&leaf_page_fixture(&[
            tagged_group_record("parent", 1),
            tagged_target_record("child", 1, 2, 15, 0),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "parent".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "parent title");
    assert_ssed_address_target(&page.hits[0].target, "HONMON.DIC", 1, 2);
}

#[test]
fn ssed_search_and_navigation_labels_resolve_gaiji_markers() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(dir.path().join("DICT.uni"), uni_fixture()).unwrap();
    fs::write(dir.path().join("GA16HALF"), ga16_fixture(0xA121, 8)).unwrap();
    fs::create_dir(dir.path().join("Templates")).unwrap();
    fs::write(dir.path().join("Templates/B123.SVG"), b"<svg/>").unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"0123456789"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha <zB123> zA128 zB999\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture("alpha", 1, 2, 13, 0)),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "alp".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(page.hits.len(), 1);
    let hit = &page.hits[0];
    assert_eq!(hit.title_text, "alpha 一 〓 〓");
    assert!(hit.title_html.contains("alpha 一 "));
    assert!(hit.title_html.contains("lvcore://resource/"));
    assert!(hit.title_html.contains(r#"data-gaiji="B999""#));
    assert!(!hit.title_html.contains("<zB123>"));
    assert!(!hit.title_html.contains("zA128"));
    assert!(
        hit.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "gaiji_unresolved")
    );

    let surface = package.open_surface("title-index").unwrap();
    let lvcore::NavigationSurface::TitleIndexBrowse { items, .. } = surface else {
        panic!("title-index should open as a title/index browse surface");
    };
    assert_eq!(items[0].label_text, "alpha 一 〓 〓");
    assert!(items[0].label_html.contains("lvcore://resource/"));
    assert!(items[0].label_html.contains(r#"data-gaiji="B999""#));
    assert!(
        items[0]
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "gaiji_unresolved")
    );

    let image_first_policy = GaijiPolicy {
        priority: vec![
            GaijiSourcePreference::ExternalResource,
            GaijiSourcePreference::Unicode,
            GaijiSourcePreference::Ga16Bitmap,
            GaijiSourcePreference::Unresolved,
        ],
    };
    let image_first_page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "alp".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: Some(image_first_policy.clone()),
        })
        .unwrap();
    assert_eq!(image_first_page.hits[0].title_text, "alpha 一 〓 〓");
    assert!(
        image_first_page.hits[0]
            .title_html
            .contains("lvcore-gaiji-external")
    );

    let image_first_surface = package
        .open_surface_page_with_options(
            "title-index",
            None,
            100,
            &LabelOptions {
                gaiji_policy: image_first_policy,
            },
        )
        .unwrap();
    let lvcore::NavigationSurface::TitleIndexBrowse { items, .. } = image_first_surface else {
        panic!("title-index should open as a title/index browse surface");
    };
    assert_eq!(items[0].label_text, "alpha 一 〓 〓");
    assert!(items[0].label_html.contains("lvcore-gaiji-external"));

    let window = package
        .resolve_target_window(
            &hit.target,
            Some(&lvcore::SequenceHint::TitleIndexOrder {
                value: "title-index".to_owned(),
                cursor: None,
            }),
            0,
            0,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(window.center.title.as_deref(), Some("alpha 一 〓 〓"));
}

#[test]
fn ssed_simple_index_search_supports_backward_matching() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0abeta\x1f\x0agamma\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("alpha", 1, 2, 13, 0),
            ("beta", 1, 4, 13, 7),
            ("gamma", 1, 6, 13, 12),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Backward,
            query: "ta".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "beta");
    assert_ssed_address_target(&page.hits[0].target, "HONMON.DIC", 1, 4);
}

#[test]
fn ssed_reversed_backward_index_supports_suffix_search() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_backward_index(),
    )
    .unwrap();
    fs::write(
        dir.path().join("BHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0abeta\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("BHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("ahpla", 1, 2, 13, 0),
            ("ateb", 1, 4, 13, 7),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let backward = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Backward,
            query: "ha".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(backward.hits.len(), 1);
    assert_eq!(backward.hits[0].title_text, "alpha");

    let exact = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "alpha".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(exact.hits.len(), 1);
    assert_eq!(exact.hits[0].title_text, "alpha");

    assert_ssed_address_target(&backward.hits[0].target, "HONMON.DIC", 1, 2);
}

#[test]
fn ssed_partial_search_prefers_forward_rows_when_bidirectional_indexes_exist() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_forward_and_backward_indexes(),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("BHTITLE.DIC"),
        sseddata_literal_fixture(b"shadow\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[("alpha", 1, 2, 13, 0)])),
    )
    .unwrap();
    fs::write(
        dir.path().join("BHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[("ahpla", 1, 4, 15, 0)])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Partial,
            query: "alp".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(
        page.hits
            .iter()
            .map(|hit| hit.title_text.as_str())
            .collect::<Vec<_>>(),
        ["alpha"]
    );
    assert_ssed_address_target(&page.hits[0].target, "HONMON.DIC", 1, 2);
}

#[test]
fn ssed_partial_search_pages_prefix_hits_before_nonprefix_contains_hits() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0aalpine\x1f\x0apal\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("alpha", 1, 2, 13, 0),
            ("alpine", 1, 4, 13, 7),
            ("pal", 1, 6, 13, 15),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let first = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Partial,
            query: "al".to_owned(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(first.hits[0].title_text, "alpha");
    assert!(
        first
            .next_cursor
            .as_deref()
            .is_some_and(|cursor| cursor.starts_with("ssed-partial-prefix:"))
    );

    let second = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Partial,
            query: "al".to_owned(),
            cursor: first.next_cursor,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(second.hits[0].title_text, "alpine");
    assert!(
        second
            .next_cursor
            .as_deref()
            .is_some_and(|cursor| cursor.starts_with("ssed-partial-nonprefix-index:"))
    );

    let third = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Partial,
            query: "al".to_owned(),
            cursor: second.next_cursor,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(
        third
            .hits
            .iter()
            .map(|hit| hit.title_text.as_str())
            .collect::<Vec<_>>(),
        ["pal"]
    );
}

#[test]
fn ssed_tagged_index_search_supports_grouped_rows_across_pages() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_index_type(0x90),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"child title\x1f\x0a"),
    )
    .unwrap();
    let index = [
        leaf_page_fixture(&[tagged_group_record("parent", 2)]),
        leaf_page_fixture(&[tagged_target_record("child", 1, 2, 13, 0)]),
    ]
    .concat();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&index),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "parent".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "child title");
    assert_ssed_address_target(&page.hits[0].target, "HONMON.DIC", 1, 2);
    assert!(
        page.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "ssed_index_variant_deferred")
    );
}

#[test]
fn ssed_partial_search_defers_nonprefix_fill_for_large_indexes() {
    let dir = tempdir().unwrap();
    let record_start = 0x80;
    let mut catalog = vec![0u8; record_start + 3 * 0x30];
    catalog[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Large Partial";
    catalog[0x0c] = title.len() as u8;
    catalog[0x0d..0x0d + title.len()].copy_from_slice(title);
    catalog[0x4d] = 3;
    write_record(
        &mut catalog[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        "HONMON.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x30..record_start + 0x60],
        0x05,
        13,
        13,
        "FHTITLE.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x60..record_start + 0x90],
        0x91,
        15,
        320,
        "FHINDEX.DIC",
    );
    fs::write(dir.path().join("DICT.IDX"), catalog).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0apal\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("alpha", 1, 2, 13, 0),
            ("pal", 1, 4, 13, 7),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let first = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Partial,
            query: "al".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(
        first
            .hits
            .iter()
            .map(|hit| hit.title_text.as_str())
            .collect::<Vec<_>>(),
        ["alpha"]
    );
    assert!(
        first
            .next_cursor
            .as_deref()
            .is_some_and(|cursor| cursor.starts_with("ssed-partial-nonprefix-index:"))
    );
}

#[test]
fn ssed_partial_search_parses_stateful_index_pages_without_byte_prefilter() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_index_type(0x90),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"child title\x1f\x0a"),
    )
    .unwrap();
    let index = [
        leaf_page_fixture(&[tagged_group_record("parent", 2)]),
        leaf_page_fixture(&[tagged_target_record("child", 1, 2, 13, 0)]),
    ]
    .concat();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&index),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Partial,
            query: "parent".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "child title");
    assert_ssed_address_target(&page.hits[0].target, "HONMON.DIC", 1, 2);
}

#[test]
fn ssed_keyword_and_cross_reference_indexes_resolve_grouped_body_targets() {
    for (component_type, target_tag) in [(0x80, 0xb0), (0x81, 0xc0)] {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("DICT.IDX"),
            ssedinfo_fixture_with_index_type(component_type),
        )
        .unwrap();
        fs::write(
            dir.path().join("FHTITLE.DIC"),
            sseddata_literal_fixture(b"group title\x1f\x0a"),
        )
        .unwrap();
        let index = leaf_page_fixture(&[
            title_group_record("group", 13, 0, 1),
            compact_body_target_record(target_tag, 1, 6),
        ]);
        fs::write(
            dir.path().join("FHINDEX.DIC"),
            sseddata_literal_fixture(&index),
        )
        .unwrap();
        let package = DriverRegistry::default().open_best(dir.path()).unwrap();

        let page = package
            .search(&SearchQuery {
                scope: SearchScope::CurrentBook {
                    book_id: package.metadata().book_id.clone(),
                },
                mode: SearchMode::Exact,
                query: "group".to_owned(),
                cursor: None,
                limit: 10,
                gaiji_policy: None,
            })
            .unwrap();

        assert_eq!(page.hits.len(), 1, "component type {component_type:02x}");
        assert_eq!(page.hits[0].title_text, "group title");
        assert_ssed_address_target(&page.hits[0].target, "HONMON.DIC", 1, 6);
    }
}

#[test]
fn ssed_body_only_and_multi_selector_indexes_resolve_targets() {
    for (component_type, index) in [
        (
            0x60,
            leaf_page_fixture(&[body_only_simple_record("body", 1, 8)]),
        ),
        (
            0x30,
            leaf_page_fixture(&[
                tagged_group_record("bodytag", 1),
                tagged_target_body_only_record("child", 1, 10),
            ]),
        ),
        (
            0xa1,
            leaf_page_fixture(&[
                multi_group_record("multi", 1),
                multi_target_record(1, 12, 13, 0),
            ]),
        ),
    ] {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("DICT.IDX"),
            ssedinfo_fixture_with_index_type(component_type),
        )
        .unwrap();
        fs::write(
            dir.path().join("FHTITLE.DIC"),
            sseddata_literal_fixture(b"multi title\x1f\x0a"),
        )
        .unwrap();
        fs::write(
            dir.path().join("FHINDEX.DIC"),
            sseddata_literal_fixture(&index),
        )
        .unwrap();
        let package = DriverRegistry::default().open_best(dir.path()).unwrap();
        let query = if component_type == 0x30 {
            "bodytag"
        } else if component_type == 0xa1 {
            "multi"
        } else {
            "body"
        };

        let page = package
            .search(&SearchQuery {
                scope: SearchScope::CurrentBook {
                    book_id: package.metadata().book_id.clone(),
                },
                mode: SearchMode::Exact,
                query: query.to_owned(),
                cursor: None,
                limit: 10,
                gaiji_policy: None,
            })
            .unwrap();

        assert_eq!(page.hits.len(), 1, "component type {component_type:02x}");
        let (_, expected_offset) = match component_type {
            0x60 => ("body", 8),
            0x30 => ("bodytag", 10),
            0xa1 => ("multi", 12),
            _ => unreachable!(),
        };
        assert_ssed_address_target(&page.hits[0].target, "HONMON.DIC", 1, expected_offset);
    }
}

#[test]
fn ssed_keyless_pointer_table_simple_leaf_is_supported() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"keyless\x1f\x0a"),
    )
    .unwrap();
    let mut page = vec![0u8; 2048];
    page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    page[2..4].copy_from_slice(&1u16.to_be_bytes());
    page[4..8].copy_from_slice(&1u32.to_be_bytes());
    page[8..10].copy_from_slice(&14u16.to_be_bytes());
    page[11..15].copy_from_slice(&13u32.to_be_bytes());
    page[15..17].copy_from_slice(&0u16.to_be_bytes());
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&page),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let surface = package.open_surface("title-index").unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = surface else {
        panic!("title-index should open as a title/index browse surface");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label_text, "keyless");
    assert_eq!(
        items[0].target.decode().unwrap(),
        InternalTarget::SsedIndexAddress {
            component: "HONMON.DIC".to_owned(),
            block: 1,
            offset: 14,
            index_component: "FHINDEX.DIC".to_owned(),
        }
    );
}

#[test]
fn ssed_exact_search_uses_internal_page_tree_for_simple_indexes() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_index_type_and_blocks(0x91, 3),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0azeta\x1f\x0a"),
    )
    .unwrap();
    let index = [
        internal_page_fixture(&[("m", 16), ("\u{10ffff}", 17)]),
        simple_index_fixture_rows(&[("alpha", 1, 2, 13, 0)]),
        simple_index_fixture_rows(&[("zeta", 1, 4, 13, 7)]),
    ]
    .concat();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&index),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "zeta".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "zeta");
    assert!(
        page.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "ssed_index_internal_page_deferred")
    );
}

#[test]
fn ssed_simple_index_search_handles_raw_ascii_key_order() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"2020\x1f\x0aDOG\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_raw_ascii_fixture_rows(&[
            ("2020", 1, 2, 13, 0),
            ("DOG", 1, 4, 13, 6),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "dog".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "DOG");
}

#[test]
fn ssed_simple_index_search_uses_cursor_pagination() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0abeta\x1f\x0agamma\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("alpha", 1, 2, 13, 0),
            ("beta", 1, 4, 13, 7),
            ("gamma", 1, 6, 13, 12),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let first = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Partial,
            query: "a".to_owned(),
            cursor: None,
            limit: 2,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(
        first
            .hits
            .iter()
            .map(|hit| hit.title_text.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
    assert!(
        first
            .next_cursor
            .as_deref()
            .is_some_and(|cursor| cursor.starts_with("ssed-partial-nonprefix-offset:"))
    );

    let second = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Partial,
            query: "a".to_owned(),
            cursor: first.next_cursor,
            limit: 2,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(
        second
            .hits
            .iter()
            .map(|hit| hit.title_text.as_str())
            .collect::<Vec<_>>(),
        ["gamma"]
    );
    assert!(second.next_cursor.is_none());
}

#[test]
fn ssed_partial_search_uses_physical_scan_cursor_for_sparse_indexes() {
    let dir = tempdir().unwrap();
    let record_start = 0x80;
    let index_pages = 12usize;
    let mut catalog = vec![0u8; record_start + 3 * 0x30];
    catalog[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Sparse Partial";
    catalog[0x0c] = title.len() as u8;
    catalog[0x0d..0x0d + title.len()].copy_from_slice(title);
    catalog[0x4d] = 3;
    write_record(
        &mut catalog[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        "HONMON.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x30..record_start + 0x60],
        0x05,
        13,
        14,
        "FHTITLE.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x60..record_start + 0x90],
        0x91,
        15,
        15 + index_pages as u32 - 1,
        "FHINDEX.DIC",
    );
    fs::write(dir.path().join("DICT.IDX"), catalog).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"needle one\x1f\x0aneedle two\x1f\x0a"),
    )
    .unwrap();

    let mut index = Vec::new();
    for page_index in 0..index_pages {
        let page = match page_index {
            0 => simple_index_fixture_rows(&[("needle-one", 1, 2, 13, 0)]),
            11 => simple_index_fixture_rows(&[("needle-two", 1, 4, 13, 12)]),
            _ => leaf_page_fixture(&[]),
        };
        index.extend_from_slice(&page);
    }
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&index),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let first = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Partial,
            query: "eedle".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(
        first
            .hits
            .iter()
            .map(|hit| hit.title_text.as_str())
            .collect::<Vec<_>>(),
        ["needle one"]
    );
    let next_cursor = first
        .next_cursor
        .as_deref()
        .expect("sparse scan should return a physical continuation cursor");
    assert!(next_cursor.starts_with("ssed-partial-nonprefix-noskip-index:"));

    let second = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Partial,
            query: "eedle".to_owned(),
            cursor: Some(next_cursor.to_owned()),
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(
        second
            .hits
            .iter()
            .map(|hit| hit.title_text.as_str())
            .collect::<Vec<_>>(),
        ["needle two"]
    );
    assert!(second.next_cursor.is_none());
}

#[test]
fn ssed_forward_search_uses_physical_prefilter_cursor_for_sparse_fallback() {
    let dir = tempdir().unwrap();
    let record_start = 0x80;
    let leaf_pages = 12usize;
    let total_index_pages = leaf_pages + 1;
    let mut catalog = vec![0u8; record_start + 3 * 0x30];
    catalog[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Sparse Forward Fallback";
    catalog[0x0c] = title.len() as u8;
    catalog[0x0d..0x0d + title.len()].copy_from_slice(title);
    catalog[0x4d] = 3;
    write_record(
        &mut catalog[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        "HONMON.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x30..record_start + 0x60],
        0x05,
        13,
        14,
        "FHTITLE.DIC",
    );
    write_record(
        &mut catalog[record_start + 0x60..record_start + 0x90],
        0x91,
        15,
        15 + total_index_pages as u32 - 1,
        "FHINDEX.DIC",
    );
    fs::write(dir.path().join("DICT.IDX"), catalog).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"needle one\x1f\x0aneedle two\x1f\x0a"),
    )
    .unwrap();

    let mut index = Vec::new();
    for page_index in 0..total_index_pages {
        let page = match page_index {
            0 => internal_page_fixture(&[("zz", 1)]),
            1 => simple_index_fixture_rows(&[("needle-one", 1, 2, 13, 0)]),
            12 => simple_index_fixture_rows(&[("needle-two", 1, 4, 13, 12)]),
            _ => leaf_page_fixture(&[]),
        };
        index.extend_from_slice(&page);
    }
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&index),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let first = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "needle".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(
        first
            .hits
            .iter()
            .map(|hit| hit.title_text.as_str())
            .collect::<Vec<_>>(),
        ["needle one"]
    );
    let next_cursor = first
        .next_cursor
        .as_deref()
        .expect("sparse fallback scan should return a physical continuation cursor");
    assert!(next_cursor.starts_with("ssed-prefiltered-index:"));

    let second = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "needle".to_owned(),
            cursor: Some(next_cursor.to_owned()),
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();
    assert_eq!(
        second
            .hits
            .iter()
            .map(|hit| hit.title_text.as_str())
            .collect::<Vec<_>>(),
        ["needle two"]
    );
    assert!(second.next_cursor.is_none());
}

#[test]
fn ssed_simple_index_search_does_not_limit_candidates_before_filtering() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"beta\x1f\x0aalpha\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("beta", 1, 2, 13, 0),
            ("alpha", 1, 4, 13, 6),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();

    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "alpha".to_owned(),
            cursor: None,
            limit: 1,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "alpha");
    assert_ssed_address_target(&page.hits[0].target, "HONMON.DIC", 1, 4);
}

#[test]
fn ssed_simple_index_targets_preserve_declared_honmon_component_name() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_honmon("HONMON.DIN"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture("alpha", 1, 2, 13, 0)),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let page = package
        .search(&SearchQuery {
            scope: SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "alpha".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_ssed_address_target(&page.hits[0].target, "HONMON.DIN", 1, 2);
}

#[test]
fn ssed_title_index_sequence_returns_before_and_after_views() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"0123456789"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        sseddata_literal_fixture(b"alpha\x1f\x0abeta\x1f\x0ainvalid\x1f\x0agamma\x1f\x0a"),
    )
    .unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_fixture_rows(&[
            ("alpha", 1, 0, 13, 0),
            ("beta", 1, 2, 13, 7),
            ("invalid", 0x20_0000, 0, 13, 13),
            ("gamma", 1, 4, 13, 22),
        ])),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();

    let window = package
        .resolve_target_window(
            &target,
            Some(&lvcore::SequenceHint::TitleIndexOrder {
                value: "title-index".to_owned(),
                cursor: None,
            }),
            1,
            1,
            &RenderOptions::default(),
        )
        .unwrap();

    assert_eq!(window.center.title.as_deref(), Some("beta"));
    assert_eq!(window.before.len(), 1);
    assert_eq!(window.before[0].title.as_deref(), Some("alpha"));
    assert_eq!(window.after.len(), 1);
    assert_eq!(window.after[0].title.as_deref(), Some("gamma"));
    assert!(
        !window
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "sequence_deferred")
    );
    assert!(
        !window
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_index_body_component_missing")
    );

    let body_order = package
        .resolve_target_window(
            &target,
            Some(&lvcore::SequenceHint::BodyOrder),
            1,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(body_order.center.title.as_deref(), Some("beta"));
    assert_eq!(body_order.before[0].title.as_deref(), Some("alpha"));
    assert_eq!(body_order.after[0].title.as_deref(), Some("gamma"));
    assert!(
        body_order
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "sequence_deferred")
    );
}

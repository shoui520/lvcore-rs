use std::fs;

use super::*;

#[test]
fn ssed_title_index_browse_uses_internal_tree_order_not_physical_leaf_order() {
    let dir = tempdir().unwrap();

    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[b"alpha body\0beta body"], 100, 100),
    )
    .unwrap();

    let mut titles = Vec::new();
    let alpha_title_offset = 0u16;
    titles.extend_from_slice(b"alpha\x1f\x0a");
    let beta_title_offset = u16::try_from(titles.len()).unwrap();
    titles.extend_from_slice(b"beta\x1f\x0a");
    let title_chunks = titles.chunks(crate::ssed::CHUNK_SIZE).collect::<Vec<_>>();
    let title_block_count = titles
        .len()
        .div_ceil(crate::ssed::BLOCK_SIZE as usize)
        .max(1);
    let title_end_block = 300 + u32::try_from(title_block_count.saturating_sub(1)).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&title_chunks, 300, title_end_block),
    )
    .unwrap();

    let mut internal = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    internal[0..2].copy_from_slice(&0x0002u16.to_be_bytes());
    internal[2..4].copy_from_slice(&2u16.to_be_bytes());
    let mut pos = 4usize;
    write_internal_index_row(&mut internal, &mut pos, b"a", 202);
    write_internal_index_row(&mut internal, &mut pos, b"b", 201);

    let mut beta_leaf = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    beta_leaf[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    beta_leaf[2..4].copy_from_slice(&1u16.to_be_bytes());
    let mut beta_pos = 4usize;
    write_simple_index_row(
        &mut beta_leaf,
        &mut beta_pos,
        b"beta",
        100,
        11,
        300,
        beta_title_offset,
    );

    let mut alpha_leaf = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    alpha_leaf[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    alpha_leaf[2..4].copy_from_slice(&1u16.to_be_bytes());
    let mut alpha_pos = 4usize;
    write_simple_index_row(
        &mut alpha_leaf,
        &mut alpha_pos,
        b"alpha",
        100,
        0,
        300,
        alpha_title_offset,
    );

    let mut index_stream = Vec::new();
    index_stream.extend_from_slice(&internal);
    index_stream.extend_from_slice(&beta_leaf);
    index_stream.extend_from_slice(&alpha_leaf);
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&[&index_stream], 200, 202),
    )
    .unwrap();

    let catalog = SsedCatalog {
        title: "Ordered".to_owned(),
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
                end_block: 202,
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
    };
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Ordered".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );

    let surface = package.open_surface("title-index").unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = &surface else {
        panic!("expected title/index browse surface");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].label_text, "alpha");
    assert_eq!(items[1].label_text, "beta");

    let beta = surface
        .actionable_targets()
        .into_iter()
        .find(|target| target.label_text == "beta")
        .unwrap();
    let window = package
        .resolve_target_window(
            &beta.target,
            beta.sequence_hint.as_ref(),
            1,
            0,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(window.center.title.as_deref(), Some("beta"));
    assert_eq!(window.before.len(), 1);
    assert_eq!(window.before[0].title.as_deref(), Some("alpha"));
}

#[test]
fn ssed_title_index_browse_falls_back_to_resolved_body_title_for_placeholder_labels() {
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

    let titles = b"?\x1f\x0a";
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&[titles], 300, 300),
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
              1,
              100,
              4,
              'resolved sidecar title',
              'sidecar body',
              'resolved sidecar title'
            );
            ",
        )
        .unwrap();

    let mut index_page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    index_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    index_page[2..4].copy_from_slice(&1u16.to_be_bytes());
    let mut pos = 4usize;
    write_simple_index_row(&mut index_page, &mut pos, b"?", 100, 0, 300, 0);
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&[&index_page], 200, 200),
    )
    .unwrap();

    let catalog = SsedCatalog {
        title: "Placeholder".to_owned(),
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
    };
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Placeholder".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );

    let surface = package.open_surface("title-index").unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = &surface else {
        panic!("expected title/index browse surface");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label_text, "resolved sidecar title");

    let view = package
        .render_target(&items[0].target, &RenderOptions::default())
        .unwrap();
    assert_eq!(view.title.as_deref(), Some("resolved sidecar title"));
}

#[test]
fn ssed_visible_title_label_fallback_does_not_return_empty_first_page_for_deep_matches() {
    let dir = tempdir().unwrap();

    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[b"body"], 100, 100),
    )
    .unwrap();

    let row_count = 300usize;
    let rows_per_leaf = 64usize;
    let leaf_count = row_count.div_ceil(rows_per_leaf);
    let mut titles = Vec::new();
    let mut title_offsets = Vec::new();
    for index in 0..row_count {
        title_offsets.push(u16::try_from(titles.len()).unwrap());
        if index + 1 == row_count {
            titles.extend_from_slice(b"target\x1f\x0a");
        } else {
            titles.extend_from_slice(b"x\x1f\x0a");
        }
    }
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&[&titles], 300, 300),
    )
    .unwrap();

    let mut internal = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    internal[0..2].copy_from_slice(&0x0002u16.to_be_bytes());
    internal[2..4].copy_from_slice(&u16::try_from(leaf_count).unwrap().to_be_bytes());
    let mut internal_pos = 4usize;
    let leaf_key = body_jis("あ");
    for leaf_index in 0..leaf_count {
        write_internal_index_row(
            &mut internal,
            &mut internal_pos,
            &leaf_key,
            201 + u32::try_from(leaf_index).unwrap(),
        );
    }

    let mut index_stream = Vec::new();
    index_stream.extend_from_slice(&internal);
    for leaf_index in 0..leaf_count {
        let start = leaf_index * rows_per_leaf;
        let end = (start + rows_per_leaf).min(row_count);
        let mut leaf = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
        leaf[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        leaf[2..4].copy_from_slice(&u16::try_from(end - start).unwrap().to_be_bytes());
        let mut pos = 4usize;
        for title_offset in &title_offsets[start..end] {
            write_simple_index_row(&mut leaf, &mut pos, &leaf_key, 100, 0, 300, *title_offset);
        }
        index_stream.extend_from_slice(&leaf);
    }
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(
            &index_stream
                .chunks(crate::ssed::CHUNK_SIZE)
                .collect::<Vec<_>>(),
            200,
            200 + leaf_count as u32,
        ),
    )
    .unwrap();

    let catalog = SsedCatalog {
        title: "Deep label".to_owned(),
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
                end_block: 200 + leaf_count as u32,
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
    };
    let search_modes = ssed_search_modes(&catalog, dir.path());
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Deep label".to_owned()),
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
            query: "target".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "target");
    assert_eq!(page.next_cursor, None);
}

#[test]
fn ssed_visible_title_label_fallback_skips_short_exact_misses() {
    let dir = tempdir().unwrap();

    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[b"body"], 100, 100),
    )
    .unwrap();

    let row_count = 3000usize;
    let rows_per_leaf = 64usize;
    let leaf_count = row_count.div_ceil(rows_per_leaf);
    let mut titles = Vec::new();
    let mut title_offsets = Vec::new();
    for index in 0..row_count {
        title_offsets.push(u16::try_from(titles.len()).unwrap());
        if index + 1 == row_count {
            titles.extend_from_slice(b"x\x1f\x0a");
        } else {
            titles.extend_from_slice(b"q\x1f\x0a");
        }
    }
    let title_chunks = titles.chunks(crate::ssed::CHUNK_SIZE).collect::<Vec<_>>();
    let title_block_count = titles
        .len()
        .div_ceil(crate::ssed::BLOCK_SIZE as usize)
        .max(1);
    let title_end_block = 300 + u32::try_from(title_block_count.saturating_sub(1)).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&title_chunks, 300, title_end_block),
    )
    .unwrap();

    let mut internal = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    internal[0..2].copy_from_slice(&0x0002u16.to_be_bytes());
    internal[2..4].copy_from_slice(&u16::try_from(leaf_count).unwrap().to_be_bytes());
    let mut internal_pos = 4usize;
    for leaf_index in 0..leaf_count {
        write_internal_index_row(
            &mut internal,
            &mut internal_pos,
            b"a",
            201 + u32::try_from(leaf_index).unwrap(),
        );
    }

    let mut index_stream = Vec::new();
    index_stream.extend_from_slice(&internal);
    for leaf_index in 0..leaf_count {
        let start = leaf_index * rows_per_leaf;
        let end = (start + rows_per_leaf).min(row_count);
        let mut leaf = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
        leaf[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        leaf[2..4].copy_from_slice(&u16::try_from(end - start).unwrap().to_be_bytes());
        let mut pos = 4usize;
        for title_offset in &title_offsets[start..end] {
            write_simple_index_row(&mut leaf, &mut pos, b"a", 100, 0, 300, *title_offset);
        }
        index_stream.extend_from_slice(&leaf);
    }
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(
            &index_stream
                .chunks(crate::ssed::CHUNK_SIZE)
                .collect::<Vec<_>>(),
            200,
            200 + leaf_count as u32,
        ),
    )
    .unwrap();

    let catalog = SsedCatalog {
        title: "Short exact".to_owned(),
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
                end_block: title_end_block,
                data: [0; 4],
                filename: "FHTITLE.DIC".to_owned(),
                role: SsedComponentRole::Title,
            },
            SsedComponent {
                index: 2,
                multi: 0,
                component_type: 0x91,
                start_block: 200,
                end_block: 200 + leaf_count as u32,
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
    };
    let search_modes = ssed_search_modes(&catalog, dir.path());
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Short exact".to_owned()),
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
            query: "x".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert!(page.hits.is_empty());
    assert!(page.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ssed_title_label_search_fallback_skipped_short_query"
    }));
}

#[test]
fn ssed_partial_physical_scan_does_not_return_empty_first_page_before_later_matches() {
    let dir = tempdir().unwrap();

    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[b"body"], 100, 100),
    )
    .unwrap();

    fs::write(
        dir.path().join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&[b"target\x1f\x0a"], 300, 300),
    )
    .unwrap();

    let leaf_count = 96usize;
    let mut internal = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    internal[0..2].copy_from_slice(&0x0002u16.to_be_bytes());
    internal[2..4].copy_from_slice(&u16::try_from(leaf_count).unwrap().to_be_bytes());
    let mut internal_pos = 4usize;
    for leaf_index in 0..leaf_count {
        write_internal_index_row(
            &mut internal,
            &mut internal_pos,
            b"x",
            201 + u32::try_from(leaf_index).unwrap(),
        );
    }

    let mut index_stream = Vec::new();
    index_stream.extend_from_slice(&internal);
    for leaf_index in 0..leaf_count {
        let mut leaf = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
        leaf[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        if leaf_index + 1 == leaf_count {
            leaf[2..4].copy_from_slice(&1u16.to_be_bytes());
            let mut pos = 4usize;
            write_simple_index_row(&mut leaf, &mut pos, b"target", 100, 0, 300, 0);
        } else {
            leaf[2..4].copy_from_slice(&0u16.to_be_bytes());
        }
        index_stream.extend_from_slice(&leaf);
    }
    let index_chunks = index_stream
        .chunks(crate::ssed::CHUNK_SIZE)
        .collect::<Vec<_>>();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&index_chunks, 200, 200 + leaf_count as u32),
    )
    .unwrap();

    let catalog = SsedCatalog {
        title: "Partial".to_owned(),
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
                end_block: 200 + leaf_count as u32,
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
    };
    let search_modes = ssed_search_modes(&catalog, dir.path());
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Partial".to_owned()),
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
            mode: SearchMode::Partial,
            query: "target".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "target");
    assert_eq!(page.next_cursor, None);
    assert!(
        page.diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "ssed_index_empty_physical_pages_skipped" })
    );
    assert!(
        !page
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "ssed_index_empty_physical_scan_limited" })
    );
}

#[test]
fn ssed_partial_physical_scan_limits_empty_prefilter_queries() {
    let dir = tempdir().unwrap();

    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[b"body"], 100, 100),
    )
    .unwrap();

    fs::write(
        dir.path().join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&[b"target\x1f\x0a"], 300, 300),
    )
    .unwrap();

    let leaf_count = 320usize;
    let mut internal = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    internal[0..2].copy_from_slice(&0x0002u16.to_be_bytes());
    internal[2..4].copy_from_slice(&u16::try_from(leaf_count).unwrap().to_be_bytes());
    let mut internal_pos = 4usize;
    for leaf_index in 0..leaf_count {
        write_internal_index_row(
            &mut internal,
            &mut internal_pos,
            b"x",
            201 + u32::try_from(leaf_index).unwrap(),
        );
    }

    let mut index_stream = Vec::new();
    index_stream.extend_from_slice(&internal);
    for _ in 0..leaf_count {
        let mut leaf = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
        leaf[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        leaf[2..4].copy_from_slice(&1u16.to_be_bytes());
        let mut pos = 4usize;
        write_simple_index_row(&mut leaf, &mut pos, b"target", 999_999, 0, 300, 0);
        index_stream.extend_from_slice(&leaf);
    }
    let index_chunks = index_stream
        .chunks(crate::ssed::CHUNK_SIZE)
        .collect::<Vec<_>>();
    let index_end_block = 200 + u32::try_from(leaf_count).unwrap();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&index_chunks, 200, index_end_block),
    )
    .unwrap();

    let catalog = SsedCatalog {
        title: "Partial".to_owned(),
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
    };
    let search_modes = ssed_search_modes(&catalog, dir.path());
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Partial".to_owned()),
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
            mode: SearchMode::Partial,
            query: "two words".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert!(page.hits.is_empty());
    assert!(
        page.next_cursor
            .as_deref()
            .is_some_and(|cursor| cursor.starts_with("ssed-partial-nonprefix-noskip-index:")),
        "{page:#?}"
    );
    assert!(page.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ssed_index_empty_physical_scan_limited"
            && diagnostic
                .context
                .get("advanced_empty_pages")
                .is_some_and(|value| value == "8")
    }));
}

#[test]
fn ssed_visible_title_label_fallback_does_not_return_empty_no_hit_cursor() {
    let dir = tempdir().unwrap();

    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[b"body"], 100, 100),
    )
    .unwrap();

    let row_count = 20_481usize;
    let rows_per_leaf = 64usize;
    let leaf_count = row_count.div_ceil(rows_per_leaf);
    let mut titles = Vec::new();
    let mut title_offsets = Vec::new();
    for _ in 0..row_count {
        title_offsets.push(u16::try_from(titles.len()).unwrap());
        titles.extend_from_slice(b"x\x1f\x0a");
    }
    let title_chunks = titles.chunks(crate::ssed::CHUNK_SIZE).collect::<Vec<_>>();
    let title_block_count = titles
        .len()
        .div_ceil(crate::ssed::BLOCK_SIZE as usize)
        .max(1);
    let title_end_block = 300 + u32::try_from(title_block_count.saturating_sub(1)).unwrap();
    fs::write(
        dir.path().join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&title_chunks, 300, title_end_block),
    )
    .unwrap();

    let mut internal = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    internal[0..2].copy_from_slice(&0x0002u16.to_be_bytes());
    internal[2..4].copy_from_slice(&u16::try_from(leaf_count).unwrap().to_be_bytes());
    let mut internal_pos = 4usize;
    let leaf_key = body_jis("あ");
    for leaf_index in 0..leaf_count {
        write_internal_index_row(
            &mut internal,
            &mut internal_pos,
            &leaf_key,
            201 + u32::try_from(leaf_index).unwrap(),
        );
    }

    let mut index_stream = Vec::new();
    index_stream.extend_from_slice(&internal);
    for leaf_index in 0..leaf_count {
        let start = leaf_index * rows_per_leaf;
        let end = (start + rows_per_leaf).min(row_count);
        let mut leaf = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
        leaf[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        leaf[2..4].copy_from_slice(&u16::try_from(end - start).unwrap().to_be_bytes());
        let mut pos = 4usize;
        for title_offset in &title_offsets[start..end] {
            write_simple_index_row(&mut leaf, &mut pos, &leaf_key, 100, 0, 300, *title_offset);
        }
        index_stream.extend_from_slice(&leaf);
    }
    let index_chunks = index_stream
        .chunks(crate::ssed::CHUNK_SIZE)
        .collect::<Vec<_>>();
    fs::write(
        dir.path().join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&index_chunks, 200, 200 + leaf_count as u32),
    )
    .unwrap();
    let connection = Connection::open(dir.path().join("body.db")).unwrap();
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
              'missing',
              '<div>missing html</div>',
              'missing body'
            );
            ",
        )
        .unwrap();

    let catalog = SsedCatalog {
        title: "No hit label".to_owned(),
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
                end_block: title_end_block,
                data: [0; 4],
                filename: "FHTITLE.DIC".to_owned(),
                role: SsedComponentRole::Title,
            },
            SsedComponent {
                index: 2,
                multi: 0,
                component_type: 0x91,
                start_block: 200,
                end_block: 200 + leaf_count as u32,
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
    };
    let search_modes = ssed_search_modes(&catalog, dir.path());
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("No hit label".to_owned()),
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
            query: "absent".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert!(page.hits.is_empty());
    assert_eq!(page.next_cursor, None);
    assert!(page.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ssed_title_label_search_fallback_no_hit_limited"
    }));

    let sidecar_page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Exact,
            query: "missing".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(sidecar_page.hits.len(), 1);
    assert_eq!(sidecar_page.hits[0].title_text, "missing");
    assert!(
        sidecar_page
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "ssed_sidecar_title_search" })
    );
    assert!(!sidecar_page.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ssed_title_label_search_fallback_no_hit_limited"
    }));
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

fn write_internal_index_row(page: &mut [u8], pos: &mut usize, key: &[u8], child_block: u32) {
    page[*pos..*pos + key.len()].copy_from_slice(key);
    *pos += 2;
    page[*pos..*pos + 4].copy_from_slice(&child_block.to_be_bytes());
    *pos += 4;
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

#[cfg(unix)]
#[test]
fn ssed_encyclopedia_index_symlink_escape_is_deferred() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(
        outside.path().join("encyclop.idx"),
        cp932(
            "#LVEDBRSR encyclopedia#Ver.1.0 2008.01.07\t\t\n\
                 #Outside\t\t\n\
                 00000000\t00000000\tOutside\t\t\n",
        ),
    )
    .unwrap();
    symlink(
        outside.path().join("encyclop.idx"),
        dir.path().join("encyclop.idx"),
    )
    .unwrap();

    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("Sample".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores::default(),
    );

    let surface = package.open_surface("encyclopedia").unwrap();
    let NavigationSurface::Deferred {
        surface_id,
        diagnostics,
    } = surface
    else {
        panic!("expected symlinked encyclop.idx to be deferred");
    };
    assert_eq!(surface_id, "encyclopedia");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ssed_encyclopedia_index_missing"
            || diagnostic.code == "ssed_encyclopedia_index_read_failed"
    }));
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
                 00005221\t00000750\t\t\t冬\n\
                 10000000\t0000FFFF\t\t西和ABC順\n\
                 01000000\t0000FFFF\t\t五十音\n",
        ),
    )
    .unwrap();
    fs::write(
        dir.path().join("Panels.xml"),
        r#"<?xml version="1.0"?><panels version="1.0"></panels>"#,
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
    let target = nodes[0].children[0]
        .target
        .as_ref()
        .unwrap()
        .decode()
        .unwrap();
    assert!(matches!(
        target,
        InternalTarget::SsedBoundedAddress {
            component,
            block: 0x5221,
            offset: 0x0722,
            end_block: 0x5221,
            end_offset: 0x0750
        } if component == "HONMON.DIC"
    ));
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
                cursor: None,
            }),
            1,
            0,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(window.before.len(), 1);
    assert_eq!(window.before[0].title.as_deref(), Some("季語"));

    let first_page = package.open_surface_page("aux-index:0", None, 2).unwrap();
    let NavigationSurface::HierarchicalTree {
        nodes, next_cursor, ..
    } = first_page
    else {
        panic!("expected paged auxiliary navigation tree");
    };
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].label_text, "大辞林 第四版");
    assert_eq!(nodes[0].children[0].label_text, "季語");
    assert!(matches!(
        nodes[0].children[0]
            .target
            .as_ref()
            .unwrap()
            .decode()
            .unwrap(),
        InternalTarget::SsedBoundedAddress {
            component,
            block: 0x5221,
            offset: 0x0722,
            end_block: 0x5221,
            end_offset: 0x0750
        } if component == "HONMON.DIC"
    ));
    assert_eq!(next_cursor.as_deref(), Some("2"));

    let second_page = package
        .open_surface_page("aux-index:0", next_cursor.as_deref(), 2)
        .unwrap();
    let NavigationSurface::HierarchicalTree {
        nodes, next_cursor, ..
    } = second_page
    else {
        panic!("expected second paged auxiliary navigation tree");
    };
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].label_text, "春");
    assert_eq!(nodes[1].label_text, "冬");
    assert_eq!(next_cursor.as_deref(), Some("4"));
}

#[test]
fn ssed_exinfo_auxiliary_index_drives_search_when_native_indexes_are_absent() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("EXINFO.INI"),
        cp932("[GENERAL]\nIDXCOUNT=1\nIDXNAME0=メニュー\nIDXINFO0=SPEECH.IDX\n"),
    )
    .unwrap();
    fs::write(
        dir.path().join("SPEECH.IDX"),
        cp932(
            "00000000\t00000000\tスピーチ文例集\n\
             00000002\t00000010\t\t▼開宴の辞（司会者）\n\
             00000002\t00000080\t\t▼閉会の辞（司会者）\n",
        ),
    )
    .unwrap();
    let mut body = vec![b'a'; 256];
    let mut entry = Vec::new();
    entry.extend_from_slice(&SSED_ENTRY_MARKER);
    entry.extend_from_slice(&body_jis("▼開宴の辞（司会者）"));
    entry.extend_from_slice(&[0x1f, 0x0a]);
    body[0x30..0x30 + entry.len()].copy_from_slice(&entry);
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&body], 2, 2),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "SPEECH".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 2,
            end_block: 2,
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
    let search_modes = ssed_aux_index_search_modes(dir.path()).unwrap();
    assert_eq!(
        search_modes,
        vec![
            SearchMode::Exact,
            SearchMode::Forward,
            SearchMode::Backward,
            SearchMode::Partial
        ]
    );
    let mut capabilities = ssed_capabilities(&catalog, dir.path());
    capabilities.push(Capability::NativeSearch);
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 95,
            title: Some("SPEECH".to_owned()),
            evidence: Vec::new(),
        },
        capabilities,
        PackageStores {
            ssed_catalog: Some(catalog),
            search_modes,
            ..Default::default()
        },
    );

    assert!(
        package
            .metadata()
            .search_modes
            .contains(&SearchMode::Forward)
    );
    assert!(
        !package
            .metadata()
            .search_modes
            .contains(&SearchMode::FullText)
    );
    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "開宴".to_owned(),
            cursor: None,
            limit: 10,
            gaiji_policy: None,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "▼開宴の辞（司会者）");
    assert!(
        page.diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "ssed_auxiliary_index_label_search" })
    );
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::SsedBoundedAddress {
            component,
            block: 2,
            offset: 0x30,
            end_block: 2,
            end_offset: 0x80
        } if component == "HONMON.DIC"
    ));
}

#[test]
fn ssed_auxiliary_index_keeps_control_offset_honmon_targets_actionable() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("EXINFO.INI"),
        cp932("[GENERAL]\nIDXINFO=000002BC.idx\nIDXTITLE=付録\n"),
    )
    .unwrap();
    fs::write(
        dir.path().join("000002BC.idx"),
        cp932(
            "00000000\t00000000\tRoot\n\
                 00000002\t00000002\t\tMarker child\n\
                 00000002\t00000004\t\tPayload child\n",
        ),
    )
    .unwrap();
    let body = [
        0x1f, 0x02, 0x1f, 0x09, 0x00, 0x01, 0x1f, 0x41, 0x00, 0x01, 0x1f, 0x61, 0x1f, 0x0a,
    ];
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&body], 2, 2),
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "Aux marker".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 2,
            end_block: 2,
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
            title: Some("Aux marker".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );

    let surface = package.open_surface("aux-index:0").unwrap();
    let NavigationSurface::HierarchicalTree { nodes, .. } = surface else {
        panic!("expected auxiliary navigation tree");
    };
    let marker_child = &nodes[0].children[0];
    let payload_child = &nodes[0].children[1];
    assert!(matches!(
        marker_child
            .target
            .as_ref()
            .unwrap()
            .decode()
            .unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 2,
            offset: 2
        } if component == "HONMON.DIC"
    ));
    assert!(matches!(
        payload_child
            .target
            .as_ref()
            .unwrap()
            .decode()
            .unwrap(),
        InternalTarget::SsedAddress {
            component,
            block: 2,
            offset: 4
        } if component == "HONMON.DIC"
    ));
}

#[test]
fn ssed_panel_cell_target_renders_panel_surface_without_internal_id_title() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("Panels.xml"),
        r#"<?xml version="1.0"?>
<panels>
  <panel index="01000000" paneltype="contents" count_x="1">
    <title>五十音</title>
    <data>
      <cell ref="20100000">あ</cell>
    </data>
  </panel>
  <panel index="20100000" paneltype="contents" count_x="1">
    <title>あ</title>
    <data>
      <cell>亜</cell>
    </data>
  </panel>
</panels>"#,
    )
    .unwrap();
    let package = ReaderBookPackage::new(
        dir.path(),
        DetectedPackage {
            root: dir.path().to_path_buf(),
            format_family: FormatFamily::Ssed,
            confidence: 80,
            title: Some("Panels".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores::default(),
    );

    let home = package.home_surfaces().unwrap();
    let panels_home = home
        .iter()
        .find(|surface| surface.surface_id == "panels")
        .expect("panel home surface");
    assert_eq!(panels_home.kind, NavigationSurfaceKind::Panel);
    assert_eq!(panels_home.title_text, "五十音");
    assert_eq!(panels_home.title_html, "五十音");

    let root_view = package
        .render_target(
            panels_home.target.as_ref().unwrap(),
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(root_view.title.as_deref(), Some("五十音"));

    let root = package.open_surface("panels").unwrap();
    let NavigationSurface::Panel { cells, .. } = root else {
        panic!("expected root panel surface");
    };
    assert_eq!(cells[0].label_text, "あ");
    let child_target = cells[0].target.as_ref().unwrap();

    let view = package
        .render_target(child_target, &RenderOptions::default())
        .unwrap();

    assert_eq!(view.kind, crate::render::ResolvedTargetKind::PanelSurface);
    assert_eq!(view.title.as_deref(), Some("あ"));
    assert_ne!(view.title.as_deref(), Some("20100000"));
    let NavigationSurface::Panel { cells, .. } = view.surface.unwrap() else {
        panic!("expected child panel surface");
    };
    assert_eq!(cells[0].label_text, "亜");
}

#[cfg(unix)]
#[test]
fn ssed_adjacent_panel_symlink_escape_is_not_advertised() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let root = dir.path().join("_DCT_SAMPLE");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("Panels.xml"),
        r#"<?xml version="1.0"?>
<panels>
  <panel index="01000000" paneltype="contents">
    <title>五十音</title>
    <data type="bin" filename="Panel\All-A.bin" />
  </panel>
</panels>"#,
    )
    .unwrap();
    let outside = dir.path().join("outside-panel");
    fs::create_dir(&outside).unwrap();
    let panel_bin = (1u32)
        .to_le_bytes()
        .into_iter()
        .chain((4u32).to_le_bytes())
        .chain((3u32).to_le_bytes())
        .chain((0x20u32).to_le_bytes())
        .chain([0x24, 0x22, 0, 0])
        .collect::<Vec<_>>();
    fs::write(outside.join("All-A.bin"), panel_bin).unwrap();
    symlink(&outside, dir.path().join("_DCT_SAMPLE_Panel")).unwrap();
    let package = ReaderBookPackage::new(
        &root,
        DetectedPackage {
            root: root.clone(),
            format_family: FormatFamily::Ssed,
            confidence: 80,
            title: Some("Panels".to_owned()),
            evidence: Vec::new(),
        },
        Vec::new(),
        PackageStores::default(),
    );

    let surface = package.open_surface("panels:01000000").unwrap();

    let NavigationSurface::Deferred { diagnostics, .. } = surface else {
        panic!("expected deferred surface when adjacent Panel root is a symlink");
    };
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ssed_panel_bin_missing"
            && diagnostic.message.contains("Panel\\All-A.bin")
    }));
}

#[test]
fn ssed_numeric_auxiliary_index_opens_without_exinfo() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("0000015f.idx"),
        cp932(
            "00000000\t00000000\tRoot\n\
                 00005221\t00000722\t\tChild\n\
                 00000001\t0000ffff\t\tPanel selector without panel metadata\n",
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
    assert_eq!(nodes[0].children.len(), 2);
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
    assert!(nodes[0].children[1].target.is_none());
    assert!(nodes[0].children[1].diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ssed_auxiliary_index_virtual_selector_without_panels"
    }));
}

#[test]
fn ssed_auxiliary_index_routes_menu_component_targets_as_menu_items() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("0000015f.idx"),
        cp932(
            "00000000\t00000000\tRoot\n\
                 00007539\t00000606\t\tA\n",
        ),
    )
    .unwrap();
    fs::write(dir.path().join("00000001.idx"), SSEDINFO_MAGIC).unwrap();
    let catalog = SsedCatalog {
        title: "Aux Menu".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 0x7539,
            end_block: 0x7602,
            data: [0; 4],
            filename: "MENU.DIC".to_owned(),
            role: SsedComponentRole::Menu,
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
            title: Some("Aux Menu".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
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
        InternalTarget::MenuItem {
            surface_id,
            item_id,
        } if surface_id == "menu" && item_id == format!("addr:{}:{}", 0x7539, 0x0606)
    ));
}

#[test]
fn ssed_ios_table_list_window_uses_plist_order() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[b"alpha body", b"beta body", b"gamma body"], 100, 102),
    )
    .unwrap();
    fs::write(
        dir.path().join("tableList.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<array>
  <dict><key>name</key><string>Alpha</string><key>block</key><integer>100</integer><key>offset</key><integer>0</integer></dict>
  <dict><key>name</key><string>Beta</string><key>block</key><integer>101</integer><key>offset</key><integer>0</integer></dict>
  <dict><key>name</key><string>Gamma</string><key>block</key><integer>102</integer><key>offset</key><integer>0</integer></dict>
</array>
</plist>
"#,
    )
    .unwrap();
    let catalog = SsedCatalog {
        title: "iOS table list".to_owned(),
        components: vec![SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0x00,
            start_block: 100,
            end_block: 102,
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
            title: Some("iOS table list".to_owned()),
            evidence: Vec::new(),
        },
        ssed_capabilities(&catalog, dir.path()),
        PackageStores {
            ssed_catalog: Some(catalog),
            ..Default::default()
        },
    );

    let surface = package
        .open_surface_page("ios-table-list:tableList.plist", None, 10)
        .unwrap();
    let NavigationSurface::TitleIndexBrowse { items, .. } = &surface else {
        panic!("expected iOS table-list title/index browse surface");
    };
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].label_text, "Alpha");
    assert_eq!(items[1].label_text, "Beta");
    assert_eq!(items[2].label_text, "Gamma");

    let targets = surface.actionable_targets();
    let beta = targets
        .iter()
        .find(|target| target.label_text == "Beta")
        .unwrap();
    assert_eq!(
        beta.sequence_hint,
        Some(SequenceHint::TitleIndexOrder {
            value: "ios-table-list:tableList.plist".to_owned(),
            cursor: Some("1".to_owned()),
        })
    );
    let window = package
        .resolve_target_window(
            &beta.target,
            beta.sequence_hint.as_ref(),
            1,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(window.center.title.as_deref(), Some("Beta"));
    assert_eq!(window.before.len(), 1);
    assert_eq!(window.before[0].title.as_deref(), Some("Alpha"));
    assert_eq!(window.after.len(), 1);
    assert_eq!(window.after[0].title.as_deref(), Some("Gamma"));
    assert!(!window.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "sequence_target_not_in_title_index"
            || diagnostic.code == "sequence_deferred"
    }));
}

#[cfg(unix)]
#[test]
fn ssed_numeric_auxiliary_index_ignores_symlinked_escape() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(
        outside.path().join("00000160.idx"),
        cp932(
            "00000000\t00000000\tOutside\n\
                 00005221\t00000722\t\tEscaped\n",
        ),
    )
    .unwrap();
    symlink(
        outside.path().join("00000160.idx"),
        dir.path().join("00000160.idx"),
    )
    .unwrap();
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

    assert!(
        !package
            .metadata()
            .capabilities
            .contains(&Capability::AuxiliaryIndex)
    );
    assert!(
        !package
            .home_surfaces()
            .unwrap()
            .iter()
            .any(|surface| surface.surface_id == "numeric-aux:00000160.idx")
    );
}

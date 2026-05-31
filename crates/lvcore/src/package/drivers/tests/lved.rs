use super::*;

#[test]
fn detects_lved_sqlite3_by_main_data_and_key() {
    let dir = tempdir().unwrap();
    write_lved_search_fixture(dir.path());

    let detected = LvedSqliteDriver.detect(dir.path()).unwrap().unwrap();
    assert_eq!(detected.format_family, FormatFamily::LvedSqlite3);
    assert!(
        detected
            .evidence
            .iter()
            .any(|item| item.starts_with("key_file:"))
    );
}

#[cfg(unix)]
#[test]
fn lved_detection_ignores_symlinked_payload_escape() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    write_lved_search_fixture(outside.path());
    symlink(
        outside.path().join("main.data"),
        dir.path().join("main.data"),
    )
    .unwrap();
    fs::write(dir.path().join("main.key"), "test-key").unwrap();

    assert!(LvedSqliteDriver.detect(dir.path()).unwrap().is_none());
}

#[cfg(unix)]
#[test]
fn lved_key_discovery_ignores_symlinked_key_escape() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let payload = dir.path().join("main.data");
    fs::write(&payload, b"payload").unwrap();
    fs::write(outside.path().join("main.key"), "outside-key").unwrap();
    symlink(outside.path().join("main.key"), dir.path().join("main.key")).unwrap();

    assert!(
        crate::lved_sqlite::discover_lved_key_file(&payload)
            .unwrap()
            .is_none()
    );
}

#[test]
fn lved_search_hits_resolve_to_preserved_content_html() {
    let dir = tempdir().unwrap();
    write_lved_search_fixture(dir.path());
    let package = LvedSqliteDriver.open(dir.path()).unwrap();
    let surfaces = package.home_surfaces().unwrap();
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::TitleIndexBrowse
            && surface.surface_id == "lved-list"
            && surface.status == NavigationStatus::Available
            && surface.target.is_some()
    }));
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::Info && surface.status == NavigationStatus::Available
    }));
    let list_surface = package.open_surface("lved-list").unwrap();
    let list_items = match list_surface {
        NavigationSurface::TitleIndexBrowse { items, .. } => items,
        _ => panic!("expected LVED list title/index surface"),
    };
    assert_eq!(list_items.len(), 3);
    assert_eq!(list_items[0].label_text, "alpha subtitle");
    assert!(list_items[0].label_html.contains("lvcore://resource/"));
    assert!(!list_items[0].label_html.contains("src=\"AC6E.svg\""));
    assert!(matches!(
        list_items[0].target.decode().unwrap(),
        InternalTarget::LvedRow {
            table,
            row_id: 100,
            anchor: Some(anchor),
            query: None
        } if table == "content" && anchor == "body-anchor"
    ));
    let info_surface = package.open_surface("info").unwrap();
    let info_target = match info_surface {
        NavigationSurface::InfoPages { pages, .. } => pages[0].target.clone(),
        _ => panic!("expected LVED info pages surface"),
    };
    let info_view = package
        .render_target(&info_target, &RenderOptions::default())
        .unwrap();
    assert_eq!(info_view.kind, ResolvedTargetKind::InfoPage);
    assert_eq!(
        info_view.display_html.as_deref(),
        Some("<h1>Example Dictionary 第2版</h1>")
    );
    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "alp".to_owned(),
            cursor: None,
            limit: 10,
        })
        .unwrap();

    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "alpha");
    assert!(page.hits[0].title_html.contains("lvcore://resource/"));
    assert!(!page.hits[0].title_html.contains("src=\"AC6E.svg\""));
    assert!(matches!(
        page.hits[0].target.decode().unwrap(),
        InternalTarget::LvedRow {
            table,
            row_id: 100,
            anchor: Some(_),
            query: None
        } if table == "content"
    ));

    let view = package
        .render_target(&page.hits[0].target, &RenderOptions::default())
        .unwrap();

    assert_eq!(view.kind, ResolvedTargetKind::EntryBody);
    let html = view.display_html.as_deref().unwrap();
    assert!(html.contains("<article><h1>Alpha</h1><p>body</p>"));
    assert!(html.contains("lvcore://resource/"));
    assert!(html.contains("lvcore://target/"));
    assert!(!html.contains("lved.dataid:101"));
    assert!(!html.contains("lved.info:help.html"));
    assert_eq!(view.links.len(), 2);
    assert!(view.links.iter().any(|link| matches!(
        link.token.decode().unwrap(),
        InternalTarget::LvedRow {
            table,
            row_id: 101,
            anchor: Some(anchor),
            query: None
        } if table == "content" && anchor == "jump"
    )));
    let help_token = view
        .links
        .iter()
        .find_map(|link| match link.token.decode().unwrap() {
            InternalTarget::LvedInfoPage {
                name,
                anchor: Some(anchor),
            } if name == "help.html" && anchor == "top" => Some(link.token.clone()),
            _ => None,
        })
        .expect("expected lved.info link to be routed through TargetToken");
    let help_view = package
        .render_target(&help_token, &RenderOptions::default())
        .unwrap();
    assert_eq!(help_view.kind, ResolvedTargetKind::InfoPage);
    assert_eq!(help_view.display_html.as_deref(), Some("<h1>Help</h1>"));
    assert_eq!(view.resources.len(), 2);
    assert!(view.capabilities.contains(&RenderCapability::Html));
    assert!(view.capabilities.contains(&RenderCapability::Images));
    assert!(view.capabilities.contains(&RenderCapability::Audio));
    assert!(
        view.resources
            .iter()
            .any(|resource| resource.kind == ResourceKind::Image)
    );
    assert!(
        view.resources
            .iter()
            .any(|resource| resource.kind == ResourceKind::Audio)
    );
    let audio = view
        .resources
        .iter()
        .find(|resource| resource.kind == ResourceKind::Audio)
        .unwrap();
    assert_eq!(audio.mime_type.as_deref(), Some("audio/mpeg"));
    assert_eq!(
        package.read_resource(&audio.token).unwrap(),
        b"ID3\x03".to_vec()
    );
    let image = view
        .resources
        .iter()
        .find(|resource| resource.kind == ResourceKind::Image)
        .unwrap();
    assert_eq!(image.mime_type.as_deref(), Some("image/svg+xml"));
    assert_eq!(
        package.read_resource(&image.token).unwrap(),
        b"<svg/>".to_vec()
    );

    let window = package
        .resolve_target_window(
            &page.hits[0].target,
            Some(&SequenceHint::LvedListOrder),
            0,
            2,
            &RenderOptions::default(),
        )
        .unwrap();
    assert!(window.before.is_empty());
    assert_eq!(window.after.len(), 2);
    assert_eq!(window.after[0].title.as_deref(), Some("beta"));
    assert_eq!(window.after[1].title.as_deref(), Some("gamma"));

    let search_result_sequence = SearchResultSequence::new(
        list_items
            .into_iter()
            .map(|item| crate::sequence::SearchResultSequenceTarget {
                target: item.target,
                title: Some(item.label_text),
            })
            .collect(),
    )
    .unwrap()
    .encode()
    .unwrap();
    let search_window = package
        .resolve_target_window(
            &window.after[0].target,
            Some(&SequenceHint::SearchResults {
                value: search_result_sequence,
            }),
            1,
            1,
            &RenderOptions::default(),
        )
        .unwrap();
    assert_eq!(search_window.before.len(), 1);
    assert_eq!(search_window.center.title.as_deref(), Some("beta"));
    assert_eq!(search_window.after.len(), 1);
    assert_eq!(search_window.after[0].title.as_deref(), Some("gamma"));
}

#[test]
fn render_modes_are_explicit_for_preserved_lved_html() {
    let dir = tempdir().unwrap();
    write_lved_search_fixture(dir.path());
    let package = LvedSqliteDriver.open(dir.path()).unwrap();
    let page = package
        .search(&SearchQuery {
            scope: crate::search::SearchScope::CurrentBook {
                book_id: package.metadata().book_id.clone(),
            },
            mode: SearchMode::Forward,
            query: "alp".to_owned(),
            cursor: None,
            limit: 10,
        })
        .unwrap();
    let target = &page.hits[0].target;

    let basic = package
        .render_target(
            target,
            &RenderOptions {
                mode: RenderMode::BasicText,
                ..RenderOptions::default()
            },
        )
        .unwrap();
    assert!(basic.display_html.is_none());
    assert!(basic.basic_text.as_deref().unwrap().contains("Alpha"));
    assert!(basic.resources.is_empty());
    assert!(basic.links.is_empty());

    let generic = package
        .render_target(
            target,
            &RenderOptions {
                mode: RenderMode::GenericHtml,
                ..RenderOptions::default()
            },
        )
        .unwrap();
    let generic_html = generic.display_html.as_deref().unwrap();
    assert!(!generic_html.contains("lvcore://target/"));
    assert!(!generic_html.contains("lvcore://resource/"));
    assert!(generic_html.contains("#lvcore-target-"));
    assert!(generic_html.contains("data:image/svg+xml;base64,"));
    assert!(
        generic
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "generic_html_resources_inlined")
    );
    assert!(
        generic
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "generic_html_targets_fragmentized")
    );

    let debug = package
        .render_target(
            target,
            &RenderOptions {
                mode: RenderMode::Debug,
                ..RenderOptions::default()
            },
        )
        .unwrap();
    let debug_trace = debug.debug_trace.as_deref().unwrap();
    assert!(debug_trace.contains(r#""mode":"debug""#));
    assert!(debug_trace.contains(r#""has_display_html":true"#));
}

#[test]
fn visual_capabilities_are_derived_from_html_and_resources() {
    let target = TargetToken::new(&InternalTarget::Unsupported {
        reason: "synthetic".to_owned(),
    })
    .unwrap();
    let resource = ResourceToken::new(&InternalResource::PackageFile {
        path: "sound.mp3".to_owned(),
        resource_kind: ResourceKind::Audio,
    })
    .unwrap();
    let view = finalize_resolved_view(
        ResolvedTargetView {
            kind: ResolvedTargetKind::EntryBody,
            target,
            title: None,
            display_html: Some(
                r#"<p>\(x+1\)</p><link rel="stylesheet" href="style.css">"#.to_owned(),
            ),
            basic_text: None,
            scroll_anchor: None,
            surface: None,
            resources: vec![ResourceRef {
                token: resource,
                kind: ResourceKind::Audio,
                label: None,
                href: None,
                mime_type: Some("audio/mpeg".to_owned()),
                diagnostics: Vec::new(),
            }],
            links: Vec::new(),
            capabilities: Vec::new(),
            diagnostics: Vec::new(),
            debug_trace: None,
        },
        &RenderOptions::default(),
    );

    assert!(view.capabilities.contains(&RenderCapability::Html));
    assert!(view.capabilities.contains(&RenderCapability::Css));
    assert!(view.capabilities.contains(&RenderCapability::MathJax));
    assert!(view.capabilities.contains(&RenderCapability::Audio));
}

#[test]
fn lved_protocol_router_preserves_observed_non_entry_hooks() {
    let dir = tempdir().unwrap();
    let payload = dir.path().join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        apply_sqlcipher_key(&connection, key).unwrap();
        connection
            .execute_batch(
                r#"
                create table content (id integer primary key, type integer, body text, media text);
                create table list (id integer primary key, refid integer, type integer, anchor text, title text, titlesub text);
                create table media (id integer primary key, name text, type integer, main blob);
                create table binran (id integer primary key, name text, body text);
                insert into content values (
                  200,
                  1,
                  '<article>
                    <a href="lved.dataid.result:201#detail">result</a>
                    <a href="lved.dataid202#legacy">legacy</a>
                    <a href="lved.dataid.dict.STEDABBR:300#cross">dict</a>
                    <a href="lved.contentlink:BUREI.400#note">contentlink</a>
                    <a href="lved.binran:usage.html#top">binran</a>
                    <a href="lved.bookmark:C001">bookmark</a>
                    <img src="lved.image:fig01.png">
                    <a href="lved.pdf:manual.pdf">pdf</a>
                    <script src="./MathJax/MathJax.js"></script>
                  </article>',
                  ''
                );
                insert into content values (201, 1, '<article>result detail</article>', '');
                insert into content values (202, 1, '<article>legacy detail</article>', '');
                insert into list values (1, 200, 1, '', 'router', '');
                insert into media values (1, 'fig01', 4, X'89504E470D0A1A0A');
                insert into media values (2, 'manual', 6, X'255044462D312E37');
                insert into binran values (1, 'usage.html', '<h1>Binran</h1>');
                "#,
            )
            .unwrap();
    }
    fs::write(dir.path().join("main.key"), key).unwrap();

    let package = LvedSqliteDriver.open(dir.path()).unwrap();
    let target = TargetToken::new(&InternalTarget::LvedRow {
        table: "content".to_owned(),
        row_id: 200,
        anchor: None,
        query: None,
    })
    .unwrap();
    let view = package
        .render_target(&target, &RenderOptions::default())
        .unwrap();
    let html = view.display_html.as_deref().unwrap();

    for raw in [
        "lved.dataid.result:",
        "lved.dataid202",
        "lved.dataid.dict.",
        "lved.contentlink:",
        "lved.binran:",
        "lved.bookmark:",
        "lved.image:",
        "lved.pdf:",
    ] {
        assert!(!html.contains(raw), "{raw} leaked through normalized HTML");
    }
    assert_eq!(
        view.resources
            .iter()
            .map(|resource| resource.kind)
            .collect::<Vec<_>>(),
        vec![ResourceKind::Image, ResourceKind::Pdf]
    );
    assert_eq!(
        view.links.iter().map(|link| link.kind).collect::<Vec<_>>(),
        vec![
            TargetKind::LvedRow,
            TargetKind::LvedRow,
            TargetKind::LvedCrossBook,
            TargetKind::LvedCrossBook,
            TargetKind::LvedNamedPage,
            TargetKind::LvedViewerHook,
        ]
    );

    let binran = view
        .links
        .iter()
        .find(|link| link.kind == TargetKind::LvedNamedPage)
        .unwrap();
    let binran_view = package
        .render_target(&binran.token, &RenderOptions::default())
        .unwrap();
    assert_eq!(binran_view.kind, ResolvedTargetKind::InfoPage);
    assert_eq!(binran_view.display_html.as_deref(), Some("<h1>Binran</h1>"));

    let cross = view
        .links
        .iter()
        .find(|link| link.kind == TargetKind::LvedCrossBook)
        .unwrap();
    let cross_view = package
        .render_target(&cross.token, &RenderOptions::default())
        .unwrap();
    assert_eq!(cross_view.kind, ResolvedTargetKind::Unsupported);
    assert!(
        cross_view
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "lved_cross_book_deferred")
    );
}

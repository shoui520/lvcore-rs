use super::*;
use crate::validate::{
    ValidateOptions, search_with_empty_cursor_follow, targetless_surface_probe,
    validate_detected_package_json, validate_package_json,
};
use lvcore::lved_sqlite::apply_sqlcipher_key;
use rusqlite::Connection;
use std::fs;

#[test]
fn discovery_ignores_resource_directories_with_non_package_idx_files() {
    let dir = tempfile::tempdir().unwrap();
    let resources = dir.path().join("Viewer.app/Contents/Resources");
    fs::create_dir_all(&resources).unwrap();
    fs::write(resources.join("Localizable.idx"), b"not an SSED catalog").unwrap();

    let discovered = DriverRegistry::default()
        .discover_roots(dir.path(), PackageDiscoveryOptions::default())
        .unwrap();

    assert!(discovered.is_empty());
}

#[test]
fn detect_command_recurses_when_root_is_not_a_package() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("NestedDictionary");
    fs::create_dir_all(&package).unwrap();
    write_lved_cli_fixture(&package);

    let detections = DriverRegistry::default()
        .detect_all(dir.path(), PackageDiscoveryOptions::default())
        .unwrap();

    assert_eq!(detections.len(), 1);
    assert_eq!(
        detections[0].format_family,
        lvcore::FormatFamily::LvedSqlite3
    );
    assert_eq!(detections[0].root, package);
}

#[test]
fn single_book_commands_open_single_nested_package_wrapper() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("NestedDictionary");
    fs::create_dir_all(&package).unwrap();
    write_lved_cli_fixture(&package);

    let output = search_command_json(
        &DriverRegistry::default(),
        dir.path(),
        "alp".to_owned(),
        SearchMode::Forward,
        10,
        None,
        RenderOptions::default(),
        false,
        0,
        0,
    )
    .unwrap();

    assert_eq!(
        output["metadata"]["package_root"].as_str().unwrap(),
        package.to_string_lossy()
    );
    assert_eq!(output["hits"].as_array().unwrap().len(), 1);
}

#[test]
fn single_book_commands_reject_ambiguous_multi_package_roots() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("FirstDictionary");
    let second = dir.path().join("SecondDictionary");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    write_lved_cli_fixture(&first);
    write_lved_cli_fixture(&second);

    let error = search_command_json(
        &DriverRegistry::default(),
        dir.path(),
        "alp".to_owned(),
        SearchMode::Forward,
        10,
        None,
        RenderOptions::default(),
        false,
        0,
        0,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("path contains multiple package candidates")
    );
}

#[test]
fn registry_can_stream_discovered_packages_without_precollecting() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("FirstDictionary");
    let second = dir.path().join("SecondDictionary");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    write_lved_cli_fixture(&first);
    write_lved_cli_fixture(&second);

    let mut seen = Vec::new();
    DriverRegistry::default()
        .for_each_best_package(dir.path(), PackageDiscoveryOptions::default(), |detected| {
            seen.push(detected.root);
            Ok(())
        })
        .unwrap();

    assert_eq!(seen, vec![first, second]);
}

#[test]
fn library_discover_command_does_not_open_lved_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("CandidateOnly");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("main.data"), b"not decryptable sqlite").unwrap();
    fs::write(package.join("main.key"), "test-key").unwrap();

    let output = library_discover_command_json(
        &DriverRegistry::default(),
        &[dir.path().to_path_buf()],
        None,
    )
    .unwrap();

    assert_eq!(output["candidate_count"], 1);
    assert_eq!(output["diagnostics"].as_array().unwrap().len(), 0);
    assert_eq!(
        output["candidates"][0]["format_family"],
        serde_json::json!("lved_sqlite3")
    );
    assert!(output["candidates"][0].get("title_hint").is_none());
    assert_eq!(
        output["candidates"][0]["root"].as_str().unwrap(),
        package.to_string_lossy()
    );
}

#[test]
fn library_discover_command_deduplicates_overlapping_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("CandidateOnly");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("main.data"), b"not decryptable sqlite").unwrap();
    fs::write(package.join("main.key"), "test-key").unwrap();

    let output = library_discover_command_json(
        &DriverRegistry::default(),
        &[dir.path().to_path_buf(), package.clone()],
        None,
    )
    .unwrap();

    assert_eq!(output["candidate_count"], 1);
    assert_eq!(
        output["candidates"][0]["root"].as_str().unwrap(),
        package.to_string_lossy()
    );
}

#[test]
fn library_discover_jsonl_streams_candidates_and_summary() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("FirstDictionary");
    let second = dir.path().join("SecondDictionary");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    write_lved_cli_fixture(&first);
    write_lved_cli_fixture(&second);
    fs::write(second.join("extra.bin"), b"distinct root metadata").unwrap();

    let mut rows = Vec::new();
    library_discover_command_jsonl_with_emit(
        &DriverRegistry::default(),
        &[dir.path().to_path_buf()],
        None,
        |row| {
            rows.push(row.clone());
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["event"], "candidate");
    assert_eq!(rows[0]["status"], "detected");
    assert_eq!(rows[1]["event"], "candidate");
    assert_eq!(rows[2]["event"], "summary");
    assert_eq!(rows[2]["candidate_count"], 2);
}

#[test]
fn library_import_jsonl_streams_package_rows_and_summary() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("NestedDictionary");
    fs::create_dir_all(&package).unwrap();
    write_lved_cli_fixture(&package);

    let mut rows = Vec::new();
    library_import_command_jsonl_with_emit(
        &DriverRegistry::default(),
        &[dir.path().to_path_buf()],
        None,
        |row| {
            rows.push(row.clone());
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["event"], "book");
    assert_eq!(rows[0]["status"], "opened");
    assert_eq!(rows[0]["metadata"]["format_family"], "lved_sqlite3");
    assert_eq!(rows[1]["event"], "summary");
    assert_eq!(rows[1]["book_count"], 1);
    assert_eq!(rows[1]["opened_book_ids"].as_array().unwrap().len(), 1);
}

#[test]
fn library_import_jsonl_reports_duplicate_rows_without_reopening_summary_count() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("NestedDictionary");
    fs::create_dir_all(&package).unwrap();
    write_lved_cli_fixture(&package);

    let mut rows = Vec::new();
    library_import_command_jsonl_with_emit(
        &DriverRegistry::default(),
        &[dir.path().to_path_buf(), package],
        None,
        |row| {
            rows.push(row.clone());
            Ok(())
        },
    )
    .unwrap();

    assert!(rows.iter().any(|row| row["status"] == "skipped_duplicate"));
    assert_eq!(rows.last().unwrap()["event"], "summary");
    assert_eq!(rows.last().unwrap()["book_count"], 1);
}

#[test]
fn metadata_for_missing_book_id_returns_error_instead_of_panicking() {
    let library = BookLibrary::new();
    let book_id = BookId("missing-book".to_owned());

    let error = metadata_for(&library, &book_id).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("opened book id missing-book is not in the library")
    );
}

#[test]
fn validate_can_open_a_previously_detected_package() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());
    let registry = DriverRegistry::default();
    let detected = registry
        .discover_best_packages(dir.path(), PackageDiscoveryOptions::default())
        .unwrap()
        .into_iter()
        .next()
        .unwrap();

    let output = validate_detected_package_json(
        &registry,
        detected,
        ValidateOptions {
            deep: false,
            include_expensive_search: false,
        },
    );

    assert_eq!(output["status"], "ok");
    assert_eq!(output["title"], "Example Dictionary");
    assert!(!validate_row_has_failure(&output));
}

#[test]
fn advanced_column_overrides_unit_search_mode() {
    assert_eq!(
        cli_search_mode(CliSearchMode::Forward, Some(" advanced1 ".to_owned())),
        SearchMode::Advanced("advanced1".to_owned())
    );
    assert_eq!(
        cli_search_mode(CliSearchMode::Exact, Some(" ".to_owned())),
        SearchMode::Exact
    );
}

#[test]
fn cli_search_mode_accepts_fulltext_aliases() {
    for alias in ["fulltext", "full-text", "full_text"] {
        let args =
            Args::try_parse_from(["lvcore", "search", "/tmp/dict", "needle", "--mode", alias])
                .unwrap();
        let Command::Search { mode, .. } = args.command else {
            panic!("expected search command");
        };
        assert_eq!(SearchMode::from(mode), SearchMode::FullText);
    }
}

#[test]
fn cli_search_mode_accepts_advanced_mode_shorthand() {
    let args = Args::try_parse_from([
        "lvcore",
        "search",
        "/tmp/dict",
        "needle",
        "--mode",
        "advanced:phrase",
    ])
    .unwrap();
    let Command::Search { mode, .. } = args.command else {
        panic!("expected search command");
    };
    assert_eq!(
        SearchMode::from(mode),
        SearchMode::Advanced("phrase".to_owned())
    );
}

#[test]
fn cli_render_mode_maps_to_reader_render_options() {
    assert_eq!(
        cli_render_options(CliRenderMode::GenericHtml, true),
        RenderOptions {
            mode: RenderMode::GenericHtml,
            include_debug_trace: true,
            ..RenderOptions::default()
        }
    );
    assert_eq!(
        cli_render_options(CliRenderMode::BasicText, false).mode,
        RenderMode::BasicText
    );
}

#[test]
fn search_command_uses_library_scoped_resource_hrefs() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let output = search_command_json(
        &DriverRegistry::default(),
        dir.path(),
        "alp".to_owned(),
        SearchMode::Forward,
        10,
        None,
        RenderOptions::default(),
        true,
        0,
        0,
    )
    .unwrap();

    let title_html = output["hits"][0]["title_html"].as_str().unwrap();
    let display_html = output["rendered_first"]["display_html"].as_str().unwrap();
    assert!(has_scoped_resource_href(title_html));
    assert!(has_scoped_resource_href(display_html));
    assert!(!title_html.contains("src=\"AC6E.svg\""));
    assert!(!display_html.contains("data=\"AC6E.svg\""));
}

#[test]
fn validate_command_reports_advertised_search_modes() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let output = validate_package_json(
        &DriverRegistry::default(),
        dir.path(),
        ValidateOptions {
            deep: false,
            include_expensive_search: false,
        },
    );

    assert_eq!(output["status"], "ok");
    assert_eq!(
        output["search_modes"],
        serde_json::json!([
            "exact",
            "forward",
            "backward",
            "partial",
            "full_text",
            { "advanced": "advanced1" },
            { "advanced": "advanced2" },
        ])
    );
    assert!(!validate_row_has_failure(&output));
}

#[test]
fn validate_failure_detector_flags_open_and_deep_exercise_errors() {
    assert!(validate_row_has_failure(&serde_json::json!({
        "status": "open_error",
        "error": "broken",
    })));
    assert!(validate_row_has_failure(&serde_json::json!({
        "status": "ok",
        "exercises": [
            { "status": "ok" },
            { "status": "render_error", "error": "broken" }
        ],
    })));
    assert!(validate_row_has_failure(&serde_json::json!({
        "status": "ok",
        "exercises": [
            {
                "status": "ok",
                "rendered_first": {
                    "status": "resource_read_error",
                    "error": "broken"
                }
            }
        ],
    })));
    assert!(!validate_row_has_failure(&serde_json::json!({
        "status": "ok",
        "exercises": [
            { "status": "ok" },
            { "status": "deferred" },
            { "status": "no_target" }
        ],
    })));
}

#[test]
fn validate_targetless_visible_diagnostic_surface_is_ok_without_fake_target() {
    let surface = lvcore::NavigationSurface::HierarchicalTree {
        surface_id: "aux-index:0".to_owned(),
        nodes: vec![lvcore::navigation::NavigationNode {
            node_id: "aux-index:1:0".to_owned(),
            label_html: "Appendix".to_owned(),
            label_text: "Appendix".to_owned(),
            target: None,
            href: None,
            child_cursor: None,
            diagnostics: Vec::new(),
            children: vec![lvcore::navigation::NavigationNode {
                node_id: "aux-index:2:1".to_owned(),
                label_html: "Diagnostic row".to_owned(),
                label_text: "Diagnostic row".to_owned(),
                target: None,
                href: None,
                child_cursor: None,
                diagnostics: vec![lvcore::Diagnostic::info(
                    "ssed_auxiliary_index_body_target_deferred",
                    "auxiliary row is not a renderable entry body",
                )],
                children: Vec::new(),
            }],
        }],
        next_cursor: None,
    };

    let probe = targetless_surface_probe(&surface);

    assert_eq!(probe.status, "ok");
    assert_eq!(probe.visible_item_count, 2);
    assert_eq!(probe.diagnostic_count, 1);
    assert_eq!(
        probe.diagnostics[0].code,
        "ssed_auxiliary_index_body_target_deferred"
    );
}

#[test]
fn validate_deep_exercises_first_rendered_resource() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let output = validate_package_json(
        &DriverRegistry::default(),
        dir.path(),
        ValidateOptions {
            deep: true,
            include_expensive_search: false,
        },
    );
    let exercises = output["exercises"].as_array().unwrap();
    let resource_probe = exercises
        .iter()
        .filter_map(|exercise| exercise.get("first_resource"))
        .find(|probe| !probe.is_null())
        .expect("deep validation should read at least one rendered resource");

    assert_eq!(resource_probe["status"], "ok");
    assert_eq!(resource_probe["kind"], "image");
    assert_eq!(resource_probe["mime_type"], "image/svg+xml");
    assert_eq!(resource_probe["byte_len"].as_u64(), Some(6));
    assert!(!validate_row_has_failure(&output));
}

#[test]
fn validate_deep_exercises_reader_render_modes() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let output = validate_package_json(
        &DriverRegistry::default(),
        dir.path(),
        ValidateOptions {
            deep: true,
            include_expensive_search: false,
        },
    );
    let exercises = output["exercises"].as_array().unwrap();
    let rendered = exercises
        .iter()
        .find(|exercise| exercise["kind"] == "surface_first_target")
        .expect("deep validation should render a surface target");
    let modes = &rendered["render_modes"];

    assert_eq!(modes["generic_html"]["status"], "ok");
    assert_eq!(modes["generic_html"]["has_router_refs"], false);
    assert!(
        modes["generic_html"]["diagnostic_codes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["code"] == "generic_html_resources_inlined")
    );
    assert!(
        modes["generic_html"]["diagnostic_codes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["code"] == "generic_html_targets_fragmentized")
    );
    assert_eq!(modes["basic_text"]["status"], "ok");
    assert_eq!(modes["basic_text"]["display_html_len"].as_u64(), Some(0));
    assert!(
        modes["basic_text"]["basic_text_len"]
            .as_u64()
            .is_some_and(|len| len > 0)
    );
    assert!(!validate_row_has_failure(&output));
}

#[test]
fn validate_deep_exercises_continuous_windows() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let output = validate_package_json(
        &DriverRegistry::default(),
        dir.path(),
        ValidateOptions {
            deep: true,
            include_expensive_search: false,
        },
    );
    let exercises = output["exercises"].as_array().unwrap();
    let surface_window = exercises
        .iter()
        .find(|exercise| exercise["kind"] == "surface_first_target")
        .and_then(|exercise| exercise.get("window"))
        .expect("surface target validation should exercise continuous view");
    assert_eq!(surface_window["status"], "ok");
    assert_eq!(surface_window["after_count"].as_u64(), Some(1));

    let search_window = exercises
        .iter()
        .find(|exercise| exercise["kind"] == "search_forward")
        .and_then(|exercise| exercise.get("window"))
        .expect("search validation should exercise search-result continuous view");
    assert_eq!(search_window["status"], "ok");
    assert_eq!(search_window["center_kind"], "entry_body");
    assert!(!validate_row_has_failure(&output));
}

#[test]
fn validate_deep_scans_beyond_first_target_for_rendered_resources() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());
    {
        let connection = Connection::open(dir.path().join("main.data")).unwrap();
        apply_sqlcipher_key(&connection, "test-key").unwrap();
        connection
            .execute(
                "update content set body = '<article><p>body</p></article>' where id = 100",
                [],
            )
            .unwrap();
        connection
            .execute(
                "update content set body = '<article><object data=\"AC6E.svg\"></object><p>next body</p></article>' where id = 101",
                [],
            )
            .unwrap();
    }

    let output = validate_package_json(
        &DriverRegistry::default(),
        dir.path(),
        ValidateOptions {
            deep: true,
            include_expensive_search: false,
        },
    );
    let exercises = output["exercises"].as_array().unwrap();
    let resource_scan = exercises
        .iter()
        .filter_map(|exercise| exercise.get("resource_scan"))
        .find(|scan| scan["status"] == "ok")
        .expect("deep validation should scan past a resource-free first target");

    assert_eq!(resource_scan["target_index"].as_u64(), Some(1));
    assert_eq!(resource_scan["checked_target_count"].as_u64(), Some(2));
    assert_eq!(resource_scan["first_resource"]["status"], "ok");
    assert_eq!(
        resource_scan["first_resource"]["byte_len"].as_u64(),
        Some(6)
    );
    assert!(!validate_row_has_failure(&output));
}

#[test]
fn validate_deep_exercises_advertised_search_modes() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let output = validate_package_json(
        &DriverRegistry::default(),
        dir.path(),
        ValidateOptions {
            deep: true,
            include_expensive_search: false,
        },
    );
    let exercises = output["exercises"].as_array().unwrap();
    let kinds = exercises
        .iter()
        .filter_map(|exercise| exercise["kind"].as_str())
        .collect::<std::collections::BTreeSet<_>>();

    for expected in [
        "search_exact",
        "search_forward",
        "search_backward",
        "search_partial",
        "search_full_text",
        "search_advanced_advanced1",
        "search_advanced_advanced2",
    ] {
        assert!(kinds.contains(expected), "missing {expected}");
    }
    assert!(!validate_row_has_failure(&output));
}

#[test]
fn validate_deep_search_probes_use_real_navigation_label_not_book_title() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let output = validate_package_json(
        &DriverRegistry::default(),
        dir.path(),
        ValidateOptions {
            deep: true,
            include_expensive_search: false,
        },
    );
    let exercises = output["exercises"].as_array().unwrap();
    let exact = exercises
        .iter()
        .find(|exercise| exercise["kind"] == "search_exact")
        .expect("missing exact validation row");
    let forward = exercises
        .iter()
        .find(|exercise| exercise["kind"] == "search_forward")
        .expect("missing forward validation row");
    let partial = exercises
        .iter()
        .find(|exercise| exercise["kind"] == "search_partial")
        .expect("missing partial validation row");

    assert_eq!(exact["query"], "alpha");
    assert_eq!(forward["query"], "al");
    assert_eq!(partial["query"], "al");
    assert_ne!(exact["query"], "Example Dictionary");
    assert!(!validate_row_has_failure(&output));
}

#[test]
fn validate_deep_exercises_ssed_advertised_search_modes() {
    let dir = tempfile::tempdir().unwrap();
    write_ssed_cli_fixture(dir.path());

    let output = validate_package_json(
        &DriverRegistry::default(),
        dir.path(),
        ValidateOptions {
            deep: true,
            include_expensive_search: false,
        },
    );
    let exercises = output["exercises"].as_array().unwrap();
    let kinds = exercises
        .iter()
        .filter_map(|exercise| exercise["kind"].as_str())
        .collect::<std::collections::BTreeSet<_>>();

    for expected in [
        "search_exact",
        "search_forward",
        "search_backward",
        "search_partial",
        "search_full_text",
    ] {
        assert!(kinds.contains(expected), "missing {expected}");
    }
    let partial = exercises
        .iter()
        .find(|exercise| exercise["kind"] == "search_partial")
        .expect("missing partial validation row");
    assert_eq!(partial["status"], "skipped_expensive");
    let fulltext = exercises
        .iter()
        .find(|exercise| exercise["kind"] == "search_full_text")
        .expect("missing fulltext validation row");
    assert_eq!(fulltext["status"], "skipped_expensive");
    assert!(!validate_row_has_failure(&output));
}

#[test]
fn validate_deep_does_not_need_empty_cursor_pages_for_deep_visible_title_hits() {
    let dir = tempfile::tempdir().unwrap();
    write_ssed_visible_title_cursor_cli_fixture(dir.path());

    let registry = DriverRegistry::default();
    let mut library = lvcore::BookLibrary::new();
    let book_id = library.open_path(dir.path(), &registry).unwrap();
    let (page, cursor_pages_followed) =
        search_with_empty_cursor_follow(&library, &book_id, &SearchMode::Exact, "target", 10)
            .unwrap();

    assert_eq!(cursor_pages_followed, 0);
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].title_text, "ｔａｒｇｅｔ");
    assert!(
        page.next_cursor
            .as_deref()
            .is_some_and(|cursor| cursor.starts_with("ssed-title-label:"))
    );
}

#[test]
fn validate_deep_can_explicitly_exercise_expensive_ssed_search_modes() {
    let dir = tempfile::tempdir().unwrap();
    write_ssed_cli_fixture(dir.path());

    let output = validate_package_json(
        &DriverRegistry::default(),
        dir.path(),
        ValidateOptions {
            deep: true,
            include_expensive_search: true,
        },
    );
    let exercises = output["exercises"].as_array().unwrap();
    let partial = exercises
        .iter()
        .find(|exercise| exercise["kind"] == "search_partial")
        .expect("missing partial validation row");
    let fulltext = exercises
        .iter()
        .find(|exercise| exercise["kind"] == "search_full_text")
        .expect("missing fulltext validation row");

    assert_eq!(partial["status"], "ok");
    assert_eq!(fulltext["status"], "ok");
    assert!(!validate_row_has_failure(&output));
}

#[test]
fn home_command_reports_metadata_and_surfaces() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let output = home_command_json(&DriverRegistry::default(), dir.path()).unwrap();

    assert_eq!(output["metadata"]["format_family"], "lved_sqlite3");
    assert_eq!(output["surface_count"].as_u64(), Some(3));
    assert!(
        output["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|surface| surface["surface_id"] == "lved-list"
                && surface["status"] == "available")
    );
    assert!(
        !output["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|surface| surface["surface_id"] == "lved-tree")
    );
}

#[test]
fn search_command_can_render_first_hit_as_basic_text() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let output = search_command_json(
        &DriverRegistry::default(),
        dir.path(),
        "alp".to_owned(),
        SearchMode::Forward,
        10,
        None,
        cli_render_options(CliRenderMode::BasicText, false),
        true,
        0,
        0,
    )
    .unwrap();

    assert!(output["rendered_first"]["display_html"].is_null());
    assert!(
        output["rendered_first"]["basic_text"]
            .as_str()
            .unwrap()
            .contains("body")
    );
}

#[test]
fn library_search_command_uses_all_books_scope_and_routed_rendering() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("FirstDictionary");
    let second = dir.path().join("SecondDictionary");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    write_lved_cli_fixture(&first);
    write_lved_cli_fixture(&second);

    let output = library_search_command_json(
        &DriverRegistry::default(),
        &[dir.path().to_path_buf()],
        None,
        "alp".to_owned(),
        SearchMode::Forward,
        10,
        None,
        RenderOptions::default(),
        true,
        0,
        1,
    )
    .unwrap();

    assert_eq!(output["book_count"].as_u64(), Some(2));
    assert_eq!(output["opened_book_ids"].as_array().unwrap().len(), 2);
    assert!(output["import_diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(output["hits"].as_array().unwrap().len(), 2);
    assert_eq!(output["rendered_first"]["view"]["kind"], "entry_body");
    assert!(output["rendered_first"]["book_id"].as_str().is_some());
    assert!(output["search_result_sequence"].as_str().is_some());
    assert_eq!(
        output["sequence_hint"]["kind"].as_str(),
        Some("search_results")
    );
    let window = &output["target_window"];
    assert_eq!(window["center"]["view"]["kind"], "entry_body");
    assert_eq!(window["before"].as_array().unwrap().len(), 0);
    assert_eq!(window["after"].as_array().unwrap().len(), 1);
    assert_ne!(
        window["center"]["book_id"].as_str(),
        window["after"][0]["book_id"].as_str()
    );
    assert!(
        window["after"][0]["view"]["display_html"]
            .as_str()
            .unwrap()
            .contains("body")
    );
}

#[test]
fn search_command_uses_backend_search_result_sequence_for_windows() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let output = search_command_json(
        &DriverRegistry::default(),
        dir.path(),
        "Example Dictionary".to_owned(),
        SearchMode::Forward,
        10,
        None,
        RenderOptions::default(),
        false,
        0,
        1,
    )
    .unwrap();

    assert!(output["search_result_sequence"].as_str().is_some());
    assert_eq!(
        output["sequence_hint"]["kind"].as_str(),
        Some("search_results")
    );
    assert_eq!(output["target_window"]["center"]["title"], "alpha");
    assert_eq!(
        output["target_window"]["after"].as_array().unwrap().len(),
        1
    );
    assert_eq!(output["target_window"]["after"][0]["title"], "beta");
    assert!(output["target_window"]["diagnostics"].is_null());
}

#[test]
fn search_command_does_not_emit_fake_sequence_for_empty_pages() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let output = search_command_json(
        &DriverRegistry::default(),
        dir.path(),
        "definitely-missing".to_owned(),
        SearchMode::Forward,
        10,
        None,
        RenderOptions::default(),
        false,
        0,
        1,
    )
    .unwrap();

    assert!(output["hits"].as_array().unwrap().is_empty());
    assert!(output["search_result_sequence"].is_null());
    assert!(output["sequence_hint"].is_null());
    assert!(output["target_window"].is_null());
}

#[test]
fn library_import_command_returns_cacheable_book_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("FirstDictionary");
    let second = dir.path().join("SecondDictionary");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    write_lved_cli_fixture(&first);
    write_lved_cli_fixture(&second);

    let output = library_import_command_json(
        &DriverRegistry::default(),
        &[dir.path().to_path_buf()],
        None,
    );
    let output = serde_json::to_value(output).unwrap();

    assert_eq!(output["book_count"].as_u64(), Some(2));
    assert_eq!(output["opened_book_ids"].as_array().unwrap().len(), 2);
    assert_eq!(output["books"].as_array().unwrap().len(), 2);
    assert!(output["import_diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(
        output["books"][0]["format_label"].as_str(),
        Some("LVED_SQLITE3")
    );
    let package_root = output["books"][0]["package_root"].as_str().unwrap();
    assert!(
        package_root.ends_with("FirstDictionary") || package_root.ends_with("SecondDictionary")
    );
}

#[test]
fn resource_command_resolves_rendered_resource_tokens() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let search_output = search_command_json(
        &DriverRegistry::default(),
        dir.path(),
        "alp".to_owned(),
        SearchMode::Forward,
        10,
        None,
        RenderOptions::default(),
        true,
        0,
        0,
    )
    .unwrap();
    let token = search_output["rendered_first"]["resources"][0]["token"]
        .as_str()
        .unwrap()
        .to_owned();

    let resource_output =
        resource_command_json(&DriverRegistry::default(), dir.path(), token).unwrap();

    assert_eq!(resource_output["byte_len"].as_u64(), Some(6));
    assert_eq!(resource_output["resource"]["kind"], "image");
    assert_eq!(resource_output["resource"]["mime_type"], "image/svg+xml");
    assert!(has_scoped_resource_href(
        resource_output["resource"]["href"].as_str().unwrap()
    ));

    let href = search_output["rendered_first"]["resources"][0]["href"]
        .as_str()
        .unwrap()
        .to_owned();
    let href_output = resource_command_json(&DriverRegistry::default(), dir.path(), href).unwrap();
    assert_eq!(href_output["byte_len"].as_u64(), Some(6));
    assert_eq!(href_output["resource"]["kind"], "image");
}

#[test]
fn window_command_resolves_continuous_view_for_target_tokens() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let search_output = search_command_json(
        &DriverRegistry::default(),
        dir.path(),
        "alp".to_owned(),
        SearchMode::Forward,
        10,
        None,
        RenderOptions::default(),
        false,
        0,
        0,
    )
    .unwrap();
    let target = search_output["hits"][0]["target"]
        .as_str()
        .unwrap()
        .to_owned();

    let output = window_command_json(
        &DriverRegistry::default(),
        dir.path(),
        target,
        Some(SequenceHint::LvedListOrder),
        0,
        1,
        RenderOptions::default(),
    )
    .unwrap();

    assert_eq!(output["window"]["center"]["title"], "alpha");
    assert_eq!(output["window"]["after"].as_array().unwrap().len(), 1);
    assert!(
        output["window"]["after"][0]["display_html"]
            .as_str()
            .unwrap()
            .contains("next body")
    );
    assert_eq!(
        output["sequence_hint"],
        serde_json::json!({ "kind": "lved_list_order" })
    );

    let rendered = search_command_json(
        &DriverRegistry::default(),
        dir.path(),
        "alp".to_owned(),
        SearchMode::Forward,
        10,
        None,
        RenderOptions::default(),
        true,
        0,
        0,
    )
    .unwrap();
    let href = first_target_href(rendered["rendered_first"]["display_html"].as_str().unwrap());

    let href_output = window_command_json(
        &DriverRegistry::default(),
        dir.path(),
        href,
        Some(SequenceHint::LvedListOrder),
        1,
        0,
        RenderOptions::default(),
    )
    .unwrap();
    assert!(
        href_output["window"]["center"]["display_html"]
            .as_str()
            .unwrap()
            .contains("next body")
    );
    assert_eq!(href_output["window"]["before"].as_array().unwrap().len(), 1);
    assert_eq!(
        href_output["routing_diagnostics"].as_array().unwrap().len(),
        0
    );
}

#[test]
fn render_command_resolves_target_hrefs_from_display_html() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let search_output = search_command_json(
        &DriverRegistry::default(),
        dir.path(),
        "alp".to_owned(),
        SearchMode::Forward,
        10,
        None,
        RenderOptions::default(),
        true,
        0,
        0,
    )
    .unwrap();
    let href = first_target_href(
        search_output["rendered_first"]["display_html"]
            .as_str()
            .unwrap(),
    );
    let output = render_command_json(
        &DriverRegistry::default(),
        dir.path(),
        href,
        RenderOptions::default(),
    )
    .unwrap();

    assert!(
        output["view"]["display_html"]
            .as_str()
            .unwrap()
            .contains("next body")
    );
    assert_eq!(output["routing_diagnostics"].as_array().unwrap().len(), 0);
}

fn has_scoped_resource_href(html: &str) -> bool {
    const PREFIX: &str = "lvcore://resource/";
    let Some(start) = html.find(PREFIX) else {
        return false;
    };
    let rest = &html[start + PREFIX.len()..];
    let value = rest
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '>'))
        .next()
        .unwrap_or_default();
    value.split('/').count() == 2
}

fn first_target_href(html: &str) -> String {
    const PREFIX: &str = "lvcore://target/";
    let start = html
        .find(PREFIX)
        .expect("rendered HTML should contain a target href");
    html[start..]
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '>'))
        .next()
        .unwrap()
        .to_owned()
}

fn write_lved_cli_fixture(root: &Path) {
    let payload = root.join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        apply_sqlcipher_key(&connection, key).unwrap();
        connection
            .execute_batch(
                r#"
                create table info (id integer, type integer, name text primary key, body text, media text);
                insert into info values (1, 1, 'about.html', '<h1>Example Dictionary</h1>', '');
                create table content (id integer primary key, type integer, body text, media text);
                create table media (id integer primary key, name text, type integer, main blob);
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
                insert into content values (100, 1, '<article><object data="AC6E.svg"></object><p>body</p><a href="lved.dataid.result:101#jump">beta</a></article>', '');
                insert into content values (101, 1, '<article><p>next body</p></article>', '');
                insert into media values (1, 'AC6E', 4, X'3C7376672F3E');
                insert into list values (1, 100, 1, '', '<img src="AC6E.svg"><b>alpha</b>', '');
                insert into list values (2, 101, 1, '', '<b>beta</b>', '');
                insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                  values (1, 'Example Dictionary alpha', 'ahpla', 'Example Dictionary alpha', 'Example Dictionary alpha body', '', '', '∥alpha∥');
                insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                  values (2, 'Example Dictionary beta', 'ateb', 'Example Dictionary beta', 'Example Dictionary beta body', '', '', '∥beta∥');
                "#,
            )
            .unwrap();
    }
    fs::write(root.join("main.key"), key).unwrap();
}

fn write_ssed_cli_fixture(root: &Path) {
    fs::write(root.join("DICT.IDX"), ssedinfo_cli_fixture()).unwrap();
    fs::write(
        root.join("HONMON.DIC"),
        sseddata_literal_fixture(&body_jis("alpha body")),
    )
    .unwrap();
    fs::write(
        root.join("FHTITLE.DIC"),
        sseddata_literal_fixture(&body_jis("alpha")),
    )
    .unwrap();
    fs::write(
        root.join("FHINDEX.DIC"),
        sseddata_literal_fixture(&simple_index_page("alpha", 1, 0, 13, 0)),
    )
    .unwrap();
}

fn write_ssed_visible_title_cursor_cli_fixture(root: &Path) {
    fs::write(
        root.join("DICT.IDX"),
        ssedinfo_cli_fixture_with_index_blocks(4),
    )
    .unwrap();
    fs::write(
        root.join("HONMON.DIC"),
        sseddata_literal_fixture(&body_jis("body")),
    )
    .unwrap();
    let mut titles = body_jis("alpha");
    titles.extend_from_slice(&[0x1f, 0x0a]);
    let target_title_offset = titles.len() as u16;
    titles.extend_from_slice(&body_jis("target"));
    titles.extend_from_slice(&[0x1f, 0x0a]);
    fs::write(root.join("FHTITLE.DIC"), sseddata_literal_fixture(&titles)).unwrap();

    let mut index = Vec::new();
    index.extend_from_slice(&internal_index_page(&[("a", 16), ("b", 17), ("c", 18)]));
    let mut body_offset = 1u16;
    for page_index in 0..3 {
        let mut rows = Vec::new();
        for row_index in 0..100 {
            let title_offset = if page_index == 2 && row_index == 70 {
                target_title_offset
            } else {
                0
            };
            rows.push(("x", 1, body_offset, 13, title_offset));
            body_offset = body_offset.saturating_add(1);
        }
        index.extend_from_slice(&simple_index_rows_page(&rows));
    }
    fs::write(root.join("FHINDEX.DIC"), sseddata_literal_fixture(&index)).unwrap();
}

fn ssedinfo_cli_fixture() -> Vec<u8> {
    ssedinfo_cli_fixture_with_index_blocks(1)
}

fn ssedinfo_cli_fixture_with_index_blocks(index_blocks: u32) -> Vec<u8> {
    let record_start = 0x80;
    let mut data = vec![0u8; record_start + 4 * 0x30];
    data[..8].copy_from_slice(lvcore::SSEDINFO_MAGIC);
    let title = b"SSED Fixture";
    data[0x0c] = title.len() as u8;
    data[0x0d..0x0d + title.len()].copy_from_slice(title);
    data[0x4d] = 4;
    write_ssedinfo_record(
        &mut data[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        "HONMON.DIC",
    );
    write_ssedinfo_record(
        &mut data[record_start + 0x30..record_start + 0x60],
        0x05,
        13,
        14,
        "FHTITLE.DIC",
    );
    write_ssedinfo_record(
        &mut data[record_start + 0x60..record_start + 0x90],
        0x91,
        15,
        15 + index_blocks.saturating_sub(1),
        "FHINDEX.DIC",
    );
    write_ssedinfo_record(
        &mut data[record_start + 0x90..record_start + 0xc0],
        0xf2,
        15 + index_blocks.saturating_add(1),
        15 + index_blocks.saturating_add(2),
        "GA16HALF",
    );
    data
}

fn write_ssedinfo_record(rec: &mut [u8], component_type: u8, start: u32, end: u32, filename: &str) {
    rec[3] = component_type;
    rec[4..8].copy_from_slice(&start.to_be_bytes());
    rec[8..12].copy_from_slice(&end.to_be_bytes());
    rec[0x10] = filename.len() as u8;
    rec[0x11..0x11 + filename.len()].copy_from_slice(filename.as_bytes());
}

fn simple_index_page(
    key: &str,
    body_block: u32,
    body_offset: u16,
    title_block: u32,
    title_offset: u16,
) -> Vec<u8> {
    let mut page = vec![0u8; 2048];
    let key = jis_fullwidth_ascii_key(key);
    page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    page[2..4].copy_from_slice(&1u16.to_be_bytes());
    page[4] = key.len() as u8;
    page[5..5 + key.len()].copy_from_slice(&key);
    let pos = 5 + key.len();
    page[pos..pos + 4].copy_from_slice(&body_block.to_be_bytes());
    page[pos + 4..pos + 6].copy_from_slice(&body_offset.to_be_bytes());
    page[pos + 6..pos + 10].copy_from_slice(&title_block.to_be_bytes());
    page[pos + 10..pos + 12].copy_from_slice(&title_offset.to_be_bytes());
    page
}

fn simple_index_rows_page(rows: &[(&str, u32, u16, u32, u16)]) -> Vec<u8> {
    let mut page = vec![0u8; 2048];
    page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    page[2..4].copy_from_slice(&(rows.len() as u16).to_be_bytes());
    let mut pos = 4usize;
    for (key, body_block, body_offset, title_block, title_offset) in rows {
        let key = jis_fullwidth_ascii_key(key);
        page[pos] = key.len() as u8;
        pos += 1;
        page[pos..pos + key.len()].copy_from_slice(&key);
        pos += key.len();
        page[pos..pos + 4].copy_from_slice(&body_block.to_be_bytes());
        page[pos + 4..pos + 6].copy_from_slice(&body_offset.to_be_bytes());
        page[pos + 6..pos + 10].copy_from_slice(&title_block.to_be_bytes());
        page[pos + 10..pos + 12].copy_from_slice(&title_offset.to_be_bytes());
        pos += 12;
    }
    page
}

fn internal_index_page(rows: &[(&str, u32)]) -> Vec<u8> {
    let mut page = vec![0u8; 2048];
    page[0..2].copy_from_slice(&2u16.to_be_bytes());
    page[2..4].copy_from_slice(&(rows.len() as u16).to_be_bytes());
    let mut pos = 4usize;
    for (key, child_block) in rows {
        let key = jis_fullwidth_ascii_key(key);
        page[pos..pos + 2].copy_from_slice(&key[..2]);
        pos += 2;
        page[pos..pos + 4].copy_from_slice(&child_block.to_be_bytes());
        pos += 4;
    }
    page
}

fn sseddata_literal_fixture(literals: &[u8]) -> Vec<u8> {
    let chunk_offset = 0x44usize;
    let block_count = literals.len().div_ceil(2048).max(1);
    let mut data = vec![0u8; chunk_offset];
    data[..8].copy_from_slice(lvcore::SSEDDATA_MAGIC);
    data[0x0f] = 1;
    data[0x16..0x18].copy_from_slice(&1u16.to_be_bytes());
    data[0x18..0x1c].copy_from_slice(&1u32.to_be_bytes());
    data[0x1c..0x20].copy_from_slice(&(block_count as u32).to_be_bytes());
    data[0x40..0x44].copy_from_slice(&(chunk_offset as u32).to_be_bytes());
    data.extend_from_slice(&[0, 0]);
    data.extend_from_slice(&(literals.len() as u16).to_be_bytes());
    data.push(0);
    for literal in literals {
        data.extend_from_slice(&[0, 0, *literal]);
    }
    data
}

fn body_jis(text: &str) -> Vec<u8> {
    jis_fullwidth_ascii_key(text)
}

fn jis_fullwidth_ascii_key(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for byte in text.bytes() {
        if (0x21..=0x7e).contains(&byte) {
            out.extend_from_slice(&[0x23, byte]);
        } else if byte == b' ' {
            out.extend_from_slice(&[0x21, 0x21]);
        } else {
            out.push(byte);
        }
    }
    out
}

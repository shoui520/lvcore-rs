use super::*;
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

    let output = validate_package_json(&DriverRegistry::default(), dir.path(), false);

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
fn validate_deep_exercises_first_rendered_resource() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let output = validate_package_json(&DriverRegistry::default(), dir.path(), true);
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

    let output = validate_package_json(&DriverRegistry::default(), dir.path(), true);
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

    let output = validate_package_json(&DriverRegistry::default(), dir.path(), true);
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
fn home_command_reports_metadata_and_surfaces() {
    let dir = tempfile::tempdir().unwrap();
    write_lved_cli_fixture(dir.path());

    let output = home_command_json(&DriverRegistry::default(), dir.path()).unwrap();

    assert_eq!(output["metadata"]["format_family"], "lved_sqlite3");
    assert_eq!(output["surface_count"].as_u64(), Some(4));
    assert!(
        output["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|surface| surface["surface_id"] == "lved-list"
                && surface["status"] == "available")
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
    )
    .unwrap();

    assert_eq!(output["book_count"].as_u64(), Some(2));
    assert_eq!(output["opened_book_ids"].as_array().unwrap().len(), 2);
    assert!(output["import_diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(output["hits"].as_array().unwrap().len(), 2);
    assert_eq!(output["rendered_first"]["view"]["kind"], "entry_body");
    assert!(output["rendered_first"]["book_id"].as_str().is_some());
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
                insert into content values (100, 1, '<article><object data="AC6E.svg"></object><p>body</p></article>', '');
                insert into content values (101, 1, '<article><p>next body</p></article>', '');
                insert into media values (1, 'AC6E', 4, X'3C7376672F3E');
                insert into list values (1, 100, 1, '', '<img src="AC6E.svg"><b>alpha</b>', '');
                insert into list values (2, 101, 1, '', '<b>beta</b>', '');
                insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                  values (1, 'alpha', 'ahpla', 'alpha', 'alpha body', '', '', '∥alpha∥');
                "#,
            )
            .unwrap();
    }
    fs::write(root.join("main.key"), key).unwrap();
}

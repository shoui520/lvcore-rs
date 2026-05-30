use super::*;
use std::io::Write;

#[test]
fn parses_nested_menu_data_items() {
    let items = parse_menu_data(
        r#"<?xml version="1.0" encoding="UTF-8"?>
        <list>
          <item label="Book" href="">
            <item label="凡例">
              <item label="まえがき" href="000001" anchor="top"></item>
            </item>
            <item label="五十音順法令一覧" href="50on"></item>
          </item>
        </list>"#,
    )
    .unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "Book");
    assert_eq!(items[0].href, None);
    assert_eq!(items[0].children[0].label, "凡例");
    assert_eq!(
        items[0].children[0].children[0].href.as_deref(),
        Some("000001")
    );
    assert_eq!(
        items[0].children[0].children[0].anchor.as_deref(),
        Some("top")
    );
    assert_eq!(items[0].children[1].href.as_deref(), Some("50on"));
}

#[test]
fn rejects_unbalanced_menu_data_items() {
    let error = parse_menu_data("<list><item label=\"broken\"></list>").unwrap_err();
    assert!(
        error.to_string().contains("XML parse error") || error.to_string().contains("unclosed")
    );
}

#[test]
fn multiview_payload_role_hints_skip_impossible_payloads() {
    assert_eq!(
        hinted_payload_role("blvdat", 1),
        Some(MultiviewPayloadRole::ContentSearchBody)
    );
    assert_eq!(
        hinted_payload_role("blvbat", 5),
        Some(MultiviewPayloadRole::LawBody)
    );
    assert_eq!(
        hinted_payload_role("hlvbat", 5),
        Some(MultiviewPayloadRole::CaseDigestBody)
    );
    assert_eq!(
        hinted_payload_role("ilvbat", 5),
        Some(MultiviewPayloadRole::HtmlIndex)
    );
    assert!(!payload_may_have_role(
        "blvbat",
        5,
        MultiviewPayloadRole::ContentSearchBody
    ));
    assert!(payload_may_have_role(
        "blvdat",
        1,
        MultiviewPayloadRole::ContentSearchBody
    ));
}

#[test]
fn payload_header_probe_reads_only_requested_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blvbat");
    let mut file = fs::File::create(&path).unwrap();
    file.write_all(b"SQLite format 3\0payload").unwrap();
    file.set_len(128 * 1024 * 1024).unwrap();

    let prefix = read_file_prefix(&path, 16).unwrap();
    assert_eq!(prefix, b"SQLite format 3\0");
}

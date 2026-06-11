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
fn parses_legacy_menu_xml_name_ref_and_menu_nodes() {
    let items = parse_menu_data(
        r#"<list>
          <group category="genre">
            <item ref="A010" name="日本国憲法" />
            <menu ref="A010_ZEN" name="前文" />
            <menu ref="A010_HON-sy1" name="第一章　天皇">
              <menu ref="A010_HON-sy1-jo1" name="第一条" />
            </menu>
            <item ref="none" name="見出しなし" />
          </group>
        </list>"#,
    )
    .unwrap();

    assert_eq!(items.len(), 4);
    assert_eq!(items[0].label, "日本国憲法");
    assert_eq!(items[0].href.as_deref(), Some("A010"));
    assert_eq!(items[1].label, "前文");
    assert_eq!(items[1].href.as_deref(), Some("A010_ZEN"));
    assert_eq!(items[2].children[0].label, "第一条");
    assert_eq!(items[3].href, None);
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

#[test]
fn logofont_cipher_payload_cache_path_is_stable_and_content_versioned() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blvbat");
    fs::write(&path, b"first payload").unwrap();

    let first = decrypted_multiview_cache_path(&path).unwrap();
    let first_again = decrypted_multiview_cache_path(&path).unwrap();
    assert_eq!(first, first_again);
    assert!(
        first
            .to_string_lossy()
            .contains("lvcore-rs/multiview-payloads")
    );

    std::thread::sleep(std::time::Duration::from_millis(2));
    fs::write(&path, b"second payload with different size").unwrap();
    let second = decrypted_multiview_cache_path(&path).unwrap();
    assert_ne!(first, second);
}

#[test]
fn law_multiview_search_uses_hore_metadata_when_search_body_table_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    let metadata_path = dir.path().join("nlvdat");
    let connection = Connection::open(&metadata_path).unwrap();
    connection
        .execute_batch(
            r#"
            create table t_hore (
                f_hore_code text primary key,
                f_name text,
                f_name_sub text,
                f_name_kana text,
                f_kana_order integer,
                f_abbr1 text,
                f_abbr1_kana text,
                f_nickname text,
                f_commonname text,
                f_commonname_kana text,
                f_commonname_ex text,
                f_abbr_user text,
                f_abbr_user_kana text,
                f_temp_kana text
            );
            insert into t_hore values
                ('M110', '民法', '民法', 'みんぽう', 100, '民', 'みん', '', '', '', '', '', '', ''),
                ('T010', '相続税法', '相続税法', 'そうぞくぜいほう', 200, '相続税', 'そうぞくぜい', '', '', '', '', '', '', '');
            "#,
        )
        .unwrap();
    drop(connection);

    let store = MultiviewStore::discover(dir.path()).unwrap().unwrap();

    let forward_hits = store
        .search_page("民法", &SearchMode::Forward, 0, 10)
        .unwrap();
    assert_eq!(forward_hits[0].href, "M110");
    assert_eq!(forward_hits[0].title_text, "民法");

    let exact_hits = store
        .search_page("民法", &SearchMode::Exact, 0, 10)
        .unwrap();
    assert_eq!(exact_hits.len(), 1);
    assert_eq!(exact_hits[0].href, "M110");

    let fulltext_name_hits = store
        .search_page("相続", &SearchMode::FullText, 0, 10)
        .unwrap();
    assert_eq!(fulltext_name_hits[0].href, "T010");
    assert_eq!(fulltext_name_hits[0].title_text, "相続税法");
}

#[test]
fn law_multiview_fulltext_search_uses_law_body_tables() {
    let dir = tempfile::tempdir().unwrap();
    let metadata_path = dir.path().join("nlvdat");
    let metadata = Connection::open(&metadata_path).unwrap();
    metadata
        .execute_batch(
            r#"
            create table t_hore (
                f_hore_code text primary key,
                f_name text,
                f_name_sub text,
                f_name_kana text,
                f_kana_order integer
            );
            insert into t_hore values
                ('M110', '民法', '民法', 'みんぽう', 100);
            "#,
        )
        .unwrap();
    drop(metadata);

    let body_path = dir.path().join("blvbat");
    let body = Connection::open(&body_path).unwrap();
    body.execute_batch(
        r#"
        create table t_M110 (
            f_hore_code text,
            f_hore_id integer,
            f_rec_id integer,
            f_rec_type integer,
            f_title_no text,
            f_title_sub text,
            f_anchor text,
            f_text text,
            f_text_plane text
        );
        insert into t_M110 values
            ('M110', 1, 10000, 0, '第一条', '基本原則', 'M110_HON-j1',
             '<p>私権は、公共の福祉に適合しなければならない。</p>',
             '私権は、公共の福祉に適合しなければならない。'),
            ('M110', 1, 20000, 0, '第一三条', '保佐人の同意を要する行為等', 'M110_HON-j13',
             '<p>保証及び相続の承認に関する条文。</p>',
             '保証及び相続の承認に関する条文。');
        "#,
    )
    .unwrap();
    drop(body);

    let store = MultiviewStore::discover(dir.path()).unwrap().unwrap();
    let name_hits = store
        .search_page("民法", &SearchMode::Forward, 0, 10)
        .unwrap();
    assert_eq!(name_hits.len(), 1);
    assert_eq!(name_hits[0].href, "M110");
    assert_eq!(name_hits[0].title_text, "民法");

    let body_hits = store
        .search_page("保証", &SearchMode::FullText, 0, 10)
        .unwrap();
    assert_eq!(body_hits.len(), 1);
    assert_eq!(body_hits[0].href, "M110_HON-j13");
    assert_eq!(
        body_hits[0].title_text,
        "第一三条 保佐人の同意を要する行為等"
    );
    assert!(
        body_hits[0]
            .snippet_html
            .as_deref()
            .unwrap()
            .contains("保証")
    );

    let body = store.body_for_href(&body_hits[0].href).unwrap().unwrap();
    assert_eq!(body.title, "第一三条 保佐人の同意を要する行為等");
    assert!(body.html.contains("保証及び相続"));
}

#[test]
fn multiview_index_prefixed_links_can_resolve_law_body_anchors() {
    let dir = tempfile::tempdir().unwrap();
    let body_path = dir.path().join("blvbat");
    let body = Connection::open(&body_path).unwrap();
    body.execute_batch(
        r#"
        create table t_B250 (
            f_hore_code text,
            f_hore_id integer,
            f_rec_id integer,
            f_rec_type integer,
            f_title_no text,
            f_title_sub text,
            f_anchor text,
            f_text text,
            f_text_plane text
        );
        insert into t_B250 values
            ('B250', 1, 20000, 0, '第二条', '定義', 'B250_HON-j2',
             '<p>所得税法の定義。</p>',
             '所得税法の定義。');
        "#,
    )
    .unwrap();
    drop(body);

    let store = MultiviewStore::discover(dir.path()).unwrap().unwrap();
    let resolved = store.body_for_href("index:B250_HON-j2").unwrap().unwrap();

    assert_eq!(resolved.title, "第二条 定義");
    assert!(resolved.html.contains("所得税法の定義"));
    assert_eq!(resolved.source, "blvbat:t_B250");
}

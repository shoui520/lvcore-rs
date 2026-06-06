use super::*;
use tempfile::tempdir;

#[cfg(unix)]
#[test]
fn tree_index_candidates_skip_symlinked_res_directory_escape() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("tree.idx"), b"100\t0\tOutside\n").unwrap();
    symlink(outside.path(), dir.path().join("res")).unwrap();

    let paths = super::tree::lved_tree_index_candidate_paths(dir.path()).unwrap();

    assert!(paths.is_empty());
}

#[test]
fn discovers_dict_code_key_file_for_main_data() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("main.data"), b"encrypted").unwrap();
    fs::write(dir.path().join("TEST.key"), b"secret").unwrap();
    let payload = dir.path().join("main.data");

    let key = discover_lved_key_file(&payload).unwrap().unwrap();

    assert_eq!(key.path.file_name().unwrap(), "TEST.key");
    assert_eq!(read_lved_key_file(&key.path).unwrap(), "secret");
}

#[test]
fn generic_main_dbc_uses_package_folder_as_dict_code() {
    let dir = tempdir().unwrap();
    let package = dir.path().join("_DCT_OXFPEU4");
    fs::create_dir(&package).unwrap();
    fs::write(package.join("main.dbc"), b"encrypted").unwrap();
    fs::write(package.join("OXFPEU4.key"), b"secret").unwrap();
    let payload = package.join("main.dbc");

    assert_eq!(infer_lved_dict_code(&payload).as_deref(), Some("OXFPEU4"));
    let key = discover_lved_key_file(&payload).unwrap().unwrap();

    assert_eq!(key.path.file_name().unwrap(), "OXFPEU4.key");
    assert_eq!(read_lved_key_file(&key.path).unwrap(), "secret");
}

#[test]
fn discovers_android_lved_payload_and_uses_dictinfo_key() {
    let dir = tempdir().unwrap();
    let package = dir.path().join("SQLite/.TESTDICT");
    fs::create_dir_all(package.join("resource")).unwrap();
    fs::write(package.join("resource/conf.ini"), b"").unwrap();
    fs::create_dir_all(dir.path().join("android viewer/res/xml")).unwrap();
    fs::write(
        dir.path().join("android viewer/res/xml/dictinfo.xml"),
        r#"
            <dictinfo>
              <dict id="750" name="TESTDICT">
                <code>TESTDICT</code>
                <title>Android&#x20;&amp;&#x20;Test Dictionary</title>
                <fonts use="1"><font>ipamp</font></fonts>
              </dict>
            </dictinfo>
            "#,
    )
    .unwrap();
    let payload = package.join("TESTDICT.db");
    let key = derive_android_lved_sqlcipher_key(750, "TESTDICT");
    {
        let connection = Connection::open(&payload).unwrap();
        apply_sqlcipher_key(&connection, &key).unwrap();
        connection
                .execute_batch(
                    "
                    create table info (id integer, type integer, name text primary key, body text, media text);
                    insert into info values (1, 1, 'about.html', '<h1>Wrong fallback title</h1>', '');
                    create table content (id integer primary key, type integer, body text, media text);
                    create table list (id integer primary key, refid integer, type integer, anchor text, title text, titlesub text);
                    create virtual table search using fts4(forward, back, part, fts, advanced1, advanced2, filter);
                    insert into content values (100, 1, '<article>body</article>', '');
                    insert into list values (1, 100, 1, '', '<b>alpha</b>', '');
                    insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                      values (1, 'alpha', 'ahpla', 'alpha', 'alpha body', '', '', '∥alpha∥');
                    ",
                )
                .unwrap();
    }

    assert!(is_lved_payload_name(&payload));
    let store = LvedSqliteStore::discover(&package).unwrap().unwrap();
    assert!(store.key_file.is_none());
    assert_eq!(
        store.android_info.as_ref().map(|info| info.dict_id),
        Some(750)
    );
    assert_eq!(
        store.title().unwrap().as_deref(),
        Some("Android & Test Dictionary")
    );
    assert_eq!(
        store.search("alp", &SearchMode::Forward, 10).unwrap()[0].title_text,
        "alpha"
    );
}

#[test]
fn discovers_ios_lved_dbc_payload_and_uses_dictlist_key() {
    let dir = tempdir().unwrap();
    let package = dir.path().join("OXFPEU4");
    fs::create_dir_all(&package).unwrap();
    fs::write(
        dir.path().join("DictList.plist"),
        br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>ItemArray</key><array><dict>
    <key>DictFolder</key><string>OXFPEU4</string>
    <key>DictName</key><string>Oxford Test Dictionary</string>
    <key>DictFtsDB</key><string>OXFPEU4/OXFPEU4.dbc</string>
  </dict></array>
</dict></plist>"#,
    )
    .unwrap();
    let payload = package.join("OXFPEU4.dbc");
    let key = derive_android_lved_sqlcipher_key(750, "OXFPEU4");
    {
        let connection = Connection::open(&payload).unwrap();
        apply_sqlcipher_key(&connection, &key).unwrap();
        connection
            .execute_batch(
                "
                create table info (id integer, type integer, name text primary key, body text, media text);
                insert into info values (1, 1, 'about.html', '<h1>Wrong fallback title</h1>', '');
                create table content (id integer primary key, type integer, body text, media text);
                create table list (id integer primary key, refid integer, type integer, anchor text, title text, titlesub text);
                create virtual table search using fts4(forward, back, part, fts, filter);
                insert into content values (100, 1, '<article>body</article>', '');
                insert into list values (1, 100, 1, '', '<b>alpha</b>', '');
                insert into search(rowid, forward, back, part, fts, filter)
                  values (1, 'alpha', 'ahpla', 'alpha', 'alpha body', '∥alpha∥');
                ",
            )
            .unwrap();
    }

    let store = LvedSqliteStore::discover(&package).unwrap().unwrap();

    assert!(store.key_file.is_none());
    assert_eq!(
        store.android_info.as_ref().map(|info| info.dict_id),
        Some(750)
    );
    assert_eq!(
        store.title().unwrap().as_deref(),
        Some("Oxford Test Dictionary")
    );
    assert_eq!(
        store.search("alp", &SearchMode::Forward, 10).unwrap()[0].title_text,
        "alpha"
    );
}

#[test]
fn android_lved_key_derivation_uses_wrapping_id_arithmetic() {
    let key = derive_android_lved_sqlcipher_key(i64::MAX, "TESTDICT");
    let expected_id = i64::MAX.wrapping_mul(19286).to_string();

    assert!(key.ends_with(&expected_id));
}

#[test]
fn android_lved_payload_detection_rejects_plaintext_helper_db() {
    let dir = tempdir().unwrap();
    let package = dir.path().join(".HELPER");
    fs::create_dir_all(package.join("resource")).unwrap();
    fs::write(package.join("resource/conf.ini"), b"").unwrap();
    let payload = package.join("HELPER.db");
    Connection::open(&payload).unwrap();

    assert!(!is_lved_payload_name(&payload));
}

#[test]
fn opens_sqlcipher_payload_and_extracts_title() {
    let dir = tempdir().unwrap();
    let payload = dir.path().join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        apply_sqlcipher_key(&connection, key).unwrap();
        connection
                .execute_batch(
                    "
                    create table info (id integer, type integer, name text primary key, body text, media text);
                    insert into info values (1, 1, 'about.html', '<h1>Example Dictionary 第2版</h1>', '');
                    ",
                )
                .unwrap();
    }
    fs::write(dir.path().join("main.key"), key).unwrap();

    let store = LvedSqliteStore::discover(dir.path()).unwrap().unwrap();

    assert_eq!(store.table_names().unwrap(), vec!["info".to_owned()]);
    assert_eq!(
        store.title().unwrap().as_deref(),
        Some("Example Dictionary 第2版")
    );
}

#[test]
fn explicit_key_discovery_skips_android_dictinfo_metadata() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("main.data"), b"encrypted").unwrap();
    fs::write(dir.path().join("main.key"), b"explicit-key").unwrap();
    fs::create_dir_all(dir.path().join("android viewer/res/xml")).unwrap();
    fs::write(
        dir.path().join("android viewer/res/xml/dictinfo.xml"),
        r#"
            <dictinfo>
              <dict id="750" name="TESTDICT">
                <code>TESTDICT</code>
                <title>Android metadata should not be loaded</title>
              </dict>
            </dictinfo>
            "#,
    )
    .unwrap();

    let store = LvedSqliteStore::discover(dir.path()).unwrap().unwrap();

    assert!(store.key_file.is_some());
    assert!(store.android_info.is_none());
}

#[test]
fn tree_index_available_validates_shape_without_loading_tree_rows() {
    let dir = tempdir().unwrap();
    let payload = dir.path().join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        apply_sqlcipher_key(&connection, key).unwrap();
        connection
            .execute_batch(
                "
                create table info (id integer, type integer, name text primary key, body text, media text);
                insert into info values (1, 1, 'about.html', '<h1>Example Dictionary 第2版</h1>', '');
                create table content (id integer primary key, type integer, body text, media text);
                create table list (id integer primary key, refid integer, type integer, anchor text, title text, titlesub text);
                create virtual table search using fts4(forward, back, part, fts, filter);
                ",
            )
            .unwrap();
    }
    fs::write(dir.path().join("main.key"), key).unwrap();
    fs::write(dir.path().join("tree.idx"), b"not a lved tree index\n").unwrap();
    let store = LvedSqliteStore::discover(dir.path()).unwrap().unwrap();

    assert!(!store.summary().unwrap().tree_available);

    fs::write(
        dir.path().join("tree.idx"),
        b"100\t0\tRoot\n101\t1\tChild\n",
    )
    .unwrap();

    assert!(store.summary().unwrap().tree_available);
}

#[test]
fn searches_lved_list_rows_and_preserves_content_html() {
    let dir = tempdir().unwrap();
    let payload = dir.path().join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        apply_sqlcipher_key(&connection, key).unwrap();
        connection
                .execute_batch(
                    "
                    create table info (id integer, type integer, name text primary key, body text, media text);
                    create table content (id integer primary key, type integer, body text, media text);
                    create table mediasub (id integer primary key, name text, type integer, main blob);
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
                    insert into info values (1, 1, 'about.html', '<h1>Example Dictionary 第2版</h1>', '');
                    insert into content values (100, 1, '<article><h1>Alpha</h1><p>body</p></article>', '');
                    insert into content values (101, 1, '<article><h1>Beta</h1></article>', '');
                    insert into content values (102, 1, '<article><h1>Gamma</h1></article>', '');
                    insert into content values (103, 1, '<article><h1>Alpha Beta</h1></article>', '');
                    insert into mediasub values (1, '00010033', 5, X'49443303');
                    insert into list values (1, 100, 1, 'body-anchor', '<b>alpha</b>', '<span>subtitle</span>');
                    insert into list values (2, 101, 1, '', '<b>beta</b>', '');
                    insert into list values (3, 102, 1, '', '<b>gamma</b>', '');
                    insert into list values (4, 103, 1, '', '<b>alpha beta</b>', '');
                    insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                      values (1, 'alpha', 'ahpla', 'alpha', 'alpha body', 'topic marker', '', '∥alpha∥');
                    insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                      values (2, 'a, a', 'a ,a', 'a, a', 'letter article', '', '', '∥a, a∥');
                    insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                      values (3, '(gamma)', ')ammag(', '(gamma)', 'gamma article', '', '', '∥(gamma)∥');
                    insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                      values (4, 'alpha beta', 'ateb ahpla', 'alpha beta', 'alpha beta article', '', '', '∥alpha beta∥');
                    ",
                )
                .unwrap();
    }
    fs::write(dir.path().join("main.key"), key).unwrap();
    let store = LvedSqliteStore::discover(dir.path()).unwrap().unwrap();
    assert_eq!(
        store.search_modes().unwrap(),
        vec![
            SearchMode::Exact,
            SearchMode::Forward,
            SearchMode::Backward,
            SearchMode::Partial,
            SearchMode::FullText,
            SearchMode::Advanced("advanced1".to_owned()),
            SearchMode::Advanced("advanced2".to_owned()),
        ]
    );

    let hits = store.search("alp", &SearchMode::Forward, 10).unwrap();

    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].content_id, 100);
    let exact_hits = store.search("alpha", &SearchMode::Exact, 10).unwrap();
    assert_eq!(exact_hits.len(), 1);
    assert_eq!(exact_hits[0].content_id, 100);
    let punctuation_exact_hits = store.search("a, a", &SearchMode::Exact, 10).unwrap();
    assert_eq!(punctuation_exact_hits.len(), 1);
    assert_eq!(punctuation_exact_hits[0].content_id, 101);
    let fallback_exact_hits = store.search("(gamma)", &SearchMode::Exact, 10).unwrap();
    assert_eq!(fallback_exact_hits.len(), 1);
    assert_eq!(fallback_exact_hits[0].content_id, 102);
    let advanced_hits = store
        .search("topic", &SearchMode::Advanced("advanced1".to_owned()), 10)
        .unwrap();
    assert_eq!(advanced_hits.len(), 1);
    assert_eq!(advanced_hits[0].content_id, 100);
    let missing_advanced_hits = store
        .search(
            "topic",
            &SearchMode::Advanced("missing_column".to_owned()),
            10,
        )
        .unwrap();
    assert!(missing_advanced_hits.is_empty());
    assert_eq!(hits[0].anchor.as_deref(), Some("body-anchor"));
    assert_eq!(hits[0].title_text, "alpha");
    assert_eq!(
        store.content_html(100).unwrap().as_deref(),
        Some("<article><h1>Alpha</h1><p>body</p></article>")
    );
    assert_eq!(
        store.info_html(1).unwrap().as_deref(),
        Some("<h1>Example Dictionary 第2版</h1>")
    );
    assert_eq!(
        store.info_pages(10).unwrap()[0].title_text,
        "Example Dictionary 第2版"
    );
    assert_eq!(
        store.media_blob("lved.mediasub", "00010033.mp3").unwrap(),
        Some(b"ID3\x03".to_vec())
    );
    let window = store.list_window_for_content(101, 1, 1).unwrap().unwrap();
    assert_eq!(window.before[0].title_text, "alpha");
    assert_eq!(window.center.title_text, "beta");
    assert_eq!(window.after[0].title_text, "gamma");
}

#[test]
fn lved_hiragana_katakana_fts_variants_merge_before_cursor_paging() {
    let dir = tempdir().unwrap();
    let payload = dir.path().join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        apply_sqlcipher_key(&connection, key).unwrap();
        connection
            .execute_batch(
                "
                create table info (id integer, type integer, name text primary key, body text, media text);
                create table content (id integer primary key, type integer, body text, media text);
                create table list (
                  id integer primary key,
                  refid integer,
                  type integer,
                  anchor text,
                  title text,
                  titlesub text
                );
                create virtual table search using fts4(forward, back, part, fts, filter);
                insert into content values (100, 1, '<article>katakana only</article>', '');
                insert into content values (101, 1, '<article>hiragana only</article>', '');
                insert into content values (102, 1, '<article>both variants</article>', '');
                insert into list values (10, 100, 1, '', 'katakana', '');
                insert into list values (20, 101, 1, '', 'hiragana', '');
                insert into list values (30, 102, 1, '', 'both', '');
                insert into search(rowid, forward, back, part, fts, filter)
                  values (10, 'ア', 'ア', 'ア', 'ア', '∥ア∥');
                insert into search(rowid, forward, back, part, fts, filter)
                  values (20, 'あ', 'あ', 'あ', 'あ', '∥あ∥');
                insert into search(rowid, forward, back, part, fts, filter)
                  values (30, 'あ ア', 'ア あ', 'あ ア', 'あ ア', '∥あ∥ア∥');
                ",
            )
            .unwrap();
    }
    fs::write(dir.path().join("main.key"), key).unwrap();
    let store = LvedSqliteStore::discover(dir.path()).unwrap().unwrap();

    let first_page = store.search_page("あ", &SearchMode::Partial, 0, 2).unwrap();
    assert_eq!(
        first_page.iter().map(|hit| hit.list_id).collect::<Vec<_>>(),
        vec![10, 20]
    );

    let second_page = store.search_page("あ", &SearchMode::Partial, 2, 2).unwrap();
    assert_eq!(
        second_page
            .iter()
            .map(|hit| hit.list_id)
            .collect::<Vec<_>>(),
        vec![30]
    );
}

#[test]
fn media_blob_resolves_observed_lved_media_aliases() {
    let dir = tempdir().unwrap();
    let payload = dir.path().join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        apply_sqlcipher_key(&connection, key).unwrap();
        connection
            .execute_batch(
                "
                create table media (id integer primary key, name text, type integer, main blob);
                create table sound (id integer primary key, name text, type integer, main blob);
                insert into media values (1, 'a15f', 2, X'89504E47');
                insert into media values (265, '05e1bb8803a200c0', 2, X'FFD8FF');
                insert into media values (809, '000010', 5, X'49443303');
                insert into sound values (1, '10000010_example', 5, X'FFF384C4');
                ",
            )
            .unwrap();
    }
    fs::write(dir.path().join("main.key"), key).unwrap();

    let store = LvedSqliteStore::discover(dir.path()).unwrap().unwrap();
    assert_eq!(
        store.media_blob("lved.media", "a15f_C.png").unwrap(),
        Some(b"\x89PNG".to_vec())
    );
    assert_eq!(
        store
            .media_blob("lved.media", "../../image/FULL/zA265.jpg")
            .unwrap(),
        Some(b"\xff\xd8\xff".to_vec())
    );
    assert_eq!(
        store.media_blob("lved.mediasub", "000010.mp3").unwrap(),
        Some(b"ID3\x03".to_vec())
    );
    assert_eq!(
        store
            .media_blob("lved.mediasub", "10000010_example.mp3")
            .unwrap(),
        Some(b"\xff\xf3\x84\xc4".to_vec())
    );
}

#[test]
fn media_blob_uses_rowid_index_for_indexless_media_tables() {
    let dir = tempdir().unwrap();
    let payload = dir.path().join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        apply_sqlcipher_key(&connection, key).unwrap();
        connection
            .execute_batch(
                "
                create table media (id integer, name text, type integer, main blob);
                insert into media values (265, '05e1bb8803a200c0', 2, X'FFD8FF');
                insert into media values (809, '000010', 5, X'49443303');
                ",
            )
            .unwrap();
    }
    fs::write(dir.path().join("main.key"), key).unwrap();

    let store = LvedSqliteStore::discover(dir.path()).unwrap().unwrap();
    assert_eq!(
        store
            .media_blob("lved.media", "../../image/FULL/zA265.jpg")
            .unwrap(),
        Some(b"\xff\xd8\xff".to_vec())
    );
    assert_eq!(
        store.media_blob("lved.media", "000010.mp3").unwrap(),
        Some(b"ID3\x03".to_vec())
    );
}

#[test]
fn title_probe_rejects_common_false_positive_shapes() {
    assert!(normalize_title_candidate("外国語は片仮名で表記した．").is_none());
    assert!(title_score("和英小辞典") < 100);
    assert!(title_score("和英小辞典あ") < 100);
    assert_eq!(
        normalize_title_candidate("『広辞苑 第七版』　　&copy;2018年").as_deref(),
        Some("広辞苑 第七版")
    );
    assert_eq!(
        normalize_title_candidate("書籍版『岩波 日本史辞典』序").as_deref(),
        Some("岩波 日本史辞典")
    );
    assert_eq!(
        normalize_title_candidate("ライトハウス和英辞典 第5版 付録").as_deref(),
        Some("ライトハウス和英辞典 第5版")
    );
    assert_eq!(
        normalize_title_candidate("ライトハウス和英辞典 第5版 著作権").as_deref(),
        Some("ライトハウス和英辞典 第5版")
    );
    assert_eq!(
        normalize_title_candidate("書名プログレッシブ英和中辞典 第5版").as_deref(),
        Some("プログレッシブ英和中辞典 第5版")
    );
    assert_eq!(
        normalize_title_candidate("『新選国語辞典』第十版　目次").as_deref(),
        Some("新選国語辞典 第十版")
    );
}

#[test]
fn title_probe_prefers_index_title_over_later_bibliography_lines() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "
                create table info (id integer, type integer, name text primary key, body text);
                insert into info values (
                  1, 1, 'index.html',
                  '<div><b>研究社　類義語使い分け辞典 凡例</b></div>'
                );
                insert into info values (
                  2, 1, 'copyright.html',
                  '<p>『基礎日本語辞典』　森田良行、角川書店、1991、第 3 版</p>'
                );
                ",
        )
        .unwrap();

    let schema = LvedSqliteSchema::load(&connection).unwrap();
    assert_eq!(
        lved_sqlite_title_from_connection(&connection, &schema).as_deref(),
        Some("研究社　類義語使い分け辞典")
    );
}

#[test]
fn lved_list_projection_prefers_observed_subtitle_columns() {
    let columns = vec![
        "id".to_owned(),
        "refid".to_owned(),
        "title".to_owned(),
        "subtext".to_owned(),
        "titleplain".to_owned(),
        "type".to_owned(),
    ];
    assert_eq!(
        lved_list_projection(&columns),
        LvedListProjection {
            anchor: "''",
            title: "l.title",
            subtitle: "l.subtext",
            kind: "l.type",
        }
    );

    let columns = vec![
        "id".to_owned(),
        "refid".to_owned(),
        "titlesub".to_owned(),
        "subtext".to_owned(),
    ];
    assert_eq!(lved_list_projection(&columns).subtitle, "l.titlesub");

    let columns = vec!["id".to_owned(), "refid".to_owned(), "titleplain".to_owned()];
    assert_eq!(lved_list_projection(&columns).subtitle, "l.titleplain");
}

#[test]
fn title_probe_finds_late_book_title_and_ignores_index_labels() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "
                create table info (id integer, type integer, name text primary key, body text);
                insert into info values (
                  1, 1, 'index.html',
                  '<div class=\"title\">索引</div><div>和英インデックス</div>'
                );
                ",
        )
        .unwrap();
    for index in 0..300 {
        connection
            .execute(
                "insert into info values (?, 1, ?, ?)",
                (
                    index + 2,
                    format!("i{index:03}.html"),
                    format!("<div class=\"title\">CEFR-J ランク {index}</div>"),
                ),
            )
            .unwrap();
    }
    connection
            .execute(
                "insert into info values (1000, 1, 'h04.html', ?)",
                ["<div class=\"Copyright\"><div class=\"凡例書籍名\">エースクラウン英和辞典 第4版</div></div>"],
            )
            .unwrap();

    let schema = LvedSqliteSchema::load(&connection).unwrap();
    assert_eq!(
        lved_sqlite_title_from_connection(&connection, &schema).as_deref(),
        Some("エースクラウン英和辞典 第4版")
    );
}

#[test]
fn title_probe_prefers_lved_index_book_title_over_info_section_heading() {
    let connection = Connection::open_in_memory().unwrap();
    connection
            .execute_batch(
                "
                create table info (id integer, type integer, name text primary key, body text);
                insert into info values (
                  1, 1, 'index.html',
                  '<div><b>現代用語の基礎知識 2022 凡例</b><br>目次</div>'
                );
                insert into info values (
                  2, 1, 'special.html',
                  '<p class=\"mainTitle\">『現代用語の基礎知識』の特色</p><p class=\"midashi_1\">(2)「読める事典」――『現代用語の基礎知識』に特徴的な用語配列</p>'
                );
                insert into info values (
                  3, 1, 'copyright.html',
                  '<p class=\"mainTitle\">現代用語の基礎知識 2022について</p><p>現代用語の基礎知識　2022年版<br>電子版</p>'
                );
                ",
            )
            .unwrap();

    let schema = LvedSqliteSchema::load(&connection).unwrap();
    assert_eq!(
        lved_sqlite_title_from_connection(&connection, &schema).as_deref(),
        Some("現代用語の基礎知識 2022")
    );
}

#[test]
fn title_probe_does_not_stop_at_lved_index_menu_when_copyright_has_book_title() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "
                create table info (id integer, type integer, name text primary key, body text);
                insert into info values (
                  1, 1, 'index.html',
                  '<div class=\"索引\"><span class=\"title\">索引</span><a>和英小辞典</a><a>和英小辞典あ</a></div>'
                );
                insert into info values (
                  1040, 104, 'h04.html',
                  '<div class=\"Copyright\"><div class=\"凡例章見出\">著作権表示</div><div class=\"凡例書籍名\">エースクラウン英和辞典 第4版</div></div>'
                );
                ",
        )
        .unwrap();

    let schema = LvedSqliteSchema::load(&connection).unwrap();
    assert_eq!(
        lved_sqlite_title_from_connection(&connection, &schema).as_deref(),
        Some("エースクラウン英和辞典 第4版")
    );
}

#[test]
fn title_probe_ignores_style_blocks_and_staff_affiliations() {
    assert_eq!(
        html_text_lines(
            r#"<html><head><style>.title { color: red; }</style></head><body><div class="title">新明解国語辞典　第八版</div></body></html>"#
        ),
        vec!["新明解国語辞典　第八版".to_owned()]
    );
    assert_eq!(
        html_text_lines("<div>A&nbsp;&amp;&lt;B&gt;&quot;C&quot;</div>"),
        vec!["A &<B>\"C\"".to_owned()]
    );
    assert_eq!(
        html_text_lines("<div>&#x2051;test &#9733; &amp;#x2605; &#39;ok&#39;</div>"),
        vec!["⁑test ★ ★ 'ok'".to_owned()]
    );
    assert_eq!(
        html_text_lines(
            "<div>ジーニアス<ruby>英和辞典<rt>えいわじてん</rt></ruby> <ruby>第6版<rt>だいろっぱん</rt></ruby></div>"
        ),
        vec!["ジーニアス英和辞典 第6版".to_owned()]
    );
    assert!(
        normalize_title_candidate("浅井　昌弘　慶應義塾大学医学部　精神神経科　教授").is_none()
    );
}

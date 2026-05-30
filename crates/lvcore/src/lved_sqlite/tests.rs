
use super::*;
use tempfile::tempdir;

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
                    insert into mediasub values (1, '00010033', 5, X'49443303');
                    insert into list values (1, 100, 1, 'body-anchor', '<b>alpha</b>', '<span>subtitle</span>');
                    insert into list values (2, 101, 1, '', '<b>beta</b>', '');
                    insert into list values (3, 102, 1, '', '<b>gamma</b>', '');
                    insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                      values (1, 'alpha', 'ahpla', 'alpha', 'alpha body', 'topic marker', '', '∥alpha∥');
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

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].content_id, 100);
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
fn title_probe_rejects_common_false_positive_shapes() {
    assert!(normalize_title_candidate("外国語は片仮名で表記した．").is_none());
    assert!(title_score("和英小辞典") < 100);
    assert_eq!(
        normalize_title_candidate("『広辞苑 第七版』　　&copy;2018年").as_deref(),
        Some("広辞苑 第七版")
    );
    assert_eq!(
        normalize_title_candidate("書籍版『岩波 日本史辞典』序").as_deref(),
        Some("岩波 日本史辞典")
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
    assert!(
        normalize_title_candidate("浅井　昌弘　慶應義塾大学医学部　精神神経科　教授").is_none()
    );
}

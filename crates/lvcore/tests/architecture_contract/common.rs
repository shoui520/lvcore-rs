#![allow(dead_code)]

pub(crate) use std::fs;
pub(crate) use std::io::Write;
pub(crate) use std::path::Path;

pub(crate) use lvcore::{
    ANDROID_LVEDINFO_MAGIC, BodySourceKind, BookLibrary, Capability, DriverRegistry, FormatFamily,
    GaijiPolicy, GaijiSourcePreference, InternalResource, InternalTarget, LabelOptions,
    NavigationStatus, NavigationSurface, NavigationSurfaceKind, PackageDiscoveryOptions,
    RenderMode, RenderOptions, RendererInput, ResolvedTargetKind, ResourceKind, ResourceToken,
    SSEDDATA_MAGIC, SSEDINFO_MAGIC, SearchMode, SearchQuery, SearchScope, StorageBackend,
    TargetKind, TargetToken, VisualBody,
};
pub(crate) use rusqlite::Connection;
pub(crate) use tempfile::tempdir;
pub(crate) use zip::unstable::write::FileOptionsExt;
pub(crate) use zip::write::{SimpleFileOptions, ZipWriter};

pub(crate) fn ssedinfo_fixture() -> Vec<u8> {
    ssedinfo_fixture_with_honmon("HONMON.DIC")
}

pub(crate) fn ssedinfo_fixture_with_index_type(index_type: u8) -> Vec<u8> {
    ssedinfo_fixture_with_honmon_index_type_and_blocks("HONMON.DIC", index_type, 2)
}

pub(crate) fn ssedinfo_fixture_with_index_type_and_blocks(
    index_type: u8,
    index_blocks: u32,
) -> Vec<u8> {
    ssedinfo_fixture_with_honmon_index_type_and_blocks("HONMON.DIC", index_type, index_blocks)
}

pub(crate) fn ssedinfo_fixture_with_backward_index() -> Vec<u8> {
    let record_start = 0x80;
    let mut data = vec![0u8; record_start + 4 * 0x30];
    data[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Backward Fixture";
    data[0x0c] = title.len() as u8;
    data[0x0d..0x0d + title.len()].copy_from_slice(title);
    data[0x4d] = 4;
    write_record(
        &mut data[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        "HONMON.DIC",
    );
    write_record(
        &mut data[record_start + 0x30..record_start + 0x60],
        0x07,
        13,
        14,
        "BHTITLE.DIC",
    );
    write_record(
        &mut data[record_start + 0x60..record_start + 0x90],
        0x71,
        15,
        15,
        "BHINDEX.DIC",
    );
    write_record(
        &mut data[record_start + 0x90..record_start + 0xc0],
        0xf2,
        17,
        18,
        "GA16HALF",
    );
    data
}

pub(crate) fn ssedinfo_fixture_with_forward_and_backward_indexes() -> Vec<u8> {
    let record_start = 0x80;
    let mut data = vec![0u8; record_start + 6 * 0x30];
    data[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Bidirectional Fixture";
    data[0x0c] = title.len() as u8;
    data[0x0d..0x0d + title.len()].copy_from_slice(title);
    data[0x4d] = 6;
    write_record(
        &mut data[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        "HONMON.DIC",
    );
    write_record(
        &mut data[record_start + 0x30..record_start + 0x60],
        0x05,
        13,
        14,
        "FHTITLE.DIC",
    );
    write_record(
        &mut data[record_start + 0x60..record_start + 0x90],
        0x91,
        17,
        17,
        "FHINDEX.DIC",
    );
    write_record(
        &mut data[record_start + 0x90..record_start + 0xc0],
        0x07,
        15,
        16,
        "BHTITLE.DIC",
    );
    write_record(
        &mut data[record_start + 0xc0..record_start + 0xf0],
        0x71,
        18,
        18,
        "BHINDEX.DIC",
    );
    write_record(
        &mut data[record_start + 0xf0..record_start + 0x120],
        0xf2,
        19,
        20,
        "GA16HALF",
    );
    data
}

pub(crate) fn write_minimal_lved_sqlite_fixture(root: &Path) {
    let payload = root.join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        connection.pragma_update(None, "key", key).unwrap();
        connection
            .pragma_update(None, "cipher_compatibility", 4)
            .unwrap();
        connection
            .execute_batch(
                "
                create table info (id integer, type integer, name text primary key, body text, media text);
                insert into info values (1, 1, 'about.html', '<h1>Example Dictionary 第2版</h1>', '');
                insert into info values (null, 1, 'null-id.html', '<h1>Null id info</h1>', '');
                create table content (id integer primary key, type integer, body text, media text);
                insert into content values (100, 1, '<article><h1>Alpha</h1><p>Tree body</p></article>', '');
                insert into content values (105, 1, '<article><h1>Beta</h1><p>Tree body</p></article>', '');
                create table list (id integer primary key, refid integer, type integer, anchor text, title text, titlesub text);
                insert into list values (1, 100, 1, '', '<b>alpha</b>', '');
                insert into list values (2, 105, 1, '', '<b>beta</b>', '');
                create virtual table search using fts4(forward, back, part, fts, advanced1, advanced2, filter);
                insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                  values (1, 'alpha', 'ahpla', 'shared alpha', 'alpha body', '', '', '∥shared∥');
                insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                  values (2, 'beta', 'ateb', 'shared beta', 'beta body', '', '', '∥shared∥');
                ",
            )
            .unwrap();
    }
    fs::create_dir(root.join("res")).unwrap();
    fs::write(
        root.join("res/tree.idx"),
        "\u{feff}0\t0\tExample Dictionary\r\n0\t1\tBrowse\r\n100\t2\tAlpha\r\n105\t2\tBeta\r\n",
    )
    .unwrap();
    fs::write(root.join("main.key"), key).unwrap();
}

pub(crate) fn write_lved_cross_book_source_fixture(root: &Path) {
    let payload = root.join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        connection.pragma_update(None, "key", key).unwrap();
        connection
            .pragma_update(None, "cipher_compatibility", 4)
            .unwrap();
        connection
            .execute_batch(
                "
                create table content (id integer primary key, type integer, body text, media text);
                insert into content values (
                  10,
                  1,
                  '<article><h1>Source</h1><a href=\"lved.contentlink:BUREI.100#dest\">target</a></article>',
                  ''
                );
                create table list (id integer primary key, refid integer, type integer, anchor text, title text, titlesub text);
                insert into list values (1, 10, 1, '', 'source', '');
                ",
            )
            .unwrap();
    }
    fs::write(root.join("main.key"), key).unwrap();
}

pub(crate) fn write_minimal_multiview_content_fixture(path: &Path) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            r#"
            create table t_contents (
              f_ID integer primary key,
              f_Title text,
              f_Body text
            );
            create table t_search (
              f_No integer primary key,
              f_ID integer,
              f_Anchor integer,
              f_KeyWord text,
              f_MainFlag integer,
              f_Level integer,
              f_TitleMain text,
              f_All text
            );
            insert into t_contents values
              (1, '<b>まえがき</b>', '<article><h1>まえがき</h1><p>body</p><a href="lved_ref:entry:000002">next</a><img src="pic.png"><img src="javascript:bad()"><object data="file:///tmp/outside.bin"></object><img src="lvcore://resource/already-normalized"></article>');
            insert into t_contents values
              (2, '<b>本文</b>', '<article><h1>本文</h1><p>body</p></article>');
            insert into t_contents values
              (3, '<b>あとがき</b>', '<article><h1>あとがき</h1><p>body</p></article>');
            insert into t_search values
              (1, 1, 1, '§まえがき§', 1, 0, '<b>まえがき</b>', 'まえがき body');
            insert into t_search values
              (2, 2, 1, '§本文§', 1, 0, '<b>本文</b>', '本文 body');
            "#,
        )
        .unwrap();
}

pub(crate) fn write_minimal_multiview_law_fixture(root: &Path) {
    let law = Connection::open(root.join("blvbat")).unwrap();
    law.execute_batch(
        r#"
        create table t_page (
          f_hore_code text,
          f_rec_id integer,
          f_rec_type integer,
          f_title_no text,
          f_title_sub text,
          f_anchor text,
          f_text text,
          f_text_plane text,
          f_text_count integer,
          f_text_plane_count integer
        );
        create table t_111S21K1 (
          f_hore_code text,
          f_rec_id integer,
          f_rec_type integer,
          f_title_no text,
          f_title_sub text,
          f_anchor text,
          f_text text,
          f_text_plane text,
          f_text_count integer,
          f_text_plane_count integer
        );
        insert into t_111S21K1 values
          ('111S21K1', 10000, 0, '見出し', '', '111S21K1_TITLE',
           '<div class="header">日本国憲法本文</div>', '日本国憲法本文', 0, 0);
        insert into t_111S21K1 values
          ('111S21K1', 20000, 0, '公布文・前文', '', '111S21K1_ZEN',
           '<div class="zenbun">前文</div>', '前文', 0, 0);
        "#,
    )
    .unwrap();

    let metadata = Connection::open(root.join("nlvbat")).unwrap();
    metadata
        .execute_batch(
            r#"
            create table t_hore (
              f_hore_code text,
              f_hore_id integer,
              f_pub_era integer,
              f_pub_year integer,
              f_pub_no integer,
              f_pub_date date,
              f_pub_desc string,
              f_name string,
              f_name_sub text,
              f_name_kana text,
              f_kana_ini string,
              f_kana_order integer,
              f_abbr1 string,
              f_abbr1_kana text,
              f_nickname text,
              f_commonname text,
              f_commonname_kana text,
              f_commonname_ex text,
              f_category_id text
            );
            insert into t_hore values
              ('111S21K1', 1, 0, 0, 0, '', '', '日本国憲法', '', 'にほんこくけんぽう', 'に', 1, '', '', '', '', '', '', '1');
            insert into t_hore values
              ('22M1', 2, 0, 0, 0, '', '', '民法', '', 'みんぽう', 'み', 2, '', '', '', '', '', '', '3');
            "#,
        )
        .unwrap();
}

pub(crate) fn write_minimal_hourei_fixture(root: &Path) {
    let database = root.join("_DataBase");
    fs::create_dir_all(database.join("HTMLs/H")).unwrap();
    fs::create_dir_all(database.join("image")).unwrap();
    fs::create_dir_all(database.join("H01")).unwrap();
    fs::create_dir_all(root.join("_Programs")).unwrap();
    fs::write(database.join("image/law.png"), b"png").unwrap();
    fs::write(
        root.join("_Programs/index_panel.html"),
        r#"<html><body>
        <p class="cell_line"><a class="cell_enable"></a><a class="cell" href="lved_ref:み">み</a></p>
        <p class="cell_line"><a class="cell" href = "lved_ref:し">し</a></p>
        </body></html>"#,
    )
    .unwrap();

    for name in ["hore_base.db", "hore_search_a.db"] {
        let connection = Connection::open(database.join(name)).unwrap();
        connection
            .execute_batch(
                r#"
                create table t_category (f_category_id integer, f_category_name string);
                create table t_hore (
                  f_hore_id integer,
                  f_name string,
                  f_name_sub string,
                  f_abbr1 string,
                  f_abbr2 string,
                  f_abbr3 string,
                  f_abbr4 string,
                  f_abbr5 string,
                  f_abbr6 string,
                  f_abbr7 string,
                  f_category_id integer,
                  f_kana_ini string,
                  f_kana_order integer,
                  f_text_plane text
                );
                insert into t_category values (10, '民事');
                insert into t_hore values
                  (401000000000000001, '民法', '', '', '', '', '', '', '', '', 10, 'み', 1, '民法本文'),
                  (401000000000000002, '商法', '', '', '', '', '', '', '', '', 10, 'し', 2, '商法本文');
                "#,
            )
            .unwrap();
    }
    Connection::open(database.join("horejo_base.db")).unwrap();
    fs::write(
        database.join("HTMLs/H/401000000000000001_H.html"),
        r#"<div class="header">民法</div><a href="lved_mark&&A1">mark</a><a href="lved_ref&1:401000000000000002&A2">商法</a><a href="lved_ref:み">み</a><img src="law.png">"#,
    )
    .unwrap();
    let shard = Connection::open(database.join("H01/401000000000000002.db")).unwrap();
    shard
        .execute_batch(
            r#"
            create table t_page (f_rec_id integer, f_text text);
            insert into t_page values (1, '<div>商法本文</div>');
            "#,
        )
        .unwrap();
}

pub(crate) fn ssedinfo_fixture_with_honmon(honmon_filename: &str) -> Vec<u8> {
    ssedinfo_fixture_with_magic_and_honmon(SSEDINFO_MAGIC, honmon_filename)
}

pub(crate) fn ssedinfo_fixture_with_magic_and_honmon(
    magic: &[u8; 8],
    honmon_filename: &str,
) -> Vec<u8> {
    ssedinfo_fixture_with_magic_honmon_index_type_and_blocks(magic, honmon_filename, 0x91, 2)
}

pub(crate) fn ssedinfo_fixture_with_honmon_index_type_and_blocks(
    honmon_filename: &str,
    index_type: u8,
    index_blocks: u32,
) -> Vec<u8> {
    ssedinfo_fixture_with_magic_honmon_index_type_and_blocks(
        SSEDINFO_MAGIC,
        honmon_filename,
        index_type,
        index_blocks,
    )
}

pub(crate) fn ssedinfo_fixture_with_magic_honmon_index_type_and_blocks(
    magic: &[u8; 8],
    honmon_filename: &str,
    index_type: u8,
    index_blocks: u32,
) -> Vec<u8> {
    let record_start = 0x80;
    let mut data = vec![0u8; record_start + 5 * 0x30];
    data[..8].copy_from_slice(magic);
    let title = b"Fixture Book";
    data[0x0c] = title.len() as u8;
    data[0x0d..0x0d + title.len()].copy_from_slice(title);
    data[0x4d] = 5;
    write_record(
        &mut data[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        honmon_filename,
    );
    write_record(
        &mut data[record_start + 0x30..record_start + 0x60],
        0x01,
        11,
        12,
        "MENU.DIC",
    );
    write_record(
        &mut data[record_start + 0x60..record_start + 0x90],
        0x05,
        13,
        14,
        "FHTITLE.DIC",
    );
    write_record(
        &mut data[record_start + 0x90..record_start + 0xc0],
        index_type,
        15,
        15 + index_blocks.saturating_sub(1),
        "FHINDEX.DIC",
    );
    write_record(
        &mut data[record_start + 0xc0..record_start + 0xf0],
        0xf2,
        17,
        18,
        "GA16HALF",
    );
    data
}

pub(crate) fn ssedinfo_fixture_with_multi_selector() -> Vec<u8> {
    let record_start = 0x80;
    let mut data = vec![0u8; record_start + 5 * 0x30];
    data[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Fixture Multi Book";
    data[0x0c] = title.len() as u8;
    data[0x0d..0x0d + title.len()].copy_from_slice(title);
    data[0x4d] = 5;
    write_record(
        &mut data[record_start..record_start + 0x30],
        0x00,
        1,
        1,
        "HONMON.DIC",
    );
    write_record(
        &mut data[record_start + 0x30..record_start + 0x60],
        0xff,
        20,
        20,
        "MULTI1.DIC",
    );
    write_record(
        &mut data[record_start + 0x60..record_start + 0x90],
        0x01,
        21,
        21,
        "MUL1_1_1.DIC",
    );
    write_record(
        &mut data[record_start + 0x90..record_start + 0xc0],
        0x05,
        22,
        22,
        "MUL1_1_2.DIC",
    );
    write_record(
        &mut data[record_start + 0xc0..record_start + 0xf0],
        0x91,
        23,
        23,
        "MUL1_1_3.DIC",
    );
    data
}

pub(crate) fn multi_descriptor_fixture() -> Vec<u8> {
    let mut data = vec![0u8; 0x10];
    data[0..2].copy_from_slice(&1u16.to_be_bytes());
    data.resize(0x30, 0);
    data[0x10] = 3;
    data[0x12..0x17].copy_from_slice(b"CLASS");
    for (component_type, start_block) in [(0x01u8, 21u32), (0x05, 22), (0x91, 23)] {
        data.push(component_type);
        data.push(0);
        data.extend_from_slice(&start_block.to_be_bytes());
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&[0; 6]);
    }
    data
}

pub(crate) fn selector_menu_fixture(labels: &[&str]) -> Vec<u8> {
    let mut data = Vec::new();
    for label in labels {
        data.extend_from_slice(&[0x1f, 0x04]);
        data.extend_from_slice(&jis_fullwidth_ascii_key(label));
        data.extend_from_slice(&[0x1f, 0x05]);
        data.extend_from_slice(&[0x1f, 0x0a]);
    }
    data
}

pub(crate) fn write_record(
    rec: &mut [u8],
    component_type: u8,
    start: u32,
    end: u32,
    filename: &str,
) {
    rec[3] = component_type;
    rec[4..8].copy_from_slice(&start.to_be_bytes());
    rec[8..12].copy_from_slice(&end.to_be_bytes());
    rec[0x10] = filename.len() as u8;
    rec[0x11..0x11 + filename.len()].copy_from_slice(filename.as_bytes());
}

pub(crate) fn sseddata_literal_fixture(literals: &[u8]) -> Vec<u8> {
    sseddata_literal_fixture_at(1, literals)
}

pub(crate) fn sseddata_literal_fixture_at(start_block: u32, literals: &[u8]) -> Vec<u8> {
    let chunk_offset = 0x44usize;
    let block_count = literals.len().div_ceil(2048).max(1);
    let mut data = vec![0u8; chunk_offset];
    data[..8].copy_from_slice(SSEDDATA_MAGIC);
    data[0x0f] = 1;
    data[0x16..0x18].copy_from_slice(&1u16.to_be_bytes());
    data[0x18..0x1c].copy_from_slice(&start_block.to_be_bytes());
    data[0x1c..0x20].copy_from_slice(
        &start_block
            .saturating_add(block_count as u32)
            .saturating_sub(1)
            .to_be_bytes(),
    );
    data[0x40..0x44].copy_from_slice(&(chunk_offset as u32).to_be_bytes());
    data.extend_from_slice(&[0, 0]);
    data.extend_from_slice(&(literals.len() as u16).to_be_bytes());
    data.push(0);
    for literal in literals {
        data.extend_from_slice(&[0, 0, *literal]);
    }
    data
}

pub(crate) fn android_wrapped_sseddata_fixture(payload: Vec<u8>) -> Vec<u8> {
    assert!(payload.len() >= 64);
    let mut wrapped = b"LV_".to_vec();
    wrapped.extend_from_slice(&payload[..64]);
    wrapped.extend_from_slice(&[0, 0]);
    wrapped.extend_from_slice(&payload[64..]);
    wrapped
}

pub(crate) fn write_zipcrypto_honmon_wrapper(
    path: &Path,
    member_name: &str,
    password: &[u8],
    payload: &[u8],
) {
    let file = fs::File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().with_deprecated_encryption(password);
    zip.start_file(member_name, options).unwrap();
    zip.write_all(payload).unwrap();
    zip.finish().unwrap();
}

pub(crate) fn menu_stream_fixture(block: u32, offset: u16) -> Vec<u8> {
    menu_stream_fixture_rows(&[([0x24, 0x22], block, offset)])
}

pub(crate) fn menu_stream_fixture_rows(rows: &[([u8; 2], u32, u16)]) -> Vec<u8> {
    let mut data = Vec::new();
    for (label, block, offset) in rows {
        data.extend_from_slice(&[0x1f, 0x09, 0x00, 0x01]);
        data.extend_from_slice(&[0x1f, 0x42]);
        data.extend_from_slice(label);
        data.extend_from_slice(&[0x1f, 0x62]);
        data.extend_from_slice(&bcd_u32(*block));
        data.extend_from_slice(&bcd_u16(*offset));
        data.extend_from_slice(&[0x1f, 0x0a]);
    }
    data
}

pub(crate) fn panel_bin_fixture(block: u32, offset: u32) -> Vec<u8> {
    panel_bin_fixture_rows(&[(block, offset, [0x24, 0x22])])
}

pub(crate) fn panel_bin_fixture_rows(rows: &[(u32, u32, [u8; 2])]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    data.extend_from_slice(&4u32.to_le_bytes());
    for (block, offset, label) in rows {
        data.extend_from_slice(&block.to_le_bytes());
        data.extend_from_slice(&offset.to_le_bytes());
        data.extend_from_slice(label);
        data.extend_from_slice(&[0x00, 0x00]);
    }
    data
}

pub(crate) fn uni_fixture() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"Ver2  ");
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(&1u32.to_be_bytes());
    data.extend_from_slice(&[
        0xB1, 0x23, 0x00, 0x00, 0x4E, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    data
}

pub(crate) fn ga16_fixture(start_code: u16, count: u16) -> Vec<u8> {
    let glyph_bytes = 2 * 16;
    let mut data = vec![0u8; 2048 + usize::from(count) * glyph_bytes];
    data[8] = 16;
    data[9] = 16;
    data[10..12].copy_from_slice(&start_code.to_be_bytes());
    data[12..14].copy_from_slice(&count.to_be_bytes());
    for index in 0..usize::from(count) {
        let start = 2048 + index * glyph_bytes;
        data[start..start + glyph_bytes].fill(0x81);
    }
    data
}

pub(crate) fn ssed_view_offset(view: &lvcore::ResolvedTargetView) -> Option<(u32, u32)> {
    match view.target.decode().ok()? {
        InternalTarget::SsedAddress { block, offset, .. }
        | InternalTarget::SsedBoundedAddress { block, offset, .. } => Some((block, offset)),
        _ => None,
    }
}

pub(crate) fn bcd_u32(value: u32) -> [u8; 4] {
    let digits = format!("{value:08}");
    let bytes = digits.as_bytes();
    [
        (bytes[0] - b'0') << 4 | (bytes[1] - b'0'),
        (bytes[2] - b'0') << 4 | (bytes[3] - b'0'),
        (bytes[4] - b'0') << 4 | (bytes[5] - b'0'),
        (bytes[6] - b'0') << 4 | (bytes[7] - b'0'),
    ]
}

pub(crate) fn bcd_u16(value: u16) -> [u8; 2] {
    let digits = format!("{value:04}");
    let bytes = digits.as_bytes();
    [
        (bytes[0] - b'0') << 4 | (bytes[1] - b'0'),
        (bytes[2] - b'0') << 4 | (bytes[3] - b'0'),
    ]
}

pub(crate) fn simple_index_fixture(
    key: &str,
    body_block: u32,
    body_offset: u16,
    title_block: u32,
    title_offset: u16,
) -> Vec<u8> {
    simple_index_fixture_rows(&[(key, body_block, body_offset, title_block, title_offset)])
}

pub(crate) fn simple_index_fixture_rows(rows: &[(&str, u32, u16, u32, u16)]) -> Vec<u8> {
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

pub(crate) fn simple_index_raw_ascii_fixture_rows(rows: &[(&str, u32, u16, u32, u16)]) -> Vec<u8> {
    let mut page = vec![0u8; 2048];
    page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    page[2..4].copy_from_slice(&(rows.len() as u16).to_be_bytes());
    let mut pos = 4usize;
    for (key, body_block, body_offset, title_block, title_offset) in rows {
        let key = key.as_bytes();
        page[pos] = key.len() as u8;
        pos += 1;
        page[pos..pos + key.len()].copy_from_slice(key);
        pos += key.len();
        page[pos..pos + 4].copy_from_slice(&body_block.to_be_bytes());
        page[pos + 4..pos + 6].copy_from_slice(&body_offset.to_be_bytes());
        page[pos + 6..pos + 10].copy_from_slice(&title_block.to_be_bytes());
        page[pos + 10..pos + 12].copy_from_slice(&title_offset.to_be_bytes());
        pos += 12;
    }
    page
}

pub(crate) fn simple_index_raw_key_fixture_rows(rows: &[(&[u8], u32, u16, u32, u16)]) -> Vec<u8> {
    let mut page = vec![0u8; 2048];
    page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    page[2..4].copy_from_slice(&(rows.len() as u16).to_be_bytes());
    let mut pos = 4usize;
    for (key, body_block, body_offset, title_block, title_offset) in rows {
        page[pos] = key.len() as u8;
        pos += 1;
        page[pos..pos + key.len()].copy_from_slice(key);
        pos += key.len();
        page[pos..pos + 4].copy_from_slice(&body_block.to_be_bytes());
        page[pos + 4..pos + 6].copy_from_slice(&body_offset.to_be_bytes());
        page[pos + 6..pos + 10].copy_from_slice(&title_block.to_be_bytes());
        page[pos + 10..pos + 12].copy_from_slice(&title_offset.to_be_bytes());
        pos += 12;
    }
    page
}

pub(crate) fn leaf_page_fixture(records: &[Vec<u8>]) -> Vec<u8> {
    let mut page = vec![0u8; 2048];
    page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    page[2..4].copy_from_slice(&(records.len() as u16).to_be_bytes());
    let mut pos = 4usize;
    for record in records {
        page[pos..pos + record.len()].copy_from_slice(record);
        pos += record.len();
    }
    page
}

pub(crate) fn internal_page_fixture(rows: &[(&str, u32)]) -> Vec<u8> {
    let mut page = vec![0u8; 2048];
    let key_len = 2usize;
    page[0..2].copy_from_slice(&(key_len as u16).to_be_bytes());
    page[2..4].copy_from_slice(&(rows.len() as u16).to_be_bytes());
    let mut pos = 4usize;
    for (key, child_block) in rows {
        let raw_key = if *key == "\u{10ffff}" {
            vec![0xff; key_len]
        } else {
            let mut key = jis_fullwidth_ascii_key(key);
            key.resize(key_len, 0);
            key
        };
        page[pos..pos + key_len].copy_from_slice(&raw_key[..key_len]);
        pos += key_len;
        page[pos..pos + 4].copy_from_slice(&child_block.to_be_bytes());
        pos += 4;
    }
    page
}

pub(crate) fn body_only_simple_record(key: &str, body_block: u32, body_offset: u16) -> Vec<u8> {
    let key = jis_fullwidth_ascii_key(key);
    let mut out = Vec::new();
    out.push(key.len() as u8);
    out.extend_from_slice(&key);
    out.extend_from_slice(&body_block.to_be_bytes());
    out.extend_from_slice(&body_offset.to_be_bytes());
    out
}

pub(crate) fn tagged_group_record(key: &str, count: u16) -> Vec<u8> {
    let key = jis_fullwidth_ascii_key(key);
    let mut out = vec![0x80, key.len() as u8];
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&key);
    out
}

pub(crate) fn tagged_target_record(
    key: &str,
    body_block: u32,
    body_offset: u16,
    title_block: u32,
    title_offset: u16,
) -> Vec<u8> {
    let key = jis_fullwidth_ascii_key(key);
    let mut out = vec![0xc0, key.len() as u8];
    out.extend_from_slice(&key);
    out.extend_from_slice(&body_block.to_be_bytes());
    out.extend_from_slice(&body_offset.to_be_bytes());
    out.extend_from_slice(&title_block.to_be_bytes());
    out.extend_from_slice(&title_offset.to_be_bytes());
    out
}

pub(crate) fn tagged_target_body_only_record(
    key: &str,
    body_block: u32,
    body_offset: u16,
) -> Vec<u8> {
    let key = jis_fullwidth_ascii_key(key);
    let mut out = vec![0xc0, key.len() as u8];
    out.extend_from_slice(&key);
    out.extend_from_slice(&body_block.to_be_bytes());
    out.extend_from_slice(&body_offset.to_be_bytes());
    out
}

pub(crate) fn title_group_record(
    key: &str,
    title_block: u32,
    title_offset: u16,
    count: u32,
) -> Vec<u8> {
    let key = jis_fullwidth_ascii_key(key);
    let mut out = vec![0x80, key.len() as u8];
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&key);
    out.extend_from_slice(&title_block.to_be_bytes());
    out.extend_from_slice(&title_offset.to_be_bytes());
    out
}

pub(crate) fn compact_body_target_record(tag: u8, body_block: u32, body_offset: u16) -> Vec<u8> {
    let mut out = vec![tag];
    out.extend_from_slice(&body_block.to_be_bytes());
    out.extend_from_slice(&body_offset.to_be_bytes());
    out
}

pub(crate) fn multi_group_record(key: &str, count: u32) -> Vec<u8> {
    let key = jis_fullwidth_ascii_key(key);
    let mut out = vec![0x80, key.len() as u8];
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&key);
    out
}

pub(crate) fn multi_target_record(
    body_block: u32,
    body_offset: u16,
    title_block: u32,
    title_offset: u16,
) -> Vec<u8> {
    let mut out = vec![0xc0];
    out.extend_from_slice(&body_block.to_be_bytes());
    out.extend_from_slice(&body_offset.to_be_bytes());
    out.extend_from_slice(&title_block.to_be_bytes());
    out.extend_from_slice(&title_offset.to_be_bytes());
    out
}

pub(crate) fn jis_fullwidth_ascii_key(text: &str) -> Vec<u8> {
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

pub(crate) fn body_jis(text: &str) -> Vec<u8> {
    text.chars()
        .flat_map(|ch| {
            let body_ch = if (0x20..=0x7e).contains(&(ch as u32)) {
                if ch == ' ' {
                    '\u{3000}'
                } else {
                    char::from_u32(ch as u32 + 0xfee0).unwrap_or(ch)
                }
            } else {
                ch
            };
            let body_text = body_ch.to_string();
            let (encoded, _encoding, _had_errors) = encoding_rs::SHIFT_JIS.encode(&body_text);
            encoded
                .chunks(2)
                .next()
                .and_then(sjis_pair_to_jis_pair)
                .unwrap_or_default()
        })
        .collect()
}

fn sjis_pair_to_jis_pair(sjis: &[u8]) -> Option<Vec<u8>> {
    if sjis.len() != 2 {
        return None;
    }
    let lead = sjis[0];
    let trail = sjis[1];
    let row_base = if (0x81..=0x9f).contains(&lead) {
        (lead - 0x81) * 2
    } else if (0xe0..=0xef).contains(&lead) {
        (lead - 0xc1) * 2
    } else {
        return None;
    };
    let (row, cell) = if (0x9f..=0xfc).contains(&trail) {
        (row_base + 1, trail - 0x9f)
    } else if (0x40..=0xfc).contains(&trail) && trail != 0x7f {
        let adjusted = if trail >= 0x80 { trail - 1 } else { trail };
        (row_base, adjusted - 0x40)
    } else {
        return None;
    };
    Some(vec![row + 0x21, cell + 0x21])
}

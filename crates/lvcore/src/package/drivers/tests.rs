use std::fs;

use aes::Aes128;
use aes::cipher::{BlockEncrypt, KeyInit};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

use crate::lved_sqlite::apply_sqlcipher_key;
use crate::render::{
    HcRendererProfileSource, HcRendererProfileStatus, RenderCapability, RendererInput,
};
use crate::ssed::SSEDINFO_MAGIC;
use crate::target::TargetKind;

use super::super::PackageDriver;
use super::super::capabilities::{ssed_search_modes, ssed_sidecar_search_modes};
use super::super::ssed_detection::ssed_capabilities;
use super::*;

mod dense_sidecar;
mod detection;
mod fulltext;
mod lved;
mod ssed_component_resources;
mod ssed_loose_resources;
mod ssed_navigation_surfaces;
mod ssed_renderer_input;

enum DenseSidecarFixture {
    BodyRows,
    SharedBodyRows,
    BodyRowsWithLvedLinks,
    AndroidRowidTimesFiveBodyRows,
    TitleOnlyThenBodyRows,
    ShardedTContentsBodyRows,
    BlobBodyRows,
    PlainOnlyBodyRows,
    EntityTitleRows,
    CjkTitleRows,
    AsciiMarkerTitleRows,
    MissingBetaRow,
    OrderedHonbunRows,
}

fn write_ssed_dense_sidecar_fixture(root: &Path, fixture: DenseSidecarFixture) -> SsedCatalog {
    let mut body = Vec::new();
    let (alpha_anchor, beta_anchor) = match fixture {
        DenseSidecarFixture::AndroidRowidTimesFiveBodyRows => ("00000005", "00000010"),
        _ => ("00000001", "00000002"),
    };
    let beta_body_offset;
    match fixture {
        DenseSidecarFixture::OrderedHonbunRows => {
            body.extend_from_slice(&ordered_honbun_entry_record("alpha", &["alpha yomi"]));
            beta_body_offset = u16::try_from(body.len()).unwrap();
            body.extend_from_slice(&ordered_honbun_entry_record(
                "beta",
                &["beta yomi", "beta alternate"],
            ));
        }
        _ => {
            body.extend_from_slice(&dense_anchor_record(alpha_anchor));
            beta_body_offset = u16::try_from(body.len()).unwrap();
            body.extend_from_slice(&dense_anchor_record(beta_anchor));
        }
    }
    fs::write(
        root.join("HONMON.DIC"),
        fixture_sseddata_literal_chunks(&[&body], 100, 100),
    )
    .unwrap();

    let mut titles = Vec::new();
    let alpha_title_offset = 0u16;
    titles.extend_from_slice(b"alpha\x1f\x0a");
    let beta_title_offset = u16::try_from(titles.len()).unwrap();
    titles.extend_from_slice(b"beta\x1f\x0a");
    fs::write(
        root.join("FHTITLE.DIC"),
        fixture_sseddata_literal_chunks(&[&titles], 300, 300),
    )
    .unwrap();

    let mut index_page = vec![0u8; crate::ssed::BLOCK_SIZE as usize];
    index_page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
    index_page[2..4].copy_from_slice(&2u16.to_be_bytes());
    let mut pos = 4usize;
    write_simple_index_row(
        &mut index_page,
        &mut pos,
        &body_jis("あ"),
        100,
        0,
        300,
        alpha_title_offset,
    );
    write_simple_index_row(
        &mut index_page,
        &mut pos,
        &body_jis("い"),
        100,
        beta_body_offset,
        300,
        beta_title_offset,
    );
    fs::write(
        root.join("FHINDEX.DIC"),
        fixture_sseddata_literal_chunks(&[&index_page], 200, 200),
    )
    .unwrap();

    match fixture {
        DenseSidecarFixture::BodyRows => {
            write_dense_body_db(root.join("body.db"), true, true, false);
        }
        DenseSidecarFixture::SharedBodyRows => {
            write_dense_shared_body_db(root.join("body.db"));
        }
        DenseSidecarFixture::BodyRowsWithLvedLinks => {
            write_dense_body_db_with_lved_links(root.join("body.db"));
        }
        DenseSidecarFixture::AndroidRowidTimesFiveBodyRows => {
            write_android_body_db(root.join("DENSE.db"), "DENSE");
        }
        DenseSidecarFixture::TitleOnlyThenBodyRows => {
            let connection = Connection::open(root.join("a-title-only.db")).unwrap();
            connection
                .execute_batch(
                    "
                        create table t_contents (f_DataId integer primary key, f_Title text);
                        insert into t_contents values (1, 'alpha title only');
                        ",
                )
                .unwrap();
            write_dense_body_db(root.join("body.db"), true, true, false);
        }
        DenseSidecarFixture::ShardedTContentsBodyRows => {
            write_sharded_t_contents_body_db(root.join("body.db"));
        }
        DenseSidecarFixture::BlobBodyRows => {
            write_dense_body_db(root.join("body.db"), true, true, true);
        }
        DenseSidecarFixture::PlainOnlyBodyRows => {
            write_dense_plain_body_db(root.join("body.db"));
        }
        DenseSidecarFixture::EntityTitleRows => {
            write_dense_body_db_with_entity_title(root.join("body.db"));
        }
        DenseSidecarFixture::CjkTitleRows => {
            write_dense_body_db_with_cjk_titles(root.join("body.db"));
        }
        DenseSidecarFixture::AsciiMarkerTitleRows => {
            write_dense_body_db_with_ascii_marker_titles(root.join("body.db"));
        }
        DenseSidecarFixture::MissingBetaRow => {
            write_dense_body_db(root.join("body.db"), true, false, false);
        }
        DenseSidecarFixture::OrderedHonbunRows => {
            write_ordered_honbun_db(root.join("vlpljblF"));
        }
    }

    SsedCatalog {
        title: "Dense".to_owned(),
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
    }
}

fn dense_anchor_record(anchor: &str) -> Vec<u8> {
    let mut record = Vec::new();
    record.extend_from_slice(&[0x1f, 0x09, 0x00, 0x01, 0x1f, 0x41]);
    record.extend_from_slice(&body_jis(anchor));
    record.extend_from_slice(&[0x1f, 0x61, 0x1f, 0x0a]);
    record.resize(32, 0);
    record
}

fn ordered_honbun_entry_record(label: &str, lines: &[&str]) -> Vec<u8> {
    let mut record = Vec::new();
    record.extend_from_slice(&SSED_ENTRY_MARKER);
    record.extend_from_slice(&[0x1f, 0x41, 0x01, 0x60, 0x1f, 0x04]);
    record.extend_from_slice(&body_jis(label));
    record.extend_from_slice(&[0x1f, 0x05, 0x1f, 0x61, 0x1f, 0x0a]);
    for line in lines {
        record.extend_from_slice(&[0x1f, 0x09, 0x00, 0x02]);
        record.extend_from_slice(&body_jis(line));
        record.extend_from_slice(&[0x1f, 0x0a]);
    }
    record
}

fn write_simple_index_row(
    page: &mut [u8],
    pos: &mut usize,
    key: &[u8],
    body_block: u32,
    body_offset: u16,
    title_block: u32,
    title_offset: u16,
) {
    page[*pos] = u8::try_from(key.len()).unwrap();
    *pos += 1;
    page[*pos..*pos + key.len()].copy_from_slice(key);
    *pos += key.len();
    page[*pos..*pos + 4].copy_from_slice(&body_block.to_be_bytes());
    page[*pos + 4..*pos + 6].copy_from_slice(&body_offset.to_be_bytes());
    page[*pos + 6..*pos + 10].copy_from_slice(&title_block.to_be_bytes());
    page[*pos + 10..*pos + 12].copy_from_slice(&title_offset.to_be_bytes());
    *pos += 12;
}

fn write_dense_body_db(path: PathBuf, alpha: bool, beta: bool, blob: bool) {
    let connection = Connection::open(path).unwrap();
    connection
            .execute_batch(
                "create table t_contents (f_DataId integer primary key, f_Title blob, f_Html blob, f_Plane blob);",
            )
            .unwrap();
    if alpha {
        connection
            .execute(
                "insert into t_contents values (?, ?, ?, ?)",
                (
                    1,
                    "alpha".as_bytes(),
                    "<div>alpha sidecar html</div>".as_bytes(),
                    "alpha sidecar body".as_bytes(),
                ),
            )
            .unwrap();
    }
    if beta {
        if blob {
            connection
                .execute(
                    "insert into t_contents values (?, ?, ?, ?)",
                    (
                        2,
                        cp932("ベータ"),
                        cp932("<div>ベータ html</div>"),
                        cp932("ベータ body"),
                    ),
                )
                .unwrap();
        } else {
            connection
                .execute(
                    "insert into t_contents values (?, ?, ?, ?)",
                    (
                        2,
                        "beta".as_bytes(),
                        "<div>beta sidecar html</div>".as_bytes(),
                        "beta sidecar body".as_bytes(),
                    ),
                )
                .unwrap();
        }
    }
}

fn write_dense_shared_body_db(path: PathBuf) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "
            create table t_contents (
              f_DataId integer primary key,
              f_Title text,
              f_Html text,
              f_Plane text
            );
            insert into t_contents values (1, 'alpha', '<div>alpha html</div>', 'alpha body');
            insert into t_contents values (2, 'shared first', '<div>first html</div>', 'shared sidecar body first');
            insert into t_contents values (3, 'shared second', '<div>second html</div>', 'shared sidecar body second');
            insert into t_contents values (4, 'shared third', '<div>third html</div>', 'shared sidecar body third');
            ",
        )
        .unwrap();
}

fn write_dense_plain_body_db(path: PathBuf) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "create table t_contents (f_DataId integer primary key, f_Title text, f_Html text, f_Plane text);",
        )
        .unwrap();
    connection
        .execute(
            "insert into t_contents values (?, ?, ?, ?)",
            (1, "alpha", "", "alpha plain body\nsecond line"),
        )
        .unwrap();
}

fn write_dense_body_db_with_lved_links(path: PathBuf) {
    let connection = Connection::open(path).unwrap();
    connection
            .execute_batch(
                "create table t_contents (f_DataId integer primary key, f_Title blob, f_Html blob, f_Plane blob);",
            )
            .unwrap();
    connection
        .execute(
            "insert into t_contents values (?, ?, ?, ?)",
            (
                1,
                "alpha".as_bytes(),
                "<div>alpha linked sidecar html</div>".as_bytes(),
                "alpha sidecar body".as_bytes(),
            ),
        )
        .unwrap();
    connection
        .execute(
            "insert into t_contents values (?, ?, ?, ?)",
            (
                2,
                "beta".as_bytes(),
                r#"<div>beta <img src="b129.png" class="icon"><img src="b159_M.png"><img src="sidecar_pic.png"><img src="furoku01_01.jpg"><object data = "KG003173.svg"></object> <a href="lved.ziptomedia:000010.wav">sound</a> <a href="lved.dataid:00000001">alpha</a> <a href="lved.dataid.result:00000002#spot">self</a> <a href="lved.addr=00000064:0000">alpha address</a></div>"#.as_bytes(),
                "beta sidecar body".as_bytes(),
            ),
        )
        .unwrap();
    connection
        .execute_batch(
            "
                create table media (No integer primary key, f_name text, f_type integer, f_main blob);
                insert into media values (1, 'sidecar_pic', 1, x'FFD8FFE0');
                ",
        )
        .unwrap();
}

fn write_dense_body_db_with_entity_title(path: PathBuf) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "create table t_contents (f_DataId integer primary key, f_Title blob, f_Html blob, f_Plane blob);",
        )
        .unwrap();
    connection
        .execute(
            "insert into t_contents values (?, ?, ?, ?)",
            (
                2,
                "<span>&amp;#x00E0; &#x002A;abaisser</span>".as_bytes(),
                "<div>entity sidecar html</div>".as_bytes(),
                "entity sidecar body".as_bytes(),
            ),
        )
        .unwrap();
    connection
        .execute(
            "insert into t_contents values (?, ?, ?, ?)",
            (
                3,
                "&#x002A;abaisser".as_bytes(),
                "<div>prefixed sidecar html</div>".as_bytes(),
                "prefixed sidecar body".as_bytes(),
            ),
        )
        .unwrap();
}

fn write_dense_body_db_with_cjk_titles(path: PathBuf) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "create table t_contents (f_DataId integer primary key, f_Title text, f_Html text, f_Plane text);
             insert into t_contents values (1, '丂', '<div>丂 html</div>', '丂 body');
             insert into t_contents values (2, '新', '<div>新 html</div>', '新 body');",
        )
        .unwrap();
}

fn write_dense_body_db_with_ascii_marker_titles(path: PathBuf) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "create table t_contents (f_DataId integer primary key, f_Title text, f_Html text, f_Plane text);
             insert into t_contents values (1, 'Alpha', '<div>Alpha html</div>', 'Alpha body');
             insert into t_contents values (2, '-a', '<div>-a html</div>', '-a body');",
        )
        .unwrap();
}

fn write_sharded_t_contents_body_db(path: PathBuf) {
    let connection = Connection::open(path).unwrap();
    connection
            .execute_batch(
                "
                create table t_contents_1 (f_DataId text primary key, f_Title text, f_Html text);
                create table t_contents_2 (f_DataId text primary key, f_Title text, f_Html text);
                insert into t_contents_2 values ('00000002', 'beta', '<div>beta sharded html</div>');
                ",
            )
            .unwrap();
}

fn write_android_body_db(path: PathBuf, table: &str) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(&format!(
            "create table {} (Html text);",
            quote_fixture_sql_identifier(table)
        ))
        .unwrap();
    connection
        .execute(
            &format!(
                "insert into {} (Html) values (?), (?)",
                quote_fixture_sql_identifier(table)
            ),
            (
                "<div>android alpha html</div>",
                "<div>android beta html</div>",
            ),
        )
        .unwrap();
}

fn write_ordered_honbun_db(path: PathBuf) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "
            create table HONBUN (
              ID text primary key,
              Title_UTF8 text,
              Contents_HTML_box text
            );
            insert into HONBUN values ('00000000', 'alpha', '<div>ordered alpha html</div>');
            insert into HONBUN values ('00000001', 'beta', '<div>ordered beta html</div>');
            ",
        )
        .unwrap();
}

fn write_block_offset_body_db(path: PathBuf, table: &str, body: &str) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(&format!(
            "
            create table {} (
              No integer primary key,
              Block integer,
              Offset integer,
              Title text,
              Body text,
              TitleJIS text
            );
            insert into {} values (
              1,
              100,
              4,
              'sidecar title',
              '{}',
              'sidecar title'
            );
            ",
            quote_fixture_sql_identifier(table),
            quote_fixture_sql_identifier(table),
            body.replace('\'', "''")
        ))
        .unwrap();
}

fn quote_fixture_sql_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn cp932(value: &str) -> Vec<u8> {
    let (encoded, _encoding, _had_errors) = SHIFT_JIS.encode(value);
    encoded.into_owned()
}

fn body_jis(value: &str) -> Vec<u8> {
    value
        .chars()
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
            cp932(&body_ch.to_string())
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
    let first = row + 0x21;
    let second = cell + 0x21;
    ((0x21..=0x7e).contains(&first) && (0x21..=0x7e).contains(&second)).then(|| vec![first, second])
}

fn screen_menu_image_control(width: u32, height: u32, block: u32, offset: u32) -> Vec<u8> {
    let mut payload = vec![0u8; 20];
    payload[0] = 0x1f;
    payload[1] = 0x4d;
    payload[10..12].copy_from_slice(&bcd_word(width));
    payload[12..14].copy_from_slice(&bcd_word(height));
    payload[14..18].copy_from_slice(&bcd_u32(block));
    payload[18..20].copy_from_slice(&bcd_word(offset));
    payload
}

fn screen_menu_hotspot_control(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    block: u32,
    offset: u32,
) -> Vec<u8> {
    let mut payload = vec![0u8; 36];
    payload[0] = 0x1f;
    payload[1] = 0x4f;
    payload[8..10].copy_from_slice(&bcd_word(x));
    payload[10..12].copy_from_slice(&bcd_word(y));
    payload[12..14].copy_from_slice(&bcd_word(width));
    payload[14..16].copy_from_slice(&bcd_word(height));
    payload[28..32].copy_from_slice(&bcd_u32(block));
    payload[32..34].copy_from_slice(&bcd_word(offset));
    payload
}

fn bcd_word(value: u32) -> [u8; 2] {
    let s = format!("{value:04}");
    [
        ((s.as_bytes()[0] - b'0') << 4) | (s.as_bytes()[1] - b'0'),
        ((s.as_bytes()[2] - b'0') << 4) | (s.as_bytes()[3] - b'0'),
    ]
}

fn bcd_u32(value: u32) -> [u8; 4] {
    let s = format!("{value:08}");
    [
        ((s.as_bytes()[0] - b'0') << 4) | (s.as_bytes()[1] - b'0'),
        ((s.as_bytes()[2] - b'0') << 4) | (s.as_bytes()[3] - b'0'),
        ((s.as_bytes()[4] - b'0') << 4) | (s.as_bytes()[5] - b'0'),
        ((s.as_bytes()[6] - b'0') << 4) | (s.as_bytes()[7] - b'0'),
    ]
}

fn encrypt_logofont_cipher_for_test(data: &[u8]) -> Vec<u8> {
    let digest = Sha256::digest(b"LogoFontCipher");
    let key = &digest[..16];
    let mut previous = [0_u8; 16];
    previous.copy_from_slice(&digest[16..32]);
    let cipher = Aes128::new_from_slice(key).unwrap();
    let mut padded = data.to_vec();
    let padding = 16 - (padded.len() % 16);
    padded.extend(std::iter::repeat_n(padding as u8, padding));
    let mut encrypted = Vec::with_capacity(padded.len());
    for chunk in padded.chunks_exact(16) {
        let mut block = [0_u8; 16];
        for index in 0..16 {
            block[index] = chunk[index] ^ previous[index];
        }
        let mut block = aes::Block::from(block);
        cipher.encrypt_block(&mut block);
        previous.copy_from_slice(&block);
        encrypted.extend_from_slice(&block);
    }
    encrypted
}

fn pcmdata_wave_chunks_for_test(format_tag: u16, data: &[u8]) -> Vec<u8> {
    let mut fmt_payload = Vec::new();
    fmt_payload.extend_from_slice(&format_tag.to_le_bytes());
    fmt_payload.extend_from_slice(&1_u16.to_le_bytes());
    fmt_payload.extend_from_slice(&8000_u32.to_le_bytes());
    fmt_payload.extend_from_slice(&8000_u32.to_le_bytes());
    fmt_payload.extend_from_slice(&1_u16.to_le_bytes());
    fmt_payload.extend_from_slice(&8_u16.to_le_bytes());

    let mut chunks = Vec::new();
    chunks.extend_from_slice(b"fmt ");
    chunks.extend_from_slice(&(fmt_payload.len() as u32).to_le_bytes());
    chunks.extend_from_slice(&fmt_payload);
    chunks.extend_from_slice(b"data");
    chunks.extend_from_slice(&(data.len() as u32).to_le_bytes());
    chunks.extend_from_slice(data);
    chunks
}

fn pcmdata_range_control_for_test(
    start_block: u32,
    start_offset: u32,
    end_block: u32,
    end_offset: u32,
) -> Vec<u8> {
    let mut control = vec![0x1f, 0x4a, 0x00, 0x01, 0x00, 0x00];
    control.extend_from_slice(&bcd_decimal_for_test(start_block, 4));
    control.extend_from_slice(&bcd_decimal_for_test(start_offset, 2));
    control.extend_from_slice(&bcd_decimal_for_test(end_block, 4));
    control.extend_from_slice(&bcd_decimal_for_test(end_offset, 2));
    control
}

fn simple_index_page_for_test(rows: &[(&[u8], u32, u16)]) -> Vec<u8> {
    let mut page = vec![0_u8; crate::ssed::BLOCK_SIZE as usize];
    page[0..2].copy_from_slice(&0xc000_u16.to_be_bytes());
    page[2..4].copy_from_slice(&(rows.len() as u16).to_be_bytes());
    let mut pos = 4usize;
    for (key, block, offset) in rows {
        page[pos] = key.len() as u8;
        pos += 1;
        page[pos..pos + key.len()].copy_from_slice(key);
        pos += key.len();
        page[pos..pos + 4].copy_from_slice(&block.to_be_bytes());
        pos += 4;
        page[pos..pos + 2].copy_from_slice(&offset.to_be_bytes());
        pos += 2;
        page[pos..pos + 4].copy_from_slice(&0_u32.to_be_bytes());
        pos += 4;
        page[pos..pos + 2].copy_from_slice(&0_u16.to_be_bytes());
        pos += 2;
    }
    page
}

fn bcd_decimal_for_test(mut value: u32, bytes: usize) -> Vec<u8> {
    let mut out = vec![0_u8; bytes];
    for byte in out.iter_mut().rev() {
        let low = value % 10;
        value /= 10;
        let high = value % 10;
        value /= 10;
        *byte = ((high as u8) << 4) | low as u8;
    }
    out
}

fn fixture_sseddata_literal_chunks(chunks: &[&[u8]], start_block: u32, end_block: u32) -> Vec<u8> {
    let chunk_count = chunks.len();
    let first_chunk_offset = 0x40 + chunk_count * 4;
    let mut data = vec![0u8; first_chunk_offset];
    data[..8].copy_from_slice(SSEDDATA_MAGIC);
    data[0x0f] = 1;
    data[0x16..0x18].copy_from_slice(&(chunk_count as u16).to_be_bytes());
    data[0x18..0x1c].copy_from_slice(&start_block.to_be_bytes());
    data[0x1c..0x20].copy_from_slice(&end_block.to_be_bytes());

    let mut compressed_chunks = Vec::with_capacity(chunk_count);
    let mut next_offset = first_chunk_offset;
    for (index, chunk) in chunks.iter().enumerate() {
        data[0x40 + index * 4..0x44 + index * 4]
            .copy_from_slice(&(next_offset as u32).to_be_bytes());
        let compressed = fixture_sseddata_literal_chunk(chunk);
        next_offset += compressed.len();
        compressed_chunks.push(compressed);
    }
    for compressed in compressed_chunks {
        data.extend_from_slice(&compressed);
    }
    data
}

fn fixture_sseddata_literal_chunk(literals: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&[0, 0]);
    chunk.extend_from_slice(&(literals.len() as u16).to_be_bytes());
    chunk.push(0);
    for literal in literals {
        chunk.extend_from_slice(&[0, 0, *literal]);
    }
    chunk
}

fn write_lved_search_fixture(root: &Path) {
    let payload = root.join("main.data");
    let key = "test-key";
    {
        let connection = Connection::open(&payload).unwrap();
        apply_sqlcipher_key(&connection, key).unwrap();
        connection
                .execute_batch(
                    "
                    create table info (id integer, type integer, name text primary key, body text, media text);
                    insert into info values (1, 1, 'about.html', '<h1>Example Dictionary 第2版</h1>', '');
                    insert into info values (2, 1, 'help.html', '<h1>Help</h1>', '');
                    create table content (id integer primary key, type integer, body text, media text);
                    create table media (id integer primary key, name text, type integer, main blob);
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
                    insert into content values (100, 1, '<article><h1>Alpha</h1><p>body</p><object class=\"icon\" data = \"AC6E.svg\"></object><a href = \"lved.media.sound:00010033.mp3\">sound</a><a href = \"lved.dataid:101#jump\">next</a><a href = \"lved.info:help.html#top\">help</a></article>', '');
                    insert into content values (101, 1, '<article><h1>Beta</h1></article>', '');
                    insert into content values (102, 1, '<article><h1>Gamma</h1></article>', '');
                    insert into media values (1, 'AC6E', 4, X'3C7376672F3E');
                    insert into mediasub values (1, '00010033', 5, X'49443303');
                    insert into list values (1, 100, 1, 'body-anchor', '<img class=\"icon\" src = \"AC6E.svg\"><b>alpha</b>', '<span>subtitle</span>');
                    insert into list values (2, 101, 1, '', '<b>beta</b>', '');
                    insert into list values (3, 102, 1, '', '<b>gamma</b>', '');
                    insert into search(rowid, forward, back, part, fts, advanced1, advanced2, filter)
                      values (1, 'alpha', 'ahpla', 'alpha', 'alpha body', '', '', '∥alpha∥');
                    ",
                )
                .unwrap();
    }
    fs::write(root.join("main.key"), key).unwrap();
}

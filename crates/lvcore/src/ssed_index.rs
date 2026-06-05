use serde::{Deserialize, Serialize};

use crate::ssed::BLOCK_SIZE;

mod text;

pub(crate) use text::decode_jis_pair;
pub use text::{decode_index_key, decode_title_text};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedIndexPointer {
    pub block: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedIndexRow {
    pub component: String,
    pub page_index: u32,
    pub logical_block: u32,
    pub row_index: u32,
    pub key: String,
    pub raw_key: Vec<u8>,
    pub target_key: String,
    pub body: SsedIndexPointer,
    pub title: SsedIndexPointer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsedIndexInternalRow {
    pub component: String,
    pub page_index: u32,
    pub logical_block: u32,
    pub row_index: u32,
    pub key: String,
    pub child_block: u32,
    pub raw_key: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SsedIndexScanState {
    current_key: Option<String>,
    current_title: Option<SsedIndexPointer>,
    current_count_hint: Option<u32>,
}

pub fn is_supported_index_type(component_type: u8) -> bool {
    matches!(
        component_type,
        0x30 | 0x60 | 0x70 | 0x71 | 0x72 | 0x80 | 0x81 | 0x90 | 0x91 | 0x92 | 0xa1
    )
}

pub fn is_simple_leaf_index_type(component_type: u8) -> bool {
    matches!(component_type, 0x71 | 0x72 | 0x91 | 0x92)
}

pub fn is_body_only_simple_leaf_index_type(component_type: u8) -> bool {
    component_type == 0x60
}

pub fn is_body_only_tagged_leaf_index_type(component_type: u8) -> bool {
    component_type == 0x30
}

pub fn is_tagged_leaf_index_type(component_type: u8) -> bool {
    matches!(component_type, 0x70 | 0x90)
}

pub fn is_kw_leaf_index_type(component_type: u8) -> bool {
    component_type == 0x80
}

pub fn is_cr_leaf_index_type(component_type: u8) -> bool {
    component_type == 0x81
}

pub fn is_multi_leaf_index_type(component_type: u8) -> bool {
    component_type == 0xa1
}

pub fn is_leaf_page(page_word: u16) -> bool {
    page_word & 0x8000 != 0
}

pub fn internal_slot_size(page_word: u16) -> usize {
    usize::from(page_word & 0x00ff) + 4
}

pub fn parse_internal_page(
    component: &str,
    page: &[u8],
    page_index: u32,
    logical_block: u32,
) -> Vec<SsedIndexInternalRow> {
    if page.len() < 4 {
        return Vec::new();
    }
    let word = be16(page, 0);
    let count = be16(page, 2);
    let slot = internal_slot_size(word);
    if slot < 6 {
        return Vec::new();
    }
    let mut pos = 4usize;
    let mut rows = Vec::new();
    for row_index in 1..=u32::from(count) {
        if pos + slot > page.len() {
            break;
        }
        let row = &page[pos..pos + slot];
        let raw_key = row[..slot - 4]
            .split(|value| *value == 0)
            .next()
            .unwrap_or(&[]);
        rows.push(SsedIndexInternalRow {
            component: component.to_owned(),
            page_index,
            logical_block,
            row_index,
            key: decode_index_key(raw_key),
            child_block: be32(row, slot - 4),
            raw_key: raw_key.to_vec(),
        });
        pos += slot;
    }
    rows
}

pub fn parse_simple_leaf_page(
    component: &str,
    page: &[u8],
    page_index: u32,
    logical_block: u32,
) -> (Vec<SsedIndexRow>, usize) {
    if page.len() < 4 {
        return (Vec::new(), 1);
    }
    let count = be16(page, 2);
    let mut pos = 4usize;
    let mut rows = Vec::new();
    let mut unknown = 0usize;

    for row_index in 1..=u32::from(count) {
        if pos >= page.len() {
            break;
        }
        let key_len = page[pos] as usize;
        if key_len == 0 {
            if page[pos..page.len().min(pos + 13)]
                .iter()
                .any(|value| *value != 0)
            {
                while pos + 13 <= page.len() && page[pos..pos + 13].iter().any(|value| *value != 0)
                {
                    let body = SsedIndexPointer {
                        block: be32(page, pos),
                        offset: u32::from(be16(page, pos + 4)),
                    };
                    let title = SsedIndexPointer {
                        block: be32(page, pos + 7),
                        offset: u32::from(be16(page, pos + 11)),
                    };
                    rows.push(SsedIndexRow {
                        component: component.to_owned(),
                        page_index,
                        logical_block,
                        row_index: rows.len() as u32 + 1,
                        key: String::new(),
                        raw_key: Vec::new(),
                        target_key: String::new(),
                        body,
                        title,
                    });
                    pos += 13;
                }
            }
            break;
        }
        pos += 1;
        if pos + key_len + 12 > page.len() {
            unknown += 1;
            break;
        }
        let raw_key = page[pos..pos + key_len].to_vec();
        let key = decode_index_key(&raw_key);
        pos += key_len;
        let body = SsedIndexPointer {
            block: be32(page, pos),
            offset: u32::from(be16(page, pos + 4)),
        };
        let title = SsedIndexPointer {
            block: be32(page, pos + 6),
            offset: u32::from(be16(page, pos + 10)),
        };
        pos += 12;
        rows.push(SsedIndexRow {
            component: component.to_owned(),
            page_index,
            logical_block,
            row_index,
            key: key.clone(),
            raw_key,
            target_key: key,
            body,
            title,
        });
    }

    (rows, unknown)
}

pub fn parse_supported_leaf_page(
    component: &str,
    component_type: u8,
    page: &[u8],
    page_index: u32,
    logical_block: u32,
    state: &mut SsedIndexScanState,
) -> (Vec<SsedIndexRow>, usize) {
    if is_simple_leaf_index_type(component_type) {
        return parse_simple_leaf_page(component, page, page_index, logical_block);
    }
    if is_body_only_simple_leaf_index_type(component_type) {
        return parse_body_only_simple_leaf_page(component, page, page_index, logical_block);
    }
    if is_body_only_tagged_leaf_index_type(component_type) {
        return parse_tagged_leaf_page(
            component,
            page,
            page_index,
            logical_block,
            state,
            TaggedLeafLayout::BodyOnly,
        );
    }
    if is_tagged_leaf_index_type(component_type) {
        return parse_tagged_leaf_page(
            component,
            page,
            page_index,
            logical_block,
            state,
            TaggedLeafLayout::BodyAndTitle,
        );
    }
    if is_kw_leaf_index_type(component_type) {
        return parse_kw_leaf_page(component, page, page_index, logical_block, state);
    }
    if is_cr_leaf_index_type(component_type) {
        return parse_cr_leaf_page(component, page, page_index, logical_block, state);
    }
    if is_multi_leaf_index_type(component_type) {
        return parse_multi_leaf_page(component, page, page_index, logical_block, state);
    }
    (Vec::new(), 0)
}

pub fn parse_supported_leaf_page_body_pointers(
    component_type: u8,
    page: &[u8],
) -> Option<(Vec<SsedIndexPointer>, usize)> {
    if is_simple_leaf_index_type(component_type) {
        return Some(parse_simple_leaf_page_body_pointers(page));
    }
    if is_body_only_simple_leaf_index_type(component_type) {
        return Some(parse_body_only_simple_leaf_page_body_pointers(page));
    }
    if is_body_only_tagged_leaf_index_type(component_type) {
        return Some(parse_tagged_leaf_page_body_pointers(
            page,
            TaggedLeafLayout::BodyOnly,
        ));
    }
    if is_tagged_leaf_index_type(component_type) {
        return Some(parse_tagged_leaf_page_body_pointers(
            page,
            TaggedLeafLayout::BodyAndTitle,
        ));
    }
    None
}

fn parse_simple_leaf_page_body_pointers(page: &[u8]) -> (Vec<SsedIndexPointer>, usize) {
    if page.len() < 4 {
        return (Vec::new(), 1);
    }
    let count = be16(page, 2);
    let mut pos = 4usize;
    let mut pointers = Vec::new();
    let mut unknown = 0usize;

    for _ in 1..=u32::from(count) {
        if pos >= page.len() {
            break;
        }
        let key_len = page[pos] as usize;
        if key_len == 0 {
            if page[pos..page.len().min(pos + 13)]
                .iter()
                .any(|value| *value != 0)
            {
                while pos + 13 <= page.len() && page[pos..pos + 13].iter().any(|value| *value != 0)
                {
                    pointers.push(SsedIndexPointer {
                        block: be32(page, pos),
                        offset: u32::from(be16(page, pos + 4)),
                    });
                    pos += 13;
                }
            }
            break;
        }
        pos += 1;
        if pos + key_len + 12 > page.len() {
            unknown += 1;
            break;
        }
        pos += key_len;
        pointers.push(SsedIndexPointer {
            block: be32(page, pos),
            offset: u32::from(be16(page, pos + 4)),
        });
        pos += 12;
    }

    (pointers, unknown)
}

fn parse_body_only_simple_leaf_page_body_pointers(page: &[u8]) -> (Vec<SsedIndexPointer>, usize) {
    if page.len() < 4 {
        return (Vec::new(), 1);
    }
    let count = be16(page, 2);
    let mut pos = 4usize;
    let mut pointers = Vec::new();
    let mut unknown = 0usize;
    for _ in 1..=u32::from(count) {
        if pos >= page.len() || page[pos] == 0 {
            break;
        }
        let key_len = page[pos] as usize;
        pos += 1;
        if pos + key_len + 6 > page.len() {
            unknown += 1;
            break;
        }
        pos += key_len;
        pointers.push(SsedIndexPointer {
            block: be32(page, pos),
            offset: u32::from(be16(page, pos + 4)),
        });
        pos += 6;
    }
    (pointers, unknown)
}

fn parse_tagged_leaf_page_body_pointers(
    page: &[u8],
    layout: TaggedLeafLayout,
) -> (Vec<SsedIndexPointer>, usize) {
    if page.len() < 4 {
        return (Vec::new(), 1);
    }
    let count = be16(page, 2);
    let mut pos = 4usize;
    let mut pointers = Vec::new();
    let mut unknown = 0usize;
    let mut subrecord = 0u16;

    while subrecord < count && pos + 2 <= page.len() {
        let tag = page[pos];
        let key_len = page[pos + 1] as usize;
        if tag == 0 && key_len == 0 {
            break;
        }
        pos += 2;

        match tag {
            0x00 | 0xc0 => {
                let pointer_len = match layout {
                    TaggedLeafLayout::BodyOnly => 6,
                    TaggedLeafLayout::BodyAndTitle => 12,
                };
                if pos + key_len + pointer_len > page.len() {
                    unknown += 1;
                    break;
                }
                pos += key_len;
                pointers.push(read_body_pointer(page, pos));
                pos += pointer_len;
            }
            0x80 => {
                if pos + 2 + key_len > page.len() {
                    unknown += 1;
                    break;
                }
                pos += 2 + key_len;
            }
            _ => {
                unknown += 1;
                break;
            }
        }
        subrecord += 1;
    }
    (pointers, unknown)
}

fn parse_body_only_simple_leaf_page(
    component: &str,
    page: &[u8],
    page_index: u32,
    logical_block: u32,
) -> (Vec<SsedIndexRow>, usize) {
    if page.len() < 4 {
        return (Vec::new(), 1);
    }
    let count = be16(page, 2);
    let mut pos = 4usize;
    let mut rows = Vec::new();
    let mut unknown = 0usize;
    for row_index in 1..=u32::from(count) {
        if pos >= page.len() || page[pos] == 0 {
            break;
        }
        let key_len = page[pos] as usize;
        pos += 1;
        if pos + key_len + 6 > page.len() {
            unknown += 1;
            break;
        }
        let raw_key = page[pos..pos + key_len].to_vec();
        let key = decode_index_key(&raw_key);
        pos += key_len;
        let body = SsedIndexPointer {
            block: be32(page, pos),
            offset: u32::from(be16(page, pos + 4)),
        };
        pos += 6;
        rows.push(SsedIndexRow {
            component: component.to_owned(),
            page_index,
            logical_block,
            row_index,
            key: key.clone(),
            raw_key,
            target_key: key,
            body,
            title: body,
        });
    }
    (rows, unknown)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaggedLeafLayout {
    BodyOnly,
    BodyAndTitle,
}

fn parse_tagged_leaf_page(
    component: &str,
    page: &[u8],
    page_index: u32,
    logical_block: u32,
    state: &mut SsedIndexScanState,
    layout: TaggedLeafLayout,
) -> (Vec<SsedIndexRow>, usize) {
    if page.len() < 4 {
        return (Vec::new(), 1);
    }
    let count = be16(page, 2);
    let mut pos = 4usize;
    let mut rows = Vec::new();
    let mut unknown = 0usize;
    let mut subrecord = 0u16;

    while subrecord < count && pos + 2 <= page.len() {
        let tag = page[pos];
        let key_len = page[pos + 1] as usize;
        if tag == 0 && key_len == 0 {
            break;
        }
        pos += 2;

        match tag {
            0x00 => {
                let pointer_len = match layout {
                    TaggedLeafLayout::BodyOnly => 6,
                    TaggedLeafLayout::BodyAndTitle => 12,
                };
                if pos + key_len + pointer_len > page.len() {
                    unknown += 1;
                    break;
                }
                let raw_key = page[pos..pos + key_len].to_vec();
                let key = decode_index_key(&raw_key);
                pos += key_len;
                let body = read_body_pointer(page, pos);
                let title = match layout {
                    TaggedLeafLayout::BodyOnly => body,
                    TaggedLeafLayout::BodyAndTitle => read_title_pointer(page, pos),
                };
                pos += pointer_len;
                rows.push(SsedIndexRow {
                    component: component.to_owned(),
                    page_index,
                    logical_block,
                    row_index: rows.len() as u32 + 1,
                    key: key.clone(),
                    raw_key,
                    target_key: key,
                    body,
                    title,
                });
            }
            0x80 => {
                if pos + 2 + key_len > page.len() {
                    unknown += 1;
                    break;
                }
                state.current_count_hint = Some(u32::from(be16(page, pos)));
                pos += 2;
                state.current_key = Some(decode_index_key(&page[pos..pos + key_len]));
                state.current_title = None;
                pos += key_len;
            }
            0xc0 => {
                let pointer_len = match layout {
                    TaggedLeafLayout::BodyOnly => 6,
                    TaggedLeafLayout::BodyAndTitle => 12,
                };
                if pos + key_len + pointer_len > page.len() {
                    unknown += 1;
                    break;
                }
                let target_key = decode_index_key(&page[pos..pos + key_len]);
                pos += key_len;
                let body = read_body_pointer(page, pos);
                let title = match layout {
                    TaggedLeafLayout::BodyOnly => body,
                    TaggedLeafLayout::BodyAndTitle => read_title_pointer(page, pos),
                };
                pos += pointer_len;
                let key = state
                    .current_key
                    .clone()
                    .unwrap_or_else(|| target_key.clone());
                rows.push(SsedIndexRow {
                    component: component.to_owned(),
                    page_index,
                    logical_block,
                    row_index: rows.len() as u32 + 1,
                    key,
                    raw_key: Vec::new(),
                    target_key,
                    body,
                    title,
                });
            }
            _ => {
                unknown += 1;
                break;
            }
        }
        subrecord += 1;
    }
    (rows, unknown)
}

fn parse_kw_leaf_page(
    component: &str,
    page: &[u8],
    page_index: u32,
    logical_block: u32,
    state: &mut SsedIndexScanState,
) -> (Vec<SsedIndexRow>, usize) {
    if page.len() < 4 {
        return (Vec::new(), 1);
    }
    let count = be16(page, 2);
    let mut pos = 4usize;
    let mut rows = Vec::new();
    let mut unknown = 0usize;
    let mut subrecord = 0u16;

    while subrecord < count && pos < page.len() {
        let tag = page[pos];
        if tag == 0 && (pos + 1 >= page.len() || page[pos + 1] == 0) {
            break;
        }
        match tag {
            0x00 => {
                if pos + 2 > page.len() {
                    unknown += 1;
                    break;
                }
                let key_len = page[pos + 1] as usize;
                if key_len == 0 {
                    break;
                }
                pos += 2;
                if pos + key_len + 12 > page.len() {
                    unknown += 1;
                    break;
                }
                let raw_key = page[pos..pos + key_len].to_vec();
                let key = decode_index_key(&raw_key);
                pos += key_len;
                let (body, title) = read_pointer_pair(page, pos);
                pos += 12;
                rows.push(SsedIndexRow {
                    component: component.to_owned(),
                    page_index,
                    logical_block,
                    row_index: rows.len() as u32 + 1,
                    key: key.clone(),
                    raw_key,
                    target_key: key,
                    body,
                    title,
                });
            }
            0x80 => {
                if pos + 6 > page.len() {
                    unknown += 1;
                    break;
                }
                let key_len = page[pos + 1] as usize;
                if pos + 6 + key_len + 6 > page.len() {
                    unknown += 1;
                    break;
                }
                state.current_count_hint = Some(be32(page, pos + 2));
                state.current_key = Some(decode_index_key(&page[pos + 6..pos + 6 + key_len]));
                pos += 6 + key_len;
                state.current_title = Some(SsedIndexPointer {
                    block: be32(page, pos),
                    offset: u32::from(be16(page, pos + 4)),
                });
                pos += 6;
            }
            0xb0 | 0xc0 => {
                if pos + 7 > page.len() {
                    unknown += 1;
                    break;
                }
                let body = SsedIndexPointer {
                    block: be32(page, pos + 1),
                    offset: u32::from(be16(page, pos + 5)),
                };
                let key = state.current_key.clone().unwrap_or_default();
                let title = state.current_title.unwrap_or(body);
                rows.push(SsedIndexRow {
                    component: component.to_owned(),
                    page_index,
                    logical_block,
                    row_index: rows.len() as u32 + 1,
                    key: key.clone(),
                    raw_key: Vec::new(),
                    target_key: key,
                    body,
                    title,
                });
                pos += 7;
            }
            _ => {
                unknown += 1;
                break;
            }
        }
        subrecord += 1;
    }
    (rows, unknown)
}

fn parse_cr_leaf_page(
    component: &str,
    page: &[u8],
    page_index: u32,
    logical_block: u32,
    state: &mut SsedIndexScanState,
) -> (Vec<SsedIndexRow>, usize) {
    if page.len() < 4 {
        return (Vec::new(), 1);
    }
    let count = be16(page, 2);
    let mut pos = 4usize;
    let mut rows = Vec::new();
    let mut unknown = 0usize;
    let mut subrecord = 0u16;

    while subrecord < count && pos + 2 <= page.len() {
        let first = page[pos];
        let second = page[pos + 1];
        if first == 0 && second == 0 {
            break;
        }
        match first {
            0x00 => {
                let key_len = second as usize;
                pos += 2;
                if pos + key_len + 12 > page.len() {
                    unknown += 1;
                    break;
                }
                let raw_key = page[pos..pos + key_len].to_vec();
                let key = decode_index_key(&raw_key);
                pos += key_len;
                let (body, title) = read_pointer_pair(page, pos);
                pos += 12;
                rows.push(SsedIndexRow {
                    component: component.to_owned(),
                    page_index,
                    logical_block,
                    row_index: rows.len() as u32 + 1,
                    key: key.clone(),
                    raw_key,
                    target_key: key,
                    body,
                    title,
                });
            }
            0x80 => {
                let key_len = second as usize;
                pos += 2;
                if pos + 4 + key_len + 6 > page.len() {
                    unknown += 1;
                    break;
                }
                state.current_count_hint = Some(be32(page, pos));
                pos += 4;
                state.current_key = Some(decode_index_key(&page[pos..pos + key_len]));
                pos += key_len;
                state.current_title = Some(SsedIndexPointer {
                    block: be32(page, pos),
                    offset: u32::from(be16(page, pos + 4)),
                });
                pos += 6;
            }
            0xc0 => {
                if pos + 7 > page.len() {
                    unknown += 1;
                    break;
                }
                let body = SsedIndexPointer {
                    block: be32(page, pos + 1),
                    offset: u32::from(be16(page, pos + 5)),
                };
                let key = state.current_key.clone().unwrap_or_default();
                let title = state.current_title.unwrap_or(body);
                rows.push(SsedIndexRow {
                    component: component.to_owned(),
                    page_index,
                    logical_block,
                    row_index: rows.len() as u32 + 1,
                    key: key.clone(),
                    raw_key: Vec::new(),
                    target_key: key,
                    body,
                    title,
                });
                pos += 7;
            }
            _ => {
                unknown += 1;
                break;
            }
        }
        subrecord += 1;
    }
    (rows, unknown)
}

fn parse_multi_leaf_page(
    component: &str,
    page: &[u8],
    page_index: u32,
    logical_block: u32,
    state: &mut SsedIndexScanState,
) -> (Vec<SsedIndexRow>, usize) {
    if page.len() < 4 {
        return (Vec::new(), 1);
    }
    let count = be16(page, 2);
    let mut pos = 4usize;
    let mut rows = Vec::new();
    let mut unknown = 0usize;
    let mut subrecord = 0u16;

    while subrecord < count && pos < page.len() {
        let tag = page[pos];
        if tag == 0 && (pos + 1 >= page.len() || page[pos + 1] == 0) {
            break;
        }
        match tag {
            0x00 => {
                if pos + 2 > page.len() {
                    unknown += 1;
                    break;
                }
                let key_len = page[pos + 1] as usize;
                if pos + 2 + key_len + 12 > page.len() {
                    unknown += 1;
                    break;
                }
                let raw_key = page[pos + 2..pos + 2 + key_len].to_vec();
                let key = decode_index_key(&raw_key);
                pos += 2 + key_len;
                let (body, title) = read_pointer_pair(page, pos);
                pos += 12;
                rows.push(SsedIndexRow {
                    component: component.to_owned(),
                    page_index,
                    logical_block,
                    row_index: rows.len() as u32 + 1,
                    key: key.clone(),
                    raw_key,
                    target_key: key,
                    body,
                    title,
                });
            }
            0x80 => {
                if pos + 6 > page.len() {
                    unknown += 1;
                    break;
                }
                let key_len = page[pos + 1] as usize;
                if pos + 6 + key_len > page.len() {
                    unknown += 1;
                    break;
                }
                state.current_count_hint = Some(be32(page, pos + 2));
                state.current_key = Some(decode_index_key(&page[pos + 6..pos + 6 + key_len]));
                state.current_title = None;
                pos += 6 + key_len;
            }
            0xc0 => {
                if pos + 13 > page.len() {
                    unknown += 1;
                    break;
                }
                let (body, title) = read_pointer_pair(page, pos + 1);
                let key = state.current_key.clone().unwrap_or_default();
                rows.push(SsedIndexRow {
                    component: component.to_owned(),
                    page_index,
                    logical_block,
                    row_index: rows.len() as u32 + 1,
                    key: key.clone(),
                    raw_key: Vec::new(),
                    target_key: key,
                    body,
                    title,
                });
                pos += 13;
            }
            _ => {
                unknown += 1;
                break;
            }
        }
        subrecord += 1;
    }
    (rows, unknown)
}

fn be16(data: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([data[offset], data[offset + 1]])
}

fn be32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn read_body_pointer(data: &[u8], pos: usize) -> SsedIndexPointer {
    SsedIndexPointer {
        block: be32(data, pos),
        offset: u32::from(be16(data, pos + 4)),
    }
}

fn read_title_pointer(data: &[u8], pos: usize) -> SsedIndexPointer {
    SsedIndexPointer {
        block: be32(data, pos + 6),
        offset: u32::from(be16(data, pos + 10)),
    }
}

fn read_pointer_pair(data: &[u8], pos: usize) -> (SsedIndexPointer, SsedIndexPointer) {
    (read_body_pointer(data, pos), read_title_pointer(data, pos))
}

pub const INDEX_PAGE_SIZE: usize = BLOCK_SIZE as usize;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_leaf_page() {
        let mut page = vec![0u8; INDEX_PAGE_SIZE];
        page[0..2].copy_from_slice(&0xc000u16.to_be_bytes());
        page[2..4].copy_from_slice(&1u16.to_be_bytes());
        page[4] = 2;
        page[5..7].copy_from_slice(&[0x24, 0x22]);
        page[7..19].copy_from_slice(&[0, 0, 0, 1, 0, 2, 0, 0, 0, 3, 0, 4]);

        let (rows, unknown) = parse_simple_leaf_page("FHINDEX.DIC", &page, 0, 100);

        assert_eq!(unknown, 0);
        assert_eq!(rows[0].key, "あ");
        assert_eq!(
            rows[0].body,
            SsedIndexPointer {
                block: 1,
                offset: 2
            }
        );
        assert_eq!(
            rows[0].title,
            SsedIndexPointer {
                block: 3,
                offset: 4
            }
        );
    }

    #[test]
    fn decodes_jis_pair_title_text() {
        assert_eq!(
            decode_title_text(&[0x1f, 0x09, 0x00, 0x01, 0x24, 0x22, 0x1f, 0x0a]),
            "あ"
        );
    }

    #[test]
    fn keeps_plain_ascii_title_text() {
        assert_eq!(decode_title_text(b"alpha\x1f\x0a"), "alpha");
        assert_eq!(decode_title_text(b"gamma\x1f\x0a"), "gamma");
        assert_eq!(decode_title_text(b"LOAN\x1f\x0a"), "LOAN");
        assert_eq!(decode_title_text(b"\ngamma\x1f\x0a"), "");
    }

    #[test]
    fn decodes_raw_jis_title_bytes_that_look_ascii() {
        assert_eq!(decode_title_text(b"BG?G\x1f\x0a"), "打診");
        assert_eq!(
            decode_title_text(b"\"~0-K!$bKtK!$J$j\x1f\x0a"),
            "◯悪法も又法なり"
        );
    }

    #[test]
    fn decodes_index_keys_that_are_plain_ascii_or_jis_like_ascii() {
        assert_eq!(decode_index_key(b"DOG"), "DOG");
        assert_eq!(decode_index_key(b"BG?G"), "打診");
    }

    #[test]
    fn decodes_title_halfwidth_span_and_drops_binary_gaiji_markers() {
        assert_eq!(
            decode_title_text(&[
                0xb4, 0x4f, 0x1f, 0x04, 0x23, 0x65, 0x23, 0x74, 0x1f, 0x05, 0x1f, 0x0a,
            ]),
            "et"
        );
        assert_eq!(
            decode_title_text(&[
                0xb4, 0x4f, 0x1f, 0x04, 0x23, 0x74, 0x23, 0x61, 0x23, 0x62, 0x23, 0x6c, 0x23, 0x69,
                0x1f, 0x0e, 0x23, 0x31, 0x1f, 0x0f, 0x1f, 0x05, 0x1f, 0x0a,
            ]),
            "tabli1"
        );
    }
}

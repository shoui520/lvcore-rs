use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ssed::{BLOCK_SIZE, SsedCatalog, SsedComponentRole};

const SCREEN_START: u8 = 0x4c;
const SCREEN_IMAGE: u8 = 0x4d;
const HOTSPOT: u8 = 0x4f;
const SCREEN_END: u8 = 0x6c;
const DIRECT_TARGET: u8 = 0x4b;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedScreenMenuParse {
    pub screens: Vec<SsedScreenMenuScreen>,
    pub stats: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedScreenMenuScreen {
    pub screen_index: u32,
    pub start_offset: usize,
    pub end_offset: Option<usize>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub image: Option<SsedScreenMenuPointer>,
    pub hotspots: Vec<SsedScreenMenuHotspot>,
    pub direct_targets: Vec<SsedScreenMenuDirectTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedScreenMenuHotspot {
    pub start_offset: usize,
    pub rect: SsedScreenMenuRect,
    pub destination: SsedScreenMenuPointer,
    pub target_screen_index: Option<u32>,
    pub target_direct_screen_index: Option<u32>,
    pub target_direct_index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedScreenMenuDirectTarget {
    pub start_offset: usize,
    pub destination: SsedScreenMenuPointer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedScreenMenuPointer {
    pub block: u32,
    pub offset: u32,
    pub target: Option<SsedScreenMenuPointerTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedScreenMenuPointerTarget {
    pub component: String,
    pub role: SsedComponentRole,
    pub relative_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsedScreenMenuRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

pub fn parse_screen_menu_stream(data: &[u8], catalog: Option<&SsedCatalog>) -> SsedScreenMenuParse {
    let mut screens = Vec::new();
    let mut current: Option<SsedScreenMenuScreen> = None;
    let mut last_closed: Option<usize> = None;
    let mut stats = BTreeMap::from([
        ("controls".to_owned(), 0),
        ("unknown_controls".to_owned(), 0),
        ("screens".to_owned(), 0),
        ("image_records".to_owned(), 0),
        ("hotspots".to_owned(), 0),
        ("direct_targets".to_owned(), 0),
        ("resolved_targets".to_owned(), 0),
        ("screen_targets".to_owned(), 0),
        ("body_targets".to_owned(), 0),
        ("media_image_targets".to_owned(), 0),
        ("unresolved_targets".to_owned(), 0),
        ("hotspot_screen_targets".to_owned(), 0),
        ("hotspot_direct_targets".to_owned(), 0),
        ("hotspot_unclassified_screen_menu_targets".to_owned(), 0),
    ]);

    let mut i = 0usize;
    while i + 1 < data.len() {
        if data[i] != 0x1f {
            i += 1;
            continue;
        }

        let op = data[i + 1];
        inc_stat(&mut stats, "controls");
        match op {
            SCREEN_START if i + 4 <= data.len() => {
                if let Some(mut screen) = current.take() {
                    screen.end_offset = Some(i);
                    screens.push(screen);
                    last_closed = Some(screens.len() - 1);
                }
                current = Some(SsedScreenMenuScreen {
                    screen_index: screens.len() as u32,
                    start_offset: i,
                    end_offset: None,
                    width: None,
                    height: None,
                    image: None,
                    hotspots: Vec::new(),
                    direct_targets: Vec::new(),
                });
                inc_stat(&mut stats, "screens");
                i += 4;
            }
            SCREEN_IMAGE if i + 20 <= data.len() => {
                let payload = &data[i + 2..i + 20];
                let width = parse_bcd_word(&payload[8..10]);
                let height = parse_bcd_word(&payload[10..12]);
                let image = parse_bcd_pointer(&payload[12..18], catalog);
                if let Some(screen) = current.as_mut() {
                    screen.width = width;
                    screen.height = height;
                    screen.image = image.clone();
                }
                if let Some(pointer) = &image {
                    count_pointer_target(&mut stats, pointer);
                }
                inc_stat(&mut stats, "image_records");
                i += 20;
            }
            HOTSPOT if i + 36 <= data.len() => {
                let payload = &data[i + 2..i + 36];
                let rect = parse_rect(&payload[6..14]);
                let destination = parse_bcd_pointer(&payload[26..32], catalog);
                match (current.as_mut(), rect, destination) {
                    (Some(screen), Some(rect), Some(destination)) => {
                        count_pointer_target(&mut stats, &destination);
                        screen.hotspots.push(SsedScreenMenuHotspot {
                            start_offset: i,
                            rect,
                            destination,
                            target_screen_index: None,
                            target_direct_screen_index: None,
                            target_direct_index: None,
                        });
                        inc_stat(&mut stats, "hotspots");
                    }
                    _ => inc_stat(&mut stats, "unresolved_targets"),
                }
                i += 36;
            }
            SCREEN_END => {
                if let Some(mut screen) = current.take() {
                    screen.end_offset = Some(i + 2);
                    screens.push(screen);
                    last_closed = Some(screens.len() - 1);
                }
                i += 2;
            }
            DIRECT_TARGET if i + 8 <= data.len() => {
                let destination = parse_bcd_pointer(&data[i + 2..i + 8], catalog);
                match (last_closed, destination) {
                    (Some(screen_index), Some(destination)) => {
                        count_pointer_target(&mut stats, &destination);
                        if let Some(screen) = screens.get_mut(screen_index) {
                            screen.direct_targets.push(SsedScreenMenuDirectTarget {
                                start_offset: i,
                                destination,
                            });
                            inc_stat(&mut stats, "direct_targets");
                        }
                    }
                    _ => inc_stat(&mut stats, "unresolved_targets"),
                }
                i += 8;
            }
            0x4b | 0x4c | 0x4d | 0x4f => {
                inc_stat(&mut stats, "unknown_controls");
                i += 2;
            }
            0x6b | 0x6d => {
                i += 2;
            }
            _ => {
                inc_stat(&mut stats, "unknown_controls");
                i += 2;
            }
        }
    }

    if let Some(mut screen) = current {
        screen.end_offset = Some(data.len());
        screens.push(screen);
    }
    annotate_screen_targets(&mut screens, &mut stats);
    SsedScreenMenuParse { screens, stats }
}

fn parse_bcd_pointer(
    payload: &[u8],
    catalog: Option<&SsedCatalog>,
) -> Option<SsedScreenMenuPointer> {
    if payload.len() != 6 {
        return None;
    }
    let block = decode_bcd_decimal(&payload[..4])?;
    let offset = decode_bcd_decimal(&payload[4..6])?;
    let target = catalog.and_then(|catalog| {
        catalog
            .components
            .iter()
            .find(|component| component.contains_block(block))
            .map(|component| {
                let relative_offset = u64::from(block - component.start_block)
                    * u64::from(BLOCK_SIZE)
                    + u64::from(offset);
                SsedScreenMenuPointerTarget {
                    component: component.filename.clone(),
                    role: component.role,
                    relative_offset,
                }
            })
    });
    Some(SsedScreenMenuPointer {
        block,
        offset,
        target,
    })
}

fn parse_rect(payload: &[u8]) -> Option<SsedScreenMenuRect> {
    if payload.len() != 8 {
        return None;
    }
    Some(SsedScreenMenuRect {
        x: parse_bcd_word(&payload[0..2])?,
        y: parse_bcd_word(&payload[2..4])?,
        width: parse_bcd_word(&payload[4..6])?,
        height: parse_bcd_word(&payload[6..8])?,
    })
}

fn parse_bcd_word(payload: &[u8]) -> Option<u32> {
    if payload.len() != 2 {
        return None;
    }
    decode_bcd_decimal(payload)
}

pub fn decode_bcd_decimal(data: &[u8]) -> Option<u32> {
    let mut value = 0u32;
    for byte in data {
        let high = byte >> 4;
        let low = byte & 0x0f;
        if high > 9 || low > 9 {
            return None;
        }
        value = value
            .saturating_mul(100)
            .saturating_add(u32::from(high) * 10 + u32::from(low));
    }
    Some(value)
}

fn count_pointer_target(stats: &mut BTreeMap<String, u32>, pointer: &SsedScreenMenuPointer) {
    if pointer.block == 0 && pointer.offset == 0 {
        return;
    }
    let Some(target) = &pointer.target else {
        inc_stat(stats, "unresolved_targets");
        return;
    };
    inc_stat(stats, "resolved_targets");
    match target.role {
        SsedComponentRole::ScreenMenu => inc_stat(stats, "screen_targets"),
        SsedComponentRole::Honmon => inc_stat(stats, "body_targets"),
        SsedComponentRole::Colscr => inc_stat(stats, "media_image_targets"),
        _ => {}
    }
}

fn annotate_screen_targets(
    screens: &mut [SsedScreenMenuScreen],
    stats: &mut BTreeMap<String, u32>,
) {
    let by_offset = screens
        .iter()
        .map(|screen| (screen.start_offset as u64, screen.screen_index))
        .collect::<BTreeMap<_, _>>();
    let direct_by_offset = screens
        .iter()
        .flat_map(|screen| {
            screen
                .direct_targets
                .iter()
                .enumerate()
                .map(move |(direct_index, direct)| {
                    (
                        direct.start_offset as u64,
                        (screen.screen_index, direct_index as u32),
                    )
                })
        })
        .collect::<BTreeMap<_, _>>();

    for screen in screens {
        for hotspot in &mut screen.hotspots {
            let Some(target) = &hotspot.destination.target else {
                continue;
            };
            if target.role != SsedComponentRole::ScreenMenu {
                continue;
            }
            if let Some(target_screen_index) = by_offset.get(&target.relative_offset) {
                hotspot.target_screen_index = Some(*target_screen_index);
                inc_stat(stats, "hotspot_screen_targets");
            } else if let Some((screen_index, direct_index)) =
                direct_by_offset.get(&target.relative_offset)
            {
                hotspot.target_direct_screen_index = Some(*screen_index);
                hotspot.target_direct_index = Some(*direct_index);
                inc_stat(stats, "hotspot_direct_targets");
            } else {
                inc_stat(stats, "hotspot_unclassified_screen_menu_targets");
            }
        }
    }
}

fn inc_stat(stats: &mut BTreeMap<String, u32>, key: &str) {
    *stats.entry(key.to_owned()).or_insert(0) += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssed::{SsedComponent, SsedInfoLayout};

    #[test]
    fn parses_screen_menu_hotspots_and_direct_targets() {
        let catalog = SsedCatalog {
            title: "screen".to_owned(),
            components: vec![
                component("SCRMENU.DIC", SsedComponentRole::ScreenMenu, 10, 10),
                component("HONMON.DIC", SsedComponentRole::Honmon, 20, 20),
                component("COLSCR.DIC", SsedComponentRole::Colscr, 30, 30),
            ],
            layout: SsedInfoLayout {
                component_count_offset: 0,
                record_start: 0,
                record_size: 0x30,
                component_count: 3,
                trailing_bytes: 0,
            },
        };
        let mut data = Vec::new();
        data.extend_from_slice(&[0x1f, SCREEN_START, 0x00, 0x00]);
        data.extend_from_slice(&[
            0x1f,
            SCREEN_IMAGE,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0x08,
            0x00,
            0x06,
            0x00,
            0x00,
            0x00,
            0x00,
            0x30,
            0x00,
            0x00,
        ]);
        data.extend_from_slice(&hotspot(1, 2, 3, 4, 10, 62));
        data.extend_from_slice(&[0x1f, SCREEN_END]);
        let direct_offset = data.len();
        data.extend_from_slice(&[0x1f, DIRECT_TARGET, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00]);

        let parsed = parse_screen_menu_stream(&data, Some(&catalog));

        assert_eq!(parsed.screens.len(), 1);
        assert_eq!(parsed.screens[0].width, Some(800));
        assert_eq!(parsed.screens[0].height, Some(600));
        assert_eq!(
            parsed.screens[0]
                .image
                .as_ref()
                .and_then(|image| image.target.as_ref())
                .map(|target| target.role),
            Some(SsedComponentRole::Colscr)
        );
        assert_eq!(
            parsed.screens[0].direct_targets[0].start_offset,
            direct_offset
        );
        assert_eq!(parsed.screens[0].hotspots[0].target_direct_index, Some(0));
        assert_eq!(parsed.stats["hotspot_direct_targets"], 1);
    }

    fn component(
        filename: &str,
        role: SsedComponentRole,
        start_block: u32,
        end_block: u32,
    ) -> SsedComponent {
        SsedComponent {
            index: 0,
            multi: 0,
            component_type: 0,
            start_block,
            end_block,
            data: [0; 4],
            filename: filename.to_owned(),
            role,
        }
    }

    fn hotspot(x: u32, y: u32, width: u32, height: u32, block: u32, offset: u32) -> Vec<u8> {
        let mut payload = vec![0u8; 36];
        payload[0] = 0x1f;
        payload[1] = HOTSPOT;
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
}

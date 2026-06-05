use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiviewMenuItem {
    pub label: String,
    pub href: Option<String>,
    pub anchor: Option<String>,
    pub children: Vec<MultiviewMenuItem>,
}

impl MultiviewMenuItem {
    fn new(label: String, href: Option<String>, anchor: Option<String>) -> Self {
        Self {
            label,
            href,
            anchor,
            children: Vec::new(),
        }
    }
}

pub fn parse_menu_data(xml: &str) -> Result<Vec<MultiviewMenuItem>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut roots = Vec::new();
    let mut stack: Vec<MultiviewMenuItem> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if is_menu_node(event.name().as_ref()) => {
                stack.push(menu_item_from_event(&reader, &event)?);
            }
            Ok(Event::Empty(event)) if is_menu_node(event.name().as_ref()) => {
                push_menu_item(
                    &mut roots,
                    &mut stack,
                    menu_item_from_event(&reader, &event)?,
                );
            }
            Ok(Event::End(event)) if is_menu_node(event.name().as_ref()) => {
                let Some(item) = stack.pop() else {
                    return Err(Error::Driver(
                        "MultiView menu XML has an unmatched item/menu close tag".to_owned(),
                    ));
                };
                push_menu_item(&mut roots, &mut stack, item);
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(Error::Driver(format!(
                    "menuData.xml XML parse error at byte {}: {error}",
                    reader.buffer_position()
                )));
            }
            _ => {}
        }
    }

    if !stack.is_empty() {
        return Err(Error::Driver(
            "MultiView menu XML ended with unclosed item/menu elements".to_owned(),
        ));
    }

    Ok(roots)
}

fn is_menu_node(name: &[u8]) -> bool {
    matches!(name, b"item" | b"menu")
}

fn menu_item_from_event(
    reader: &Reader<&[u8]>,
    event: &BytesStart<'_>,
) -> Result<MultiviewMenuItem> {
    let mut label = String::new();
    let mut href = None;
    let mut anchor = None;

    for attribute in event.attributes() {
        let attribute = attribute.map_err(|error| {
            Error::Driver(format!(
                "menuData.xml has an invalid attribute at byte {}: {error}",
                reader.buffer_position()
            ))
        })?;
        let value = attribute
            .decode_and_unescape_value(reader.decoder())
            .map_err(|error| {
                Error::Driver(format!(
                    "menuData.xml has an invalid attribute value at byte {}: {error}",
                    reader.buffer_position()
                ))
            })?
            .into_owned();
        match attribute.key.as_ref() {
            b"label" | b"name" => label = value,
            b"href" | b"ref" => href = nonempty_value(value),
            b"anchor" => anchor = nonempty_value(value),
            _ => {}
        }
    }

    Ok(MultiviewMenuItem::new(label, href, anchor))
}

fn nonempty_value(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && !value.eq_ignore_ascii_case("none")).then(|| value.to_owned())
}

fn push_menu_item(
    roots: &mut Vec<MultiviewMenuItem>,
    stack: &mut [MultiviewMenuItem],
    item: MultiviewMenuItem,
) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(item);
    } else {
        roots.push(item);
    }
}

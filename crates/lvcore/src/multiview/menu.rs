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
            Ok(Event::Start(event)) if event.name().as_ref() == b"item" => {
                stack.push(menu_item_from_event(&reader, &event)?);
            }
            Ok(Event::Empty(event)) if event.name().as_ref() == b"item" => {
                push_menu_item(
                    &mut roots,
                    &mut stack,
                    menu_item_from_event(&reader, &event)?,
                );
            }
            Ok(Event::End(event)) if event.name().as_ref() == b"item" => {
                let Some(item) = stack.pop() else {
                    return Err(Error::Driver(
                        "menuData.xml has an unmatched </item>".to_owned(),
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
            "menuData.xml ended with unclosed <item> elements".to_owned(),
        ));
    }

    Ok(roots)
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
            b"label" => label = value,
            b"href" => href = nonempty_value(value),
            b"anchor" => anchor = nonempty_value(value),
            _ => {}
        }
    }

    Ok(MultiviewMenuItem::new(label, href, anchor))
}

fn nonempty_value(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
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

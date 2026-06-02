use std::collections::BTreeMap;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlistValue {
    Dict(BTreeMap<String, PlistValue>),
    Array(Vec<PlistValue>),
    String(String),
    Bool(bool),
    Integer(i64),
    Real,
    Data,
    Date,
}

pub(crate) fn parse_xml_plist(bytes: &[u8], source_label: &str) -> Result<PlistValue> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| Error::Driver(format!("{source_label} is not UTF-8 XML: {error}")))?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.name().as_ref() == b"plist" => {
                return parse_plist_root(&mut reader, source_label);
            }
            Ok(Event::Decl(_)) | Ok(Event::DocType(_)) | Ok(Event::Comment(_)) => {}
            Ok(Event::Eof) => {
                return Err(Error::Driver(format!(
                    "{source_label} ended before plist root"
                )));
            }
            Err(error) => return Err(plist_xml_error(&reader, error, source_label)),
            _ => {}
        }
    }
}

fn parse_plist_root(reader: &mut Reader<&[u8]>, source_label: &str) -> Result<PlistValue> {
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => return parse_plist_value(reader, event, source_label),
            Ok(Event::Empty(event)) => return parse_empty_plist_value(event, source_label),
            Ok(Event::End(event)) if event.name().as_ref() == b"plist" => {
                return Err(Error::Driver(format!("empty {source_label}")));
            }
            Ok(Event::Comment(_)) => {}
            Ok(Event::Eof) => {
                return Err(Error::Driver(format!(
                    "{source_label} ended inside plist root"
                )));
            }
            Err(error) => return Err(plist_xml_error(reader, error, source_label)),
            _ => {}
        }
    }
}

fn parse_plist_value(
    reader: &mut Reader<&[u8]>,
    event: BytesStart<'_>,
    source_label: &str,
) -> Result<PlistValue> {
    match event.name().as_ref() {
        b"dict" => parse_plist_dict(reader, source_label),
        b"array" => parse_plist_array(reader, source_label),
        b"string" | b"key" => {
            parse_text_value(reader, event.name().as_ref(), source_label).map(PlistValue::String)
        }
        b"integer" => {
            let raw = parse_text_value(reader, b"integer", source_label)?;
            let value = raw.trim().parse::<i64>().map_err(|error| {
                Error::Driver(format!("invalid {source_label} integer {raw:?}: {error}"))
            })?;
            Ok(PlistValue::Integer(value))
        }
        b"real" => {
            let _ = parse_text_value(reader, b"real", source_label)?;
            Ok(PlistValue::Real)
        }
        b"data" => {
            let _ = parse_text_value(reader, b"data", source_label)?;
            Ok(PlistValue::Data)
        }
        b"date" => {
            let _ = parse_text_value(reader, b"date", source_label)?;
            Ok(PlistValue::Date)
        }
        b"true" => {
            consume_until_end(reader, b"true", source_label)?;
            Ok(PlistValue::Bool(true))
        }
        b"false" => {
            consume_until_end(reader, b"false", source_label)?;
            Ok(PlistValue::Bool(false))
        }
        name => Err(Error::Driver(format!(
            "unsupported {source_label} value element <{}>",
            String::from_utf8_lossy(name)
        ))),
    }
}

fn parse_empty_plist_value(event: BytesStart<'_>, source_label: &str) -> Result<PlistValue> {
    match event.name().as_ref() {
        b"array" => Ok(PlistValue::Array(Vec::new())),
        b"dict" => Ok(PlistValue::Dict(BTreeMap::new())),
        b"string" => Ok(PlistValue::String(String::new())),
        b"integer" => Ok(PlistValue::Integer(0)),
        b"real" => Ok(PlistValue::Real),
        b"data" => Ok(PlistValue::Data),
        b"date" => Ok(PlistValue::Date),
        b"true" => Ok(PlistValue::Bool(true)),
        b"false" => Ok(PlistValue::Bool(false)),
        name => Err(Error::Driver(format!(
            "unsupported empty {source_label} value <{}>",
            String::from_utf8_lossy(name)
        ))),
    }
}

fn parse_plist_dict(reader: &mut Reader<&[u8]>, source_label: &str) -> Result<PlistValue> {
    let mut rows = BTreeMap::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.name().as_ref() == b"key" => {
                let key = parse_text_value(reader, b"key", source_label)?;
                let value = parse_next_value(reader, source_label)?;
                rows.insert(key, value);
            }
            Ok(Event::End(event)) if event.name().as_ref() == b"dict" => {
                return Ok(PlistValue::Dict(rows));
            }
            Ok(Event::Comment(_)) => {}
            Ok(Event::Eof) => {
                return Err(Error::Driver(format!("{source_label} ended inside dict")));
            }
            Err(error) => return Err(plist_xml_error(reader, error, source_label)),
            _ => {}
        }
    }
}

fn parse_plist_array(reader: &mut Reader<&[u8]>, source_label: &str) -> Result<PlistValue> {
    let mut rows = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => rows.push(parse_plist_value(reader, event, source_label)?),
            Ok(Event::Empty(event)) => rows.push(parse_empty_plist_value(event, source_label)?),
            Ok(Event::End(event)) if event.name().as_ref() == b"array" => {
                return Ok(PlistValue::Array(rows));
            }
            Ok(Event::Comment(_)) => {}
            Ok(Event::Eof) => {
                return Err(Error::Driver(format!("{source_label} ended inside array")));
            }
            Err(error) => return Err(plist_xml_error(reader, error, source_label)),
            _ => {}
        }
    }
}

fn parse_next_value(reader: &mut Reader<&[u8]>, source_label: &str) -> Result<PlistValue> {
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => return parse_plist_value(reader, event, source_label),
            Ok(Event::Empty(event)) => return parse_empty_plist_value(event, source_label),
            Ok(Event::Comment(_)) => {}
            Ok(Event::Eof) => {
                return Err(Error::Driver(format!(
                    "{source_label} ended before dict value"
                )));
            }
            Err(error) => return Err(plist_xml_error(reader, error, source_label)),
            _ => {}
        }
    }
}

fn parse_text_value(
    reader: &mut Reader<&[u8]>,
    expected_end: &[u8],
    source_label: &str,
) -> Result<String> {
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Text(event)) => {
                let value = event.xml_content().map_err(|error| {
                    Error::Driver(format!(
                        "{source_label} text decode error at byte {}: {error}",
                        reader.buffer_position()
                    ))
                })?;
                text.push_str(&value);
            }
            Ok(Event::CData(event)) => {
                let value = event.xml_content().map_err(|error| {
                    Error::Driver(format!(
                        "{source_label} CDATA decode error at byte {}: {error}",
                        reader.buffer_position()
                    ))
                })?;
                text.push_str(&value);
            }
            Ok(Event::End(event)) if event.name().as_ref() == expected_end => return Ok(text),
            Ok(Event::Eof) => {
                return Err(Error::Driver(format!(
                    "{source_label} ended inside <{}>",
                    String::from_utf8_lossy(expected_end)
                )));
            }
            Err(error) => return Err(plist_xml_error(reader, error, source_label)),
            _ => {}
        }
    }
}

fn consume_until_end(
    reader: &mut Reader<&[u8]>,
    expected_end: &[u8],
    source_label: &str,
) -> Result<()> {
    loop {
        match reader.read_event() {
            Ok(Event::End(event)) if event.name().as_ref() == expected_end => return Ok(()),
            Ok(Event::Eof) => {
                return Err(Error::Driver(format!(
                    "{source_label} ended inside <{}>",
                    String::from_utf8_lossy(expected_end)
                )));
            }
            Err(error) => return Err(plist_xml_error(reader, error, source_label)),
            _ => {}
        }
    }
}

fn plist_xml_error(reader: &Reader<&[u8]>, error: quick_xml::Error, source_label: &str) -> Error {
    Error::Driver(format!(
        "{source_label} XML error at byte {}: {error}",
        reader.buffer_position()
    ))
}

impl PlistValue {
    pub(crate) fn as_dict(&self) -> Option<&BTreeMap<String, PlistValue>> {
        match self {
            Self::Dict(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn as_array(&self) -> Option<&[PlistValue]> {
        match self {
            Self::Array(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }
}

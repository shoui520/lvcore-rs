use quick_xml::Reader;
use quick_xml::events::Event;

use super::*;

pub(super) const IOS_APP_MENU_XML_PREFIX: &str = "ios-app-menu:";

#[derive(Debug, Clone)]
pub(super) struct SsedIosAppMenuXmlSource {
    pub surface_id: String,
    pub source_id: String,
    pub title: String,
    pub label: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct SsedIosAppMenuXmlItem {
    item_id: String,
    label_text: String,
    path: String,
    anchor: Option<String>,
}

#[derive(Debug, Default)]
struct RawSsedIosAppMenuXmlItem {
    label: String,
    reference: String,
    reference_type: String,
    directory: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppMenuXmlField {
    Label,
    Reference,
    ReferenceType,
    Directory,
}

impl ReaderBookPackage {
    pub(super) fn ssed_ios_app_menu_xml_sources(&self) -> Result<Vec<SsedIosAppMenuXmlSource>> {
        let mut sources = Vec::new();
        for path in self.storage.list_dir(Path::new(""))? {
            let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !filename.to_ascii_lowercase().ends_with("_menu.xml") {
                continue;
            }
            let bytes = self.storage.read(Path::new(filename))?;
            let source = SsedIosAppMenuXmlSource {
                surface_id: format!("{IOS_APP_MENU_XML_PREFIX}{filename}"),
                source_id: filename.to_owned(),
                title: "App menu".to_owned(),
                label: filename.to_owned(),
                bytes,
            };
            let Ok(items) = self.ssed_ios_app_menu_xml_items(&source) else {
                continue;
            };
            if items.is_empty() {
                continue;
            }
            sources.push(source);
        }
        Ok(sources)
    }

    pub(super) fn open_ssed_ios_app_menu_xml_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        let Some(source_id) = surface_id.strip_prefix(IOS_APP_MENU_XML_PREFIX) else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "surface_open_deferred",
                    "iOS app-menu XML surface was not found or is not implemented",
                )],
            });
        };
        let Some(source) = self
            .ssed_ios_app_menu_xml_sources()?
            .into_iter()
            .find(|source| source.source_id.eq_ignore_ascii_case(source_id))
        else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "surface_open_deferred",
                    "iOS app-menu XML surface was not found or is not implemented",
                )],
            });
        };

        let items = self.ssed_ios_app_menu_xml_items(&source)?;
        let offset = decode_offset_cursor(cursor);
        let mut page = Vec::new();
        let mut has_more = false;
        for item in items.into_iter().skip(offset) {
            if page.len() >= limit {
                has_more = true;
                break;
            }
            let resource = ResourceToken::new(&InternalResource::PackageFile {
                path: item.path,
                resource_kind: ResourceKind::Html,
            })?;
            let target = TargetToken::new(&InternalTarget::Resource {
                resource,
                anchor: item.anchor,
            })?;
            page.push(NavigationItem {
                item_id: item.item_id,
                label_html: escape_plain_label_html(&item.label_text),
                label_text: item.label_text,
                target,
                href: String::new(),
                diagnostics: Vec::new(),
            });
        }
        let next_cursor = has_more.then(|| offset.saturating_add(limit).to_string());
        Ok(NavigationSurface::InfoPages {
            surface_id: surface_id.to_owned(),
            pages: page,
            next_cursor,
        })
    }

    fn ssed_ios_app_menu_xml_items(
        &self,
        source: &SsedIosAppMenuXmlSource,
    ) -> Result<Vec<SsedIosAppMenuXmlItem>> {
        let raw_items = parse_ssed_ios_app_menu_xml_items(&source.bytes, &source.label)?;
        let mut items = Vec::new();
        for raw in raw_items {
            if !raw.reference_type.trim().eq_ignore_ascii_case("html") {
                continue;
            }
            let label_text = raw.label.trim();
            if label_text.is_empty() {
                continue;
            }
            let directory = raw.directory.trim().replace('\\', "/");
            let Some(reference) = package_relative_html_reference(&directory, raw.reference.trim())
            else {
                continue;
            };
            if !path_has_extension(&reference.path, &["html", "htm"])
                || !self.storage.exists(Path::new(&reference.path))?
            {
                continue;
            }
            let item_id = items.len().to_string();
            items.push(SsedIosAppMenuXmlItem {
                item_id,
                label_text: label_text.to_owned(),
                path: reference.path,
                anchor: reference.anchor,
            });
        }
        Ok(items)
    }
}

pub(super) fn is_ssed_ios_app_menu_xml_surface_id(surface_id: &str) -> bool {
    surface_id.starts_with(IOS_APP_MENU_XML_PREFIX)
}

fn parse_ssed_ios_app_menu_xml_items(
    bytes: &[u8],
    source_label: &str,
) -> Result<Vec<RawSsedIosAppMenuXmlItem>> {
    let xml = decode_package_html_text(bytes);
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);
    let mut items = Vec::new();
    let mut item = None::<RawSsedIosAppMenuXmlItem>;
    let mut field = None::<AppMenuXmlField>;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.name().as_ref() == b"item" => {
                item = Some(RawSsedIosAppMenuXmlItem::default());
            }
            Ok(Event::End(event)) if event.name().as_ref() == b"item" => {
                if let Some(item) = item.take() {
                    items.push(item);
                }
                field = None;
            }
            Ok(Event::Start(event)) if item.is_some() => {
                field = app_menu_xml_field(event.name().as_ref());
            }
            Ok(Event::End(event)) if app_menu_xml_field(event.name().as_ref()).is_some() => {
                field = None;
            }
            Ok(Event::Text(event)) if item.is_some() => {
                if let (Some(field), Some(item)) = (field, item.as_mut()) {
                    let text = event.xml_content().map_err(|error| {
                        Error::Driver(format!(
                            "{source_label} text decode error at byte {}: {error}",
                            reader.buffer_position()
                        ))
                    })?;
                    push_app_menu_xml_text(item, field, &text);
                }
            }
            Ok(Event::CData(event)) if item.is_some() => {
                if let (Some(field), Some(item)) = (field, item.as_mut()) {
                    let text = event.xml_content().map_err(|error| {
                        Error::Driver(format!(
                            "{source_label} CDATA decode error at byte {}: {error}",
                            reader.buffer_position()
                        ))
                    })?;
                    push_app_menu_xml_text(item, field, &text);
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(Error::Driver(format!(
                    "{source_label} XML parse error at byte {}: {error}",
                    reader.buffer_position()
                )));
            }
            _ => {}
        }
    }
    Ok(items)
}

fn app_menu_xml_field(name: &[u8]) -> Option<AppMenuXmlField> {
    match name {
        b"label" => Some(AppMenuXmlField::Label),
        b"ref" => Some(AppMenuXmlField::Reference),
        b"reftype" => Some(AppMenuXmlField::ReferenceType),
        b"directory" => Some(AppMenuXmlField::Directory),
        _ => None,
    }
}

fn push_app_menu_xml_text(item: &mut RawSsedIosAppMenuXmlItem, field: AppMenuXmlField, text: &str) {
    match field {
        AppMenuXmlField::Label => item.label.push_str(text),
        AppMenuXmlField::Reference => item.reference.push_str(text),
        AppMenuXmlField::ReferenceType => item.reference_type.push_str(text),
        AppMenuXmlField::Directory => item.directory.push_str(text),
    }
}

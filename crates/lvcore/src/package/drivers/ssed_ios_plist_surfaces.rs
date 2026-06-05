use super::*;

pub(super) const IOS_PLIST_PANEL_PREFIX: &str = "ios-plist:";
pub(super) const IOS_HTML_LIST_PREFIX: &str = "ios-html-list:";
pub(super) const IOS_TABLE_LIST_PREFIX: &str = "ios-table-list:";

#[derive(Debug, Clone)]
pub(super) struct SsedIosPlistSurfaceSource {
    pub surface_id: String,
    pub source_id: String,
    pub title: String,
    pub label: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(super) struct SsedIosHtmlListItem {
    pub index: u32,
    pub label_html: String,
    pub label_text: String,
    pub html: String,
}

impl ReaderBookPackage {
    pub(super) fn ssed_ios_panel_plist_sources(&self) -> Result<Vec<SsedIosPlistSurfaceSource>> {
        let mut sources = Vec::new();
        for file in self.ssed_ios_plist_files()? {
            if !is_ios_panel_plist_candidate(&file.source_id) {
                continue;
            }
            let Ok(plist) = parse_xml_plist(&file.bytes, &file.label) else {
                continue;
            };
            let Ok(parsed) = parse_panel_plist_value_for_panel(&plist, None) else {
                continue;
            };
            if parsed.inline_cells.is_empty() && parsed.data_refs.is_empty() {
                continue;
            }
            sources.push(SsedIosPlistSurfaceSource {
                surface_id: format!("{IOS_PLIST_PANEL_PREFIX}{}", file.source_id),
                source_id: file.source_id.clone(),
                title: ios_plist_surface_title(&file.source_id),
                label: file.label,
                bytes: file.bytes,
            });
        }
        Ok(sources)
    }

    pub(super) fn ssed_ios_html_list_sources(&self) -> Result<Vec<SsedIosPlistSurfaceSource>> {
        Ok(self
            .ssed_ios_plist_files()?
            .into_iter()
            .filter(|file| file.source_id.eq_ignore_ascii_case("HTMLList.plist"))
            .map(|file| SsedIosPlistSurfaceSource {
                surface_id: format!("{IOS_HTML_LIST_PREFIX}{}", file.source_id),
                source_id: file.source_id.clone(),
                title: "HTML info pages".to_owned(),
                label: file.label,
                bytes: file.bytes,
            })
            .collect())
    }

    pub(super) fn ssed_ios_table_list_sources(&self) -> Result<Vec<SsedIosPlistSurfaceSource>> {
        Ok(self
            .ssed_ios_plist_files()?
            .into_iter()
            .filter(|file| file.source_id.eq_ignore_ascii_case("tableList.plist"))
            .map(|file| SsedIosPlistSurfaceSource {
                surface_id: format!("{IOS_TABLE_LIST_PREFIX}{}", file.source_id),
                source_id: file.source_id.clone(),
                title: "Table list".to_owned(),
                label: file.label,
                bytes: file.bytes,
            })
            .collect())
    }

    pub(super) fn open_ssed_ios_html_list_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        let Some(source_id) = surface_id.strip_prefix(IOS_HTML_LIST_PREFIX) else {
            return Ok(surface_open_deferred(surface_id));
        };
        let items = self.ssed_ios_html_list_items(source_id)?;
        let offset = decode_offset_cursor(cursor);
        let mut page = Vec::new();
        let mut has_more = false;
        for item in items.into_iter().skip(offset) {
            if page.len() >= limit {
                has_more = true;
                break;
            }
            let target = TargetToken::new(&InternalTarget::SsedIosHtmlPage {
                source_id: source_id.to_owned(),
                index: item.index,
                anchor: None,
            })?;
            page.push(NavigationItem {
                item_id: item.index.to_string(),
                label_html: item.label_html,
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

    pub(super) fn open_ssed_ios_table_list_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
        options: &LabelOptions,
    ) -> Result<NavigationSurface> {
        let Some(source_id) = surface_id.strip_prefix(IOS_TABLE_LIST_PREFIX) else {
            return Ok(surface_open_deferred(surface_id));
        };
        let Some(source) = self
            .ssed_ios_table_list_sources()?
            .into_iter()
            .find(|source| source.source_id.eq_ignore_ascii_case(source_id))
        else {
            return Ok(surface_open_deferred(surface_id));
        };
        let plist = parse_xml_plist(&source.bytes, &source.label)?;
        let rows = plist.as_array().unwrap_or_default();
        let offset = decode_offset_cursor(cursor);
        let mut items = Vec::new();
        let mut has_more = false;
        let mut diagnostics = Vec::new();
        for (index, row) in rows.iter().enumerate().skip(offset) {
            if items.len() >= limit {
                has_more = true;
                break;
            }
            let Some(dict) = row.as_dict() else {
                continue;
            };
            let label = plist_string(dict, &["name", "item", "title", "label"]);
            if label.trim().is_empty() {
                continue;
            }
            let Some(block) = plist_u32(dict, "block").filter(|value| *value > 0) else {
                continue;
            };
            let (block, offset) =
                self.convert_ios_ssed_address(block, plist_u32(dict, "offset").unwrap_or(0))?;
            let target = self.ssed_target_for_loose_address(block, offset, &mut diagnostics)?;
            let Some(target) = target else {
                continue;
            };
            let rich_label = self.ssed_rich_label_with_policy(&label, &options.gaiji_policy);
            items.push(NavigationItem {
                item_id: index.to_string(),
                label_html: rich_label.html,
                label_text: rich_label.text,
                target,
                href: String::new(),
                diagnostics: rich_label.diagnostics,
            });
        }
        if !diagnostics.is_empty() && items.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics,
            });
        }
        let next_cursor = has_more.then(|| offset.saturating_add(limit).to_string());
        Ok(NavigationSurface::TitleIndexBrowse {
            surface_id: surface_id.to_owned(),
            items,
            next_cursor,
        })
    }

    pub(super) fn ssed_ios_html_list_item(
        &self,
        source_id: &str,
        index: u32,
    ) -> Result<Option<SsedIosHtmlListItem>> {
        Ok(self
            .ssed_ios_html_list_items(source_id)?
            .into_iter()
            .find(|item| item.index == index))
    }

    pub(super) fn visual_body_for_ssed_ios_html_page(
        &self,
        source_id: &str,
        index: u32,
    ) -> Result<VisualBody> {
        let Some(item) = self.ssed_ios_html_list_item(source_id, index)? else {
            return Ok(VisualBody::Unsupported {
                reason: "iOS HTMLList page was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "ssed_ios_html_list_missing",
                    format!("{source_id} did not contain page {index}"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html: item.html,
            source: BodySourceKind::SidecarHtml,
        })
    }

    fn ssed_ios_html_list_items(&self, source_id: &str) -> Result<Vec<SsedIosHtmlListItem>> {
        let Some(source) = self
            .ssed_ios_html_list_sources()?
            .into_iter()
            .find(|source| source.source_id.eq_ignore_ascii_case(source_id))
        else {
            return Ok(Vec::new());
        };
        let plist = parse_xml_plist(&source.bytes, &source.label)?;
        let rows = plist.as_array().unwrap_or_default();
        let raw_html_values = extract_html_data_values(&source.bytes);
        let mut items = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            let Some(dict) = row.as_dict() else {
                continue;
            };
            let Some(html) = raw_html_values
                .get(index)
                .cloned()
                .or_else(|| plist_string_opt(dict, &["htmlData"]))
            else {
                continue;
            };
            let html = decode_ios_plist_html_fragment(&html);
            let label_text = ios_html_list_label(dict, &html, index);
            items.push(SsedIosHtmlListItem {
                index: u32::try_from(index).unwrap_or(u32::MAX),
                label_html: escape_plain_label_html(&label_text),
                label_text,
                html,
            });
        }
        Ok(items)
    }

    fn ssed_ios_plist_files(&self) -> Result<Vec<SsedIosPlistFile>> {
        let mut files = Vec::new();
        let mut seen = BTreeSet::new();
        collect_ios_plist_files_from_base(&self.root, "", &mut files, &mut seen)?;
        if let Some(parent) = self.root.parent() {
            collect_ios_plist_files_from_base(parent, "", &mut files, &mut seen)?;
        }
        files.sort_by(|left, right| left.source_id.cmp(&right.source_id));
        Ok(files)
    }
}

#[derive(Debug, Clone)]
struct SsedIosPlistFile {
    source_id: String,
    label: String,
    bytes: Vec<u8>,
}

fn collect_ios_plist_files_from_base(
    base: &Path,
    prefix: &str,
    files: &mut Vec<SsedIosPlistFile>,
    seen: &mut BTreeSet<String>,
) -> Result<()> {
    if !base.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(base)? {
        let entry = entry?;
        let path = entry.path();
        if !regular_file_inside_root(base, &path)? {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !filename.to_ascii_lowercase().ends_with(".plist") {
            continue;
        }
        let source_id = if prefix.is_empty() {
            filename.to_owned()
        } else {
            format!("{prefix}/{filename}")
        };
        if !seen.insert(source_id.to_ascii_lowercase()) {
            continue;
        }
        files.push(SsedIosPlistFile {
            label: source_id.clone(),
            source_id,
            bytes: std::fs::read(path)?,
        });
    }
    Ok(())
}

fn is_ios_panel_plist_candidate(source_id: &str) -> bool {
    let filename = source_id.rsplit('/').next().unwrap_or(source_id);
    let lower = filename.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "dictlist.plist"
            | "resourcescopy.plist"
            | "gaiji.plist"
            | "gaijis.plist"
            | "gaijiicon.plist"
            | "panelsgaiji.plist"
            | "htmllist.plist"
            | "tablelist.plist"
            | "checkuni2cid22.plist"
    ) {
        return false;
    }
    !matches!(
        lower.as_str(),
        "menu.plist" | "menu_.plist" | "menu_ipad.plist"
    )
}

fn ios_plist_surface_title(source_id: &str) -> String {
    let filename = source_id.rsplit('/').next().unwrap_or(source_id);
    filename
        .strip_suffix(".plist")
        .unwrap_or(filename)
        .replace(['_', '-'], " ")
}

fn ios_html_list_label(dict: &BTreeMap<String, PlistValue>, html: &str, index: usize) -> String {
    if let Some(label) = html_document_label(html) {
        return label;
    }
    let text = html_basic_text(html);
    if let Some(line) = text.lines().map(str::trim).find(|line| !line.is_empty()) {
        return line.to_owned();
    }
    if let Some(names) = dict.get("name").and_then(PlistValue::as_array) {
        let joined = names
            .iter()
            .filter_map(PlistValue::as_str)
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" / ");
        if !joined.is_empty() {
            return joined;
        }
    }
    format!("HTML page {}", index.saturating_add(1))
}

fn decode_ios_plist_html_fragment(value: &str) -> String {
    let once = html_unescape_minimal(value);
    html_unescape_minimal(&once)
}

fn extract_html_data_values(bytes: &[u8]) -> Vec<String> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Vec::new();
    };
    let mut values = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative_key) = text[cursor..].find("<key>htmlData</key>") {
        let after_key = cursor + relative_key + "<key>htmlData</key>".len();
        let Some(relative_start) = text[after_key..].find("<string") else {
            break;
        };
        let string_start = after_key + relative_start;
        let Some(content_start) = text[string_start..]
            .find('>')
            .map(|offset| string_start + offset + 1)
        else {
            break;
        };
        let Some(relative_end) = text[content_start..].find("</string>") else {
            break;
        };
        let content_end = content_start + relative_end;
        values.push(xml_string_payload_text(&text[content_start..content_end]));
        cursor = content_end + "</string>".len();
    }
    values
}

fn xml_string_payload_text(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(inner) = trimmed
        .strip_prefix("<![CDATA[")
        .and_then(|rest| rest.strip_suffix("]]>"))
    {
        inner.to_owned()
    } else {
        trimmed.to_owned()
    }
}

pub(super) fn is_ssed_ios_panel_surface_id(surface_id: &str) -> bool {
    surface_id.starts_with(IOS_PLIST_PANEL_PREFIX)
}

pub(super) fn is_ssed_ios_html_list_surface_id(surface_id: &str) -> bool {
    surface_id.starts_with(IOS_HTML_LIST_PREFIX)
}

pub(super) fn is_ssed_ios_table_list_surface_id(surface_id: &str) -> bool {
    surface_id.starts_with(IOS_TABLE_LIST_PREFIX)
}

fn surface_open_deferred(surface_id: &str) -> NavigationSurface {
    NavigationSurface::Deferred {
        surface_id: surface_id.to_owned(),
        diagnostics: vec![Diagnostic::info(
            "surface_open_deferred",
            "iOS plist surface was not found or is not implemented",
        )],
    }
}

fn plist_string(dict: &BTreeMap<String, PlistValue>, keys: &[&str]) -> String {
    plist_string_opt(dict, keys).unwrap_or_default()
}

fn plist_string_opt(dict: &BTreeMap<String, PlistValue>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        dict.get(*key)
            .and_then(PlistValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn plist_u32(dict: &BTreeMap<String, PlistValue>, key: &str) -> Option<u32> {
    dict.get(key)
        .and_then(PlistValue::as_i64)
        .and_then(|value| u32::try_from(value).ok())
}

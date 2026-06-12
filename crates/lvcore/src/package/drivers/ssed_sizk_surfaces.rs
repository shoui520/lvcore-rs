use super::*;

pub(super) const SSED_SIZK_SURFACE_ID: &str = "sizk-read-aloud";
pub(super) const SSED_SIZK_SOURCE_ID: &str = "sizk-read-aloud";

const SIZK_TEXT_PATH: &str = "shizuku_honbun.txt";
const SIZK_TIME_PATH: &str = "shizuku_time.txt";
const SIZK_AUDIO_PATH: &str = "shizuku.mp3";

#[derive(Debug, Clone)]
struct SsedSizkEntry {
    index: usize,
    role: String,
    template_path: Option<String>,
    title: String,
    sections: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct SsedSizkPlaybackRow {
    index: usize,
    time_ms: Option<u64>,
    text: String,
}

impl ReaderBookPackage {
    pub(super) fn has_ssed_sizk_surface(&self) -> Result<bool> {
        self.is_ssed_sizk_package()
    }

    pub(super) fn ssed_sizk_home_surface(&self) -> Result<Option<HomeSurface>> {
        if !self.has_ssed_sizk_surface()? {
            return Ok(None);
        }
        let title = self
            .ssed_sizk_entries()?
            .into_iter()
            .find(|entry| entry.role == "overview" && !entry.title.is_empty())
            .map(|entry| entry.title)
            .unwrap_or_else(|| "SIZK read-aloud".to_owned());
        Ok(Some(HomeSurface {
            href: None,
            surface_id: SSED_SIZK_SURFACE_ID.to_owned(),
            kind: NavigationSurfaceKind::Info,
            status: NavigationStatus::Available,
            title_html: escape_plain_label_html(&title),
            title_text: title,
            target: Some(TargetToken::new(&InternalTarget::MenuItem {
                surface_id: SSED_SIZK_SURFACE_ID.to_owned(),
                item_id: "root".to_owned(),
            })?),
            diagnostics: vec![Diagnostic::info(
                "ssed_sizk_read_aloud",
                "SIZK read-aloud package sidecars expose template pages and synchronized audio",
            )],
        }))
    }

    pub(super) fn open_ssed_sizk_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        if limit == 0 {
            return Ok(NavigationSurface::InfoPages {
                surface_id: surface_id.to_owned(),
                pages: Vec::new(),
                next_cursor: None,
            });
        }
        if !self.has_ssed_sizk_surface()? {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_sizk_missing",
                    "SIZK read-aloud sidecars were not found in this package",
                )],
            });
        }
        let entries = self.ssed_sizk_entries()?;
        if entries.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "ssed_sizk_entries_missing",
                    "SIZK HONMON entries did not expose template pages",
                )],
            });
        }
        let offset = decode_offset_cursor(cursor);
        let next_cursor =
            (entries.len() > offset.saturating_add(limit)).then(|| (offset + limit).to_string());
        let pages = entries
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|entry| {
                let label = ssed_sizk_entry_label(&entry);
                let target = TargetToken::new(&InternalTarget::SsedAuxRecord {
                    source: SSED_SIZK_SOURCE_ID.to_owned(),
                    key: entry.role.clone(),
                    anchor: None,
                })?;
                Ok(NavigationItem {
                    href: String::new(),
                    item_id: entry.role,
                    label_html: escape_plain_label_html(&label),
                    label_text: label,
                    target,
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::InfoPages {
            surface_id: surface_id.to_owned(),
            pages,
            next_cursor,
        })
    }

    pub(super) fn title_for_ssed_sizk_record(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .ssed_sizk_entry_for_key(key)?
            .map(|entry| ssed_sizk_entry_label(&entry)))
    }

    pub(super) fn visual_body_for_ssed_sizk_record(&self, key: &str) -> Result<VisualBody> {
        let Some(entry) = self.ssed_sizk_entry_for_key(key)? else {
            return Ok(VisualBody::Unsupported {
                reason: "SIZK entry was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "ssed_sizk_entry_missing",
                    format!("SIZK entry {key} was not found"),
                )],
            });
        };
        let html = if entry.role == "playback" {
            self.render_ssed_sizk_playback_html(&entry)?
        } else {
            self.render_ssed_sizk_template_html(&entry)?
        };
        Ok(VisualBody::PreservedHtml {
            html,
            source: BodySourceKind::SidecarHtml,
        })
    }

    fn is_ssed_sizk_package(&self) -> Result<bool> {
        let has_sidecars = self.resolve_package_file_path(SIZK_AUDIO_PATH)?.is_some()
            && self.resolve_package_file_path(SIZK_TEXT_PATH)?.is_some()
            && self.resolve_package_file_path(SIZK_TIME_PATH)?.is_some();
        if has_sidecars {
            return Ok(true);
        }
        let Ok(exinfo) = self.storage.read(Path::new("EXINFO.INI")) else {
            return Ok(false);
        };
        let mp3 = crate::ssed_panel::exinfo_general_value(&exinfo, "MP3NAME")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        Ok(mp3 == SIZK_AUDIO_PATH)
    }

    fn ssed_sizk_entry_for_key(&self, key: &str) -> Result<Option<SsedSizkEntry>> {
        let key = key.trim();
        Ok(self
            .ssed_sizk_entries()?
            .into_iter()
            .find(|entry| entry.role == key || entry.index.to_string() == key))
    }

    fn ssed_sizk_entries(&self) -> Result<Vec<SsedSizkEntry>> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(Vec::new());
        };
        let Some(component) = catalog.honmon() else {
            return Ok(Vec::new());
        };
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Ok(Vec::new());
        };
        let mut reader = SsedDataFile::open(path)?;
        let expanded = reader.read_range(0, reader.header().expanded_size())?;
        let mut entries = Vec::new();
        for (index, slice) in ssed_sizk_entry_slices(&expanded).into_iter().enumerate() {
            let template_code = ssed_sizk_template_code(slice).unwrap_or_default();
            let Some(role) = ssed_sizk_template_role(&template_code) else {
                continue;
            };
            let sections = ssed_sizk_sections(slice);
            let title =
                ssed_sizk_entry_title(&sections).unwrap_or_else(|| role_label(role).to_owned());
            let template_path = self.ssed_sizk_template_path(&template_code)?;
            entries.push(SsedSizkEntry {
                index: index + 1,
                role: role.to_owned(),
                template_path,
                title,
                sections,
            });
        }
        Ok(entries)
    }

    fn ssed_sizk_template_path(&self, template_code: &str) -> Result<Option<String>> {
        if template_code.is_empty() {
            return Ok(None);
        }
        let path = format!("HTMLs/{template_code}.html");
        Ok(self
            .resolve_package_file_path(&path)?
            .is_some()
            .then_some(path))
    }

    fn render_ssed_sizk_template_html(&self, entry: &SsedSizkEntry) -> Result<String> {
        let html = if let Some(path) = &entry.template_path {
            decode_package_html_text(&self.read_package_file_bytes(path)?)
        } else {
            ssed_sizk_fallback_template_html(entry)
        };
        let html = ssed_sizk_apply_sections(&html, &entry.sections);
        self.inline_ssed_sizk_css_path(&html)
    }

    fn inline_ssed_sizk_css_path(&self, html: &str) -> Result<String> {
        if !html.contains("&cssPath;") {
            return Ok(html.to_owned());
        }
        let css = self
            .read_package_file_bytes("Templates/00000190.css")
            .ok()
            .map(|bytes| decode_package_html_text(&bytes));
        let Some(css) = css else {
            return Ok(html.replace("&cssPath;", ""));
        };
        let style = format!("<style type=\"text/css\">\n{css}\n</style>");
        let mut output = html.replace(
            "<link rel=\"stylesheet\" type=\"text/css\" href=\"&cssPath;\">",
            &style,
        );
        output = output.replace(
            "<link rel=\"stylesheet\" type=\"text/css\" href=\"&cssPath;\" />",
            &style,
        );
        Ok(output.replace("&cssPath;", ""))
    }

    fn render_ssed_sizk_playback_html(&self, entry: &SsedSizkEntry) -> Result<String> {
        let rows = self.ssed_sizk_playback_rows()?;
        let title = entry.title.trim();
        let reading = entry
            .sections
            .get("0005")
            .map(String::as_str)
            .unwrap_or_default()
            .trim();
        let mut html = String::new();
        html.push_str("<article class=\"lv-sizk-playback\">");
        html.push_str("<h1>");
        html.push_str(&escape_plain_label_html(if title.is_empty() {
            "SIZK playback"
        } else {
            title
        }));
        html.push_str("</h1>");
        if !reading.is_empty() {
            html.push_str("<p class=\"lv-sizk-reading\">");
            html.push_str(&escape_plain_label_html(reading));
            html.push_str("</p>");
        }
        html.push_str("<audio controls preload=\"metadata\" src=\"");
        html.push_str(SIZK_AUDIO_PATH);
        html.push_str("\"></audio>");
        html.push_str("<ol class=\"lv-sizk-lines\">");
        for row in rows {
            html.push_str("<li");
            if let Some(time_ms) = row.time_ms {
                html.push_str(" data-time-ms=\"");
                html.push_str(&time_ms.to_string());
                html.push('"');
            }
            html.push_str("><span class=\"lv-sizk-line-index\">");
            html.push_str(&row.index.to_string());
            html.push_str("</span> ");
            html.push_str(&escape_plain_label_html(&row.text));
            html.push_str("</li>");
        }
        html.push_str("</ol>");
        html.push_str("</article>");
        Ok(html)
    }

    fn ssed_sizk_playback_rows(&self) -> Result<Vec<SsedSizkPlaybackRow>> {
        let text = decode_sizk_sidecar_text(&self.read_package_file_bytes(SIZK_TEXT_PATH)?);
        let times = decode_sizk_sidecar_text(&self.read_package_file_bytes(SIZK_TIME_PATH)?);
        let text_lines = text.lines().collect::<Vec<_>>();
        let time_lines = times.lines().collect::<Vec<_>>();
        Ok(text_lines
            .into_iter()
            .zip(time_lines)
            .enumerate()
            .map(|(index, (text, time))| SsedSizkPlaybackRow {
                index: index + 1,
                time_ms: parse_sizk_timestamp_ms(time),
                text: text.trim().to_owned(),
            })
            .collect())
    }
}

fn ssed_sizk_entry_slices(data: &[u8]) -> Vec<&[u8]> {
    let starts = data
        .windows(SSED_ENTRY_MARKER.len())
        .enumerate()
        .filter_map(|(index, window)| (window == SSED_ENTRY_MARKER).then_some(index))
        .collect::<Vec<_>>();
    let mut slices = Vec::new();
    for (position, start) in starts.iter().enumerate() {
        let end = starts
            .get(position + 1)
            .copied()
            .unwrap_or_else(|| trim_trailing_zeroes(data).len());
        if *start < end {
            slices.push(&data[*start..end]);
        }
    }
    slices
}

fn trim_trailing_zeroes(data: &[u8]) -> &[u8] {
    let end = data
        .iter()
        .rposition(|byte| *byte != 0)
        .map(|index| index + 1)
        .unwrap_or(0);
    &data[..end]
}

fn ssed_sizk_template_code(data: &[u8]) -> Option<String> {
    let mut offset = SSED_ENTRY_MARKER.len();
    while offset + 1 < data.len() {
        if data[offset] == 0x1f {
            offset += 2 + ssed_control_arg_length(data, offset);
            continue;
        }
        if (0xa1..=0xfe).contains(&data[offset]) {
            return Some(format!("{:02x}{:02x}", data[offset], data[offset + 1]));
        }
        offset += 1;
    }
    None
}

fn ssed_sizk_sections(data: &[u8]) -> BTreeMap<String, String> {
    let mut rows = BTreeMap::new();
    let mut current_code: Option<String> = None;
    let mut current_text = String::new();
    let mut offset = 0usize;

    while offset < data.len() {
        if data[offset] == 0x1f {
            if data.get(offset + 1) == Some(&0x09) && offset + 3 < data.len() {
                flush_sizk_section(&mut rows, &mut current_code, &mut current_text);
                let code = format!("{:02x}{:02x}", data[offset + 2], data[offset + 3]);
                if code != "0001" {
                    current_code = Some(code);
                }
                offset += 4;
                continue;
            }
            if data.get(offset + 1) == Some(&0x0a) {
                current_text.push('\n');
                offset += 2;
                continue;
            }
            offset += 2 + ssed_control_arg_length(data, offset);
            continue;
        }

        if current_code.is_some() {
            if offset + 1 < data.len()
                && (0x21..=0x7e).contains(&data[offset])
                && (0x21..=0x7e).contains(&data[offset + 1])
            {
                if let Some(ch) = crate::ssed_index::decode_jis_pair(data[offset], data[offset + 1])
                {
                    current_text.push(ch);
                }
                offset += 2;
                continue;
            }
            if data[offset].is_ascii_graphic() || data[offset] == b' ' {
                current_text.push(char::from(data[offset]));
            }
        }
        offset += 1;
    }
    flush_sizk_section(&mut rows, &mut current_code, &mut current_text);
    rows
}

fn flush_sizk_section(
    rows: &mut BTreeMap<String, String>,
    current_code: &mut Option<String>,
    current_text: &mut String,
) {
    let Some(code) = current_code.take() else {
        current_text.clear();
        return;
    };
    let text = clean_sizk_section_text(current_text);
    if !text.is_empty() {
        rows.insert(code, text);
    }
    current_text.clear();
}

fn clean_sizk_section_text(value: &str) -> String {
    value
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn ssed_sizk_entry_title(sections: &BTreeMap<String, String>) -> Option<String> {
    ["0004", "0011", "0021"]
        .into_iter()
        .find_map(|code| sections.get(code))
        .map(|title| title.trim())
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
}

fn ssed_sizk_entry_label(entry: &SsedSizkEntry) -> String {
    let role = role_label(&entry.role);
    if entry.title.trim().is_empty() || entry.title == role {
        role.to_owned()
    } else {
        format!("{role}: {}", entry.title)
    }
}

fn ssed_sizk_template_role(code: &str) -> Option<&'static str> {
    match code {
        "b121" => Some("overview"),
        "b122" => Some("author"),
        "b123" => Some("narrator"),
        "b124" => Some("playback"),
        _ => None,
    }
}

fn role_label(role: &str) -> &'static str {
    match role {
        "overview" => "Overview",
        "author" => "Author",
        "narrator" => "Narrator",
        "playback" => "Playback",
        _ => "SIZK page",
    }
}

fn ssed_sizk_apply_sections(html: &str, sections: &BTreeMap<String, String>) -> String {
    let mut output = html.to_owned();
    for (code, text) in sections {
        let escaped = escape_plain_label_html(text).replace('\n', "<br>");
        output = output.replace(
            &format!("<!--&IND{};-->", code.to_ascii_uppercase()),
            &escaped,
        );
        output = output.replace(
            &format!("<!--&IND{};-->", code.to_ascii_lowercase()),
            &escaped,
        );
    }
    output
}

fn ssed_sizk_fallback_template_html(entry: &SsedSizkEntry) -> String {
    let mut html = String::new();
    html.push_str("<article class=\"lv-sizk-page\"><h1>");
    html.push_str(&escape_plain_label_html(&entry.title));
    html.push_str("</h1>");
    for (code, value) in &entry.sections {
        html.push_str("<section data-sizk-section=\"");
        html.push_str(code);
        html.push_str("\">");
        html.push_str(&escape_plain_label_html(value).replace('\n', "<br>"));
        html.push_str("</section>");
    }
    html.push_str("</article>");
    html
}

fn decode_sizk_sidecar_text(data: &[u8]) -> String {
    if data.starts_with(&[0xff, 0xfe]) {
        return decode_utf16_lossy(&data[2..], false);
    }
    if data.starts_with(&[0xfe, 0xff]) {
        return decode_utf16_lossy(&data[2..], true);
    }
    if data.len() >= 4 && data[1] == 0 && data[3] == 0 {
        return decode_utf16_lossy(data, false);
    }
    match std::str::from_utf8(data) {
        Ok(text) => text.to_owned(),
        Err(_) => {
            let (decoded, _, _) = SHIFT_JIS.decode(data);
            decoded.into_owned()
        }
    }
}

fn decode_utf16_lossy(data: &[u8], big_endian: bool) -> String {
    let units = data
        .chunks_exact(2)
        .map(|chunk| {
            if big_endian {
                u16::from_be_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_le_bytes([chunk[0], chunk[1]])
            }
        })
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&units)
}

fn parse_sizk_timestamp_ms(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(ms) = value.parse::<u64>() {
        return Some(ms);
    }
    let normalized = value.replace(',', ".");
    let parts = normalized.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [hours, minutes, seconds] => {
            let hours = hours.parse::<u64>().ok()?;
            let minutes = minutes.parse::<u64>().ok()?;
            let seconds = seconds.parse::<f64>().ok()?;
            Some(((hours * 3600 + minutes * 60) as f64 * 1000.0 + seconds * 1000.0) as u64)
        }
        [minutes, seconds] => {
            let minutes = minutes.parse::<u64>().ok()?;
            let seconds = seconds.parse::<f64>().ok()?;
            Some(((minutes * 60) as f64 * 1000.0 + seconds * 1000.0) as u64)
        }
        _ => None,
    }
}

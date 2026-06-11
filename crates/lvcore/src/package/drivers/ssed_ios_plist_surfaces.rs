use super::sequence::sequence_targets_match;
use super::*;

pub(super) const IOS_PLIST_PANEL_PREFIX: &str = "ios-plist:";
pub(super) const IOS_HTML_LIST_PREFIX: &str = "ios-html-list:";
pub(super) const IOS_TABLE_LIST_PREFIX: &str = "ios-table-list:";
pub(super) const IOS_DICTLIST_OTHER_SURFACE_ID: &str = "ios-dictlist-other";
const IOS_DICTLIST_OTHER_SOURCE_ID: &str = "DictList.plist:Other";

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
    pub path: Option<String>,
    pub anchor: Option<String>,
}

#[derive(Debug, Default)]
struct SsedIosTableListResolutionStats {
    address_rows: usize,
    unresolved_rows: usize,
    converted_rows: usize,
    raw_min_block: Option<u32>,
    raw_max_block: Option<u32>,
    raw_min_offset: Option<u32>,
    raw_max_offset: Option<u32>,
    converted_min_block: Option<u32>,
    converted_max_block: Option<u32>,
}

impl SsedIosTableListResolutionStats {
    fn record(
        &mut self,
        raw_block: u32,
        raw_offset: u32,
        converted_block: u32,
        converted_offset: u32,
        resolved: bool,
    ) {
        self.address_rows = self.address_rows.saturating_add(1);
        self.raw_min_block = Some(
            self.raw_min_block
                .map_or(raw_block, |value| value.min(raw_block)),
        );
        self.raw_max_block = Some(
            self.raw_max_block
                .map_or(raw_block, |value| value.max(raw_block)),
        );
        self.raw_min_offset = Some(
            self.raw_min_offset
                .map_or(raw_offset, |value| value.min(raw_offset)),
        );
        self.raw_max_offset = Some(
            self.raw_max_offset
                .map_or(raw_offset, |value| value.max(raw_offset)),
        );
        self.converted_min_block = Some(
            self.converted_min_block
                .map_or(converted_block, |value| value.min(converted_block)),
        );
        self.converted_max_block = Some(
            self.converted_max_block
                .map_or(converted_block, |value| value.max(converted_block)),
        );
        if (raw_block, raw_offset) != (converted_block, converted_offset) {
            self.converted_rows = self.converted_rows.saturating_add(1);
        }
        if !resolved {
            self.unresolved_rows = self.unresolved_rows.saturating_add(1);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SsedIosTableListCrossBookOwner {
    dict_code: String,
    component: String,
}

impl ReaderBookPackage {
    pub(super) fn ssed_ios_panel_plist_sources(&self) -> Result<Vec<SsedIosPlistSurfaceSource>> {
        let mut sources = Vec::new();
        for file in self.ssed_ios_plist_files()? {
            if !is_ssed_ios_panel_plist_candidate(&file.source_id) {
                continue;
            }
            let Ok(plist) = self.cached_ssed_panel_plist(&file.bytes, &file.label) else {
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

    pub(super) fn ssed_ios_table_list_source_status(
        &self,
        source: &SsedIosPlistSurfaceSource,
    ) -> Result<(NavigationStatus, Vec<Diagnostic>)> {
        let plist = self.cached_ssed_panel_plist(&source.bytes, &source.label)?;
        let rows = plist.as_array().unwrap_or_default();
        let mut stats = SsedIosTableListResolutionStats::default();
        for row in rows {
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
            let raw_block = block;
            let raw_offset = plist_u32(dict, "offset").unwrap_or(0);
            let (block, offset) = self.convert_ios_ssed_address(raw_block, raw_offset)?;
            let mut row_diagnostics = Vec::new();
            let resolved = self
                .ssed_target_for_loose_address(block, offset, &mut row_diagnostics)?
                .is_some();
            stats.record(raw_block, raw_offset, block, offset, resolved);
            if resolved {
                return Ok((
                    NavigationStatus::Available,
                    vec![Diagnostic::info(
                        "ssed_ios_table_list",
                        "iOS tableList.plist exposes table/index entry targets",
                    )],
                ));
            }
        }
        if stats.address_rows == 0 {
            return Ok((
                NavigationStatus::Empty,
                vec![Diagnostic::info(
                    "ssed_ios_table_list_empty",
                    "iOS tableList.plist did not contain targetable address rows",
                )],
            ));
        }
        if let Some(owner) = self.ssed_ios_table_list_cross_book_owner(source, rows)? {
            return Ok((
                NavigationStatus::Available,
                vec![
                    Diagnostic::info(
                        "ssed_ios_table_list_cross_book",
                        "iOS tableList.plist rows resolve to a sibling SSED dictionary in the same iOS package set",
                    )
                    .with_context("dict_code", owner.dict_code)
                    .with_context("component", owner.component),
                ],
            ));
        }
        Ok((
            NavigationStatus::Deferred,
            vec![self.ssed_ios_table_list_unresolved_diagnostic(&stats)],
        ))
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
                anchor: item.anchor.clone(),
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

    pub(super) fn open_ssed_ios_dictlist_other_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        if surface_id != IOS_DICTLIST_OTHER_SURFACE_ID {
            return Ok(surface_open_deferred(surface_id));
        }
        let items = self.ssed_ios_dictlist_other_items()?;
        let offset = decode_offset_cursor(cursor);
        let mut page = Vec::new();
        let mut has_more = false;
        for item in items.into_iter().skip(offset) {
            if page.len() >= limit {
                has_more = true;
                break;
            }
            let target = TargetToken::new(&InternalTarget::SsedIosHtmlPage {
                source_id: IOS_DICTLIST_OTHER_SOURCE_ID.to_owned(),
                index: item.index,
                anchor: item.anchor.clone(),
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
        let plist = self.cached_ssed_panel_plist(&source.bytes, &source.label)?;
        let rows = plist.as_array().unwrap_or_default();
        let cross_book_owner = self.ssed_ios_table_list_cross_book_owner(&source, rows)?;
        let offset = decode_offset_cursor(cursor);
        let mut items = Vec::new();
        let mut has_more = false;
        let mut stats = SsedIosTableListResolutionStats::default();
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
            let raw_block = block;
            let raw_offset = plist_u32(dict, "offset").unwrap_or(0);
            let (block, offset) = self.convert_ios_ssed_address(raw_block, raw_offset)?;
            let mut row_diagnostics = Vec::new();
            let local_target =
                self.ssed_target_for_loose_address(block, offset, &mut row_diagnostics)?;
            let target = if let Some(target) = local_target {
                stats.record(raw_block, raw_offset, block, offset, true);
                target
            } else if let Some(owner) = &cross_book_owner {
                stats.record(raw_block, raw_offset, block, offset, true);
                TargetToken::new(&InternalTarget::SsedCrossBookAddress {
                    dict_code: owner.dict_code.clone(),
                    component: owner.component.clone(),
                    block: raw_block,
                    offset: raw_offset,
                })?
            } else {
                stats.record(raw_block, raw_offset, block, offset, false);
                continue;
            };
            let rich_label = self.ssed_rich_label_with_policy(&label, &options.gaiji_policy);
            let mut diagnostics = rich_label.diagnostics;
            if let Some(owner) = &cross_book_owner
                && matches!(
                    target.decode()?,
                    InternalTarget::SsedCrossBookAddress { .. }
                )
            {
                diagnostics.push(
                    Diagnostic::info(
                        "ssed_ios_table_list_cross_book_address",
                        "iOS tableList.plist row was exposed as a cross-book SSED address",
                    )
                    .with_context("dict_code", &owner.dict_code)
                    .with_context("component", &owner.component),
                );
            }
            items.push(NavigationItem {
                item_id: index.to_string(),
                label_html: rich_label.html,
                label_text: rich_label.text,
                target,
                href: String::new(),
                diagnostics,
            });
        }
        if stats.address_rows > 0 && items.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![self.ssed_ios_table_list_unresolved_diagnostic(&stats)],
            });
        }
        let next_cursor = has_more.then(|| offset.saturating_add(limit).to_string());
        Ok(NavigationSurface::TitleIndexBrowse {
            surface_id: surface_id.to_owned(),
            items,
            next_cursor,
        })
    }

    fn ssed_ios_table_list_unresolved_diagnostic(
        &self,
        stats: &SsedIosTableListResolutionStats,
    ) -> Diagnostic {
        let mut diagnostic = Diagnostic::info(
            "ssed_ios_table_list_unresolved",
            "iOS tableList.plist rows use an address namespace that did not resolve to retained SSED or sidecar entry targets",
        )
        .with_context("address_rows", stats.address_rows.to_string())
        .with_context("unresolved_rows", stats.unresolved_rows.to_string())
        .with_context("converted_rows", stats.converted_rows.to_string())
        .with_context(
            "convert_addr_payloads",
            self.retained_ios_convert_addr_payloads.len().to_string(),
        );
        if let Some(value) = stats.raw_min_block {
            diagnostic = diagnostic.with_context("raw_min_block", value.to_string());
        }
        if let Some(value) = stats.raw_max_block {
            diagnostic = diagnostic.with_context("raw_max_block", value.to_string());
        }
        if let Some(value) = stats.raw_min_offset {
            diagnostic = diagnostic.with_context("raw_min_offset", value.to_string());
        }
        if let Some(value) = stats.raw_max_offset {
            diagnostic = diagnostic.with_context("raw_max_offset", value.to_string());
        }
        if let Some(value) = stats.converted_min_block {
            diagnostic = diagnostic.with_context("converted_min_block", value.to_string());
        }
        if let Some(value) = stats.converted_max_block {
            diagnostic = diagnostic.with_context("converted_max_block", value.to_string());
        }
        if let Some(catalog) = &self.ssed_catalog {
            let component_ranges = catalog
                .components
                .iter()
                .filter(|component| component.has_positive_range());
            let min_component_block = component_ranges
                .clone()
                .map(|component| component.start_block)
                .min();
            let max_component_block = component_ranges.map(|component| component.end_block).max();
            if let Some(value) = min_component_block {
                diagnostic = diagnostic.with_context("component_min_block", value.to_string());
            }
            if let Some(value) = max_component_block {
                diagnostic = diagnostic.with_context("component_max_block", value.to_string());
            }
            if let Some(component) = catalog
                .components_by_role(SsedComponentRole::Honmon)
                .find(|component| component.has_positive_range())
            {
                diagnostic = diagnostic
                    .with_context("honmon_start_block", component.start_block.to_string())
                    .with_context("honmon_end_block", component.end_block.to_string());
            }
        }
        diagnostic
    }

    fn ssed_ios_table_list_cross_book_owner(
        &self,
        source: &SsedIosPlistSurfaceSource,
        rows: &[PlistValue],
    ) -> Result<Option<SsedIosTableListCrossBookOwner>> {
        let addresses = rows
            .iter()
            .filter_map(ssed_ios_table_list_raw_address)
            .collect::<Vec<_>>();
        if addresses.is_empty() {
            return Ok(None);
        }
        for candidate_root in self.ssed_ios_table_list_sibling_package_roots()? {
            let Some(candidate_bytes) =
                ssed_ios_table_list_candidate_bytes(&candidate_root, &source.source_id)
            else {
                continue;
            };
            if candidate_bytes != source.bytes {
                continue;
            }
            let Ok(catalog) = ssed_catalog_for_root(&candidate_root) else {
                continue;
            };
            let Some(honmon) = catalog.honmon() else {
                continue;
            };
            if !addresses
                .iter()
                .all(|(block, offset)| *offset < BLOCK_SIZE && honmon.contains_block(*block))
            {
                continue;
            }
            let Some(dict_code) = ssed_ios_sibling_dict_code(&candidate_root) else {
                continue;
            };
            return Ok(Some(SsedIosTableListCrossBookOwner {
                dict_code,
                component: honmon.filename.clone(),
            }));
        }
        Ok(None)
    }

    fn ssed_ios_table_list_sibling_package_roots(&self) -> Result<Vec<PathBuf>> {
        let Some(wrapper_root) = self.root.parent() else {
            return Ok(Vec::new());
        };
        let Some(collection_root) = wrapper_root.parent() else {
            return Ok(Vec::new());
        };
        let mut roots = Vec::new();
        for entry in fs::read_dir(collection_root)? {
            let path = entry?.path();
            if !path.is_dir() || path == wrapper_root {
                continue;
            }
            if path != self.root && path.is_dir() {
                roots.push(path.clone());
            }
            if let Some(name) = path.file_name() {
                let nested = path.join(name);
                if nested != self.root && nested.is_dir() {
                    roots.push(nested);
                }
            }
        }
        roots.sort();
        roots.dedup();
        Ok(roots)
    }

    pub(super) fn resolve_ssed_ios_table_list_window(
        &self,
        target: &TargetToken,
        surface_id: &str,
        cursor_hint: Option<&str>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let Some(source_id) = surface_id.strip_prefix(IOS_TABLE_LIST_PREFIX) else {
            return Ok(None);
        };
        let Some(source) = self
            .ssed_ios_table_list_sources()?
            .into_iter()
            .find(|source| source.source_id.eq_ignore_ascii_case(source_id))
        else {
            return Ok(Some(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![Diagnostic::info(
                    "sequence_deferred",
                    "iOS tableList.plist order is unavailable for this target",
                )],
            }));
        };
        let plist = self.cached_ssed_panel_plist(&source.bytes, &source.label)?;
        let rows = plist.as_array().unwrap_or_default();

        if let Some(cursor_index) = cursor_hint.and_then(|value| value.parse::<usize>().ok())
            && let Some(window) = self.resolve_ssed_ios_table_list_window_from_cursor(
                target,
                rows,
                cursor_index,
                before,
                after,
                options,
            )?
        {
            return Ok(Some(window));
        }

        let mut diagnostics = Vec::new();
        let mut ordered = Vec::new();
        for row in rows.iter() {
            if let Some(item) =
                self.ssed_ios_table_list_ordered_target_for_row(row, options, &mut diagnostics)?
            {
                ordered.push(item);
            }
        }
        let mut window = self.resolve_ordered_target_window(
            target,
            &ordered,
            before,
            after,
            options,
            Diagnostic::info(
                "sequence_target_not_in_ios_table_list",
                "target is not present in the iOS tableList.plist order",
            ),
        )?;
        window.diagnostics.extend(diagnostics);
        Ok(Some(window))
    }

    fn resolve_ssed_ios_table_list_window_from_cursor(
        &self,
        target: &TargetToken,
        rows: &[PlistValue],
        cursor_index: usize,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let Some(row) = rows.get(cursor_index) else {
            return Ok(None);
        };
        let mut diagnostics = Vec::new();
        let Some(center) =
            self.ssed_ios_table_list_ordered_target_for_row(row, options, &mut diagnostics)?
        else {
            return Ok(None);
        };
        if !sequence_targets_match(&center.target, target) {
            return Ok(None);
        }

        let mut ordered = Vec::new();
        let mut before_items = Vec::new();
        for index in (0..cursor_index).rev() {
            if before_items.len() >= before {
                break;
            }
            if let Some(item) = self.ssed_ios_table_list_ordered_target_for_row(
                &rows[index],
                options,
                &mut diagnostics,
            )? {
                before_items.push(item);
            }
        }
        before_items.reverse();
        ordered.extend(before_items);
        ordered.push(center);
        for row in rows.iter().skip(cursor_index.saturating_add(1)) {
            if ordered.len() >= before.saturating_add(1).saturating_add(after) {
                break;
            }
            if let Some(item) =
                self.ssed_ios_table_list_ordered_target_for_row(row, options, &mut diagnostics)?
            {
                ordered.push(item);
            }
        }

        let mut window = self.resolve_ordered_target_window(
            target,
            &ordered,
            before,
            after,
            options,
            Diagnostic::info(
                "sequence_target_not_in_ios_table_list",
                "target is not present in the iOS tableList.plist cursor window",
            ),
        )?;
        window.diagnostics.extend(diagnostics);
        Ok(Some(window))
    }

    fn ssed_ios_table_list_ordered_target_for_row(
        &self,
        row: &PlistValue,
        options: &RenderOptions,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Option<OrderedSequenceTarget>> {
        let Some(dict) = row.as_dict() else {
            return Ok(None);
        };
        let label = plist_string(dict, &["name", "item", "title", "label"]);
        if label.trim().is_empty() {
            return Ok(None);
        }
        let Some(block) = plist_u32(dict, "block").filter(|value| *value > 0) else {
            return Ok(None);
        };
        let (block, offset) =
            self.convert_ios_ssed_address(block, plist_u32(dict, "offset").unwrap_or(0))?;
        let target = self.ssed_target_for_loose_address(block, offset, diagnostics)?;
        let Some(target) = target else {
            return Ok(None);
        };
        let rich_label = self.ssed_rich_label_with_policy(&label, &options.gaiji_policy);
        diagnostics.extend(rich_label.diagnostics);
        Ok(Some(OrderedSequenceTarget {
            target,
            title: Some(rich_label.text),
        }))
    }

    pub(super) fn ssed_ios_html_list_item(
        &self,
        source_id: &str,
        index: u32,
    ) -> Result<Option<SsedIosHtmlListItem>> {
        let items = if source_id.eq_ignore_ascii_case(IOS_DICTLIST_OTHER_SOURCE_ID) {
            self.ssed_ios_dictlist_other_items()?
        } else {
            self.ssed_ios_html_list_items(source_id)?
        };
        Ok(items.into_iter().find(|item| item.index == index))
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
                path: None,
                anchor: None,
            });
        }
        Ok(items)
    }

    pub(super) fn ssed_ios_dictlist_other_items(&self) -> Result<Vec<SsedIosHtmlListItem>> {
        let Some(source) = self.ssed_ios_plist_file_by_source_id("DictList.plist")? else {
            return Ok(Vec::new());
        };
        let plist = parse_xml_plist(&source.bytes, &source.label)?;
        let Some(dict) = plist.as_dict() else {
            return Ok(Vec::new());
        };
        let Some(statuses) = dict.get("StatusArray").and_then(PlistValue::as_array) else {
            return Ok(Vec::new());
        };

        let mut items = Vec::new();
        for status in statuses.iter().filter_map(PlistValue::as_dict) {
            let Some(other_rows) = status.get("Other").and_then(PlistValue::as_array) else {
                continue;
            };
            for row in other_rows.iter().filter_map(PlistValue::as_dict) {
                let label_text = plist_string(row, &["key", "name", "item", "title", "label"]);
                if label_text.trim().is_empty() {
                    continue;
                }
                let Some(path_value) = row.get("path").and_then(PlistValue::as_str) else {
                    continue;
                };
                let Some(reference) = package_relative_html_reference("", path_value) else {
                    continue;
                };
                if !path_has_extension(&reference.path, &["html", "htm"])
                    || !self.storage.exists(Path::new(&reference.path))?
                {
                    continue;
                }
                let bytes = self.storage.read(Path::new(&reference.path))?;
                let html = decode_package_html_text(&bytes);
                let index = u32::try_from(items.len()).unwrap_or(u32::MAX);
                items.push(SsedIosHtmlListItem {
                    index,
                    label_html: escape_plain_label_html(&label_text),
                    label_text,
                    html,
                    path: Some(reference.path),
                    anchor: reference.anchor,
                });
            }
        }
        Ok(items)
    }

    fn ssed_ios_plist_files(&self) -> Result<Vec<SsedIosPlistFile>> {
        let cached = self.ssed_ios_plist_files.get_or_init(|| {
            let mut files = Vec::new();
            let mut seen = BTreeSet::new();
            collect_ios_plist_files_from_base(&self.root, "", &mut files, &mut seen)
                .map_err(|error| error.to_string())?;
            if let Some(parent) = self.root.parent() {
                collect_ios_plist_files_from_base(parent, "", &mut files, &mut seen)
                    .map_err(|error| error.to_string())?;
            }
            files.sort_by(|left, right| left.source_id.cmp(&right.source_id));
            Ok(files)
        });
        match cached {
            Ok(files) => Ok(files.clone()),
            Err(error) => Err(Error::Driver(error.clone())),
        }
    }

    pub(super) fn ssed_ios_plist_file_by_source_id(
        &self,
        source_id: &str,
    ) -> Result<Option<SsedIosPlistFile>> {
        Ok(self
            .ssed_ios_plist_files()?
            .into_iter()
            .find(|file| file.source_id.eq_ignore_ascii_case(source_id)))
    }
}

#[derive(Debug, Clone)]
pub(super) struct SsedIosPlistFile {
    pub(super) source_id: String,
    pub(super) label: String,
    pub(super) bytes: Vec<u8>,
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

pub(super) fn is_ssed_ios_panel_plist_candidate(source_id: &str) -> bool {
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

pub(super) fn is_ssed_ios_dictlist_other_surface_id(surface_id: &str) -> bool {
    surface_id == IOS_DICTLIST_OTHER_SURFACE_ID
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
    keys.iter()
        .filter_map(|key| dict.get(*key))
        .filter_map(plist_value_label_text)
        .find(|value| !value.is_empty())
}

fn plist_value_label_text(value: &PlistValue) -> Option<String> {
    ssed_panel_plist_value_label_text(value)
}

fn plist_u32(dict: &BTreeMap<String, PlistValue>, key: &str) -> Option<u32> {
    dict.get(key)
        .and_then(PlistValue::as_i64)
        .and_then(|value| u32::try_from(value).ok())
}

fn ssed_ios_table_list_raw_address(row: &PlistValue) -> Option<(u32, u32)> {
    let dict = row.as_dict()?;
    let label = plist_string(dict, &["name", "item", "title", "label"]);
    if label.trim().is_empty() {
        return None;
    }
    let block = plist_u32(dict, "block").filter(|value| *value > 0)?;
    let offset = plist_u32(dict, "offset").unwrap_or(0);
    Some((block, offset))
}

fn ssed_ios_table_list_candidate_bytes(candidate_root: &Path, source_id: &str) -> Option<Vec<u8>> {
    let direct = candidate_root.join(source_id);
    if direct.is_file() {
        return fs::read(direct).ok();
    }
    let wrapper = candidate_root.parent()?.join(source_id);
    if wrapper.is_file() {
        return fs::read(wrapper).ok();
    }
    None
}

fn ssed_ios_sibling_dict_code(candidate_root: &Path) -> Option<String> {
    let value = candidate_root.file_name()?.to_string_lossy();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(
        trimmed
            .strip_prefix("_DCT_")
            .unwrap_or(trimmed)
            .to_ascii_uppercase(),
    )
}

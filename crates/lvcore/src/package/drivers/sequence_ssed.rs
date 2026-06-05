use super::sequence::sequence_targets_match;
use super::ssed_navigation::{
    decode_ssed_menu_node_cursor, nearest_higher_menu_destination, ssed_menu_destination_target,
    ssed_menu_link_display_label, ssed_menu_record_target,
};
use super::*;
use std::collections::VecDeque;

const SSED_SEQUENCE_SURFACE_PAGE_LIMIT: usize = 128;
const SSED_SEQUENCE_SURFACE_MAX_PAGES: usize = 4096;

struct SsedPagedMenuWindowRequest<'a> {
    surface_id: &'a str,
    cursor_hint: Option<&'a str>,
    target: &'a TargetToken,
    before: usize,
    after: usize,
    options: &'a RenderOptions,
    max_pages: usize,
}

struct SsedTitleIndexWindowScanRequest<'a> {
    target: &'a TargetToken,
    component: &'a str,
    block: u32,
    offset: u32,
    before: usize,
    after: usize,
    options: &'a RenderOptions,
    component_filter: Option<&'a str>,
    start_offset: Option<usize>,
    allow_miss: bool,
}

impl ReaderBookPackage {
    pub(super) fn resolve_ssed_title_index_window(
        &self,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let (component, block, offset) = match target.decode()? {
            InternalTarget::SsedAddress {
                component,
                block,
                offset,
            }
            | InternalTarget::SsedBoundedAddress {
                component,
                block,
                offset,
                ..
            }
            | InternalTarget::SsedIndexAddress {
                component,
                block,
                offset,
                ..
            } => (component, block, offset),
            _ => return Ok(None),
        };

        if let Some(cursor) = ssed_title_index_cursor(sequence_hint, before)
            && let Some(window) =
                self.resolve_ssed_title_index_window_scan(SsedTitleIndexWindowScanRequest {
                    target,
                    component: &component,
                    block,
                    offset,
                    before,
                    after,
                    options,
                    component_filter: Some(cursor.component.as_str()),
                    start_offset: Some(cursor.start_offset),
                    allow_miss: true,
                })?
        {
            return Ok(Some(window));
        }

        self.resolve_ssed_title_index_window_scan(SsedTitleIndexWindowScanRequest {
            target,
            component: &component,
            block,
            offset,
            before,
            after,
            options,
            component_filter: None,
            start_offset: None,
            allow_miss: false,
        })
    }

    fn resolve_ssed_title_index_window_scan(
        &self,
        request: SsedTitleIndexWindowScanRequest<'_>,
    ) -> Result<Option<TargetWindow>> {
        let skip_backward_rows = self.ssed_has_forward_browse_index();
        let mut scanned_any = false;
        let mut row_ordinal = 0usize;
        let mut before_rows = VecDeque::<SsedIndexRow>::with_capacity(request.before);
        let mut center_row = None::<SsedIndexRow>;
        let mut tail_rows = Vec::<SsedIndexRow>::with_capacity(request.after.saturating_add(1));
        let mut diagnostics = self.scan_ssed_simple_index_rows_with_filters(
            None,
            |component| {
                if skip_backward_rows && ssed_index_component_name_is_backward(&component.filename)
                {
                    return false;
                }
                if let Some(filter) = request.component_filter {
                    return component.filename.eq_ignore_ascii_case(filter);
                }
                true
            },
            |_, _| true,
            |row| {
                let current_ordinal = row_ordinal;
                row_ordinal = row_ordinal.saturating_add(1);
                if request
                    .start_offset
                    .is_some_and(|start| current_ordinal < start)
                {
                    return Ok(true);
                }
                scanned_any = true;
                let Some(row_component) = self.ssed_component_for_index_pointer(row.body) else {
                    return Ok(true);
                };
                let row_matches = row.body.block == request.block
                    && row.body.offset == request.offset
                    && row_component.eq_ignore_ascii_case(request.component);
                if center_row.is_some() {
                    tail_rows.push(row);
                    return Ok(tail_rows.len() <= request.after);
                }
                if row_matches {
                    center_row = Some(row);
                    return Ok(true);
                }
                if request.before > 0 {
                    if before_rows.len() >= request.before {
                        before_rows.pop_front();
                    }
                    before_rows.push_back(row);
                }
                Ok(true)
            },
        )?;
        if !scanned_any {
            if request.allow_miss {
                return Ok(None);
            }
            diagnostics.push(Diagnostic::info(
                "sequence_deferred",
                "SSED title/index order is unavailable for this target",
            ));
            return Ok(Some(TargetWindow {
                center: self.render_target(request.target, request.options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics,
            }));
        }

        let Some(center_row) = center_row else {
            if request.allow_miss {
                return Ok(None);
            }
            diagnostics.push(Diagnostic::info(
                "sequence_target_not_in_title_index",
                "target is not present in the simple SSED title/index order",
            ));
            return Ok(Some(TargetWindow {
                center: self.render_target(request.target, request.options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics,
            }));
        };

        let mut context_rows = Vec::with_capacity(
            before_rows
                .len()
                .saturating_add(1)
                .saturating_add(tail_rows.len()),
        );
        context_rows.extend(before_rows.iter().cloned());
        let center_index = context_rows.len();
        context_rows.push(center_row.clone());
        context_rows.extend(tail_rows.iter().cloned());

        let center = match self.render_ssed_index_row(
            &center_row,
            next_distinct_index_row(&context_rows, center_index),
            request.options,
            &mut diagnostics,
        )? {
            Some(view) => view,
            None => self.render_target(request.target, request.options)?,
        };

        let mut before_views = Vec::new();
        for index in 0..center_index {
            if let Some(view) = self.render_ssed_index_row(
                &context_rows[index],
                next_distinct_index_row(&context_rows, index),
                request.options,
                &mut diagnostics,
            )? {
                before_views.push(view);
            }
        }
        let mut after_views = Vec::new();
        for index in center_index + 1..context_rows.len().min(center_index + 1 + request.after) {
            if let Some(view) = self.render_ssed_index_row(
                &context_rows[index],
                next_distinct_index_row(&context_rows, index),
                request.options,
                &mut diagnostics,
            )? {
                after_views.push(view);
            }
        }

        Ok(Some(TargetWindow {
            center,
            before: before_views,
            after: after_views,
            diagnostics,
        }))
    }

    fn render_ssed_index_row(
        &self,
        row: &SsedIndexRow,
        next_row: Option<&SsedIndexRow>,
        options: &RenderOptions,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Option<ResolvedTargetView>> {
        let target = match self.ssed_browse_target_for_index_row(row, next_row)? {
            Ok(target) => target,
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                return Ok(None);
            }
        };
        let mut view = self.render_target(&target, options)?;
        let label = self.ssed_index_row_label_with_policy(row, &options.gaiji_policy);
        view.title = Some(label.text);
        view.diagnostics.extend(label.diagnostics);
        Ok(Some(view))
    }

    pub(super) fn resolve_ssed_menu_window(
        &self,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let Some(SequenceHint::MenuOrder {
            value: surface_id,
            cursor,
        }) = sequence_hint
        else {
            return Ok(None);
        };
        if let Some(component_name) = ssed_direct_menu_component_name(surface_id) {
            if let Some(window) =
                self.resolve_ssed_paged_menu_window(SsedPagedMenuWindowRequest {
                    surface_id,
                    cursor_hint: cursor.as_deref(),
                    target,
                    before,
                    after,
                    options,
                    max_pages: 1,
                })?
            {
                return Ok(Some(window));
            }
            return Ok(Some(self.resolve_ssed_direct_menu_window(
                surface_id,
                component_name,
                target,
                before,
                after,
                options,
            )?));
        }
        self.resolve_ssed_paged_menu_window(SsedPagedMenuWindowRequest {
            surface_id,
            cursor_hint: cursor.as_deref(),
            target,
            before,
            after,
            options,
            max_pages: SSED_SEQUENCE_SURFACE_MAX_PAGES,
        })
    }

    fn resolve_ssed_paged_menu_window(
        &self,
        request: SsedPagedMenuWindowRequest<'_>,
    ) -> Result<Option<TargetWindow>> {
        let label_options = LabelOptions {
            gaiji_policy: request.options.gaiji_policy.clone(),
        };
        let mut collector = SsedMenuSequenceWindowCollector::new(request.before, request.after);
        let mut diagnostics = Vec::new();
        let mut cursor = ssed_menu_sequence_start_cursor(request.cursor_hint, request.before);
        let mut reached_page_limit = false;
        let page_limit = SSED_SEQUENCE_SURFACE_PAGE_LIMIT.max(
            request
                .before
                .saturating_add(request.after)
                .saturating_add(1),
        );
        for page_index in 0..SSED_SEQUENCE_SURFACE_MAX_PAGES.min(request.max_pages) {
            let surface = self.open_surface_page_with_options(
                request.surface_id,
                cursor.as_deref(),
                page_limit,
                &label_options,
            )?;
            let next_cursor = match surface {
                NavigationSurface::SimpleMenu {
                    nodes, next_cursor, ..
                }
                | NavigationSurface::HierarchicalTree {
                    nodes, next_cursor, ..
                } => {
                    collector.collect_nodes(&nodes, request.target);
                    next_cursor
                }
                NavigationSurface::Deferred {
                    diagnostics: surface_diagnostics,
                    ..
                } => {
                    diagnostics.extend(surface_diagnostics);
                    break;
                }
                _ => {
                    return Ok(Some(TargetWindow {
                        center: self.render_target(request.target, request.options)?,
                        before: Vec::new(),
                        after: Vec::new(),
                        diagnostics: vec![Diagnostic::info(
                            "sequence_surface_not_ordered",
                            format!(
                                "{} is not an ordered SSED navigation surface",
                                request.surface_id
                            ),
                        )],
                    }));
                }
            };
            if collector.is_satisfied() {
                break;
            }
            let Some(next_cursor) = next_cursor else {
                break;
            };
            if page_index + 1 >= SSED_SEQUENCE_SURFACE_MAX_PAGES {
                reached_page_limit = true;
                break;
            }
            if page_index + 1 >= request.max_pages {
                return Ok(None);
            }
            if cursor.as_deref() == Some(next_cursor.as_str()) {
                diagnostics.push(Diagnostic::warning(
                    "sequence_surface_cursor_stalled",
                    format!(
                        "{} returned a repeated pagination cursor",
                        request.surface_id
                    ),
                ));
                break;
            }
            cursor = Some(next_cursor);
        }
        if reached_page_limit {
            diagnostics.push(Diagnostic::warning(
                "sequence_surface_page_limit_reached",
                format!(
                    "{} sequence lookup stopped at the page limit",
                    request.surface_id
                ),
            ));
        }
        let ordered = collector.into_ordered_context();
        let mut window = if ordered.is_empty() {
            if request.max_pages < SSED_SEQUENCE_SURFACE_MAX_PAGES {
                return Ok(None);
            }
            TargetWindow {
                center: self.render_target(request.target, request.options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![Diagnostic::info(
                    "sequence_target_not_in_ssed_menu",
                    "target is not present in the requested SSED MENU/TOC order",
                )],
            }
        } else {
            self.resolve_ordered_target_window(
                request.target,
                &ordered,
                request.before,
                request.after,
                request.options,
                Diagnostic::info(
                    "sequence_target_not_in_ssed_menu",
                    "target is not present in the requested SSED MENU/TOC order",
                ),
            )?
        };
        window.diagnostics.extend(diagnostics);
        Ok(Some(window))
    }

    pub(super) fn resolve_ssed_panel_window(
        &self,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<Option<TargetWindow>> {
        let Some(SequenceHint::PanelOrder { value: panel_id }) = sequence_hint else {
            return Ok(None);
        };
        let surface_id = if panel_id == "panels" || panel_id.starts_with("panels:") {
            panel_id.clone()
        } else {
            format!("panels:{panel_id}")
        };
        let label_options = LabelOptions {
            gaiji_policy: options.gaiji_policy.clone(),
        };
        let mut collector = SsedMenuSequenceWindowCollector::new(before, after);
        let mut diagnostics = Vec::new();
        let mut cursor = None::<String>;
        let mut reached_page_limit = false;
        for page_index in 0..SSED_SEQUENCE_SURFACE_MAX_PAGES {
            let surface = self.open_surface_page_with_options(
                &surface_id,
                cursor.as_deref(),
                SSED_SEQUENCE_SURFACE_PAGE_LIMIT,
                &label_options,
            )?;
            let next_cursor = match surface {
                NavigationSurface::Panel {
                    cells, next_cursor, ..
                } => {
                    for cell in cells {
                        if let Some(cell_target) = cell.target {
                            let item = OrderedSequenceTarget {
                                target: cell_target,
                                title: Some(cell.label_text),
                            };
                            if !collector.visit(item, target) {
                                break;
                            }
                        }
                    }
                    next_cursor
                }
                _ => {
                    return Ok(Some(TargetWindow {
                        center: self.render_target(target, options)?,
                        before: Vec::new(),
                        after: Vec::new(),
                        diagnostics: vec![Diagnostic::info(
                            "sequence_surface_not_ordered",
                            format!("{surface_id} is not an SSED panel surface"),
                        )],
                    }));
                }
            };
            if collector.is_satisfied() {
                break;
            }
            let Some(next_cursor) = next_cursor else {
                break;
            };
            if page_index + 1 >= SSED_SEQUENCE_SURFACE_MAX_PAGES {
                reached_page_limit = true;
                break;
            }
            if cursor.as_deref() == Some(next_cursor.as_str()) {
                diagnostics.push(Diagnostic::warning(
                    "sequence_surface_cursor_stalled",
                    format!("{surface_id} returned a repeated pagination cursor"),
                ));
                break;
            }
            cursor = Some(next_cursor);
        }
        if reached_page_limit {
            diagnostics.push(Diagnostic::warning(
                "sequence_surface_page_limit_reached",
                format!("{surface_id} sequence lookup stopped at the page limit"),
            ));
        }
        let ordered = collector.into_ordered_context();
        let mut window = if ordered.is_empty() {
            TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics: vec![Diagnostic::info(
                    "sequence_target_not_in_ssed_panel",
                    "target is not present in the requested SSED panel order",
                )],
            }
        } else {
            self.resolve_ordered_target_window(
                target,
                &ordered,
                before,
                after,
                options,
                Diagnostic::info(
                    "sequence_target_not_in_ssed_panel",
                    "target is not present in the requested SSED panel order",
                ),
            )?
        };
        window.diagnostics.extend(diagnostics);
        Ok(Some(window))
    }
}

fn ssed_direct_menu_component_name(surface_id: &str) -> Option<&'static str> {
    match surface_id {
        "menu" => Some("MENU.DIC"),
        "toc" => Some("TOC.DIC"),
        _ => None,
    }
}

fn ssed_menu_sequence_start_cursor(cursor: Option<&str>, before: usize) -> Option<String> {
    let cursor = cursor.map(str::trim).filter(|cursor| !cursor.is_empty())?;
    let parsed = decode_ssed_menu_node_cursor(Some(cursor));
    if parsed.link_offset > 0 {
        let link_offset = parsed.link_offset.saturating_sub(before);
        return Some(format!("link:{}:{link_offset}", parsed.record_offset));
    }
    let record_offset = parsed.record_offset.saturating_sub(before);
    (record_offset > 0).then(|| record_offset.to_string())
}

impl ReaderBookPackage {
    fn resolve_ssed_direct_menu_window(
        &self,
        surface_id: &str,
        component_name: &str,
        target: &TargetToken,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<TargetWindow> {
        let mut diagnostics = Vec::new();
        let Some(catalog) = &self.ssed_catalog else {
            diagnostics.push(Diagnostic::info(
                "ssed_catalog_missing",
                "SSED MENU/TOC sequence lookup requires a parsed SSEDINFO catalog",
            ));
            return Ok(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics,
            });
        };
        let Some(component) = catalog
            .component_named(component_name)
            .filter(|component| component.has_positive_range())
        else {
            diagnostics.push(Diagnostic::info(
                "ssed_navigation_component_missing",
                format!("{component_name} is not declared in this SSED catalog"),
            ));
            return Ok(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics,
            });
        };
        let data = match self.decoded_ssed_navigation_component_data(component) {
            Ok(data) => data,
            Err(error) => {
                diagnostics.push(Diagnostic::warning(
                    "ssed_navigation_component_decode_failed",
                    format!("{} is not readable as SSEDDATA: {error}", component.filename),
                ));
                return Ok(TargetWindow {
                    center: self.render_target(target, options)?,
                    before: Vec::new(),
                    after: Vec::new(),
                    diagnostics,
                });
            }
        };
        let parsed = parse_menu_stream(&data);
        if parsed.empty_sentinel || parsed.records.is_empty() {
            let (code, message) = if parsed.empty_sentinel {
                (
                    "ssed_navigation_empty_sentinel",
                    format!(
                        "{} contains an explicit empty navigation sentinel",
                        component.filename
                    ),
                )
            } else {
                (
                    "ssed_navigation_empty",
                    format!("{} did not decode any navigation rows", component.filename),
                )
            };
            diagnostics.push(Diagnostic::info(code, message));
            return Ok(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics,
            });
        }
        if parsed.unknown_controls > 0 {
            diagnostics.push(Diagnostic::info(
                "ssed_navigation_unknown_controls",
                format!(
                    "{} contained {} unknown MENU/TOC controls while resolving sequence order",
                    component.filename, parsed.unknown_controls
                ),
            ));
        }

        let mut collector = SsedMenuSequenceWindowCollector::new(before, after);
        for record in &parsed.records {
            self.collect_ssed_direct_menu_record_sequence_targets(
                &parsed.records,
                record,
                &mut collector,
                target,
                &options.gaiji_policy,
            )?;
            if collector.is_satisfied() {
                break;
            }
        }

        let ordered = collector.into_ordered_context();
        if ordered.is_empty() {
            diagnostics.push(Diagnostic::info(
                "sequence_target_not_in_ssed_menu",
                format!("target is not present in the requested SSED {surface_id} order"),
            ));
            return Ok(TargetWindow {
                center: self.render_target(target, options)?,
                before: Vec::new(),
                after: Vec::new(),
                diagnostics,
            });
        }
        let mut window = self.resolve_ordered_target_window(
            target,
            &ordered,
            before,
            after,
            options,
            Diagnostic::info(
                "sequence_target_not_in_ssed_menu",
                format!("target is not present in the requested SSED {surface_id} order"),
            ),
        )?;
        window.diagnostics.extend(diagnostics);
        Ok(window)
    }

    fn collect_ssed_direct_menu_record_sequence_targets(
        &self,
        records: &[SsedMenuRecord],
        record: &SsedMenuRecord,
        collector: &mut SsedMenuSequenceWindowCollector,
        target: &TargetToken,
        gaiji_policy: &GaijiPolicy,
    ) -> Result<()> {
        let target_links = record
            .links
            .iter()
            .filter(|link| {
                link.destination
                    .as_ref()
                    .is_some_and(|destination| !destination.is_null())
                    && !link.label.trim().is_empty()
            })
            .collect::<Vec<_>>();
        if target_links.len() > 1 {
            for link in target_links {
                let Some(destination) = link.destination.as_ref() else {
                    continue;
                };
                let mut target_diagnostics = Vec::new();
                let Some(menu_target) = ssed_menu_destination_target(
                    self,
                    destination,
                    nearest_higher_menu_destination(records, destination),
                    &mut target_diagnostics,
                )?
                else {
                    continue;
                };
                let label = ssed_menu_link_display_label(&link.label);
                if label.is_empty() {
                    continue;
                }
                let rich_label = self.ssed_rich_label_with_policy(&label, gaiji_policy);
                let item = OrderedSequenceTarget {
                    target: menu_target,
                    title: Some(rich_label.text),
                };
                if !collector.visit(item, target) {
                    return Ok(());
                }
            }
            return Ok(());
        }

        let label = record.label();
        if label.is_empty() {
            return Ok(());
        }
        let mut target_diagnostics = Vec::new();
        let Some(menu_target) =
            ssed_menu_record_target(self, records, record, &mut target_diagnostics)?
        else {
            return Ok(());
        };
        let rich_label = self.ssed_rich_label_with_policy(label, gaiji_policy);
        let item = OrderedSequenceTarget {
            target: menu_target,
            title: Some(rich_label.text),
        };
        collector.visit(item, target);
        Ok(())
    }
}

struct SsedMenuSequenceWindowCollector {
    before_limit: usize,
    after_limit: usize,
    before: VecDeque<OrderedSequenceTarget>,
    center: Option<OrderedSequenceTarget>,
    after: Vec<OrderedSequenceTarget>,
}

impl SsedMenuSequenceWindowCollector {
    fn new(before_limit: usize, after_limit: usize) -> Self {
        Self {
            before_limit,
            after_limit,
            before: VecDeque::with_capacity(before_limit),
            center: None,
            after: Vec::with_capacity(after_limit),
        }
    }

    fn collect_nodes(&mut self, nodes: &[NavigationNode], target: &TargetToken) -> bool {
        for node in nodes {
            if let Some(node_target) = &node.target {
                let item = OrderedSequenceTarget {
                    target: node_target.clone(),
                    title: Some(node.label_text.clone()),
                };
                if !self.visit(item, target) {
                    return false;
                }
            }
            if !self.collect_nodes(&node.children, target) {
                return false;
            }
        }
        true
    }

    fn visit(&mut self, item: OrderedSequenceTarget, target: &TargetToken) -> bool {
        if self.center.is_some() {
            if self.after.len() < self.after_limit {
                self.after.push(item);
            }
            return !self.is_satisfied();
        }
        if sequence_targets_match(&item.target, target) {
            self.center = Some(item);
            return !self.is_satisfied();
        }
        if self.before_limit > 0 {
            if self.before.len() >= self.before_limit {
                self.before.pop_front();
            }
            self.before.push_back(item);
        }
        true
    }

    fn is_satisfied(&self) -> bool {
        self.center.is_some() && self.after.len() >= self.after_limit
    }

    fn into_ordered_context(self) -> Vec<OrderedSequenceTarget> {
        let Some(center) = self.center else {
            return Vec::new();
        };
        let mut ordered = Vec::with_capacity(
            self.before
                .len()
                .saturating_add(1)
                .saturating_add(self.after.len()),
        );
        ordered.extend(self.before);
        ordered.push(center);
        ordered.extend(self.after);
        ordered
    }
}
fn next_distinct_index_row(rows: &[SsedIndexRow], index: usize) -> Option<&SsedIndexRow> {
    let row = rows.get(index)?;
    rows.iter()
        .skip(index + 1)
        .find(|next| next.body != row.body)
}

struct SsedTitleIndexCursor {
    component: String,
    start_offset: usize,
}

fn ssed_title_index_cursor(
    sequence_hint: Option<&SequenceHint>,
    before: usize,
) -> Option<SsedTitleIndexCursor> {
    let Some(SequenceHint::TitleIndexOrder {
        cursor: Some(cursor),
        ..
    }) = sequence_hint
    else {
        return None;
    };
    let (component, offset) = cursor.rsplit_once(':')?;
    let offset = offset.parse::<usize>().ok()?;
    if component.is_empty() {
        return None;
    }
    Some(SsedTitleIndexCursor {
        component: component.to_owned(),
        start_offset: offset.saturating_sub(before),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssed_target(block: u32, offset: u32) -> TargetToken {
        TargetToken::new(&InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block,
            offset,
        })
        .unwrap()
    }

    fn node(label: &str, target: TargetToken, children: Vec<NavigationNode>) -> NavigationNode {
        NavigationNode {
            href: None,
            node_id: label.to_owned(),
            label_html: label.to_owned(),
            label_text: label.to_owned(),
            target: Some(target),
            diagnostics: Vec::new(),
            children,
        }
    }

    #[test]
    fn ssed_menu_sequence_collector_stops_after_requested_nested_window() {
        let center = ssed_target(10, 0);
        let after = ssed_target(10, 2);
        let skipped_child = ssed_target(10, 4);
        let skipped_sibling = ssed_target(10, 6);
        let nodes = vec![
            node(
                "center",
                center.clone(),
                vec![
                    node("after", after, Vec::new()),
                    node("skipped-child", skipped_child, Vec::new()),
                ],
            ),
            node("skipped-sibling", skipped_sibling, Vec::new()),
        ];

        let mut collector = SsedMenuSequenceWindowCollector::new(0, 1);
        assert!(
            !collector.collect_nodes(&nodes, &center),
            "collector should stop traversing once the after-window is complete"
        );
        let ordered = collector.into_ordered_context();
        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0].title.as_deref(), Some("center"));
        assert_eq!(ordered[1].title.as_deref(), Some("after"));
    }
}

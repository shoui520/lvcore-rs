use super::*;

impl ReaderBookPackage {
    fn ssed_navigation_home_surface(
        &self,
        surface_id: &str,
        kind: NavigationSurfaceKind,
        title: &str,
        role: SsedComponentRole,
        fallback_name: &str,
    ) -> Result<Option<HomeSurface>> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(None);
        };
        let Some(component) = catalog
            .component_named(fallback_name)
            .filter(|component| component.has_positive_range())
        else {
            return Ok(None);
        };

        match self.resolve_readable_ssed_component_path(component) {
            Ok(Some(_)) => {}
            Ok(None) => return Ok(None),
            Err(error) => {
                return Ok(Some(HomeSurface {
                    surface_id: surface_id.to_owned(),
                    kind,
                    status: NavigationStatus::Deferred,
                    title_html: title.to_owned(),
                    title_text: title.to_owned(),
                    target: None,
                    diagnostics: vec![
                        Diagnostic::warning(
                            "ssed_navigation_component_decode_failed",
                            format!(
                                "{} is present but not readable as SSEDDATA: {error}",
                                component.filename
                            ),
                        )
                        .with_context("component", &component.filename),
                    ],
                }));
            }
        }

        let empty_diagnostic = self.ssed_navigation_empty_diagnostic(role, fallback_name)?;
        let is_empty = empty_diagnostic.is_some();
        let target = if is_empty {
            None
        } else {
            Some(match role {
                SsedComponentRole::Toc => TargetToken::new(&InternalTarget::TocItem {
                    surface_id: surface_id.to_owned(),
                    item_id: "root".to_owned(),
                })?,
                _ => TargetToken::new(&InternalTarget::MenuItem {
                    surface_id: surface_id.to_owned(),
                    item_id: "root".to_owned(),
                })?,
            })
        };

        Ok(Some(HomeSurface {
            surface_id: surface_id.to_owned(),
            kind,
            status: if is_empty {
                NavigationStatus::Empty
            } else {
                NavigationStatus::Available
            },
            title_html: title.to_owned(),
            title_text: title.to_owned(),
            target,
            diagnostics: empty_diagnostic.into_iter().collect(),
        }))
    }

    fn ssed_multi_home_surfaces(&self) -> Result<Vec<HomeSurface>> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(Vec::new());
        };
        let mut surfaces = Vec::new();
        for component in catalog.components_by_role(SsedComponentRole::MultiDescriptor) {
            if !component.has_positive_range() {
                continue;
            }
            let surface_id = ssed_multi_root_surface_id(&component.filename);
            let title = format!("Multi Selector: {}", component.filename);
            match self.read_ssed_multi_descriptor(component) {
                Ok(descriptor) if !descriptor.records.is_empty() => {
                    surfaces.push(HomeSurface {
                        surface_id: surface_id.clone(),
                        kind: NavigationSurfaceKind::MultiSelector,
                        status: NavigationStatus::Available,
                        title_html: escape_plain_label_html(&title),
                        title_text: title,
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id,
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: Vec::new(),
                    });
                }
                Ok(_) => surfaces.push(HomeSurface {
                    surface_id,
                    kind: NavigationSurfaceKind::MultiSelector,
                    status: NavigationStatus::Empty,
                    title_html: escape_plain_label_html(&title),
                    title_text: title,
                    target: None,
                    diagnostics: vec![
                        Diagnostic::info(
                            "ssed_multi_descriptor_empty",
                            format!("{} did not decode any selector records", component.filename),
                        )
                        .with_context("component", &component.filename),
                    ],
                }),
                Err(error) => surfaces.push(HomeSurface {
                    surface_id,
                    kind: NavigationSurfaceKind::MultiSelector,
                    status: NavigationStatus::Deferred,
                    title_html: escape_plain_label_html(&title),
                    title_text: title,
                    target: None,
                    diagnostics: vec![
                        Diagnostic::warning(
                            "ssed_multi_descriptor_decode_failed",
                            format!("{} could not be decoded: {error}", component.filename),
                        )
                        .with_context("component", &component.filename),
                    ],
                }),
            }
        }
        Ok(surfaces)
    }

    fn read_ssed_multi_descriptor(&self, component: &SsedComponent) -> Result<SsedMultiDescriptor> {
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Err(Error::Driver(format!(
                "{} is declared but not present on disk",
                component.filename
            )));
        };
        let mut reader = SsedDataFile::open(path)?;
        let read_len = reader
            .header()
            .expanded_size()
            .min(SSED_NAVIGATION_DETECTION_MAX_BYTES);
        let data = reader.read_range(0, read_len)?;
        parse_multi_descriptor(&data)
    }
}

impl ReaderBookPackage {
    fn open_ssed_title_index_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        if limit == 0 {
            return Ok(NavigationSurface::TitleIndexBrowse {
                surface_id: surface_id.to_owned(),
                items: Vec::new(),
                next_cursor: None,
            });
        }
        let offset = decode_offset_cursor(cursor);
        let (mut rows, mut diagnostics) =
            self.ssed_simple_index_rows_page(offset, limit.saturating_add(1))?;
        let next_cursor = (rows.len() > limit).then(|| (offset + limit).to_string());
        rows.truncate(limit);
        if rows.is_empty() && !diagnostics.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics,
            });
        }
        let mut items = Vec::new();
        for (index, row) in rows.into_iter().enumerate() {
            let label = self
                .ssed_title_text(row.title)
                .unwrap_or_else(|| row.key.clone());
            let label = self.ssed_rich_label(&label);
            let target = match self.ssed_target_for_index_pointer(row.body)? {
                Ok(target) => target,
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    continue;
                }
            };
            items.push(NavigationItem {
                item_id: format!("{}:{}", row.component, offset + index),
                label_html: label.html,
                label_text: label.text,
                target,
                diagnostics: label.diagnostics,
            });
        }
        Ok(NavigationSurface::TitleIndexBrowse {
            surface_id: surface_id.to_owned(),
            items,
            next_cursor,
        })
    }

    fn open_ssed_menu_surface(
        &self,
        surface_id: &str,
        _role: SsedComponentRole,
        fallback_name: &str,
    ) -> Result<NavigationSurface> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(deferred_surface_error(
                surface_id,
                "ssed_catalog_missing",
                "SSED MENU/TOC surfaces require a parsed SSEDINFO catalog",
            ));
        };
        let Some(component) = catalog
            .component_named(fallback_name)
            .filter(|component| component.has_positive_range())
        else {
            return Ok(deferred_surface_info(
                surface_id,
                "ssed_navigation_component_missing",
                format!("{fallback_name} is not declared in this SSED catalog"),
            ));
        };
        let path = match self.resolve_readable_ssed_component_path(component) {
            Ok(Some(path)) => path,
            Ok(None) => {
                return Ok(deferred_component_surface_warning(
                    surface_id,
                    "ssed_navigation_component_file_missing",
                    format!("{} is declared but not present on disk", component.filename),
                    component,
                ));
            }
            Err(error) => {
                return Ok(deferred_component_surface_warning(
                    surface_id,
                    "ssed_navigation_component_decode_failed",
                    format!(
                        "{} is not readable as SSEDDATA: {error}",
                        component.filename
                    ),
                    component,
                ));
            }
        };
        let mut reader = match SsedDataFile::open(&path) {
            Ok(reader) => reader,
            Err(error) => {
                return Ok(deferred_component_surface_warning(
                    surface_id,
                    "ssed_navigation_component_decode_failed",
                    format!(
                        "{} is not readable as plain SSEDDATA: {error}",
                        component.filename
                    ),
                    component,
                ));
            }
        };
        let data = reader.read_range(0, reader.header().expanded_size())?;
        let parsed = parse_menu_stream(&data);
        if parsed.records.is_empty() {
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
            return Ok(deferred_component_surface_info(
                surface_id, code, message, component,
            ));
        }
        let mut diagnostics = Vec::new();
        let nodes = ssed_menu_records_to_nodes(self, &parsed.records, &mut diagnostics)?;
        if nodes.is_empty() {
            return Ok(deferred_surface(surface_id, diagnostics));
        }
        Ok(NavigationSurface::SimpleMenu {
            surface_id: surface_id.to_owned(),
            nodes,
        })
    }

    fn open_ssed_multi_selector_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        let Some(parsed_surface) = parse_ssed_multi_surface_id(surface_id) else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "ssed_multi_surface_id_invalid",
                    format!("{surface_id} is not a valid SSED MULTI selector surface id"),
                )],
            });
        };
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "ssed_catalog_missing",
                    "SSED MULTI selector surfaces require a parsed SSEDINFO catalog",
                )],
            });
        };
        let Some(component) = catalog.component_named(&parsed_surface.descriptor) else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "ssed_multi_descriptor_missing",
                    format!(
                        "{} is not declared in this SSED catalog",
                        parsed_surface.descriptor
                    ),
                )],
            });
        };
        let descriptor = match self.read_ssed_multi_descriptor(component) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                return Ok(NavigationSurface::Deferred {
                    surface_id: surface_id.to_owned(),
                    diagnostics: vec![
                        Diagnostic::warning(
                            "ssed_multi_descriptor_decode_failed",
                            format!("{} could not be decoded: {error}", component.filename),
                        )
                        .with_context("component", &component.filename),
                    ],
                });
            }
        };
        if let Some(record_index) = parsed_surface.record_index {
            let Some(record) = descriptor
                .records
                .iter()
                .find(|record| record.index == record_index)
            else {
                return Ok(NavigationSurface::Deferred {
                    surface_id: surface_id.to_owned(),
                    diagnostics: vec![Diagnostic::warning(
                        "ssed_multi_record_missing",
                        format!(
                            "{} does not contain selector record {record_index}",
                            parsed_surface.descriptor
                        ),
                    )],
                });
            };
            return self.open_ssed_multi_record_browse_surface(
                surface_id,
                record,
                parsed_surface.filter.as_deref(),
                cursor,
                limit,
            );
        }

        let mut diagnostics = Vec::new();
        let nodes = self.ssed_multi_descriptor_nodes(
            &parsed_surface.descriptor,
            &descriptor,
            &mut diagnostics,
        )?;
        if nodes.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics,
            });
        }
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
        })
    }

    fn open_ssed_multi_record_browse_surface(
        &self,
        surface_id: &str,
        record: &SsedMultiRecord,
        filter: Option<&str>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        if limit == 0 {
            return Ok(NavigationSurface::TitleIndexBrowse {
                surface_id: surface_id.to_owned(),
                items: Vec::new(),
                next_cursor: None,
            });
        }
        let Some(index_ref) = ssed_multi_record_index_ref(record) else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_multi_index_missing",
                    format!(
                        "MULTI record {} does not reference a supported index component",
                        record.index
                    ),
                )],
            });
        };
        let Some(index_component) = self.ssed_component_for_multi_ref(index_ref) else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "ssed_multi_index_component_missing",
                    format!(
                        "MULTI record {} points to component type {:02x} blocks {}..{}, but no catalog component matches",
                        record.index,
                        index_ref.component_type,
                        index_ref.start_block,
                        index_ref
                            .start_block
                            .saturating_add(index_ref.block_count.saturating_sub(1))
                    ),
                )],
            });
        };
        let offset = decode_offset_cursor(cursor);
        let filter_normalized = filter.map(normalize_search_match_text);
        let mut seen = 0usize;
        let mut rows = Vec::new();
        let mut diagnostics =
            self.scan_ssed_index_component_rows(index_component, None, |row| {
                let row_matches = filter_normalized
                    .as_ref()
                    .is_none_or(|filter| normalize_search_match_text(&row.key) == *filter);
                if row_matches {
                    if seen >= offset {
                        rows.push(row);
                    }
                    seen = seen.saturating_add(1);
                }
                Ok(rows.len() < limit.saturating_add(1))
            })?;
        let next_cursor = (rows.len() > limit).then(|| (offset + limit).to_string());
        rows.truncate(limit);
        if rows.is_empty() && !diagnostics.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics,
            });
        }
        let mut items = Vec::new();
        for (index, row) in rows.into_iter().enumerate() {
            let label = self
                .ssed_title_text(row.title)
                .unwrap_or_else(|| row.target_key.clone());
            let label = self.ssed_rich_label(&label);
            let target = match self.ssed_target_for_index_pointer(row.body)? {
                Ok(target) => target,
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    continue;
                }
            };
            items.push(NavigationItem {
                item_id: format!("{}:{}", row.component, offset + index),
                label_html: label.html,
                label_text: label.text,
                target,
                diagnostics: label.diagnostics,
            });
        }
        Ok(NavigationSurface::TitleIndexBrowse {
            surface_id: surface_id.to_owned(),
            items,
            next_cursor,
        })
    }

    fn ssed_multi_descriptor_nodes(
        &self,
        descriptor_name: &str,
        descriptor: &SsedMultiDescriptor,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Vec<NavigationNode>> {
        let mut nodes = Vec::new();
        for record in &descriptor.records {
            let label = if record.label.is_empty() {
                format!("Record {}", record.index)
            } else {
                record.label.clone()
            };
            let rich_label = self.ssed_rich_label(&label);
            let children =
                self.ssed_multi_record_selector_nodes(descriptor_name, record, diagnostics)?;
            let target = if children.is_empty() && ssed_multi_record_index_ref(record).is_some() {
                Some(TargetToken::new(&InternalTarget::TitleIndexItem {
                    surface_id: ssed_multi_record_surface_id(descriptor_name, record.index, None),
                    item_id: "root".to_owned(),
                })?)
            } else {
                None
            };
            nodes.push(NavigationNode {
                node_id: format!("multi:{}:record:{}", descriptor_name, record.index),
                label_html: rich_label.html,
                label_text: rich_label.text,
                target,
                diagnostics: rich_label.diagnostics,
                children,
            });
        }
        Ok(nodes)
    }

    fn ssed_multi_record_selector_nodes(
        &self,
        descriptor_name: &str,
        record: &SsedMultiRecord,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Vec<NavigationNode>> {
        if ssed_multi_record_index_ref(record).is_none() {
            diagnostics.push(Diagnostic::info(
                "ssed_multi_index_missing",
                format!(
                    "MULTI record {} has selector labels but no supported index component",
                    record.index
                ),
            ));
            return Ok(Vec::new());
        }
        let Some(menu_ref) = ssed_multi_record_menu_ref(record) else {
            return Ok(Vec::new());
        };
        let Some(menu_component) = self.ssed_component_for_multi_ref(menu_ref) else {
            diagnostics.push(Diagnostic::warning(
                "ssed_multi_menu_component_missing",
                format!(
                    "MULTI record {} points to selector component type {:02x} block {}, but no catalog component matches",
                    record.index, menu_ref.component_type, menu_ref.start_block
                ),
            ));
            return Ok(Vec::new());
        };
        let data = match self.read_ssed_component_expanded_bytes(menu_component) {
            Ok(data) => data,
            Err(error) => {
                diagnostics.push(
                    Diagnostic::warning(
                        "ssed_multi_menu_component_decode_failed",
                        format!("{} could not be decoded: {error}", menu_component.filename),
                    )
                    .with_context("component", &menu_component.filename),
                );
                return Ok(Vec::new());
            }
        };
        let parsed = parse_menu_stream(&data);
        if parsed.records.is_empty() {
            diagnostics.push(
                Diagnostic::info(
                    "ssed_multi_menu_empty",
                    format!(
                        "{} did not decode any selector labels",
                        menu_component.filename
                    ),
                )
                .with_context("component", &menu_component.filename),
            );
            return Ok(Vec::new());
        }
        ssed_multi_selector_records_to_nodes(self, descriptor_name, record.index, &parsed.records)
    }

    fn ssed_component_for_multi_ref(
        &self,
        reference: &SsedMultiComponentRef,
    ) -> Option<&SsedComponent> {
        let catalog = self.ssed_catalog.as_ref()?;
        catalog.components.iter().find(|component| {
            component.component_type == reference.component_type
                && component.start_block == reference.start_block
                && component.block_count() == reference.block_count
        })
    }

    fn read_ssed_component_expanded_bytes(&self, component: &SsedComponent) -> Result<Vec<u8>> {
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Err(Error::Driver(format!(
                "{} is declared but not present on disk",
                component.filename
            )));
        };
        let mut reader = SsedDataFile::open(path)?;
        reader.read_range(0, reader.header().expanded_size())
    }

    pub(super) fn ssed_navigation_empty_diagnostic(
        &self,
        _role: SsedComponentRole,
        fallback_name: &str,
    ) -> Result<Option<Diagnostic>> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(None);
        };
        let Some(component) = catalog
            .component_named(fallback_name)
            .filter(|component| component.has_positive_range())
        else {
            return Ok(None);
        };
        let path = match self.resolve_readable_ssed_component_path(component) {
            Ok(Some(path)) => path,
            Ok(None) | Err(_) => return Ok(None),
        };
        let data = match read_ssed_navigation_detection_bytes(&path) {
            Ok(Some(data)) => data,
            Ok(None) | Err(_) => return Ok(None),
        };
        let parsed = parse_menu_stream(&data);
        if parsed.records.is_empty() {
            let diagnostic = if parsed.empty_sentinel {
                Diagnostic::info(
                    "ssed_navigation_empty_sentinel",
                    format!(
                        "{} contains an explicit empty navigation sentinel",
                        component.filename
                    ),
                )
                .with_context("component", &component.filename)
            } else {
                Diagnostic::info(
                    "ssed_navigation_empty",
                    format!("{} did not decode any navigation rows", component.filename),
                )
                .with_context("component", &component.filename)
            };
            return Ok(Some(diagnostic));
        }
        Ok(None)
    }

    fn open_ssed_screen_menu_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(deferred_surface_error(
                surface_id,
                "ssed_catalog_missing",
                "SSED screen-menu surfaces require a parsed SSEDINFO catalog",
            ));
        };
        let Some(component) = catalog
            .components_by_role(SsedComponentRole::ScreenMenu)
            .find(|component| component.has_positive_range())
            .or_else(|| catalog.component_named("SCRMENU.DIC"))
        else {
            return Ok(deferred_surface_info(
                surface_id,
                "ssed_screen_menu_missing",
                "SCRMENU.DIC is not declared in this SSED catalog",
            ));
        };
        let path = match self.resolve_readable_ssed_component_path(component) {
            Ok(Some(path)) => path,
            Ok(None) => {
                return Ok(deferred_component_surface_warning(
                    surface_id,
                    "ssed_screen_menu_file_missing",
                    format!("{} is declared but not present on disk", component.filename),
                    component,
                ));
            }
            Err(error) => {
                return Ok(deferred_component_surface_warning(
                    surface_id,
                    "ssed_screen_menu_decode_failed",
                    format!(
                        "{} is not readable as SSEDDATA: {error}",
                        component.filename
                    ),
                    component,
                ));
            }
        };
        let mut reader = SsedDataFile::open(&path)?;
        let data = reader.read_range(0, reader.header().expanded_size())?;
        let parsed = parse_screen_menu_stream(&data, Some(catalog));
        if parsed.screens.is_empty() {
            return Ok(deferred_component_surface_info(
                surface_id,
                "ssed_screen_menu_empty",
                format!(
                    "{} did not decode any screen-menu screens",
                    component.filename
                ),
                component,
            ));
        }
        let screens = self.ssed_screen_menu_screens(surface_id, &parsed)?;
        Ok(NavigationSurface::ScreenMenu {
            surface_id: surface_id.to_owned(),
            screens,
            stats: parsed.stats,
            diagnostics: Vec::new(),
        })
    }

    fn ssed_screen_menu_screens(
        &self,
        surface_id: &str,
        parsed: &SsedScreenMenuParse,
    ) -> Result<Vec<ScreenMenuScreen>> {
        parsed
            .screens
            .iter()
            .map(|screen| {
                let background = screen
                    .image
                    .as_ref()
                    .and_then(|pointer| pointer.target.as_ref().map(|target| (pointer, target)))
                    .filter(|(_, target)| target.role == SsedComponentRole::Colscr)
                    .map(|(pointer, target)| {
                        let resource =
                            ResourceToken::new(&InternalResource::SsedComponentAddress {
                                component: target.component.clone(),
                                block: pointer.block,
                                offset: pointer.offset,
                                resource_kind: ResourceKind::Colscr,
                            })?;
                        self.resolve_resource(&resource)
                    })
                    .transpose()?;
                let hotspots = screen
                    .hotspots
                    .iter()
                    .enumerate()
                    .map(|(index, hotspot)| {
                        let (target, target_kind) =
                            self.ssed_screen_menu_hotspot_target(surface_id, parsed, hotspot)?;
                        Ok(ScreenMenuHotspot {
                            hotspot_id: format!("hotspot-{index}"),
                            rect: ScreenMenuRect {
                                x: hotspot.rect.x,
                                y: hotspot.rect.y,
                                width: hotspot.rect.width,
                                height: hotspot.rect.height,
                            },
                            target,
                            target_kind,
                            diagnostics: Vec::new(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(ScreenMenuScreen {
                    screen_id: format!("screen-{}", screen.screen_index),
                    screen_index: screen.screen_index,
                    width: screen.width,
                    height: screen.height,
                    background,
                    hotspots,
                    diagnostics: Vec::new(),
                })
            })
            .collect()
    }

    fn ssed_screen_menu_hotspot_target(
        &self,
        surface_id: &str,
        parsed: &SsedScreenMenuParse,
        hotspot: &SsedScreenMenuHotspot,
    ) -> Result<(Option<TargetToken>, Option<String>)> {
        if let Some(target) = &hotspot.destination.target
            && target.role == SsedComponentRole::Honmon
        {
            return Ok((
                Some(TargetToken::new(&InternalTarget::SsedAddress {
                    component: target.component.clone(),
                    block: hotspot.destination.block,
                    offset: hotspot.destination.offset,
                })?),
                Some("body".to_owned()),
            ));
        }
        if let Some(screen_index) = hotspot.target_screen_index {
            return Ok((
                Some(TargetToken::new(&InternalTarget::MenuItem {
                    surface_id: surface_id.to_owned(),
                    item_id: format!("screen:{screen_index}"),
                })?),
                Some("screen".to_owned()),
            ));
        }
        if let (Some(screen_index), Some(direct_index)) = (
            hotspot.target_direct_screen_index,
            hotspot.target_direct_index,
        ) {
            let direct = parsed
                .screens
                .get(screen_index as usize)
                .and_then(|screen| screen.direct_targets.get(direct_index as usize));
            if let Some(direct) = direct
                && let Some(target) = &direct.destination.target
                && target.role == SsedComponentRole::Honmon
            {
                return Ok((
                    Some(TargetToken::new(&InternalTarget::SsedAddress {
                        component: target.component.clone(),
                        block: direct.destination.block,
                        offset: direct.destination.offset,
                    })?),
                    Some("body".to_owned()),
                ));
            }
        }
        Ok((None, None))
    }

    fn open_ssed_encyclopedia_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        let Some(path) = self.storage.resolve_casefolded(Path::new("encyclop.idx"))? else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_encyclopedia_index_missing",
                    "encyclop.idx is not present in this SSED package",
                )],
            });
        };
        let parsed = match parse_encyclopedia_index(&path) {
            Ok(parsed) => parsed,
            Err(error) => {
                return Ok(NavigationSurface::Deferred {
                    surface_id: surface_id.to_owned(),
                    diagnostics: vec![Diagnostic::warning(
                        "ssed_encyclopedia_index_parse_failed",
                        format!("failed to parse encyclop.idx: {error}"),
                    )],
                });
            }
        };
        let mut diagnostics = Vec::new();
        let nodes = ssed_encyclopedia_rows_to_nodes(self, &parsed.rows, &mut diagnostics)?;
        if nodes.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_encyclopedia_index_empty",
                    "encyclop.idx did not expose navigation rows",
                )],
            });
        }
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
        })
    }

    fn open_britannica_whatday_surface(
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
        let offset = decode_offset_cursor(cursor);
        let files = discover_britannica_whatday_paths(&self.root)?;
        if files.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_britannica_whatday_missing",
                    "Britannica loose whatday files were not found",
                )],
            });
        }
        let next_cursor = (files.len() > offset + limit).then(|| (offset + limit).to_string());
        let pages = files
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|file| {
                let resource = ResourceToken::new(&InternalResource::SsedLooseFile {
                    root_name: file.root_name.clone(),
                    path: file.relative_path.clone(),
                    resource_kind: ResourceKind::Html,
                })?;
                let label = format!(
                    "{}月{}日 {}",
                    file.month,
                    file.day,
                    file.fragment_kind.as_str()
                );
                Ok(NavigationItem {
                    item_id: format!(
                        "{}:{}",
                        file.root_name,
                        file.relative_path.replace('\\', "/")
                    ),
                    label_html: escape_plain_label_html(&label),
                    label_text: label,
                    target: TargetToken::new(&InternalTarget::Resource {
                        resource,
                        anchor: None,
                    })?,
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

    fn open_britannica_top_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        let dat_files = discover_britannica_top_dat_files(&self.root)?;
        if dat_files.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_britannica_top_missing",
                    "Britannica loose top_*.dat files were not found",
                )],
            });
        }
        let mut diagnostics = Vec::new();
        let mut nodes = Vec::new();
        for dat in dat_files {
            let mut children = Vec::new();
            for record in dat.records {
                let label = self.ssed_rich_label(&record.title);
                let label_html = if let Some(image) = &record.image_resource {
                    let resource = InternalResource::SsedLooseFile {
                        root_name: image.root_name.clone(),
                        path: image.relative_path.clone(),
                        resource_kind: ResourceKind::Image,
                    };
                    let token = ResourceToken::new(&resource)?;
                    format!(
                        r#"<img class="lv-britannica-top-thumb" src="lvcore://resource/{}" alt=""> {}"#,
                        token.as_str(),
                        label.html
                    )
                } else {
                    label.html
                };
                let target = self.ssed_target_for_loose_address(
                    record.address.block,
                    record.address.offset,
                    &mut diagnostics,
                )?;
                let mut node_diagnostics = label.diagnostics;
                if record.image_resource.is_none() && !record.image_name.is_empty() {
                    node_diagnostics.push(Diagnostic::info(
                        "ssed_britannica_top_image_missing",
                        format!(
                            "top_*.dat image {} was not found next to the media index",
                            record.image_name
                        ),
                    ));
                }
                children.push(NavigationNode {
                    node_id: format!("{}:{}", dat.relative_path, record.index),
                    label_html,
                    label_text: label.text,
                    target,
                    diagnostics: node_diagnostics,
                    children: Vec::new(),
                });
            }
            let category = dat.category.clone();
            nodes.push(NavigationNode {
                node_id: dat.relative_path,
                label_html: escape_plain_label_html(&category),
                label_text: category,
                target: None,
                diagnostics: Vec::new(),
                children,
            });
        }
        if nodes.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics,
            });
        }
        if !diagnostics.is_empty() {
            nodes.insert(
                0,
                NavigationNode {
                    node_id: "diagnostics".to_owned(),
                    label_html: "Diagnostics".to_owned(),
                    label_text: "Diagnostics".to_owned(),
                    target: None,
                    diagnostics,
                    children: Vec::new(),
                },
            );
        }
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
        })
    }

    fn open_ssed_aux_index_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        let spec = match self.ssed_aux_index_spec_for_surface(surface_id) {
            Ok(spec) => spec,
            Err(error) => {
                return Ok(NavigationSurface::Deferred {
                    surface_id: surface_id.to_owned(),
                    diagnostics: vec![Diagnostic::warning(
                        "ssed_auxiliary_index_invalid_surface",
                        error.to_string(),
                    )],
                });
            }
        };
        if !path_has_extension(&spec.info, &["idx"]) {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_auxiliary_index_unsupported_target",
                    format!(
                        "EXINFO auxiliary target {} is not a text IDX tree",
                        spec.info
                    ),
                )],
            });
        }
        let Some(path) = self.storage.resolve_casefolded(Path::new(&spec.info))? else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_auxiliary_index_file_missing",
                    format!("EXINFO auxiliary index {} was not found", spec.info),
                )],
            });
        };
        let rows = parse_aux_index_text_bytes(&fs::read(path)?)?;
        let mut diagnostics = Vec::new();
        let nodes = ssed_aux_index_rows_to_nodes(self, &rows, &mut diagnostics)?;
        if nodes.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_auxiliary_index_empty",
                    format!("EXINFO auxiliary index {} did not expose rows", spec.info),
                )],
            });
        }
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
        })
    }

    fn ssed_aux_index_spec_for_surface(&self, surface_id: &str) -> Result<SsedAuxIndexSpec> {
        if let Some(raw_index) = surface_id.strip_prefix("aux-index:") {
            let Ok(index) = raw_index.parse::<usize>() else {
                return Err(Error::Driver(
                    "auxiliary index surface id does not contain a numeric EXINFO index".to_owned(),
                ));
            };
            return self
                .ssed_aux_index_specs()?
                .into_iter()
                .find(|spec| spec.index == index)
                .ok_or_else(|| {
                    Error::Driver(
                        "EXINFO.INI did not declare the requested auxiliary index".to_owned(),
                    )
                });
        }
        if let Some(name) = surface_id.strip_prefix("numeric-aux:") {
            let excluded = self
                .ssed_aux_index_specs()?
                .into_iter()
                .map(|spec| spec.info.to_ascii_lowercase())
                .collect::<BTreeSet<_>>();
            return self
                .ssed_numeric_aux_index_specs(&excluded)?
                .into_iter()
                .find(|spec| spec.info.eq_ignore_ascii_case(name))
                .ok_or_else(|| {
                    Error::Driver(format!("numeric auxiliary index was not found: {name}"))
                });
        }
        Err(Error::Driver(
            "auxiliary index surface id is malformed".to_owned(),
        ))
    }

    fn open_ssed_panel_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        let Some(path) = self.storage.resolve_casefolded(Path::new("Panels.xml"))? else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_panels_missing",
                    "Panels.xml was not found",
                )],
            });
        };
        let parsed = match parse_panel_xml_bytes(&fs::read(path)?) {
            Ok(parsed) => parsed,
            Err(error) => {
                return Ok(NavigationSurface::Deferred {
                    surface_id: surface_id.to_owned(),
                    diagnostics: vec![Diagnostic::warning(
                        "ssed_panels_xml_parse_failed",
                        format!("Panels.xml could not be parsed: {error}"),
                    )],
                });
            }
        };
        let requested_panel_id = surface_id
            .strip_prefix("panels:")
            .filter(|id| !id.is_empty());
        let root_panel_id = requested_panel_id.or_else(|| {
            parsed
                .inline_cells
                .first()
                .map(|cell| cell.panel_id.as_str())
        });
        let inline_cells = parsed
            .inline_cells
            .iter()
            .filter(|cell| root_panel_id.is_none_or(|panel_id| cell.panel_id == panel_id))
            .cloned()
            .collect::<Vec<_>>();
        let include_external_bins = requested_panel_id.is_some() || inline_cells.is_empty();
        let mut diagnostics = Vec::new();
        let mut cells = Vec::new();
        for cell in inline_cells {
            cells.push(ssed_panel_inline_cell_to_navigation_cell(self, &cell)?);
        }
        for data_ref in parsed.data_refs.into_iter().filter(|data_ref| {
            include_external_bins
                && requested_panel_id.is_none_or(|panel_id| data_ref.panel_id == panel_id)
        }) {
            let Some(path) = self.resolve_ssed_panel_bin_path(&data_ref.filename)? else {
                diagnostics.push(Diagnostic::warning(
                    "ssed_panel_bin_missing",
                    format!("Panel BIN {} was not found", data_ref.filename),
                ));
                continue;
            };
            let panel = match parse_panel_bin(&fs::read(path)?) {
                Ok(panel) => panel,
                Err(error) => {
                    diagnostics.push(Diagnostic::warning(
                        "ssed_panel_bin_parse_failed",
                        format!(
                            "Panel BIN {} could not be parsed: {error}",
                            data_ref.filename
                        ),
                    ));
                    continue;
                }
            };
            for record in panel.records {
                cells.push(ssed_panel_bin_record_to_navigation_cell(
                    self,
                    &data_ref,
                    &record,
                    &mut diagnostics,
                )?);
            }
        }
        if cells.is_empty() {
            if diagnostics.is_empty() {
                diagnostics.push(Diagnostic::info(
                    "ssed_panels_empty",
                    "Panels.xml did not expose inline cells or decoded BIN rows",
                ));
            }
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics,
            });
        }
        Ok(NavigationSurface::Panel {
            surface_id: surface_id.to_owned(),
            cells,
        })
    }

    fn resolve_ssed_panel_bin_path(&self, filename: &str) -> Result<Option<PathBuf>> {
        let relative = filename.replace('\\', "/");
        if let Some(path) = self.storage.resolve_casefolded(Path::new(&relative))? {
            return Ok(Some(path));
        }
        let Some(stripped) = relative.strip_prefix("Panel/") else {
            return Ok(None);
        };
        let Some(package_name) = self.root.file_name().and_then(|name| name.to_str()) else {
            return Ok(None);
        };
        let sibling_panel_root = self.root.with_file_name(format!("{package_name}_Panel"));
        if !sibling_panel_root.is_dir() {
            return Ok(None);
        }
        DirectoryStorage::new(sibling_panel_root).resolve_casefolded(Path::new(stripped))
    }

    pub(super) fn resolve_package_file_path(&self, path: &str) -> Result<Option<PathBuf>> {
        let normalized = path.replace('\\', "/");
        let relative = Path::new(&normalized);
        if self.storage.exists(relative)?
            && let Some(path) = self.storage.resolve_casefolded(relative)?
        {
            return Ok(Some(path));
        }
        self.resolve_adjacent_templates_file_path(&normalized)
    }

    pub(super) fn read_package_file_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let normalized = path.replace('\\', "/");
        let relative = Path::new(&normalized);
        if self.storage.exists(relative)? {
            return self.storage.read(relative);
        }
        let Some((templates_root, stripped)) = self.adjacent_templates_root_and_path(relative)
        else {
            return Err(Error::Driver(format!("resource not found: {path}")));
        };
        DirectoryStorage::new(templates_root).read(stripped)
    }

    fn resolve_adjacent_templates_file_path(&self, path: &str) -> Result<Option<PathBuf>> {
        let relative = Path::new(path);
        let Some((templates_root, stripped)) = self.adjacent_templates_root_and_path(relative)
        else {
            return Ok(None);
        };
        let storage = DirectoryStorage::new(templates_root);
        if storage.exists(stripped)? {
            return storage.resolve_casefolded(stripped);
        }
        Ok(None)
    }

    fn adjacent_templates_root_and_path<'a>(
        &self,
        relative: &'a Path,
    ) -> Option<(PathBuf, &'a Path)> {
        let mut components = relative.components();
        let first = components.next()?;
        if !first
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("Templates")
        {
            return None;
        }
        let stripped = components.as_path();
        if stripped.as_os_str().is_empty() {
            return None;
        }
        let package_name = self.root.file_name().and_then(|name| name.to_str())?;
        let sibling_templates_root = self
            .root
            .with_file_name(format!("{package_name}_Templates"));
        if !sibling_templates_root.is_dir() {
            return None;
        }
        Some((sibling_templates_root, stripped))
    }

    fn open_ssed_hanrei_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        if cursor.is_none()
            && limit > 0
            && let Some(nodes) = self.discover_ssed_hanrei_chm_toc_nodes("HANREI.chm")?
        {
            return Ok(NavigationSurface::HierarchicalTree {
                surface_id: surface_id.to_owned(),
                nodes,
            });
        }
        if limit == 0 {
            return Ok(NavigationSurface::InfoPages {
                surface_id: surface_id.to_owned(),
                pages: Vec::new(),
                next_cursor: None,
            });
        }
        let offset = decode_offset_cursor(cursor);
        let mut pages = self.discover_ssed_hanrei_pages()?;
        if pages.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_hanrei_missing",
                    "SSED HANREI files were not found",
                )],
            });
        }
        let next_cursor = (pages.len() > offset + limit).then(|| (offset + limit).to_string());
        pages = pages.into_iter().skip(offset).take(limit).collect();
        let items = pages
            .into_iter()
            .map(|page| {
                let resource = ResourceToken::new(&page.resource)?;
                Ok(NavigationItem {
                    item_id: page.item_id,
                    label_html: escape_plain_label_html(&page.label),
                    label_text: page.label,
                    target: TargetToken::new(&InternalTarget::Resource {
                        resource,
                        anchor: page.anchor,
                    })?,
                    diagnostics: page.diagnostics,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::InfoPages {
            surface_id: surface_id.to_owned(),
            pages: items,
            next_cursor,
        })
    }

    fn discover_ssed_hanrei_chm_toc_nodes(
        &self,
        chm_path: &str,
    ) -> Result<Option<Vec<NavigationNode>>> {
        if !self.storage.exists(Path::new(chm_path))? {
            return Ok(None);
        }
        let Some(resolved) = self.storage.resolve_casefolded(Path::new(chm_path))? else {
            return Ok(None);
        };
        let Ok(entries) = list_chm_entries(&resolved) else {
            return Ok(None);
        };
        let mut toc_items = Vec::new();
        for entry in &entries {
            if !path_has_extension(&entry.path, &["hhc"]) {
                continue;
            }
            let Ok(bytes) = read_chm_entry(&resolved, &entry.path) else {
                continue;
            };
            let html = decode_package_html_text(&bytes);
            toc_items.extend(parse_chm_hhc_toc(&html));
        }
        if toc_items.is_empty() {
            return Ok(None);
        }
        let nodes = chm_hhc_toc_items_to_nodes(chm_path, &toc_items)?;
        Ok((!nodes.is_empty()).then_some(nodes))
    }

    pub(super) fn open_multiview_menu_surface(
        &self,
        surface_id: &str,
    ) -> Result<NavigationSurface> {
        let bytes = self.storage.read(Path::new("menuData.xml"))?;
        let xml = String::from_utf8(bytes)
            .map_err(|error| Error::Driver(format!("menuData.xml is not valid UTF-8: {error}")))?;
        let items = parse_menu_data(&xml)?;
        let nodes = items
            .iter()
            .enumerate()
            .map(|(index, item)| multiview_menu_item_to_node(item, &index.to_string()))
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
        })
    }

    pub(super) fn multiview_navigation_surface_for_href(
        &self,
        href: &str,
    ) -> Result<Option<(String, NavigationSurface)>> {
        let Some(store) = &self.multiview_store else {
            return Ok(None);
        };
        let Some(list) = store.law_list_for_href(href)? else {
            return Ok(None);
        };
        let title = list.title;
        let items = list
            .items
            .into_iter()
            .map(|item| {
                let target = TargetToken::new(&InternalTarget::MultiviewHref {
                    href: item.code.clone(),
                    anchor: None,
                })?;
                let label_text = if item.kana.is_empty() {
                    item.name
                } else {
                    format!("{} ({})", item.name, item.kana)
                };
                Ok(NavigationItem {
                    item_id: item.code,
                    label_html: escape_plain_label_html(&label_text),
                    label_text,
                    target,
                    diagnostics: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Some((
            title,
            NavigationSurface::TitleIndexBrowse {
                surface_id: format!("multiview:{href}"),
                items,
                next_cursor: None,
            },
        )))
    }

    fn open_hourei_law_tree_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        let Some(store) = &self.hourei_store else {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "hourei_store_missing",
                    "Hourei law tree requires an opened Hourei store",
                )],
            });
        };
        let categories = store.categories_with_laws()?;
        let nodes = categories
            .into_iter()
            .map(|category| {
                let children = category
                    .laws
                    .into_iter()
                    .map(|law| {
                        let label = hourei_law_node_label(&law);
                        Ok(NavigationNode {
                            node_id: format!("law:{}", law.hore_id),
                            label_html: escape_hourei_label_html(&label),
                            label_text: label,
                            target: Some(TargetToken::new(&InternalTarget::HoureiLaw {
                                hore_id: law.hore_id,
                                anchor: None,
                            })?),
                            diagnostics: Vec::new(),
                            children: Vec::new(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(NavigationNode {
                    node_id: format!("category:{}", category.id),
                    label_html: escape_hourei_label_html(&category.name),
                    label_text: category.name,
                    target: None,
                    diagnostics: Vec::new(),
                    children,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(NavigationSurface::HierarchicalTree {
            surface_id: surface_id.to_owned(),
            nodes,
        })
    }

    fn ssed_simple_index_rows_page(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<SsedIndexRow>, Vec<Diagnostic>)> {
        if limit == 0 {
            return Ok((Vec::new(), Vec::new()));
        }
        let mut rows = Vec::new();
        let mut seen = 0usize;
        let skip_backward_rows = self.ssed_has_forward_browse_index();
        let diagnostics = self.scan_ssed_simple_index_rows(None, |row| {
            if skip_backward_rows && ssed_index_component_name_is_backward(&row.component) {
                return Ok(true);
            }
            if seen >= offset {
                rows.push(row);
            }
            seen = seen.saturating_add(1);
            Ok(rows.len() < limit)
        })?;
        Ok((rows, diagnostics))
    }

    fn ssed_has_forward_browse_index(&self) -> bool {
        self.ssed_catalog.as_ref().is_some_and(|catalog| {
            catalog
                .components_by_role(SsedComponentRole::Index)
                .any(|component| {
                    is_supported_index_type(component.component_type)
                        && !ssed_index_component_name_is_backward(&component.filename)
                })
        })
    }
}

impl NavigationProvider for ReaderBookPackage {
    fn home_surfaces(&self) -> Result<Vec<HomeSurface>> {
        let mut surfaces = Vec::new();
        match self.metadata.format_family {
            FormatFamily::Ssed => {
                if let Some(surface) = self.ssed_navigation_home_surface(
                    "menu",
                    NavigationSurfaceKind::Menu,
                    "MENU",
                    SsedComponentRole::Menu,
                    "MENU.DIC",
                )? {
                    surfaces.push(surface);
                }
                if let Some(surface) = self.ssed_navigation_home_surface(
                    "toc",
                    NavigationSurfaceKind::Toc,
                    "TOC",
                    SsedComponentRole::Toc,
                    "TOC.DIC",
                )? {
                    surfaces.push(surface);
                }
                surfaces.extend(self.ssed_multi_home_surfaces()?);
                if self
                    .ssed_catalog
                    .as_ref()
                    .is_some_and(|catalog| catalog.has_role(SsedComponentRole::ScreenMenu))
                    || self.storage.exists(Path::new("SCRMENU.DIC"))?
                {
                    surfaces.push(HomeSurface {
                        surface_id: "screen-menu".to_owned(),
                        kind: NavigationSurfaceKind::ScreenMenu,
                        status: NavigationStatus::Available,
                        title_html: "Screen Menu".to_owned(),
                        title_text: "Screen Menu".to_owned(),
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id: "screen-menu".to_owned(),
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: vec![Diagnostic::info(
                            "ssed_screen_menu",
                            "SCRMENU.DIC exposes a bitmap-backed screen-map navigation surface",
                        )],
                    });
                }
                if self.storage.exists(Path::new("encyclop.idx"))? {
                    surfaces.push(HomeSurface {
                        surface_id: "encyclopedia".to_owned(),
                        kind: NavigationSurfaceKind::EncyclopediaIndex,
                        status: NavigationStatus::Available,
                        title_html: "Multimedia Index".to_owned(),
                        title_text: "Multimedia Index".to_owned(),
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id: "encyclopedia".to_owned(),
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: vec![Diagnostic::info(
                            "ssed_encyclopedia_index",
                            "encyclop.idx exposes an LVEDBRSR tab-indented multimedia navigation index",
                        )],
                    });
                }
                if has_britannica_whatday_files(&self.root)? {
                    surfaces.push(HomeSurface {
                        surface_id: "britannica-whatday".to_owned(),
                        kind: NavigationSurfaceKind::Info,
                        status: NavigationStatus::Available,
                        title_html: "Britannica What Happened Today".to_owned(),
                        title_text: "Britannica What Happened Today".to_owned(),
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id: "britannica-whatday".to_owned(),
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: vec![Diagnostic::info(
                            "ssed_britannica_whatday",
                            "Britannica loose whatday HTML fragments are available as info pages",
                        )],
                    });
                }
                if has_britannica_top_dat_files(&self.root)? {
                    surfaces.push(HomeSurface {
                        surface_id: "britannica-top".to_owned(),
                        kind: NavigationSurfaceKind::AuxiliaryIndex,
                        status: NavigationStatus::Available,
                        title_html: "Britannica Top Media Index".to_owned(),
                        title_text: "Britannica Top Media Index".to_owned(),
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id: "britannica-top".to_owned(),
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: vec![Diagnostic::info(
                            "ssed_britannica_top",
                            "Britannica loose top_*.dat media indexes are available",
                        )],
                    });
                }
                let aux_specs = self.ssed_aux_index_specs()?;
                let mut declared_aux_paths = BTreeSet::new();
                for spec in &aux_specs {
                    declared_aux_paths.insert(spec.info.to_ascii_lowercase());
                    let relative = Path::new(&spec.info);
                    if !path_has_extension(&spec.info, &["idx"]) {
                        continue;
                    }
                    if !self.storage.exists(relative)? {
                        continue;
                    }
                    let title = if spec.name.is_empty() {
                        spec.info.clone()
                    } else {
                        spec.name.clone()
                    };
                    let surface_id = format!("aux-index:{}", spec.index);
                    surfaces.push(HomeSurface {
                        surface_id: surface_id.clone(),
                        kind: NavigationSurfaceKind::AuxiliaryIndex,
                        status: NavigationStatus::Available,
                        title_html: escape_plain_label_html(&title),
                        title_text: title,
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id,
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: vec![Diagnostic::info(
                            "ssed_auxiliary_index",
                            "EXINFO.INI declares a tab-indented auxiliary navigation index",
                        )],
                    });
                }
                for spec in self.ssed_numeric_aux_index_specs(&declared_aux_paths)? {
                    let title = spec.info.clone();
                    let surface_id = format!("numeric-aux:{}", spec.info);
                    surfaces.push(HomeSurface {
                        surface_id: surface_id.clone(),
                        kind: NavigationSurfaceKind::AuxiliaryIndex,
                        status: NavigationStatus::Available,
                        title_html: escape_plain_label_html(&title),
                        title_text: title,
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id,
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: vec![Diagnostic::info(
                            "ssed_numeric_auxiliary_index",
                            "Numeric tab-indented auxiliary index is present without an EXINFO declaration",
                        )],
                    });
                }
                let hanrei_pages = self.discover_ssed_hanrei_pages()?;
                if !hanrei_pages.is_empty() {
                    let diagnostics = hanrei_pages
                        .iter()
                        .flat_map(|page| page.diagnostics.clone())
                        .collect::<Vec<_>>();
                    surfaces.push(HomeSurface {
                        surface_id: "hanrei".to_owned(),
                        kind: NavigationSurfaceKind::Hanrei,
                        status: NavigationStatus::Available,
                        title_html: "凡例".to_owned(),
                        title_text: "凡例".to_owned(),
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id: "hanrei".to_owned(),
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics,
                    });
                }
                push_surface_if_exists(
                    &mut surfaces,
                    &self.storage,
                    "panels",
                    NavigationSurfaceKind::Panel,
                    "Panels",
                    &["Panels.xml", "Panel"],
                )?;
                if self
                    .ssed_catalog
                    .as_ref()
                    .is_some_and(|catalog| has_decodable_ssed_index_rows(catalog, &self.storage))
                {
                    surfaces.push(HomeSurface {
                        surface_id: "title-index".to_owned(),
                        kind: NavigationSurfaceKind::TitleIndexBrowse,
                        status: NavigationStatus::Available,
                        title_html: "Title/Index Browse".to_owned(),
                        title_text: "Title/Index Browse".to_owned(),
                        target: Some(TargetToken::new(&InternalTarget::TitleIndexItem {
                            surface_id: "title-index".to_owned(),
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: vec![Diagnostic::info(
                            "surface_partial",
                            "SSED title/index browsing is available for supported leaf row grammars; exact/forward simple-index search can use internal tree pages while other paths may still scan linearly",
                        )],
                    });
                }
            }
            FormatFamily::LvedSqlite3 => {
                let list_available = self
                    .lved_summary
                    .as_ref()
                    .is_some_and(|summary| summary.list_available);
                let info_available = self
                    .lved_summary
                    .as_ref()
                    .is_some_and(|summary| summary.info_available);
                let tree_available = self
                    .lved_summary
                    .as_ref()
                    .is_some_and(|summary| summary.tree_available);
                surfaces.push(HomeSurface {
                    surface_id: "lved-list".to_owned(),
                    kind: NavigationSurfaceKind::TitleIndexBrowse,
                    status: if list_available {
                        NavigationStatus::Available
                    } else {
                        NavigationStatus::Missing
                    },
                    title_html: "LVED list".to_owned(),
                    title_text: "LVED list".to_owned(),
                    target: list_available
                        .then(|| {
                            TargetToken::new(&InternalTarget::TitleIndexItem {
                                surface_id: "lved-list".to_owned(),
                                item_id: "root".to_owned(),
                            })
                        })
                        .transpose()?,
                    diagnostics: Vec::new(),
                });
                surfaces.push(HomeSurface {
                    surface_id: "info".to_owned(),
                    kind: NavigationSurfaceKind::Info,
                    status: if info_available {
                        NavigationStatus::Available
                    } else {
                        NavigationStatus::Missing
                    },
                    title_html: "Info".to_owned(),
                    title_text: "Info".to_owned(),
                    target: None,
                    diagnostics: Vec::new(),
                });
                surfaces.push(HomeSurface {
                    surface_id: "lved-tree".to_owned(),
                    kind: NavigationSurfaceKind::LvedTree,
                    status: if tree_available {
                        NavigationStatus::Available
                    } else {
                        NavigationStatus::Missing
                    },
                    title_html: "LVED tree".to_owned(),
                    title_text: "LVED tree".to_owned(),
                    target: tree_available
                        .then(|| {
                            TargetToken::new(&InternalTarget::MenuItem {
                                surface_id: "lved-tree".to_owned(),
                                item_id: "root".to_owned(),
                            })
                        })
                        .transpose()?,
                    diagnostics: Vec::new(),
                });
            }
            FormatFamily::LvlMultiView => {
                surfaces.push(HomeSurface {
                    surface_id: "menuData".to_owned(),
                    kind: NavigationSurfaceKind::MultiviewTree,
                    status: NavigationStatus::Available,
                    title_html: "MultiView menu".to_owned(),
                    title_text: "MultiView menu".to_owned(),
                    target: Some(TargetToken::new(&InternalTarget::MultiviewHref {
                        href: "menuData.xml".to_owned(),
                        anchor: None,
                    })?),
                    diagnostics: Vec::new(),
                });
            }
            FormatFamily::Hourei => {
                surfaces.push(HomeSurface {
                    surface_id: "law-tree".to_owned(),
                    kind: NavigationSurfaceKind::LawTree,
                    status: if self.hourei_store.is_some() {
                        NavigationStatus::Available
                    } else {
                        NavigationStatus::Deferred
                    },
                    title_html: "法令".to_owned(),
                    title_text: "法令".to_owned(),
                    target: self
                        .hourei_store
                        .is_some()
                        .then(|| {
                            TargetToken::new(&InternalTarget::MenuItem {
                                surface_id: "law-tree".to_owned(),
                                item_id: "root".to_owned(),
                            })
                        })
                        .transpose()?,
                    diagnostics: if self.hourei_store.is_some() {
                        Vec::new()
                    } else {
                        vec![Diagnostic::info(
                            "surface_deferred",
                            "Hourei law tree requires an opened Hourei store",
                        )]
                    },
                });
            }
            FormatFamily::Unknown => {}
        }
        surfaces.push(HomeSurface {
            surface_id: "search".to_owned(),
            kind: NavigationSurfaceKind::SearchFallback,
            status: NavigationStatus::Available,
            title_html: "Search".to_owned(),
            title_text: "Search".to_owned(),
            target: None,
            diagnostics: Vec::new(),
        });
        surfaces.sort_by(|left, right| {
            home_surface_reader_priority(left)
                .cmp(&home_surface_reader_priority(right))
                .then_with(|| left.surface_id.cmp(&right.surface_id))
        });
        Ok(surfaces)
    }

    fn open_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        self.open_surface_page(surface_id, None, 100)
    }

    fn open_surface_page(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        match (self.metadata.format_family, surface_id) {
            (FormatFamily::Ssed, "title-index") => {
                self.open_ssed_title_index_surface(surface_id, cursor, limit)
            }
            (FormatFamily::Ssed, "menu") => {
                self.open_ssed_menu_surface(surface_id, SsedComponentRole::Menu, "MENU.DIC")
            }
            (FormatFamily::Ssed, "toc") => {
                self.open_ssed_menu_surface(surface_id, SsedComponentRole::Toc, "TOC.DIC")
            }
            (FormatFamily::Ssed, id) if id.starts_with("multi:") => {
                self.open_ssed_multi_selector_surface(surface_id, cursor, limit)
            }
            (FormatFamily::Ssed, "screen-menu") => self.open_ssed_screen_menu_surface(surface_id),
            (FormatFamily::Ssed, "encyclopedia") => self.open_ssed_encyclopedia_surface(surface_id),
            (FormatFamily::Ssed, "britannica-whatday") => {
                self.open_britannica_whatday_surface(surface_id, cursor, limit)
            }
            (FormatFamily::Ssed, "britannica-top") => self.open_britannica_top_surface(surface_id),
            (FormatFamily::Ssed, id)
                if id.starts_with("aux-index:") || id.starts_with("numeric-aux:") =>
            {
                self.open_ssed_aux_index_surface(surface_id)
            }
            (FormatFamily::Ssed, "hanrei") => {
                self.open_ssed_hanrei_surface(surface_id, cursor, limit)
            }
            (FormatFamily::Ssed, "panels") => self.open_ssed_panel_surface(surface_id),
            (FormatFamily::Ssed, id) if id.starts_with("panels:") => {
                self.open_ssed_panel_surface(surface_id)
            }
            (FormatFamily::LvedSqlite3, "lved-list") => {
                self.open_lved_list_surface(surface_id, cursor, limit)
            }
            (FormatFamily::LvedSqlite3, "info") => {
                self.open_lved_info_surface(surface_id, cursor, limit)
            }
            (FormatFamily::LvedSqlite3, "lved-tree") => self.open_lved_tree_surface(surface_id),
            (FormatFamily::LvlMultiView, "menuData") => {
                self.open_multiview_menu_surface(surface_id)
            }
            (FormatFamily::Hourei, "law-tree") => self.open_hourei_law_tree_surface(surface_id),
            (FormatFamily::Ssed, _) => Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "surface_open_deferred",
                    "surface parsing is not implemented yet",
                )],
            }),
            _ => Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "surface_open_deferred",
                    "surface parsing will be implemented by the matching provider",
                )],
            }),
        }
    }
}

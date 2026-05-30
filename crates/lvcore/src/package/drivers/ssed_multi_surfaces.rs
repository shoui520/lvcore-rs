use super::*;

impl ReaderBookPackage {
    pub(super) fn ssed_multi_home_surfaces(&self) -> Result<Vec<HomeSurface>> {
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
    pub(super) fn open_ssed_multi_selector_surface(
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
}

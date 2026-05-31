use super::*;

impl ReaderBookPackage {
    pub(super) fn ssed_navigation_home_surface(
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
}

impl ReaderBookPackage {
    pub(super) fn open_ssed_title_index_surface(
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

    pub(super) fn open_ssed_menu_surface(
        &self,
        surface_id: &str,
        _role: SsedComponentRole,
        fallback_name: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        if limit == 0 {
            return Ok(NavigationSurface::SimpleMenu {
                surface_id: surface_id.to_owned(),
                nodes: Vec::new(),
                next_cursor: None,
            });
        }
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
        let offset = decode_offset_cursor(cursor);
        let parsed = parse_menu_stream_page(&data, offset, limit);
        if parsed.records.is_empty() {
            if !parsed.empty_sentinel && offset > 0 {
                return Ok(NavigationSurface::SimpleMenu {
                    surface_id: surface_id.to_owned(),
                    nodes: Vec::new(),
                    next_cursor: None,
                });
            }
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
        let nodes =
            ssed_menu_records_to_nodes_from(self, &parsed.records, offset, &mut diagnostics)?;
        if nodes.is_empty() {
            return Ok(deferred_surface(surface_id, diagnostics));
        }
        Ok(NavigationSurface::SimpleMenu {
            surface_id: surface_id.to_owned(),
            nodes,
            next_cursor: parsed.next_cursor,
        })
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

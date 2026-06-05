use super::*;

const SSED_HOME_NAVIGATION_EMPTY_CHECK_MAX_BYTES: u64 = 64 * 1024;

impl ReaderBookPackage {
    pub(super) fn decoded_ssed_navigation_component_data(
        &self,
        component: &SsedComponent,
    ) -> Result<Arc<Vec<u8>>> {
        let cache_key = component.filename.to_ascii_lowercase();
        {
            let cache = self
                .ssed_navigation_component_data
                .lock()
                .map_err(|_| Error::Driver("SSED navigation cache was poisoned".to_owned()))?;
            if let Some(cached) = cache.get(&cache_key) {
                return cached
                    .as_ref()
                    .map(Arc::clone)
                    .map_err(|error| Error::Driver(error.clone()));
            }
        }

        let decoded = (|| {
            let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
                return Err(Error::Driver(format!(
                    "{} is declared but not present on disk",
                    component.filename
                )));
            };
            let mut reader = SsedDataFile::open(&path)?;
            reader.read_range(0, reader.header().expanded_size())
        })()
        .map(Arc::new)
        .map_err(|error| error.to_string());

        let mut cache = self
            .ssed_navigation_component_data
            .lock()
            .map_err(|_| Error::Driver("SSED navigation cache was poisoned".to_owned()))?;
        let cached = cache.entry(cache_key).or_insert_with(|| decoded);
        cached
            .as_ref()
            .map(Arc::clone)
            .map_err(|error| Error::Driver(error.clone()))
    }

    fn cached_ssed_navigation_surface_page(&self, key: &str) -> Result<Option<NavigationSurface>> {
        let cache = self
            .ssed_navigation_surface_pages
            .lock()
            .map_err(|_| Error::Driver("SSED navigation surface cache was poisoned".to_owned()))?;
        Ok(cache.get(key).map(|surface| surface.as_ref().clone()))
    }

    fn cache_ssed_navigation_surface_page(
        &self,
        key: String,
        surface: &NavigationSurface,
    ) -> Result<()> {
        self.ssed_navigation_surface_pages
            .lock()
            .map_err(|_| Error::Driver("SSED navigation surface cache was poisoned".to_owned()))?
            .insert(key, Arc::new(surface.clone()));
        Ok(())
    }

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
                    href: None,
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
            href: None,
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
        options: &LabelOptions,
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
        for (index, row) in rows.iter().enumerate() {
            let label = self
                .ssed_title_text(row.title)
                .unwrap_or_else(|| row.key.clone());
            let label = self.ssed_rich_label_with_policy(&label, &options.gaiji_policy);
            let target = match self.ssed_browse_target_for_index_row(
                row,
                rows.iter()
                    .skip(index + 1)
                    .find(|next| next.body != row.body),
            )? {
                Ok(target) => target,
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                    continue;
                }
            };
            items.push(NavigationItem {
                href: String::new(),
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
        options: &LabelOptions,
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
        let cache_key =
            ssed_navigation_surface_page_cache_key(component, surface_id, cursor, limit, options);
        if let Some(surface) = self.cached_ssed_navigation_surface_page(&cache_key)? {
            return Ok(surface);
        }
        let address_offset =
            match ssed_navigation_address_cursor_offset(cursor, component, surface_id) {
                Ok(offset) => offset,
                Err(diagnostic) => {
                    return Ok(NavigationSurface::Deferred {
                        surface_id: surface_id.to_owned(),
                        diagnostics: vec![diagnostic],
                    });
                }
            };
        let node_cursor = if address_offset.is_some() {
            SsedMenuNodeCursor {
                record_offset: 0,
                link_offset: 0,
            }
        } else {
            decode_ssed_menu_node_cursor(cursor)
        };
        let data = match self.decoded_ssed_navigation_component_data(component) {
            Ok(data) => data,
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
        let parse_data = if let Some(offset) = address_offset {
            if offset >= data.len() {
                return Ok(NavigationSurface::Deferred {
                    surface_id: surface_id.to_owned(),
                    diagnostics: vec![
                        Diagnostic::warning(
                            "ssed_navigation_address_cursor_out_of_data",
                            format!(
                                "{surface_id} address cursor resolves past decoded {} data",
                                component.filename
                            ),
                        )
                        .with_context("component", &component.filename),
                    ],
                });
            }
            &data[offset..]
        } else {
            data.as_slice()
        };
        let parsed = parse_menu_stream_page(parse_data, node_cursor.record_offset, limit);
        if parsed.records.is_empty() {
            if !parsed.empty_sentinel && node_cursor.record_offset > 0 {
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
        let node_page = ssed_menu_records_to_nodes_page_from(
            SsedMenuNodePageRequest {
                package: self,
                records: &parsed.records,
                base_index: if address_offset.is_some() {
                    0
                } else {
                    node_cursor.record_offset
                },
                initial_link_offset: node_cursor.link_offset,
                limit,
                parsed_next_cursor: parsed.next_cursor,
                gaiji_policy: &options.gaiji_policy,
            },
            &mut diagnostics,
        )?;
        let nodes = node_page.nodes;
        if nodes.is_empty() {
            return Ok(deferred_surface(surface_id, diagnostics));
        }
        let surface = NavigationSurface::SimpleMenu {
            surface_id: surface_id.to_owned(),
            nodes,
            next_cursor: node_page.next_cursor,
        };
        self.cache_ssed_navigation_surface_page(cache_key, &surface)?;
        Ok(surface)
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
        if u64::from(component.block_count()) * u64::from(BLOCK_SIZE)
            > SSED_HOME_NAVIGATION_EMPTY_CHECK_MAX_BYTES
        {
            return Ok(None);
        }
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
        let diagnostics = self.scan_ssed_simple_index_rows_with_filters(
            None,
            |component| {
                !(skip_backward_rows && ssed_index_component_name_is_backward(&component.filename))
            },
            |_, _| true,
            |row| {
                if seen >= offset {
                    rows.push(row);
                }
                seen = seen.saturating_add(1);
                Ok(rows.len() < limit)
            },
        )?;
        Ok((rows, diagnostics))
    }

    pub(super) fn ssed_has_forward_browse_index(&self) -> bool {
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

fn ssed_navigation_address_cursor_offset(
    cursor: Option<&str>,
    component: &SsedComponent,
    surface_id: &str,
) -> std::result::Result<Option<usize>, Diagnostic> {
    let Some(cursor) = cursor.map(str::trim).filter(|cursor| !cursor.is_empty()) else {
        return Ok(None);
    };
    let Some(rest) = cursor.strip_prefix("addr:") else {
        return Ok(None);
    };
    let mut parts = rest.split(':');
    let parsed = match (parts.next(), parts.next(), parts.next()) {
        (Some(block), Some(offset), None) => {
            let block = block.parse::<u32>().ok();
            let offset = offset.parse::<u32>().ok();
            block.zip(offset)
        }
        _ => None,
    };
    let Some((block, offset)) = parsed else {
        return Err(Diagnostic::warning(
            "ssed_navigation_address_cursor_invalid",
            format!("{surface_id} address cursor is malformed: {cursor}"),
        ));
    };
    let Some(relative_offset) = component.relative_offset(block, offset) else {
        return Err(Diagnostic::warning(
            "ssed_navigation_address_cursor_out_of_range",
            format!(
                "{surface_id} address cursor {block}:{offset} is outside {}",
                component.filename
            ),
        )
        .with_context("component", &component.filename));
    };
    usize::try_from(relative_offset).map(Some).map_err(|_| {
        Diagnostic::warning(
            "ssed_navigation_address_cursor_too_large",
            format!("{surface_id} address cursor {block}:{offset} does not fit this platform"),
        )
        .with_context("component", &component.filename)
    })
}

fn ssed_navigation_surface_page_cache_key(
    component: &SsedComponent,
    surface_id: &str,
    cursor: Option<&str>,
    limit: usize,
    options: &LabelOptions,
) -> String {
    let policy = options
        .gaiji_policy
        .priority
        .iter()
        .map(|source| match source {
            GaijiSourcePreference::Unicode => "unicode",
            GaijiSourcePreference::ExternalResource => "external",
            GaijiSourcePreference::Ga16Bitmap => "ga16",
            GaijiSourcePreference::Unresolved => "unresolved",
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{}\0{}\0{}\0{}\0{}",
        component.filename.to_ascii_lowercase(),
        surface_id,
        cursor.unwrap_or_default(),
        limit,
        policy
    )
}

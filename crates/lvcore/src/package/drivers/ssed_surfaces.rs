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

    pub(super) fn cached_ssed_navigation_surface_page(
        &self,
        key: &str,
    ) -> Result<Option<NavigationSurface>> {
        let cache = self
            .ssed_navigation_surface_pages
            .lock()
            .map_err(|_| Error::Driver("SSED navigation surface cache was poisoned".to_owned()))?;
        Ok(cache.get(key).map(|surface| surface.as_ref().clone()))
    }

    pub(super) fn cache_ssed_navigation_surface_page(
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
        _role: SsedComponentRole,
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
        self.ssed_navigation_home_surface_for_component(surface_id, kind, title, component)
    }

    pub(super) fn ssed_navigation_home_surface_for_component(
        &self,
        surface_id: &str,
        kind: NavigationSurfaceKind,
        title: &str,
        component: &SsedComponent,
    ) -> Result<Option<HomeSurface>> {
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

        let empty_diagnostic = self.ssed_navigation_component_empty_diagnostic(component)?;
        let is_empty = empty_diagnostic.is_some();
        let target = if is_empty {
            None
        } else {
            Some(ssed_direct_navigation_target_for_component(
                component,
                surface_id.to_owned(),
                "root".to_owned(),
            )?)
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
        let mut scan_offset = decode_offset_cursor(cursor);
        let mut diagnostics = Vec::new();
        let mut items = Vec::new();
        let mut next_cursor = None;
        let mut saw_raw_rows = false;
        while items.len() < limit {
            let remaining = limit.saturating_sub(items.len());
            let batch_limit = remaining.saturating_add(1).max(128);
            let (rows, mut row_diagnostics) =
                self.ssed_simple_index_rows_page(scan_offset, batch_limit)?;
            diagnostics.append(&mut row_diagnostics);
            if rows.is_empty() {
                break;
            }
            saw_raw_rows = true;
            let row_count = rows.len();
            for (index, row) in rows.iter().enumerate() {
                let next_distinct_row = rows
                    .iter()
                    .skip(index + 1)
                    .find(|next| next.body != row.body);
                let target = match self.ssed_browse_target_for_index_row(row, next_distinct_row)? {
                    Ok(target) => target,
                    Err(diagnostic) => {
                        diagnostics.push(diagnostic);
                        continue;
                    }
                };
                let label_text = self.ssed_browse_display_text_for_index_row(row, &target)?;
                if label_text.trim().is_empty() {
                    continue;
                }
                let label = self.ssed_rich_label_with_policy(&label_text, &options.gaiji_policy);
                items.push(NavigationItem {
                    href: String::new(),
                    item_id: format!("{}:{}", row.component, scan_offset + index),
                    label_html: label.html,
                    label_text: label.text,
                    target,
                    diagnostics: label.diagnostics,
                });
                if items.len() == limit {
                    if row_count == batch_limit || index + 1 < row_count {
                        next_cursor = Some((scan_offset + index + 1).to_string());
                    }
                    break;
                }
            }
            if items.len() == limit || row_count < batch_limit {
                break;
            }
            scan_offset = scan_offset.saturating_add(row_count);
        }
        if !saw_raw_rows && !diagnostics.is_empty() {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics,
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
        self.open_ssed_navigation_component_surface(surface_id, component, cursor, limit, options)
    }

    pub(super) fn open_ssed_navigation_component_surface(
        &self,
        surface_id: &str,
        component: &SsedComponent,
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
        let cache_key =
            ssed_navigation_surface_page_cache_key(component, surface_id, cursor, limit, options);
        if let Some(surface) = self.cached_ssed_navigation_surface_page(&cache_key)? {
            return Ok(surface);
        }
        let address_cursor = match ssed_navigation_address_cursor(cursor, component, surface_id) {
            Ok(offset) => offset,
            Err(diagnostic) => {
                return Ok(NavigationSurface::Deferred {
                    surface_id: surface_id.to_owned(),
                    diagnostics: vec![diagnostic],
                });
            }
        };
        let node_cursor = if let Some(address_cursor) = &address_cursor {
            decode_ssed_menu_node_cursor(address_cursor.page_cursor.as_deref())
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
        let parse_data = if let Some(address_cursor) = &address_cursor {
            let offset = address_cursor.relative_offset;
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
                base_index: if address_cursor.is_some() {
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
            next_cursor: ssed_navigation_scoped_next_cursor(
                address_cursor.as_ref(),
                node_page.next_cursor,
            ),
        };
        self.cache_ssed_navigation_surface_page(cache_key, &surface)?;
        Ok(surface)
    }

    pub(super) fn ssed_navigation_component_empty_diagnostic(
        &self,
        component: &SsedComponent,
    ) -> Result<Option<Diagnostic>> {
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
        let primary_browse_indexes = self.ssed_primary_browse_index_names();
        let diagnostics = self.scan_ssed_ordered_index_rows_with_filters(
            None,
            |component| {
                if skip_backward_rows && ssed_index_component_name_is_backward(&component.filename)
                {
                    return false;
                }
                primary_browse_indexes.is_empty()
                    || primary_browse_indexes.contains(&component.filename.to_ascii_uppercase())
            },
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

    pub(super) fn ssed_primary_browse_index_names(&self) -> BTreeSet<String> {
        let Some(catalog) = &self.ssed_catalog else {
            return BTreeSet::new();
        };
        let mut best_rank = None::<u8>;
        let mut names = BTreeSet::new();
        for component in catalog.components_by_role(SsedComponentRole::Index) {
            if !is_supported_index_type(component.component_type)
                || ssed_index_component_name_is_backward(&component.filename)
            {
                continue;
            }
            let rank = ssed_browse_index_priority(component);
            match best_rank {
                None => {
                    best_rank = Some(rank);
                    names.insert(component.filename.to_ascii_uppercase());
                }
                Some(best) if rank < best => {
                    best_rank = Some(rank);
                    names.clear();
                    names.insert(component.filename.to_ascii_uppercase());
                }
                Some(best) if rank == best => {
                    names.insert(component.filename.to_ascii_uppercase());
                }
                _ => {}
            }
        }
        names
    }
}

fn ssed_browse_index_priority(component: &SsedComponent) -> u8 {
    let upper = component.filename.to_ascii_uppercase();
    if upper == "FHINDEX.DIC" {
        0
    } else if component.component_type == 0x91 {
        1
    } else if upper == "FKINDEX.DIC" {
        2
    } else if component.component_type == 0x90 {
        3
    } else {
        4
    }
}

struct SsedNavigationAddressCursor {
    block: u32,
    offset: u32,
    relative_offset: usize,
    page_cursor: Option<String>,
}

fn ssed_navigation_address_cursor(
    cursor: Option<&str>,
    component: &SsedComponent,
    surface_id: &str,
) -> std::result::Result<Option<SsedNavigationAddressCursor>, Diagnostic> {
    let Some(cursor) = cursor.map(str::trim).filter(|cursor| !cursor.is_empty()) else {
        return Ok(None);
    };
    let Some(rest) = cursor.strip_prefix("addr:") else {
        return Ok(None);
    };
    let mut parts = rest.splitn(3, ':');
    let parsed = match (parts.next(), parts.next(), parts.next()) {
        (Some(block), Some(offset), page_cursor) => {
            let block = block.parse::<u32>().ok();
            let offset = offset.parse::<u32>().ok();
            block.zip(offset).map(|(block, offset)| {
                let page_cursor = page_cursor
                    .map(str::trim)
                    .filter(|cursor| !cursor.is_empty())
                    .map(str::to_owned);
                (block, offset, page_cursor)
            })
        }
        _ => None,
    };
    let Some((block, offset, page_cursor)) = parsed else {
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
    usize::try_from(relative_offset)
        .map(Some)
        .map_err(|_| {
            Diagnostic::warning(
                "ssed_navigation_address_cursor_too_large",
                format!("{surface_id} address cursor {block}:{offset} does not fit this platform"),
            )
            .with_context("component", &component.filename)
        })
        .map(|relative_offset| {
            relative_offset.map(|relative_offset| SsedNavigationAddressCursor {
                block,
                offset,
                relative_offset,
                page_cursor,
            })
        })
}

fn ssed_navigation_scoped_next_cursor(
    address_cursor: Option<&SsedNavigationAddressCursor>,
    next_cursor: Option<String>,
) -> Option<String> {
    let next_cursor = next_cursor?;
    match address_cursor {
        Some(address_cursor) => Some(format!(
            "addr:{}:{}:{}",
            address_cursor.block, address_cursor.offset, next_cursor
        )),
        None => Some(next_cursor),
    }
}

pub(super) fn ssed_navigation_surface_page_cache_key(
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

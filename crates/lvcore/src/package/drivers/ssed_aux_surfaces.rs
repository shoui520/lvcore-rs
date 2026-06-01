use super::*;

impl ReaderBookPackage {
    pub(super) fn open_ssed_encyclopedia_surface(
        &self,
        surface_id: &str,
        options: &LabelOptions,
    ) -> Result<NavigationSurface> {
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
        let nodes = ssed_encyclopedia_rows_to_nodes(
            self,
            &parsed.rows,
            &mut diagnostics,
            &options.gaiji_policy,
        )?;
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
            next_cursor: None,
        })
    }

    pub(super) fn open_britannica_whatday_surface(
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

    pub(super) fn open_britannica_top_surface(
        &self,
        surface_id: &str,
        options: &LabelOptions,
    ) -> Result<NavigationSurface> {
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
                let label = self.ssed_rich_label_with_policy(&record.title, &options.gaiji_policy);
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
            next_cursor: None,
        })
    }

    pub(super) fn open_ssed_aux_index_surface(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
        options: &LabelOptions,
    ) -> Result<NavigationSurface> {
        if limit == 0 {
            return Ok(NavigationSurface::HierarchicalTree {
                surface_id: surface_id.to_owned(),
                nodes: Vec::new(),
                next_cursor: None,
            });
        }
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
        if !self.storage.exists(Path::new(&spec.info))? {
            return Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "ssed_auxiliary_index_file_missing",
                    format!("EXINFO auxiliary index {} was not found", spec.info),
                )],
            });
        }
        let rows = parse_aux_index_text_bytes(&self.storage.read(Path::new(&spec.info))?)?;
        let offset = decode_offset_cursor(cursor);
        let next_cursor = (rows.len() > offset.saturating_add(limit))
            .then(|| offset.saturating_add(limit).to_string());
        let page_rows = rows
            .get(offset..offset.saturating_add(limit).min(rows.len()))
            .unwrap_or_default();
        if page_rows.is_empty() && offset > 0 {
            return Ok(NavigationSurface::HierarchicalTree {
                surface_id: surface_id.to_owned(),
                nodes: Vec::new(),
                next_cursor: None,
            });
        }
        let mut diagnostics = Vec::new();
        let nodes = if offset == 0 {
            ssed_aux_index_rows_to_nodes(self, page_rows, &mut diagnostics, &options.gaiji_policy)?
        } else {
            ssed_aux_index_rows_to_flat_nodes(
                self,
                page_rows,
                &mut diagnostics,
                &options.gaiji_policy,
            )?
        };
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
            next_cursor,
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
}

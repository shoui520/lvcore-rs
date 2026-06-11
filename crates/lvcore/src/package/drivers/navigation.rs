use super::*;

const LVED_NAMED_BODY_SURFACES: &[(&str, &str)] = &[("binran", "Binran")];

impl ReaderBookPackage {
    fn has_multiview_provider(&self) -> bool {
        self.multiview_store.is_some() || self.metadata.format_family == FormatFamily::LvlMultiView
    }

    fn has_hourei_provider(&self) -> bool {
        self.hourei_store.is_some() || self.metadata.format_family == FormatFamily::Hourei
    }

    fn push_lved_sqlite_home_surfaces(&self, surfaces: &mut Vec<HomeSurface>) -> Result<()> {
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
            href: None,
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
            href: None,
            surface_id: "info".to_owned(),
            kind: NavigationSurfaceKind::Info,
            status: if info_available {
                NavigationStatus::Available
            } else {
                NavigationStatus::Missing
            },
            title_html: "Info".to_owned(),
            title_text: "Info".to_owned(),
            target: info_available
                .then(|| {
                    TargetToken::new(&InternalTarget::MenuItem {
                        surface_id: "info".to_owned(),
                        item_id: "root".to_owned(),
                    })
                })
                .transpose()?,
            diagnostics: Vec::new(),
        });
        if tree_available {
            surfaces.push(HomeSurface {
                href: None,
                surface_id: "lved-tree".to_owned(),
                kind: NavigationSurfaceKind::LvedTree,
                status: NavigationStatus::Available,
                title_html: "LVED tree".to_owned(),
                title_text: "LVED tree".to_owned(),
                target: Some(TargetToken::new(&InternalTarget::MenuItem {
                    surface_id: "lved-tree".to_owned(),
                    item_id: "root".to_owned(),
                })?),
                diagnostics: Vec::new(),
            });
        }
        if let Some(store) = &self.lved_store {
            for (surface_id, title) in LVED_NAMED_BODY_SURFACES {
                if !store.named_pages_available(surface_id)? {
                    continue;
                }
                surfaces.push(HomeSurface {
                    href: None,
                    surface_id: (*surface_id).to_owned(),
                    kind: NavigationSurfaceKind::Info,
                    status: NavigationStatus::Available,
                    title_html: (*title).to_owned(),
                    title_text: (*title).to_owned(),
                    target: Some(TargetToken::new(&InternalTarget::MenuItem {
                        surface_id: (*surface_id).to_owned(),
                        item_id: "root".to_owned(),
                    })?),
                    diagnostics: Vec::new(),
                });
            }
        }
        Ok(())
    }
}

impl NavigationProvider for ReaderBookPackage {
    fn home_surfaces(&self) -> Result<Vec<HomeSurface>> {
        let mut surfaces = Vec::new();
        let lved_is_primary_family =
            self.metadata.format_family == FormatFamily::LvedSqlite3 && self.lved_store.is_some();
        if lved_is_primary_family {
            self.push_lved_sqlite_home_surfaces(&mut surfaces)?;
        }
        if self.ssed_catalog.is_some() || self.metadata.format_family == FormatFamily::Ssed {
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
            if let Some(catalog) = &self.ssed_catalog {
                for component in ssed_direct_navigation_components(catalog) {
                    let surface_id = ssed_direct_navigation_surface_id_for_component(component);
                    if surface_id == "menu" || surface_id == "toc" {
                        continue;
                    }
                    let kind = ssed_direct_navigation_kind_for_component(component);
                    let title = component.filename.clone();
                    if let Some(surface) = self.ssed_navigation_home_surface_for_component(
                        &surface_id,
                        kind,
                        &title,
                        component,
                    )? {
                        surfaces.push(surface);
                    }
                }
            }
            surfaces.extend(self.ssed_multi_home_surfaces()?);
            if self
                .ssed_catalog
                .as_ref()
                .is_some_and(|catalog| catalog.has_role(SsedComponentRole::ScreenMenu))
                || self.storage.exists(Path::new("SCRMENU.DIC"))?
            {
                surfaces.push(HomeSurface {
                    href: None,
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
                    href: None,
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
                    href: None,
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
                    href: None,
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
                    href: None,
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
            href: None,
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
            if self.has_ssed_hanrei_surface()? {
                surfaces.push(HomeSurface {
                    href: None,
                    surface_id: "hanrei".to_owned(),
                    kind: NavigationSurfaceKind::Hanrei,
                    status: NavigationStatus::Available,
                    title_html: "凡例".to_owned(),
                    title_text: "凡例".to_owned(),
                    target: Some(TargetToken::new(&InternalTarget::MenuItem {
                        surface_id: "hanrei".to_owned(),
                        item_id: "root".to_owned(),
                    })?),
                    diagnostics: Vec::new(),
                });
            }
            if !self.retained_ios_fts_payloads.is_empty() {
                surfaces.push(HomeSurface {
                    href: None,
                    surface_id: "ios-retained-fts".to_owned(),
                    kind: NavigationSurfaceKind::Info,
                    status: NavigationStatus::Deferred,
                    title_html: "iOS retained FTS database".to_owned(),
                    title_text: "iOS retained FTS database".to_owned(),
                    target: Some(TargetToken::new(&InternalTarget::MenuItem {
                        surface_id: "ios-retained-fts".to_owned(),
                        item_id: "root".to_owned(),
                    })?),
                    diagnostics: self.retained_ios_fts_deferred_diagnostics(),
                });
            }
            if self.has_ssed_panel_metadata()? {
                let title = self
                    .ssed_panel_home_title()?
                    .unwrap_or_else(|| "Panels".to_owned());
                surfaces.push(HomeSurface {
                    href: None,
                    surface_id: "panels".to_owned(),
                    kind: NavigationSurfaceKind::Panel,
                    status: NavigationStatus::Available,
                    title_html: escape_plain_label_html(&title),
                    title_text: title,
                    target: Some(TargetToken::new(&InternalTarget::MenuItem {
                        surface_id: "panels".to_owned(),
                        item_id: "root".to_owned(),
                    })?),
                    diagnostics: Vec::new(),
                });
            }
            for source in self.ssed_ios_panel_plist_sources()? {
                surfaces.push(HomeSurface {
                    href: None,
                    surface_id: source.surface_id.clone(),
                    kind: NavigationSurfaceKind::Panel,
                    status: NavigationStatus::Available,
                    title_html: escape_plain_label_html(&source.title),
                    title_text: source.title,
                    target: Some(TargetToken::new(&InternalTarget::MenuItem {
                        surface_id: source.surface_id,
                        item_id: "root".to_owned(),
                    })?),
                    diagnostics: vec![Diagnostic::info(
                        "ssed_ios_plist_panel",
                        "iOS plist navigation is available as a panel-style surface",
                    )],
                });
            }
            for source in self.ssed_ios_html_list_sources()? {
                surfaces.push(HomeSurface {
                    href: None,
                    surface_id: source.surface_id.clone(),
                    kind: NavigationSurfaceKind::Info,
                    status: NavigationStatus::Available,
                    title_html: escape_plain_label_html(&source.title),
                    title_text: source.title,
                    target: Some(TargetToken::new(&InternalTarget::MenuItem {
                        surface_id: source.surface_id,
                        item_id: "root".to_owned(),
                    })?),
                    diagnostics: vec![Diagnostic::info(
                        "ssed_ios_html_list",
                        "iOS HTMLList.plist exposes preserved info pages",
                    )],
                });
            }
            if !self.ssed_ios_dictlist_other_items()?.is_empty() {
                let surface_id =
                    super::ssed_ios_plist_surfaces::IOS_DICTLIST_OTHER_SURFACE_ID.to_owned();
                surfaces.push(HomeSurface {
                    href: None,
                    surface_id: surface_id.clone(),
                    kind: NavigationSurfaceKind::Info,
                    status: NavigationStatus::Available,
                    title_html: "Other info pages".to_owned(),
                    title_text: "Other info pages".to_owned(),
                    target: Some(TargetToken::new(&InternalTarget::MenuItem {
                        surface_id,
                        item_id: "root".to_owned(),
                    })?),
                    diagnostics: vec![Diagnostic::info(
                        "ssed_ios_dictlist_other",
                        "iOS DictList.plist Other entries expose preserved info pages",
                    )],
                });
            }
            for source in self.ssed_ios_app_menu_xml_sources()? {
                surfaces.push(HomeSurface {
                    href: None,
                    surface_id: source.surface_id.clone(),
                    kind: NavigationSurfaceKind::Info,
                    status: NavigationStatus::Available,
                    title_html: escape_plain_label_html(&source.title),
                    title_text: source.title,
                    target: Some(TargetToken::new(&InternalTarget::MenuItem {
                        surface_id: source.surface_id,
                        item_id: "root".to_owned(),
                    })?),
                    diagnostics: vec![Diagnostic::info(
                        "ssed_ios_app_menu",
                        "iOS app-menu XML exposes package HTML info pages",
                    )],
                });
            }
            if let Some(source) = self.ssed_exinfo_index_url_source()? {
                surfaces.push(HomeSurface {
                    href: None,
                    surface_id: super::ssed_package_html_surfaces::SSED_EXINFO_INDEX_URL_SURFACE_ID
                        .to_owned(),
                    kind: NavigationSurfaceKind::Info,
                    status: NavigationStatus::Available,
                    title_html: escape_plain_label_html(&source.title),
                    title_text: source.title,
                    target: Some(TargetToken::new(&InternalTarget::MenuItem {
                        surface_id:
                            super::ssed_package_html_surfaces::SSED_EXINFO_INDEX_URL_SURFACE_ID
                                .to_owned(),
                        item_id: "root".to_owned(),
                    })?),
                    diagnostics: vec![Diagnostic::info(
                        "ssed_exinfo_index_url",
                        "EXINFO.INI INDEXURL exposes a package HTML start page",
                    )],
                });
            }
            for source in self.ssed_ios_table_list_sources()? {
                let (status, diagnostics) = self.ssed_ios_table_list_source_status(&source)?;
                let target = (status == NavigationStatus::Available)
                    .then(|| {
                        TargetToken::new(&InternalTarget::TitleIndexItem {
                            surface_id: source.surface_id.clone(),
                            item_id: "root".to_owned(),
                        })
                    })
                    .transpose()?;
                surfaces.push(HomeSurface {
                    href: None,
                    surface_id: source.surface_id.clone(),
                    kind: NavigationSurfaceKind::TitleIndexBrowse,
                    status,
                    title_html: escape_plain_label_html(&source.title),
                    title_text: source.title,
                    target,
                    diagnostics,
                });
            }
            for source in self.ssed_ios_full_db_list_sources()? {
                surfaces.push(HomeSurface {
                    href: None,
                    surface_id: source.surface_id.clone(),
                    kind: NavigationSurfaceKind::TitleIndexBrowse,
                    status: NavigationStatus::Available,
                    title_html: escape_plain_label_html(&source.title),
                    title_text: source.title,
                    target: Some(TargetToken::new(&InternalTarget::TitleIndexItem {
                        surface_id: source.surface_id,
                        item_id: "root".to_owned(),
                    })?),
                    diagnostics: vec![Diagnostic::info(
                        "ssed_ios_fulldb_list",
                        "iOS DictFULLDB exposes ordered title/body rows as a browse surface",
                    )],
                });
            }
            if self
                .ssed_catalog
                .as_ref()
                .is_some_and(|catalog| has_decodable_ssed_index_rows(catalog, &self.storage))
            {
                surfaces.push(HomeSurface {
                        href: None,
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
        if self.lved_store.is_some() && !lved_is_primary_family {
            self.push_lved_sqlite_home_surfaces(&mut surfaces)?;
        }
        if self.has_multiview_provider() {
            for (index, path) in self.multiview_menu_surface_files()?.into_iter().enumerate() {
                let surface_id = super::multiview_navigation::multiview_menu_surface_id(index);
                let title = if index == 0 {
                    "MultiView menu".to_owned()
                } else {
                    format!("MultiView menu: {path}")
                };
                surfaces.push(HomeSurface {
                    href: None,
                    surface_id: surface_id.clone(),
                    kind: NavigationSurfaceKind::MultiviewTree,
                    status: NavigationStatus::Available,
                    title_html: escape_plain_label_html(&title),
                    title_text: title,
                    target: Some(TargetToken::new(&InternalTarget::MultiviewHref {
                        href: path,
                        anchor: None,
                    })?),
                    diagnostics: Vec::new(),
                });
            }
        }
        if self.has_hourei_provider() {
            if self.has_hourei_kana_panel()? {
                let surface_id =
                    super::hourei_navigation::hourei_kana_panel_surface_id().to_owned();
                surfaces.push(HomeSurface {
                    href: None,
                    surface_id: surface_id.clone(),
                    kind: NavigationSurfaceKind::Panel,
                    status: NavigationStatus::Available,
                    title_html: "五十音".to_owned(),
                    title_text: "五十音".to_owned(),
                    target: Some(TargetToken::new(&InternalTarget::MenuItem {
                        surface_id,
                        item_id: "root".to_owned(),
                    })?),
                    diagnostics: vec![Diagnostic::info(
                        "hourei_kana_panel",
                        "Hourei kana panel is available as a first-class browse surface",
                    )],
                });
            }
            surfaces.push(HomeSurface {
                href: None,
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
        surfaces.push(HomeSurface {
            href: None,
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
        populate_home_surface_hrefs(&mut surfaces);
        Ok(surfaces)
    }

    fn open_surface(&self, surface_id: &str) -> Result<NavigationSurface> {
        self.open_surface_page_with_options(surface_id, None, 100, &LabelOptions::default())
    }

    fn open_surface_page(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<NavigationSurface> {
        self.open_surface_page_with_options(surface_id, cursor, limit, &LabelOptions::default())
    }

    fn open_surface_with_options(
        &self,
        surface_id: &str,
        options: &LabelOptions,
    ) -> Result<NavigationSurface> {
        self.open_surface_page_with_options(surface_id, None, 100, options)
    }

    fn open_surface_page_with_options(
        &self,
        surface_id: &str,
        cursor: Option<&str>,
        limit: usize,
        options: &LabelOptions,
    ) -> Result<NavigationSurface> {
        if surface_id == "search" {
            return Ok(NavigationSurface::FallbackSearch {
                surface_id: surface_id.to_owned(),
            });
        }
        let has_ssed_components =
            self.ssed_catalog.is_some() || self.metadata.format_family == FormatFamily::Ssed;
        let has_multiview_provider = self.has_multiview_provider();
        let has_hourei_provider = self.has_hourei_provider();
        let mut surface = match surface_id {
            "title-index" if has_ssed_components => {
                self.open_ssed_title_index_surface(surface_id, cursor, limit, options)
            }
            "menu" if has_ssed_components => self.open_ssed_menu_surface(
                surface_id,
                SsedComponentRole::Menu,
                "MENU.DIC",
                cursor,
                limit,
                options,
            ),
            "toc" if has_ssed_components => self.open_ssed_menu_surface(
                surface_id,
                SsedComponentRole::Toc,
                "TOC.DIC",
                cursor,
                limit,
                options,
            ),
            id if has_ssed_components && id.starts_with("ssed-nav:") => {
                let component_name =
                    ssed_direct_navigation_component_name_from_surface_id(surface_id);
                let Some(component_name) = component_name else {
                    return Ok(NavigationSurface::Deferred {
                        surface_id: surface_id.to_owned(),
                        diagnostics: vec![Diagnostic::warning(
                            "ssed_navigation_surface_id_invalid",
                            format!("{surface_id} is not a valid SSED navigation surface id"),
                        )],
                    });
                };
                let Some(catalog) = &self.ssed_catalog else {
                    return Ok(NavigationSurface::Deferred {
                        surface_id: surface_id.to_owned(),
                        diagnostics: vec![Diagnostic::error(
                            "ssed_catalog_missing",
                            "SSED navigation surfaces require a parsed SSEDINFO catalog",
                        )],
                    });
                };
                let Some(component) = catalog
                    .component_named(&component_name)
                    .filter(|component| component.has_positive_range())
                else {
                    return Ok(NavigationSurface::Deferred {
                        surface_id: surface_id.to_owned(),
                        diagnostics: vec![Diagnostic::info(
                            "ssed_navigation_component_missing",
                            format!("{component_name} is not declared in this SSED catalog"),
                        )],
                    });
                };
                self.open_ssed_navigation_component_surface(
                    surface_id, component, cursor, limit, options,
                )
            }
            id if has_ssed_components && id.starts_with("multi:") => {
                self.open_ssed_multi_selector_surface(surface_id, cursor, limit, options)
            }
            "screen-menu" if has_ssed_components => self.open_ssed_screen_menu_surface(surface_id),
            "encyclopedia" if has_ssed_components => {
                self.open_ssed_encyclopedia_surface(surface_id, options)
            }
            "britannica-whatday" if has_ssed_components => {
                self.open_britannica_whatday_surface(surface_id, cursor, limit)
            }
            "britannica-top" if has_ssed_components => {
                self.open_britannica_top_surface(surface_id, options)
            }
            id if has_ssed_components
                && (id.starts_with("aux-index:") || id.starts_with("numeric-aux:")) =>
            {
                self.open_ssed_aux_index_surface(surface_id, cursor, limit, options)
            }
            "hanrei" if has_ssed_components => {
                self.open_ssed_hanrei_surface(surface_id, cursor, limit)
            }
            "ios-retained-fts" if has_ssed_components => Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: self.retained_ios_fts_deferred_diagnostics(),
            }),
            "lved-list" if self.lved_store.is_some() => {
                self.open_lved_list_surface(surface_id, cursor, limit)
            }
            "info" if self.lved_store.is_some() => {
                self.open_lved_info_surface(surface_id, cursor, limit)
            }
            id if self.lved_store.is_some()
                && LVED_NAMED_BODY_SURFACES
                    .iter()
                    .any(|(named_surface_id, _)| id == *named_surface_id) =>
            {
                self.open_lved_named_page_surface(surface_id, cursor, limit)
            }
            "lved-tree" if self.lved_store.is_some() => {
                self.open_lved_tree_surface(surface_id, cursor, limit)
            }
            "panels" if has_ssed_components => {
                self.open_ssed_panel_surface(surface_id, cursor, limit, options)
            }
            id if has_ssed_components && id.starts_with("panels:") => {
                self.open_ssed_panel_surface(surface_id, cursor, limit, options)
            }
            id if has_ssed_components
                && super::ssed_ios_plist_surfaces::is_ssed_ios_panel_surface_id(id) =>
            {
                self.open_ssed_panel_surface(surface_id, cursor, limit, options)
            }
            id if has_ssed_components
                && super::ssed_ios_plist_surfaces::is_ssed_ios_html_list_surface_id(id) =>
            {
                self.open_ssed_ios_html_list_surface(surface_id, cursor, limit)
            }
            id if has_ssed_components
                && super::ssed_ios_plist_surfaces::is_ssed_ios_dictlist_other_surface_id(id) =>
            {
                self.open_ssed_ios_dictlist_other_surface(surface_id, cursor, limit)
            }
            id if has_ssed_components
                && super::ssed_ios_app_menu_surfaces::is_ssed_ios_app_menu_xml_surface_id(id) =>
            {
                self.open_ssed_ios_app_menu_xml_surface(surface_id, cursor, limit)
            }
            id if has_ssed_components
                && super::ssed_package_html_surfaces::is_ssed_exinfo_index_url_surface_id(id) =>
            {
                self.open_ssed_exinfo_index_url_surface(surface_id, cursor, limit)
            }
            id if has_ssed_components
                && super::ssed_ios_plist_surfaces::is_ssed_ios_table_list_surface_id(id) =>
            {
                self.open_ssed_ios_table_list_surface(surface_id, cursor, limit, options)
            }
            id if has_ssed_components
                && super::ssed_ios_search::is_ssed_ios_full_db_list_surface_id(id) =>
            {
                self.open_ssed_ios_full_db_list_surface(surface_id, cursor, limit, options)
            }
            id if has_multiview_provider && (id == "menuData" || id.starts_with("menuData:")) => {
                self.open_multiview_menu_surface(surface_id, cursor, limit)
            }
            "kana-panel" if has_hourei_provider => self.open_hourei_kana_panel_surface(surface_id),
            id if has_hourei_provider
                && super::hourei_navigation::hourei_kana_initial_from_surface_id(id).is_some() =>
            {
                let kana_initial =
                    super::hourei_navigation::hourei_kana_initial_from_surface_id(id)
                        .unwrap_or_default();
                self.open_hourei_kana_initial_surface(surface_id, kana_initial, cursor, limit)
            }
            "law-tree" if has_hourei_provider => self.open_hourei_law_tree_surface(surface_id),
            _ if has_ssed_components => Ok(NavigationSurface::Deferred {
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
        }?;
        surface.populate_target_hrefs();
        Ok(surface)
    }
}

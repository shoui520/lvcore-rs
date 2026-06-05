use super::*;

impl ReaderBookPackage {
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
        surfaces.push(HomeSurface {
            href: None,
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
        Ok(())
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
                let hanrei_pages = self.discover_ssed_hanrei_pages()?;
                if !hanrei_pages.is_empty() {
                    let diagnostics = hanrei_pages
                        .iter()
                        .flat_map(|page| page.diagnostics.clone())
                        .collect::<Vec<_>>();
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
                        diagnostics,
                    });
                }
                if self.lved_store.is_some() {
                    self.push_lved_sqlite_home_surfaces(&mut surfaces)?;
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
                    surfaces.push(HomeSurface {
                        href: None,
                        surface_id: "panels".to_owned(),
                        kind: NavigationSurfaceKind::Panel,
                        status: NavigationStatus::Available,
                        title_html: "Panels".to_owned(),
                        title_text: "Panels".to_owned(),
                        target: Some(TargetToken::new(&InternalTarget::MenuItem {
                            surface_id: "panels".to_owned(),
                            item_id: "root".to_owned(),
                        })?),
                        diagnostics: Vec::new(),
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
            FormatFamily::LvedSqlite3 => {
                self.push_lved_sqlite_home_surfaces(&mut surfaces)?;
            }
            FormatFamily::LvlMultiView => {
                surfaces.push(HomeSurface {
                    href: None,
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
            FormatFamily::Unknown => {}
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
        match (self.metadata.format_family, surface_id) {
            (FormatFamily::Ssed, "title-index") => {
                self.open_ssed_title_index_surface(surface_id, cursor, limit, options)
            }
            (FormatFamily::Ssed, "menu") => self.open_ssed_menu_surface(
                surface_id,
                SsedComponentRole::Menu,
                "MENU.DIC",
                cursor,
                limit,
                options,
            ),
            (FormatFamily::Ssed, "toc") => self.open_ssed_menu_surface(
                surface_id,
                SsedComponentRole::Toc,
                "TOC.DIC",
                cursor,
                limit,
                options,
            ),
            (FormatFamily::Ssed, id) if id.starts_with("multi:") => {
                self.open_ssed_multi_selector_surface(surface_id, cursor, limit, options)
            }
            (FormatFamily::Ssed, "screen-menu") => self.open_ssed_screen_menu_surface(surface_id),
            (FormatFamily::Ssed, "encyclopedia") => {
                self.open_ssed_encyclopedia_surface(surface_id, options)
            }
            (FormatFamily::Ssed, "britannica-whatday") => {
                self.open_britannica_whatday_surface(surface_id, cursor, limit)
            }
            (FormatFamily::Ssed, "britannica-top") => {
                self.open_britannica_top_surface(surface_id, options)
            }
            (FormatFamily::Ssed, id)
                if id.starts_with("aux-index:") || id.starts_with("numeric-aux:") =>
            {
                self.open_ssed_aux_index_surface(surface_id, cursor, limit, options)
            }
            (FormatFamily::Ssed, "hanrei") => {
                self.open_ssed_hanrei_surface(surface_id, cursor, limit)
            }
            (FormatFamily::Ssed, "ios-retained-fts") => Ok(NavigationSurface::Deferred {
                surface_id: surface_id.to_owned(),
                diagnostics: self.retained_ios_fts_deferred_diagnostics(),
            }),
            (FormatFamily::Ssed, "lved-list") if self.lved_store.is_some() => {
                self.open_lved_list_surface(surface_id, cursor, limit)
            }
            (FormatFamily::Ssed, "info") if self.lved_store.is_some() => {
                self.open_lved_info_surface(surface_id, cursor, limit)
            }
            (FormatFamily::Ssed, "lved-tree") if self.lved_store.is_some() => {
                self.open_lved_tree_surface(surface_id)
            }
            (FormatFamily::Ssed, "panels") => {
                self.open_ssed_panel_surface(surface_id, cursor, limit, options)
            }
            (FormatFamily::Ssed, id) if id.starts_with("panels:") => {
                self.open_ssed_panel_surface(surface_id, cursor, limit, options)
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
            (FormatFamily::Hourei, "kana-panel") => self.open_hourei_kana_panel_surface(surface_id),
            (FormatFamily::Hourei, id)
                if super::hourei_navigation::hourei_kana_initial_from_surface_id(id).is_some() =>
            {
                let kana_initial =
                    super::hourei_navigation::hourei_kana_initial_from_surface_id(id)
                        .unwrap_or_default();
                self.open_hourei_kana_initial_surface(surface_id, kana_initial, cursor, limit)
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

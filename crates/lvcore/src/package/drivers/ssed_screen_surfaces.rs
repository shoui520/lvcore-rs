use super::*;

impl ReaderBookPackage {
    pub(super) fn open_ssed_screen_menu_surface(
        &self,
        surface_id: &str,
    ) -> Result<NavigationSurface> {
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
}

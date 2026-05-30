use super::*;

impl ReaderBookPackage {
    fn view_for_navigation_surface_target(
        &self,
        target: TargetToken,
        surface_id: &str,
        title: Option<String>,
    ) -> Result<ResolvedTargetView> {
        let scroll_anchor = scroll_anchor_for_token(&target)?;
        let surface = self.open_surface(surface_id)?;
        let kind = match &surface {
            NavigationSurface::Panel { .. } => ResolvedTargetKind::PanelSurface,
            NavigationSurface::InfoPages { .. } => ResolvedTargetKind::InfoPage,
            NavigationSurface::Deferred { .. } => ResolvedTargetKind::Deferred,
            _ => ResolvedTargetKind::NavigationSurface,
        };
        let capabilities = if matches!(kind, ResolvedTargetKind::PanelSurface) {
            vec![crate::render::RenderCapability::Panels]
        } else {
            Vec::new()
        };
        let mut diagnostics = Vec::new();
        if let NavigationSurface::Deferred {
            diagnostics: surface_diagnostics,
            ..
        } = &surface
        {
            diagnostics.extend(surface_diagnostics.clone());
        }
        Ok(ResolvedTargetView {
            kind,
            target,
            title: title.or_else(|| Some(surface_id.to_owned())),
            display_html: None,
            basic_text: None,
            scroll_anchor,
            surface: Some(surface),
            resources: Vec::new(),
            links: Vec::new(),
            capabilities,
            diagnostics,
            debug_trace: None,
        })
    }

    fn view_for_multiview_navigation_target(
        &self,
        target: TargetToken,
        href: &str,
    ) -> Result<Option<ResolvedTargetView>> {
        let Some((title, surface)) = self.multiview_navigation_surface_for_href(href)? else {
            return Ok(None);
        };
        Ok(Some(ResolvedTargetView {
            kind: ResolvedTargetKind::NavigationSurface,
            target,
            title: Some(title),
            display_html: None,
            basic_text: None,
            scroll_anchor: None,
            surface: Some(surface),
            resources: Vec::new(),
            links: Vec::new(),
            capabilities: Vec::new(),
            diagnostics: Vec::new(),
            debug_trace: None,
        }))
    }

    fn view_for_renderer_input(
        &self,
        input: RendererInput,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        match input {
            RendererInput::PreservedHtml {
                target,
                html,
                source,
            } => {
                let scroll_anchor = scroll_anchor_for_token(&target)?;
                let view_kind = self.resolved_kind_for_body_target(&target)?;
                let title = self.title_for_body_target(&target)?;
                if options.mode == RenderMode::BasicText {
                    return Ok(ResolvedTargetView {
                        kind: view_kind,
                        target,
                        title: Some(title.unwrap_or_else(|| "Entry".to_owned())),
                        display_html: None,
                        basic_text: Some(html_basic_text(&html)),
                        scroll_anchor,
                        surface: None,
                        resources: Vec::new(),
                        links: Vec::new(),
                        capabilities: Vec::new(),
                        diagnostics: Vec::new(),
                        debug_trace: None,
                    });
                }
                let normalized = match source {
                    BodySourceKind::LvedSqlite => self.normalize_lved_html_refs(&html)?,
                    BodySourceKind::LvlMultiViewSqlite => {
                        self.normalize_multiview_html_refs(&html)?
                    }
                    BodySourceKind::HoureiSqlite => self.normalize_hourei_html_refs(&html)?,
                    _ => NormalizedHtmlRefs {
                        html,
                        resources: Vec::new(),
                        links: Vec::new(),
                        diagnostics: Vec::new(),
                    },
                };
                Ok(ResolvedTargetView {
                    kind: view_kind,
                    target,
                    title: Some(title.unwrap_or_else(|| "Entry".to_owned())),
                    display_html: Some(normalized.html),
                    basic_text: None,
                    scroll_anchor,
                    surface: None,
                    resources: normalized.resources,
                    links: normalized.links,
                    capabilities: vec![crate::render::RenderCapability::Html],
                    diagnostics: normalized.diagnostics,
                    debug_trace: None,
                })
            }
            RendererInput::HcSsedStream {
                target,
                component,
                offset,
                length,
                profile_hint,
                hc_profile,
                resources,
                mut diagnostics,
            } => {
                let scroll_anchor = scroll_anchor_for_token(&target)?;
                Ok(ResolvedTargetView {
                    kind: crate::render::ResolvedTargetKind::Deferred,
                    target,
                    title: Some("SSED entry stream".to_owned()),
                    display_html: None,
                    basic_text: None,
                    scroll_anchor,
                    surface: None,
                    resources,
                    links: Vec::new(),
                    capabilities: vec![crate::render::RenderCapability::HcRenderInput],
                    diagnostics: {
                        diagnostics.push(Diagnostic::info(
                            "hc_render_deferred",
                            "SSED stream resolved successfully; HC/profile rendering is not implemented yet",
                        ));
                        diagnostics
                    },
                    debug_trace: (options.include_debug_trace || options.mode == RenderMode::Debug)
                        .then(|| {
                            json!({
                                "body": {
                                    "kind": "ssed_stream",
                                    "component": component,
                                    "offset": offset,
                                    "length": length,
                                    "profile_hint": profile_hint,
                                    "hc_profile": hc_profile,
                                }
                            })
                            .to_string()
                        }),
                })
            }
            RendererInput::SemanticFallback { target, text } => {
                let scroll_anchor = scroll_anchor_for_token(&target)?;
                Ok(ResolvedTargetView {
                    kind: crate::render::ResolvedTargetKind::EntryBody,
                    target,
                    title: Some("Semantic fallback".to_owned()),
                    display_html: None,
                    basic_text: Some(text),
                    scroll_anchor,
                    surface: None,
                    resources: Vec::new(),
                    links: Vec::new(),
                    capabilities: Vec::new(),
                    diagnostics: vec![Diagnostic::info(
                        "semantic_fallback",
                        "visual renderer is unavailable; semantic fallback was returned",
                    )],
                    debug_trace: None,
                })
            }
            RendererInput::Unsupported {
                target,
                reason,
                diagnostics,
            } => {
                let scroll_anchor = scroll_anchor_for_token(&target)?;
                Ok(ResolvedTargetView {
                    kind: crate::render::ResolvedTargetKind::Unsupported,
                    target,
                    title: Some(reason),
                    display_html: None,
                    basic_text: None,
                    scroll_anchor,
                    surface: None,
                    resources: Vec::new(),
                    links: Vec::new(),
                    capabilities: Vec::new(),
                    diagnostics,
                    debug_trace: None,
                })
            }
        }
    }

    fn finalize_resolved_view(
        &self,
        view: ResolvedTargetView,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let view = if options.mode == RenderMode::GenericHtml {
            self.finalize_generic_html_view(view)?
        } else {
            view
        };
        Ok(finalize_resolved_view(view, options))
    }

    fn finalize_generic_html_view(&self, view: ResolvedTargetView) -> Result<ResolvedTargetView> {
        finalize_generic_html_display(view, |token| self.generic_html_data_url(token))
    }

    fn generic_html_data_url(&self, token: &str) -> Result<Option<String>> {
        let resource_token = ResourceToken::from_opaque(token.to_owned());
        let resource_ref = self.resolve_resource(&resource_token)?;
        let Some(mime_type) = resource_ref.mime_type.as_deref() else {
            return Ok(None);
        };
        let bytes = self.read_resource(&resource_token)?;
        if bytes.len() > generic_html_inline_resource_max_bytes() {
            return Ok(None);
        }
        Ok(Some(generic_html_data_url(mime_type, &bytes)))
    }

    pub(super) fn validate_plain_component(
        &self,
        component: &SsedComponent,
    ) -> std::result::Result<(), Diagnostic> {
        if !component.has_positive_range() {
            return Err(Diagnostic::warning(
                "ssed_component_optional_absent",
                format!("{} has no positive block range", component.filename),
            ));
        }
        let path = match self.resolve_readable_ssed_component_path(component) {
            Ok(Some(path)) => path,
            Ok(None) => {
                return Err(Diagnostic::warning(
                    "ssed_component_file_missing",
                    format!("{} is declared but not present on disk", component.filename),
                ));
            }
            Err(err) => {
                return Err(Diagnostic::warning(
                    "ssed_component_decode_deferred",
                    format!(
                        "{} is not readable as SSEDDATA yet: {err}",
                        component.filename
                    ),
                ));
            }
        };
        SsedDataHeader::parse_file(&path).map_err(|err| {
            Diagnostic::warning(
                "ssed_component_decode_deferred",
                format!(
                    "{} does not expose a readable plain SSEDDATA header yet: {err}",
                    component.filename
                ),
            )
        })?;
        Ok(())
    }
}

impl RendererProvider for ReaderBookPackage {
    fn render_target(
        &self,
        token: &TargetToken,
        options: &crate::render::RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let target = token.decode()?;
        let view = match target {
            InternalTarget::Unsupported { reason } => Ok(ResolvedTargetView::unsupported(
                token.clone(),
                "Unsupported target",
                Diagnostic::warning("target_unsupported", reason),
            )),
            InternalTarget::LvedCrossBook {
                link_kind,
                dict_code,
                content_id,
                ..
            } => Ok(ResolvedTargetView::unsupported(
                token.clone(),
                "Cross-dictionary LVED link",
                Diagnostic::info(
                    "lved_cross_book_deferred",
                    format!(
                        "LVED {link_kind} link to dictionary {dict_code} content {content_id} requires library-wide routing"
                    ),
                ),
            )),
            InternalTarget::LvedViewerHook { hook, value } => Ok(ResolvedTargetView::unsupported(
                token.clone(),
                "LVED viewer hook",
                Diagnostic::info(
                    "lved_viewer_hook_deferred",
                    format!("LVED viewer hook {hook} is intentionally not executed: {value}"),
                ),
            )),
            InternalTarget::Resource { resource, anchor } => {
                let decoded_resource = resource.decode()?;
                let resource_ref = self.resolve_resource(&resource)?;
                Ok(
                    if let InternalResource::PackageFile {
                        path,
                        resource_kind,
                    } = &decoded_resource
                        && (*resource_kind == ResourceKind::Html
                            || path_has_extension(path, &["html", "htm"]))
                    {
                        self.render_package_html_resource(
                            token.clone(),
                            &resource,
                            path,
                            resource_ref,
                            options,
                        )?
                    } else if let InternalResource::SsedLooseFile {
                        path,
                        resource_kind,
                        ..
                    } = &decoded_resource
                        && (*resource_kind == ResourceKind::Html
                            || path_has_extension(path, &["html", "htm", "body", "top"]))
                    {
                        self.render_ssed_loose_html_resource(
                            token.clone(),
                            &resource,
                            path,
                            resource_ref,
                            options,
                        )?
                    } else if let InternalResource::ChmFile {
                        chm_path,
                        entry_path,
                        resource_kind,
                    } = &decoded_resource
                        && (*resource_kind == ResourceKind::Html
                            || path_has_extension(entry_path, &["html", "htm"]))
                    {
                        self.render_chm_html_resource(
                            token.clone(),
                            &resource,
                            chm_path,
                            entry_path,
                            resource_ref,
                            options,
                        )?
                    } else {
                        let diagnostics = resource_ref.diagnostics.clone();
                        ResolvedTargetView {
                            kind: ResolvedTargetKind::MediaResource,
                            target: token.clone(),
                            title: resource_ref.label.clone(),
                            display_html: None,
                            basic_text: None,
                            scroll_anchor: anchor,
                            surface: None,
                            resources: vec![resource_ref],
                            links: Vec::new(),
                            capabilities: Vec::new(),
                            diagnostics,
                            debug_trace: None,
                        }
                    },
                )
            }
            InternalTarget::PanelCell { panel_id, .. } => {
                let surface_id = format!("panels:{panel_id}");
                self.view_for_navigation_surface_target(token.clone(), &surface_id, Some(panel_id))
            }
            InternalTarget::MenuItem { surface_id, .. }
            | InternalTarget::TocItem { surface_id, .. }
            | InternalTarget::TitleIndexItem { surface_id, .. } => {
                self.view_for_navigation_surface_target(token.clone(), &surface_id, None)
            }
            InternalTarget::MultiviewHref { href, anchor: _ } if href == "menuData.xml" => self
                .view_for_navigation_surface_target(
                    token.clone(),
                    "menuData",
                    Some("MultiView menu".to_owned()),
                ),
            InternalTarget::MultiviewHref { href, anchor } => {
                if anchor.is_none()
                    && let Some(view) =
                        self.view_for_multiview_navigation_target(token.clone(), &href)?
                {
                    Ok(view)
                } else {
                    let input = self.renderer_input_for_target(token)?;
                    self.view_for_renderer_input(input, options)
                }
            }
            _ => {
                let input = self.renderer_input_for_target(token)?;
                self.view_for_renderer_input(input, options)
            }
        }?;
        self.finalize_resolved_view(view, options)
    }
}

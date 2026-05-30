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

    fn render_package_html_resource(
        &self,
        target: TargetToken,
        resource: &ResourceToken,
        path: &str,
        resource_ref: ResourceRef,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let scroll_anchor = scroll_anchor_for_token(&target)?;
        let data = self.read_resource(resource)?;
        let html = decode_package_html_text(&data);
        let title = resource_ref.label.clone();
        if options.mode == RenderMode::BasicText {
            return Ok(ResolvedTargetView {
                kind: resolved_kind_for_package_html_path(path),
                target,
                title,
                display_html: None,
                basic_text: Some(html_basic_text(&html)),
                scroll_anchor,
                surface: None,
                resources: Vec::new(),
                links: Vec::new(),
                capabilities: Vec::new(),
                diagnostics: resource_ref.diagnostics,
                debug_trace: None,
            });
        }

        let mut normalized = self.normalize_package_file_html_refs(&html, path)?;
        let resources = normalized.resources;
        let mut diagnostics = resource_ref.diagnostics;
        diagnostics.append(&mut normalized.diagnostics);
        Ok(ResolvedTargetView {
            kind: resolved_kind_for_package_html_path(path),
            target,
            title,
            display_html: Some(normalized.html),
            basic_text: None,
            scroll_anchor,
            surface: None,
            resources,
            links: normalized.links,
            capabilities: vec![crate::render::RenderCapability::Html],
            diagnostics,
            debug_trace: None,
        })
    }

    fn render_ssed_loose_html_resource(
        &self,
        target: TargetToken,
        resource: &ResourceToken,
        path: &str,
        resource_ref: ResourceRef,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let scroll_anchor = scroll_anchor_for_token(&target)?;
        let data = self.read_resource(resource)?;
        let raw_html = decode_package_html_text(&data);
        let html = if path_has_extension(path, &["body", "top"]) {
            render_britannica_html_fragment(&raw_html)
        } else {
            raw_html
        };
        let title = resource_ref.label.clone();
        if options.mode == RenderMode::BasicText {
            return Ok(ResolvedTargetView {
                kind: ResolvedTargetKind::InfoPage,
                target,
                title,
                display_html: None,
                basic_text: Some(html_basic_text(&html)),
                scroll_anchor,
                surface: None,
                resources: Vec::new(),
                links: Vec::new(),
                capabilities: Vec::new(),
                diagnostics: resource_ref.diagnostics,
                debug_trace: None,
            });
        }

        let mut normalized = self.normalize_britannica_loose_html_refs(&html)?;
        let resources = normalized.resources;
        let mut diagnostics = resource_ref.diagnostics;
        diagnostics.append(&mut normalized.diagnostics);
        Ok(ResolvedTargetView {
            kind: ResolvedTargetKind::InfoPage,
            target,
            title,
            display_html: Some(normalized.html),
            basic_text: None,
            scroll_anchor,
            surface: None,
            resources,
            links: normalized.links,
            capabilities: vec![crate::render::RenderCapability::Html],
            diagnostics,
            debug_trace: None,
        })
    }

    fn render_chm_html_resource(
        &self,
        target: TargetToken,
        resource: &ResourceToken,
        chm_path: &str,
        entry_path: &str,
        resource_ref: ResourceRef,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let scroll_anchor = scroll_anchor_for_token(&target)?;
        let data = self.read_resource(resource)?;
        let html = decode_package_html_text(&data);
        let title = resource_ref.label.clone();
        let kind = resolved_kind_for_package_html_path(&format!("{chm_path}/{entry_path}"));
        if options.mode == RenderMode::BasicText {
            return Ok(ResolvedTargetView {
                kind,
                target,
                title,
                display_html: None,
                basic_text: Some(html_basic_text(&html)),
                scroll_anchor,
                surface: None,
                resources: Vec::new(),
                links: Vec::new(),
                capabilities: Vec::new(),
                diagnostics: resource_ref.diagnostics,
                debug_trace: None,
            });
        }

        let mut normalized = self.normalize_chm_html_refs(&html, chm_path, entry_path)?;
        let resources = normalized.resources;
        let mut diagnostics = resource_ref.diagnostics;
        diagnostics.append(&mut normalized.diagnostics);
        Ok(ResolvedTargetView {
            kind,
            target,
            title,
            display_html: Some(normalized.html),
            basic_text: None,
            scroll_anchor,
            surface: None,
            resources,
            links: normalized.links,
            capabilities: vec![crate::render::RenderCapability::Html],
            diagnostics,
            debug_trace: None,
        })
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

    fn normalize_britannica_loose_html_refs(&self, html: &str) -> Result<NormalizedHtmlRefs> {
        let inline = self.expand_britannica_inline_address_markers(html)?;
        let mut output = String::with_capacity(inline.html.len());
        let mut links = inline.links;
        let resources = Vec::new();
        let mut diagnostics = inline.diagnostics;
        let mut seen_target_tokens = BTreeSet::new();
        for link in &links {
            seen_target_tokens.insert(link.token.as_str().to_owned());
        }
        let mut cursor = 0usize;
        let lower = inline.html.to_ascii_lowercase();

        while let Some(attr) = next_html_href_or_src_attr(&inline.html, &lower, cursor) {
            output.push_str(&inline.html[cursor..attr.value_start]);
            let raw_value = &inline.html[attr.value_start..attr.value_end];
            if attr.name == HtmlAttrName::Href
                && let Some(address) = parse_lved_address(raw_value)
                && let Some(target) = self.ssed_target_for_loose_address(
                    address.block,
                    address.offset,
                    &mut diagnostics,
                )?
            {
                let decoded = target.decode()?;
                if seen_target_tokens.insert(target.as_str().to_owned()) {
                    links.push(TargetLink::new(raw_value, &decoded)?);
                }
                output.push_str(&format!("lvcore://target/{}", target.as_str()));
            } else {
                output.push_str(raw_value);
            }
            cursor = attr.value_end;
        }
        output.push_str(&inline.html[cursor..]);

        Ok(NormalizedHtmlRefs {
            html: output,
            resources,
            links,
            diagnostics,
        })
    }

    fn expand_britannica_inline_address_markers(&self, html: &str) -> Result<NormalizedHtmlRefs> {
        let mut output = String::with_capacity(html.len());
        let mut links = Vec::new();
        let resources = Vec::new();
        let mut diagnostics = Vec::new();
        let mut cursor = 0usize;
        let mut seen_target_tokens = BTreeSet::new();

        while let Some((marker_start, marker_kind)) = next_britannica_inline_marker(html, cursor) {
            output.push_str(&html[cursor..marker_start]);
            let spec_start = marker_start + marker_kind.start.len();
            let Some(spec) = html.get(spec_start..spec_start + 13) else {
                output.push_str(&html[marker_start..]);
                cursor = html.len();
                break;
            };
            let Some((block_hex, offset_hex)) = spec.split_once(':') else {
                output.push_str(&html[marker_start..marker_start + marker_kind.start.len()]);
                cursor = marker_start + marker_kind.start.len();
                continue;
            };
            if block_hex.len() != 8
                || offset_hex.len() != 4
                || !block_hex.bytes().all(|byte| byte.is_ascii_hexdigit())
                || !offset_hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                output.push_str(&html[marker_start..marker_start + marker_kind.start.len()]);
                cursor = marker_start + marker_kind.start.len();
                continue;
            }
            let label_start = spec_start + 13;
            let Some(end_relative) = html[label_start..].find(marker_kind.end) else {
                output.push_str(&html[marker_start..]);
                cursor = html.len();
                break;
            };
            let label_end = label_start + end_relative;
            let label = &html[label_start..label_end];
            let block = u32::from_str_radix(block_hex, 16).unwrap_or_default();
            let offset = u32::from_str_radix(offset_hex, 16).unwrap_or_default();
            if let Some(target) =
                self.ssed_target_for_loose_address(block, offset, &mut diagnostics)?
            {
                let decoded = target.decode()?;
                if seen_target_tokens.insert(target.as_str().to_owned()) {
                    links.push(TargetLink::new(label, &decoded)?);
                }
                output.push_str(&format!(
                    r#"<a class="link" href="lvcore://target/{}">{}</a>"#,
                    target.as_str(),
                    escape_plain_label_html(label)
                ));
            } else {
                output.push_str(&escape_plain_label_html(label));
            }
            cursor = label_end + marker_kind.end.len();
        }
        output.push_str(&html[cursor..]);
        Ok(NormalizedHtmlRefs {
            html: output,
            resources,
            links,
            diagnostics,
        })
    }

    fn normalize_package_file_html_refs(
        &self,
        html: &str,
        path: &str,
    ) -> Result<NormalizedHtmlRefs> {
        let base_dir = package_html_base_dir(path);
        let mut output = String::with_capacity(html.len());
        let mut resources = Vec::new();
        let mut links = Vec::new();
        let mut diagnostics = Vec::new();
        let mut seen_resource_tokens = BTreeSet::new();
        let mut seen_target_tokens = BTreeSet::new();
        let mut cursor = 0usize;
        let lower = html.to_ascii_lowercase();

        while let Some(attr) = next_html_href_or_src_attr(html, &lower, cursor) {
            output.push_str(&html[cursor..attr.value_start]);
            let raw_value = &html[attr.value_start..attr.value_end];
            if let Some(reference) = package_relative_html_reference(&base_dir, raw_value) {
                if attr.name == HtmlAttrName::Href
                    && path_has_extension(&reference.path, &["html", "htm"])
                {
                    let resource = InternalResource::PackageFile {
                        path: reference.path.clone(),
                        resource_kind: ResourceKind::Html,
                    };
                    let resource = ResourceToken::new(&resource)?;
                    let target = InternalTarget::Resource {
                        resource,
                        anchor: reference.anchor,
                    };
                    let token = TargetToken::new(&target)?;
                    if seen_target_tokens.insert(token.as_str().to_owned()) {
                        links.push(TargetLink::new(raw_value, &target)?);
                    }
                    output.push_str(&format!("lvcore://target/{}", token.as_str()));
                } else {
                    let resource = InternalResource::PackageFile {
                        resource_kind: resource_kind_from_path(&reference.path),
                        path: reference.path,
                    };
                    let token = ResourceToken::new(&resource)?;
                    let href = format!("lvcore://resource/{}", token.as_str());
                    if seen_resource_tokens.insert(token.as_str().to_owned()) {
                        let resource_ref = self.resolve_resource(&token)?;
                        diagnostics.extend(resource_ref.diagnostics.clone());
                        resources.push(resource_ref);
                    }
                    output.push_str(&href);
                    if let Some(anchor) = reference.anchor {
                        output.push('#');
                        output.push_str(&anchor);
                    }
                }
            } else {
                output.push_str(raw_value);
            }
            cursor = attr.value_end;
        }
        output.push_str(&html[cursor..]);

        Ok(NormalizedHtmlRefs {
            html: output,
            resources,
            links,
            diagnostics,
        })
    }

    fn normalize_chm_html_refs(
        &self,
        html: &str,
        chm_path: &str,
        entry_path: &str,
    ) -> Result<NormalizedHtmlRefs> {
        let base_dir = package_html_base_dir(entry_path);
        let mut output = String::with_capacity(html.len());
        let mut resources = Vec::new();
        let mut links = Vec::new();
        let mut diagnostics = Vec::new();
        let mut seen_resource_tokens = BTreeSet::new();
        let mut seen_target_tokens = BTreeSet::new();
        let mut cursor = 0usize;
        let lower = html.to_ascii_lowercase();

        while let Some(attr) = next_html_href_or_src_attr(html, &lower, cursor) {
            output.push_str(&html[cursor..attr.value_start]);
            let raw_value = &html[attr.value_start..attr.value_end];
            if let Some(reference) = package_relative_html_reference(&base_dir, raw_value) {
                if attr.name == HtmlAttrName::Href
                    && path_has_extension(&reference.path, &["html", "htm"])
                {
                    let resource = InternalResource::ChmFile {
                        chm_path: chm_path.to_owned(),
                        entry_path: reference.path,
                        resource_kind: ResourceKind::Html,
                    };
                    let resource = ResourceToken::new(&resource)?;
                    let target = InternalTarget::Resource {
                        resource,
                        anchor: reference.anchor,
                    };
                    let token = TargetToken::new(&target)?;
                    if seen_target_tokens.insert(token.as_str().to_owned()) {
                        links.push(TargetLink::new(raw_value, &target)?);
                    }
                    output.push_str(&format!("lvcore://target/{}", token.as_str()));
                } else {
                    let resource = InternalResource::ChmFile {
                        resource_kind: resource_kind_from_path(&reference.path),
                        chm_path: chm_path.to_owned(),
                        entry_path: reference.path,
                    };
                    let token = ResourceToken::new(&resource)?;
                    let href = format!("lvcore://resource/{}", token.as_str());
                    if seen_resource_tokens.insert(token.as_str().to_owned()) {
                        let resource_ref = self.resolve_resource(&token)?;
                        diagnostics.extend(resource_ref.diagnostics.clone());
                        resources.push(resource_ref);
                    }
                    output.push_str(&href);
                    if let Some(anchor) = reference.anchor {
                        output.push('#');
                        output.push_str(&anchor);
                    }
                }
            } else {
                output.push_str(raw_value);
            }
            cursor = attr.value_end;
        }
        output.push_str(&html[cursor..]);

        Ok(NormalizedHtmlRefs {
            html: output,
            resources,
            links,
            diagnostics,
        })
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

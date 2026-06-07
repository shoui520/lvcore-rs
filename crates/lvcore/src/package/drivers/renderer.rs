use super::*;

impl ReaderBookPackage {
    fn view_for_navigation_surface_target(
        &self,
        target: TargetToken,
        surface_id: &str,
        title: Option<String>,
        cursor: Option<&str>,
        options: &RenderOptions,
    ) -> Result<ResolvedTargetView> {
        let scroll_anchor = scroll_anchor_for_token(&target)?;
        let surface = self.open_surface_page_with_options(
            surface_id,
            cursor,
            100,
            &LabelOptions {
                gaiji_policy: options.gaiji_policy.clone(),
            },
        )?;
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
        let view_title = navigation_surface_view_title(title, surface_id, &surface);
        Ok(ResolvedTargetView {
            href: String::new(),
            kind,
            target,
            title: Some(view_title),
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
        let scroll_anchor = scroll_anchor_for_token(&target)?;
        Ok(Some(ResolvedTargetView {
            href: String::new(),
            kind: ResolvedTargetKind::NavigationSurface,
            target,
            title: Some(title),
            display_html: None,
            basic_text: None,
            scroll_anchor,
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
                        href: String::new(),
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
                    BodySourceKind::BritannicaChronologySqlite => {
                        self.normalize_britannica_loose_html_refs(&html)?
                    }
                    BodySourceKind::SidecarHtml | BodySourceKind::RendererDatabase => {
                        self.normalize_ssed_sidecar_lved_html_refs(&html)?
                    }
                    _ => NormalizedHtmlRefs {
                        html,
                        resources: Vec::new(),
                        links: Vec::new(),
                        diagnostics: Vec::new(),
                    },
                };
                Ok(ResolvedTargetView {
                    href: String::new(),
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
                let marker_profile =
                    hc_marker_profile_for_renderer(profile_hint.as_deref().or_else(|| {
                        hc_profile
                            .as_ref()
                            .map(|profile| profile.profile_id.as_str())
                    }));
                if matches!(options.mode, RenderMode::BasicText | RenderMode::Debug) {
                    let data = self.read_ssed_stream_render_slice(&component, offset, length)?;
                    let rendered = decode_hc_stream_basic_text_with_gaiji_policy(
                        &data,
                        |code| {
                            let lookup_code = marker_profile.gaiji_lookup_code(code);
                            let resolution =
                                self.resolve_gaiji(&lookup_code, &options.gaiji_policy);
                            let resolved = resolution.unicode.is_some();
                            let text = resolution
                                .unicode
                                .clone()
                                .unwrap_or_else(|| "〓".to_owned());
                            diagnostics.extend(resolution.diagnostics);
                            Some(HcBasicTextGaiji { text, resolved })
                        },
                        |code| marker_profile.suppresses_gaiji_code(code),
                    );
                    let title = self
                        .title_for_body_target(&target)?
                        .unwrap_or_else(|| "SSED entry stream".to_owned());
                    diagnostics.extend(rendered.diagnostics);
                    if options.mode == RenderMode::Debug {
                        diagnostics.push(Diagnostic::info(
                            "hc_debug_visual_incomplete",
                            "Debug decoded the SSED stream with common HC control lengths; visual HC/profile rendering remains deferred",
                        ));
                    } else {
                        diagnostics.push(Diagnostic::info(
                            "hc_basic_text_visual_incomplete",
                            "BasicText decoded the SSED stream with common HC control lengths; visual HC/profile rendering remains separate",
                        ));
                    }
                    return Ok(ResolvedTargetView {
                        href: String::new(),
                        kind: if options.mode == RenderMode::Debug {
                            crate::render::ResolvedTargetKind::Deferred
                        } else {
                            crate::render::ResolvedTargetKind::EntryBody
                        },
                        target,
                        title: Some(title),
                        display_html: None,
                        basic_text: Some(rendered.text),
                        scroll_anchor,
                        surface: None,
                        resources: if options.mode == RenderMode::Debug {
                            resources
                        } else {
                            Vec::new()
                        },
                        links: Vec::new(),
                        capabilities: vec![crate::render::RenderCapability::HcRenderInput],
                        diagnostics,
                        debug_trace: (options.include_debug_trace
                            || options.mode == RenderMode::Debug)
                            .then(|| {
                                json!({
                                    "body": {
                                        "kind": if options.mode == RenderMode::Debug {
                                            "ssed_stream_debug_basic_text"
                                        } else {
                                            "ssed_stream_basic_text"
                                        },
                                        "component": component,
                                        "offset": offset,
                                        "length": length,
                                        "profile_hint": profile_hint,
                                        "hc_profile": hc_profile,
                                        "stats": rendered.stats,
                                    }
                                })
                                .to_string()
                            }),
                    });
                }
                let data = self.read_ssed_stream_render_slice(&component, offset, length)?;
                let mut gaiji_resources = BTreeMap::<String, ResourceRef>::new();
                let rendered = decode_hc_stream_common_html_with_gaiji_render_policy(
                    &data,
                    |code| {
                        let lookup_code = marker_profile.gaiji_lookup_code(code);
                        let resolution = self.resolve_gaiji(&lookup_code, &options.gaiji_policy);
                        let text = resolution
                            .unicode
                            .clone()
                            .unwrap_or_else(|| "〓".to_owned());
                        let html = gaiji_resource_html(&resolution, &text);
                        let resolved = resolution.unicode.is_some() || html.is_some();
                        if html.is_some()
                            && let Some(resource) = resolution.resource.clone()
                        {
                            gaiji_resources
                                .entry(resource.token.as_str().to_owned())
                                .or_insert(resource);
                        }
                        diagnostics.extend(resolution.diagnostics);
                        Some(HcCommonHtmlGaiji {
                            text,
                            html,
                            resolved,
                        })
                    },
                    |code| marker_profile.suppresses_gaiji_code(code),
                );
                let title = self
                    .title_for_body_target(&target)?
                    .unwrap_or_else(|| "SSED entry stream".to_owned());
                let links = self.hc_common_html_target_links(&rendered.links, &mut diagnostics)?;
                let html = rewrite_hc_common_html_link_hrefs(rendered.html, &links);
                let html =
                    rewrite_hc_common_html_media_placeholders(html, &rendered.media, &resources);
                let mut resources = resources;
                resources.extend(gaiji_resources.into_values());
                diagnostics.extend(rendered.diagnostics);
                diagnostics.push(Diagnostic::warning(
                    "hc_render_common_html_fallback",
                    "SSED stream was rendered through common HC HTML fallback; product visual HC/profile rendering is not implemented yet",
                ));
                Ok(ResolvedTargetView {
                    href: String::new(),
                    kind: crate::render::ResolvedTargetKind::EntryBody,
                    target,
                    title: Some(title),
                    display_html: Some(html),
                    basic_text: Some(rendered.text),
                    scroll_anchor,
                    surface: None,
                    resources,
                    links,
                    capabilities: vec![crate::render::RenderCapability::HcRenderInput],
                    diagnostics,
                    debug_trace: (options.include_debug_trace || options.mode == RenderMode::Debug)
                        .then(|| {
                            json!({
                                "body": {
                                    "kind": "ssed_stream_common_html",
                                    "component": component,
                                    "offset": offset,
                                    "length": length,
                                    "profile_hint": profile_hint,
                                    "hc_profile": hc_profile,
                                    "stats": rendered.stats,
                                }
                            })
                            .to_string()
                        }),
                })
            }
            RendererInput::SemanticFallback { target, text } => {
                let scroll_anchor = scroll_anchor_for_token(&target)?;
                Ok(ResolvedTargetView {
                    href: String::new(),
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
                    href: String::new(),
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

    fn hc_common_html_target_links(
        &self,
        links: &[crate::ssed_hc::HcCommonHtmlLink],
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Vec<TargetLink>> {
        let mut target_links = Vec::new();
        for link in links {
            let Some(token) =
                self.ssed_target_for_loose_address(link.block, link.offset, diagnostics)?
            else {
                continue;
            };
            let kind = token.decode()?.kind();
            let mut attributes = BTreeMap::new();
            attributes.insert("href".to_owned(), link.href.clone());
            attributes.insert("control".to_owned(), link.control.clone());
            let href = token.href();
            target_links.push(TargetLink {
                token,
                href,
                label: link.href.clone(),
                kind,
                diagnostics: Vec::new(),
                attributes,
            });
        }
        Ok(target_links)
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
        let internal_resource = resource_token.decode()?;
        let resource_ref = self.resolve_resource(&resource_token)?;
        let Some(mime_type) = resource_ref.mime_type.as_deref() else {
            return Ok(None);
        };
        let bytes = match self.read_resource(&resource_token) {
            Ok(bytes) => bytes,
            Err(error) => {
                if let Some(data_url) = generic_html_optional_missing_resource_data_url(
                    &internal_resource,
                    mime_type,
                    &error,
                ) {
                    return Ok(Some(data_url));
                }
                return Err(error);
            }
        };
        if bytes.len() > generic_html_inline_resource_max_bytes() {
            return Ok(None);
        }
        Ok(Some(generic_html_data_url(mime_type, &bytes)))
    }

    fn read_ssed_stream_render_slice(
        &self,
        component_name: &str,
        offset: u64,
        length: Option<u64>,
    ) -> Result<Vec<u8>> {
        const HC_BASIC_TEXT_FALLBACK_LIMIT: usize = 256 * 1024;

        let Some(component) = self.ssed_component_by_name(component_name) else {
            return Err(Error::Driver(format!(
                "{component_name} is not declared in the SSED catalog"
            )));
        };
        if let Err(diagnostic) = self.validate_plain_component(component) {
            return Err(Error::Driver(diagnostic.message));
        }
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Err(Error::Driver(format!(
                "{} was not found in the package",
                component.filename
            )));
        };
        let mut reader = SsedDataFile::open(path)?;
        let start = usize::try_from(offset)
            .map_err(|_| Error::Driver("SSED stream offset is too large".to_owned()))?;
        let available = reader.header().expanded_size().saturating_sub(start);
        let size = length
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or(HC_BASIC_TEXT_FALLBACK_LIMIT)
            .min(available)
            .min(HC_BASIC_TEXT_FALLBACK_LIMIT);
        reader.read_range(start, size)
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

fn generic_html_optional_missing_resource_data_url(
    resource: &InternalResource,
    mime_type: &str,
    error: &Error,
) -> Option<String> {
    let InternalResource::ChmFile {
        entry_path,
        resource_kind: ResourceKind::Javascript,
        ..
    } = resource
    else {
        return None;
    };
    if !entry_path
        .rsplit('/')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("font.js"))
    {
        return None;
    }
    if !error.to_string().contains("CHM entry not found") {
        return None;
    }
    Some(generic_html_data_url(mime_type, b""))
}

fn rewrite_hc_common_html_link_hrefs(mut html: String, links: &[TargetLink]) -> String {
    for link in links {
        let Some(raw_href) = link.attributes.get("href") else {
            continue;
        };
        let from = format!("href=\"{}\"", escape_html_attr_minimal(raw_href));
        let to = format!(
            "href=\"lvcore://target/{}\"",
            escape_html_attr_minimal(link.token.as_str())
        );
        html = html.replace(&from, &to);
    }
    html
}

fn rewrite_hc_common_html_media_placeholders(
    mut html: String,
    media: &[crate::ssed_hc::HcCommonHtmlMedia],
    resources: &[ResourceRef],
) -> String {
    let mut resource_index = 0usize;
    for media_control in media {
        if !hc_common_media_control_uses_resource(media_control) {
            continue;
        }
        let Some(resource) = resources.get(resource_index) else {
            break;
        };
        resource_index = resource_index.saturating_add(1);
        let from = hc_common_media_placeholder_html(media_control);
        let to = hc_common_resource_html(resource);
        html = html.replace(&from, &to);
    }
    html
}

fn hc_common_media_control_uses_resource(media: &crate::ssed_hc::HcCommonHtmlMedia) -> bool {
    matches!(
        media.control.as_str(),
        "1f3c" | "1f4a" | "1f4d" | "1f64" | "sounddata"
    )
}

fn hc_common_media_placeholder_html(media: &crate::ssed_hc::HcCommonHtmlMedia) -> String {
    let mut html = format!(
        "<span class=\"lv-hc-media-placeholder\" data-lv-control=\"{}\" data-lv-media-index=\"{}\"",
        escape_html_attr_minimal(&media.control),
        media.index
    );
    if !media.payload_hex.is_empty() {
        html.push_str(" data-lv-payload=\"");
        html.push_str(&escape_html_attr_minimal(&media.payload_hex));
        html.push('"');
    }
    html.push_str("></span>");
    html
}

fn hc_common_resource_html(resource: &ResourceRef) -> String {
    let href = resource
        .href
        .clone()
        .unwrap_or_else(|| format!("lvcore://resource/{}", resource.token.as_str()));
    let href = escape_html_attr_minimal(&href);
    let label = escape_html_attr_minimal(resource.label.as_deref().unwrap_or("Dictionary media"));
    match resource.kind {
        ResourceKind::Image | ResourceKind::Template | ResourceKind::Colscr => {
            format!("<img class=\"lv-hc-media lv-hc-media-image\" src=\"{href}\" alt=\"{label}\">")
        }
        ResourceKind::Audio | ResourceKind::PcmData | ResourceKind::SoundData => {
            format!(
                "<audio class=\"lv-hc-media lv-hc-media-audio\" controls src=\"{href}\"></audio>"
            )
        }
        ResourceKind::Video | ResourceKind::MediaBlob => {
            format!(
                "<video class=\"lv-hc-media lv-hc-media-video\" controls src=\"{href}\"></video>"
            )
        }
        _ => format!("<a class=\"lv-hc-media lv-hc-media-link\" href=\"{href}\">{label}</a>"),
    }
}

fn gaiji_resource_html(
    resolution: &crate::gaiji::GaijiResolution,
    fallback_text: &str,
) -> Option<String> {
    if !matches!(
        resolution.preferred_source,
        Some(GaijiSourcePreference::ExternalResource | GaijiSourcePreference::Ga16Bitmap)
    ) {
        return None;
    }
    let resource = resolution.resource.as_ref()?;
    let href = resource
        .href
        .clone()
        .unwrap_or_else(|| format!("lvcore://resource/{}", resource.token.as_str()));
    let class = match resolution.preferred_source {
        Some(GaijiSourcePreference::Ga16Bitmap) => "lvcore-gaiji-ga16",
        _ => "lvcore-gaiji-external",
    };
    Some(format!(
        "<img class=\"lvcore-gaiji {class} lv-hc-gaiji\" src=\"{}\" alt=\"{}\" title=\"{}\">",
        escape_html_attr_minimal(&href),
        escape_html_attr_minimal(fallback_text),
        escape_html_attr_minimal(&resolution.identity)
    ))
}

fn escape_html_attr_minimal(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
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
            InternalTarget::LvedAddress {
                block, offset, raw, ..
            } => Ok(ResolvedTargetView::unsupported(
                token.clone(),
                "LVED address link",
                Diagnostic::info(
                    "lved_address_deferred",
                    format!(
                        "LVED address link {raw} points to block {block:08x} offset {offset:04x}; no address resolver is available for this package"
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
                            href: String::new(),
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
                self.view_for_navigation_surface_target(
                    token.clone(),
                    &surface_id,
                    None,
                    None,
                    options,
                )
            }
            InternalTarget::MenuItem {
                surface_id,
                item_id,
            }
            | InternalTarget::TocItem {
                surface_id,
                item_id,
            } => {
                let cursor = (item_id != "root").then_some(item_id.as_str());
                self.view_for_navigation_surface_target(
                    token.clone(),
                    &surface_id,
                    None,
                    cursor,
                    options,
                )
            }
            InternalTarget::TitleIndexItem { surface_id, .. } => self
                .view_for_navigation_surface_target(
                    token.clone(),
                    &surface_id,
                    None,
                    None,
                    options,
                ),
            InternalTarget::MultiviewHref { href, anchor } => {
                if let Some(surface_id) = self.multiview_menu_surface_id_for_href(&href)? {
                    return self.view_for_navigation_surface_target(
                        token.clone(),
                        &surface_id,
                        Some("MultiView menu".to_owned()),
                        None,
                        options,
                    );
                }
                let _ = anchor;
                if let Some(view) =
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

fn navigation_surface_view_title(
    explicit: Option<String>,
    surface_id: &str,
    surface: &NavigationSurface,
) -> String {
    if let Some(title) = explicit.filter(|title| !is_internal_panel_title(title, surface)) {
        return title;
    }
    match surface {
        NavigationSurface::Panel { .. } => "Panels".to_owned(),
        _ => surface_id.to_owned(),
    }
}

fn is_internal_panel_title(title: &str, surface: &NavigationSurface) -> bool {
    matches!(surface, NavigationSurface::Panel { .. })
        && title.chars().all(|ch| ch.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_html_stubs_missing_observed_chm_font_js_only() {
        let error = Error::Driver("CHM entry not found: Source/font.js".to_owned());
        let font_js = InternalResource::ChmFile {
            chm_path: "HANREI.chm".to_owned(),
            entry_path: "Source/font.js".to_owned(),
            resource_kind: ResourceKind::Javascript,
        };
        let css = InternalResource::ChmFile {
            chm_path: "HANREI.chm".to_owned(),
            entry_path: "Source/css.css".to_owned(),
            resource_kind: ResourceKind::Css,
        };
        let image = InternalResource::ChmFile {
            chm_path: "HANREI.chm".to_owned(),
            entry_path: "Source/pic.png".to_owned(),
            resource_kind: ResourceKind::Image,
        };

        assert_eq!(
            generic_html_optional_missing_resource_data_url(&font_js, "text/javascript", &error)
                .as_deref(),
            Some("data:text/javascript;base64,")
        );
        assert!(
            generic_html_optional_missing_resource_data_url(&css, "text/css", &error).is_none()
        );
        assert!(
            generic_html_optional_missing_resource_data_url(&image, "image/png", &error).is_none()
        );
    }
}

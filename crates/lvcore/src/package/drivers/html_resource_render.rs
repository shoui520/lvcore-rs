use super::*;

impl ReaderBookPackage {
    pub(super) fn render_package_html_resource(
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
                href: String::new(),
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
            href: String::new(),
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

    pub(super) fn render_ssed_loose_html_resource(
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
                href: String::new(),
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
            href: String::new(),
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

    pub(super) fn render_chm_html_resource(
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
                href: String::new(),
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
            href: String::new(),
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

    pub(super) fn normalize_britannica_loose_html_refs(
        &self,
        html: &str,
    ) -> Result<NormalizedHtmlRefs> {
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

    pub(super) fn normalize_package_file_html_refs(
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
            if attr.name == HtmlAttrName::Href {
                if let Some((anchor, html_anchor)) = lved_dataid_anchor(raw_value) {
                    let target = InternalTarget::SsedDenseAnchor {
                        anchor,
                        resolver_hint: None,
                    };
                    let token = TargetToken::new(&target)?;
                    if seen_target_tokens.insert(token.as_str().to_owned()) {
                        let mut link = TargetLink::new(raw_value, &target)?;
                        if let Some(html_anchor) = html_anchor {
                            link.attributes
                                .insert("html_anchor".to_owned(), html_anchor);
                            link.diagnostics.push(Diagnostic::info(
                                "ssed_package_html_dataid_anchor_preserved_as_link_attribute",
                                "SSED dense-anchor targets do not yet carry secondary HTML scroll anchors",
                            ));
                        }
                        links.push(link);
                    }
                    output.push_str(&format!("lvcore://target/{}", token.as_str()));
                } else if let Some(address) = parse_lved_address(raw_value) {
                    if ssed_loose_address_is_package_html_ui_sentinel(address.block, address.offset)
                    {
                        output.push_str(raw_value);
                    } else if let Some(target) = self.ssed_target_for_loose_address(
                        address.block,
                        address.offset,
                        &mut diagnostics,
                    )? {
                        let decoded = target.decode()?;
                        if seen_target_tokens.insert(target.as_str().to_owned()) {
                            links.push(TargetLink::new(raw_value, &decoded)?);
                        }
                        output.push_str(&format!("lvcore://target/{}", target.as_str()));
                    } else {
                        output.push_str(raw_value);
                    }
                } else if package_html_attr_value_has_scheme(raw_value) {
                    output.push_str(raw_value);
                } else if let Some(reference) =
                    package_relative_html_reference(&base_dir, raw_value)
                {
                    let reference = self.package_file_html_reference_with_templates_fallback(
                        &base_dir, reference,
                    )?;
                    if path_has_extension(&reference.path, &["html", "htm"]) {
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
                        let resource_kind = resource_kind_from_path(&reference.path);
                        let resource = InternalResource::PackageFile {
                            resource_kind,
                            path: reference.path.clone(),
                        };
                        let token = ResourceToken::new(&resource)?;
                        let resource_ref = self.resolve_resource(&token)?;
                        if let Some(data_url) = optional_missing_package_html_data_url(
                            &reference.path,
                            resource_kind,
                            &resource_ref,
                        ) {
                            output.push_str(&data_url);
                        } else {
                            let href = format!("lvcore://resource/{}", token.as_str());
                            if seen_resource_tokens.insert(token.as_str().to_owned()) {
                                diagnostics.extend(resource_ref.diagnostics.clone());
                                resources.push(resource_ref);
                            }
                            output.push_str(&href);
                        }
                        if let Some(anchor) = reference.anchor.as_deref() {
                            output.push('#');
                            output.push_str(anchor);
                        }
                    }
                } else {
                    output.push_str(raw_value);
                }
            } else if package_html_attr_value_has_scheme(raw_value) {
                output.push_str(raw_value);
            } else if let Some(reference) = package_relative_html_reference(&base_dir, raw_value) {
                let reference =
                    self.package_file_html_reference_with_templates_fallback(&base_dir, reference)?;
                let resource_kind = resource_kind_from_path(&reference.path);
                let resource = InternalResource::PackageFile {
                    resource_kind,
                    path: reference.path.clone(),
                };
                let token = ResourceToken::new(&resource)?;
                let resource_ref = self.resolve_resource(&token)?;
                if let Some(data_url) = optional_missing_package_html_data_url(
                    &reference.path,
                    resource_kind,
                    &resource_ref,
                ) {
                    output.push_str(&data_url);
                } else {
                    let href = format!("lvcore://resource/{}", token.as_str());
                    if seen_resource_tokens.insert(token.as_str().to_owned()) {
                        diagnostics.extend(resource_ref.diagnostics.clone());
                        resources.push(resource_ref);
                    }
                    output.push_str(&href);
                }
                if let Some(anchor) = reference.anchor.as_deref() {
                    output.push('#');
                    output.push_str(anchor);
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

    fn package_file_html_reference_with_templates_fallback(
        &self,
        base_dir: &str,
        reference: PackageHtmlReference,
    ) -> Result<PackageHtmlReference> {
        if self.resolve_package_file_path(&reference.path)?.is_some()
            || !base_dir.is_empty()
            || reference.path.contains('/')
        {
            return Ok(reference);
        }
        let candidate = format!("Templates/{}", reference.path);
        if self.resolve_package_file_path(&candidate)?.is_some() {
            return Ok(PackageHtmlReference {
                path: candidate,
                anchor: reference.anchor,
            });
        }
        Ok(reference)
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
}

fn ssed_loose_address_is_package_html_ui_sentinel(block: u32, offset: u32) -> bool {
    block == 0x8000_0000 && offset == 0xffff
}

fn package_html_attr_value_has_scheme(raw_value: &str) -> bool {
    let unescaped = html_unescape_minimal(raw_value);
    let value = unescaped.trim();
    let Some(colon) = value.find(':') else {
        return false;
    };
    let first_separator = value.find(['/', '?', '#']).unwrap_or(value.len());
    if colon > first_separator {
        return false;
    }
    let Some(first) = value.as_bytes().first().copied() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && value[..colon]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn optional_missing_package_html_data_url(
    path: &str,
    resource_kind: ResourceKind,
    resource_ref: &ResourceRef,
) -> Option<String> {
    match resource_kind {
        ResourceKind::Css => {}
        ResourceKind::Javascript => {
            if !Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.eq_ignore_ascii_case("font.js") || name.eq_ignore_ascii_case("MathJax.js")
                })
            {
                return None;
            }
        }
        _ => return None,
    }
    if resource_ref.href.is_some()
        || !resource_ref
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "resource_missing")
    {
        return None;
    }
    let fallback_mime = match resource_kind {
        ResourceKind::Css => "text/css; charset=utf-8",
        ResourceKind::Javascript => "text/javascript; charset=utf-8",
        _ => return None,
    };
    let mime_type = resource_ref.mime_type.as_deref().unwrap_or(fallback_mime);
    Some(generic_html_data_url(mime_type, b""))
}

use super::*;

impl ReaderBookPackage {
    pub(super) fn normalize_lved_html_refs(&self, html: &str) -> Result<NormalizedHtmlRefs> {
        let mut output = String::with_capacity(html.len());
        let mut resources = Vec::new();
        let mut links = Vec::new();
        let mut diagnostics = Vec::new();
        let mut seen_resource_tokens = BTreeSet::new();
        let mut seen_target_tokens = BTreeSet::new();
        let mut cursor = 0usize;
        let lower = html.to_ascii_lowercase();
        while let Some((relative_start, ref_kind)) = next_lved_ref(&lower[cursor..]) {
            let start = cursor + relative_start;
            output.push_str(&html[cursor..start]);
            let end = html[start..]
                .find(is_lved_ref_terminator)
                .map(|index| start + index)
                .unwrap_or(html.len());
            let raw_ref = &html[start..end];
            match ref_kind {
                LvedHtmlRefKind::Media => {
                    if let Some(resource) = lved_media_resource(raw_ref) {
                        let token = ResourceToken::new(&resource)?;
                        let href = format!("lvcore://resource/{}", token.as_str());
                        if seen_resource_tokens.insert(token.as_str().to_owned()) {
                            let resource_ref = self.resolve_resource(&token)?;
                            diagnostics.extend(resource_ref.diagnostics.clone());
                            resources.push(resource_ref);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "lved_media_ref_unparsed",
                            format!("could not parse LVED media reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::ZipToMedia => {
                    if let Some(resource) = lved_ziptomedia_resource(raw_ref) {
                        let token = ResourceToken::new(&resource)?;
                        let href = format!("lvcore://resource/{}", token.as_str());
                        if seen_resource_tokens.insert(token.as_str().to_owned()) {
                            let resource_ref = self.resolve_resource(&token)?;
                            diagnostics.extend(resource_ref.diagnostics.clone());
                            resources.push(resource_ref);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "lved_ziptomedia_ref_unparsed",
                            format!("could not parse LVED ziptomedia reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::Image => {
                    if let Some(resource) = lved_image_resource(raw_ref) {
                        let token = ResourceToken::new(&resource)?;
                        let href = format!("lvcore://resource/{}", token.as_str());
                        if seen_resource_tokens.insert(token.as_str().to_owned()) {
                            let resource_ref = self.resolve_resource(&token)?;
                            diagnostics.extend(resource_ref.diagnostics.clone());
                            resources.push(resource_ref);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "lved_image_ref_unparsed",
                            format!("could not parse LVED image reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::ImageAddressHook => {
                    if let Some(target) = lved_image_address_viewer_hook_target(raw_ref) {
                        let token = TargetToken::new(&target)?;
                        let href = format!("lvcore://target/{}", token.as_str());
                        if seen_target_tokens.insert(token.as_str().to_owned()) {
                            let mut link = TargetLink::new(raw_ref, &target)?;
                            link.diagnostics.push(Diagnostic::info(
                                "lved_image_address_hook_deferred",
                                "LVED address-style image hook is preserved as a non-executed target",
                            ));
                            links.push(link);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "lved_image_address_hook_unparsed",
                            format!("could not parse LVED address-style image hook {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::Pdf => {
                    if let Some(resource) = lved_pdf_resource(raw_ref) {
                        let token = ResourceToken::new(&resource)?;
                        let href = format!("lvcore://resource/{}", token.as_str());
                        if seen_resource_tokens.insert(token.as_str().to_owned()) {
                            let resource_ref = self.resolve_resource(&token)?;
                            diagnostics.extend(resource_ref.diagnostics.clone());
                            resources.push(resource_ref);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "lved_pdf_ref_unparsed",
                            format!("could not parse LVED PDF reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::Address => {
                    if let Some(target) = lved_address_target(raw_ref) {
                        let token = TargetToken::new(&target)?;
                        let href = format!("lvcore://target/{}", token.as_str());
                        if seen_target_tokens.insert(token.as_str().to_owned()) {
                            let mut link = TargetLink::new(raw_ref, &target)?;
                            link.diagnostics.push(Diagnostic::info(
                                "lved_address_deferred",
                                "LVED address link is preserved as a typed target; this package did not provide an address resolver for it",
                            ));
                            links.push(link);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "lved_address_ref_unparsed",
                            format!("could not parse LVED address reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::DataId => {
                    if let Some(target) = lved_dataid_target(raw_ref) {
                        let token = TargetToken::new(&target)?;
                        let href = format!("lvcore://target/{}", token.as_str());
                        if seen_target_tokens.insert(token.as_str().to_owned()) {
                            links.push(TargetLink::new(raw_ref, &target)?);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "lved_dataid_ref_unparsed",
                            format!("could not parse LVED dataid reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::CrossBook => {
                    if let Some(target) = lved_cross_book_target(raw_ref) {
                        let token = TargetToken::new(&target)?;
                        let href = format!("lvcore://target/{}", token.as_str());
                        if seen_target_tokens.insert(token.as_str().to_owned()) {
                            let mut link = TargetLink::new(raw_ref, &target)?;
                            link.diagnostics.push(Diagnostic::info(
                                "lved_cross_book_deferred",
                                "cross-dictionary LVED link requires library-wide routing",
                            ));
                            links.push(link);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "lved_cross_book_ref_unparsed",
                            format!("could not parse cross-dictionary LVED reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::Info => {
                    if let Some(target) = lved_info_target(raw_ref) {
                        let token = TargetToken::new(&target)?;
                        let href = format!("lvcore://target/{}", token.as_str());
                        if seen_target_tokens.insert(token.as_str().to_owned()) {
                            links.push(TargetLink::new(raw_ref, &target)?);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "lved_info_ref_unparsed",
                            format!("could not parse LVED info reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::Binran => {
                    if let Some(target) = lved_binran_target(raw_ref) {
                        let token = TargetToken::new(&target)?;
                        let href = format!("lvcore://target/{}", token.as_str());
                        if seen_target_tokens.insert(token.as_str().to_owned()) {
                            links.push(TargetLink::new(raw_ref, &target)?);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "lved_binran_ref_unparsed",
                            format!("could not parse LVED binran reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::ViewerHook => {
                    let target = lved_viewer_hook_target(raw_ref);
                    let token = TargetToken::new(&target)?;
                    let href = format!("lvcore://target/{}", token.as_str());
                    if seen_target_tokens.insert(token.as_str().to_owned()) {
                        let mut link = TargetLink::new(raw_ref, &target)?;
                        link.diagnostics.push(Diagnostic::info(
                            "lved_viewer_hook_deferred",
                            "LVED viewer hook is preserved as a non-executed target",
                        ));
                        links.push(link);
                    }
                    output.push_str(&href);
                }
            }
            cursor = end;
        }
        output.push_str(&html[cursor..]);
        let html = self.normalize_lved_direct_resource_attrs(
            &output,
            &mut resources,
            &mut links,
            &mut diagnostics,
            &mut seen_resource_tokens,
            &mut seen_target_tokens,
        )?;
        Ok(NormalizedHtmlRefs {
            html,
            resources,
            links,
            diagnostics,
        })
    }

    pub(super) fn normalize_ssed_sidecar_lved_html_refs(
        &self,
        html: &str,
    ) -> Result<NormalizedHtmlRefs> {
        let mut output = String::with_capacity(html.len());
        let mut resources = Vec::new();
        let mut links = Vec::new();
        let mut diagnostics = Vec::new();
        let mut seen_resource_tokens = BTreeSet::new();
        let mut seen_target_tokens = BTreeSet::new();
        let mut cursor = 0usize;
        let lower = html.to_ascii_lowercase();
        while let Some((relative_start, ref_kind)) = next_lved_ref(&lower[cursor..]) {
            let start = cursor + relative_start;
            output.push_str(&html[cursor..start]);
            let end = html[start..]
                .find(is_lved_ref_terminator)
                .map(|index| start + index)
                .unwrap_or(html.len());
            let raw_ref = &html[start..end];
            match ref_kind {
                LvedHtmlRefKind::DataId => {
                    if let Some((anchor, html_anchor)) = lved_dataid_anchor(raw_ref) {
                        let target = InternalTarget::SsedDenseAnchor {
                            anchor,
                            resolver_hint: None,
                        };
                        let token = TargetToken::new(&target)?;
                        let href = format!("lvcore://target/{}", token.as_str());
                        if seen_target_tokens.insert(token.as_str().to_owned()) {
                            let mut link = TargetLink::new(raw_ref, &target)?;
                            if let Some(html_anchor) = html_anchor {
                                link.attributes
                                    .insert("html_anchor".to_owned(), html_anchor);
                                link.diagnostics.push(Diagnostic::info(
                                    "ssed_sidecar_dataid_anchor_preserved_as_link_attribute",
                                    "SSED dense-anchor targets do not yet carry secondary HTML scroll anchors",
                                ));
                            }
                            links.push(link);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "ssed_sidecar_lved_dataid_ref_unparsed",
                            format!("could not parse SSED sidecar LVED dataid reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::Address => {
                    if let Some(address) = parse_lved_address(raw_ref) {
                        if let Some(target) = self.ssed_target_for_loose_address(
                            address.block,
                            address.offset,
                            &mut diagnostics,
                        )? {
                            let decoded = target.decode()?;
                            if seen_target_tokens.insert(target.as_str().to_owned()) {
                                links.push(TargetLink::new(raw_ref, &decoded)?);
                            }
                            output.push_str(&format!("lvcore://target/{}", target.as_str()));
                        } else {
                            output.push_str(raw_ref);
                        }
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "ssed_sidecar_lved_address_ref_unparsed",
                            format!(
                                "could not parse SSED sidecar LVED address reference {raw_ref}"
                            ),
                        ));
                    }
                }
                LvedHtmlRefKind::Media => {
                    if let Some(resource) = lved_media_resource(raw_ref) {
                        let token = ResourceToken::new(&resource)?;
                        let href = format!("lvcore://resource/{}", token.as_str());
                        if seen_resource_tokens.insert(token.as_str().to_owned()) {
                            let resource_ref = self.resolve_resource(&token)?;
                            diagnostics.extend(resource_ref.diagnostics.clone());
                            resources.push(resource_ref);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "ssed_sidecar_lved_media_ref_unparsed",
                            format!("could not parse SSED sidecar LVED media reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::ZipToMedia => {
                    if let Some(resource) = lved_ziptomedia_resource(raw_ref) {
                        let token = ResourceToken::new(&resource)?;
                        let href = format!("lvcore://resource/{}", token.as_str());
                        if seen_resource_tokens.insert(token.as_str().to_owned()) {
                            let resource_ref = self.resolve_resource(&token)?;
                            diagnostics.extend(resource_ref.diagnostics.clone());
                            resources.push(resource_ref);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "ssed_sidecar_lved_ziptomedia_ref_unparsed",
                            format!(
                                "could not parse SSED sidecar LVED ziptomedia reference {raw_ref}"
                            ),
                        ));
                    }
                }
                LvedHtmlRefKind::Image => {
                    if let Some(resource) = lved_image_resource(raw_ref) {
                        let token = ResourceToken::new(&resource)?;
                        let href = format!("lvcore://resource/{}", token.as_str());
                        if seen_resource_tokens.insert(token.as_str().to_owned()) {
                            let resource_ref = self.resolve_resource(&token)?;
                            diagnostics.extend(resource_ref.diagnostics.clone());
                            resources.push(resource_ref);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "ssed_sidecar_lved_image_ref_unparsed",
                            format!("could not parse SSED sidecar LVED image reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::ImageAddressHook => {
                    if let Some(target) = lved_image_address_viewer_hook_target(raw_ref) {
                        let token = TargetToken::new(&target)?;
                        let href = format!("lvcore://target/{}", token.as_str());
                        if seen_target_tokens.insert(token.as_str().to_owned()) {
                            let mut link = TargetLink::new(raw_ref, &target)?;
                            link.diagnostics.push(Diagnostic::info(
                                "ssed_sidecar_lved_image_address_hook_deferred",
                                "SSED sidecar address-style LVED image hook is preserved as a non-executed target",
                            ));
                            links.push(link);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "ssed_sidecar_lved_image_address_hook_unparsed",
                            format!(
                                "could not parse SSED sidecar address-style LVED image hook {raw_ref}"
                            ),
                        ));
                    }
                }
                LvedHtmlRefKind::Pdf => {
                    if let Some(resource) = lved_pdf_resource(raw_ref) {
                        let token = ResourceToken::new(&resource)?;
                        let href = format!("lvcore://resource/{}", token.as_str());
                        if seen_resource_tokens.insert(token.as_str().to_owned()) {
                            let resource_ref = self.resolve_resource(&token)?;
                            diagnostics.extend(resource_ref.diagnostics.clone());
                            resources.push(resource_ref);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "ssed_sidecar_lved_pdf_ref_unparsed",
                            format!("could not parse SSED sidecar LVED PDF reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::CrossBook => {
                    if let Some(target) = lved_cross_book_target(raw_ref) {
                        let token = TargetToken::new(&target)?;
                        let href = format!("lvcore://target/{}", token.as_str());
                        if seen_target_tokens.insert(token.as_str().to_owned()) {
                            let mut link = TargetLink::new(raw_ref, &target)?;
                            link.diagnostics.push(Diagnostic::info(
                                "ssed_sidecar_cross_book_deferred",
                                "cross-dictionary sidecar link requires library-wide routing",
                            ));
                            links.push(link);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "ssed_sidecar_lved_cross_book_ref_unparsed",
                            format!(
                                "could not parse SSED sidecar cross-dictionary reference {raw_ref}"
                            ),
                        ));
                    }
                }
                LvedHtmlRefKind::Info => {
                    if let Some(target) = lved_info_target(raw_ref) {
                        let token = TargetToken::new(&target)?;
                        let href = format!("lvcore://target/{}", token.as_str());
                        if seen_target_tokens.insert(token.as_str().to_owned()) {
                            links.push(TargetLink::new(raw_ref, &target)?);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "ssed_sidecar_lved_info_ref_unparsed",
                            format!("could not parse SSED sidecar LVED info reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::Binran => {
                    if let Some(target) = lved_binran_target(raw_ref) {
                        let token = TargetToken::new(&target)?;
                        let href = format!("lvcore://target/{}", token.as_str());
                        if seen_target_tokens.insert(token.as_str().to_owned()) {
                            links.push(TargetLink::new(raw_ref, &target)?);
                        }
                        output.push_str(&href);
                    } else {
                        output.push_str(raw_ref);
                        diagnostics.push(Diagnostic::warning(
                            "ssed_sidecar_lved_binran_ref_unparsed",
                            format!("could not parse SSED sidecar LVED binran reference {raw_ref}"),
                        ));
                    }
                }
                LvedHtmlRefKind::ViewerHook => {
                    let target = lved_viewer_hook_target(raw_ref);
                    let token = TargetToken::new(&target)?;
                    let href = format!("lvcore://target/{}", token.as_str());
                    if seen_target_tokens.insert(token.as_str().to_owned()) {
                        let mut link = TargetLink::new(raw_ref, &target)?;
                        link.diagnostics.push(Diagnostic::info(
                            "ssed_sidecar_lved_viewer_hook_deferred",
                            "SSED sidecar LVED viewer hook is preserved as a non-executed target",
                        ));
                        links.push(link);
                    }
                    output.push_str(&href);
                }
            }
            cursor = end;
        }
        output.push_str(&html[cursor..]);
        let html = self.normalize_ssed_sidecar_direct_resource_attrs(
            &output,
            &mut resources,
            &mut links,
            &mut diagnostics,
            &mut seen_resource_tokens,
            &mut seen_target_tokens,
        )?;
        Ok(NormalizedHtmlRefs {
            html,
            resources,
            links,
            diagnostics,
        })
    }

    pub(super) fn normalize_lved_label_html(&self, html: &str) -> Result<String> {
        Ok(sanitize_rich_label_html(
            &self.normalize_lved_html_refs(html)?.html,
        ))
    }

    fn normalize_lved_direct_resource_attrs(
        &self,
        html: &str,
        resources: &mut Vec<ResourceRef>,
        links: &mut Vec<TargetLink>,
        diagnostics: &mut Vec<Diagnostic>,
        seen_resource_tokens: &mut BTreeSet<String>,
        seen_target_tokens: &mut BTreeSet<String>,
    ) -> Result<String> {
        let mut output = String::with_capacity(html.len());
        let mut cursor = 0usize;
        let lower = html.to_ascii_lowercase();
        while let Some(attr) = next_html_href_or_src_attr(html, &lower, cursor) {
            output.push_str(&html[cursor..attr.value_start]);
            let raw_value = &html[attr.value_start..attr.value_end];
            if attr.name == HtmlAttrName::Href
                && !raw_value.starts_with("lvcore://")
                && let Some(target) = lved_relative_viewer_hook_target(raw_value)
            {
                let token = TargetToken::new(&target)?;
                let href = format!("lvcore://target/{}", token.as_str());
                if seen_target_tokens.insert(token.as_str().to_owned()) {
                    let mut link = TargetLink::new(raw_value, &target)?;
                    link.diagnostics.push(Diagnostic::info(
                        "lved_relative_viewer_hook_deferred",
                        "LVED relative appendix hook is preserved as a non-executed target",
                    ));
                    links.push(link);
                }
                output.push_str(&href);
            } else if matches!(attr.name, HtmlAttrName::Src | HtmlAttrName::Data)
                && !raw_value.starts_with("lvcore://")
                && let Some(resource) = self.lved_direct_resource(raw_value)?
            {
                let token = ResourceToken::new(&resource)?;
                let href = format!("lvcore://resource/{}", token.as_str());
                if seen_resource_tokens.insert(token.as_str().to_owned()) {
                    let resource_ref = self.resolve_resource(&token)?;
                    diagnostics.extend(resource_ref.diagnostics.clone());
                    resources.push(resource_ref);
                }
                output.push_str(&href);
            } else {
                output.push_str(raw_value);
            }
            cursor = attr.value_end;
        }
        output.push_str(&html[cursor..]);
        Ok(output)
    }

    fn normalize_ssed_sidecar_direct_resource_attrs(
        &self,
        html: &str,
        resources: &mut Vec<ResourceRef>,
        links: &mut Vec<TargetLink>,
        diagnostics: &mut Vec<Diagnostic>,
        seen_resource_tokens: &mut BTreeSet<String>,
        seen_target_tokens: &mut BTreeSet<String>,
    ) -> Result<String> {
        let mut output = String::with_capacity(html.len());
        let mut cursor = 0usize;
        let lower = html.to_ascii_lowercase();
        while let Some(attr) = next_html_href_or_src_attr(html, &lower, cursor) {
            let raw_value = &html[attr.value_start..attr.value_end];
            if attr.name == HtmlAttrName::Href
                && !raw_value.starts_with("lvcore://")
                && let Some(address) = parse_lved_address(raw_value)
                && let Some(target) =
                    self.ssed_target_for_loose_address(address.block, address.offset, diagnostics)?
            {
                output.push_str(&html[cursor..attr.value_start]);
                let decoded = target.decode()?;
                if seen_target_tokens.insert(target.as_str().to_owned()) {
                    links.push(TargetLink::new(raw_value, &decoded)?);
                }
                output.push_str(&format!("lvcore://target/{}", target.as_str()));
                cursor = attr.value_end;
            } else if matches!(attr.name, HtmlAttrName::Src | HtmlAttrName::Data)
                && !raw_value.starts_with("lvcore://")
            {
                match self.ssed_sidecar_direct_resource(raw_value)? {
                    Some(resource) => {
                        output.push_str(&html[cursor..attr.value_start]);
                        let token = ResourceToken::new(&resource)?;
                        let href = format!("lvcore://resource/{}", token.as_str());
                        if seen_resource_tokens.insert(token.as_str().to_owned()) {
                            let resource_ref = self.resolve_resource(&token)?;
                            diagnostics.extend(resource_ref.diagnostics.clone());
                            resources.push(resource_ref);
                        }
                        output.push_str(&href);
                        cursor = attr.value_end;
                    }
                    None if looks_like_relative_html_resource_ref(raw_value) => {
                        if attr.name == HtmlAttrName::Src
                            && let Some(fallback) = self.ssed_sidecar_ios_gaiji_img_fallback(
                                raw_value,
                                &html[attr.tag_start..attr.tag_end],
                            )
                        {
                            output.push_str(&html[cursor..attr.tag_start]);
                            output.push_str(&fallback);
                            cursor = attr.tag_end;
                        } else {
                            output.push_str(&html[cursor..attr.value_start]);
                            output.push_str(raw_value);
                            diagnostics.push(Diagnostic::warning(
                                "ssed_sidecar_direct_resource_missing",
                                format!(
                                    "could not resolve SSED sidecar resource reference {raw_value}"
                                ),
                            ));
                            cursor = attr.value_end;
                        }
                    }
                    None => {
                        output.push_str(&html[cursor..attr.value_start]);
                        output.push_str(raw_value);
                        cursor = attr.value_end;
                    }
                }
            } else {
                output.push_str(&html[cursor..attr.value_start]);
                output.push_str(raw_value);
                cursor = attr.value_end;
            }
        }
        output.push_str(&html[cursor..]);
        Ok(output)
    }

    fn ssed_sidecar_ios_gaiji_img_fallback(
        &self,
        raw_value: &str,
        raw_tag: &str,
    ) -> Option<String> {
        let tag_name = raw_tag
            .trim_start()
            .strip_prefix('<')?
            .trim_start()
            .split_ascii_whitespace()
            .next()?
            .trim_end_matches('/')
            .to_ascii_lowercase();
        if tag_name != "img" {
            return None;
        }
        let value = html_unescape_minimal(raw_value).trim().replace('\\', "/");
        let relative = normalized_sidecar_direct_resource_ref(&value)?;
        let filename = relative.rsplit('/').next().unwrap_or(relative.as_str());
        let stem = filename
            .rsplit_once('.')
            .map(|(stem, _extension)| stem)
            .unwrap_or(filename);
        let code = normalize_gaiji_identity(stem)?;
        let text = self.gaiji_unicode_map.get(&code)?;
        let mut output = String::new();
        output.push_str(r#"<span class="lvcore-gaiji lvcore-gaiji-ios-plist" data-gaiji=""#);
        output.push_str(&escape_plain_label_html(&code));
        output.push_str(r#"">"#);
        output.push_str(&escape_plain_label_html(text));
        output.push_str("</span>");
        Some(output)
    }

    fn ssed_sidecar_direct_resource(&self, raw_value: &str) -> Result<Option<InternalResource>> {
        let value = html_unescape_minimal(raw_value).trim().replace('\\', "/");
        let Some(relative) = normalized_sidecar_direct_resource_ref(&value) else {
            return Ok(None);
        };
        let candidates = ssed_sidecar_direct_resource_candidates(&relative);
        for candidate in candidates {
            if self.resolve_package_file_path(&candidate)?.is_some() {
                return Ok(Some(InternalResource::PackageFile {
                    resource_kind: resource_kind_from_path(&candidate),
                    path: candidate,
                }));
            }
        }
        for root_name in ["img", "img_un"] {
            if resolve_loose_media_file(&self.root, root_name, &relative)?.is_some() {
                return Ok(Some(InternalResource::SsedLooseFile {
                    root_name: root_name.to_owned(),
                    path: relative.clone(),
                    resource_kind: resource_kind_from_path(&relative),
                }));
            }
        }
        for candidate in ssed_sidecar_ios_appendix_resource_candidates(&relative) {
            if self.resolve_package_file_path(&candidate)?.is_some() {
                return Ok(Some(InternalResource::PackageFile {
                    resource_kind: resource_kind_from_path(&candidate),
                    path: candidate,
                }));
            }
            if let Some((root_name, path)) = candidate.split_once('/')
                && resolve_loose_media_file(&self.root, root_name, path)?.is_some()
            {
                return Ok(Some(InternalResource::SsedLooseFile {
                    root_name: root_name.to_owned(),
                    path: path.to_owned(),
                    resource_kind: resource_kind_from_path(&candidate),
                }));
            }
        }
        if let Some(resource) = self.ssed_sidecar_media_resource_for_ref(&relative)? {
            return Ok(Some(resource));
        }
        Ok(None)
    }

    fn lved_direct_resource(&self, raw_value: &str) -> Result<Option<InternalResource>> {
        let value = html_unescape_minimal(raw_value).trim().replace('\\', "/");
        if value.is_empty()
            || value.starts_with('#')
            || value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("data:")
            || value.starts_with("file:")
            || value.starts_with("javascript:")
            || value.starts_with("lvcore://")
            || value.starts_with("lved.")
        {
            return Ok(None);
        }
        let relative = value.split(['#', '?']).next().unwrap_or("").trim();
        if relative.is_empty() {
            return Ok(None);
        }
        let lower_relative = relative.to_ascii_lowercase();
        if lower_relative == "mathjax/mathjax.js"
            || lower_relative == "./mathjax/mathjax.js"
            || lower_relative.starts_with("mathjax/")
            || lower_relative.starts_with("./mathjax/")
        {
            return Ok(None);
        }
        let candidates = [relative.to_owned(), format!("res/{relative}")];
        for candidate in candidates {
            if self.storage.exists(Path::new(&candidate))? {
                return Ok(Some(InternalResource::PackageFile {
                    resource_kind: resource_kind_from_path(&candidate),
                    path: candidate,
                }));
            }
        }
        Ok(Some(InternalResource::MediaBlob {
            store: "lved.media".to_owned(),
            key: relative.to_owned(),
            resource_kind: resource_kind_from_path(relative),
        }))
    }
}

fn ssed_sidecar_direct_resource_candidates(relative: &str) -> Vec<String> {
    let mut candidates = vec![relative.to_owned()];
    if relative.contains('/') {
        return candidates;
    }
    candidates.extend(
        [
            "OTHER/image",
            "OTHER/images",
            "OTHER/_images",
            "Templates",
            "templates",
            "HANREI/img",
            "HANREI/contents/img",
            "res",
            "resources",
            "img",
            "image",
            "images",
            "Gaijitemp",
            "gaijitemp",
            "appendix/img",
            "manual/contents/img",
            "resource/kmkimges",
        ]
        .into_iter()
        .map(|root| format!("{root}/{relative}")),
    );
    candidates
}

fn ssed_sidecar_ios_appendix_resource_candidates(relative: &str) -> Vec<String> {
    let filename = relative.rsplit('/').next().unwrap_or(relative);
    let lower = filename.to_ascii_lowercase();
    if !lower.ends_with(".jpg") && !lower.ends_with(".jpeg") {
        return Vec::new();
    }
    let stem = lower
        .strip_suffix(".jpeg")
        .or_else(|| lower.strip_suffix(".jpg"))
        .unwrap_or(lower.as_str());
    let Some((prefix, number)) = parse_ios_appendix_image_stem(stem) else {
        return Vec::new();
    };
    match prefix {
        "furoku" if number > 0 => {
            let resource_number = number - 1;
            vec![
                format!("img/Furoku{resource_number}.pdf"),
                format!("Furoku{resource_number}.pdf"),
            ]
        }
        "kanmatsu" => match number {
            1 => vec!["img/Furoku9.pdf".to_owned(), "Furoku9.pdf".to_owned()],
            2 => vec!["img/Furoku10.pdf".to_owned(), "Furoku10.pdf".to_owned()],
            3 => vec![
                "img/Furoku11.png".to_owned(),
                "Furoku11.png".to_owned(),
                "img/Furoku11.html".to_owned(),
                "Furoku11.html".to_owned(),
            ],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn parse_ios_appendix_image_stem(stem: &str) -> Option<(&'static str, u32)> {
    for prefix in ["furoku", "kanmatsu"] {
        let Some(rest) = stem.strip_prefix(prefix) else {
            continue;
        };
        let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        if digits.is_empty() || !rest[digits.len()..].starts_with('_') {
            return None;
        }
        return digits.parse().ok().map(|number| (prefix, number));
    }
    None
}

fn looks_like_relative_html_resource_ref(raw_value: &str) -> bool {
    normalized_sidecar_direct_resource_ref(
        &html_unescape_minimal(raw_value).trim().replace('\\', "/"),
    )
    .is_some()
}

fn normalized_sidecar_direct_resource_ref(value: &str) -> Option<String> {
    if value.is_empty()
        || value.starts_with('#')
        || value.starts_with('/')
        || value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("data:")
        || value.starts_with("file:")
        || value.starts_with("javascript:")
        || value.starts_with("mailto:")
        || value.starts_with("lvcore://")
        || value.starts_with("lved.")
    {
        return None;
    }
    let relative = value.split(['#', '?']).next().unwrap_or("").trim();
    if relative.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            _ => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ios_iwkoku_appendix_image_aliases_match_observed_resource_names() {
        assert_eq!(
            ssed_sidecar_ios_appendix_resource_candidates("furoku01_01.jpg"),
            vec!["img/Furoku0.pdf", "Furoku0.pdf"]
        );
        assert_eq!(
            ssed_sidecar_ios_appendix_resource_candidates("appendix/furoku09_46.jpg"),
            vec!["img/Furoku8.pdf", "Furoku8.pdf"]
        );
        assert_eq!(
            ssed_sidecar_ios_appendix_resource_candidates("kanmatsu01_02.jpg"),
            vec!["img/Furoku9.pdf", "Furoku9.pdf"]
        );
        assert_eq!(
            ssed_sidecar_ios_appendix_resource_candidates("kanmatsu03_01.jpg"),
            vec![
                "img/Furoku11.png",
                "Furoku11.png",
                "img/Furoku11.html",
                "Furoku11.html"
            ]
        );
        assert!(ssed_sidecar_ios_appendix_resource_candidates("image.png").is_empty());
    }
}

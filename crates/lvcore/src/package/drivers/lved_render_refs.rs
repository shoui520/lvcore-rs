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
            &mut diagnostics,
            &mut seen_resource_tokens,
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
        diagnostics: &mut Vec<Diagnostic>,
        seen_resource_tokens: &mut BTreeSet<String>,
    ) -> Result<String> {
        let mut output = String::with_capacity(html.len());
        let mut cursor = 0usize;
        let lower = html.to_ascii_lowercase();
        while let Some(attr) = next_html_href_or_src_attr(html, &lower, cursor) {
            output.push_str(&html[cursor..attr.value_start]);
            let raw_value = &html[attr.value_start..attr.value_end];
            if matches!(attr.name, HtmlAttrName::Src | HtmlAttrName::Data)
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

use super::*;

impl ReaderBookPackage {
    pub(super) fn normalize_multiview_html_refs(&self, html: &str) -> Result<NormalizedHtmlRefs> {
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
                if let Some(replacement) =
                    self.rewrite_multiview_href(raw_value, &mut links, &mut seen_target_tokens)?
                {
                    output.push_str(&replacement);
                } else {
                    output.push_str(raw_value);
                }
            } else if let Some(resource) = self.multiview_package_resource(raw_value)? {
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
        Ok(NormalizedHtmlRefs {
            html: output,
            resources,
            links,
            diagnostics,
        })
    }

    pub(super) fn normalize_hourei_html_refs(&self, html: &str) -> Result<NormalizedHtmlRefs> {
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
                if let Some(replacement) =
                    self.rewrite_hourei_href(raw_value, &mut links, &mut seen_target_tokens)?
                {
                    output.push_str(&replacement);
                } else {
                    output.push_str(raw_value);
                }
            } else if let Some(resource) = self.hourei_package_resource(raw_value)? {
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
        Ok(NormalizedHtmlRefs {
            html: output,
            resources,
            links,
            diagnostics,
        })
    }

    fn rewrite_hourei_href(
        &self,
        raw_value: &str,
        links: &mut Vec<TargetLink>,
        seen_target_tokens: &mut BTreeSet<String>,
    ) -> Result<Option<String>> {
        let value = html_unescape_minimal(raw_value).trim().to_owned();
        if value.is_empty()
            || value.starts_with('#')
            || value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("mailto:")
            || value.starts_with("javascript:")
        {
            return Ok(None);
        }
        if let Some(anchor) = value.strip_prefix("lved_mark&&") {
            return Ok(Some(format!("#{anchor}")));
        }
        if let Some(anchor) = value.strip_prefix("lved_ref&&") {
            return Ok(Some(format!("#{anchor}")));
        }
        if let Some(query) = value.strip_prefix("lved_ref:") {
            let query = query.trim();
            if query.is_empty() {
                return Ok(None);
            }
            let target = InternalTarget::MenuItem {
                surface_id: super::hourei_navigation::hourei_kana_surface_id(query),
                item_id: "root".to_owned(),
            };
            let token = TargetToken::new(&target)?;
            if seen_target_tokens.insert(token.as_str().to_owned()) {
                links.push(TargetLink::new(raw_value, &target)?);
            }
            return Ok(Some(format!("lvcore://target/{}", token.as_str())));
        }
        if value.eq_ignore_ascii_case("lved_unsafe") {
            return Ok(Some("#".to_owned()));
        }
        if let Some(rest) = value.strip_prefix("lved_ref&")
            && let Some((mode, body)) = rest.split_once(':')
        {
            if mode == "1" {
                let (hore_id, anchor) = body.split_once('&').unwrap_or((body, ""));
                if hore_id.chars().all(|ch| ch.is_ascii_digit()) {
                    let target = InternalTarget::HoureiLaw {
                        hore_id: hore_id.to_owned(),
                        anchor: (!anchor.is_empty()).then(|| anchor.to_owned()),
                    };
                    let token = TargetToken::new(&target)?;
                    if seen_target_tokens.insert(token.as_str().to_owned()) {
                        links.push(TargetLink::new(raw_value, &target)?);
                    }
                    return Ok(Some(format!("lvcore://target/{}", token.as_str())));
                }
            }
            if mode == "4" {
                let (primary, _) = body.split_once(':').unwrap_or((body, ""));
                if primary.chars().all(|ch| ch.is_ascii_digit()) {
                    let target = InternalTarget::HoureiLaw {
                        hore_id: primary.to_owned(),
                        anchor: None,
                    };
                    let token = TargetToken::new(&target)?;
                    if seen_target_tokens.insert(token.as_str().to_owned()) {
                        let mut link = TargetLink::new(raw_value, &target)?;
                        link.diagnostics.push(Diagnostic::info(
                            "hourei_revision_ref_partial",
                            "Hourei future/revision reference was routed to the primary law; related revision semantics are deferred",
                        ));
                        links.push(link);
                    }
                    return Ok(Some(format!("lvcore://target/{}", token.as_str())));
                }
            }
        }
        Ok(None)
    }

    fn hourei_package_resource(&self, raw_value: &str) -> Result<Option<InternalResource>> {
        let value = html_unescape_minimal(raw_value).trim().replace('\\', "/");
        if html_resource_ref_is_not_package_owned(&value) {
            return Ok(None);
        }
        let Some(store) = &self.hourei_store else {
            return Ok(None);
        };
        let Some(path) = store.resource_path_by_reference(&value)? else {
            return Ok(None);
        };
        let path = path.to_string_lossy().replace('\\', "/");
        Ok(Some(InternalResource::PackageFile {
            resource_kind: resource_kind_from_path(&path),
            path,
        }))
    }

    fn rewrite_multiview_href(
        &self,
        raw_value: &str,
        links: &mut Vec<TargetLink>,
        seen_target_tokens: &mut BTreeSet<String>,
    ) -> Result<Option<String>> {
        let value = html_unescape_minimal(raw_value).trim().to_owned();
        if value.is_empty()
            || value.starts_with('#')
            || value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("mailto:")
            || value.starts_with("javascript:")
        {
            return Ok(None);
        }
        if let Some(anchor) = value
            .strip_prefix("lved_mark:")
            .and_then(|rest| rest.split_once(':').map(|(_, anchor)| anchor))
        {
            return Ok(Some(format!("#{anchor}")));
        }
        let target_href = value
            .strip_prefix("lved_ref:")
            .and_then(|rest| rest.split_once(':').map(|(_, target)| target))
            .unwrap_or(&value);
        let target = InternalTarget::MultiviewHref {
            href: target_href.to_owned(),
            anchor: None,
        };
        let token = TargetToken::new(&target)?;
        if seen_target_tokens.insert(token.as_str().to_owned()) {
            links.push(TargetLink::new(raw_value, &target)?);
        }
        Ok(Some(format!("lvcore://target/{}", token.as_str())))
    }

    fn multiview_package_resource(&self, raw_value: &str) -> Result<Option<InternalResource>> {
        let value = html_unescape_minimal(raw_value).trim().replace('\\', "/");
        if html_resource_ref_is_not_package_owned(&value) {
            return Ok(None);
        }
        let relative = value.split(['#', '?']).next().unwrap_or("").trim();
        if relative.is_empty() {
            return Ok(None);
        }
        let candidates = [
            relative.to_owned(),
            format!("Templates/{relative}"),
            format!("Help/image/{relative}"),
            format!("Help/{relative}"),
        ];
        for candidate in candidates {
            if self.storage.exists(Path::new(&candidate))? {
                return Ok(Some(InternalResource::PackageFile {
                    resource_kind: resource_kind_from_path(&candidate),
                    path: candidate,
                }));
            }
        }
        Ok(Some(InternalResource::PackageFile {
            resource_kind: resource_kind_from_path(relative),
            path: relative.to_owned(),
        }))
    }
}

fn html_resource_ref_is_not_package_owned(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.starts_with('#') || value.starts_with('/') {
        return true;
    }
    let lower = value.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("javascript:")
        || lower.starts_with("data:")
        || lower.starts_with("file:")
        || lower.starts_with("lvcore://")
}

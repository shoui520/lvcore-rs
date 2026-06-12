use super::*;

impl ReaderBookPackage {
    fn template_gaiji_resource(&self, code: &str) -> Option<ResourceRef> {
        for extension in ["svg", "png", "gif", "jpg", "jpeg"] {
            let candidate = format!("Templates/{code}.{extension}");
            if self
                .resolve_package_file_path(&candidate)
                .ok()
                .flatten()
                .is_none()
            {
                continue;
            }
            let token = ResourceToken::new(&InternalResource::PackageFile {
                path: candidate,
                resource_kind: ResourceKind::Template,
            })
            .ok()?;
            return self.resolve_resource(&token).ok();
        }
        None
    }

    fn ga16_gaiji_resource_ref(&self, code: &str) -> Option<ResourceRef> {
        let first = code.as_bytes().first()?.to_ascii_uppercase();
        let candidates: &[&str] = match first {
            b'A' => &[
                "GA16HALF", "GAI16H", "GAI16H00", "GA16FULL", "GAI16F", "GAI16F00",
            ],
            b'B' => &[
                "GA16FULL", "GAI16F", "GAI16F00", "GA16HALF", "GAI16H", "GAI16H00",
            ],
            _ => &[
                "GA16FULL", "GAI16F", "GAI16F00", "GA16HALF", "GAI16H", "GAI16H00",
            ],
        };
        for candidate in candidates {
            let Ok(data) = self.read_package_file_bytes(candidate) else {
                continue;
            };
            if !ga16_resource_covers_code(&data, code) {
                continue;
            }
            let token = ResourceToken::new(&InternalResource::SsedGa16Glyph {
                path: (*candidate).to_owned(),
                code: code.to_owned(),
            })
            .ok()?;
            return self.resolve_resource(&token).ok();
        }
        None
    }
}

impl GaijiProvider for ReaderBookPackage {
    fn resolve_gaiji(&self, identity: &str, policy: &GaijiPolicy) -> GaijiResolution {
        let Some(code) = normalize_gaiji_identity(identity) else {
            return GaijiResolution {
                identity: identity.to_owned(),
                preferred_source: None,
                unicode: None,
                resource: None,
                nonliteral_marker: false,
                diagnostics: vec![Diagnostic::warning(
                    "gaiji_identity_invalid",
                    format!("{identity} is not a four-hex-digit LogoVista gaiji identity"),
                )],
            };
        };

        let unicode = self.gaiji_unicode_map.get(&code).cloned();
        let template_resource = self.template_gaiji_resource(&code);
        let ga16_resource = self.ga16_gaiji_resource_ref(&code);
        let formatting_helper_candidate = unicode.is_none()
            && template_resource.is_none()
            && ga16_resource.is_none()
            && is_observed_formatting_helper_gaiji_code(&code);
        let preferred_source = policy.priority.iter().copied().find(|source| match source {
            GaijiSourcePreference::Unicode => unicode.is_some(),
            GaijiSourcePreference::ExternalResource => template_resource.is_some(),
            GaijiSourcePreference::Ga16Bitmap => ga16_resource.is_some(),
            GaijiSourcePreference::Unresolved => true,
        });
        let resource = match preferred_source {
            Some(GaijiSourcePreference::ExternalResource) => template_resource,
            Some(GaijiSourcePreference::Ga16Bitmap) => ga16_resource,
            _ => template_resource.or(ga16_resource),
        };
        let diagnostics = if formatting_helper_candidate {
            vec![
                Diagnostic::info(
                    "gaiji_formatting_helper_candidate",
                    format!("{code} has no Unicode, Template, or GA16 display backing and is classified as an observed LogoVista formatting helper"),
                )
                .with_context("gaiji_space", "full"),
            ]
        } else if matches!(preferred_source, Some(GaijiSourcePreference::Unresolved)) {
            vec![Diagnostic::info(
                "gaiji_unresolved",
                format!("{code} was left unresolved by gaiji policy"),
            )]
        } else if preferred_source.is_none() {
            vec![Diagnostic::info(
                "gaiji_unresolved",
                format!("{code} was not resolved to Unicode, Template, or GA16 resource"),
            )]
        } else {
            Vec::new()
        };

        GaijiResolution {
            identity: code,
            preferred_source,
            unicode,
            resource,
            nonliteral_marker: formatting_helper_candidate,
            diagnostics,
        }
    }
}

fn is_observed_formatting_helper_gaiji_code(code: &str) -> bool {
    matches!(code, "B947" | "B948")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolve_gaiji_keeps_fallbacks_while_rendering_selected_policy_source() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("Templates")).unwrap();
        fs::write(dir.path().join("Templates").join("B123.svg"), b"<svg/>").unwrap();

        let package = ReaderBookPackage::new(
            dir.path(),
            DetectedPackage {
                root: dir.path().to_path_buf(),
                format_family: FormatFamily::Ssed,
                confidence: 80,
                title: Some("Gaiji policy".to_owned()),
                evidence: Vec::new(),
            },
            Vec::new(),
            PackageStores {
                gaiji_unicode_map: BTreeMap::from([("B123".to_owned(), "一".to_owned())]),
                ..Default::default()
            },
        );

        let unicode_first = GaijiPolicy {
            priority: vec![
                GaijiSourcePreference::Unicode,
                GaijiSourcePreference::ExternalResource,
                GaijiSourcePreference::Unresolved,
            ],
        };
        let unicode_resolution = package.resolve_gaiji("B123", &unicode_first);
        assert_eq!(
            unicode_resolution.preferred_source,
            Some(GaijiSourcePreference::Unicode)
        );
        assert_eq!(unicode_resolution.unicode.as_deref(), Some("一"));
        assert_eq!(
            unicode_resolution
                .resource
                .as_ref()
                .map(|resource| resource.kind),
            Some(ResourceKind::Template)
        );

        let external_first = GaijiPolicy {
            priority: vec![
                GaijiSourcePreference::ExternalResource,
                GaijiSourcePreference::Unicode,
                GaijiSourcePreference::Unresolved,
            ],
        };
        let external_resolution = package.resolve_gaiji("B123", &external_first);
        assert_eq!(
            external_resolution.preferred_source,
            Some(GaijiSourcePreference::ExternalResource)
        );
        assert_eq!(external_resolution.unicode.as_deref(), Some("一"));
        assert_eq!(
            external_resolution
                .resource
                .as_ref()
                .map(|resource| resource.kind),
            Some(ResourceKind::Template)
        );

        let unresolved_first = GaijiPolicy {
            priority: vec![
                GaijiSourcePreference::Unresolved,
                GaijiSourcePreference::Unicode,
                GaijiSourcePreference::ExternalResource,
            ],
        };
        let unresolved_resolution = package.resolve_gaiji("B123", &unresolved_first);
        assert_eq!(
            unresolved_resolution.preferred_source,
            Some(GaijiSourcePreference::Unresolved)
        );
        assert_eq!(unresolved_resolution.unicode.as_deref(), Some("一"));
        assert_eq!(
            unresolved_resolution
                .resource
                .as_ref()
                .map(|resource| resource.kind),
            Some(ResourceKind::Template)
        );
        assert!(
            unresolved_resolution
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "gaiji_unresolved")
        );

        let label = resolve_rich_label(&package, "A<zB123>B", &unresolved_first);
        assert_eq!(label.text, "A〓B");
        assert_eq!(
            label.html,
            r#"A<span class="lvcore-gaiji-unresolved" data-gaiji="B123">〓</span>B"#
        );
    }
}

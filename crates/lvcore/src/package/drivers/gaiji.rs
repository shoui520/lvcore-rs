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
            && is_full_width_gaiji_code(&code);
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
                    format!("{code} has no Unicode, Template, or GA16 display backing and is classified as a probable LogoVista formatting helper"),
                )
                .with_context("gaiji_space", "full"),
            ]
        } else if matches!(
            preferred_source,
            None | Some(GaijiSourcePreference::Unresolved)
        ) {
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

fn is_full_width_gaiji_code(code: &str) -> bool {
    u16::from_str_radix(code, 16).is_ok_and(|value| value >= 0xB000)
}

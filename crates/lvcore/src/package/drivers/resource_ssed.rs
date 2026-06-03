use super::*;

impl ReaderBookPackage {
    pub(super) fn ssed_sidecar_media_resolvers(&self) -> Result<&[SsedSidecarMediaResolver]> {
        let resolvers = self.ssed_sidecar_media_resolvers.get_or_init(|| {
            discover_ssed_sidecar_media_resolvers(
                &self.root,
                inferred_folder_title(&self.root).as_deref(),
            )
            .map_err(|error| error.to_string())
        });
        match resolvers {
            Ok(resolvers) => Ok(resolvers.as_slice()),
            Err(error) => Err(Error::Driver(error.clone())),
        }
    }

    pub(super) fn ssed_sidecar_media_resource_for_ref(
        &self,
        media_ref: &str,
    ) -> Result<Option<InternalResource>> {
        let Some(media) =
            lookup_ssed_sidecar_media(self.ssed_sidecar_media_resolvers()?, None, None, media_ref)?
        else {
            return Ok(None);
        };
        let sidecar = media
            .resolver
            .path
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default();
        if sidecar.is_empty() {
            return Ok(None);
        }
        Ok(Some(InternalResource::SsedSidecarMedia {
            sidecar,
            table: media.resolver.table,
            name: media.name,
            label: media_ref.to_owned(),
            resource_kind: resource_kind_from_path(media_ref),
        }))
    }

    pub(super) fn resolve_ssed_sidecar_media_resource(
        &self,
        token: &ResourceToken,
        sidecar: &str,
        table: &str,
        name: &str,
        label: &str,
        resource_kind: ResourceKind,
    ) -> Result<ResourceRef> {
        let media = lookup_ssed_sidecar_media(
            self.ssed_sidecar_media_resolvers()?,
            Some(sidecar),
            Some(table),
            name,
        )?;
        let mut diagnostics = Vec::new();
        let href = if media.is_some() {
            Some(format!("lvcore://resource/{}", token.as_str()))
        } else {
            diagnostics.push(Diagnostic::warning(
                "resource_missing",
                format!("{sidecar}:{table}:{name} was not found in SSED sidecar media"),
            ));
            None
        };
        Ok(ResourceRef {
            token: token.clone(),
            kind: resource_kind,
            label: Some(label.to_owned()),
            href,
            mime_type: resource_mime_type(resource_kind, Some(label)).map(str::to_owned),
            diagnostics,
        })
    }

    pub(super) fn read_ssed_sidecar_media_resource(
        &self,
        sidecar: &str,
        table: &str,
        name: &str,
    ) -> Result<Vec<u8>> {
        let Some(media) = lookup_ssed_sidecar_media(
            self.ssed_sidecar_media_resolvers()?,
            Some(sidecar),
            Some(table),
            name,
        )?
        else {
            return Err(Error::Driver(format!(
                "SSED sidecar media resource not found: {sidecar}:{table}:{name}"
            )));
        };
        Ok(media.data)
    }

    pub(super) fn resolve_ssed_loose_file_resource(
        &self,
        token: &ResourceToken,
        root_name: &str,
        path: &str,
        resource_kind: ResourceKind,
    ) -> Result<ResourceRef> {
        let resolved = resolve_loose_media_file(&self.root, root_name, path)?;
        let mut diagnostics = Vec::new();
        let href = if resolved.is_some() {
            Some(format!("lvcore://resource/{}", token.as_str()))
        } else {
            diagnostics.push(Diagnostic::warning(
                "resource_missing",
                format!("{root_name}/{path} was not found next to the SSED package"),
            ));
            None
        };
        Ok(ResourceRef {
            token: token.clone(),
            kind: resource_kind,
            label: Some(path.to_owned()),
            href,
            mime_type: resource_mime_type(resource_kind, Some(path)).map(str::to_owned),
            diagnostics,
        })
    }

    pub(super) fn read_ssed_loose_file_resource(
        &self,
        root_name: &str,
        path: &str,
    ) -> Result<Vec<u8>> {
        let Some(resolved) = resolve_loose_media_file(&self.root, root_name, path)? else {
            return Err(Error::Driver(format!(
                "loose SSED resource not found: {root_name}/{path}"
            )));
        };
        read_path_inside_loose_root(&self.root, root_name, &resolved)
    }

    pub(super) fn resolve_ssed_component_address_resource(
        &self,
        token: &ResourceToken,
        component: &str,
        block: u32,
        offset: u32,
        resource_kind: ResourceKind,
    ) -> Result<ResourceRef> {
        if resource_kind == ResourceKind::PcmData
            && let Some(record) = resolve_pcmu_record(&self.root, block)?
        {
            return Ok(ResourceRef {
                token: token.clone(),
                kind: resource_kind,
                label: Some(format!("_PCM_U/{}", record.stem)),
                href: Some(format!("lvcore://resource/{}", token.as_str())),
                mime_type: Some("audio/mpeg".to_owned()),
                diagnostics: Vec::new(),
            });
        }
        let resolved = self
            .ssed_component_by_name(component)
            .and_then(|component| self.resolve_readable_ssed_component_path(component).ok())
            .flatten();
        let mut diagnostics = Vec::new();
        let href = if resolved.is_some() {
            Some(format!("lvcore://resource/{}", token.as_str()))
        } else {
            diagnostics.push(Diagnostic::warning(
                "resource_missing",
                format!("{component} was not found in the package"),
            ));
            None
        };
        Ok(ResourceRef {
            token: token.clone(),
            kind: resource_kind,
            label: Some(format!("{component}:{block:08}:{offset:04}")),
            href,
            mime_type: resource_mime_type(resource_kind, Some(component)).map(str::to_owned),
            diagnostics,
        })
    }

    pub(super) fn read_ssed_component_address_resource(
        &self,
        component: &str,
        block: u32,
        offset: u32,
        resource_kind: ResourceKind,
    ) -> Result<Vec<u8>> {
        if resource_kind == ResourceKind::Colscr {
            return self.read_ssed_colscr_image(component, block, offset);
        }
        if resource_kind == ResourceKind::Image && self.is_ssed_monoscr_component(component) {
            return self.read_ssed_monoscr_png(component, block, offset);
        }
        if resource_kind == ResourceKind::PcmData {
            if let Some(bytes) = read_pcmu_record(&self.root, block)? {
                return Ok(bytes);
            }
            return Err(Error::Driver(format!(
                "_PCM_U audio for PCMDATA.DIC block {block} was not found"
            )));
        }
        Err(Error::Driver(format!(
            "SSED component-address resources are not readable for {resource_kind:?}"
        )))
    }

    pub(super) fn resolve_ssed_figure_resource(
        &self,
        token: &ResourceToken,
        component: &str,
        block: u32,
        offset: u32,
        width: u32,
        height: u32,
    ) -> Result<ResourceRef> {
        let resolved = self
            .ssed_component_by_name(component)
            .and_then(|component| self.resolve_readable_ssed_component_path(component).ok())
            .flatten();
        let mut diagnostics = Vec::new();
        let href = if resolved.is_some() && FigureDimensions::new(width, height).is_ok() {
            Some(format!("lvcore://resource/{}", token.as_str()))
        } else {
            diagnostics.push(Diagnostic::warning(
                "resource_missing",
                format!("{component} figure resource was not found or has invalid dimensions"),
            ));
            None
        };
        Ok(ResourceRef {
            token: token.clone(),
            kind: ResourceKind::Image,
            label: Some(format!(
                "{component}:{block:08}:{offset:04}:{width}x{height}"
            )),
            href,
            mime_type: Some("image/png".to_owned()),
            diagnostics,
        })
    }

    pub(super) fn resolve_ssed_ga16_glyph_resource(
        &self,
        token: &ResourceToken,
        path: &str,
        code: &str,
    ) -> Result<ResourceRef> {
        let mut diagnostics = Vec::new();
        let href = if self.resolve_package_file_path(path)?.is_some() {
            match self.read_package_file_bytes(path) {
                Ok(data) if ga16_resource_covers_code(&data, code) => {
                    Some(format!("lvcore://resource/{}", token.as_str()))
                }
                Ok(_) => {
                    diagnostics.push(Diagnostic::warning(
                        "ga16_glyph_missing",
                        format!("{path} does not contain GA16 glyph {code}"),
                    ));
                    None
                }
                Err(err) => {
                    diagnostics.push(Diagnostic::warning(
                        "resource_missing",
                        format!("{path} could not be read: {err}"),
                    ));
                    None
                }
            }
        } else {
            diagnostics.push(Diagnostic::warning(
                "resource_missing",
                format!("{path} was not found in the package"),
            ));
            None
        };
        Ok(ResourceRef {
            token: token.clone(),
            kind: ResourceKind::Image,
            label: Some(format!("{path}:{code}")),
            href,
            mime_type: Some("image/png".to_owned()),
            diagnostics,
        })
    }

    pub(super) fn read_ssed_ga16_glyph_resource(&self, path: &str, code: &str) -> Result<Vec<u8>> {
        if self.resolve_package_file_path(path)?.is_none() {
            return Err(Error::Driver(format!("GA16 resource not found: {path}")));
        }
        let data = self.read_package_file_bytes(path)?;
        ga16_glyph_png(&data, code)
    }

    pub(super) fn resolve_ssed_pcmdata_range_resource(
        &self,
        token: &ResourceToken,
        component: &str,
        start_block: u32,
        start_offset: u32,
        end_block: u32,
        end_offset: u32,
    ) -> Result<ResourceRef> {
        if let Some(record) = resolve_pcmu_record(&self.root, start_block)? {
            return Ok(ResourceRef {
                token: token.clone(),
                kind: ResourceKind::PcmData,
                label: Some(format!("_PCM_U/{}", record.stem)),
                href: Some(format!("lvcore://resource/{}", token.as_str())),
                mime_type: Some("audio/mpeg".to_owned()),
                diagnostics: Vec::new(),
            });
        }
        let resolved = self
            .ssed_component_by_name(component)
            .and_then(|component| self.resolve_readable_ssed_component_path(component).ok())
            .flatten();
        let mut diagnostics = Vec::new();
        let href = if resolved.is_some() {
            Some(format!("lvcore://resource/{}", token.as_str()))
        } else {
            diagnostics.push(Diagnostic::warning(
                "resource_missing",
                format!("{component} was not found in the package"),
            ));
            None
        };
        let mime_type = if href.is_some() {
            match self.ssed_pcmdata_range_summary(
                component,
                start_block,
                start_offset,
                end_block,
                end_offset,
            ) {
                Ok(summary) => Some(summary.media_kind.mime_type().to_owned()),
                Err(err) => {
                    diagnostics.push(Diagnostic::warning(
                        "resource_decode_deferred",
                        err.to_string(),
                    ));
                    None
                }
            }
        } else {
            None
        };
        Ok(ResourceRef {
            token: token.clone(),
            kind: ResourceKind::PcmData,
            label: Some(format!(
                "{component}:{start_block:08}:{start_offset:04}-{end_block:08}:{end_offset:04}"
            )),
            href,
            mime_type,
            diagnostics,
        })
    }

    pub(super) fn read_ssed_pcmdata_range_resource(
        &self,
        component: &str,
        start_block: u32,
        start_offset: u32,
        end_block: u32,
        end_offset: u32,
    ) -> Result<Vec<u8>> {
        if let Some(bytes) = read_pcmu_record(&self.root, start_block)? {
            return Ok(bytes);
        }
        self.read_ssed_pcmdata_range(component, start_block, start_offset, end_block, end_offset)
    }

    pub(super) fn resolve_loose_movie_resource(
        &self,
        token: &ResourceToken,
        movie_id: &str,
    ) -> Result<ResourceRef> {
        let resolved = find_movie_file(&self.root, movie_id)?;
        let mut diagnostics = Vec::new();
        let href = if resolved.is_some() {
            Some(format!("lvcore://resource/{}", token.as_str()))
        } else {
            diagnostics.push(Diagnostic::warning(
                "resource_missing",
                format!("_MOVIE file {movie_id} was not found in the package"),
            ));
            None
        };
        Ok(ResourceRef {
            token: token.clone(),
            kind: ResourceKind::Video,
            label: Some(movie_id.to_owned()),
            href,
            mime_type: Some("video/mpeg".to_owned()),
            diagnostics,
        })
    }

    pub(super) fn read_loose_movie_resource(&self, movie_id: &str) -> Result<Vec<u8>> {
        let Some(path) = find_movie_file(&self.root, movie_id)? else {
            return Err(Error::Driver(format!("_MOVIE file not found: {movie_id}")));
        };
        read_path_inside_resolved_parent(&path, "_MOVIE")
    }

    pub(super) fn resolve_ssed_pdfspread_resource(
        &self,
        token: &ResourceToken,
        page_id: &str,
    ) -> Result<ResourceRef> {
        let lookup = self.lookup_pdfspread_page(page_id)?;
        let mut diagnostics = Vec::new();
        let href = if lookup.is_some() {
            Some(format!("lvcore://resource/{}", token.as_str()))
        } else {
            diagnostics.push(Diagnostic::warning(
                "resource_missing",
                format!("PDFSpread page {page_id} was not found in the package"),
            ));
            None
        };
        Ok(ResourceRef {
            token: token.clone(),
            kind: ResourceKind::Pdf,
            label: Some(format!("PDFSpread/{page_id}")),
            href,
            mime_type: Some("application/pdf".to_owned()),
            diagnostics,
        })
    }

    pub(super) fn read_ssed_pdfspread_resource(&self, page_id: &str) -> Result<Vec<u8>> {
        let Some(lookup) = self.lookup_pdfspread_page(page_id)? else {
            return Err(Error::Driver(format!(
                "PDFSpread page not found: {page_id}"
            )));
        };
        Ok(lookup.pdf)
    }

    pub(super) fn resolve_sounddata_resource(
        &self,
        token: &ResourceToken,
        sound_id: u32,
    ) -> Result<ResourceRef> {
        let resolved = self
            .ssed_sounddata_index()?
            .and_then(|index| index.record(sound_id));
        let mut diagnostics = Vec::new();
        let href = if resolved.is_some() {
            Some(format!("lvcore://resource/{}", token.as_str()))
        } else {
            diagnostics.push(Diagnostic::warning(
                "resource_missing",
                format!("SoundData record {sound_id:08x} was not found in the package"),
            ));
            None
        };
        Ok(ResourceRef {
            token: token.clone(),
            kind: ResourceKind::SoundData,
            label: Some(format!("SoundData/{sound_id:08x}")),
            href,
            mime_type: Some("audio/wav".to_owned()),
            diagnostics,
        })
    }

    pub(super) fn read_sounddata_resource(&self, sound_id: u32) -> Result<Vec<u8>> {
        let Some(index) = self.ssed_sounddata_index()? else {
            return Err(Error::Driver("SoundData index not found".to_owned()));
        };
        let Some(bytes) = index.read_record(sound_id)? else {
            return Err(Error::Driver(format!(
                "SoundData record not found: {sound_id:08x}"
            )));
        };
        Ok(bytes)
    }
}

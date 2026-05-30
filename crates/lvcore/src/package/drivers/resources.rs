use super::*;

impl ResourceProvider for ReaderBookPackage {
    fn resolve_resource(&self, token: &ResourceToken) -> Result<ResourceRef> {
        match token.decode()? {
            InternalResource::PackageFile {
                path,
                resource_kind,
            } => {
                let relative = Path::new(&path);
                let resolved = self.resolve_package_file_path(&path)?;
                let mut diagnostics = Vec::new();
                let href = if resolved.is_some() {
                    Some(format!("lvcore://resource/{}", token.as_str()))
                } else {
                    diagnostics.push(Diagnostic::warning(
                        "resource_missing",
                        format!("{path} was not found in the package"),
                    ));
                    None
                };
                let label = resolved
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .or_else(|| relative.file_name())
                    .map(|value| value.to_string_lossy().to_string());
                Ok(ResourceRef {
                    token: token.clone(),
                    kind: resource_kind,
                    label,
                    href,
                    mime_type: resource_mime_type(resource_kind, Some(&path)).map(str::to_owned),
                    diagnostics,
                })
            }
            InternalResource::SsedLooseFile {
                root_name,
                path,
                resource_kind,
            } => {
                let resolved = resolve_loose_media_file(&self.root, &root_name, &path)?;
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
                    label: Some(path.clone()),
                    href,
                    mime_type: resource_mime_type(resource_kind, Some(&path)).map(str::to_owned),
                    diagnostics,
                })
            }
            InternalResource::SsedComponentAddress {
                component,
                block,
                offset,
                resource_kind,
            } => {
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
                    .ssed_component_by_name(&component)
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
                    mime_type: resource_mime_type(resource_kind, Some(&component))
                        .map(str::to_owned),
                    diagnostics,
                })
            }
            InternalResource::SsedFigure {
                component,
                block,
                offset,
                width,
                height,
            } => {
                let resolved = self
                    .ssed_component_by_name(&component)
                    .and_then(|component| self.resolve_readable_ssed_component_path(component).ok())
                    .flatten();
                let mut diagnostics = Vec::new();
                let href = if resolved.is_some() && FigureDimensions::new(width, height).is_ok() {
                    Some(format!("lvcore://resource/{}", token.as_str()))
                } else {
                    diagnostics.push(Diagnostic::warning(
                        "resource_missing",
                        format!(
                            "{component} figure resource was not found or has invalid dimensions"
                        ),
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
            InternalResource::SsedGa16Glyph { path, code } => {
                let mut diagnostics = Vec::new();
                let href = if self.resolve_package_file_path(&path)?.is_some() {
                    match self.read_package_file_bytes(&path) {
                        Ok(data) if ga16_resource_covers_code(&data, &code) => {
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
            InternalResource::SsedPcmDataRange {
                component,
                start_block,
                start_offset,
                end_block,
                end_offset,
            } => {
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
                    .ssed_component_by_name(&component)
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
                        &component,
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
            InternalResource::LooseMovie { movie_id } => {
                let resolved = find_movie_file(&self.root, &movie_id)?;
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
                    label: Some(movie_id),
                    href,
                    mime_type: Some("video/mpeg".to_owned()),
                    diagnostics,
                })
            }
            InternalResource::SsedPdfSpread { page_id } => {
                let lookup = self.lookup_pdfspread_page(&page_id)?;
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
            InternalResource::SoundData { sound_id } => {
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
            InternalResource::ChmFile {
                chm_path,
                entry_path,
                resource_kind,
            } => {
                let chm_relative = Path::new(&chm_path);
                let resolved = self.storage.resolve_casefolded(chm_relative)?;
                let mut diagnostics = Vec::new();
                let href = if resolved.is_some() {
                    Some(format!("lvcore://resource/{}", token.as_str()))
                } else {
                    diagnostics.push(Diagnostic::warning(
                        "resource_missing",
                        format!("{chm_path} was not found in the package"),
                    ));
                    None
                };
                let label = Path::new(&entry_path)
                    .file_name()
                    .map(|value| value.to_string_lossy().to_string())
                    .or_else(|| Some(entry_path.clone()));
                Ok(ResourceRef {
                    token: token.clone(),
                    kind: resource_kind,
                    label,
                    href,
                    mime_type: resource_mime_type(resource_kind, Some(&entry_path))
                        .map(str::to_owned),
                    diagnostics,
                })
            }
            InternalResource::MediaBlob {
                key, resource_kind, ..
            } => Ok(ResourceRef {
                token: token.clone(),
                kind: resource_kind,
                label: Some(key.clone()),
                href: self
                    .lved_store
                    .is_some()
                    .then(|| format!("lvcore://resource/{}", token.as_str())),
                mime_type: resource_mime_type(resource_kind, Some(&key)).map(str::to_owned),
                diagnostics: if self.lved_store.is_some() {
                    Vec::new()
                } else {
                    vec![Diagnostic::info(
                        "resource_deferred",
                        "media blob resource resolution is not implemented yet for this package",
                    )]
                },
            }),
            InternalResource::Unsupported { reason } => Ok(ResourceRef {
                token: token.clone(),
                kind: ResourceKind::Other,
                label: None,
                href: None,
                mime_type: None,
                diagnostics: vec![Diagnostic::warning("resource_unsupported", reason)],
            }),
        }
    }

    fn read_resource(&self, token: &ResourceToken) -> Result<Vec<u8>> {
        match token.decode()? {
            InternalResource::PackageFile { path, .. } => self.read_package_file_bytes(&path),
            InternalResource::SsedLooseFile {
                root_name, path, ..
            } => {
                let Some(resolved) = resolve_loose_media_file(&self.root, &root_name, &path)?
                else {
                    return Err(Error::Driver(format!(
                        "loose SSED resource not found: {root_name}/{path}"
                    )));
                };
                read_path_inside_loose_root(&self.root, &root_name, &resolved)
            }
            InternalResource::SsedComponentAddress {
                component,
                block,
                offset,
                resource_kind,
            } => {
                if resource_kind == ResourceKind::Colscr {
                    return self.read_ssed_colscr_image(&component, block, offset);
                }
                if resource_kind == ResourceKind::Image
                    && self.is_ssed_monoscr_component(&component)
                {
                    return self.read_ssed_monoscr_png(&component, block, offset);
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
            InternalResource::SsedFigure {
                component,
                block,
                offset,
                width,
                height,
            } => self.read_ssed_figure_resource(&component, block, offset, width, height),
            InternalResource::SsedGa16Glyph { path, code } => {
                if self.resolve_package_file_path(&path)?.is_none() {
                    return Err(Error::Driver(format!("GA16 resource not found: {path}")));
                }
                let data = self.read_package_file_bytes(&path)?;
                ga16_glyph_png(&data, &code)
            }
            InternalResource::SsedPcmDataRange {
                component,
                start_block,
                start_offset,
                end_block,
                end_offset,
            } => {
                if let Some(bytes) = read_pcmu_record(&self.root, start_block)? {
                    return Ok(bytes);
                }
                self.read_ssed_pcmdata_range(
                    &component,
                    start_block,
                    start_offset,
                    end_block,
                    end_offset,
                )
            }
            InternalResource::LooseMovie { movie_id } => {
                let Some(path) = find_movie_file(&self.root, &movie_id)? else {
                    return Err(Error::Driver(format!("_MOVIE file not found: {movie_id}")));
                };
                read_path_inside_resolved_parent(&path, "_MOVIE")
            }
            InternalResource::SsedPdfSpread { page_id } => {
                let Some(lookup) = self.lookup_pdfspread_page(&page_id)? else {
                    return Err(Error::Driver(format!(
                        "PDFSpread page not found: {page_id}"
                    )));
                };
                Ok(lookup.pdf)
            }
            InternalResource::SoundData { sound_id } => {
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
            InternalResource::ChmFile {
                chm_path,
                entry_path,
                ..
            } => {
                let relative = Path::new(&chm_path);
                let Some(resolved) = self.storage.resolve_casefolded(relative)? else {
                    return Err(Error::Driver(format!("resource not found: {chm_path}")));
                };
                if !path_stays_inside_root(&self.root, &resolved)? {
                    return Err(Error::Driver(format!(
                        "CHM resource path is outside the package: {chm_path}"
                    )));
                }
                read_chm_entry(&resolved, &entry_path)
            }
            InternalResource::MediaBlob { store, key, .. } => {
                let Some(lved_store) = &self.lved_store else {
                    return Err(Error::Driver(
                        "media blob resource reading is not implemented yet for this package"
                            .to_owned(),
                    ));
                };
                let Some(bytes) = lved_store.media_blob(&store, &key)? else {
                    return Err(Error::Driver(format!(
                        "media blob not found: {store}:{key}"
                    )));
                };
                Ok(bytes)
            }
            InternalResource::Unsupported { reason } => Err(Error::Driver(reason)),
        }
    }
}

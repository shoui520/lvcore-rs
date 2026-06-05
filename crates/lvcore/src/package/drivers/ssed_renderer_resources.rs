use super::*;

impl ReaderBookPackage {
    pub(super) fn ssed_stream_renderer_resources(
        &self,
        component_name: &str,
        offset: u64,
        length: Option<u64>,
    ) -> Result<(Vec<ResourceRef>, Vec<Diagnostic>)> {
        const RESOURCE_SCAN_LIMIT: usize = 256 * 1024;
        const UNKNOWN_LENGTH_ANCHOR_SCAN_LIMIT: usize = 128;

        let Some(component) = self.ssed_component_by_name(component_name) else {
            return Ok((
                Vec::new(),
                vec![Diagnostic::warning(
                    "ssed_renderer_resource_scan_skipped",
                    format!("{component_name} is not declared in the SSED catalog"),
                )],
            ));
        };
        if let Err(diagnostic) = self.validate_plain_component(component) {
            return Ok((Vec::new(), vec![diagnostic]));
        }
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Ok((
                Vec::new(),
                vec![Diagnostic::warning(
                    "ssed_renderer_resource_scan_skipped",
                    format!("{} was not found in the package", component.filename),
                )],
            ));
        };

        let mut reader = SsedDataFile::open(path)?;
        let start = usize::try_from(offset)
            .map_err(|_| Error::Driver("SSED stream offset is too large".to_owned()))?;
        let available = reader.header().expanded_size().saturating_sub(start);
        let Some(length) = length else {
            let size = available.min(UNKNOWN_LENGTH_ANCHOR_SCAN_LIMIT);
            let data = reader.read_range(start, size)?;
            let mut candidates = Vec::new();
            if let Some(pdfspread) = self.ssed_pdfspread_resource_candidate(&data)? {
                candidates.push(pdfspread);
            }
            let (resources, mut diagnostics) =
                self.resolve_ssed_renderer_resource_candidates(candidates);
            diagnostics.push(Diagnostic::info(
                "ssed_renderer_resource_scan_deferred",
                "SSED stream length is unknown; broad media resource extraction is deferred to the HC renderer",
            ));
            return Ok((resources, diagnostics));
        };
        let explicit_length = usize::try_from(length).ok();
        let requested = explicit_length.unwrap_or(RESOURCE_SCAN_LIMIT);
        let size = available.min(requested).min(RESOURCE_SCAN_LIMIT);
        let data = reader.read_range(start, size)?;
        let mut candidates = self.ssed_renderer_resource_candidates(&data);
        if let Some(pdfspread) = self.ssed_pdfspread_resource_candidate(&data)? {
            candidates.push(pdfspread);
        }
        let (resources, mut diagnostics) =
            self.resolve_ssed_renderer_resource_candidates(candidates);
        if explicit_length.is_none() && available > size {
            diagnostics.push(Diagnostic::info(
                "ssed_renderer_resource_scan_bounded",
                format!(
                    "scanned {size} of {available} available SSED stream bytes for media resources"
                ),
            ));
        }
        Ok((resources, diagnostics))
    }

    fn resolve_ssed_renderer_resource_candidates(
        &self,
        candidates: Vec<InternalResource>,
    ) -> (Vec<ResourceRef>, Vec<Diagnostic>) {
        let mut seen = BTreeSet::new();
        let mut resources = Vec::new();
        let mut diagnostics = Vec::new();

        for resource in candidates {
            let Ok(token) = ResourceToken::new(&resource) else {
                diagnostics.push(Diagnostic::warning(
                    "ssed_renderer_resource_unresolved",
                    "resource candidate could not be tokenized",
                ));
                continue;
            };
            if !seen.insert(token.as_str().to_owned()) {
                continue;
            }
            match self.resolve_resource(&token) {
                Ok(resource_ref) => resources.push(resource_ref),
                Err(err) => diagnostics.push(Diagnostic::warning(
                    "ssed_renderer_resource_unresolved",
                    err.to_string(),
                )),
            }
        }
        (resources, diagnostics)
    }

    pub(super) fn ssed_renderer_resource_candidates(&self, data: &[u8]) -> Vec<InternalResource> {
        let mut candidates = Vec::new();
        let mut latest_figure_descriptor: Option<Vec<u8>> = None;
        let mut pos = 0usize;
        while pos + 2 <= data.len() {
            if let Some(reference) = parse_sounddata_marker_at(data, pos) {
                candidates.push(InternalResource::SoundData {
                    sound_id: reference.sound_id,
                });
                pos = reference.end;
                continue;
            }
            if data[pos] != 0x1f {
                pos += 1;
                continue;
            }
            let op = data[pos + 1];
            let arg_len = ssed_control_arg_length(data, pos);
            let payload = data.get(pos + 2..pos + 2 + arg_len).unwrap_or(&[]);
            match op {
                0x3c | 0x4d if payload.len() == 18 => {
                    if let Some((block, offset)) = parse_colscr_pointer(payload)
                        && let Some(component) = self.ssed_component_for_role_or_name(
                            SsedComponentRole::Colscr,
                            "COLSCR.DIC",
                        )
                    {
                        candidates.push(InternalResource::SsedComponentAddress {
                            component: component.filename.clone(),
                            block,
                            offset,
                            resource_kind: ResourceKind::Colscr,
                        });
                    }
                }
                0x44 if payload.len() == 10 => {
                    latest_figure_descriptor = Some(payload.to_vec());
                }
                0x4a if payload.len() >= 16 => {
                    if let Some((start_block, start_offset, end_block, end_offset)) =
                        parse_pcmdata_range_pointer(payload)
                    {
                        let component = self
                            .ssed_component_for_role_or_name(
                                SsedComponentRole::PcmData,
                                "PCMDATA.DIC",
                            )
                            .map(|component| component.filename.clone())
                            .unwrap_or_else(|| "PCMDATA.DIC".to_owned());
                        candidates.push(InternalResource::SsedPcmDataRange {
                            component,
                            start_block,
                            start_offset,
                            end_block,
                            end_offset,
                        });
                    }
                }
                0x64 if payload.len() == 6 => {
                    if let Some((block, offset)) = parse_packed_bcd_pointer(payload) {
                        if let Some(descriptor) = latest_figure_descriptor.as_deref()
                            && let Some(dimensions) =
                                crate::ssed_figure::parse_figure_dimensions(descriptor)
                            && let Some(component) = self.ssed_component_for_role_or_name(
                                SsedComponentRole::Figure,
                                "FIGURE.DIC",
                            )
                            && component.contains_block(block)
                        {
                            candidates.push(InternalResource::SsedFigure {
                                component: component.filename.clone(),
                                block,
                                offset,
                                width: dimensions.width,
                                height: dimensions.height,
                            });
                        } else if let Some(component) = self.ssed_component_for_role_or_name(
                            SsedComponentRole::MonoScr,
                            "MONOSCR.DIC",
                        ) && component.contains_block(block)
                        {
                            candidates.push(InternalResource::SsedComponentAddress {
                                component: component.filename.clone(),
                                block,
                                offset,
                                resource_kind: ResourceKind::Image,
                            });
                        }
                    }
                    latest_figure_descriptor = None;
                }
                _ => {}
            }
            pos += 2 + arg_len;
        }
        candidates
    }

    pub(super) fn ssed_pdfspread_resource_candidate(
        &self,
        data: &[u8],
    ) -> Result<Option<InternalResource>> {
        if self.ssed_pdfspread_database()?.is_none() {
            return Ok(None);
        }
        let text = hc03e9_pdfspread_anchor_text(data);
        let Some(page_id) = normalize_pdfspread_page_id(&text) else {
            return Ok(None);
        };
        Ok(Some(InternalResource::SsedPdfSpread { page_id }))
    }

    pub(super) fn lookup_pdfspread_page(
        &self,
        page_id: &str,
    ) -> Result<Option<crate::ssed_pdfspread::PdfSpreadLookup>> {
        let Some(path) = self.ssed_pdfspread_database()? else {
            return Ok(None);
        };
        lookup_pdfspread(path, page_id)
    }

    pub(super) fn ssed_pdfspread_database(&self) -> Result<Option<&PathBuf>> {
        let database = self
            .ssed_pdfspread_database
            .get_or_init(|| find_pdfspread_database(&self.root).map_err(|error| error.to_string()));
        match database {
            Ok(path) => Ok(path.as_ref()),
            Err(error) => Err(Error::Driver(error.clone())),
        }
    }

    pub(super) fn ssed_sounddata_index(&self) -> Result<Option<&SoundDataIndex>> {
        let index = self
            .ssed_sounddata_index
            .get_or_init(|| load_sounddata_index(&self.root).map_err(|error| error.to_string()));
        match index {
            Ok(index) => Ok(index.as_ref()),
            Err(error) => Err(Error::Driver(error.clone())),
        }
    }
}

use super::*;

impl ReaderBookPackage {
    pub(super) fn ssed_stream_renderer_resources(
        &self,
        component_name: &str,
        offset: u64,
        length: Option<u64>,
    ) -> Result<(Vec<ResourceRef>, Vec<Diagnostic>)> {
        const RESOURCE_SCAN_LIMIT: usize = 256 * 1024;

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
        let explicit_length = length.and_then(|length| usize::try_from(length).ok());
        let requested = explicit_length.unwrap_or(RESOURCE_SCAN_LIMIT);
        let size = available.min(requested).min(RESOURCE_SCAN_LIMIT);
        let data = reader.read_range(start, size)?;
        let mut candidates = self.ssed_renderer_resource_candidates(&data);
        if let Some(pdfspread) = self.ssed_pdfspread_resource_candidate(&data)? {
            candidates.push(pdfspread);
        }
        let mut seen = BTreeSet::new();
        let mut resources = Vec::new();
        let mut diagnostics = Vec::new();

        for resource in candidates {
            let token = ResourceToken::new(&resource)?;
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

    pub(super) fn ssed_renderer_resource_candidates(&self, data: &[u8]) -> Vec<InternalResource> {
        let mut candidates = Vec::new();
        let mut latest_figure_descriptor: Option<Vec<u8>> = None;
        let mut pos = 0usize;
        while pos + 2 <= data.len() {
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

    pub(super) fn ssed_component_for_role_or_name(
        &self,
        role: SsedComponentRole,
        name: &str,
    ) -> Option<&SsedComponent> {
        let catalog = self.ssed_catalog.as_ref()?;
        catalog
            .components_by_role(role)
            .next()
            .or_else(|| catalog.component_named(name))
    }

    pub(super) fn resolve_readable_ssed_component_path(
        &self,
        component: &SsedComponent,
    ) -> Result<Option<PathBuf>> {
        let candidates = self.resolve_ssed_component_candidate_paths(component)?;
        if candidates.is_empty() {
            return Ok(None);
        }
        let mut unreadable = Vec::new();
        for path in candidates {
            match self.materialize_readable_ssed_component_path(component, &path)? {
                Some(readable) => return Ok(Some(readable)),
                None => unreadable.push(path),
            }
        }
        Err(Error::Driver(format!(
            "candidate file(s) were found but none decoded as plain, zipped, or encrypted SSEDDATA: {}",
            unreadable
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )))
    }

    pub(super) fn resolve_ssed_component_candidate_paths(
        &self,
        component: &SsedComponent,
    ) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        let mut seen = BTreeSet::new();
        if let Some(path) = self
            .storage
            .resolve_casefolded(Path::new(&component.filename))?
        {
            seen.insert(path.clone());
            paths.push(path);
        }
        for alias in ssed_component_filename_aliases(component) {
            if let Some(path) = self.storage.resolve_casefolded(Path::new(&alias))?
                && seen.insert(path.clone())
            {
                paths.push(path);
            }
        }
        Ok(paths)
    }

    pub(super) fn ssed_component_by_name(&self, component_name: &str) -> Option<&SsedComponent> {
        self.ssed_catalog
            .as_ref()?
            .components
            .iter()
            .find(|component| {
                component.filename.eq_ignore_ascii_case(component_name)
                    || (component_name.eq_ignore_ascii_case("COLSCR.DIC")
                        && component.role == SsedComponentRole::Colscr)
            })
    }

    pub(super) fn read_ssed_colscr_image(
        &self,
        component_name: &str,
        block: u32,
        offset: u32,
    ) -> Result<Vec<u8>> {
        let Some(component) = self.ssed_component_by_name(component_name) else {
            return Err(Error::Driver(format!(
                "SSED component not declared: {component_name}"
            )));
        };
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Err(Error::Driver(format!(
                "SSED component not found: {}",
                component.filename
            )));
        };
        let mut reader = SsedDataFile::open(path)?;
        if offset >= BLOCK_SIZE {
            return Err(Error::Driver(format!(
                "invalid COLSCR offset {offset}; block offsets must be less than {BLOCK_SIZE}"
            )));
        }
        let start_block = reader.header().start_block;
        if block < start_block {
            return Err(Error::Driver(format!(
                "COLSCR block {block} is before component start block {start_block}"
            )));
        }
        let relative_offset =
            (block - start_block) as usize * BLOCK_SIZE as usize + offset as usize;
        let header = reader.read_range(relative_offset, 70)?;
        let Some(payload_size) = parse_colscr_wrapped_payload_size(&header) else {
            return Err(Error::Driver(format!(
                "COLSCR image header did not decode at {component_name}:{block:08}:{offset:04}"
            )));
        };
        let wrapped = reader.read_range(relative_offset, 8 + payload_size)?;
        if wrapped.len() != 8 + payload_size {
            return Err(Error::Driver(format!(
                "COLSCR image at {component_name}:{block:08}:{offset:04} is truncated"
            )));
        }
        Ok(wrapped[8..].to_vec())
    }

    pub(super) fn is_ssed_monoscr_component(&self, component_name: &str) -> bool {
        self.ssed_component_by_name(component_name)
            .is_some_and(|component| {
                component.role == SsedComponentRole::MonoScr
                    || component.filename.eq_ignore_ascii_case("MONOSCR.DIC")
            })
    }

    pub(super) fn read_ssed_monoscr_png(
        &self,
        component_name: &str,
        block: u32,
        offset: u32,
    ) -> Result<Vec<u8>> {
        let Some(component) = self.ssed_component_by_name(component_name) else {
            return Err(Error::Driver(format!(
                "SSED component not declared: {component_name}"
            )));
        };
        if component.role != SsedComponentRole::MonoScr
            && !component.filename.eq_ignore_ascii_case("MONOSCR.DIC")
        {
            return Err(Error::Driver(format!(
                "{} is not a MONOSCR component",
                component.filename
            )));
        }
        let Some(relative_offset) = component.relative_offset(block, offset) else {
            return Err(Error::Driver(format!(
                "MONOSCR address {component_name}:{block:08}:{offset:04} is outside the component range"
            )));
        };
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Err(Error::Driver(format!(
                "SSED component not found: {}",
                component.filename
            )));
        };
        let mut reader = SsedDataFile::open(path)?;
        let bitmap = reader.read_range(relative_offset as usize, MONOSCR_BITMAP_BYTES)?;
        if bitmap.len() != MONOSCR_BITMAP_BYTES {
            return Err(Error::Driver(format!(
                "MONOSCR cell at {component_name}:{block:08}:{offset:04} is truncated"
            )));
        }
        encode_png_rgba(
            MONOSCR_WIDTH,
            MONOSCR_HEIGHT,
            &monoscr_bitmap_to_rgba(&bitmap),
        )
    }

    pub(super) fn read_ssed_figure_resource(
        &self,
        component_name: &str,
        block: u32,
        offset: u32,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>> {
        let Some(component) = self.ssed_component_by_name(component_name) else {
            return Err(Error::Driver(format!(
                "SSED component not declared: {component_name}"
            )));
        };
        if component.role != SsedComponentRole::Figure
            && !component.filename.eq_ignore_ascii_case("FIGURE.DIC")
        {
            return Err(Error::Driver(format!(
                "{} is not a FIGURE component",
                component.filename
            )));
        }
        let dimensions = FigureDimensions::new(width, height)?;
        let Some(relative_offset) = component.relative_offset(block, offset) else {
            return Err(Error::Driver(format!(
                "FIGURE address {component_name}:{block:08}:{offset:04} is outside the component range"
            )));
        };
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Err(Error::Driver(format!(
                "SSED component not found: {}",
                component.filename
            )));
        };
        let size = dimensions.bitmap_bytes()?;
        let mut reader = SsedDataFile::open(path)?;
        let relative_offset = usize::try_from(relative_offset)
            .map_err(|_| Error::Driver("FIGURE offset is too large".to_owned()))?;
        let bitmap = reader.read_range(relative_offset, size)?;
        if bitmap.len() != size {
            return Err(Error::Driver(format!(
                "FIGURE bitmap at {component_name}:{block:08}:{offset:04} is truncated"
            )));
        }
        figure_bitmap_to_png(&bitmap, dimensions)
    }

    pub(super) fn read_ssed_pcmdata_range(
        &self,
        component_name: &str,
        start_block: u32,
        start_offset: u32,
        end_block: u32,
        end_offset: u32,
    ) -> Result<Vec<u8>> {
        let (start_relative, raw, prefix) = self.read_ssed_pcmdata_raw_range(
            component_name,
            start_block,
            start_offset,
            end_block,
            end_offset,
        )?;
        let (portable, _summary) = pcmdata_portable_audio_bytes(start_relative, &raw, &prefix)?;
        Ok(portable)
    }

    pub(super) fn ssed_pcmdata_range_summary(
        &self,
        component_name: &str,
        start_block: u32,
        start_offset: u32,
        end_block: u32,
        end_offset: u32,
    ) -> Result<PcmDataParseResult> {
        let (start_relative, raw, prefix) = self.read_ssed_pcmdata_raw_range(
            component_name,
            start_block,
            start_offset,
            end_block,
            end_offset,
        )?;
        pcmdata_audio_summary(start_relative, &raw, &prefix)
    }

    pub(super) fn read_ssed_pcmdata_raw_range(
        &self,
        component_name: &str,
        start_block: u32,
        start_offset: u32,
        end_block: u32,
        end_offset: u32,
    ) -> Result<(usize, Vec<u8>, Vec<u8>)> {
        let Some(component) = self.ssed_component_by_name(component_name) else {
            return Err(Error::Driver(format!(
                "SSED component not declared: {component_name}"
            )));
        };
        if component.role != SsedComponentRole::PcmData
            && !component.filename.eq_ignore_ascii_case("PCMDATA.DIC")
        {
            return Err(Error::Driver(format!(
                "{} is not a PCMDATA component",
                component.filename
            )));
        }
        if start_offset >= BLOCK_SIZE || end_offset >= BLOCK_SIZE {
            return Err(Error::Driver(format!(
                "invalid PCMDATA offsets {start_offset}..{end_offset}; block offsets must be less than {BLOCK_SIZE}"
            )));
        }
        let Some(start_relative) = component.relative_offset(start_block, start_offset) else {
            return Err(Error::Driver(format!(
                "PCMDATA start address {component_name}:{start_block:08}:{start_offset:04} is outside the component range"
            )));
        };
        let Some(end_relative) = component.relative_offset(end_block, end_offset) else {
            return Err(Error::Driver(format!(
                "PCMDATA end address {component_name}:{end_block:08}:{end_offset:04} is outside the component range"
            )));
        };
        if end_relative < start_relative {
            return Err(Error::Driver(format!(
                "PCMDATA range end is before start: {component_name}:{start_block:08}:{start_offset:04}-{end_block:08}:{end_offset:04}"
            )));
        }
        let size = usize::try_from(end_relative - start_relative + 1)
            .map_err(|_| Error::Driver("PCMDATA range is too large".to_owned()))?;
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Err(Error::Driver(format!(
                "SSED component not found: {}",
                component.filename
            )));
        };
        let mut reader = SsedDataFile::open(path)?;
        let start_relative = usize::try_from(start_relative)
            .map_err(|_| Error::Driver("PCMDATA start offset is too large".to_owned()))?;
        let raw = reader.read_range(start_relative, size)?;
        if raw.len() != size {
            return Err(Error::Driver(format!(
                "PCMDATA range {component_name}:{start_block:08}:{start_offset:04}-{end_block:08}:{end_offset:04} is truncated"
            )));
        }
        let prefix = reader.read_range(0, 2048)?;
        Ok((start_relative, raw, prefix))
    }

    pub(super) fn materialize_readable_ssed_component_path(
        &self,
        component: &SsedComponent,
        path: &Path,
    ) -> Result<Option<PathBuf>> {
        if SsedDataHeader::parse_file(path).is_ok() {
            return Ok(Some(path.to_path_buf()));
        }

        if file_starts_with_android_wrapped_sseddata(path)? {
            let cache_path =
                self.ssed_component_cache_path(component, path, "android_lved_wrapped", "dic")?;
            if SsedDataHeader::parse_file(&cache_path).is_ok() {
                return Ok(Some(cache_path));
            }
            let tmp_path = cache_path.with_extension("tmp");
            if tmp_path.exists() {
                fs::remove_file(&tmp_path)?;
            }
            normalize_android_wrapped_sseddata_file_to_path(path, &tmp_path)?;
            SsedDataHeader::parse_file(&tmp_path)?;
            fs::rename(&tmp_path, &cache_path)?;
            return Ok(Some(cache_path));
        }

        if looks_like_zip_file(path)?
            && let Some(extracted) = self.extract_zipped_ssed_component(component, path)?
        {
            return self.materialize_readable_ssed_component_path(component, &extracted);
        }

        if let Some(decrypted) = self.decrypt_ssed_component_if_needed(component, path)? {
            return Ok(Some(decrypted));
        }

        Ok(None)
    }

    pub(super) fn extract_zipped_ssed_component(
        &self,
        component: &SsedComponent,
        zip_path: &Path,
    ) -> Result<Option<PathBuf>> {
        let member_name = match zip_member_name_for_component(component, zip_path)? {
            Some(member_name) => member_name,
            None => return Ok(None),
        };
        for password in self.mac_honmon_zip_passwords()? {
            let file = File::open(zip_path)?;
            let mut archive = ZipArchive::new(file).map_err(zip_error)?;
            let mut member = match password.as_deref() {
                Some(password) => match archive.by_name_decrypt(&member_name, password) {
                    Ok(member) => member,
                    Err(ZipError::InvalidPassword)
                    | Err(ZipError::UnsupportedArchive(ZipError::PASSWORD_REQUIRED)) => continue,
                    Err(err) => return Err(zip_error(err)),
                },
                None => match archive.by_name(&member_name) {
                    Ok(member) if member.encrypted() => continue,
                    Ok(member) => member,
                    Err(ZipError::UnsupportedArchive(ZipError::PASSWORD_REQUIRED)) => continue,
                    Err(err) => return Err(zip_error(err)),
                },
            };
            let size_limit =
                zipped_ssed_component_size_limit(component, &member_name, member.size())?;
            let cache_path = self.ssed_component_cache_path(
                component,
                zip_path,
                &format!(
                    "zip:{}:{}",
                    member_name,
                    password
                        .as_deref()
                        .map(hex::encode)
                        .unwrap_or_else(|| "none".to_owned())
                ),
                "bin",
            )?;
            if cache_path.exists() {
                return Ok(Some(cache_path));
            }
            let tmp_path = cache_path.with_extension("tmp");
            {
                let mut outfile = File::create(&tmp_path)?;
                if let Err(error) =
                    copy_zip_member_with_size_limit(&mut member, &mut outfile, size_limit)
                {
                    let _ = fs::remove_file(&tmp_path);
                    match error {
                        Error::Io(error) => {
                            if password.is_none()
                                && matches!(
                                    error.kind(),
                                    std::io::ErrorKind::Unsupported
                                        | std::io::ErrorKind::PermissionDenied
                                        | std::io::ErrorKind::InvalidData
                                )
                            {
                                continue;
                            }
                            return Err(Error::Io(error));
                        }
                        error => return Err(error),
                    }
                }
                outfile.flush()?;
            }
            fs::rename(&tmp_path, &cache_path)?;
            return Ok(Some(cache_path));
        }
        Ok(None)
    }

    pub(super) fn decrypt_ssed_component_if_needed(
        &self,
        component: &SsedComponent,
        path: &Path,
    ) -> Result<Option<PathBuf>> {
        let mut file = File::open(path)?;
        let mut prefix = vec![0_u8; 4096];
        let read = file.read(&mut prefix)?;
        prefix.truncate(read);
        if prefix.len() < 16 {
            return Ok(None);
        }
        let attempts: [(&str, PrefixDecryptFn, FileDecryptFn); 3] = [
            (
                "android_honmon_diw",
                decrypt_android_diw_prefix,
                decrypt_android_diw_file_to_path,
            ),
            (
                "macos_logofont_cipher",
                decrypt_macos_logofont_cipher_prefix,
                decrypt_macos_logofont_cipher_file_to_path,
            ),
            (
                "logofont_cipher",
                decrypt_logofont_cipher_prefix,
                decrypt_logofont_cipher_file_to_path,
            ),
        ];
        for (name, prefix_decrypt, file_decrypt) in attempts {
            let decrypted_prefix = prefix_decrypt(&prefix, prefix.len())?;
            if !decrypted_prefix.starts_with(SSEDDATA_MAGIC) {
                continue;
            }
            let cache_path = self.ssed_component_cache_path(component, path, name, "dic")?;
            if SsedDataHeader::parse_file(&cache_path).is_ok() {
                return Ok(Some(cache_path));
            }
            let tmp_path = cache_path.with_extension("tmp");
            file_decrypt(path, &tmp_path)?;
            SsedDataHeader::parse_file(&tmp_path)?;
            fs::rename(&tmp_path, &cache_path)?;
            return Ok(Some(cache_path));
        }
        Ok(None)
    }

    pub(super) fn mac_honmon_zip_passwords(&self) -> Result<Vec<Option<Vec<u8>>>> {
        let mut passwords = vec![None];
        for path in self.storage.list_dir(Path::new(""))? {
            if !path.is_file() {
                continue;
            }
            let Some(stem) = path.file_stem().map(|value| value.to_string_lossy()) else {
                continue;
            };
            let Some(extension) = path.extension().map(|value| value.to_string_lossy()) else {
                continue;
            };
            if !extension.eq_ignore_ascii_case("idx") {
                continue;
            }
            let lower = stem.to_ascii_lowercase();
            if lower.len() == 8 && lower.chars().all(|ch| ch.is_ascii_hexdigit()) {
                passwords.push(Some(format!("casKet{lower}").into_bytes()));
            }
        }
        Ok(passwords)
    }

    pub(super) fn ssed_component_cache_path(
        &self,
        component: &SsedComponent,
        source: &Path,
        stage: &str,
        extension: &str,
    ) -> Result<PathBuf> {
        let metadata = fs::metadata(source)?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let mut hasher = Sha256::new();
        hasher.update(self.metadata.root_fingerprint.as_bytes());
        hasher.update(b"\0");
        hasher.update(component.filename.as_bytes());
        hasher.update(b"\0");
        hasher.update(stage.as_bytes());
        hasher.update(b"\0");
        hasher.update(source.as_os_str().to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(metadata.len().to_le_bytes());
        hasher.update(modified.to_le_bytes());
        let hash = hex::encode(hasher.finalize());
        let dir = private_cache_dir("ssed-components")?;
        Ok(dir.join(format!("{hash}.{extension}")))
    }

    pub(super) fn ssed_aux_index_specs(&self) -> Result<Vec<SsedAuxIndexSpec>> {
        let relative = Path::new("EXINFO.INI");
        if !self.storage.exists(relative)? {
            return Ok(Vec::new());
        }
        let bytes = self.storage.read(relative)?;
        Ok(parse_aux_index_specs_from_exinfo(&bytes))
    }

    pub(super) fn ssed_numeric_aux_index_specs(
        &self,
        excluded_infos: &BTreeSet<String>,
    ) -> Result<Vec<SsedAuxIndexSpec>> {
        let mut specs = Vec::new();
        for path in self.storage.list_dir(Path::new(""))? {
            let Some(name) = path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
            else {
                continue;
            };
            if !is_numeric_aux_index_filename(&name) {
                continue;
            }
            if excluded_infos.contains(&name.to_ascii_lowercase()) {
                continue;
            }
            if file_starts_with_ssedinfo_magic(&path)? {
                continue;
            }
            let index = specs.len();
            specs.push(SsedAuxIndexSpec {
                index,
                name: name.clone(),
                info: name,
            });
        }
        Ok(specs)
    }

    pub(super) fn discover_ssed_hanrei_pages(&self) -> Result<Vec<SsedHanreiPage>> {
        let mut pages = Vec::new();
        let mut seen = BTreeSet::new();

        for candidate in [
            "hanrei.html",
            "HANREI.html",
            "HANREI/index.html",
            "HANREI/index.htm",
            "HANREI/hanrei.html",
            "HANREI/hanrei.htm",
        ] {
            self.push_ssed_hanrei_page(candidate, &mut pages, &mut seen)?;
        }

        self.push_ssed_hanrei_folder_pages("HANREI", &mut pages, &mut seen, 0)?;
        self.push_ssed_hanrei_chm_pages("HANREI.chm", &mut pages, &mut seen)?;

        for path in self.storage.list_dir(Path::new(""))? {
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().map(|value| value.to_string_lossy()) else {
                continue;
            };
            if name.starts_with("._") || !name.to_ascii_lowercase().ends_with("_help.localized") {
                continue;
            }
            let root = name.replace('\\', "/");
            for candidate in [
                format!("{root}/index.html"),
                format!("{root}/index.htm"),
                format!("{root}/menu.html"),
                format!("{root}/top.html"),
                format!("{root}/contents/hanrei.html"),
                format!("{root}/contents/hanrei.htm"),
                format!("{root}/contents/copyright.html"),
                format!("{root}/contents/copyright.htm"),
            ] {
                self.push_ssed_hanrei_page(&candidate, &mut pages, &mut seen)?;
            }

            let contents_dir = format!("{root}/contents");
            for child in self.storage.list_dir(Path::new(&contents_dir))? {
                if !child.is_file() {
                    continue;
                }
                let Some(file_name) = child.file_name().map(|value| value.to_string_lossy()) else {
                    continue;
                };
                if file_name.starts_with("._") {
                    continue;
                }
                if !path_has_extension(&file_name, &["html", "htm"]) {
                    continue;
                }
                let candidate = format!("{contents_dir}/{file_name}");
                self.push_ssed_hanrei_page(&candidate, &mut pages, &mut seen)?;
            }
        }

        Ok(pages)
    }

    pub(super) fn push_ssed_hanrei_folder_pages(
        &self,
        relative_dir: &str,
        pages: &mut Vec<SsedHanreiPage>,
        seen: &mut BTreeSet<String>,
        depth: usize,
    ) -> Result<()> {
        if depth > 8 || !self.storage.exists(Path::new(relative_dir))? {
            return Ok(());
        }
        for child in self.storage.list_dir(Path::new(relative_dir))? {
            let Some(file_name) = child.file_name().map(|value| value.to_string_lossy()) else {
                continue;
            };
            if file_name.starts_with("._") {
                continue;
            }
            let candidate = format!("{relative_dir}/{file_name}");
            if child.is_dir() {
                self.push_ssed_hanrei_folder_pages(&candidate, pages, seen, depth + 1)?;
            } else if child.is_file() && path_has_extension(&file_name, &["html", "htm"]) {
                self.push_ssed_hanrei_page(&candidate, pages, seen)?;
            }
        }
        Ok(())
    }

    pub(super) fn push_ssed_hanrei_page(
        &self,
        candidate: &str,
        pages: &mut Vec<SsedHanreiPage>,
        seen: &mut BTreeSet<String>,
    ) -> Result<()> {
        let normalized = candidate.replace('\\', "/");
        if normalized
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        {
            return Ok(());
        }
        if !self.storage.exists(Path::new(&normalized))? {
            return Ok(());
        }
        if !seen.insert(normalized.to_ascii_lowercase()) {
            return Ok(());
        }
        let resource_kind = resource_kind_from_path(&normalized);
        pages.push(SsedHanreiPage {
            item_id: normalized.clone(),
            label: self.ssed_hanrei_package_page_label(&normalized),
            resource: InternalResource::PackageFile {
                path: normalized,
                resource_kind,
            },
            anchor: None,
            diagnostics: Vec::new(),
        });
        Ok(())
    }

    pub(super) fn ssed_hanrei_package_page_label(&self, normalized: &str) -> String {
        if path_has_extension(normalized, &["html", "htm"])
            && let Ok(data) = self.storage.read(Path::new(normalized))
        {
            let html = decode_package_html_text(&data);
            if let Some(label) = html_document_label(&html) {
                return label;
            }
        }
        ssed_hanrei_page_label(normalized)
    }

    pub(super) fn push_ssed_hanrei_chm_pages(
        &self,
        chm_path: &str,
        pages: &mut Vec<SsedHanreiPage>,
        seen: &mut BTreeSet<String>,
    ) -> Result<()> {
        if !self.storage.exists(Path::new(chm_path))? {
            return Ok(());
        }
        let Some(resolved) = self.storage.resolve_casefolded(Path::new(chm_path))? else {
            return Ok(());
        };
        let mut entries = match list_chm_entries(&resolved) {
            Ok(entries) => entries,
            Err(err) => {
                let item_id = chm_path.replace('\\', "/");
                if seen.insert(item_id.to_ascii_lowercase()) {
                    pages.push(SsedHanreiPage {
                        item_id: item_id.clone(),
                        label: ssed_hanrei_page_label(&item_id),
                        resource: InternalResource::PackageFile {
                            path: item_id,
                            resource_kind: ResourceKind::Other,
                        },
                        anchor: None,
                        diagnostics: vec![Diagnostic::info(
                            "ssed_hanrei_chm_deferred",
                            format!("HANREI.chm was found, but CHM decoding failed: {err}"),
                        )],
                    });
                }
                return Ok(());
            }
        };
        entries.sort_by_key(|entry| chm_hanrei_entry_sort_key(&entry.path));
        let mut hhc_items = Vec::new();
        for entry in &entries {
            if !path_has_extension(&entry.path, &["hhc"]) {
                continue;
            }
            if let Ok(bytes) = read_chm_entry(&resolved, &entry.path) {
                let html = decode_package_html_text(&bytes);
                hhc_items.extend(parse_chm_hhc_toc(&html));
            }
        }
        let mut html_count = 0usize;
        for entry in entries.iter().filter(|entry| {
            path_has_extension(&entry.path, &["html", "htm"])
                && chm_hanrei_entry_sort_key(&entry.path).0 == 0
        }) {
            if self.push_ssed_hanrei_chm_entry_page(
                chm_path,
                &entry.path,
                None,
                None,
                pages,
                seen,
            )? {
                html_count += 1;
            }
        }
        for item in hhc_items {
            let Some(local) = item.local.as_deref() else {
                continue;
            };
            let Some(reference) = chm_local_reference(local) else {
                continue;
            };
            if !path_has_extension(&reference.path, &["html", "htm"]) {
                continue;
            }
            if self.push_ssed_hanrei_chm_entry_page(
                chm_path,
                &reference.path,
                reference.anchor,
                Some(item.name),
                pages,
                seen,
            )? {
                html_count += 1;
            }
        }
        for entry in entries {
            if !path_has_extension(&entry.path, &["html", "htm"]) {
                continue;
            }
            if self.push_ssed_hanrei_chm_entry_page(
                chm_path,
                &entry.path,
                None,
                None,
                pages,
                seen,
            )? {
                html_count += 1;
            }
        }
        if html_count == 0 {
            let item_id = chm_path.replace('\\', "/");
            if seen.insert(item_id.to_ascii_lowercase()) {
                pages.push(SsedHanreiPage {
                    item_id: item_id.clone(),
                    label: ssed_hanrei_page_label(&item_id),
                    resource: InternalResource::PackageFile {
                        path: item_id,
                        resource_kind: ResourceKind::Other,
                    },
                    anchor: None,
                    diagnostics: vec![Diagnostic::info(
                        "ssed_hanrei_chm_deferred",
                        "HANREI.chm was found, but no HTML entries were discovered",
                    )],
                });
            }
        }
        Ok(())
    }

    pub(super) fn push_ssed_hanrei_chm_entry_page(
        &self,
        chm_path: &str,
        entry_path: &str,
        anchor: Option<String>,
        label: Option<String>,
        pages: &mut Vec<SsedHanreiPage>,
        seen: &mut BTreeSet<String>,
    ) -> Result<bool> {
        let item_id = if let Some(anchor) = &anchor {
            format!("{chm_path}!/{entry_path}#{anchor}")
        } else {
            format!("{chm_path}!/{entry_path}")
        };
        if !seen.insert(item_id.to_ascii_lowercase()) {
            return Ok(false);
        }
        pages.push(SsedHanreiPage {
            item_id: item_id.clone(),
            label: label.unwrap_or_else(|| ssed_hanrei_page_label(&item_id)),
            resource: InternalResource::ChmFile {
                chm_path: chm_path.to_owned(),
                entry_path: entry_path.to_owned(),
                resource_kind: ResourceKind::Html,
            },
            anchor,
            diagnostics: Vec::new(),
        });
        Ok(true)
    }
}

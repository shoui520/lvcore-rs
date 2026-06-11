use super::*;

impl ReaderBookPackage {
    pub(super) fn ssed_color_sample_table(&self) -> Result<Option<&ColorSampleTable>> {
        let table = self.ssed_color_sample_table.get_or_init(|| {
            self.load_ssed_color_sample_table()
                .map_err(|error| error.to_string())
        });
        match table {
            Ok(Some(table)) => Ok(Some(table)),
            Ok(None) => Ok(None),
            Err(error) => Err(Error::Driver(error.clone())),
        }
    }

    fn load_ssed_color_sample_table(&self) -> Result<Option<ColorSampleTable>> {
        let Some(component) = self.ssed_catalog.as_ref().and_then(|catalog| {
            catalog.components.iter().find(|component| {
                component.role == SsedComponentRole::ColSample
                    || component.filename.eq_ignore_ascii_case("COLSMPL.DIC")
            })
        }) else {
            return Ok(None);
        };
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Ok(None);
        };
        let mut reader = SsedDataFile::open(path)?;
        let data = reader.read_range(0, reader.header().expanded_size())?;
        Ok(Some(parse_color_sample_table(&data)))
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
}

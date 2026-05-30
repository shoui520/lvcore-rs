use super::*;

impl ResourceProvider for ReaderBookPackage {
    fn resolve_resource(&self, token: &ResourceToken) -> Result<ResourceRef> {
        match token.decode()? {
            InternalResource::PackageFile {
                path,
                resource_kind,
            } => self.resolve_package_file_resource(token, &path, resource_kind),
            InternalResource::SsedLooseFile {
                root_name,
                path,
                resource_kind,
            } => self.resolve_ssed_loose_file_resource(token, &root_name, &path, resource_kind),
            InternalResource::SsedComponentAddress {
                component,
                block,
                offset,
                resource_kind,
            } => self.resolve_ssed_component_address_resource(
                token,
                &component,
                block,
                offset,
                resource_kind,
            ),
            InternalResource::SsedFigure {
                component,
                block,
                offset,
                width,
                height,
            } => self.resolve_ssed_figure_resource(token, &component, block, offset, width, height),
            InternalResource::SsedGa16Glyph { path, code } => {
                self.resolve_ssed_ga16_glyph_resource(token, &path, &code)
            }
            InternalResource::SsedPcmDataRange {
                component,
                start_block,
                start_offset,
                end_block,
                end_offset,
            } => self.resolve_ssed_pcmdata_range_resource(
                token,
                &component,
                start_block,
                start_offset,
                end_block,
                end_offset,
            ),
            InternalResource::LooseMovie { movie_id } => {
                self.resolve_loose_movie_resource(token, &movie_id)
            }
            InternalResource::SsedPdfSpread { page_id } => {
                self.resolve_ssed_pdfspread_resource(token, &page_id)
            }
            InternalResource::SoundData { sound_id } => {
                self.resolve_sounddata_resource(token, sound_id)
            }
            InternalResource::ChmFile {
                chm_path,
                entry_path,
                resource_kind,
            } => self.resolve_chm_file_resource(token, &chm_path, &entry_path, resource_kind),
            InternalResource::MediaBlob {
                key, resource_kind, ..
            } => self.resolve_media_blob_resource(token, &key, resource_kind),
            InternalResource::Unsupported { reason } => {
                self.resolve_unsupported_resource(token, reason)
            }
        }
    }

    fn read_resource(&self, token: &ResourceToken) -> Result<Vec<u8>> {
        match token.decode()? {
            InternalResource::PackageFile { path, .. } => self.read_package_file_bytes(&path),
            InternalResource::SsedLooseFile {
                root_name, path, ..
            } => self.read_ssed_loose_file_resource(&root_name, &path),
            InternalResource::SsedComponentAddress {
                component,
                block,
                offset,
                resource_kind,
            } => {
                self.read_ssed_component_address_resource(&component, block, offset, resource_kind)
            }
            InternalResource::SsedFigure {
                component,
                block,
                offset,
                width,
                height,
            } => self.read_ssed_figure_resource(&component, block, offset, width, height),
            InternalResource::SsedGa16Glyph { path, code } => {
                self.read_ssed_ga16_glyph_resource(&path, &code)
            }
            InternalResource::SsedPcmDataRange {
                component,
                start_block,
                start_offset,
                end_block,
                end_offset,
            } => self.read_ssed_pcmdata_range_resource(
                &component,
                start_block,
                start_offset,
                end_block,
                end_offset,
            ),
            InternalResource::LooseMovie { movie_id } => self.read_loose_movie_resource(&movie_id),
            InternalResource::SsedPdfSpread { page_id } => {
                self.read_ssed_pdfspread_resource(&page_id)
            }
            InternalResource::SoundData { sound_id } => self.read_sounddata_resource(sound_id),
            InternalResource::ChmFile {
                chm_path,
                entry_path,
                ..
            } => self.read_chm_file_resource(&chm_path, &entry_path),
            InternalResource::MediaBlob { store, key, .. } => {
                self.read_media_blob_resource(&store, &key)
            }
            InternalResource::Unsupported { reason } => Err(Error::Driver(reason)),
        }
    }
}

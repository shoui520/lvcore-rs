use super::*;

const SSED_MARKER_VARIANT_BOUNDARY_SCAN_LIMIT: usize = 256 * 1024;

impl ReaderBookPackage {
    pub(super) fn visual_body_for_ssed_address(
        &self,
        requested_component: &str,
        block: u32,
        offset: u32,
    ) -> Result<VisualBody> {
        let Some(catalog) = &self.ssed_catalog else {
            return Ok(VisualBody::Unsupported {
                reason: "SSED catalog is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "ssed_catalog_missing",
                    "SSED address targets require a parsed SSEDINFO catalog",
                )],
            });
        };
        let component = catalog
            .component_named(requested_component)
            .or_else(|| catalog.component_for_address(block));
        let Some(component) = component else {
            return Ok(VisualBody::Unsupported {
                reason: "SSED address does not resolve to a catalog component".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "ssed_address_outside_components",
                    format!("no component contains logical block {block}"),
                )],
            });
        };
        let Some(component_offset) = component.relative_offset(block, offset) else {
            return Ok(VisualBody::Unsupported {
                reason: "SSED address is outside the resolved component".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "ssed_address_invalid_for_component",
                    format!(
                        "{} does not contain logical block {block} offset {offset}",
                        component.filename
                    ),
                )],
            });
        };
        if let Err(diagnostic) = self.validate_plain_component(component) {
            return Ok(VisualBody::Unsupported {
                reason: "SSED component is not readable as plain SSEDDATA".to_owned(),
                diagnostics: vec![diagnostic],
            });
        }
        if component.role == SsedComponentRole::Honmon
            && self.ssed_pdfspread_database()?.is_none()
            && let Some(anchor_id) = self.ssed_dense_anchor_at_component_offset(
                component,
                usize::try_from(component_offset).unwrap_or(usize::MAX),
            )?
        {
            return self.visual_body_for_ssed_dense_anchor(&anchor_id, None);
        }
        let stream_offset = self.ssed_stream_start_offset(component, component_offset);
        let length = self.infer_ssed_stream_length(component, stream_offset);
        Ok(VisualBody::SsedStream {
            component: component.filename.clone(),
            offset: stream_offset,
            length,
        })
    }

    fn ssed_stream_start_offset(&self, component: &SsedComponent, component_offset: u64) -> u64 {
        if component.role != SsedComponentRole::Honmon || component_offset < 2 {
            return component_offset;
        }
        let Some(prefix_offset) = component_offset.checked_sub(2) else {
            return component_offset;
        };
        let Some(path) = self
            .resolve_readable_ssed_component_path(component)
            .ok()
            .flatten()
        else {
            return component_offset;
        };
        let Ok(mut reader) = SsedDataFile::open(path) else {
            return component_offset;
        };
        let Ok(prefix_offset_usize) = usize::try_from(prefix_offset) else {
            return component_offset;
        };
        let Ok(data) = reader.read_range(prefix_offset_usize, SSED_ENTRY_MARKER.len() + 2) else {
            return component_offset;
        };
        if data.starts_with(&[0x1f, 0x02])
            && data
                .get(2..2 + SSED_ENTRY_MARKER.len())
                .is_some_and(|marker| marker == SSED_ENTRY_MARKER)
        {
            prefix_offset
        } else {
            component_offset
        }
    }

    fn infer_ssed_stream_length(
        &self,
        component: &SsedComponent,
        component_offset: u64,
    ) -> Option<u64> {
        if component.role != SsedComponentRole::Honmon {
            return None;
        }
        let path = self
            .resolve_readable_ssed_component_path(component)
            .ok()
            .flatten()?;
        let mut reader = SsedDataFile::open(path).ok()?;
        let start = usize::try_from(component_offset).ok()?;
        if start >= reader.header().expanded_size() {
            return None;
        }
        if let Some(marker_len) = ssed_reader_generic_entry_marker_len(&mut reader, start).ok()? {
            return ssed_find_next_entry_marker_offset(
                &mut reader,
                start.saturating_add(marker_len),
            )
            .ok()
            .flatten()
            .map(|next| next.saturating_sub(start) as u64)
            .or_else(|| Some((reader.header().expanded_size() - start) as u64));
        }
        if let Some(marker_len) = ssed_reader_index_boundary_marker_variant_len(&mut reader, start)
            .ok()
            .flatten()
        {
            return ssed_find_next_marker_variant_offset(
                &mut reader,
                start.saturating_add(marker_len),
                &[0x1f, 0x09, 0x00, 0x02],
            )
            .ok()
            .flatten()
            .map(|next| next.saturating_sub(start) as u64);
        }
        if let Some(marker_len) = ssed_reader_metadata_record_marker_len(&mut reader, start)
            .ok()
            .flatten()
        {
            return ssed_find_next_metadata_record_boundary_offset(
                &mut reader,
                start.saturating_add(marker_len),
            )
            .ok()
            .flatten()
            .map(|next| next.saturating_sub(start) as u64);
        }
        None
    }

    pub(super) fn visual_body_for_ssed_dense_anchor(
        &self,
        anchor_id: &str,
        resolver_hint: Option<&str>,
    ) -> Result<VisualBody> {
        match lookup_ssed_dense_sidecar_body_with_resolvers(
            self.ssed_sidecar_body_resolvers()?,
            anchor_id,
            resolver_hint,
        )? {
            SsedSidecarLookup::Resolved(body) => {
                if let Some(html) = body.html {
                    Ok(VisualBody::PreservedHtml {
                        html,
                        source: match body.resolver.kind {
                            SsedSidecarKind::TContents => BodySourceKind::RendererDatabase,
                            _ => BodySourceKind::SidecarHtml,
                        },
                    })
                } else {
                    Ok(VisualBody::SemanticFallback { text: body.text })
                }
            }
            SsedSidecarLookup::MissingRow { diagnostics, .. } => Ok(VisualBody::Unsupported {
                reason: "dense HONMON sidecar row is missing".to_owned(),
                diagnostics,
            }),
            SsedSidecarLookup::NoResolver { diagnostics } => Ok(VisualBody::Unsupported {
                reason: "dense HONMON sidecar resolver is unavailable".to_owned(),
                diagnostics,
            }),
        }
    }

    pub(super) fn visual_body_for_britannica_chronology_record(
        &self,
        inc_code: &str,
    ) -> Result<VisualBody> {
        match lookup_britannica_chronology_record(&self.root, inc_code)? {
            Some(record) => Ok(VisualBody::PreservedHtml {
                html: record.html,
                source: BodySourceKind::BritannicaChronologySqlite,
            }),
            None => Ok(VisualBody::Unsupported {
                reason: "Britannica chronology row is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "ssed_britannica_chronology_row_missing",
                    format!("Britannica chronology row {inc_code} was not found"),
                )],
            }),
        }
    }

    pub(super) fn ssed_sidecar_body_resolvers(&self) -> Result<&[SsedSidecarBodyResolver]> {
        let resolvers = self.ssed_sidecar_body_resolvers.get_or_init(|| {
            discover_ssed_sidecar_body_resolvers(
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

    fn ssed_dense_anchor_at_component_offset(
        &self,
        component: &SsedComponent,
        offset: usize,
    ) -> Result<Option<String>> {
        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Ok(None);
        };
        let mut reader = SsedDataFile::open(&path)?;
        let mut data = reader.read_range(offset, 256)?;
        if let Some(anchor_id) = parse_observed_ssed_dense_anchor_id(&data) {
            return Ok(Some(anchor_id));
        }
        if let Some(end) = find_ssed_dense_anchor_record_end(&data) {
            data.truncate(end);
        }
        let decoded = decode_ssed_body_search_text(&data);
        let compact = decoded
            .chars()
            .filter(|ch| !ch.is_whitespace() && *ch != '\0')
            .collect::<String>();
        if compact.len() >= 4
            && compact.len() <= 16
            && compact.chars().all(|ch| ch.is_ascii_digit())
        {
            Ok(Some(compact))
        } else {
            Ok(None)
        }
    }
}

fn ssed_reader_index_boundary_marker_variant_len(
    reader: &mut SsedDataFile,
    offset: usize,
) -> Result<Option<usize>> {
    let data = reader.read_range(offset, SSED_ENTRY_MARKER.len())?;
    if data == [0x1f, 0x09, 0x00, 0x02] {
        Ok(Some(SSED_ENTRY_MARKER.len()))
    } else {
        Ok(None)
    }
}

fn ssed_find_next_marker_variant_offset(
    reader: &mut SsedDataFile,
    offset: usize,
    marker: &[u8],
) -> Result<Option<usize>> {
    if marker.is_empty() {
        return Ok(None);
    }
    let available = reader.header().expanded_size().saturating_sub(offset);
    let size = available.min(SSED_MARKER_VARIANT_BOUNDARY_SCAN_LIMIT);
    let data = reader.read_range(offset, size)?;
    Ok(data
        .windows(marker.len())
        .position(|window| window == marker)
        .map(|relative| offset.saturating_add(relative)))
}

fn ssed_reader_metadata_record_marker_len(
    reader: &mut SsedDataFile,
    offset: usize,
) -> Result<Option<usize>> {
    const METADATA_RECORD_MARKER: [u8; 4] = [0x1f, 0x09, 0x99, 0x99];
    let data = reader.read_range(offset, METADATA_RECORD_MARKER.len())?;
    if data == METADATA_RECORD_MARKER {
        Ok(Some(METADATA_RECORD_MARKER.len()))
    } else {
        Ok(None)
    }
}

fn ssed_find_next_metadata_record_boundary_offset(
    reader: &mut SsedDataFile,
    offset: usize,
) -> Result<Option<usize>> {
    const RECORD_CLOSE_BEFORE_NEXT_METADATA: [u8; 8] =
        [0x1f, 0x61, 0x1f, 0x0a, 0x1f, 0x09, 0x99, 0x99];
    let available = reader.header().expanded_size().saturating_sub(offset);
    let size = available.min(SSED_MARKER_VARIANT_BOUNDARY_SCAN_LIMIT);
    let data = reader.read_range(offset, size)?;
    Ok(data
        .windows(RECORD_CLOSE_BEFORE_NEXT_METADATA.len())
        .position(|window| window == RECORD_CLOSE_BEFORE_NEXT_METADATA)
        .map(|relative| offset.saturating_add(relative).saturating_add(4)))
}

use super::*;

const SSED_MARKER_VARIANT_BOUNDARY_SCAN_LIMIT: usize = 256 * 1024;

impl ReaderBookPackage {
    pub(super) fn visual_body_for_ssed_address(
        &self,
        requested_component: &str,
        block: u32,
        offset: u32,
    ) -> Result<VisualBody> {
        self.visual_body_for_ssed_address_with_options(
            requested_component,
            block,
            offset,
            None,
            false,
        )
    }

    pub(super) fn visual_body_for_ssed_index_address(
        &self,
        requested_component: &str,
        block: u32,
        offset: u32,
        _index_component: &str,
    ) -> Result<VisualBody> {
        self.visual_body_for_ssed_address_with_options(
            requested_component,
            block,
            offset,
            None,
            true,
        )
    }

    pub(super) fn visual_body_for_ssed_bounded_address(
        &self,
        requested_component: &str,
        block: u32,
        offset: u32,
        end_block: u32,
        end_offset: u32,
    ) -> Result<VisualBody> {
        self.visual_body_for_ssed_address_with_options(
            requested_component,
            block,
            offset,
            Some((end_block, end_offset)),
            false,
        )
    }

    fn visual_body_for_ssed_address_with_options(
        &self,
        requested_component: &str,
        block: u32,
        offset: u32,
        end: Option<(u32, u32)>,
        allow_index_boundary: bool,
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
        let (block, offset) = self.convert_ios_ssed_address(block, offset)?;
        let end = match end {
            Some((end_block, end_offset)) => {
                let (end_block, end_offset) =
                    self.convert_ios_ssed_address(end_block, end_offset)?;
                Some((end_block, end_offset))
            }
            None => None,
        };
        let component = catalog
            .component_named(requested_component)
            .or_else(|| catalog.component_for_address(block));
        let Some(component) = component else {
            if self.ssed_pdfspread_database()?.is_none()
                && let Some(body) = self.visual_body_for_ssed_sidecar_address(block, offset)?
            {
                return Ok(body);
            }
            return Ok(VisualBody::Unsupported {
                reason: "SSED address does not resolve to a catalog component".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "ssed_address_outside_components",
                    format!("no component contains logical block {block}"),
                )],
            });
        };
        let Some(component_offset) = component.relative_offset(block, offset) else {
            if self.ssed_pdfspread_database()?.is_none()
                && let Some(body) = self.visual_body_for_ssed_sidecar_address(block, offset)?
            {
                return Ok(body);
            }
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
        if component.role == SsedComponentRole::Honmon
            && self.ssed_pdfspread_database()?.is_none()
            && let Some(body) =
                self.visual_body_for_ssed_ordered_honbun_entry(component, component_offset)?
        {
            return Ok(body);
        }
        if component.role == SsedComponentRole::Honmon
            && self.ssed_pdfspread_database()?.is_none()
            && let Some(body) = self.visual_body_for_ssed_sidecar_address(block, offset)?
        {
            return Ok(body);
        }
        let stream_offset = self.ssed_stream_start_offset(component, component_offset);
        let inferred_length = self.infer_ssed_stream_length(component, stream_offset);
        let explicit_bounded_length = end.and_then(|(end_block, end_offset)| {
            bounded_ssed_stream_length(catalog, component, stream_offset, end_block, end_offset)
        });
        let index_bounded_length =
            if allow_index_boundary && end.is_none() && inferred_length.is_none() {
                self.ssed_next_index_body_pointer_after(SsedIndexPointer { block, offset })?
                    .filter(|end| {
                        ssed_index_bound_is_plausible(SsedIndexPointer { block, offset }, *end)
                    })
                    .and_then(|end| {
                        bounded_ssed_stream_length(
                            catalog,
                            component,
                            stream_offset,
                            end.block,
                            end.offset,
                        )
                    })
            } else {
                None
            };
        let sidecar_range_bounded_length = if component.role == SsedComponentRole::Honmon {
            self.ssed_sidecar_range_bound(block, offset)?
                .and_then(|bound| {
                    bounded_ssed_stream_length(catalog, component, stream_offset, bound.0, bound.1)
                })
        } else {
            None
        };
        let bounded_length = [
            explicit_bounded_length,
            index_bounded_length,
            sidecar_range_bounded_length,
        ]
        .into_iter()
        .flatten()
        .min();
        let length = match (inferred_length, bounded_length) {
            (Some(inferred), Some(bound)) => Some(inferred.min(bound)),
            (Some(inferred), None) => Some(inferred),
            (None, Some(bound)) => Some(bound),
            (None, None) => None,
        };
        Ok(VisualBody::SsedStream {
            component: component.filename.clone(),
            offset: stream_offset,
            length,
        })
    }

    fn visual_body_for_ssed_ordered_honbun_entry(
        &self,
        component: &SsedComponent,
        component_offset: u64,
    ) -> Result<Option<VisualBody>> {
        if !self
            .ssed_sidecar_body_resolvers()?
            .iter()
            .any(SsedSidecarBodyResolver::is_ordered_honbun_renderer_body)
        {
            return Ok(None);
        }
        let Some(row_index) =
            self.ssed_entry_slice_row_index_at_component_offset(component, component_offset)?
        else {
            return Ok(None);
        };
        match lookup_ssed_ordered_honbun_body_by_row(
            self.ssed_sidecar_body_resolvers()?,
            row_index,
        )? {
            SsedSidecarLookup::Resolved(body) => Ok(Some(VisualBody::PreservedHtml {
                html: body.html.unwrap_or(body.text),
                source: BodySourceKind::RendererDatabase,
            })),
            SsedSidecarLookup::MissingRow { .. } | SsedSidecarLookup::NoResolver { .. } => Ok(None),
        }
    }

    fn ssed_sidecar_range_bound(&self, block: u32, offset: u32) -> Result<Option<(u32, u32)>> {
        let Some(bound) = lookup_ssed_sidecar_range_bound_with_resolvers(
            self.ssed_sidecar_range_resolvers()?,
            block,
            offset,
        )?
        else {
            return Ok(None);
        };
        if (bound.end_block, bound.end_offset) > (block, offset) {
            Ok(Some((bound.end_block, bound.end_offset)))
        } else {
            Ok(None)
        }
    }

    pub(super) fn visual_body_for_ssed_sidecar_address(
        &self,
        block: u32,
        offset: u32,
    ) -> Result<Option<VisualBody>> {
        match lookup_ssed_sidecar_body_by_address_with_resolvers(
            self.ssed_sidecar_body_resolvers()?,
            block,
            offset,
        )? {
            SsedSidecarLookup::Resolved(body) => {
                Ok(Some(ssed_address_sidecar_body_to_visual(body)))
            }
            SsedSidecarLookup::MissingRow { .. } | SsedSidecarLookup::NoResolver { .. } => Ok(None),
        }
    }

    fn ssed_entry_slice_row_index_at_component_offset(
        &self,
        component: &SsedComponent,
        component_offset: u64,
    ) -> Result<Option<usize>> {
        const ENTRY_MARKER_SCAN_CHUNK_BYTES: usize = 64 * 1024;

        let Some(path) = self.resolve_readable_ssed_component_path(component)? else {
            return Ok(None);
        };
        let mut reader = SsedDataFile::open(path)?;
        let target_offset = usize::try_from(component_offset)
            .map_err(|_| Error::Driver("SSED component offset is too large".to_owned()))?;
        if target_offset >= reader.header().expanded_size() {
            return Ok(None);
        }

        let scan_end = target_offset
            .saturating_add(SSED_ENTRY_MARKER.len())
            .min(reader.header().expanded_size());
        let tail_size = SSED_ENTRY_MARKER.len() + 1;
        let mut carry = Vec::new();
        let mut carry_base = 0usize;
        let mut emitted_start: Option<usize> = None;
        let mut row_index = 0usize;
        let mut containing_row_index = None;
        let mut offset = 0usize;

        while offset < scan_end {
            let read_len = (scan_end - offset).min(ENTRY_MARKER_SCAN_CHUNK_BYTES);
            let chunk = reader.read_range(offset, read_len)?;
            let base = carry_base;
            let mut buffer = Vec::with_capacity(carry.len() + chunk.len());
            buffer.extend_from_slice(&carry);
            buffer.extend_from_slice(&chunk);

            let mut pos = find_marker(&buffer, &SSED_ENTRY_MARKER, 0);
            while let Some(marker_pos) = pos {
                let absolute_marker = base.saturating_add(marker_pos);
                let start = if marker_pos >= 2 && buffer[marker_pos - 2..marker_pos] == [0x1f, 0x02]
                {
                    absolute_marker.saturating_sub(2)
                } else {
                    absolute_marker
                };
                if emitted_start != Some(start) {
                    emitted_start = Some(start);
                    if start > target_offset {
                        return Ok(containing_row_index);
                    }
                    containing_row_index = Some(row_index);
                    row_index = row_index.saturating_add(1);
                }
                pos = find_marker(&buffer, &SSED_ENTRY_MARKER, marker_pos.saturating_add(1));
            }

            if buffer.len() >= tail_size {
                carry = buffer[buffer.len() - tail_size..].to_vec();
                carry_base = base + buffer.len() - tail_size;
            } else {
                carry = buffer;
                carry_base = base;
            }
            offset = offset.saturating_add(read_len);
        }

        Ok(containing_row_index)
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
                            SsedSidecarKind::TContents | SsedSidecarKind::Honbun => {
                                BodySourceKind::RendererDatabase
                            }
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
            discover_ssed_sidecar_body_resolvers_with_candidates(
                &self.root,
                inferred_folder_title(&self.root).as_deref(),
                &self.retained_ios_full_db_sidecar_paths(),
            )
            .map_err(|error| error.to_string())
        });
        match resolvers {
            Ok(resolvers) => Ok(resolvers.as_slice()),
            Err(error) => Err(Error::Driver(error.clone())),
        }
    }

    pub(super) fn ssed_sidecar_range_resolvers(&self) -> Result<&[SsedSidecarRangeResolver]> {
        let resolvers = self.ssed_sidecar_range_resolvers.get_or_init(|| {
            discover_ssed_sidecar_range_resolvers_with_candidates(
                &self.root,
                inferred_folder_title(&self.root).as_deref(),
                &self.retained_ios_full_db_sidecar_paths(),
            )
            .map_err(|error| error.to_string())
        });
        match resolvers {
            Ok(resolvers) => Ok(resolvers.as_slice()),
            Err(error) => Err(Error::Driver(error.clone())),
        }
    }

    pub(super) fn ssed_dense_anchor_at_component_offset(
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

    fn retained_ios_full_db_sidecar_paths(&self) -> Vec<PathBuf> {
        self.retained_ios_full_db_payloads
            .iter()
            .map(|payload| payload.absolute_path.clone())
            .collect()
    }
}

fn bounded_ssed_stream_length(
    catalog: &SsedCatalog,
    start_component: &SsedComponent,
    stream_offset: u64,
    end_block: u32,
    end_offset: u32,
) -> Option<u64> {
    let end_component = catalog.component_for_address(end_block)?;
    if !end_component
        .filename
        .eq_ignore_ascii_case(&start_component.filename)
    {
        return None;
    }
    let end_component_offset = end_component.relative_offset(end_block, end_offset)?;
    end_component_offset
        .checked_sub(stream_offset)
        .filter(|length| *length > 0)
}

fn ssed_address_sidecar_body_to_visual(body: SsedSidecarBody) -> VisualBody {
    if let Some(html) = body.html {
        return VisualBody::PreservedHtml {
            html,
            source: match body.resolver.kind {
                SsedSidecarKind::TContents | SsedSidecarKind::Honbun => {
                    BodySourceKind::RendererDatabase
                }
                _ => BodySourceKind::SidecarHtml,
            },
        };
    }
    VisualBody::PreservedHtml {
        html: sidecar_plain_text_to_html(&body.text),
        source: BodySourceKind::SidecarText,
    }
}

fn sidecar_plain_text_to_html(text: &str) -> String {
    let mut html = String::from("<div class=\"lvcore-sidecar-text\">");
    for (index, line) in text.lines().enumerate() {
        if index > 0 {
            html.push_str("<br>");
        }
        html.push_str(&escape_plain_label_html(line));
    }
    html.push_str("</div>");
    html
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

fn find_marker(data: &[u8], marker: &[u8], from: usize) -> Option<usize> {
    if marker.is_empty() || from >= data.len() {
        return None;
    }
    data[from..]
        .windows(marker.len())
        .position(|window| window == marker)
        .map(|relative| from + relative)
}

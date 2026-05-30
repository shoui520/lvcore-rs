use super::*;

impl ReaderBookPackage {
    pub(super) fn resolved_kind_for_body_target(
        &self,
        target: &TargetToken,
    ) -> Result<ResolvedTargetKind> {
        match target.decode()? {
            InternalTarget::LvedRow { table, .. } if table.eq_ignore_ascii_case("info") => {
                Ok(ResolvedTargetKind::InfoPage)
            }
            InternalTarget::LvedInfoPage { .. } => Ok(ResolvedTargetKind::InfoPage),
            InternalTarget::LvedNamedPage { .. } => Ok(ResolvedTargetKind::InfoPage),
            InternalTarget::HoureiLaw { .. } => Ok(ResolvedTargetKind::LawArticle),
            _ => Ok(ResolvedTargetKind::EntryBody),
        }
    }

    pub(super) fn title_for_body_target(&self, target: &TargetToken) -> Result<Option<String>> {
        match target.decode()? {
            InternalTarget::HoureiLaw { hore_id, .. } => {
                let Some(store) = &self.hourei_store else {
                    return Ok(None);
                };
                Ok(store
                    .law_entry(&hore_id)?
                    .map(|entry| hourei_law_node_label(&entry)))
            }
            _ => Ok(None),
        }
    }

    fn visual_body_for_ssed_address(
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
        if let Some(next_offset) =
            self.infer_next_ssed_index_body_offset(component, component_offset)
            && next_offset > component_offset
        {
            return Some(next_offset - component_offset);
        }
        ssed_find_next_entry_marker_offset(&mut reader, start.saturating_add(1))
            .ok()
            .flatten()
            .filter(|next| *next > start)
            .map(|next| (next - start) as u64)
    }

    fn infer_next_ssed_index_body_offset(
        &self,
        component: &SsedComponent,
        component_offset: u64,
    ) -> Option<u64> {
        let mut next_offset: Option<u64> = None;
        self.scan_ssed_simple_index_rows(None, |row| {
            let Some(row_component) = self
                .ssed_catalog
                .as_ref()
                .and_then(|catalog| catalog.component_for_address(row.body.block))
            else {
                return Ok(true);
            };
            if !row_component
                .filename
                .eq_ignore_ascii_case(&component.filename)
            {
                return Ok(true);
            }
            let Some(row_offset) = row_component.relative_offset(row.body.block, row.body.offset)
            else {
                return Ok(true);
            };
            if row_offset > component_offset
                && next_offset.is_none_or(|current| row_offset < current)
            {
                next_offset = Some(row_offset);
            }
            Ok(true)
        })
        .ok()?;
        next_offset
    }

    fn visual_body_for_ssed_dense_anchor(
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

    fn visual_body_for_lved_row(&self, table: &str, row_id: i64) -> Result<VisualBody> {
        if table.eq_ignore_ascii_case("info") {
            return self.visual_body_for_lved_info_row(row_id);
        }
        if !table.eq_ignore_ascii_case("content") {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 target table is not renderable yet".to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "lved_row_table_deferred",
                    format!("LVED_SQLITE3 table {table} is not a renderable content table"),
                )],
            });
        }
        let Some(store) = &self.lved_store else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 content targets require an opened SQLCipher store",
                )],
            });
        };
        let Some(html) = store.content_html(row_id)? else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 content row was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "lved_content_missing",
                    format!("LVED_SQLITE3 content row {row_id} was not found"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html,
            source: BodySourceKind::LvedSqlite,
        })
    }

    fn visual_body_for_lved_info_row(&self, row_id: i64) -> Result<VisualBody> {
        let Some(store) = &self.lved_store else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 info targets require an opened SQLCipher store",
                )],
            });
        };
        let Some(html) = store.info_html(row_id)? else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 info row was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "lved_info_missing",
                    format!("LVED_SQLITE3 info row {row_id} was not found"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html,
            source: BodySourceKind::LvedSqlite,
        })
    }

    fn visual_body_for_lved_info_name(&self, name: &str) -> Result<VisualBody> {
        let Some(store) = &self.lved_store else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 info targets require an opened SQLCipher store",
                )],
            });
        };
        let Some(html) = store.info_html_by_name(name)? else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 info page was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "lved_info_missing",
                    format!("LVED_SQLITE3 info page {name} was not found"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html,
            source: BodySourceKind::LvedSqlite,
        })
    }

    fn visual_body_for_lved_named_page(&self, table: &str, name: &str) -> Result<VisualBody> {
        let Some(store) = &self.lved_store else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "lved_store_missing",
                    "LVED_SQLITE3 named page targets require an opened SQLCipher store",
                )],
            });
        };
        let Some(html) = store.named_html_by_name(table, name)? else {
            return Ok(VisualBody::Unsupported {
                reason: "LVED_SQLITE3 named page was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "lved_named_page_missing",
                    format!("LVED_SQLITE3 {table} page {name} was not found"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html,
            source: BodySourceKind::LvedSqlite,
        })
    }

    fn visual_body_for_multiview_href(
        &self,
        href: &str,
        anchor: Option<&str>,
    ) -> Result<VisualBody> {
        let Some(store) = &self.multiview_store else {
            return Ok(VisualBody::Unsupported {
                reason: "LVLMultiView store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "multiview_store_missing",
                    "LVLMultiView targets require opened LogoFontCipher SQLite payloads",
                )],
            });
        };
        let lookup = anchor.unwrap_or(href);
        let Some(body) = store.body_for_href(lookup)? else {
            return Ok(VisualBody::Unsupported {
                reason: "LVLMultiView target was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "multiview_target_missing",
                    format!("LVLMultiView target {lookup} was not found in decoded payloads"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html: body.html,
            source: BodySourceKind::LvlMultiViewSqlite,
        })
    }

    fn visual_body_for_hourei_law(&self, hore_id: &str) -> Result<VisualBody> {
        let Some(store) = &self.hourei_store else {
            return Ok(VisualBody::Unsupported {
                reason: "Hourei store is unavailable".to_owned(),
                diagnostics: vec![Diagnostic::error(
                    "hourei_store_missing",
                    "Hourei law targets require an opened Hourei store",
                )],
            });
        };
        let Some(html) = store.law_html(hore_id)? else {
            return Ok(VisualBody::Unsupported {
                reason: "Hourei law body was not found".to_owned(),
                diagnostics: vec![Diagnostic::warning(
                    "hourei_law_missing",
                    format!("Hourei law {hore_id} was not found in cached HTML or law shard DB"),
                )],
            });
        };
        Ok(VisualBody::PreservedHtml {
            html,
            source: BodySourceKind::HoureiSqlite,
        })
    }
}

impl BodyProvider for ReaderBookPackage {
    fn visual_body_for_target(&self, token: &TargetToken) -> Result<VisualBody> {
        match token.decode()? {
            InternalTarget::SsedDenseAnchor {
                anchor,
                resolver_hint,
            } => self.visual_body_for_ssed_dense_anchor(&anchor, resolver_hint.as_deref()),
            InternalTarget::SsedAddress {
                component,
                block,
                offset,
            } => self.visual_body_for_ssed_address(&component, block, offset),
            InternalTarget::LvedRow {
                table,
                row_id,
                anchor: _,
                query: _,
            } => self.visual_body_for_lved_row(&table, row_id),
            InternalTarget::LvedInfoPage { name, anchor: _ } => {
                self.visual_body_for_lved_info_name(&name)
            }
            InternalTarget::LvedNamedPage {
                table,
                name,
                anchor: _,
            } => self.visual_body_for_lved_named_page(&table, &name),
            InternalTarget::MultiviewHref { href, anchor } => {
                self.visual_body_for_multiview_href(&href, anchor.as_deref())
            }
            InternalTarget::HoureiLaw { hore_id, anchor: _ } => {
                self.visual_body_for_hourei_law(&hore_id)
            }
            _ => Ok(VisualBody::Unsupported {
                reason: "body provider deferred".to_owned(),
                diagnostics: vec![Diagnostic::info(
                    "body_deferred",
                    "body provider is not implemented for this target",
                )],
            }),
        }
    }
}

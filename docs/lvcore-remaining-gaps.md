# LVCore Remaining Gaps

Date: 2026-06-12

Latest full-corpus gate:

- `/tmp/lvcore-all-corpora-validation-20260612-ios-panel-cache.jsonl`
- Produced after caching parsed SSED plist panel projections by source label and
  requested panel id, avoiding repeated iOS panel projection work during
  surface/render/window validation.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

Previous planning baseline:

- `/tmp/lvcore-all-corpora-validation-20260612-lved-fts-rowid-order.jsonl`
- Produced after changing LVED_SQLITE3 FTS list joins to order by the FTS
  virtual-table rowid instead of the joined `list.id`, avoiding temp B-tree
  sorts for broad CJK full-text searches.
- 336 packages validated with package status 336 `ok`.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260612-body-offset-cursor-skip.jsonl`
- Produced after bounding native SSED full-text body continuation validation:
  body byte-candidate lookup uses `memmem`, and deep validation no longer
  follows expensive `body-offset:*` full-text continuation cursors by default.
- 336 packages validated with package status 336 `ok`.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.
- `/tmp/lvcore-all-corpora-validation-20260612-ssed-partial-unverified-cursor.jsonl`
- Produced after changing large SSED partial-prefix pages to expose unverified
  non-prefix continuations instead of proving those continuations during
  first-page search.
- 336 packages validated: the previous 334-package corpus set plus two
  additional `Other/Android` packages discovered by the gate root set.
- Package-level status: 336 `ok`.
- `/tmp/lvcore-all-corpora-validation-20260612-html-attr-scanner.jsonl`
- Produced after fixing shared HTML attribute scanning for large LVED
  preserved-HTML pages and CHM/package-HTML pages with non-tag `<` text.
- `/tmp/lvcore-all-corpora-validation-20260612-ios-ssed-cross-book-routing.jsonl`
- Produced after routing iOS SSED cross-book validation targets through sibling
  packages without relying on reader-facing diagnostics.
- `/tmp/lvcore-all-corpora-validation-20260612-home-surface-diagnostic-cleanup.jsonl`
- Produced after removing available home-surface success diagnostics.
- `/tmp/lvcore-all-corpora-validation-20260612-navigation-diagnostic-cleanup.jsonl`
- Produced after removing stale success-path SSED navigation diagnostics.
- `/tmp/lvcore-all-corpora-validation-20260612-gaiji-helper-tightened.jsonl`
- Produced after tightening shared gaiji formatting-helper classification.
- `/tmp/lvcore-all-corpora-validation-20260612-sidecar-start-cursor.jsonl`
- Produced after the SSED sidecar body start cursor fix.
- `/tmp/lvcore-all-corpora-validation-20260612-title-prepass-row-cursor.jsonl`
- Produced after the SSED native title-prepass row cursor fix.

This is a working backlog, not a claim that lvcore is complete.

Target completion definition:

> lvcore can reliably open, search, browse, and render all non-HC LogoVista
> content needed by a reader/frontend.

HC visual rendering and frontend reader work remain intentionally deferred.

## Operating Rules

- Use the latest full-corpus JSONL as the baseline for choosing work.
- Do not treat validation itself as progress.
- Pick one concrete non-HC gap, inspect the affected real packages, write code,
  then run focused tests and focused package validation.
- Run full-corpus validation only before commit/push or after broad shared
  provider changes.
- Use logovista-tools as evidence for format facts when needed, not as an
  architecture template.

## Current State

Package opening and broad deep validation are green across the known corpus set.
No concrete non-HC warning-level blocker remains in the latest gate. The
remaining warning class is the explicitly deferred HC common HTML fallback.

Warning diagnostics in the baseline:

| Diagnostic | Count | Classification |
| --- | ---: | --- |
| `hc_render_common_html_fallback` | 1936 | Deferred HC visual rendering |
| `ssed_loose_address_unresolved` | 0 | Closed by packed SSED link-address normalization |

Important info/status classes from the latest gate:

| Marker | Count | Classification |
| --- | ---: | --- |
| `sidecar-body-row:*` cursor probed `ok` | 28 | Dense sidecar body cursor fix verified |
| `row:0` full-text cursor probed `ok` | 122 | Native title-prepass row cursor fix verified |
| `sidecar-body-start` cursor probed `ok` | 33 | Sidecar body phase start cursor fix verified |
| `sidecar-body:*` cursor `not_probed` | 0 | Closed by row/start/physical cursor split |
| `body-offset:*` cursor `not_probed` | 1 | Expensive native body continuation intentionally deferred |
| `ssed_fulltext_body_window_scan` | 0 | Closed by direct native HONMON scan fallback |
| `ssed_fulltext_body_direct_scan` | 10 | Direct native HONMON fallback exercised |
| `ssed_index_empty_physical_pages_skipped` | 0 | Closed by sparse partial-search cursor fix |
| `ssed-partial-nonprefix-unverified-index:*` cursor `not_probed` | 51 | Large-index partial-search continuation intentionally deferred |
| `lved_viewer_hook_deferred` | 214 info diagnostics plus deferred samples | Intentional external viewer policy |
| `gaiji_formatting_helper_candidate` | 36 | Observed OUKOKU11 `B947`/`B948` helper codes |
| `ssed_navigation_empty_sentinel` | 19 | Expected sentinel classification |
| `skipped_large_view` | 38 | Validator cap for large native HTML alternate mode |
| `no_resource`, `no_link`, `no_target` | many | Usually validator sample result, not a failure |

## Fix-Now / Recently Closed Candidates

### 0i. iOS SSED plist panel projection latency (resolved)

Why this matters:

- The latest full-corpus gate had no non-HC correctness failures, but it exposed
  a concrete iOS SSED panel latency gap in `Other/iOS/HABGESPA/HABGESPA`.
- The `ios-plist:sakuin.plist` `surface_first_target` exercise took about
  4.5s while opening the `Ａ` panel target, even though the row had no HC
  diagnostics and produced a normal panel surface.
- The package already cached the raw plist XML, but each requested panel id was
  re-projected into a fresh `SsedPanelXml` during surface open, render, and
  panel-window checks.

Current status:

- Parsed plist panel projections are now cached per package by plist source
  label and requested panel id.
- Callers still receive an owned `SsedPanelXml`, so iOS panel open can attach
  inferred BIN refs without mutating cached state.
- Referenced child plist panels use the same parsed-projection cache.
- Focused tests passed:
  - `cargo test -p lvcore ssed_panel -- --nocapture`
  - `cargo test -p lvcore ssed_ios -- --nocapture`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-habgespa-panel-cache.jsonl`
  - `HABGESPA` package status `ok`.
  - Package elapsed dropped from the baseline about 7.9s to about 4.6s.
  - `ios-plist:sakuin.plist` `surface_first_target` elapsed dropped from about
    4.5s to about 0.8s.
- Full-corpus validation gate:
  - `/tmp/lvcore-all-corpora-validation-20260612-ios-panel-cache.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback`.
  - Total gate wall time was about 574s.
  - In the full gate, `Other/iOS/HABGESPA/HABGESPA`
    `ios-plist:sakuin.plist` `surface_first_target` elapsed about 0.65s, and
    package elapsed was about 4.0s.

Baseline evidence:

- Baseline full-corpus JSONL:
  - `/tmp/lvcore-all-corpora-validation-20260612-lved-fts-rowid-order.jsonl`
- Baseline symptom:
  - `Other/iOS/HABGESPA/HABGESPA` `ios-plist:sakuin.plist`
    `surface_first_target` label `Ａ` elapsed about 4.5s.

Changed code area:

- `crates/lvcore/src/package/drivers.rs`
- `crates/lvcore/src/package/drivers/ssed_panel_surfaces.rs`

### 0h. LVED SQLCipher broad CJK full-text latency (resolved)

Why this matters:

- The latest full-corpus gate had no non-HC correctness failures, but it exposed
  a concrete LVED_SQLITE3 search latency gap: `_DCT_HBHYAKKA` spent about 6.1s
  in `search_full_text` for query `アイ`, plus about 1.4s probing cursor `1`.
- `_DCT_SSJPKOKU` showed the same pattern for query `あ`, with a roughly 4.2s
  first page and about 1.4s cursor probe.
- Direct SQL evidence on `_DCT_HBHYAKKA` showed the old query plan scanned the
  FTS virtual table and then used a temp B-tree for `order by l.id`, even though
  the join condition is `l.id = s.rowid`.

Current status:

- LVED_SQLITE3 FTS list joins now order by the FTS virtual-table rowid. This is
  semantically equivalent to `list.id` for those joined rows, but lets SQLite
  stream FTS rowid order without materializing a broad match set for sorting.
- The same rowid ordering is used for single-variant direct FTS queries and
  hiragana/katakana variant query pages before the variant merge step.
- Focused tests passed:
  - `cargo test -p lvcore searches_lved_list_rows_and_preserves_content_html -- --nocapture`
  - `cargo test -p lvcore lved_sqlite -- --nocapture`
- Direct real-package probes after the change:
  - `_DCT_HBHYAKKA`, query `アイ`, first page about 0.23s and cursor page about
    0.24s.
  - `_DCT_SSJPKOKU`, query `あ`, first page about 1.67s and cursor page about
    1.44s.
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-lved-fts-rowid-order.jsonl`
  - `_DCT_HBHYAKKA` package status `ok`; `search_full_text` elapsed about 37ms
    and cursor probe about 18ms.
  - `_DCT_SSJPKOKU` package status `ok`; `search_full_text` elapsed about 2.1s
    and cursor probe about 1.1s.
- Full-corpus validation gate:
  - `/tmp/lvcore-all-corpora-validation-20260612-lved-fts-rowid-order.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback`.
  - Total gate wall time was about 561s.
  - `_DCT_HBHYAKKA` package elapsed about 0.7s in the full gate, with
    `search_full_text` about 35ms and cursor probe about 17ms.
  - `_DCT_SSJPKOKU` remains the slowest LVED_SQLITE3 full-text sample but is
    improved from the previous gate: `search_full_text` about 2.2s and cursor
    probe about 1.1s.

Baseline evidence:

- Baseline full-corpus JSONL:
  - `/tmp/lvcore-all-corpora-validation-20260612-body-offset-cursor-skip.jsonl`
- Baseline symptoms:
  - `_DCT_HBHYAKKA` `search_full_text` query `アイ` elapsed about 6.1s; cursor
    probe about 1.4s.
  - `_DCT_SSJPKOKU` `search_full_text` query `あ` elapsed about 4.2s; cursor
    probe about 1.4s.

Changed code area:

- `crates/lvcore/src/lved_sqlite/sql_search.rs`

### 0g. Large SSED partial-search first-page latency (resolved)

Why this matters:

- The latest full-corpus gate had no non-HC correctness failures, but it did
  expose a concrete SSED search usability/performance gap: `_DCT_SAIYOREI`
  spent about 18.2 seconds in `search_partial`.
- The slow query was `〆を`. Forward/prefix search found the visible first hit
  quickly, but partial search then synchronously probed the large non-prefix
  contains continuation to prove whether more hits existed.
- SAIYOREI has about 31k supported index blocks, including a large
  `MUL1_1_2.DIC` index. Treating the continuation probe as first-page work made
  ordinary partial search appear stalled.

Current status:

- Large SSED partial-prefix pages now return the prefix hits immediately and
  expose an explicit unverified non-prefix continuation cursor:
  `ssed-partial-nonprefix-unverified-index:*`.
- The unverified cursor remains executable. If followed, it performs the same
  bounded non-prefix scan and then converts later matched-offset continuations
  back to visible physical-page anchors.
- Deep validation does not automatically probe this unverified continuation,
  because doing so turns an explicit next-page operation back into first-page
  validation latency.
- Direct `_DCT_SAIYOREI` partial search for `〆を` dropped from roughly 18-20
  seconds to about 20 ms for the first page.
- Focused tests passed:
  - `cargo test -p lvcore partial_nonprefix_cursors_preserve_prefix_skip_state -- --nocapture`
  - `cargo test -p lvcore ssed_partial_deferred_nonprefix_cursor_resumes_at_visible_physical_page -- --nocapture`
  - `cargo test -p lvcore ssed_partial_prefix_page_defers_large_nonprefix_cursor_without_visibility_probe -- --nocapture`
  - `cargo test -p lvcore ssed_partial_search_defers_nonprefix_fill_for_large_indexes -- --nocapture`
  - `cargo test -p lvcore-cli validate_search_cursor_probe_skips_expensive_fulltext_body_cursors -- --nocapture`
  - `cargo test -p lvcore-cli validate_deep_probes_ssed_partial_and_fulltext_by_default -- --nocapture`
  - `cargo test -p lvcore-cli validate_deep_exercises_ssed_advertised_search_modes -- --nocapture`
- Test note:
  - The broader `cargo test -p lvcore ssed_partial -- --nocapture` still hits
    the known pre-existing sparse partial-search architecture failure
    `ssed_partial_search_uses_physical_scan_cursor_for_sparse_indexes`.
    That failure predates this fix and remains outside this item.
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-saiyorei-unverified-partial.jsonl`
  - `_DCT_SAIYOREI` validated with package status `ok`.
  - Total focused validation wall time was about 2.9 seconds.
  - The `search_partial` exercise elapsed about 8 ms and reported
    `ssed-partial-nonprefix-unverified-index:0:0` with cursor probe status
    `not_probed`.
- Adjacent slow-package check:
  - `/tmp/lvcore-focused-validate-kqnewej6-unverified-partial.jsonl`
  - `_DCT_KQNEWEJ6` validated with package status `ok`.
  - Its `search_partial` exercise elapsed about 665 ms and also reported the
    unverified continuation with cursor probe status `not_probed`.
- Full-corpus validation gate:
  - `/tmp/lvcore-all-corpora-validation-20260612-ssed-partial-unverified-cursor.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback`.
  - The gate has 51
    `ssed-partial-nonprefix-unverified-index:*` cursors marked `not_probed`.
  - `_DCT_SAIYOREI` package elapsed about 3.0 seconds in the full gate, with
    `search_partial` about 9 ms.
  - `_DCT_KQNEWEJ6` package elapsed about 7.6 seconds in the full gate, with
    `search_partial` about 666 ms.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_SAIYOREI`
- Baseline full-corpus JSONL:
  - `/tmp/lvcore-all-corpora-validation-20260612-html-attr-scanner.jsonl`
- Baseline symptom:
  - `search_partial` elapsed about 18.2 seconds for query `〆を`.
  - The package-level validation elapsed about 21.3 seconds.

Changed code areas:

- `crates/lvcore/src/package/drivers/search_ssed.rs`
- `crates/lvcore-cli/src/validate.rs`

### 0f. Large LVED preserved-HTML info page validation latency (resolved)

Why this matters:

- The latest full-corpus gate had no non-HC correctness failures, but it did
  expose a concrete LVED usability/performance gap: `_DCT_GENIUSE6` spent
  roughly 50 seconds in the `info` surface exercise.
- The slow samples were very large preserved-HTML index pages such as
  `rank_d.html`, with about 7,000 `lved.dataid:` links and rendered HTML near
  1.8 MB.
- This affected real reader work because opening or validating those pages made
  normal non-HC browse/render behavior feel stalled even though output was
  correct.

Current status:

- The shared HTML `href`/`src`/`data` attribute scanner now walks forward by
  likely real tags and skips comments directly instead of reverse-searching
  from the start of the document for every attribute.
- The scanner ignores implausible `<` starts so package HTML with JavaScript or
  text comparisons does not degrade while probing CHM/hanrei pages.
- LVED link construction now reuses the already-created target token when
  building `TargetLink` records.
- Direct render of `_DCT_GENIUSE6` `info/rank_d.html` dropped from about 13
  seconds to about 1 second.
- Focused tests passed:
  - `cargo test -p lvcore package::html -- --nocapture`
  - `cargo test -p lvcore lved -- --nocapture`
  - `cargo test -p lvcore-cli validate_deep -- --nocapture`
  - `cargo test -p lvcore-cli -- --nocapture`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-geniuse6-tag-filter.jsonl`
  - `_DCT_GENIUSE6` validated with package status `ok`.
  - Total focused validation wall time was about 4.2 seconds.
  - The `info` surface exercise elapsed about 3.2 seconds, with
    `resource_scan` about 1.5 seconds and `link_scan` about 1.7 seconds.
  - The same large linked pages still render as `info_page` with zero target
    diagnostics in the focused link scan.
- Focused CHM/package-HTML regression validation passed:
  - `/tmp/lvcore-focused-validate-sinmei7-tag-filter.jsonl`
  - `_DCT_SINMEI7` validated with package status `ok`.
  - The Windows SSED `hanrei` surface exercise elapsed about 13 ms after the
    scanner began skipping implausible `<` starts.
- Full-corpus validation gate:
  - `/tmp/lvcore-all-corpora-validation-20260612-html-attr-scanner.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 334-package baseline path set is fully covered; the gate also
    includes two additional `Other/Android` packages.
  - Warning diagnostics remain only `hc_render_common_html_fallback`.
  - `_DCT_GENIUSE6` package elapsed about 3.9 seconds in the full gate.
  - `_DCT_SINMEI7` package elapsed about 5.3 seconds in the full gate.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SQLCIPHER_DICTS_WINDOWS/_DCT_GENIUSE6`
- Baseline full-corpus JSONL:
  - `/tmp/lvcore-all-corpora-validation-20260612-ios-ssed-cross-book-routing.jsonl`
- Baseline symptom:
  - `info` surface exercise elapsed about 50 seconds.
  - `resource_scan` elapsed about 24 seconds.
  - `link_scan` elapsed about 26 seconds.

Changed code areas:

- `crates/lvcore/src/package/html.rs`
- `crates/lvcore/src/package/drivers/lved_render_refs.rs`

### 0e. iOS SSED cross-book validation routing context (resolved)

Why this matters:

- The latest gate after home-surface diagnostic cleanup exposed one real
  validator-context gap: iOS `KQNEWJE5` tableList rows target sibling SSED
  dictionary `KQNEWEJ6`, but deep package validation no longer opened that
  sibling after success diagnostics were removed.
- The underlying library routing path already supports SSED cross-book targets
  when the sibling package is open.
- The validator was incorrectly coupled to a reader-facing
  `HomeSurface.diagnostics` marker as its internal sibling-discovery signal.

Current status:

- Deep validation now probes available surface targets, decodes
  `SsedCrossBookAddress` tokens, and opens matching sibling packages before the
  exercise pass.
- This keeps reader-facing home surfaces clean while preserving validation
  coverage for iOS tableList cross-book rows.
- Focused test passed:
  - `cargo test -p lvcore-cli validate_deep_routes_ios_table_list_cross_book_sibling -- --nocapture`
- Focused cross-book regression tests passed:
  - `cargo test -p lvcore-cli cross_book -- --nocapture`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-ios-ssed-cross-book-target-routing.jsonl`
  - `KQNEWJE5` validates with package status `ok`.
  - `ios-table-list:tableList.plist` routes to
    `SSED:KQNEWEJ6:*`, renders as `entry_body`, and emits
    `ssed_cross_book_routed`.
  - `ssed_cross_book_destination_missing` and `ssed_cross_book_deferred` are
    zero.
- Full-corpus validation gate:
  - `/tmp/lvcore-all-corpora-validation-20260612-ios-ssed-cross-book-routing.jsonl`
  - 334 packages validated with package status 334 `ok`.
  - Baseline warnings remain only `hc_render_common_html_fallback`.
  - `ssed_cross_book_destination_missing` and `ssed_cross_book_deferred` are
    zero in the baseline diagnostic scope.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/Other/iOS/KQNEWJE5/KQNEWJE5`
- Destination package:
  - `/home/shoui/Agents/CodexMax/LogoVista/Other/iOS/KQNEWEJ6/KQNEWEJ6`
- Observed row:
  - `surface_id`: `ios-table-list:tableList.plist`
  - label: `United States`
  - target: `HONMON.DIC:231605:1770` in dictionary `KQNEWEJ6`

Changed code area:

- `crates/lvcore-cli/src/validate.rs`

### 0d. Available home-surface success diagnostic noise (resolved)

Why this matters:

- The previous full-corpus gate had 278 available home-surface info diagnostics
  that duplicated `kind`, `status`, `surface_id`, and `target` metadata.
- These were successful reader-facing navigation surfaces, not degraded
  behavior.
- Keeping them in `HomeSurface.diagnostics` made normal SSED/iOS/SIZK/Hourei
  browse support look incomplete.

Current status:

- Available home surfaces no longer emit success-path diagnostics for menu,
  encyclopedia, auxiliary, iOS plist/list/menu, EXINFO, SIZK read-aloud, and
  Hourei kana-panel surfaces.
- Empty/deferred/error diagnostics remain intact, including
  `ssed_navigation_empty_sentinel`.
- Focused tests passed:
  - `cargo test -p lvcore ssed_exinfo_aux_html_idxinfo_exposes_package_html_surface -- --nocapture`
  - `cargo test -p lvcore ssed_ios_extra_plist_surfaces_are_first_class_navigation -- --nocapture`
  - `cargo test -p lvcore library_routes_ios_ssed_table_list_cross_book_addresses_through_sibling_aliases -- --nocapture`
  - `cargo test -p lvcore ssed_sizk_read_aloud_surface_renders_playback_with_audio_resource -- --nocapture`
  - `cargo test -p lvcore ssed_screen_menu_surface_exposes_backgrounds_and_hotspot_targets -- --nocapture`
  - `cargo test -p lvcore ssed_encyclopedia_index_opens_as_navigation_tree -- --nocapture`
  - `cargo test -p lvcore hourei_law_tree_search_body_links_and_sequence_are_backend_owned -- --nocapture`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-home-surface-success-diagnostic-cleanup.jsonl`
  - 10 affected package samples validated with package status 10 `ok`.
  - Removed success diagnostics were zero across both home-surface and concrete
    diagnostic checks; the only remaining home diagnostic was one expected
    `ssed_navigation_empty_sentinel`.
- Full-corpus home-surface diagnostic cleanup gate:
  - `/tmp/lvcore-all-corpora-validation-20260612-home-surface-diagnostic-cleanup.jsonl`
  - 334 packages validated with package status 334 `ok`.
  - Surface diagnostics now contain only 18
    `ssed_navigation_empty_sentinel` classifications.
  - The removed success diagnostics are zero concrete diagnostics.

Baseline evidence:

- Previous full-corpus surface diagnostic counts:
  - `ssed_auxiliary_index`: 113
  - `ssed_ios_dictlist_other`: 35
  - `ssed_auxiliary_html`: 34
  - `ssed_sizk_read_aloud`: 30
  - `ssed_ios_fulldb_list`: 27
  - `ssed_ios_plist_panel`: 20
  - `ssed_exinfo_index_url`: 7
  - `ssed_numeric_auxiliary_index`: 4
  - `ssed_ios_app_menu`: 2
  - `ssed_ios_html_list`: 2
  - `ssed_screen_menu`: 1
  - `ssed_encyclopedia_index`: 1
  - `hourei_kana_panel`: 1
  - `ssed_ios_table_list`: 1
  - `ssed_ios_table_list_cross_book`: 1

### 0c. SSED auxiliary virtual-selector success diagnostic noise (resolved)

Why this matters:

- The previous full-corpus gate had three
  `ssed_auxiliary_index_virtual_selector` info diagnostics, all from
  `_DCT_ZYAKUKOG`.
- Those rows are successful auxiliary-index routes into package panel targets,
  not degraded behavior.
- Keeping success-path routing evidence as reader-facing diagnostics made normal
  auxiliary navigation look like a gap.

Current status:

- Auxiliary-index virtual selectors still route through `PanelCell` targets when
  `Panels.xml` is present.
- The diagnostic remains for the real gap case where a virtual selector exists
  but no panel metadata is available.
- Focused tests passed:
  - `cargo test -p lvcore ssed_numeric_auxiliary_index_routes_virtual_selectors_without_success_noise -- --nocapture`
  - `cargo test -p lvcore ssed_numeric_auxiliary_index_opens_without_exinfo -- --nocapture`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-zyakukog-aux-selector-cleanup.jsonl`
  - `_DCT_ZYAKUKOG` validated with package status `ok` and zero concrete
    `ssed_auxiliary_index_virtual_selector` diagnostics.
- Full-corpus navigation regression gate:
  - `/tmp/lvcore-all-corpora-validation-20260612-navigation-diagnostic-cleanup.jsonl`
  - 334 packages validated with package status 334 `ok`.
  - `ssed_auxiliary_index_virtual_selector` is now zero concrete diagnostics.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_ZYAKUKOG`
- Observed successful panel IDs: `16000000`, `17000000`, `18000000`.

### 0b. SSED title-index surface partial diagnostic noise (resolved)

Why this matters:

- `surface_partial` was emitted for every decodable SSED title/index browse
  surface, even though the browse surface itself is available, cursor-paged,
  and reader-facing.
- The diagnostic text described conservative search-provider internals, not a
  limitation of opening or browsing the title/index surface.
- Keeping it on the home surface made working SSED browse support look
  partially implemented in reader-facing metadata.

Current status:

- Decodable SSED title/index home surfaces are now exposed as available without
  `surface_partial` diagnostics.
- The title/index browse implementation and target resolution behavior are
  unchanged.
- Focused tests passed:
  - `cargo test -p lvcore ssed_title_index_home_surface_is_available_without_partial_diagnostic_noise -- --nocapture`
  - `cargo test -p lvcore ssed_simple_title_index_surface_resolves_entry_targets -- --nocapture`
  - `cargo fmt --check`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-25igaku-surface-partial-cleanup.jsonl`
  - `_DCT_25IGAKU` validated with package status `ok` and zero concrete
    `surface_partial` diagnostics.
- Full-corpus navigation regression gate:
  - `/tmp/lvcore-all-corpora-validation-20260612-navigation-diagnostic-cleanup.jsonl`
  - 334 packages validated with package status 334 `ok`.
  - `surface_partial` is now zero concrete diagnostics.

Baseline evidence:

- Previous full-corpus gate
  `/tmp/lvcore-all-corpora-validation-20260612-gaiji-helper-tightened.jsonl`
  had 186 concrete `surface_partial` info diagnostics, all on SSED packages.

### 0a. Gaiji formatting helper classification overbreadth (resolved)

Why this matters:

- The latest gate had 16 `gaiji_formatting_helper_candidate` info markers,
  all from Android/iOS OUKOKU11.
- LVCore had been classifying any unbacked full-width `B***` gaiji as a
  nonliteral helper, but the corpus evidence only supported the observed
  OUKOKU11 helper pair `B947`/`B948`.
- Overbroad classification could hide a real unresolved full-width gaiji in
  labels/search/navigation output.

Current status:

- The shared gaiji provider now treats only observed helper codes `B947` and
  `B948` as `nonliteral_marker`.
- Normal unbacked full-width gaiji such as `B123` remain visible unresolved
  markers, preserving the reader/debug signal instead of silently suppressing
  them.
- This is provider-level gaiji classification only; HC visual/profile fallback
  behavior remains deferred.
- Focused tests passed:
  - `cargo test -p lvcore gaiji -- --nocapture`
  - `cargo test -p lvcore ssed_basic_text_uses_logovista_gaiji_placeholders_for_unresolved_stream_pairs -- --nocapture`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-oukok11-gaiji-helper-tightened.jsonl`
  - `/tmp/lvcore-focused-validate-ios-oukok11-gaiji-helper-tightened.jsonl`
  - Both OUKOKU11 packages validated with package status `ok`.
- Full-corpus shared-provider regression gate:
  - `/tmp/lvcore-all-corpora-validation-20260612-gaiji-helper-tightened.jsonl`
  - 334 packages validated with package status 334 `ok`.
  - Warning profile unchanged: only deferred `hc_render_common_html_fallback`
    at 965 concrete diagnostics.
  - `gaiji_formatting_helper_candidate` remains limited to 16 concrete info
    diagnostics in the two OUKOKU11 packages.

Baseline evidence:

- Packages:
  - `/home/shoui/Agents/CodexMax/LogoVista/LogoVistaAndroid/SSED/.OUKOKU11`
  - `/home/shoui/Agents/CodexMax/LogoVista/Other/iOS/OUKOKU11/OUKOKU11`
- Observed helper messages were for `B947` and `B948` only.
- No `logovista-tools` source match was found for `B947`/`B948`, so the LVCore
  fix is deliberately corpus-observation-scoped rather than copied from HC
  renderer logic.

### 0. Gaiji policy selected-source rendering (resolved)

Why this matters:

- The architecture requires gaiji preference order to be a runtime user setting,
  not a display hardcode.
- `GaijiResolution` intentionally keeps fallback data, for example Unicode plus
  Template/GA16 resource refs, but rich-label rendering must still honor the
  selected `preferred_source`.
- Before this fix, an explicit `Unresolved` preference could still display a
  Unicode fallback in rich labels.

Current status:

- `resolve_rich_label` now renders `GaijiSourcePreference::Unresolved` as the
  unresolved marker/span even when Unicode fallback data is present.
- Package gaiji diagnostics distinguish policy-selected unresolved gaiji from
  genuinely missing backing data.
- Existing fallback-retention semantics are preserved: a resolution may still
  carry Unicode/resource fallbacks for inspection and alternate policy choices.
- Focused tests passed:
  - `cargo test -p lvcore gaiji -- --nocapture`
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
- Full-corpus shared-provider regression gate:
  - `/tmp/lvcore-all-corpora-validation-20260612-gaiji-policy.jsonl`
  - 334 packages validated with package status 334 `ok`.
  - Warning profile unchanged: only deferred `hc_render_common_html_fallback`.

Baseline evidence:

- This was source/contract-backed rather than a warning class in the latest
  full-corpus JSONL.
- Relevant architecture note: gaiji policy can reorder Unicode, Template, GA16,
  and unresolved priority across labels and surfaces.

### 1. SSED dense sidecar full-text continuation performance (resolved)

Why this matters:

- Legacy full-text sidecar cursors were blanket skipped by validation as
  `not_probed`.
- `sidecar-body:N` was a matched-result offset, not a physical resolver/row
  cursor.
- For non-authoritative prefilter queries, continuation could rescan a large
  sidecar table from the beginning to skip matched rows.

Current status:

- LVCore now emits physical `sidecar-body-row:*` cursors for dense sidecar body
  hits.
- Legacy `sidecar-body:N` cursors are still accepted and convert to physical
  cursors after the next body hit.
- Validator cursor policy now probes physical `sidecar-body-row:*` cursors while
  continuing to skip legacy matched-offset `sidecar-body:*` cursors.
- Full-corpus gate
  `/tmp/lvcore-all-corpora-validation-20260612-sidecar-row-cursor.jsonl`
  validated 334 packages with package status 334 `ok`.
- The gate found 27 `sidecar-body-row:*` cursors, all with cursor probe status
  `ok`.
- At that gate, the old `sidecar-body:1` `not_probed` bucket was gone and 154
  legacy `sidecar-body:0` title-prepass phase cursors remained skipped.
- Focused real-package probes passed:
  - `_DCT_PROYAL53`, query `ひゃ`: first page and physical continuation returned
    quickly with `ssed_fulltext_sidecar_scan`.
  - iOS `IBIO5`, query `亜-`: first page and physical continuation returned
    quickly with `ssed_fulltext_sidecar_scan`.
  - iOS `KENCOLLO`, query `ab`: sidecar-title prepass returned legacy
    `sidecar-body:0`; continuation returned a physical `sidecar-body-row:*`
    cursor quickly.

Baseline evidence:

- 181 `not_probed` sidecar body cursors:
  - 154 `sidecar-body:0`
  - 27 `sidecar-body:1`
- The old cursor prefix was overloaded:
  - `ssed_fulltext_sidecar_scan` rows are true dense-sidecar body cursor work.
  - `ssed_fulltext_sidecar_title_prepass` rows are sidecar-title prepass
    continuations that enter the sidecar body phase.
  - `ssed_fulltext_title_index_prepass` rows are native title/index prepass
    continuations; these belong to the native full-text body-scan gap below.
- `_DCT_EJJE100`, query `co`, is now classified as native title/index
  prepass-to-body-phase work, not a dense-sidecar continuation issue.

Likely code area:

- `crates/lvcore/src/ssed_sidecar.rs`
- `search_ssed_dense_sidecar_bodies_with_resolvers`
- `search_ssed_dense_sidecar_bodies_prefiltered`
- `crates/lvcore/src/package/drivers/search_ssed.rs`
- `crates/lvcore-cli/src/validate.rs`

### 2. SSED native title-prepass full-text phase cursor (resolved)

Why this matters:

- The previous full-corpus baseline still had 154 `sidecar-body:0` full-text
  cursor probes marked `not_probed`.
- These cursors came from native title/index prepass pages, not dense sidecar
  body result pages.
- When the query can prove there are no dense sidecar body hits, the next phase
  can safely be the native row-driven HONMON body cursor instead of the legacy
  sidecar phase cursor.

Current status:

- Native SSED full-text title/index prepass now emits `row:0` after title hits
  when there are no sidecar body resolvers.
- If sidecar body resolvers exist, LVCore first uses the dense sidecar SQL
  prefilter only for queries where that prefilter is authoritative. If it proves
  there are no sidecar body hits, the prepass emits `row:0`.
- When a sidecar body phase has an initial SQL-prefiltered hit, LVCore emits the
  probe-safe `sidecar-body-start` cursor so the dense sidecar body phase still
  runs first without using the legacy matched-offset cursor.
- Legacy `sidecar-body:0` remains accepted for compatibility and as a fallback
  when no safe start cursor can be established.
- Focused real-package probes passed:
  - `_DCT_25IGAKU`, query `カー`: first page now returns `row:0`.
  - `_DCT_45KAGAKU`, query `0`: first page now returns `row:0`.
  - `_DCT_GENKANA5`, query `01`: native title-prepass now returns
    `sidecar-body-start`.
  - iOS `KENCOLLO`, query `ab`: sidecar-title prepass now returns
    `sidecar-body-start`.
  - iOS `Dconci98`, query `A`: native title-prepass now returns
    `sidecar-body-start`.
- Focused validation passed:
  - `/tmp/lvcore-focused-validate-25igaku-row-cursor.jsonl`
  - `/tmp/lvcore-focused-validate-45kagaku-row-cursor.jsonl`
  - Both packages validated with package status `ok`, zero warnings/errors, and
    `search_full_text.cursor_probe.status` `ok` for cursor `row:0`.
  - `/tmp/lvcore-focused-validate-sidecar-start-affected-32.jsonl`
  - The 32-package affected set validated with package status 32 `ok`; all 32
    `sidecar-body-start` probes had status `ok`.
- Full-corpus gate
  `/tmp/lvcore-all-corpora-validation-20260612-sidecar-start-cursor.jsonl`
  validated 334 packages with package status 334 `ok`.
- The gate has 122 `row:0` full-text cursor probes, all with status `ok`.
- The gate has 32 `sidecar-body-start` full-text cursor probes, all with status
  `ok`.
- No `sidecar-body:*` cursor remains `not_probed`.

Baseline evidence:

- The previous full-corpus baseline had 154 `sidecar-body:0` `not_probed` cursor
  probes.
- Representative rows were native `ssed_fulltext_title_index_prepass`
  continuations for `_DCT_25IGAKU` and `_DCT_45KAGAKU`.

Changed code area:

- `crates/lvcore/src/package/drivers/search_ssed.rs`
- `crates/lvcore/src/package/drivers.rs`
- `crates/lvcore/src/ssed_sidecar.rs`
- `crates/lvcore/src/package/drivers/tests/dense_sidecar.rs`
- `crates/lvcore/src/package/drivers/tests/fulltext.rs`

Done criteria:

- Emit a probeable native body cursor for native title-prepass continuation
  pages when doing so cannot skip dense sidecar body results.
- Preserve sidecar body phase ordering when sidecar body hits may exist.
- Verify with focused synthetic tests and focused real-package validation.

### 3. SSED native full-text first-page body scan cost (resolved)

Why this matters:

- Commit `386b714` fixed native HONMON continuation cursor cost for KENE7J5.
- The first page can still require a broad body-window scan.
- Native title/index prepass continuations still use the legacy
  `sidecar-body:0` phase cursor even when no dense body sidecar exists.
- A correct search page taking tens of seconds is not acceptable reader UX if it
  appears in common workflows.

Current status:

- Legacy `sidecar-body:*` phase cursors now allow the existing bounded
  row-driven native body prefetch before falling through to broad HONMON
  body-window scanning.
- Focused `_DCT_EJJE100` probe, query `co`, cursor `sidecar-body:0`, now returns
  quickly with `ssed_fulltext_row_driven_body_prefetch` and a `row:1`
  continuation instead of entering `ssed_fulltext_body_window_scan`.
- LVCore now has a direct HONMON byte-window scan fallback for native full-text
  first pages. When row-driven prefetch misses and a byte-window match can be
  anchored to an SSED entry marker, it returns a renderable `SsedAddress` hit
  without the expensive index-remap pass.
- Focused probes after the direct-scan change:
  - `_DCT_GEN2005`, query `曙光`: about 8.4s before, about 3.6s after; now uses
    `ssed_fulltext_body_direct_scan`.
  - `_DCT_KENE7J5`, query `は殺`: about 29.7s before, about 4.3s after; now uses
    `ssed_fulltext_body_direct_scan` and preserves `body-offset:*`
    continuation.
  - `_DCT_NCOMP4`, query `1計`: about 7.0s before, about 5.1s after; no hit, now
    exits through direct byte-window scan without index remap.
- Residual tradeoff: direct body hits use body-derived labels rather than native
  index titles when the expensive index-remap path is skipped.
- The direct HONMON byte-candidate lookup now uses `memmem` instead of a
  byte-window equality loop, preserving the earliest candidate after the
  requested start offset while reducing broad scan overhead.
- Deep validation no longer automatically follows native `body-offset:*`
  full-text continuations. Those cursors remain executable next-page cursors,
  but probing them by default reintroduced multi-second body scans as validation
  work rather than first-page coverage.
- Focused `_DCT_KENE7J5` validation after the `body-offset:*` probe policy
  change:
  - `/tmp/lvcore-focused-validate-kene7j5-body-offset-skip.jsonl`
  - Package status `ok`; total wall time about 4.1s.
  - `search_full_text` query `は殺` elapsed about 3.6s, returned one hit, and
    preserved `body-offset:484f4e4d4f4e2e444943:f6534`.
  - The continuation cursor was reported as `not_probed` with reason
    `body full-text continuation cursors may rescan large SSED body windows`.
- Full-corpus gate
  `/tmp/lvcore-all-corpora-validation-20260612-native-direct-scan.jsonl`
  validated 334 packages with package status 334 `ok`.
- The gate has no remaining `ssed_fulltext_body_window_scan` diagnostics.
- Full-corpus gate after the `body-offset:*` probe policy change:
  - `/tmp/lvcore-all-corpora-validation-20260612-body-offset-cursor-skip.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback`.
  - The gate has one `body-offset:*` cursor, marked `not_probed`.
  - `_DCT_KENE7J5` package elapsed about 4.0s; `search_full_text` elapsed about
    3.5s and retained the same continuation cursor.

Latest-gate packages with `ssed_fulltext_body_window_scan` before the direct
scan change:

- `_DCT_GEN2005`
- `_DCT_KENE7J5`
- `_DCT_NCOMP4`

Known example:

- `_DCT_KENE7J5`, query from validation, first page previously took roughly 30s.
- Its continuation uses `body-offset:*`; the cursor remains executable but is no
  longer followed by default deep validation.

Likely code area:

- `crates/lvcore/src/package/drivers/search_ssed.rs`
- `ssed_fulltext_body_hit_ranges`
- row-driven prefetch before broad HONMON scans

Done criteria:

- Reduce first-page broad scan cost for the affected package class without
  changing search semantics.
- Preserve the current physical `body-offset:*` continuation behavior.
- Add focused synthetic regression only after reproducing the real package
  behavior.

### 4. iOS HKKIGAK6 sparse partial-search native index cursor (resolved)

Why this matters:

- This is the remaining `ssed_index_empty_physical_pages_skipped` class.
- It is not HC and not LVED.
- It is probably performance/cursor quality rather than missing results.

Current status:

- SSED partial non-prefix scans now use a larger leaf-page batch only when byte
  prefilter candidates exist. Whitespace/no-prefilter queries keep the smaller
  bounded scan budget.
- Matched-offset continuations now preserve the first visible physical row when
  the sparse scan finds more than one hit, so HKKIGAK6 emits
  `ssed-partial-nonprefix-noskip-physical-offset:*` instead of a generic
  matched-offset cursor.
- Focused HKKIGAK6 validation
  `/tmp/lvcore-focused-validate-hkkigak6-partial-prefilter.jsonl` validated the
  Windows and iOS HKKIGAK6 packages with package status 2 `ok`.
- Full-corpus gate
  `/tmp/lvcore-all-corpora-validation-20260612-hkkigak6-partial-prefilter.jsonl`
  validated 334 packages with package status 334 `ok`.
- The gate has no remaining `ssed_index_empty_physical_pages_skipped` or
  `ssed_index_empty_physical_scan_limited` diagnostics.

Baseline evidence:

- Package: `/home/shoui/Agents/CodexMax/LogoVista/Other/iOS/HKKIGAK6/HKKIGAK6`
- Count: 2 diagnostics
- Diagnostic contexts:
  - `advanced_empty_pages=7`, next cursor
    `ssed-partial-nonprefix-noskip-index:5:40`
  - `advanced_empty_pages=2`, next cursor
    `ssed-partial-nonprefix-noskip-index:5:80`

Important prior finding:

- The iOS SQLite `DictSearchDB` and `DictFULLDB` search tables do not exactly
  replace native SSED partial title/index semantics for this query class.
- Do not force basic partial search to SQLite unless the semantics are proven.

Likely code area:

- `crates/lvcore/src/package/drivers/search_ssed.rs`
- `crates/lvcore/src/package/drivers/ssed_index.rs`
- `crates/lvcore/src/package/drivers/ssed_ios_search.rs`

Done criteria:

- Reduce or eliminate empty physical page advances for HKKIGAK6 partial search.
- Preserve native title/index partial search semantics.
- Focused validation only on HKKIGAK6 until commit gate.

### 5. KOJIEN6 loose SSED address warning (resolved)

Why this matters:

- It is the only non-HC warning-level diagnostic left.
- It may represent an unresolved link target, or it may be a package-authored
  sentinel/address pattern that needs classification.

Baseline evidence:

- Package: `_DCT_KOJIEN6`
- Diagnostic: `ssed_loose_address_unresolved`
- Address: `00640000:0064`
- Count: 6

Current status:

- KOJIEN6 common stream links can encode a valid HONMON block in the high word of
  the decoded block value. The observed `00640000:0064` link normalizes to block
  `100`, offset `100`, which is inside the declared HONMON component.
- LVCore now normalizes that packed address form before loose SSED target
  resolution, yielding a normal `ssed_address` target link instead of an
  unresolved-address warning.
- Focused KOJIEN6 validation
  `/tmp/lvcore-focused-validate-kojien6-packed-link.jsonl` validated the package
  with status `ok` and zero `ssed_loose_address_unresolved` diagnostics.
- Full-corpus gate
  `/tmp/lvcore-all-corpora-validation-20260612-kojien6-packed-link.jsonl`
  validated 334 packages with package status 334 `ok`.
- The gate has no remaining `ssed_loose_address_unresolved` diagnostics.

Changed code area:

- `crates/lvcore/src/package/drivers/renderer.rs`
- `crates/lvcore/src/package/drivers/tests/ssed_renderer_input.rs`

Done criteria:

- Identify the source of the loose address in the real package.
- If it is a real non-HC target, route it.
- If it is a sentinel or HC/profile-only reference, classify it explicitly so it
  stops looking like an unresolved warning.

## Accepted Or Deferred

### HC visual/profile rendering

Deferred by project policy.

Relevant markers:

- `hc_render_common_html_fallback`
- `hc_renderer_input_ready`
- `hc_basic_text_visual_incomplete`
- `hc_basic_text_gaiji_placeholders`
- `ssed_renderer_resource_scan_deferred`

These should not drive LVCore work until HC architecture is intentionally
resumed.

### LVED viewer hooks

Deferred by policy unless product requirements change.

Current behavior:

- Viewer hooks are preserved as non-executed targets.
- Validation reports `lved_viewer_hook_deferred`.

Do not execute external viewer hooks inside lvcore.

### Empty navigation sentinels

Expected classification.

Current behavior:

- Some SSED `MENU.DIC` entries are explicit empty/null destinations.
- They should not become clickable broken targets.
- `ssed_navigation_empty_sentinel` is evidence of classification, not a gap.

### Large alternate generic HTML validation skips

Mostly validator policy.

Current behavior:

- Some native views are too large for the generic HTML alternate render probe.
- The validator marks `skipped_large_view` with
  `native_display_html_too_large`.

This is not currently a reader-facing failure unless a frontend needs those
alternate render modes for very large pages.

### `no_resource`, `no_link`, and `no_target`

Usually not gaps by themselves.

Current behavior:

- `no_resource`: sampled target has no extractable resources.
- `no_link`: sampled target has no links.
- `no_target`: sampled search window had no first target for that mode/query.

Only investigate these when attached to a user-facing workflow that should have
resources, links, or targets.

## Completion Checklist By Area

### Package detection/opening

Current status:

- All 336 known packages in the baseline open and deep-validate at package
  status `ok`.

Known gap:

- None from the latest baseline.

### Search

Current status:

- Exact, forward, backward, partial, full-text, advanced SQLite/iOS, SIZK, and
  cross-book routed paths have broad coverage.

Known gaps:

- None from the latest baseline.

### Browse/navigation surfaces

Current status:

- Hanrei, title-index, menu, auxiliary index, panel, multi-selector, LVED tree,
  LVED list, iOS plist/list/menu, SIZK read-aloud, and Hourei kana panel surfaces
  are represented.

Known gap:

- No concrete non-HC blocker from the latest baseline.

### Rendering/resources

Current status:

- Non-HC preserved HTML, generic HTML resource rewriting, BasicText fallback,
  resources, template images, and package-local links are broadly covered.

Known gaps:

- HC visual rendering is intentionally deferred.
- Large alternate render validation is capped.
- No concrete non-HC rendering blocker from the latest baseline.

### Cross-book and external targets

Current status:

- SSED and LVED cross-book routes are represented in diagnostics.
- LVED viewer hooks are preserved but not executed.

Known gap:

- None unless viewer hooks become in scope.

# LVCore Remaining Gaps

Date: 2026-06-12

Latest full-corpus gate:

- `/tmp/lvcore-all-corpora-validation-20260612-home-surface-diagnostic-cleanup.jsonl`
- Produced after removing available home-surface success diagnostics.
- 334 packages validated.
- Package-level status: 334 `ok`.

Previous planning baseline:

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
| `hc_render_common_html_fallback` | 431 | Deferred HC visual rendering |
| `ssed_loose_address_unresolved` | 0 | Closed by packed SSED link-address normalization |

Important info/status classes from the latest gate:

| Marker | Count | Classification |
| --- | ---: | --- |
| `sidecar-body-row:*` cursor probed `ok` | 27 | Dense sidecar body cursor fix verified |
| `row:0` full-text cursor probed `ok` | 122 | Native title-prepass row cursor fix verified |
| `sidecar-body-start` cursor probed `ok` | 32 | Sidecar body phase start cursor fix verified |
| `sidecar-body:*` cursor `not_probed` | 0 | Closed by row/start/physical cursor split |
| `ssed_fulltext_body_window_scan` | 0 | Closed by direct native HONMON scan fallback |
| `ssed_fulltext_body_direct_scan` | 5 | Direct native HONMON fallback exercised |
| `ssed_index_empty_physical_pages_skipped` | 0 | Closed by sparse partial-search cursor fix |
| `lved_viewer_hook_deferred` | 188 info diagnostics plus deferred samples | Intentional external viewer policy |
| `gaiji_formatting_helper_candidate` | 16 | Observed OUKOKU11 `B947`/`B948` helper codes |
| `ssed_navigation_empty_sentinel` | 18 | Expected sentinel classification |
| `skipped_large_view` | 38 | Validator cap for large native HTML alternate mode |
| `no_resource`, `no_link`, `no_target` | many | Usually validator sample result, not a failure |

## Fix-Now / Recently Closed Candidates

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
- Full-corpus gate
  `/tmp/lvcore-all-corpora-validation-20260612-native-direct-scan.jsonl`
  validated 334 packages with package status 334 `ok`.
- The gate has no remaining `ssed_fulltext_body_window_scan` diagnostics.

Latest-gate packages with `ssed_fulltext_body_window_scan` before the direct
scan change:

- `_DCT_GEN2005`
- `_DCT_KENE7J5`
- `_DCT_NCOMP4`

Known example:

- `_DCT_KENE7J5`, query from validation, first page previously took roughly 30s.
- Its continuation uses `body-offset:*` and validates successfully.

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

- All 334 known packages in the baseline open and deep-validate at package
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

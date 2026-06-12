# LVCore Remaining Gaps

Date: 2026-06-12

Baseline artifact:

- `/tmp/lvcore-all-corpora-validation-20260612-body-cursor.jsonl`
- Produced after commit `386b714` (`Resume SSED fulltext body cursors physically`)
- 334 packages validated
- Package-level status: 334 `ok`

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
Remaining work is mostly in feature depth, performance, classification policy,
and explicitly deferred rendering areas.

Warning diagnostics in the baseline:

| Diagnostic | Count | Classification |
| --- | ---: | --- |
| `hc_render_common_html_fallback` | 965 | Deferred HC visual rendering |
| `ssed_loose_address_unresolved` | 6 | Non-HC candidate, needs triage |

Important info/status classes:

| Marker | Count | Classification |
| --- | ---: | --- |
| `sidecar-body:*` cursor `not_probed` | 181 | Real SSED performance/cursor candidate |
| `ssed_fulltext_body_window_scan` | 5 | Real SSED full-text performance candidate |
| `ssed_index_empty_physical_pages_skipped` | 2 | Real SSED/iOS partial-search candidate |
| `lved_viewer_hook_deferred` | 188 info diagnostics plus deferred samples | Intentional external viewer policy |
| `ssed_navigation_empty_sentinel` | 18 | Expected sentinel classification |
| `skipped_large_view` | 38 | Validator cap for large native HTML alternate mode |
| `no_resource`, `no_link`, `no_target` | many | Usually validator sample result, not a failure |

## Fix-Now Candidates

### 1. SSED dense sidecar full-text continuation performance

Why this matters:

- Full-text sidecar cursors are currently blanket skipped by validation as
  `not_probed`.
- Most sampled cursor probes are cheap, but broad ASCII queries can be very
  expensive.
- A direct probe of `_DCT_EJJE100` query `co`, cursor `sidecar-body:0`, did not
  finish within roughly two minutes before the interrupted turn.

Baseline evidence:

- 181 `not_probed` sidecar body cursors:
  - 154 `sidecar-body:0`
  - 27 `sidecar-body:1`
- Slow first-page examples from the baseline, all returning a sidecar body
  cursor:
  - `_DCT_EJJE100`, query `co`, elapsed 516 ms in validation, direct cursor
    probe was much worse.
  - `_DCT_HKDKSR13`, query `FU`/fullwidth FU, elapsed around 1204 ms.
  - `_DCT_HKDKSR30`, query fullwidth FU, elapsed around 1144 ms.
  - `_DCT_YHOUGO3`, query from validator, elapsed around 1255 ms.
  - iOS `IBIO5`, cursor `sidecar-body:1`, elapsed around 387 ms.

Likely code area:

- `crates/lvcore/src/ssed_sidecar.rs`
- `search_ssed_dense_sidecar_bodies_with_resolvers`
- `search_ssed_dense_sidecar_bodies_prefiltered`
- `crates/lvcore/src/package/drivers/search_ssed.rs`
- `crates/lvcore-cli/src/validate.rs`

Hypothesis:

- `sidecar-body:N` is a matched-result offset, not a physical resolver/row
  cursor.
- For non-authoritative prefilter queries, continuation can rescan a large
  sidecar table from the beginning to skip matched rows.

Done criteria:

- Introduce a physical sidecar body cursor that can resume by resolver identity
  and row position or stable row id.
- Keep existing `sidecar-body:N` accepted for compatibility.
- Validate with focused real packages:
  - `_DCT_EJJE100`
  - `_DCT_HKDKSR13`
  - `_DCT_YHOUGO3`
  - iOS `IBIO5`
- Update validator policy so physical sidecar cursors are probed.
- Do not run full corpus until the commit gate.

### 2. SSED native full-text first-page body scan cost

Why this matters:

- Commit `386b714` fixed native HONMON continuation cursor cost for KENE7J5.
- The first page can still require a broad body-window scan.
- A correct search page taking tens of seconds is not acceptable reader UX if it
  appears in common workflows.

Baseline packages with `ssed_fulltext_body_window_scan`:

- `_DCT_GEN2005`
- `_DCT_KENE7J5`
- `_DCT_KQDENTAL`
- `_DCT_KQNEWEJ6`
- `_DCT_NCOMP4`

Known example:

- `_DCT_KENE7J5`, query from validation, first page still takes roughly 30s.
- Its continuation now uses `body-offset:*` and validates successfully.

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

### 3. iOS HKKIGAK6 sparse partial-search native index cursor

Why this matters:

- This is the remaining `ssed_index_empty_physical_pages_skipped` class.
- It is not HC and not LVED.
- It is probably performance/cursor quality rather than missing results.

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

### 4. KOJIEN6 loose SSED address warning

Why this matters:

- It is the only non-HC warning-level diagnostic left.
- It may represent an unresolved link target, or it may be a package-authored
  sentinel/address pattern that needs classification.

Baseline evidence:

- Package: `_DCT_KOJIEN6`
- Diagnostic: `ssed_loose_address_unresolved`
- Address: `00640000:0064`
- Count: 6

Likely code area:

- `crates/lvcore/src/package/drivers/html_resource_render.rs`
- `crates/lvcore/src/package/drivers/ssed_navigation.rs`
- SSED loose media/address helpers

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

- SSED dense sidecar full-text continuation performance.
- SSED native HONMON full-text first-page cost on a small package set.
- iOS HKKIGAK6 sparse native partial-search cursor behavior.

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

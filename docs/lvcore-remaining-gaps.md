# LVCore Remaining Gaps

Date: 2026-06-14

Latest full-corpus gate:

- `/tmp/lvcore-all-corpora-validation-20260614-fulltext-nonprefix-title-proof-v1.jsonl`
- Produced after the current scoped LVCore SSED changes:
  - exact/forward/backward dense sidecar-title searches prove one extra row for
    authoritative multi-character queries and emit verified
    `sidecar-title-row:*` cursors when safe;
  - title-label fallback pages do bounded post-hit lookahead and emit verified
    `ssed-title-label:*` cursors only when a next distinct hit is found within
    the existing 256-row post-hit budget;
  - sidecar-avoidance native probes use a private internal start cursor instead
    of treating the internal `0` offset as an external continuation;
  - SSED menu address pages preserve address scope in continuation cursors;
  - the native partial-prefix prepass shares the normal prefix-page completion
    path;
  - partial-prefix page completion now proves bounded non-prefix continuation
    visibility, emitting verified `ssed-partial-nonprefix-index:*` cursors or no
    cursor instead of broad unverified start cursors;
  - SSED partial non-prefix proof now skips prefix-matching rows inside the
    index collector and only uses in-memory page prefilter extension when an
    anchor is longer than the minimum prefilter length; and
  - full-text non-prefix title continuation proof now verifies fast mixed
    CJK+kana exhaustion, while keeping slow digit+CJK continuation proof
    deferred.
- 335 reconstructed package paths validated with package status 335 `ok`.
  The gate used the 333 currently discovered documented-root package paths plus
  the two explicit `Other/Android` package rows.
- Format-family coverage: 1 `hourei`, 23 `lvl_multi_view`, 87 `lved_sqlite3`,
  and 224 `ssed` packages.
- Reconstructed path-list file:
  `/tmp/lvcore-full-paths-20260613-native-first-kana.txt`
- Reconstructed path-list hash:
  `b3c7c347c4587d6e3b29db8767748f2ebd9402b17f712dd5ae980d45cc5a601a`.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback, 816 aggregate warning entries in this gate.
- `sidecar-title-unverified-row:*` search rows dropped from 43 in the previous
  full gate to 19. Remaining rows are intentionally broad single-character
  exact/forward/backward title continuations.
- `ssed-title-label-unverified:*` direct/nested search rows dropped from 38 in
  the previous full gate to 27; 8 title-label rows now emit verified
  `ssed-title-label:*` cursors with successful cursor probes.
- `ssed-partial-nonprefix-unverified-index:*` rows dropped from 22 in the
  previous full gate to 0. The latest gate has 29
  `ssed-partial-nonprefix-index:*` cursors, all with cursor probe `ok`.
- `title-nonprefix-unverified:*` full-text rows dropped from 2 to 1. The
  latest gate proves `_DCT_YHOUGO3` full-text `一ス` has no remaining
  non-prefix title continuation and falls through to `body:0`; `_DCT_NCOMP4`
  full-text `1計` remains intentionally deferred.
- `_DCT_GKBUSINE` `search_forward` `10` is 86 ms and now returns
  `ssed-title-label:3` with cursor probe `ok` at 4 ms.
- The SSED partial-prefix/non-prefix proof latency cluster improved without
  changing cursor semantics: `_DCT_HKDKSR13` partial `AC` is 935 ms, down from
  1271 ms; `_DCT_HKDKSR14` partial `AC` is 778 ms, down from 1027 ms;
  `_DCT_HKDKSR29` partial `AC` is 747 ms, down from 972 ms; `_DCT_HKDKSR30`
  partial `AC` is 783 ms, down from 1054 ms; and `_DCT_KQCOLEXP` partial
  `ビー` is 737 ms, down from 1121 ms. All retain verified
  `ssed-partial-nonprefix-index:*` cursors with cursor probe `ok`.
- `_DCT_NANMED20` `search_exact` `0歳平均余命` returns a verified
  `sidecar-title-row:*` cursor with cursor probe `ok` at 80 ms; iOS
  `NANMED20` is also verified with cursor probe `ok` at 56 ms.
- `_DCT_IWKOKUG8` `search_forward` `さん` returns a verified
  `sidecar-title-row:*` cursor with cursor probe `ok` at 3 ms; iOS `IWKOKUG8`
  is also `ok` at 4 ms.
- `_DCT_NCOMP4` `search_full_text` `1計` is 412 ms with the
  same `0-1計画法` first hit and deferred `title-nonprefix-unverified:*`
  continuation. The standard deep validator does not currently exercise the
  direct `_DCT_NCOMP4` partial `1計` query; that query is covered by focused
  direct probes below.
- Digit-prefix controls stayed stable: `_DCT_GKBUSINE` partial `10` is 38 ms
  with cursor probe `ok` at 13 ms, `_DCT_GEN2011` partial `01` now returns a
  verified `ssed-partial-nonprefix-index:3:8` cursor in 262 ms with cursor
  probe `ok` at 37 ms, and `_DCT_GEN2009` partial `0` is 129 ms with cursor
  probe `ok` at 61 ms.
- Windows `_DCT_KENROWA` `search_partial` `しめ` is 20 ms and its cursor probe
  is `ok` at 10 ms.
- The `ssed_partial_native_prefix_fast_prepass` info diagnostic appears on
  8 pure-kana partial rows in the full gate. Continuation cursors are wrapped
  as `ssed-partial-prefix:*`, not raw native offsets.
- Deferred cursor probes in the latest gate are currently concentrated in:
  209 native offset continuations, 124 body full-text continuations, 27
  title-label fallback continuations, 22 sidecar-title continuations, and
  1 full-text non-prefix title continuation.
- Existing exact visible-title and digit/no-alpha full-text improvements
  remained stable: `_DCT_KQNEWEJ6` exact `画像一覧` is 15 ms,
  `_DCT_KQNEWJE5` exact `外字一覧` is 17 ms, and `_DCT_GKBUSINE` full-text
  `10` is 28 ms.

Focused validation for the latest full-corpus gate:

- Focused validation:
  `/tmp/lvcore-focused-validate-partial-nonprefix-proof-v1.jsonl`
  covered the 22 packages that returned
  `ssed-partial-nonprefix-unverified-index:0:0` in the previous full gate, with
  package status 22 `ok`.
- The old deep-validation reason
  `unverified partial non-prefix continuation may scan large SSED indexes`
  dropped to 0 in that focused set.
- Verified cursor examples: `_DCT_GEN2011` partial `01` now returns
  `ssed-partial-nonprefix-index:3:8` with cursor probe `ok`; `_DCT_HKDKSR13`
  partial `AC` returns `ssed-partial-nonprefix-index:11:82` with cursor probe
  `ok`; `_DCT_NIHONSHI` partial `〆売` proves exhaustion and returns no cursor.
- Focused validation warnings remained only the explicitly deferred HC common
  HTML fallback, 139 aggregate warning entries.
- Focused latency validation:
  `/tmp/lvcore-focused-validate-partial-prefix-nonprefix-proof-latency-v1.jsonl`
  covered 7 SSED packages with package status 7 `ok`.
- Target rows improved while preserving verified continuations:
  `_DCT_HKDKSR13` partial `AC` is 954 ms focused and 935 ms in the full gate,
  `_DCT_HKDKSR14` partial `AC` is 738 ms focused and 778 ms in the full gate,
  `_DCT_HKDKSR29` partial `AC` is 712 ms focused and 747 ms in the full gate,
  `_DCT_HKDKSR30` partial `AC` is 749 ms focused and 783 ms in the full gate,
  and `_DCT_KQCOLEXP` partial `ビー` is 702 ms focused and 737 ms in the full
  gate.
- Focused latency-validation warnings remained only the explicitly deferred HC
  common HTML fallback.
- Focused full-text non-prefix title validation:
  `/tmp/lvcore-focused-validate-fulltext-nonprefix-title-proof-v1.jsonl`
  covered `_DCT_YHOUGO3` and `_DCT_NCOMP4` with package status 2 `ok`.
- `_DCT_YHOUGO3` full-text `一ス` now returns `body:0` in 156 ms in focused
  validation instead of `title-nonprefix-unverified:*`; `_DCT_NCOMP4`
  full-text `1計` remains `title-nonprefix-unverified:*` in 414 ms because its
  direct cursor proof still takes about 1.6s.

Focused validation after the current LVCore change:

- Target gap from the latest full-corpus JSONL: SSED dense sidecar title rows
  that returned `sidecar-title-unverified-row:*` cursors even for bounded
  multi-character exact/forward/backward title queries.
- Code change: exact/forward/backward sidecar title lookahead now proves one
  additional row for authoritative multi-character queries, but keeps broad
  single-character non-ASCII queries deferred. Partial behavior is unchanged.
- A broader exact-mode version was rejected during focused validation because
  `_DCT_DAIJIRN4` exact `あ` needed about 1.7s to prove cursor exhaustion.
  With the final scoped rule, `_DCT_DAIJIRN4` exact `あ` stays at 13 ms and
  keeps its intentional `sidecar-title-unverified-row:*` cursor.
- Focused tests passed:
  - `cargo test -p lvcore sidecar_title_lookahead_only_defers_broad_single_char_queries`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-sidecar-title-lookahead-multichar-v1.jsonl`
  - 21 affected packages validated with package status 21 `ok`.
  - `sidecar-title-unverified-row:*` rows in that affected set dropped from
    43 in the latest full-corpus baseline to 19 focused rows; the remaining
    rows are intentionally broad single-character exact/forward/backward
    queries.
  - `_DCT_NANMED20` exact `0歳平均余命` now returns a verified
    `sidecar-title-row:*` cursor and its cursor probe is `ok` at 81 ms.
  - iOS `NANMED20` exact `0歳平均余命` now returns a verified
    `sidecar-title-row:*` cursor and its cursor probe is `ok` at 57 ms.
  - `_DCT_IWKOKUG8` forward `さん` now returns a verified
    `sidecar-title-row:*` cursor and its cursor probe is `ok` at 3 ms; iOS
    `IWKOKUG8` is also `ok` at 3 ms.
  - iOS `OUKOKU11` forward `スリ` now returns a verified
    `sidecar-title-row:*` cursor and its cursor probe is `ok` at 2 ms.
- Second target gap from the latest full-corpus JSONL: title-label fallback
  pages returned `ssed-title-label-unverified:*` even when a bounded post-hit
  lookahead could prove that a next distinct title-label hit exists.
- Code change: title-label fallback now continues after filling the requested
  page only within the existing 256-row post-hit budget. If another distinct
  hit is found, the page emits a normal `ssed-title-label:*` cursor; if not, it
  keeps the existing unverified cursor.
- Focused tests passed:
  - `cargo test -p lvcore title_label -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-title-label-lookahead-v1.jsonl`
  - 32 affected packages validated with package status 32 `ok`.
  - `ssed-title-label-unverified:*` rows in that affected set dropped from
    38 in the latest full-corpus baseline to 27 focused rows, with 8 newly
    probeable `ssed-title-label:*` rows.
  - `_DCT_GKBUSINE` forward `10` now returns `ssed-title-label:3`; its cursor
    probe is `ok` at 4 ms.
  - `_DCT_KQNEWEJ6` backward `一覧` now returns `ssed-title-label:1`; its
    cursor probe is `ok` at 16 ms. `_DCT_IWKOKUG8` exact `さんディー` remains
    intentionally unverified because no next hit was proved within the bounded
    lookahead.
- Third target gap from the latest full-corpus JSONL: the native-first probe
  used before dense sidecar title search set cursor `0` internally, which made
  the native index path treat the probe as an external continuation and emit
  `ssed-offset-unverified:*` even for specific long-kana exact queries.
- Code change: the sidecar-avoidance native probe now uses a private internal
  start cursor. External/user native offset cursors still force deferred
  overfetch, and broad one/two-character native queries still use the existing
  large-index defer predicate.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore native_internal_start_cursor_does_not_count_as_external_continuation`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-native-internal-start-cursor-v1.jsonl`
  - 6 packages validated with package status 6 `ok`: `_DCT_PROYAL53`,
    `_DCT_JSSAURU2`, iOS `JSSAURU2`, `_DCT_KENROWA`, `_DCT_IBIO4`, and
    `_DCT_KENCOLLO`.
  - The three long-kana exact rows from the latest full-corpus baseline now
    prove exhaustion and return no cursor: `_DCT_PROYAL53` exact
    `ひゃくとうばん`, `_DCT_JSSAURU2` exact `あぎと`, and iOS `JSSAURU2`
    exact `あぎと`.
  - Broad controls remain intentionally deferred: `_DCT_PROYAL53` forward
    `ひゃ` still returns `ssed-offset-unverified:3`, `_DCT_KENROWA` exact
    `しめ` still returns `ssed-offset-unverified:1`, and `_DCT_IBIO4` exact
    `蓚酸` still returns `ssed-offset-unverified:1`.
- Fourth target gap from the latest full-corpus JSONL: SSED menu target
  rendering for `SINMEI7` was slow enough to stand out, and direct inspection
  showed a correctness issue in the address-based menu target path. Address
  cursors such as `addr:23203:310` produced continuation cursors like `4` or
  `link:11:53` without preserving the address scope, so following the cursor
  reopened the root MENU stream instead of continuing the selected address
  subtree.
- Code change: SSED menu address cursors now decode into an address scope plus
  an optional page cursor, and address-scoped pages encode continuations as
  `addr:block:offset:<page-cursor>`. Root menu cursors are unchanged.
- Related local blocker fixed: the native partial-prefix prepass now shares the
  same prefix-page finishing logic as the normal native path. Large-index
  prefix hits now expose an explicit deferred non-prefix cursor instead of
  silently ending the partial result stream or proving a broad non-prefix page
  during prefix-page completion.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore ssed_menu_address_continuation_preserves_address_scope`
  - `cargo test -p lvcore ssed_partial_deferred_nonprefix_cursor_resumes_at_visible_physical_page`
  - `cargo test -p lvcore ssed_partial_prefix_page_defers_large_nonprefix_cursor_without_visibility_probe`
  - `cargo test -p lvcore ssed_navigation_surfaces`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-ssed-menu-address-and-partial-prefix-v1.jsonl`
  - 6 affected packages validated with package status 6 `ok`: Windows
    `_DCT_SINMEI7`, iOS `SINMEI7`, `_DCT_NCOMP4`, `_DCT_GKBUSINE`,
    `_DCT_GEN2011`, and `_DCT_GEN2009`.
  - Direct Windows `_DCT_SINMEI7` probes now preserve address scope:
    `menu --cursor addr:23203:310 --limit 16` returns `addr:23203:310:4`,
    and render of the `あ行` target returns `addr:23203:310:link:11:53`.
  - Direct iOS `SINMEI7` probes now preserve address scope:
    `menu --cursor addr:20914:1698 --limit 16` returns
    `addr:20914:1698:link:0:16`, and render of the same address target returns
    `addr:20914:1698:link:1:12`.
  - Following those scoped cursors stays inside the selected address stream;
    for example Windows `_DCT_SINMEI7` `addr:23203:310:link:11:53` returns the
    next `あ行` entry labels rather than root MENU rows.
  - Direct `_DCT_NCOMP4` partial `1計 --limit 1` still returns `0-1計画法`
    and now exposes
    `ssed-partial-nonprefix-noskip-unverified-physical-offset:8:121:1`,
    preserving the existing large-index deferred non-prefix continuation.
- Focused validation controls stayed stable: `_DCT_GKBUSINE` partial `10`
    returns `ssed-partial-nonprefix-noskip-offset:1`, `_DCT_GEN2011` partial
    `01` returns `ssed-partial-nonprefix-unverified-index:0:0`, and
    `_DCT_GEN2009` partial `0` returns `ssed-partial-prefix:1`.
- Fifth target gap from the latest full-corpus JSONL: 22 SSED partial search
  rows returned `ssed-partial-nonprefix-unverified-index:0:0`, so deep
  validation intentionally skipped the continuation even though direct probes
  showed bounded proof was practical for the affected packages.
- Code change: partial-prefix page completion now uses the bounded non-prefix
  visibility proof path. If a visible non-prefix continuation exists, the first
  page returns a verified `ssed-partial-nonprefix-index:*` cursor; if the
  bounded proof reaches exhaustion, it returns no cursor.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore ssed_partial_deferred_nonprefix_cursor_resumes_at_visible_physical_page`
  - `cargo test -p lvcore ssed_partial_prefix_page_proves_no_large_nonprefix_continuation`
  - `cargo test -p lvcore ssed_navigation_surfaces`
  - `cargo test -p lvcore native_internal_start_cursor_does_not_count_as_external_continuation`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-partial-nonprefix-proof-v1.jsonl`
  - 22 affected packages validated with package status 22 `ok`.
  - The old `ssed-partial-nonprefix-unverified-index:*` cursor class dropped to
    0 in the focused set.
  - Former affected rows now either emit verified
    `ssed-partial-nonprefix-index:*` cursors with cursor probe `ok`, or prove
    exhaustion and return no cursor.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260614-partial-nonprefix-proof-v1.jsonl`
  - 335 package statuses `ok`.
  - Warning diagnostics remain only the explicitly deferred HC common HTML
    fallback, 816 aggregate warning entries.
  - The old `ssed-partial-nonprefix-unverified-index:*` cursor class is 0 in the
    full gate; 29 `ssed-partial-nonprefix-index:*` cursors are probeable and all
    have cursor probe status `ok`.
- Sixth target gap from the latest full-corpus JSONL: several SSED
  partial-prefix pages had correct verified `ssed-partial-nonprefix-index:*`
  continuations but spent about 1.0-1.3s proving visibility for common short
  ASCII or kana anchors.
- Code change: the partial non-prefix collector now owns prefix skipping, so a
  row's body/display keys are computed once for both "skip prefix" and "match
  partial" decisions. It also skips already-seen targets before expensive
  display-label matching when the current offset does not need that target.
  The SSED page-prefilter extension now avoids full expanded-index scans for
  minimum-length anchors such as `AC`, keeping those cases on the sequential
  paged scan path.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore ssed_navigation_surfaces`
  - `cargo test -p lvcore native_internal_start_cursor_does_not_count_as_external_continuation`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-partial-prefix-nonprefix-proof-latency-v1.jsonl`
  - 7 affected packages validated with package status 7 `ok`: `_DCT_HKDKSR10`,
    `_DCT_HKDKSR13`, `_DCT_HKDKSR14`, `_DCT_HKDKSR29`, `_DCT_HKDKSR30`,
    `_DCT_KQCOLEXP`, and `_DCT_YHOUGO4`.
  - `_DCT_HKDKSR13` partial `AC` is 954 ms with cursor probe `ok` at 77 ms,
    down from 1271 ms in the previous full gate; `_DCT_HKDKSR14` partial `AC`
    is 738 ms, down from 1027 ms; `_DCT_HKDKSR29` partial `AC` is 712 ms, down
    from 972 ms; `_DCT_HKDKSR30` partial `AC` is 749 ms, down from 1054 ms; and
    `_DCT_KQCOLEXP` partial `ビー` is 702 ms, down from 1121 ms.
  - `_DCT_HKDKSR10` partial `AB` stayed roughly flat at 426 ms because the
    remaining cost is dominated by initial prefix work, not non-prefix proof.
- Full-corpus regression gate passed because this touched shared SSED index
  scan/collector behavior:
  - `/tmp/lvcore-all-corpora-validation-20260614-partial-prefix-nonprefix-proof-latency-v1.jsonl`
  - 335 package statuses `ok`.
  - Warning diagnostics remain only the explicitly deferred HC common HTML
    fallback, 816 aggregate warning entries.
  - The full gate still has 0 `ssed-partial-nonprefix-unverified-index:*`
    cursors and 29 probeable `ssed-partial-nonprefix-index:*` cursors, all with
    cursor probe status `ok`.
- Seventh target gap from the latest full-corpus JSONL: the only remaining
  full-text non-prefix title continuation deferrals were `_DCT_YHOUGO3`
  full-text `一ス` and `_DCT_NCOMP4` full-text `1計`. Direct cursor probes
  showed `_DCT_YHOUGO3` could prove exhaustion quickly, while `_DCT_NCOMP4`
  still needed about 1.6s and should remain deferred.
- Code change: full-text non-prefix title continuation proof now runs a bounded
  visibility/exhaustion probe only for mixed CJK+kana title-prepass queries.
  Digit+CJK and punctuation cases continue to return the existing
  `title-nonprefix-unverified:*` cursor.
- Focused tests passed:
  - `cargo fmt`
  - `cargo test -p lvcore fulltext_nonprefix_title -- --nocapture`
  - `cargo test -p lvcore fulltext_title_cursors_preserve_physical_offset -- --nocapture`
  - `cargo build -p lvcore-cli`
- Direct real-package probes:
  - `_DCT_YHOUGO3` full-text `一ス --limit 1` returns `body:0` in about 0.18s;
    following `body:0` returns a body hit in about 0.10s.
  - `_DCT_NCOMP4` full-text `1計 --limit 1` still returns
    `title-nonprefix-unverified:*` in about 0.49s; following that cursor still
    takes about 1.6s, so it remains intentionally deferred.
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-fulltext-nonprefix-title-proof-v1.jsonl`
  - 2 affected/control packages validated with package status 2 `ok`:
    `_DCT_YHOUGO3` and `_DCT_NCOMP4`.
  - `_DCT_YHOUGO3` full-text `一ス` is 156 ms through
    `ssed_fulltext_partial_nonprefix_title_prepass` and now returns `body:0`;
    `_DCT_NCOMP4` full-text `1計` remains deferred at 414 ms.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260614-fulltext-nonprefix-title-proof-v1.jsonl`
  - 335 package statuses `ok`.
  - Warning diagnostics remain only the explicitly deferred HC common HTML
    fallback, 816 aggregate warning entries.
  - `title-nonprefix-unverified:*` full-text rows dropped from 2 to 1; body
    full-text continuations rose from 123 to 124 because `_DCT_YHOUGO3` now
    falls through to the body phase.

Previous full-corpus gates:

- `/tmp/lvcore-all-corpora-validation-20260614-partial-prefix-nonprefix-proof-latency-v1.jsonl`
- Produced after integrating partial non-prefix prefix skipping into the SSED
  index collector and avoiding expanded-index page prefilter extension for
  minimum-length anchors such as `AC`.
- 335 reconstructed package paths validated with package status 335 `ok`.
- Warning diagnostics remained only the explicitly deferred HC common HTML
  fallback, 816 aggregate warning entries.
- `ssed-partial-nonprefix-unverified-index:*` remained 0; the gate had 29
  probeable `ssed-partial-nonprefix-index:*` cursors, all with cursor probe
  status `ok`.
- This gate became the baseline for the full-text non-prefix title proof work:
  `_DCT_YHOUGO3` full-text `一ス` still returned
  `title-nonprefix-unverified:*`, and `_DCT_NCOMP4` full-text `1計` remained
  intentionally deferred.

- `/tmp/lvcore-all-corpora-validation-20260614-partial-nonprefix-proof-v1.jsonl`
- Produced after making partial-prefix page completion prove bounded
  non-prefix continuation visibility, emitting verified
  `ssed-partial-nonprefix-index:*` cursors or no cursor instead of broad
  unverified start cursors.
- 335 reconstructed package paths validated with package status 335 `ok`.
- Warning diagnostics remained only the explicitly deferred HC common HTML
  fallback, 816 aggregate warning entries.
- The old `ssed-partial-nonprefix-unverified-index:*` cursor class was 0 in the
  full gate; 29 `ssed-partial-nonprefix-index:*` cursors were probeable and all
  had cursor probe status `ok`.
- This gate became the baseline for the subsequent partial-prefix/non-prefix
  proof latency work: `_DCT_HKDKSR13` partial `AC` was 1271 ms,
  `_DCT_HKDKSR14` partial `AC` was 1027 ms, `_DCT_HKDKSR29` partial `AC` was
  972 ms, `_DCT_HKDKSR30` partial `AC` was 1054 ms, and `_DCT_KQCOLEXP`
  partial `ビー` was 1121 ms.

- `/tmp/lvcore-all-corpora-validation-20260614-ssed-menu-address-partial-prefix-v1.jsonl`
- Produced after preserving SSED menu address scope in continuation cursors and
  routing the native partial-prefix prepass through the normal prefix-page
  completion path.
- 335 reconstructed package paths validated with package status 335 `ok`.
- Warning diagnostics remained only the explicitly deferred HC common HTML
  fallback, 816 occurrences.
- The gate still had 22 intentionally unprobed
  `ssed-partial-nonprefix-unverified-index:*` continuations; those are closed by
  the latest partial non-prefix proof gate.

- `/tmp/lvcore-all-corpora-validation-20260614-gkbusine-forward-title-label-fast-prepass-v1.jsonl`
- Produced after making no-cursor SSED forward digit/no-ASCII-letter title
  queries eligible for the bounded title-label fast prepass when a cheap
  one-leaf native forward-hit probe misses. Real native prefix hits keep the
  existing native path; empty native prefix probes can return early visible
  title-label hits without paying for the full native miss and broad fallback.
- 335 reconstructed package paths validated with package status 335 `ok`.
- Warning diagnostics remained only the explicitly deferred HC common HTML
  fallback, 949 occurrences.
- `_DCT_GKBUSINE` `search_forward` `10` was 83 ms, down from 439 ms in the
  previous full gate and about 0.70s in direct probes before the fix. It
  preserved the same first three hits and continuation
  `ssed-title-label-unverified:3`, through `ssed_title_label_fast_prepass`.

- `/tmp/lvcore-all-corpora-validation-20260613-ncomp4-partial-prefix-fast-miss-gate-v1.jsonl`
- Produced after the focused `_DCT_NCOMP4` partial `1計` fix. The fix keeps
  empty initial partial-prefix pages from surfacing to callers, defers large
  initial non-prefix partial continuation proof behind an explicit unverified
  physical-offset cursor, and applies the existing digit/no-ASCII-letter
  one-leaf prefix-hit probe before expensive fallback prefix scans on large
  packages.
- 335 reconstructed package paths validated with package status 335 `ok`.
- Warning diagnostics remained only the explicitly deferred HC common HTML
  fallback, 949 occurrences.
- `_DCT_NCOMP4` `search_full_text` `1計` was 446 ms with the same
  `0-1計画法` first hit and deferred `title-nonprefix-unverified:*`
  continuation.
- `_DCT_GKBUSINE` forward `10` remained a concrete candidate at 439 ms.

- `/tmp/lvcore-all-corpora-validation-20260613-partial-digit-prefix-fast-probe-v1.jsonl`
- Produced after making SSED partial-search native prefix probing conditional
  for no-cursor single-token digit/no-ASCII-letter queries. These queries run a
  cheap one-leaf native forward-hit probe first: real digit prefix hits keep the
  existing partial prefix path, while empty native prefix probes skip the
  expensive miss and fall through to the existing partial title-index path.
- 335 reconstructed package paths validated with package status 335 `ok`.
- Warning diagnostics remained only the explicitly deferred HC common HTML
  fallback.
- `_DCT_GKBUSINE` `search_partial` `10` was 37 ms, down from 428 ms in the
  previous full gate. It preserved the same first hit and
  `ssed-partial-nonprefix-noskip-offset:1` continuation.
- Digit-prefix controls stayed on the native partial prefix path:
  `_DCT_GEN2011` partial `01` was 13 ms through
  `ssed_partial_prefix_prepass`, and `_DCT_GEN2009` partial `0` was 122 ms
  with a cursor probe `ok` at 57 ms.
- Windows `_DCT_KENROWA` `search_partial` `しめ` was 20 ms, iOS `KENROWA`
  `search_partial` `しめ` was 20 ms, and `_DCT_PROYAL53` `search_partial`
  `ひゃ` was 37 ms.

- `/tmp/lvcore-all-corpora-validation-20260613-partial-native-prefix-fast-prepass-v1.jsonl`
- Produced after adding a bounded SSED partial-search native prefix prepass for
  no-cursor pure-kana queries on large dense-sidecar packages. The prepass uses
  the native near-key title probe before the broad sidecar contains scan; if it
  finds no prefix hit, the existing dense-sidecar partial behavior remains
  available.
- 335 reconstructed package paths validated with package status 335 `ok`.
- Warning diagnostics remained only the explicitly deferred HC common HTML
  fallback.
- Windows `_DCT_KENROWA` partial `しめ` was 20 ms, iOS `KENROWA` partial
  `しめ` was 20 ms, and `_DCT_PROYAL53` partial `ひゃ` was 37 ms.
- `_DCT_GKBUSINE` partial `10` remained a concrete separate candidate at
  428 ms.

- `/tmp/lvcore-all-corpora-validation-20260613-title-label-fast-prepass-v1.jsonl`
- Produced after adding a bounded SSED exact-search visible title-label prepass
  for no-cursor single-token non-ASCII queries. The prepass scans at most 64
  ordered title rows before the native-key path; if it finds no hit, the
  existing native search and full title-label fallback path remain unchanged.
- 335 reconstructed package paths validated with package status 335 `ok`.
- Warning diagnostics remained only the explicitly deferred HC common HTML
  fallback.
- `_DCT_KQNEWEJ6` `search_exact` `画像一覧` was 12 ms, down from 381 ms in the
  previous full gate, and returned through `ssed_title_label_fast_prepass`.
- `_DCT_KQNEWJE5` `search_exact` `外字一覧` was 12 ms, down from 271 ms, and
  returned through the same bounded prepass.

- `/tmp/lvcore-all-corpora-validation-20260613-digit-forward-fast-probe-v2.jsonl`
- Produced after making initial SSED full-text forward-title prepass conditional
  for single-token digit/no-ASCII-letter queries. These queries now run a cheap
  one-leaf forward-hit probe first: real prefix hits still take the normal
  forward title prepass, while empty prefix probes skip the expensive prepass
  and fall through to the existing partial-title/body phases.
- 335 reconstructed package paths validated with package status 335 `ok`.
- Warning diagnostics remained only the explicitly deferred HC common HTML
  fallback.
- `_DCT_NMEDEJ12` `search_full_text` `01` was 169 ms, down from 556 ms in the
  previous full gate, and returned through `ssed_fulltext_body_direct_scan`.
- `_DCT_KQDENTAL` `search_full_text` `01` was 190 ms, down from 546 ms, and
  kept the same title-index first hit.
- `_DCT_GKBUSINE` `search_full_text` `10` was 27 ms, down from 387 ms, and
  kept the same title-index first hit.
- `_DCT_NCOMP4` `search_full_text` `1計` was 432 ms, down from 545 ms, but
  remained a concrete candidate because its continuation was still
  intentionally deferred behind `title-nonprefix-unverified:*`.

- `/tmp/lvcore-all-corpora-validation-20260613-mixed-japanese-nonprefix-fulltext-v2.jsonl`
- Produced after allowing short CJK-leading mixed kana+CJK SSED full-text title
  queries to use the existing bounded non-prefix native-title prepass, instead
  of falling through to the slower all-title partial scan when the prefix probe
  is empty. The gate also verifies that kana-leading mixed queries such as
  `_DCT_KENE7J5` `は殺` stay on the direct HONMON body scan.
- 335 reconstructed package paths validated with package status 335 `ok`.
- Warning diagnostics remained only the explicitly deferred HC common HTML
  fallback.
- Windows `_DCT_YHOUGO3` `search_full_text` `一ス` was 53 ms, down from 585 ms
  in the previous full gate, and returned through
  `ssed_fulltext_partial_nonprefix_title_prepass`.
- iOS `HKKIGAK6` `search_full_text` `体の` was 177 ms, down from 369 ms in the
  previous full gate, and returned through the same non-prefix title prepass.

- `/tmp/lvcore-all-corpora-validation-20260613-native-first-kana-backward-v1.jsonl`
- Produced after making large SSED sidecar packages try native
  exact/forward/backward title search first for pure-kana no-cursor queries of
  at least two characters, avoiding empty sidecar title prepasses when native
  index hits are already available.
- 335 reconstructed package paths validated with package status 335 `ok`.
- Warning diagnostics remained only the explicitly deferred HC common HTML
  fallback.
- Windows `_DCT_KENROWA` exact `しめ` was 53 ms, forward `しめ` was 27 ms, and
  backward `しめ` was 10 ms. Windows `_DCT_JSSAURU2` exact `あぎと` was 33 ms,
  forward `あぎ` was 34 ms, and backward `ぎと` was 14 ms.

- `/tmp/lvcore-all-corpora-validation-20260613-circle-marker-row-prefetch-v1.jsonl`
- Produced after two scoped SSED provider changes: keeping KOJIEN7 native
  circle-marker title queries on the native index path, and skipping redundant
  initial row-driven SSED full-text body prefetch when byte-candidate direct
  HONMON scanning is available.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path-set hash is unchanged:
  `f57adc5dfa92d5aecabfd02579639a34e311fbc3edcda62883717489479d5b82`.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.
- Windows `_DCT_KOJIEN7` exact
  `◯に滅せずんば炎炎を若何いかんせん` is 45 ms and forward `◯に` is 67 ms.
  iOS `KOJIEN7` exact is 54 ms and forward `◯に` is 60 ms.
- Initial SSED full-text row prefetch no longer appears in the gate.
  `_DCT_NMEDEJ12` `search_full_text` `01` is now a pure direct HONMON scan at
  621 ms, `_DCT_GEN2005` `曙光` is 101 ms, and `_DCT_KENE7J5` `は殺` is 276 ms.

- `/tmp/lvcore-all-corpora-validation-20260613-hkkigak6-partial-title-prepass-v1.jsonl`
- Produced after making initial bounded SSED partial search avoid the slow
  prefix fallback when the fast native prefix probe is empty, then satisfy the
  first page through the bounded native partial-title scan. This gate also
  includes the mode-invariant surface render-mode validation skip from 0ai.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered, including the
  two `Other/Android` rows.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.
- `Other/iOS/HKKIGAK6/HKKIGAK6` `search_partial` `体の` dropped from 502 ms in
  the previous full gate to 293 ms; its cursor probe is `ok` at 39 ms and
  resumes as `ssed-partial-nonprefix-noskip-physical-offset:5:79:2`.
- `Other/iOS/RDRSP2/RDRSP2` `ios-plist:indexSearch.plist` now records
  `mode_invariant_surface` render-mode skips in the full gate and is 1001 ms,
  down from 2073 ms in the previous full gate.

- `/tmp/lvcore-all-corpora-validation-20260613-title-prepass-filled-page-stop-v1.jsonl`
- Produced after making the initial SSED full-text title/index prepass stop at
  the requested page limit instead of scanning one extra title hit to prove
  continuation that is immediately replaced by the body/title continuation
  policy.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered, including the
  two `Other/Android` rows.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.
- `_DCT_KQDENTAL` `search_full_text` `01` dropped from 681 ms to 579 ms,
  `_DCT_YHOUGO3` `search_full_text` `一ス` dropped from 660 ms to 613 ms, and
  `_DCT_HKDKSR10` `search_full_text` `FU` dropped from 596 ms to 427 ms.

- `/tmp/lvcore-all-corpora-validation-20260613-direct-body-filled-page-cursor-v2.jsonl`
- Produced after making SSED direct HONMON full-text scans continue through all
  byte-candidate entries in a scan window, stop filled pages without proving an
  extra hit, and resume `body-offset:*` pages through the same byte-window scan
  from the next body entry instead of a 4096-row index pass.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered, including the
  two `Other/Android` rows.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.
- `_DCT_NMEDEJ12` `search_full_text` `01` scans one direct body window and
  returns an earlier body hit in 686 ms, down from 807 ms and five scanned
  windows in the previous full gate. `_DCT_KENE7J5` `search_full_text` `は殺`
  dropped from 607 ms to 343 ms, and `_DCT_GEN2005` `search_full_text` `曙光`
  dropped from 506 ms to 193 ms.

- `/tmp/lvcore-all-corpora-validation-20260613-tagged-nonprefix-prefilter-v3.jsonl`
- Produced after adding a state-aware SSED tagged-leaf page prefilter, scoped to
  large non-prefix title scans. The prefilter tracks inherited tagged group
  keys so continuation pages remain complete, while avoiding broad title-prepass
  behavior changes.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered, including the
  two `Other/Android` rows.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.
- `_DCT_NCOMP4` `search_full_text` `1計` dropped from 2056 ms in the previous
  full gate to 545 ms. `_DCT_KQNEWEJ6` stayed on its fast title-prepass path at
  406 ms, and `_DCT_KENE7J5` remained on its prior native body-scan path at
  607 ms after the page-prefilter extensions were scoped to non-prefix scans.

- `/tmp/lvcore-all-corpora-validation-20260613-sidecar-body-phase-deferral-v1.jsonl`
- Produced after making explicit SSED full-text `sidecar-body-row:*`
  continuations stop at the sidecar phase boundary and return `body:0` when
  native HONMON scanning remains eligible, instead of performing that native
  body scan inside the sidecar cursor probe.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered, including the
  two `Other/Android` rows.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-nonascii-sidecar-prepass-v1.jsonl`
- Produced after adding authoritative non-ASCII SSED sidecar title/body
  prepasses for iOS dense sidecars, broadening non-ASCII sidecar-title
  continuation deferral to exact/forward/backward searches, and making SSED
  navigation detection probe only the first menu page when checking whether a
  menu surface exists.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered, including the
  two `Other/Android` rows.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-ssed-jis-prefilter-memmem-v2.jsonl`
- Produced after making the SSED separator-aware JIS byte prefilter seek to
  occurrences of the first JIS pair before verifying separator-skipped pair
  sequences. This keeps native search semantics but avoids testing every byte
  offset in sparse index/body prefilter windows.
- The artifact combines the 334-row all-root gate with focused validation of
  the two baseline `Other/Android` package rows omitted by that root list.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-ios-table-list-cross-book-shortcut-v1.jsonl`
- Produced after making iOS SSED `tableList.plist` cross-book rows skip
  repeated local loose-address misses when a sibling owner is known, while
  keeping mixed local/cross-book tableLists lazily fallback-capable.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-unverified-nonprefix-title-v1.jsonl`
- Produced after deferring large SSED full-text non-prefix title continuation
  proof behind explicit `title-nonprefix-unverified:*` cursors and teaching
  deep validation not to probe those intentionally unverified continuations.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-unverified-sidecar-title-v1.jsonl`
- Produced after deferring large exact CJK SSED sidecar-title continuation
  proof behind explicit `sidecar-title-unverified-row:*` cursors and teaching
  deep validation not to probe those intentionally unverified continuations.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-cjk-sidecar-prefix-v1.jsonl`
- Produced after adding an authoritative CJK SSED partial-prefix sidecar-title
  fast path and making dense sidecar block ranges lazy during discovery.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-nonprefix-title-fulltext-v4.jsonl`
- Produced after adding a bounded SSED full-text non-prefix native-title
  prepass and an opaque `title-nonprefix:*` continuation cursor carrying
  already-returned title targets.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-title-physical-offset.jsonl`
- Produced after adding physical-offset cursors for large SSED full-text title
  prepass continuations and after making dense sidecar title search project only
  id/title-like columns.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-main-wordlist-jtext.jsonl`
- Produced after treating `K_text`/`J_text` pairs in dense SSED `main`
  wordlist sidecars as bidirectional title columns.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-initial-native-offset-mode-sized.jsonl`
- Produced after changing large SSED native exact/forward/backward first pages
  to defer expensive next-page proof behind explicit
  `ssed-offset-unverified:*` cursors, including nested
  `ssed-partial-prefix:ssed-offset-unverified:*` validation handling.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-title-label-unverified-nested-skip.jsonl`
- Produced after changing SSED title-label fallback continuation proof to
  explicit `ssed-title-label-unverified:*` cursors, including the nested
  `ssed-partial-prefix:ssed-title-label-unverified:*` form.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-sidecar-title-physical-cursor.jsonl`
- Produced after changing dense SSED sidecar title continuations from logical
  offset cursors to physical sidecar row cursors.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-ssed-title-cursor-budget.jsonl`
- Produced after reducing the empty physical-title continuation prefilter budget
  for SSED full-text title cursors.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-ssed-direct-scan-chunk-cache.jsonl`
- Produced after adding a small MRU cache for expanded `SsedDataFile` chunks and
  widening SSED direct full-text scan windows from 256 KiB to 1 MiB.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-lved-direct-fts-variants.jsonl`
- Produced after routing guarded multi-variant LVED FTS searches through direct
  FTS table expressions and deferring exact proof of filled LVED continuation
  pages behind `lved-offset-unverified:*` cursors.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260613-ssed-multi-near-key.jsonl`
- Produced after caching SSED MULTI descriptors/selector menus and adding a
  simple-leaf near-key fast path for filtered MULTI browse surfaces.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260612-native-offset-cursor.jsonl`
- Produced after changing native SSED exact/forward/backward numeric offset
  continuation pages to defer expensive one-extra-hit proof behind
  `ssed-offset-unverified:*` cursors.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260612-generic-html-resource-byte-cap.jsonl`
- Produced after capping deep-validation alternate `GenericHtml` probes by
  known native resource byte totals and streaming eligible data-URL output into
  the final HTML buffer.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260612-ssed-fulltext-row-prefetch-cap.jsonl`
- Produced after capping first-page row-driven SSED full-text body prefetch
  when byte candidates are available, so late/no-hit cases fall through to the
  direct HONMON byte-window scan sooner.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260612-ssed-fulltext-body-cursor.jsonl`
- Produced after changing post-title-prepass SSED full-text continuation from
  row-driven body cursors to the existing deferred native body cursor.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

- `/tmp/lvcore-all-corpora-validation-20260612-ios-panel-cache.jsonl`
- Produced after caching parsed SSED plist panel projections by source label and
  requested panel id, avoiding repeated iOS panel projection work during
  surface/render/window validation.
- 336 packages validated with package status 336 `ok`.
- The previous 336-package baseline path set is fully covered.
- Warning diagnostics remain only the explicitly deferred HC common HTML
  fallback.

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
| `hc_render_common_html_fallback` | 816 | Deferred HC visual rendering |
| `ssed_loose_address_unresolved` | 0 | Closed by packed SSED link-address normalization |

Important info/status classes from the latest gate:

| Marker | Count | Classification |
| --- | ---: | --- |
| `sidecar-body-row:*` cursor probed `ok` | 47 | Dense sidecar body cursor fix verified |
| `sidecar-title-unverified-row:*` cursor `not_probed` | 19 | Broad single-character exact/forward/backward sidecar-title continuations intentionally deferred |
| `title-nonprefix-unverified:*` cursor `not_probed` | 1 | Large full-text non-prefix title continuation intentionally deferred |
| `body:0`/`body-offset:*` full-text cursor `not_probed` | 124 | Post-title native body continuation intentionally deferred |
| `sidecar-body-start` cursor probed `ok` | 15 | Sidecar body phase start cursor fix verified |
| `title-nonprefix:*` cursor probed `ok` | 1 | Small bounded non-prefix title continuation still probeable |
| `sidecar-body:*` cursor `not_probed` | 0 | Closed by row/start/physical cursor split |
| `ssed_fulltext_body_window_scan` | 0 | Closed by direct native HONMON scan fallback |
| `ssed_fulltext_title_index_prepass` | 137 | Full-text title/index prepass remains the dominant successful fast path |
| `ssed_fulltext_body_direct_scan` | 5 | Direct native HONMON fallback exercised |
| `ssed_fulltext_partial_nonprefix_title_prepass` | 4 | NCOMP4, YHOUGO3, and HKKIGAK6 title prepasses exercised; some cursor probes intentionally deferred |
| `ssed_index_empty_physical_pages_skipped` | 1 | Sparse physical scan advances exercised by non-prefix title searches |
| `ssed_index_empty_physical_scan_limited` | 0 | Not exercised in the latest gate after recent prefix fast paths |
| `ssed-partial-nonprefix-unverified-index:*` cursor `not_probed` | 0 | Replaced by bounded/probeable non-prefix partial continuations in the latest gate |
| `ssed-partial-nonprefix-noskip-unverified-physical-offset:*` cursor `not_probed` | focused direct-only | New NCOMP4 partial `1計` continuation deferral; not exercised by the standard full-gate query set |
| `ssed-offset-unverified:*` direct/nested cursor `not_probed` | 209 | Native offset next-page proof intentionally deferred |
| `ssed-title-label-unverified:*` direct/nested cursor `not_probed` | 27 | Distant/unknown title-label continuations intentionally deferred after bounded post-hit lookahead |
| `ssed-partial-prefix:*` cursor probed `ok` | 54 | Partial prefix continuation pages remain probeable |
| `ssed_partial_native_prefix_fast_prepass` | 8 | Bounded pure-kana partial native-prefix prepass exercised |
| `ssed_title_label_fast_prepass` | 29 | Bounded exact non-ASCII and forward digit/no-alpha visible title-label prepass exercised |
| `lved_viewer_hook_deferred` | 260 | Intentional external viewer policy |
| `gaiji_formatting_helper_candidate` | 24 | Observed helper code candidates |
| `ssed_navigation_empty_sentinel` | 19 | Expected sentinel classification |
| `skipped_large_view` | 407 nested render-mode statuses | Validator cap for large alternate render probes |
| `no_resource`, `no_link`, `no_target` | many | Usually validator sample result, not a failure |

Latest concrete non-HC performance candidates from the full gate:

- Several top `surface_first_target` rows are likely validator render/window
  work over large browse targets; measure direct `home`/`surface`/`window`
  before treating them as LVCore gaps.
- `_DCT_NCOMP4`, `search_full_text` query `1計`: 412 ms, improved from 545 ms
  but still behind intentionally deferred `title-nonprefix-unverified:*`
  continuation proof in the latest full baseline. Direct focused probing also
  exposed a related `search_partial` `1計` first-page gap; that partial gap is
  resolved by the focused NCOMP4 partial-prefix fallthrough/fast-miss fix
  below.
- `Other/iOS/NANMED20/NANMED20`, `search_forward` query `0歳`: 390 ms in the
  full gate. Direct search probes were fast, so measure validation/render
  context before changing search code.
- `_DCT_HKDKSR10`, `search_full_text` query `FU`: 392 ms through
  `ssed_fulltext_title_index_prepass`, with native body continuation deferred
  behind `body:0`.
- `_DCT_EJJE100`, `search_full_text` query `co`: 401 ms through
  `ssed_fulltext_title_index_prepass`, with native body continuation deferred
  behind `body:0`.
- `_DCT_HKDKSR10`, `search_partial` query `AB`: 420 ms through
  `ssed_partial_prefix_prepass`; direct forward/partial probes are similar, so
  this may be package/open or native prefix setup rather than partial-only
  behavior.
- `_DCT_HKKIGAK6`, `search_partial` query `Ab`: 307 ms through
  `ssed_partial_title_index_prepass`; direct probing reproduced roughly
  0.4s, making it a possible future scoped partial-title candidate.
- `_DCT_GKBUSINE`, `search_partial` query `10`: resolved in the latest full
  gate at 38 ms, down from 428 ms, by using a one-leaf fast-hit probe before
  the expensive native partial-prefix probe.
- `_DCT_GKBUSINE`, `search_forward` query `10`: resolved in the latest full
  gate at 86 ms, down from 439 ms, by running the bounded title-label fast
  prepass after a cheap one-leaf native forward probe proves there is no native
  prefix hit.
- SSED partial-prefix/non-prefix proof latency for `_DCT_HKDKSR13`,
  `_DCT_HKDKSR14`, `_DCT_HKDKSR29`, `_DCT_HKDKSR30`, and `_DCT_KQCOLEXP` is
  improved in the latest full gate. The rows still cost about 0.7-0.9s, but
  verified continuation proof no longer does the avoidable extra display-label
  prefix pass or short-anchor expanded-index scan.
- `_DCT_NCOMP4`, direct `search_partial` query `1計`: resolved by focused
  direct probing. It now returns `0-1計画法` in about 0.49s instead of exposing
  an empty initial prefix page or spending about 2.6s proving a non-prefix
  continuation.
- Windows and iOS `KENROWA`, `search_partial` query `しめ`: resolved in the
  latest full gate at 20 ms, down from 1091 ms and 1232 ms, by running the
  bounded native prefix prepass before broad dense-sidecar contains search.
- `_DCT_PROYAL53`, `search_partial` query `ひゃ`: resolved in the latest full
  gate at 37 ms, down from 787 ms, by the same prepass.
- `_DCT_KQNEWEJ6`, `search_exact` query `画像一覧`: resolved in the latest full
  gate at 15 ms, down from 381 ms, by running the bounded exact visible
  title-label fast prepass.
- `_DCT_KQNEWJE5`, `search_exact` query `外字一覧`: resolved in the latest full
  gate at 17 ms, down from 271 ms, by the same prepass.
- `_DCT_NMEDEJ12`, `search_full_text` query `01`: resolved in the latest full
  gate at 167 ms, down from 556 ms, by skipping the empty forward-title prepass
  after a one-leaf fast-hit probe.
- `_DCT_KQDENTAL`, `search_full_text` query `01`: resolved in the latest full
  gate at 189 ms, down from 546 ms, while preserving its title-index first hit.
- `_DCT_GKBUSINE`, `search_full_text` query `10`: resolved in the latest full
  gate at 28 ms, down from 387 ms, while preserving its title-index first hit.
- `_DCT_YHOUGO3`, `search_full_text` query `一ス`: resolved further by proving
  the mixed CJK+kana non-prefix title continuation exhausted and falling
  through to `body:0`; latest full-gate validation is 162 ms.
- Windows `_DCT_KOJIEN7` and iOS `KOJIEN7`, exact query
  `◯に滅せずんば炎炎を若何いかんせん` and forward query `◯に`: resolved in
  0ak and verified in the latest full-corpus gate.
- `Other/iOS/HKKIGAK6/HKKIGAK6`, `search_partial` query `体の`: resolved in
  0aj and verified in the latest full-corpus gate at 282 ms. Its
  `search_full_text` query `体の` remains on the bounded CJK-leading mixed
  Japanese non-prefix title prepass at 187 ms.
- `Other/iOS/RDRSP2/RDRSP2` `ios-plist:indexSearch.plist` and
  `SINMEI7` menu/plist panel rows were inspected after the previous gate. The
  mode-invariant render-mode validation overwork is resolved in 0ai and is now
  reflected in the latest full-corpus gate.
- `Other/iOS/IBIO5/IBIO5`, title/full-text query `亜-`: resolved in 0ad and
  verified by the latest full gate.

Rows such as `_DCT_GKKNJPZL` `search_forward` query `00` and `_DCT_IWKOKU7N`
`search_forward` query `3D` include HC fallback rendering diagnostics and
should not drive LVCore-only work while HC remains deferred.

`_DCT_GENIUSEB` `search_forward` query `co` was reclassified after focused
inspection: direct native search is fast, while validation spends the row time
rendering the first hit/window through the deferred HC fallback. It should not
drive LVCore-only work while HC remains deferred.

`Other/iOS/KQNEWJE5/KQNEWJE5` `search_forward` query `和英` was reclassified
after focused inspection: direct native search is fast, while the full-gate
validation row includes HC fallback rendering for the first hit. It should not
drive LVCore-only work while HC remains deferred.

`Other/iOS/KQNEWJE5/KQNEWJE5` `ios-table-list:tableList.plist` was resolved in
0ab: it dropped from 5462 ms with a 1883 ms cursor probe in the previous full
gate to 927 ms with a 0 ms cursor probe in the current full gate.

## Fix-Now / Recently Closed Candidates

### 0as. GKBUSINE forward digit title-label fast prepass (resolved, full gate)

Why this matters:

- The latest baseline left `_DCT_GKBUSINE` `search_forward` query `10` at
  439 ms in the full gate, and direct no-render probes reproduced about
  0.70s.
- The first page did not need HC or body rendering. It returned visible title
  labels such as `10000YENPERSETISOURROCKBOTTOMPRICE`, but only after the
  forward native-key path missed and the broader title-label fallback ran.
- Exact lookup for the full first title was already about 0.01s, and
  `_DCT_GKBUSINE` partial/full-text `10` were already resolved, so this was a
  narrow forward title-label routing gap.

Current status:

- No-cursor SSED forward queries that are single-token digit/no-ASCII-letter
  strings can now use the existing bounded title-label fast prepass.
- The fast prepass is guarded by the existing one-leaf native forward hit
  probe. If the native prefix probe finds a real native hit, the query keeps
  the existing native forward path and cursor behavior. If the probe misses,
  the first page can be satisfied from the bounded title-label scan before the
  expensive native miss and fallback path.
- Cursor pages still use the existing title-label fallback cursor handling.

Verification:

- Focused tests passed:
  - `cargo test -p lvcore search_ssed -- --nocapture`
  - `cargo check -p lvcore`
  - `cargo build -p lvcore-cli`
- Direct real-package probes:
  - `_DCT_GKBUSINE` forward `10 --limit 1`: about 0.04s, hit
    `10000YENPERSETISOURROCKBOTTOMPRICE`, cursor
    `ssed-title-label-unverified:1`, diagnostic
    `ssed_title_label_fast_prepass`.
  - `_DCT_GKBUSINE` forward `10 --limit 3`: about 0.05s, same first three
    hits as before, cursor `ssed-title-label-unverified:3`, diagnostic
    `ssed_title_label_fast_prepass`.
  - Native-hit controls stayed native: `_DCT_GEN2011` forward `01 --limit 1`
    has no title-label diagnostic, and `_DCT_GEN2009` forward `0 --limit 3`
    keeps cursor `3` with no title-label diagnostic.
  - `_DCT_GKBUSINE` partial `10` and full-text `10` stayed on their previous
    fast paths.
- Focused package validation passed:
  - `/tmp/lvcore-focused-validate-gkbusine-forward-title-label-fast-prepass-v1.jsonl`
  - Package status 5 `ok` across `_DCT_GKBUSINE`, `_DCT_GEN2011`,
    `_DCT_GEN2009`, iOS `NANMED20`, and `_DCT_NCOMP4`.
  - `_DCT_GKBUSINE` forward `10` is 81 ms through
    `ssed_title_label_fast_prepass`.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260614-gkbusine-forward-title-label-fast-prepass-v1.jsonl`
  - 335 package statuses `ok`.
  - Warning diagnostics remain the explicitly deferred HC common HTML fallback
    only, 949 occurrences.
  - `_DCT_GKBUSINE` forward `10` is 83 ms, down from 439 ms in the previous
    full gate.
  - Native-hit digit controls stayed stable: `_DCT_GEN2011` forward `01` is
    61 ms with no title-label diagnostic, and `_DCT_GEN2009` forward `0` is
    185 ms with cursor probe `ok`.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_GKBUSINE`
- Previous full-gate row:
  - `_DCT_GKBUSINE`, `search_forward`, query `10`, 439 ms, hit_count 3,
    cursor `ssed-title-label-unverified:3`.
- Direct pre-fix probes:
  - `search_forward` `10 --limit 1`: about 0.70s, first hit
    `10000YENPERSETISOURROCKBOTTOMPRICE`, cursor
    `ssed-title-label-unverified:1`.
  - `search_forward` `10000 --limit 1`: about 0.68s, same first title.
  - `search_exact` `10000YENPERSETISOURROCKBOTTOMPRICE --limit 1`: about
    0.01s.

### 0ar. NCOMP4 partial `1計` empty prefix fallthrough (resolved, focused)

Why this matters:

- While investigating the latest full-gate `_DCT_NCOMP4` full-text `1計` row,
  direct partial probing exposed a concrete search behavior gap:
  `search_partial` query `1計 --limit 1` returned zero hits with
  `ssed-partial-prefix:ssed-prefiltered-index:6:90`.
- The same package and query has a visible non-prefix title hit,
  `0-1計画法`, through the native title/index scan. Returning an empty first
  page forced callers to chase a slow prefix cursor before seeing the match.
- A behavior-only fallthrough fixed the empty page but still took about 2.6s,
  because large non-prefix partial search proved the next continuation before
  returning.

Current status:

- Initial partial-prefix pages with zero hits now fall through to the normal
  partial/non-prefix path instead of surfacing an empty prefix cursor.
- Large initial non-prefix partial pages now defer next-page proof behind
  `ssed-partial-nonprefix-noskip-unverified-physical-offset:*`. The cursor
  preserves both the physical start page and matched-row offset, so continuation
  can resume without repeating the first hit.
- Initial digit/no-ASCII-letter partial queries now run the existing one-leaf
  native forward prefix-hit probe before the expensive fallback prefix scan,
  even on packages too large for the bounded native prefix prepass. Empty
  prefix misses go straight to non-prefix partial search; real prefix hits keep
  the existing prefix path.
- Validator cursor probing treats the new unverified physical-offset cursor as
  intentionally not probed, with the same reason as other large partial
  non-prefix continuations.

Verification:

- Focused tests passed:
  - `cargo test -p lvcore search_ssed -- --nocapture`
  - `cargo test -p lvcore-cli validate_search_cursor_probe_skips_expensive_fulltext_body_cursors -- --nocapture`
  - `cargo check -p lvcore`
  - `cargo build -p lvcore-cli`
- Direct real-package probes:
  - `_DCT_NCOMP4` `search_partial` `1計 --limit 1`: about 0.49s, hit
    `0-1計画法`, cursor
    `ssed-partial-nonprefix-noskip-unverified-physical-offset:8:121:1`.
  - `_DCT_NCOMP4` `search_full_text` `1計 --limit 1`: about 0.49s, same hit,
    existing `title-nonprefix-unverified:*` cursor.
  - `_DCT_GKBUSINE` `search_partial` `10 --limit 1`: about 0.05s, still
    through `ssed_partial_title_index_prepass`.
  - `_DCT_GEN2011` `search_partial` `01 --limit 1`: about 0.04s, still
    through `ssed_partial_prefix_prepass`.
- Focused package validation passed:
  - `/tmp/lvcore-focused-validate-ncomp4-partial-prefix-fast-miss-gate-v1.jsonl`
  - Package status 5 `ok` across `_DCT_NCOMP4`, `_DCT_GKBUSINE`,
    `_DCT_GEN2011`, `_DCT_GEN2009`, and `_DCT_KENROWA`.
  - Control rows stayed stable: `_DCT_GKBUSINE` partial `10` is 36 ms with
    cursor probe `ok`; `_DCT_GEN2011` partial `01` is 13 ms; `_DCT_GEN2009`
    partial `0` is 121 ms with cursor probe `ok`; `_DCT_KENROWA` partial
    `しめ` is 20 ms through the native prefix fast path.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-ncomp4-partial-prefix-fast-miss-gate-v1.jsonl`
  - 335 package statuses `ok`.
  - Warning diagnostics remain the explicitly deferred HC common HTML fallback
    only, 949 occurrences.
  - Standard validation controls stayed stable: `_DCT_GKBUSINE` partial `10`
    is 37 ms, `_DCT_GEN2011` partial `01` is 13 ms, `_DCT_GEN2009` partial
    `0` is 123 ms, and `_DCT_KENROWA` partial `しめ` is 20 ms.
  - `_DCT_NCOMP4` full-text `1計` remains stable at 446 ms with the existing
    deferred `title-nonprefix-unverified:*` continuation.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_NCOMP4`
- Direct pre-fix probe:
  - `search_partial` `1計 --limit 1`: about 1.05s, zero hits, cursor
    `ssed-partial-prefix:ssed-prefiltered-index:6:90`.
- Direct behavior-only fallthrough probe:
  - `search_partial` `1計 --limit 1`: about 2.6s, hit `0-1計画法`, cursor
    `ssed-partial-nonprefix-noskip-physical-offset:8:121:1`.

### 0aq. SSED digit/no-alpha partial prefix fast-hit probe (resolved, full gate)

Why this matters:

- The latest baseline left `_DCT_GKBUSINE` `search_partial` query `10` at
  428 ms even though the same first hit was available quickly through the
  partial title-index path.
- Direct probes showed the no-cursor partial route was spending about 680 ms
  on an empty native prefix probe before falling through to
  `ssed_partial_title_index_prepass`.
- A blanket digit skip was rejected because GEN year dictionaries have real
  digit prefix hits. Baseline controls included `_DCT_GEN2011` partial `01`
  at 8 ms and `_DCT_GEN2009` partial `0` at 120 ms through
  `ssed_partial_prefix_prepass`.

Current status:

- No-cursor SSED partial searches now run a one-leaf native forward-hit probe
  before the bounded native prefix scan for single-token queries containing
  ASCII digits and no ASCII letters.
- If the cheap probe finds a prefix hit, the existing partial prefix path still
  runs. If it misses, LVCore skips the expensive native prefix miss and falls
  through to the existing partial title-index/non-prefix path.
- Normal alphabetic, mixed ASCII-letter, kana, cursor, and non-digit partial
  queries keep their previous behavior.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo check -p lvcore`
  - `cargo test -p lvcore search_ssed -- --nocapture`
  - `cargo build -p lvcore-cli`
  - `git diff --check -- crates/lvcore/src/package/drivers/search_ssed.rs`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-partial-digit-prefix-fast-probe-v1.jsonl`
  - Package status: 5 `ok`.
  - `_DCT_GKBUSINE` partial `10`: 35 ms through
    `ssed_partial_title_index_prepass`, cursor probe `ok` at 12 ms.
  - `_DCT_GEN2011` partial `01`: 12 ms through `ssed_partial_prefix_prepass`.
  - `_DCT_GEN2009` partial `0`: 112 ms through `ssed_partial_prefix_prepass`,
    cursor probe `ok` at 53 ms.
  - `_DCT_KENROWA` partial `しめ` and `_DCT_PROYAL53` partial `ひゃ` stayed on
    the pure-kana native-prefix fast prepass.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-partial-digit-prefix-fast-probe-v1.jsonl`
  - 335 reconstructed package paths validated with package status 335 `ok`.
  - Warning diagnostics remained only the explicitly deferred HC common HTML
    fallback.
  - `_DCT_GKBUSINE` partial `10`: 37 ms, down from 428 ms.
  - `_DCT_GEN2011` partial `01`: 13 ms through `ssed_partial_prefix_prepass`.
  - `_DCT_GEN2009` partial `0`: 122 ms through `ssed_partial_prefix_prepass`,
    cursor probe `ok` at 57 ms.

### 0ap. SSED pure-kana partial native prefix fast prepass (resolved, full gate)

Why this matters:

- The latest baseline exposed dense-sidecar SSED partial searches spending most
  of their first-page time in broad sidecar-title contains scans even though a
  native prefix hit was available immediately:
  - Windows `_DCT_KENROWA` `search_partial` `しめ`: 1091 ms.
  - iOS `KENROWA` `search_partial` `しめ`: 1232 ms.
  - `_DCT_PROYAL53` `search_partial` `ひゃ`: 787 ms.
- Exact/forward/backward pure-kana native-first search was already resolved.
  Partial search still entered the dense sidecar contains path before the
  existing native prefix phase.
- Direct probes showed the native prefix hits render correctly and are much
  faster: KENROWA `しめ` and PROYAL53 `ひゃ` returned in about 30-50 ms.

Current status:

- No-cursor SSED partial searches now try a bounded native prefix near-key
  prepass before broad dense-sidecar title contains search when all of these
  are true: the query is pure kana, at least two characters, single-token, and
  the package has a large title-bearing dense sidecar.
- If the native prefix prepass finds no hit, LVCore falls through to the
  existing sidecar partial behavior.
- Native prefix continuation cursors are wrapped as `ssed-partial-prefix:*`,
  so continuation pages stay in partial-search state instead of leaking raw
  native offset cursors.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo check -p lvcore`
  - `cargo test -p lvcore search_ssed -- --nocapture`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-partial-native-prefix-fast-prepass-v1.jsonl`
  - Package status: 4 `ok`.
  - Windows `_DCT_KENROWA` partial `しめ`: 20 ms through
    `ssed_partial_native_prefix_fast_prepass`, cursor probe `ok` at 10 ms.
  - iOS `KENROWA` partial `しめ`: 20 ms through the same prepass, cursor probe
    `ok` at 10 ms.
  - `_DCT_PROYAL53` partial `ひゃ`: 34 ms through the same prepass, cursor
    probe `ok` at 17 ms.
  - `_DCT_GKBUSINE` partial/forward `10` remained on the previous
    non-kana/simple-search paths and is still tracked separately.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-partial-native-prefix-fast-prepass-v1.jsonl`
  - 335 reconstructed package paths validated with package status 335 `ok`.
  - Warning diagnostics remained only the explicitly deferred HC common HTML
    fallback.
  - Windows `_DCT_KENROWA` partial `しめ`: 20 ms, down from 1091 ms.
  - iOS `KENROWA` partial `しめ`: 20 ms, down from 1232 ms.
  - `_DCT_PROYAL53` partial `ひゃ`: 37 ms, down from 787 ms.
  - `ssed_partial_native_prefix_fast_prepass` appeared on 8 pure-kana partial
    rows.

### 0ao. SSED exact visible title-label fast prepass (resolved, full gate)

Why this matters:

- The latest baseline exposed exact SSED searches where the user-visible title
  label matched immediately, but LVCore first paid for a native-key miss:
  - `_DCT_KQNEWEJ6` `search_exact` `画像一覧`: 381 ms.
  - `_DCT_KQNEWJE5` `search_exact` `外字一覧`: 271 ms.
- Direct fallback-only probes showed the visible title-label path was already
  fast, around 50 ms, and returned the expected first visible title.
- Broader forward/partial title-label cases such as `_DCT_GKBUSINE` `10` were
  left alone because they have wider search semantics and should be handled as
  separate candidates.

Current status:

- No-cursor SSED exact searches now run a bounded visible title-label prepass
  only for single-token non-ASCII queries. The prepass scans at most 64 ordered
  title rows before native search.
- If the prepass finds hits, LVCore returns that page with
  `ssed_title_label_fast_prepass`. If it finds no hit, the existing native
  search and normal title-label fallback path run unchanged.
- Cursor pages, ASCII exact searches, forward/partial searches, and the normal
  full title-label fallback budget are unchanged.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo check -p lvcore`
  - `cargo test -p lvcore search_ssed -- --nocapture`
  - focused SSED architecture-contract tests for title-label fallback behavior.
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-title-label-fast-prepass-v1.jsonl`
  - Package status: 5 `ok`.
  - `_DCT_KQNEWEJ6` exact `画像一覧`: 12 ms through
    `ssed_title_label_fast_prepass`.
  - `_DCT_KQNEWJE5` exact `外字一覧`: 16 ms through
    `ssed_title_label_fast_prepass`.
  - `_DCT_EJJE100` ASCII/full-text control did not use the exact title-label
    fast prepass.
  - `_DCT_GKBUSINE` forward/partial `10` remained on the previous simple-search
    paths and is still tracked separately.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-title-label-fast-prepass-v1.jsonl`
  - 335 reconstructed package paths validated with package status 335 `ok`.
  - Warning diagnostics remained only the explicitly deferred HC common HTML
    fallback.
  - `_DCT_KQNEWEJ6` exact `画像一覧`: 12 ms, down from 381 ms.
  - `_DCT_KQNEWJE5` exact `外字一覧`: 12 ms, down from 271 ms.
  - `ssed_title_label_fast_prepass` appeared on 28 exact search rows.

### 0an. SSED digit/no-alpha full-text forward-title fast-hit probe (resolved, full gate)

Why this matters:

- The latest baseline showed a cluster of SSED full-text rows spending hundreds
  of milliseconds on an empty initial forward-title prepass before the useful
  partial-title or direct HONMON body phase:
  - `_DCT_NMEDEJ12` `search_full_text` `01`: 556 ms, direct body scan after the
    empty forward/title setup.
  - `_DCT_KQDENTAL` `search_full_text` `01`: 546 ms, title-index prepass after
    an empty forward probe.
  - `_DCT_GKBUSINE` `search_full_text` `10`: 387 ms, title-index prepass after
    an empty forward probe.
  - `_DCT_NCOMP4` `search_full_text` `1計`: 545 ms, partial non-prefix title
    prepass with intentionally deferred continuation proof.
- Direct cursor probes showed that the no-cursor orchestration was the avoidable
  cost; direct body or partial/title continuation paths were materially faster.
- A broad "skip digit queries" rule was rejected because yearly GEN dictionaries
  have real digit prefix hits such as `GEN2011` `01` and `GEN2009` `0`.

Current status:

- Initial SSED full-text forward-title prepass is now conditional for
  single-token queries containing ASCII digits and no ASCII letters. Those
  queries run a one-leaf near-key forward-hit probe first.
- If the probe finds a forward title hit, the normal forward-title prepass still
  runs. If the probe is empty, LVCore skips the expensive empty prepass and lets
  the existing partial-title/body phases handle the query.
- Normal alphabetic and mixed ASCII-letter queries keep the previous behavior.
- `SsedDataFile::read_range` now reserves the requested bounded range length
  instead of the chunk-size cap, avoiding avoidable buffer growth on larger
  bounded reads.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo check -p lvcore`
  - `cargo test -p lvcore search_ssed -- --nocapture`
  - `cargo test -p lvcore --lib ssed_search -- --nocapture`
  - `cargo test -p lvcore-cli validate_render_mode_probe_skips_mode_invariant_surface_views -- --nocapture`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-digit-forward-fast-probe-v2.jsonl`
  - Package status: 10 `ok`.
  - `_DCT_NMEDEJ12` `search_full_text` `01`: 166 ms, direct body scan.
  - `_DCT_KQDENTAL` `search_full_text` `01`: 190 ms, same title-index first
    hit.
  - `_DCT_GKBUSINE` `search_full_text` `10`: 26 ms, same title-index first hit.
  - `_DCT_NCOMP4` `search_full_text` `1計`: 441 ms, partial non-prefix title
    prepass.
  - GEN digit-prefix controls stayed fast: `_DCT_GEN2011` `01` at 13 ms and
    `_DCT_GEN2009` `0` at 63 ms, both through title-index prepass.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-digit-forward-fast-probe-v2.jsonl`
  - 335 reconstructed package paths validated with package status 335 `ok`.
  - Warning diagnostics remained only the explicitly deferred HC common HTML
    fallback.
  - `_DCT_NMEDEJ12` `search_full_text` `01`: 169 ms, down from 556 ms.
  - `_DCT_KQDENTAL` `search_full_text` `01`: 190 ms, down from 546 ms.
  - `_DCT_GKBUSINE` `search_full_text` `10`: 27 ms, down from 387 ms.
  - `_DCT_NCOMP4` `search_full_text` `1計`: 432 ms, down from 545 ms but still
    a remaining candidate because continuation proof is intentionally deferred.
  - GEN digit-prefix controls stayed fast: `_DCT_GEN2011` `01` at 13 ms and
    `_DCT_GEN2009` `0` at 64 ms.

### 0am. SSED large pure-kana simple-search native-first probe (resolved, full gate)

Why this matters:

- The latest full-corpus gate exposed a clean non-HC simple-search latency
  cluster in large SSED sidecar packages:
  - Windows `_DCT_KENROWA` exact `しめ`: 427 ms.
  - Windows `_DCT_KENROWA` forward `しめ`: 415 ms.
  - Windows `_DCT_KENROWA` backward `しめ`: 493 ms after exact/forward was
    fixed.
  - iOS `KENROWA` exact `しめ`: 454 ms.
  - iOS `KENROWA` forward `しめ`: 423 ms.
  - iOS `KENROWA` backward `しめ`: 449 ms after exact/forward was fixed.
  - Windows `_DCT_JSSAURU2` exact `あぎと`: 432 ms.
  - Windows `_DCT_JSSAURU2` forward `あぎ`: 431 ms.
  - Windows `_DCT_JSSAURU2` backward `ぎと`: 77 ms after exact/forward was
    fixed.
  - iOS `JSSAURU2` exact `あぎと`: 462 ms.
  - iOS `JSSAURU2` forward `あぎ`: 446 ms.
  - iOS `JSSAURU2` backward `ぎと`: 86 ms after exact/forward was fixed.
- Direct `--cursor 0` probes showed the native title/index path was already
  fast. The avoidable work was the initial no-cursor sidecar-title prepass
  scanning a large sidecar table before native index search.
- A blanket kana exclusion was rejected because single-kana and small sidecar
  kana searches such as `_DCT_DAIJIRN4` `あ`, `_DCT_IWKOKUG8` `さん`,
  `KQJCOLLO` `あい`, and `KQCOLEXP` `ああ` legitimately use sidecar titles.

Current status:

- For no-cursor SSED exact/forward/backward searches, large sidecar-title
  packages now try the native path first when the query is pure kana and at
  least two characters long. If that native probe finds hits, the page is
  returned immediately; if native finds nothing, the existing sidecar/title
  fallback behavior remains available.
- Backward search is included for this large pure-kana class because the native
  backward index preserves title-index search semantics and avoids sidecar
  display-title suffix scans that can be both slow and narrower than the native
  result set.
- Single-kana sidecar-title searches are also excluded to preserve
  `_DCT_DAIJIRN4` and similar sidecar-backed rows.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo build -p lvcore-cli`
  - `cargo test -p lvcore search_ssed -- --nocapture`
  - `cargo test -p lvcore sidecar_title -- --nocapture`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-native-first-kana-backward-v1.jsonl`
  - Package status: 10 `ok`.
  - Windows `_DCT_KENROWA` exact `しめ`: 54 ms, forward `しめ`: 28 ms,
    backward `しめ`: 9 ms.
  - iOS `KENROWA` exact `しめ`: 54 ms, forward `しめ`: 30 ms, backward
    `しめ`: 9 ms.
  - Windows `_DCT_JSSAURU2` exact `あぎと`: 38 ms, forward `あぎ`: 35 ms,
    backward `ぎと`: 14 ms.
  - iOS `JSSAURU2` exact `あぎと`: 33 ms, forward `あぎ`: 34 ms, backward
    `ぎと`: 15 ms.
  - Sidecar controls stayed sidecar-backed for `_DCT_DAIJIRN4` `あ`,
    `_DCT_IWKOKUG8` `さん`/`ィー`, iOS `KQJCOLLO` `あい`/`しい`, iOS
    `KQCOLEXP` `ああ`, and Windows/iOS `NANMED20` `0歳`/`余命`.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-native-first-kana-backward-v1.jsonl`
  - 335 reconstructed package paths validated with package status 335 `ok`.
  - Reconstructed path-list hash:
    `b3c7c347c4587d6e3b29db8767748f2ebd9402b17f712dd5ae980d45cc5a601a`.
  - Warning diagnostics remained only the explicitly deferred HC common HTML
    fallback.
  - Windows `_DCT_KENROWA` exact `しめ`: 53 ms, forward `しめ`: 27 ms,
    backward `しめ`: 10 ms.
  - iOS `KENROWA` exact `しめ`: 54 ms, forward `しめ`: 30 ms, backward
    `しめ`: 10 ms.
  - Windows `_DCT_JSSAURU2` exact `あぎと`: 33 ms, forward `あぎ`: 34 ms,
    backward `ぎと`: 14 ms.
  - iOS `JSSAURU2` exact `あぎと`: 33 ms, forward `あぎ`: 34 ms, backward
    `ぎと`: 14 ms.

### 0al. SSED full-text direct body prefetch gate (resolved, full gate)

Why this matters:

- The latest full-corpus gate had exactly three SSED full-text rows using the
  initial row-driven native body prefetch before falling through to direct
  HONMON byte-window scanning:
  - `_DCT_NMEDEJ12` `search_full_text` `01`: 663 ms.
  - `_DCT_GEN2005` `search_full_text` `曙光`: 194 ms.
  - `_DCT_KENE7J5` `search_full_text` `は殺`: 330 ms.
- In all three rows, the row prefetch decoded zero hits before direct scanning
  found the first hit. Once the direct scanner can seek byte candidates, this
  initial prefetch is redundant for byte-candidate queries.

Current status:

- Initial no-cursor SSED full-text body search skips row-driven prefetch when
  byte candidates exist and proceeds directly to the byte-window HONMON scan.
- Explicit row cursors, sidecar-body phase cursors, and no-byte-candidate
  initial searches keep the existing row-driven path.
- Focused tests passed:
  - `cargo fmt`
  - `cargo test -p lvcore search_ssed -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-direct-body-skip-initial-row-prefetch-v1.jsonl`
  - Package status: 3 `ok`.
  - `_DCT_NMEDEJ12` `search_full_text` `01`: 655 ms, now only
    `ssed_fulltext_body_direct_scan`.
  - `_DCT_GEN2005` `search_full_text` `曙光`: 105 ms, down from 194 ms, now
    only `ssed_fulltext_body_direct_scan`.
  - `_DCT_KENE7J5` `search_full_text` `は殺`: 331 ms, essentially unchanged,
    now only `ssed_fulltext_body_direct_scan`.
- This does not close the remaining `_DCT_NMEDEJ12` direct-scan cost. Further
  improvement likely requires direct HONMON scan-window sizing or streaming
  changes and should be evaluated across all direct-scan rows before editing.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-circle-marker-row-prefetch-v1.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - Path-set hash matched the previous 336-row baseline.
  - Warning diagnostics remained only the explicitly deferred HC common HTML
    fallback.
  - Initial row-driven full-text body prefetch no longer appears in the gate.
  - `_DCT_NMEDEJ12` `search_full_text` `01`: 621 ms.
  - `_DCT_GEN2005` `search_full_text` `曙光`: 101 ms.
  - `_DCT_KENE7J5` `search_full_text` `は殺`: 276 ms.

### 0ak. KOJIEN7 native circle-marker title search (resolved, full gate)

Why this matters:

- The latest full-corpus gate exposed a clean non-HC SSED search gap in both
  KOJIEN7 package layouts:
  - Windows `_DCT_KOJIEN7` exact
    `◯に滅せずんば炎炎を若何いかんせん`: 708 ms.
  - Windows `_DCT_KOJIEN7` forward `◯に`: 675 ms.
  - iOS `KOJIEN7` exact `◯に滅せずんば炎炎を若何いかんせん`: 718 ms.
  - iOS `KOJIEN7` forward `◯に`: 710 ms.
- Direct inspection showed the real hit is a normal native `CRINDEX.DIC` title
  hit. The expensive work was the dense sidecar title prepass and auto-append
  preference check, not native index lookup.
- Cursor and direct probes showed native search is already fast once those
  sidecar prepasses are skipped for this query class.

Current status:

- SSED sidecar title prepasses now stay bounded for the existing authoritative
  cases, but skip native circle-marker title queries containing `◯` or `○`.
- The change is intentionally narrow. Normal dense sidecar title searches still
  run for ordinary CJK sidecar-backed title queries; direct probing of forward
  `あ` on KOJIEN7 still uses `ssed_sidecar_title_search`.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo build -p lvcore-cli`
  - `cargo test -p lvcore sidecar_title_auto_append_accepts_bounded_single_token_queries -- --nocapture`
  - `cargo test -p lvcore sidecar_title_authoritative_prepass_skips_native_circle_markers -- --nocapture`
  - `cargo test -p lvcore search_ssed -- --nocapture`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-kojien7-circle-native-title-v1.jsonl`
  - Package status: 2 `ok`.
  - Windows `_DCT_KOJIEN7` exact
    `◯に滅せずんば炎炎を若何いかんせん`: 43 ms.
  - Windows `_DCT_KOJIEN7` forward `◯に`: 43 ms.
  - iOS `KOJIEN7` exact `◯に滅せずんば炎炎を若何いかんせん`: 50 ms.
  - iOS `KOJIEN7` forward `◯に`: 49 ms.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-circle-marker-row-prefetch-v1.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - Path-set hash matched the previous 336-row baseline.
  - Warning diagnostics remained only the explicitly deferred HC common HTML
    fallback.
  - Windows `_DCT_KOJIEN7` exact
    `◯に滅せずんば炎炎を若何いかんせん`: 45 ms.
  - Windows `_DCT_KOJIEN7` forward `◯に`: 67 ms.
  - iOS `KOJIEN7` exact `◯に滅せずんば炎炎を若何いかんせん`: 54 ms.
  - iOS `KOJIEN7` forward `◯に`: 60 ms.

### 0aj. HKKIGAK6 bounded partial-title first page (resolved, focused)

Why this matters:

- The latest full-corpus gate exposed `Other/iOS/HKKIGAK6/HKKIGAK6`
  `search_partial` query `体の` at 502 ms, with no HC involvement and no
  actionable warning diagnostics.
- Direct probing showed the first native partial hit was correct, but partial
  search spent avoidable time proving that the prefix phase was empty before
  scanning the bounded native partial-title index. Full-text already reached
  the same title through the bounded title-index prepass.
- The iOS SQLite sidecars do not preserve the native first result for this
  query class, so this remains a native SSED title/index path rather than a
  SQLite shortcut.

Current status:

- Initial bounded partial searches with byte prefilter candidates now run a
  fast native prefix probe for packages whose title indexes are small enough
  for the existing bounded all-title scan. If the prefix probe is empty, LVCore
  proceeds directly to the bounded native partial-title page instead of running
  the slower prefix fallback.
- The first page returns the same native hit, `ＨＬＡＢ27と体の病気`, and now uses
  `ssed-partial-nonprefix-noskip-offset:1`; the cursor probe then resumes into
  the existing physical sparse continuation path.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo build -p lvcore-cli`
  - `cargo test -p lvcore search_ssed -- --nocapture`
  - `cargo test -p lvcore partial_nonprefix_cursors_preserve_prefix_skip_state -- --nocapture`
  - `cargo test -p lvcore fulltext_title_cursors_preserve_physical_offset -- --nocapture`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-hkkigak6-partial-title-prepass-v1.jsonl`
  - Package status: `ok`, elapsed 13164 ms.
  - `search_partial` `体の`: 276 ms, down from 502 ms in the latest full gate.
  - Cursor probe: `ssed-partial-nonprefix-noskip-offset:1` returned one hit in
    36 ms and continued as
    `ssed-partial-nonprefix-noskip-physical-offset:5:79:2`.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-hkkigak6-partial-title-prepass-v1.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package path set is fully covered.
  - Warning diagnostics remain only the explicitly deferred HC common HTML
    fallback.

### 0ai. Mode-invariant surface render-mode validation overwork (resolved, focused)

Why this matters:

- The latest full-corpus gate exposed several non-HC `surface_first_target`
  rows where the slow work was validation, not package behavior. A concrete
  case was `Other/iOS/RDRSP2/RDRSP2` `ios-plist:indexSearch.plist`: 2073 ms,
  no resource-scan cost, no link-scan cost, and no HC diagnostics.
- Direct probing showed the surface itself opened quickly. The repeated work
  came from validating GenericHtml and BasicText render modes for surface views
  such as `panel_surface` and `navigation_surface`; those views do not produce
  entry-body HTML/text under render-mode changes.

Current status:

- Deep validation now skips render-mode contract probes for mode-invariant
  surface views: `navigation_surface`, `panel_surface`, and `deferred`.
  Entry-body, info-page, hanrei, law, and search-result views still run the
  GenericHtml/BasicText contract probes.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore-cli validate_render_mode_probe_skips_mode_invariant_surface_views -- --nocapture`
  - `cargo test -p lvcore-cli validate_deep_exercises -- --nocapture`
  - `cargo test -p lvcore-cli validate -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-mode-invariant-surface-v1.jsonl`
  - 3 packages validated with package status 3 `ok`: `RDRSP2`, iOS
    `SINMEI7`, and Windows SSED `_DCT_SINMEI7`.
  - `RDRSP2` `ios-plist:indexSearch.plist` dropped from 2073 ms in the latest
    full gate to 1068 ms focused, with render modes explicitly skipped as
    `mode_invariant_surface`.
  - iOS `SINMEI7` `menu` and `ios-plist:dataCONV.plist` now record the same
    explicit skip for `navigation_surface` render-mode probes.
- Full-corpus regression gate
  `/tmp/lvcore-all-corpora-validation-20260613-hkkigak6-partial-title-prepass-v1.jsonl`
  includes this cleanup. `RDRSP2` `ios-plist:indexSearch.plist` is now 1001 ms
  with render modes explicitly skipped as `mode_invariant_surface`.

### 0ah. SSED title-prepass filled-page stop (resolved, full gate)

Why this matters:

- The latest full-corpus gate exposed a concrete non-HC title-prepass cluster:
  `_DCT_KQDENTAL` full-text query `01` at 681 ms, `_DCT_YHOUGO3` full-text
  query `一ス` at 660 ms, and `_DCT_HKDKSR10` full-text query `FU` at 596 ms.
- The initial SSED full-text title/index prepass was collecting `limit + 1`
  candidates to prove a title continuation. For this path, that proof is
  avoidable: once the requested page is filled, LVCore immediately applies the
  existing body/title continuation policy instead of needing to surface that
  extra candidate.

Current status:

- The initial title/index prepass now collects only `query.limit` visible hits
  and uses the pending page-limit stop path.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore package::drivers::tests::fulltext -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-title-prepass-filled-page-stop-v1.jsonl`
  - `_DCT_KQDENTAL` `search_full_text` `01`: 579 ms, down from 681 ms; cursor
    probe 167 ms in the full gate.
  - `_DCT_YHOUGO3` `search_full_text` `一ス`: 613 ms, down from 660 ms.
  - `_DCT_HKDKSR10` `search_full_text` `FU`: 427 ms, down from 596 ms.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-title-prepass-filled-page-stop-v1.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - Path set matched the previous 336-row baseline, including the two
    `Other/Android` rows.
  - Warning diagnostics remained only the explicitly deferred HC common HTML
    fallback.

### 0ag. SSED direct HONMON filled-page cursor (resolved, full gate)

Why this matters:

- The latest full-corpus gate exposed `_DCT_NMEDEJ12` full-text query `01` as
  a concrete non-HC SSED body-search gap: 807 ms, five scanned direct HONMON
  windows, and a `body-offset:*` continuation.
- Direct probing also showed that following the old `body-offset:*` cursor could
  spend about 12 seconds in the row-driven physical cursor path for the next
  page.
- While inspecting that path, direct byte-window scanning was found to inspect
  only the first byte-candidate entry in each scan window. That could skip
  earlier body hits in numeric/common-byte windows.

Current status:

- Direct HONMON full-text scanning now walks byte-candidate entries within the
  current scan window before advancing to the next window.
- Filled direct body pages now return a physical `body-offset:*` cursor at the
  last returned hit without proving an extra hit. Following that cursor resumes
  direct byte-window scanning from the next body entry instead of first building
  a 4096-row index resume set.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore package::drivers::tests::fulltext -- --nocapture`
  - `cargo test -p lvcore-cli validate_search_cursor_probe_skips_expensive_fulltext_body_cursors -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-direct-body-filled-page-cursor-v2.jsonl`
  - `_DCT_NMEDEJ12` package status `ok`; `search_full_text` `01` scanned one
    direct body window and returned one hit in 707 ms.
  - `_DCT_KENE7J5` package status `ok`; `search_full_text` `は殺` scanned one
    direct body window and returned one hit in 364 ms.
- Direct real-package cursor probes:
  - `_DCT_NMEDEJ12` first page `01`: 0.72s wall time, one scanned direct body
    window, cursor `body-offset:484f4e4d4f4e2e444943:5f5a2`.
  - Following that cursor returned the next body hit in 0.11s wall time through
    `ssed_fulltext_body_cursor_scan`. The old row-driven physical cursor path
    had taken about 12s on the prior cursor.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-direct-body-filled-page-cursor-v2.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - Path set matched the previous 336-row baseline, including the two
    `Other/Android` rows.
  - Warning diagnostics remained only the explicitly deferred HC common HTML
    fallback.
  - `_DCT_NMEDEJ12` `search_full_text` `01`: 686 ms, one scanned direct body
    window, down from 807 ms and five scanned windows.
  - `_DCT_KENE7J5` `search_full_text` `は殺`: 343 ms, down from 607 ms.
  - `_DCT_GEN2005` `search_full_text` `曙光`: 193 ms, down from 506 ms.

### 0af. SSED tagged non-prefix title page prefilter (resolved, full gate)

Why this matters:

- The previous full-corpus gate exposed `_DCT_NCOMP4` full-text query `1計` as
  a concrete non-HC gap: 2056 ms to return one title hit from native indexes.
- The hit lives behind tagged/sparse native index pages. Naively treating
  tagged pages as byte-prefilter-safe is wrong because `0xc0` continuation rows
  inherit the current tagged group key from prior records or pages.

Current status:

- Large non-prefix title scans now opt into page-prefilter extensions: a
  tagged-leaf state prefilter that tracks whether the inherited tagged group
  key can match the query, and an in-memory candidate-page jump for bounded
  simple index components. Broader title prepasses keep the previous
  conservative streaming behavior.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore package::drivers::ssed_index::tests -- --nocapture`
  - `cargo test -p lvcore package::ssed_search::tests -- --nocapture`
  - `cargo test -p lvcore package::drivers::tests::fulltext -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-tagged-nonprefix-scope-v3.jsonl`
  - `_DCT_KQNEWEJ6` `search_full_text` `画像`: 422 ms, hit_count 1, retained
    the prior fast title-prepass path.
  - `_DCT_NCOMP4` `search_full_text` `1計`: 547 ms, hit_count 1.
  - `_DCT_KENE7J5` `search_full_text` `は殺`: 634 ms, hit_count 1, retained
    the prior native body-scan path.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-tagged-nonprefix-prefilter-v3.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered, including the
    two `Other/Android` rows.
  - Warning diagnostics remain only the explicitly deferred HC common HTML
    fallback.
  - `_DCT_KQNEWEJ6` `search_full_text` `画像`: 406 ms, matching the previous
    fast title-prepass path rather than the broader in-memory prefilter.
  - `_DCT_NCOMP4` `search_full_text` `1計`: 545 ms, down from 2056 ms.
  - `_DCT_KENE7J5` `search_full_text` `は殺`: 607 ms, matching the previous
    body-scan path rather than the broader tagged-title prepass.

### 0ae. SSED sidecar-body cursor phase deferral (resolved, full gate)

Why this matters:

- The latest full-corpus gate exposed Android SSED `.MEIKYO2R` and
  `.MEIKYO2R_renew` as concrete non-HC sidecar continuation latency rows:
  - `.MEIKYO2R` `search_full_text` `仝`: 1173 ms.
  - `.MEIKYO2R_renew` `search_full_text` `仝`: 1160 ms.
- The first page returned the expected sidecar body hit quickly, but deep
  validation followed the physical `sidecar-body-row:*` cursor. Once the
  sidecar table was exhausted, LVCore immediately entered native HONMON
  row-driven/direct body scanning and spent about 1.1s proving there were no
  native body hits.
- Direct package inspection showed `MEIKYO2R.db` has one body hit for `仝` at
  `MEIKYO2R.No = 42631`; the raw SQLite continuation after that row is fast and
  empty.

Current status:

- Explicit full-text sidecar-body continuation cursors now stop at the sidecar
  phase boundary. If the sidecar phase is exhausted before filling the page,
  LVCore returns the hits collected so far plus the existing `body:0`
  continuation when native HONMON scanning is still eligible.
- This preserves completeness for callers that continue searching, while
  preventing sidecar cursor probes from doing native body scanning as hidden
  follow-up work.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore package::drivers::tests::dense_sidecar -- --nocapture`
  - `cargo test -p lvcore package::drivers::tests::fulltext -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-meikyo2r-sidecar-phase-deferral-v1.jsonl`
  - `.MEIKYO2R` package status `ok`, elapsed 1531 ms.
  - `.MEIKYO2R` `search_full_text` `仝`: 44 ms; cursor probe 16 ms,
    hit_count 0, remaining cursor `body:0`.
  - `.MEIKYO2R_renew` package status `ok`, elapsed 1680 ms.
  - `.MEIKYO2R_renew` `search_full_text` `仝`: 44 ms; cursor probe 16 ms,
    hit_count 0, remaining cursor `body:0`.
- Direct real-package probes:
  - `lvcore search .../.MEIKYO2R '仝' --mode full-text --limit 1`: about 0.04s.
  - Following the returned `sidecar-body-row:*` cursor: about 0.05s, hit_count
    0, remaining cursor `body:0`.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-sidecar-body-phase-deferral-v1.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered, including the
    two `Other/Android` rows.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (261),
    which is deferred HC work.
  - `.MEIKYO2R` package status `ok`, elapsed 1574 ms.
  - `.MEIKYO2R` `search_full_text` `仝`: 59 ms; cursor probe 29 ms,
    hit_count 0, remaining cursor `body:0`.
  - `.MEIKYO2R_renew` package status `ok`, elapsed 1669 ms.
  - `.MEIKYO2R_renew` `search_full_text` `仝`: 42 ms; cursor probe 15 ms,
    hit_count 0, remaining cursor `body:0`.

Baseline evidence:

- Latest full-corpus JSONL:
  `/tmp/lvcore-all-corpora-validation-20260613-nonascii-sidecar-prepass-v1.jsonl`
- Observed rows in that baseline:
  - `.MEIKYO2R` `search_full_text` `仝`: 1173 ms, diagnostic
    `ssed_fulltext_sidecar_scan`, cursor `sidecar-body-row:*`; cursor probe
    1142 ms with native body diagnostics.
  - `.MEIKYO2R_renew` `search_full_text` `仝`: 1160 ms, diagnostic
    `ssed_fulltext_sidecar_scan`, cursor `sidecar-body-row:*`; cursor probe
    1128 ms with native body diagnostics.

### 0ad. iOS SSED non-ASCII sidecar title/body prepass (resolved, full gate)

Why this matters:

- The latest full-corpus gate exposed
  `/home/shoui/Agents/CodexMax/LogoVista/Other/iOS/IBIO5/IBIO5` as a concrete
  non-HC iOS SSED sidecar latency cluster for query `亜-`:
  - `search_exact`: 585 ms.
  - `search_forward`: 496 ms.
  - `search_backward`: 543 ms.
  - `search_full_text`: 622 ms.
  - Package validation elapsed: 6054 ms.
- Direct package inspection showed `IBIO5.sql` stores renderable titles and
  bodies in sidecar tables such as
  `IBIO5_1(No, Block, Offset, Title, Body, TitleJIS)`, with the first `亜-`
  hit available as sidecar row `No = 1`.
- Raw SQLite probes were effectively instant, so the cost was LVCore control
  flow: title search ran the dense-sidecar/native preference probe before
  returning the sidecar title page, and full-text search sampled native
  HONMON/index payloads before using the sidecar body row that could fill the
  page.

Current status:

- Exact/forward/backward SSED title search now tries an authoritative
  non-ASCII sidecar title page before the dense-sidecar/native preference
  probe. If the sidecar page has visible hits, the search returns directly.
- Full-text SSED search now tries an authoritative non-ASCII sidecar body page
  before computing whether native HONMON body-window scanning is needed. If the
  sidecar page fills the requested limit, the search returns directly.
- Medium/large authoritative non-ASCII sidecar title searches defer
  exact/forward/backward continuation proof behind the existing
  `sidecar-title-unverified-row:*` cursor path.
- SSED navigation detection now checks only the first parsed menu page when it
  only needs to know whether a menu component has a non-empty surface.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore package::drivers::tests::dense_sidecar -- --nocapture`
  - `cargo test -p lvcore package::drivers::tests::fulltext -- --nocapture`
  - `cargo test -p lvcore package::drivers::tests::ssed_navigation_surfaces:: -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-ibio5-nonascii-sidecar-prepass-v1.jsonl`
  - Package status `ok`, elapsed 1844 ms.
  - `search_exact` `亜-`: 2 ms, hit_count 1, cursor
    `sidecar-title-unverified-row:*`.
  - `search_forward` `亜-`: 18 ms, hit_count 1.
  - `search_backward` `亜-`: 2 ms, hit_count 1, cursor
    `sidecar-title-unverified-row:*`.
  - `search_full_text` `亜-`: 9 ms, hit_count 1, cursor
    `sidecar-body-row:*`.
- Direct real-package probes:
  - `lvcore search .../IBIO5 '亜-' --mode exact --limit 1`: about 0.04s after
    warmup.
  - `lvcore search .../IBIO5 '亜-' --mode forward --limit 1`: about 0.05s.
  - `lvcore search .../IBIO5 '亜-' --mode backward --limit 1`: about 0.04s.
  - `lvcore search .../IBIO5 '亜-' --mode full-text --limit 1`: about 0.06s.
  - `lvcore home .../IBIO5`: about 1.19s after the first-page menu probe
    change.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-nonascii-sidecar-prepass-v1.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered, including the
    two `Other/Android` rows.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (261),
    which is deferred HC work.
  - IBIO5 package status remained `ok`, elapsed 1778 ms.
  - `search_exact` `亜-`: 2 ms.
  - `search_forward` `亜-`: 21 ms.
  - `search_backward` `亜-`: 2 ms.
  - `search_full_text` `亜-`: 9 ms.

Baseline evidence:

- Latest full-corpus JSONL:
  `/tmp/lvcore-all-corpora-validation-20260613-ssed-jis-prefilter-memmem-v2.jsonl`
- Observed IBIO5 rows in that baseline:
  - `search_exact` `亜-`: 585 ms, diagnostic `ssed_sidecar_title_search`.
  - `search_forward` `亜-`: 496 ms, diagnostic `ssed_sidecar_title_search`.
  - `search_backward` `亜-`: 543 ms, diagnostic `ssed_sidecar_title_search`.
  - `search_full_text` `亜-`: 622 ms, diagnostic
    `ssed_fulltext_sidecar_scan`, cursor `sidecar-body-row:*`.

### 0ac. SSED separator-aware JIS prefilter seek (resolved, full gate)

Why this matters:

- The latest full gate still had concrete non-HC SSED search latency in native
  title/body byte prefilters:
  - `_DCT_NCOMP4`, full-text `1計`: 2540 ms before this change.
  - `_DCT_KENE7J5`, full-text `は殺`: 905 ms before this change.
  - `_DCT_NMEDEJ12`, full-text `01`: 900 ms before this change.
- The shared SSED byte prefilter supported LogoVista title separators inside
  JIS pair sequences, but it tested every byte offset in each candidate page or
  body window.
- That was semantically correct but expensive for sparse native index pages and
  Japanese body windows.

Current status:

- `contains_jis_pair_sequence_with_title_separators` now seeks to occurrences
  of the first two-byte JIS pair with `memmem`, then verifies the remaining JIS
  pairs with the existing title-separator skipping rule.
- The search advances by one byte after each first-pair candidate, so
  overlapping candidate starts remain covered and semantics match the previous
  every-offset scan.
- No cursor formats or validation skip policies changed.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore package::ssed_search::tests:: -- --nocapture`
  - `cargo test -p lvcore package::drivers::tests::fulltext::ssed_fulltext_searches_late_nonprefix_title_before_body_scan -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-ssed-jis-prefilter-memmem-v2.jsonl`
  - `_DCT_NCOMP4` package status remained `ok`; `search_full_text` `1計`
    improved from 2540 ms to 2153 ms in focused validation.
  - `_DCT_KENE7J5` package status remained `ok`; `search_full_text` `は殺`
    improved from 905 ms to 631 ms in focused validation.
  - `_DCT_NMEDEJ12` package status remained `ok`; `search_full_text` `01`
    improved from 900 ms to 864 ms in focused validation.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-ssed-jis-prefilter-memmem-v2.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (261),
    which is deferred HC work.
  - Full-gate timing examples:
    - `_DCT_NCOMP4` full-text `1計`: 2540 ms to 2262 ms.
    - `_DCT_KENE7J5` full-text `は殺`: 905 ms to 633 ms.
    - `_DCT_NMEDEJ12` full-text `01`: 900 ms to 781 ms.

### 0ab. iOS SSED tableList cross-book row shortcut (resolved, full gate)

Why this matters:

- The previous full-corpus gate exposed `Other/iOS/KQNEWJE5/KQNEWJE5`
  `ios-table-list:tableList.plist` as a concrete non-HC navigation surface
  latency row: 5462 ms total exercise time with a 1883 ms cursor probe.
- Focused baseline validation reproduced the same path at 3415 ms for the
  surface exercise and 1177 ms for the cursor probe.
- Direct package timing isolated the non-HC cost: `home` took about 4.7s and
  each tableList page took about 1.45s before rendering. The rows are
  cross-book addresses owned by sibling `KQNEWEJ6`, but the source package was
  trying local loose-address resolution for every row before falling back to
  cross-book targets.

Current status:

- iOS `tableList.plist` cross-book owner resolution is cached per package and
  source id.
- tableList status/page/window handling now uses a cheap local catalog address
  check. When no tableList rows are locally owned and a sibling owner is known,
  lvcore emits cross-book targets directly instead of paying repeated local
  loose-address misses.
- Mixed local/cross-book tableLists still lazily fall back to owner detection
  after a local miss.
- Package-level tableList sequence windows now recognize cross-book tableList
  targets instead of scanning the plist and reporting that the target is absent.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore package::drivers::tests::ssed_navigation_surfaces::ssed_ios_table_list -- --nocapture`
  - `cargo test -p lvcore-cli tests::validate_deep_routes_ios_table_list_cross_book_sibling -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-kqnewje5-table-list-shortcut-v2.jsonl`
  - `KQNEWJE5` package status remained `ok`.
  - Package validation dropped from 20956 ms focused baseline to 6275 ms.
  - `ios-table-list:tableList.plist` dropped from 3415 ms to 1044 ms.
  - Its cursor probe dropped from 1172 ms to 0 ms.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-ios-table-list-cross-book-shortcut-v1.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (1924),
    which is deferred HC work.
  - `Other/iOS/KQNEWJE5/KQNEWJE5` package validation dropped from 33538 ms to
    6064 ms versus the previous full gate.
  - `ios-table-list:tableList.plist` dropped from 5462 ms to 927 ms, and its
    cursor probe dropped from 1883 ms to 0 ms.
- Direct real-package timings:
  - `lvcore home .../Other/iOS/KQNEWJE5/KQNEWJE5` dropped from about 4.7s to
    about 0.07s locally.
  - `lvcore surface --limit 16 ... ios-table-list:tableList.plist` dropped
    from about 1.47s to about 0.03s locally.
  - `lvcore surface --limit 16 --cursor 16 ... ios-table-list:tableList.plist`
    dropped from about 1.44s to about 0.03s locally.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/Other/iOS/KQNEWJE5/KQNEWJE5`
- Observed row in
  `/tmp/lvcore-all-corpora-validation-20260613-unverified-nonprefix-title-v1.jsonl`:
  - `surface_first_target`, `ios-table-list:tableList.plist`, 5462 ms, cursor
    probe 1883 ms, routed to `SSED:KQNEWEJ6:*` with only
    `ssed_cross_book_routed` info diagnostics.

### 0aa. SSED full-text non-prefix title continuation deferral (resolved, full gate)

Why this matters:

- The previous full-corpus gate exposed `_DCT_NCOMP4` full-text search for
  `1計` as the largest non-HC performance row: validation spent 4704 ms on the
  first page, then another 2413 ms proving the `title-nonprefix:*`
  continuation.
- The first page already returned the visible title `0-1計画法`; the extra
  cursor proof was searching for the next distinct non-prefix title across
  sparse native index pages.
- This is not a body full-text problem and not HC. The diagnostics were
  `ssed_fulltext_partial_nonprefix_title_prepass` and
  `ssed_index_empty_physical_pages_skipped`.

Current status:

- Large SSED full-text non-prefix title prepass now fills the requested page and
  defers next-title proof behind an explicit
  `title-nonprefix-unverified:*` cursor.
- The cursor payload still carries the underlying partial non-prefix physical
  cursor plus already-returned targets, so manually following it resumes the
  same non-prefix title scan without repeating the visible first title.
- Deep validation treats this cursor as intentionally not probed, with reason
  `unverified full-text non-prefix title continuation may scan large SSED indexes`.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore package::drivers::tests::fulltext -- --nocapture`
  - `cargo test -p lvcore package::drivers::search_ssed::tests -- --nocapture`
  - `cargo test -p lvcore-cli validate_search_cursor_probe_skips_expensive_fulltext_body_cursors -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-ncomp4-unverified-nonprefix-title-v2.jsonl`
  - `_DCT_NCOMP4` package status remained `ok`.
  - `search_full_text` `1計` is 2083 ms focused, hit_count 1, with remaining
    cursor `title-nonprefix-unverified:*`.
  - The cursor probe is `not_probed` for the new explicit reason.
- Direct real-package probe:
  - `lvcore search .../_DCT_NCOMP4 1計 --mode full-text --limit 1` returns
    `0-1計画法` and a `title-nonprefix-unverified:*` cursor in about 2.2s
    locally.
  - Following that cursor resumes the non-prefix title scan and returns
    `zero-one programming` in about 2.9s locally.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-unverified-nonprefix-title-v1.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (1924),
    which is deferred HC work.
  - `_DCT_NCOMP4` `search_full_text` `1計` is 1990 ms, hit_count 1, with its
    non-prefix title continuation intentionally not probed.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_NCOMP4`
- Observed row in the previous full gate
  `/tmp/lvcore-all-corpora-validation-20260613-unverified-sidecar-title-v1.jsonl`:
  - `_DCT_NCOMP4`, `search_full_text`, query `1計`, 4704 ms, hit_count 1,
    diagnostics `ssed_fulltext_partial_nonprefix_title_prepass` and
    `ssed_index_empty_physical_pages_skipped`, with a 2413 ms
    `title-nonprefix:*` cursor probe.

### 0z. SSED exact CJK sidecar-title lookahead deferral (resolved, full gate)

Why this matters:

- The previous full-corpus gate exposed `_DCT_DAIJIRN4` exact search for `あ`
  as a concrete non-HC sidecar-title latency row: validation spent 701 ms on
  the first page and another 688 ms probing the sidecar title-row cursor.
- The first page only needed the visible exact title hit. The extra work was
  proving that another exact sidecar title row existed on a large dense sidecar
  table.
- Direct package inspection showed the row is sidecar-backed and the native
  index route is intentionally skipped, so this is a sidecar continuation proof
  problem rather than an HC or native-index problem.

Current status:

- Large authoritative exact CJK sidecar-title first pages now return the visible
  hit without one-extra-row lookahead.
- The continuation is explicit:
  `sidecar-title-unverified-row:<db>:<table>:<id-column>:direct:<rowid>`.
- Existing verified `sidecar-title-row:*` cursors still decode normally, and
  the new unverified cursor resumes through the same physical sidecar-row path.
- Deep validation treats the unverified cursor as intentionally not probed, with
  reason `unverified sidecar title continuation may scan large SSED sidecars`.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore package::drivers::tests::dense_sidecar -- --nocapture`
  - `cargo test -p lvcore-cli validate_search_cursor_probe_skips_expensive_fulltext_body_cursors -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-daijirn4-unverified-sidecar-title-v1.jsonl`
  - `_DCT_DAIJIRN4` package status remained `ok`.
  - `search_exact` `あ` is 13 ms, hit_count 1, with remaining cursor
    `sidecar-title-unverified-row:*`.
  - The cursor probe is `not_probed` for the new explicit reason.
- Direct real-package probe:
  - `lvcore search .../_DCT_DAIJIRN4 あ --mode exact --limit 1` returns the
    expected `あ` hit and a `sidecar-title-unverified-row:*` cursor in about
    0.08s locally.
  - Following the unverified cursor resumes the sidecar physical-row search and
    returns the next exact title row in about 0.05s locally.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-unverified-sidecar-title-v1.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (1924),
    which is deferred HC work.
  - `_DCT_DAIJIRN4` `search_exact` `あ` is 13 ms, with its sidecar-title
    continuation intentionally not probed.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_DAIJIRN4`
- Observed row in the previous full gate
  `/tmp/lvcore-all-corpora-validation-20260613-cjk-sidecar-prefix-v1.jsonl`:
  - `_DCT_DAIJIRN4`, `search_exact`, query `あ`, 701 ms, hit_count 1,
    diagnostics `ssed_native_index_search_skipped_sidecar_backed` and
    `ssed_sidecar_title_search`, with a 688 ms sidecar title-row cursor probe.

### 0y. SSED CJK partial sidecar-prefix fast path (resolved, full gate)

Why this matters:

- The previous full-corpus gate exposed `_DCT_GENKANA5` partial search for
  `アル` as a concrete non-HC first-page latency gap: validation spent 914 ms
  before returning the visible sidecar title
  `アルカーイダ【Al-Qaeda；Al-Qaida】`.
- The cursor probe was already fast, so the gap was first-page orchestration,
  not continuation correctness.
- Direct inspection showed the relevant `vlpljbl` sidecar stores visual titles
  in `TEXT` columns; direct sidecar-title cursor search returned the same hit
  in about 0.07s, while the ordinary first page paid native prefix routing work
  first.

Current status:

- Partial-prefix search now tries authoritative CJK sidecar title prefixes
  before native prefix scanning.
- The returned cursor remains wrapped as
  `ssed-partial-prefix:sidecar-title-row:*`, preserving the existing partial
  prefix continuation shape.
- Dense sidecar discovery no longer eagerly scans every block/offset table for
  min/max block ranges. Address lookup still uses exact block/offset matching
  when ranges are unknown, but title search no longer pays that setup cost.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore ssed_sidecar::tests::sidecar_body_discovery_leaves_block_ranges_lazy -- --nocapture`
  - `cargo test -p lvcore package::drivers::tests::dense_sidecar -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-genkana5-cjk-sidecar-prefix-v2.jsonl`
  - `_DCT_GENKANA5` package status remained `ok`.
  - `search_partial` `アル` dropped from 914 ms in the previous full-gate
    baseline to 13 ms, with hit_count 1 and diagnostics
    `ssed_partial_prefix_prepass` plus `ssed_sidecar_title_search`.
  - The cursor probe remained `ok` at 5 ms and returned the next sidecar title
    row cursor.
- Direct real-package probe:
  - `lvcore search .../_DCT_GENKANA5 アル --mode partial --limit 1` now returns
    the same `アルカーイダ【Al-Qaeda；Al-Qaida】` hit in about 0.04s locally.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-cjk-sidecar-prefix-v1.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (1924),
    which is deferred HC work.
  - `_DCT_GENKANA5` `search_partial` `アル` remains at 13 ms, with an `ok`
    cursor probe at 5 ms.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_GENKANA5`
- Observed row in the previous full gate
  `/tmp/lvcore-all-corpora-validation-20260613-nonprefix-title-fulltext-v4.jsonl`:
  - `_DCT_GENKANA5`, `search_partial`, query `アル`, 914 ms, hit_count 1,
    diagnostics `ssed_partial_prefix_prepass` and `ssed_sidecar_title_search`.

### 0x. SSED full-text non-prefix native-title prepass (resolved, full gate)

Why this matters:

- The previous full-corpus gate exposed `_DCT_NCOMP4` full-text search for
  `1計` as a concrete non-HC correctness gap: partial search found the title
  `0-1計画法`, but full-text search returned no hits after row-driven and
  direct HONMON body scans.
- The match is a native title/index label where the query appears after the
  title prefix. Full-text title-before-body ordering needs to include that
  evidence before falling through to HONMON body scanning.

Current status:

- Full-text search now runs a bounded non-prefix native-title prepass for
  single-token mixed digit/punctuation plus non-ASCII queries.
- The first page returns non-prefix title hits before body scanning and emits
  `ssed_fulltext_partial_nonprefix_title_prepass`.
- Continuation uses `title-nonprefix:*`, carrying the partial non-prefix cursor
  and already-returned body targets so duplicate title entries from later native
  index components are skipped while later distinct title hits remain reachable.
- Pure numeric cases such as `_DCT_NMEDEJ12` full-text `01` remain on the body
  scan path and do not enter this prepass.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore package::drivers::tests::fulltext -- --nocapture`
  - `cargo test -p lvcore package::drivers::search_ssed::tests -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-ncomp4-nonprefix-title-fulltext-v3.jsonl`
  - `_DCT_NCOMP4` package status remained `ok`.
  - `search_full_text` `1計` now returns hit_count 1 with title
    `0-1計画法` and remaining cursor `title-nonprefix:*`.
  - The cursor probe is `ok` and returns the next distinct title
    `zero-one programming`.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-nonprefix-title-fulltext-v4.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (1936),
    which is deferred HC work.
  - `_DCT_NCOMP4` `search_full_text` `1計` now has hit_count 1, diagnostic
    `ssed_fulltext_partial_nonprefix_title_prepass`, and an `ok`
    `title-nonprefix:*` cursor probe.
  - `_DCT_NMEDEJ12` `search_full_text` `01` remains on
    row-driven/direct-body diagnostics at 838 ms, avoiding the earlier pure
    numeric no-hit title-scan regression.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_NCOMP4`
- Observed row in the previous full gate
  `/tmp/lvcore-all-corpora-validation-20260613-title-physical-offset.jsonl`:
  - `_DCT_NCOMP4`, `search_full_text`, query `1計`, 1440 ms, hit_count 0,
    row-driven body prefetch plus direct native HONMON scan.

### 0w. SSED full-text title prepass physical-offset cursor (resolved, full gate)

Why this matters:

- The previous full-corpus gate exposed `_DCT_KQNEWEJ6` full-text search for
  `画像` and `_DCT_KQDENTAL` full-text search for `01` as concrete non-HC
  title-prepass latency rows.
- Both rows found a native title/index hit first, then spent first-page time
  proving or locating the next native title continuation before body scanning.
- This made the first visible page slower even though the next phase can be
  resumed lazily.

Current status:

- Large SSED full-text partial title prepass now stops once it has a visible
  title hit instead of scanning to the broader leaf-page budget.
- It returns a physical-offset title cursor:
  `title:ssed-partial-index-offset:<component>:<page>:<matched>`.
- The continuation resumes at the same physical title-index page while skipping
  the already-returned matched title rows, preserving title-before-body ordering
  without first-page overfetch.
- Direct real-package probes:
  - `_DCT_KQNEWEJ6` full-text `画像 --limit 1`: about 0.24s; the returned
    cursor continued into direct body scan in about 0.37s without repeating
    `画像一覧`.
  - `_DCT_KQDENTAL` full-text `01 --limit 1`: about 0.46s; the returned cursor
    continued into direct body scan in about 0.37s without repeating
    `0.01規定の...`.
- Focused tests passed:
  - `cargo test -p lvcore package::drivers::search_ssed::tests -- --nocapture`
  - `cargo test -p lvcore package::drivers::tests::fulltext -- --nocapture`
  - `cargo test -p lvcore package::drivers::tests::dense_sidecar -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-kqnewej6-title-physical-offset.jsonl`
    - package status `ok`
    - `search_full_text` `画像`: 466 ms, cursor
      `title:ssed-partial-index-offset:2:156:1`
    - cursor probe status `ok`, 301 ms, direct body scan
  - `/tmp/lvcore-focused-validate-kqdental-title-physical-offset.jsonl`
    - package status `ok`
    - `search_full_text` `01`: 731 ms, cursor
      `title:ssed-partial-index-offset:2:48:1`
    - cursor probe status `ok`, 332 ms, direct body scan
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-title-physical-offset.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (1936),
    which is deferred HC work.
  - `_DCT_KQNEWEJ6` `search_full_text` `画像` is 469 ms, down from 804 ms in
    the previous full gate, with cursor probe status `ok`.
  - `_DCT_KQDENTAL` `search_full_text` `01` is 752 ms, down only slightly from
    779 ms; it now returns the physical-offset title cursor and remains a
    current performance candidate.

Baseline evidence:

- Packages:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_KQNEWEJ6`
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_KQDENTAL`
- Observed rows in the previous full gate
  `/tmp/lvcore-all-corpora-validation-20260613-main-wordlist-jtext.jsonl`:
  - `_DCT_KQNEWEJ6`, `search_full_text`, query `画像`, 804 ms
  - `_DCT_KQDENTAL`, `search_full_text`, query `01`, 779 ms

### 0v. Dense sidecar title search title-only projection (resolved, full gate)

Why this matters:

- Dense SSED sidecar title search only needs anchor/id and title-like columns,
  but it was materializing full body/html columns through the same row builder
  used for body full-text search.
- Search hits also inherited `ssed_dense_sidecar_body_resolved` diagnostics even
  though title search had not resolved body content.
- The previous full-corpus gate still showed sidecar-title latency candidates
  such as `_DCT_GENKANA5` partial `アル` and iOS `IBIO5` exact `亜-`.

Current status:

- Dense sidecar title search now selects only the resolver id column and
  title-like columns.
- Body/html columns are still resolved when opening/rendering the dense anchor.
- Title search hits no longer claim `ssed_dense_sidecar_body_resolved`.
- Direct real-package probes:
  - `_DCT_GENKANA5` partial `アル --limit 1`: about 0.92s locally; output still
    returns `アルカーイダ【Al-Qaeda；Al-Qaida】` with a sidecar title cursor.
  - iOS `IBIO5` exact `亜- --limit 1`: about 1.10s locally; output still
    returns `亜-`.
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-genkana5-title-only-sidecar.jsonl`
  - `/tmp/lvcore-focused-validate-ibio5-title-only-sidecar.jsonl`
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-title-physical-offset.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (1936),
    which is deferred HC work.
- This is primarily a semantic/search-surface cleanup; the remaining
  `_DCT_GENKANA5` first-use time appears dominated by sidecar initialization and
  search orchestration, not SQLite title-row projection.

Baseline evidence:

- Packages:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_GENKANA5`
  - `/home/shoui/Agents/CodexMax/LogoVista/Other/iOS/IBIO5/IBIO5`
- Observed rows in the previous full gate
  `/tmp/lvcore-all-corpora-validation-20260613-main-wordlist-jtext.jsonl`:
  - `_DCT_GENKANA5`, `search_partial`, query `アル`, 822 ms
  - `Other/iOS/IBIO5/IBIO5`, `search_exact`, query `亜-`, 566 ms

### 0u. SSED main wordlist bidirectional sidecar titles (resolved)

Why this matters:

- The latest full-corpus baseline exposed `_DCT_KJJK100` exact search for `新`
  as a concrete non-HC dense-sidecar title latency row.
- The real sidecar table is `main(ID, Class, K_text, J_text)`. LVCore treated
  only `K_text` as title-like, so Japanese exact title search scanned about
  1.6 million `K_text` rows before finding `ID=01649735`.
- The same table has early `J_text='新'` entries, and the package is a
  bidirectional Korean/Japanese wordlist. `K_text` and `J_text` are both
  title-like in this format shape.

Current status:

- Dense sidecar title search now treats the `plain` column as an alternate
  title only for `main` wordlist resolvers whose columns are the observed
  `K_text`/`J_text` pair.
- When the alternate column matched, the search hit label uses the matched
  alternate title text, while the target still resolves through the same dense
  sidecar anchor.
- Direct real-package probe:
  - `_DCT_KJJK100` exact `新 --limit 1` dropped to about 0.03s locally.
  - The first hit is now `ID=00025646` with title `新`, matching `J_text`,
    instead of the late `K_text` row.
- Focused tests passed:
  - `cargo fmt --check`
  - `cargo test -p lvcore package::drivers::tests::dense_sidecar -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-kjjk100-main-wordlist-jtext.jsonl`
  - `_DCT_KJJK100` package status remained `ok`.
  - `search_exact` query `新` dropped from 810-835 ms to 46 ms focused.
  - The exact-search cursor probe remained fast at 23 ms.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-main-wordlist-jtext.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (1936),
    which is deferred HC work.
  - `_DCT_KJJK100` package status remained `ok`.
  - `search_exact` query `新` is 52 ms in the full gate, with a 27 ms cursor
    probe.

Touched code:

- `crates/lvcore/src/ssed_sidecar.rs`
- `crates/lvcore/src/package/drivers/tests.rs`
- `crates/lvcore/src/package/drivers/tests/dense_sidecar.rs`

### 0t. SSED large native initial offset proof latency (resolved)

Why this matters:

- The latest full-corpus baseline exposed iOS `LMEDEJ12` native
  exact/forward/backward searches for the short query `A` as concrete non-HC
  cursor-latency rows.
- The first exact page correctly returned one hit with cursor `1`, but the old
  initial-page path still overfetched one extra visible hit to prove whether a
  numeric continuation should exist. On large SSED native indexes, that proof
  can scan far past the returned page.
- The same native offset phase can be wrapped by partial search as
  `ssed-partial-prefix:*`, so validator cursor probing needed to treat the
  nested unverified form consistently.

Current status:

- Large SSED native exact/forward/backward initial pages for short queries now
  emit `ssed-offset-unverified:*` when index size makes next-page proof
  expensive. Small native-index packages keep verified first-page numeric
  cursors.
- Native offset collection can stop once the requested page is filled in this
  deferred-proof mode; the pending row is still flushed into the returned
  `SearchPage`.
- Existing numeric cursors and existing `ssed-offset-unverified:*` cursors
  still decode and continue normally.
- The validator treats nested
  `ssed-partial-prefix:ssed-offset-unverified:*` cursors as the same
  intentionally unverified native offset class and does not probe them
  speculatively.
- Focused tests passed:
  - `cargo test -p lvcore package::drivers::tests::ssed_navigation_surfaces::ssed_native_initial_offset_defers_overfetch_for_large_short_query -- --nocapture`
  - `cargo test -p lvcore-cli validate_search_cursor_probe_skips_expensive_fulltext_body_cursors -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-initial-native-offset-mode-sized.jsonl`
  - iOS `LMEDEJ12` package status remained `ok`.
  - `search_exact` `A` reported `ssed-offset-unverified:1` as `not_probed`
    and ran in 275 ms focused.
  - `search_forward` `A` reported `ssed-offset-unverified:3` as
    `not_probed` and ran in 65 ms focused.
  - `search_backward` `A` reported `ssed-offset-unverified:1` as
    `not_probed` and ran in 92 ms focused.
  - `_DCT_SAIYOREI` package status remained `ok`; `search_partial` `〆を`
    reported nested `ssed-partial-prefix:ssed-offset-unverified:1` as
    `not_probed`.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-initial-native-offset-mode-sized.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous full-gate path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (1936),
    which is deferred HC work.
  - iOS `LMEDEJ12` package elapsed time dropped from 8870 ms in the previous
    full gate to 6068 ms.
  - iOS `LMEDEJ12` `search_exact` `A` dropped from 545 ms plus a 272 ms cursor
    probe to 281 ms with its native offset cursor intentionally not probed.
  - Direct and nested native offset unverified cursors are intentionally not
    probed 227 times in the gate.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/Other/iOS/LMEDEJ12/LMEDEJ12`
- Observed rows:
  - `kind`: `search_exact`, query: `A`
  - `kind`: `search_forward`, query: `A`
  - `kind`: `search_backward`, query: `A`

### 0s. SSED title-label fallback continuation proof latency (resolved)

Why this matters:

- The latest full-corpus gate exposed the iOS `Saitoje` backward search for
  `唖〕` as a concrete non-HC cursor-latency row.
- The first page correctly returned `あ〔唖〕` with cursor
  `ssed-title-label:1`, but the old title-label fallback path then
  recursively scanned ahead to prove whether another visible page existed.
- This made cursor probing expensive for packages whose title-label fallback
  search must scan large native title indexes without a precise key seek.

Current status:

- Filled SSED title-label fallback pages now emit
  `ssed-title-label-unverified:*` cursors when the old path would have run the
  recursive next-page proof.
- Existing `ssed-title-label:*` cursors still decode and continue normally.
- The validator treats `ssed-title-label-unverified:*` as an intentionally
  unverified exact/forward/backward continuation and does not probe it
  speculatively.
- Partial search can wrap the same phase as
  `ssed-partial-prefix:ssed-title-label-unverified:*`; the validator treats
  that nested form as the same unverified continuation class.
- Focused tests passed:
  - `cargo test -p lvcore package::drivers::search_ssed::tests -- --nocapture`
  - `cargo test -p lvcore-cli validate_search_cursor_probe_skips_expensive_fulltext_body_cursors -- --nocapture`
  - `cargo fmt --check`
  - `cargo build -p lvcore-cli`
- Direct real-package probes:
  - iOS `Saitoje` backward `唖〕 --limit 1` now emits
    `ssed-title-label-unverified:1`.
  - Continuing with `ssed-title-label-unverified:1` returns the same
    `おし〔唖〕` hit as the legacy `ssed-title-label:1` cursor.
  - The continuation run dropped from about 1.53s before this change to about
    0.77s locally because it no longer scans ahead to prove the following page.
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-saitoje-title-label-unverified.jsonl`
  - iOS `Saitoje` package status remained `ok`.
  - `search_backward` `唖〕` reported
    `ssed-title-label-unverified:1` as `not_probed` with reason
    `unverified title-label fallback continuation may scan large SSED indexes`.
  - `/tmp/lvcore-focused-validate-kqnewej6-title-label-unverified-nested-skip.jsonl`
  - `_DCT_KQNEWEJ6` package status remained `ok`.
  - `search_partial` `画像` reported
    `ssed-partial-prefix:ssed-title-label-unverified:1` as `not_probed` with
    the same reason.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-title-label-unverified-nested-skip.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous full-gate path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (1936),
    which is deferred HC work.
  - iOS `Saitoje` `search_backward` `唖〕` is 9 ms and its
    `ssed-title-label-unverified:1` cursor is `not_probed`.
  - `_DCT_KQNEWEJ6` `search_partial` `画像` is 162 ms and its nested
    `ssed-partial-prefix:ssed-title-label-unverified:1` cursor is
    `not_probed`.
  - 17 direct or nested title-label unverified cursors are intentionally not
    probed.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/Other/iOS/Saitoje/Saitoje`
- Observed row:
  - `kind`: `search_backward`, query: `唖〕`

### 0r. SSED dense sidecar title continuation rescans (resolved)

Why this matters:

- The latest full-corpus gate exposed `_DCT_KJJK100` exact search for `新` as
  the slowest remaining non-HC search row.
- The package is backed by a 272 MB dense sidecar SQLite database with about
  3.26 million rows and only the primary-key index on `ID`.
- The exact `新` title query has only a few matches, but the old continuation
  cursor was `sidecar-title:1`, so every continuation page repeated the same
  large unindexed title scan and skipped by logical offset.

Current status:

- Dense sidecar title search now emits physical `sidecar-title-row:*` cursors
  using the sidecar name, table, id column/rule, and last returned order value.
- Continuation queries use the existing primary-key order column as a lower
  bound, e.g. `ID > last_id`, while preserving the title prefilter and Rust
  normalized-title verification.
- Legacy `sidecar-title:N` cursors remain accepted and upgrade to physical
  cursors after the next page.
- Focused tests passed:
  - `cargo test -p lvcore package::drivers::tests::dense_sidecar -- --nocapture`
  - `cargo test -p lvcore package::drivers::search_ssed::tests -- --nocapture`
  - `cargo build -p lvcore-cli`
- Direct real-package probes:
  - `_DCT_KJJK100` exact `新 --limit 1` now emits a physical
    `sidecar-title-row:*` cursor.
  - Continuing with that physical cursor returns the same next `新` hit and
    drops from about 0.8s with `sidecar-title:1` to about 0.02s locally.
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-kjjk100-sidecar-title-physical-cursor.jsonl`
  - `_DCT_KJJK100` package status remained `ok`.
  - The exact `新` cursor probe dropped from 797 ms in the latest full gate to
    20 ms focused.
  - Forward/backward sidecar-title cursor probes now also use physical cursors.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-sidecar-title-physical-cursor.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous full-gate path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (1936),
    which is deferred HC work.
  - `_DCT_KJJK100` package elapsed time dropped from 5681 ms in the previous
    full gate to 5402 ms.
  - The exact `新` search row dropped from 1578 ms to 890 ms.
  - The sidecar title cursor probe dropped from 797 ms to 18 ms and still
    returned one `新` hit.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_KJJK100`
- Observed row:
  - `kind`: `search_exact`, query: `新`

### 0q. SSED full-text title continuation prefilter churn (resolved)

Why this matters:

- The latest full-corpus gate exposed `_DCT_KQNEWEJ6` full-text search for
  `画像` as a concrete non-HC continuation latency row.
- The first page correctly returned the native title/index hit `画像一覧` with
  cursor `title:ssed-partial-index:2:1181`.
- Direct probes showed the follow-up page was not slow because of HONMON body
  scanning. `body:0` returned the same direct body hit in about 0.2s, while the
  title cursor spent about 1.2s proving enough empty partial-title index pages
  before falling through to body results.

Current status:

- Partial-index scanning keeps the existing default prefilter budget for normal
  partial search and initial title prepasses.
- Full-text physical title continuation cursors now use a smaller
  prefiltered-leaf budget before falling through to HONMON body search. This
  preserves the existing bounded title-prepass policy while avoiding thousands
  of raw leaf-page prefilter reads on large SSED indexes.
- Direct real-package probes:
  - `_DCT_KQNEWEJ6` full-text `画像 --limit 1` still returns `画像一覧` with
    cursor `title:ssed-partial-index:2:1181`.
  - Continuing from `title:ssed-partial-index:2:1181` returns the same
    `a・ cu・ tance...` body hit and `body-offset:*` cursor, but dropped from
    about 1.2s in the latest full gate to about 0.34s locally.
- Focused tests passed:
  - `cargo test -p lvcore package::drivers::search_ssed::tests -- --nocapture`
  - `cargo test -p lvcore package::drivers::ssed_index::tests -- --nocapture`
  - `cargo build -p lvcore-cli`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-kqnewej6-title-cursor-budget.jsonl`
  - `_DCT_KQNEWEJ6` package status remained `ok`.
  - The `search_full_text` `画像` row dropped from 1716 ms in the latest full
    gate to 826 ms focused.
  - The cursor probe dropped from 1202 ms in the latest full gate to 302 ms
    focused, still returning one hit and the same direct-body diagnostic shape.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-ssed-title-cursor-budget.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous full-gate path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (1936),
    which is deferred HC work.
  - `_DCT_KQNEWEJ6` package elapsed time dropped from 6633 ms in the previous
    full gate to 5732 ms.
  - The `search_full_text` `画像` row dropped from 1716 ms to 842 ms.
  - The title-cursor probe dropped from 1202 ms to 312 ms and still returned
    one hit with `body-offset:484f4e4d4f4e2e444943:1c6f5c`.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_KQNEWEJ6`
- Observed row:
  - `kind`: `search_full_text`, query: `画像`

### 0p. SSED direct full-text scan range-read churn (resolved)

Why this matters:

- The previous full-corpus gate exposed `_DCT_NCOMP4` full-text search for `1計`
  as a concrete non-HC search-latency row.
- Direct real-package probes reproduced the cost at about 2.1s for
  `--mode full-text --limit 1`, returning no hits.
- Diagnostics showed the search checked 64 native-index body windows, then ran
  a direct HONMON byte scan across 114 windows with zero byte-candidate windows.
  The direct scan used overlapping `SsedDataFile::read_range` calls over
  compressed SSEDDATA chunks.

Current status:

- `SsedDataFile` now keeps a small MRU cache of expanded chunks instead of a
  single expanded chunk, avoiding repeated decompression when overlapping range
  reads revisit adjacent chunks.
- SSED direct full-text scan windows increased from 256 KiB to 1 MiB while
  preserving the existing body lookbehind/overlap, reducing direct scan windows
  for this package from 114 to 29.
- Focused tests passed:
  - `cargo test -p lvcore ssed::tests::file_backed_reader -- --nocapture`
  - `cargo test -p lvcore package::drivers::search_ssed::tests -- --nocapture`
  - `cargo build -p lvcore-cli`
- Direct real-package probe:
  - `_DCT_NCOMP4` full-text `1計 --limit 1` dropped from about 2.14s to about
    1.65-1.68s.
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-ncomp4-1m-window.jsonl`
  - `_DCT_NCOMP4` package status remained `ok`.
  - Package elapsed time dropped from 3187 ms in the previous full-gate baseline
    to 2512 ms in focused validation.
  - The `search_full_text` row dropped from 1959 ms to 1471 ms.
  - The direct body scan still found no hits, but scanned 29 windows instead of
    114.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-ssed-direct-scan-chunk-cache.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous full-gate path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (1936),
    which is deferred HC work.
  - `_DCT_NCOMP4` package elapsed time is 2538 ms.
  - The `search_full_text` row is 1499 ms and still scans 29 direct body
    windows.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_NCOMP4`
- Observed row:
  - `kind`: `search_full_text`, query: `1計`

### 0o. LVED guarded FTS variant latency (resolved)

Why this matters:

- The latest full-corpus gate exposed `_DCT_SSJPKOKU` full-text and partial
  searches for `あ` as concrete non-HC search latency rows.
- Direct real-package probes reproduced the issue: first-page and cursor-page
  full-text searches were about 1.4-1.6s.
- The slow SQL shape was the hiragana/katakana variant path for guarded FTS
  searches. LVCore used `rowid in (select rowid from search where match ...)`
  subqueries joined with `or`, which forced broad FTS result materialization for
  high-frequency CJK terms.

Current status:

- Multi-variant LVED FTS searches now run each variant as a direct FTS table
  expression with the same value guard patterns, then merge/deduplicate by list
  id in row order.
- LVED numeric continuation pages also return `lved-offset-unverified:*` when a
  continuation page fills exactly, deferring next-page proof instead of doing
  immediate overfetch on every continuation.
- Focused tests passed:
  - `cargo test -p lvcore lved_sqlite::sql_search -- --nocapture`
  - `cargo test -p lvcore-cli validate_search_cursor_probe_skips_expensive_fulltext_body_cursors -- --nocapture`
  - `cargo fmt --check`
- Direct real-package probes:
  - `_DCT_SSJPKOKU` full-text `あ --limit 1` dropped from about 1.4-1.6s to
    about 0.37s.
  - Cursor page `--cursor 1` dropped to about 0.36s and returns
    `lved-offset-unverified:2`.
  - Partial `あ --limit 1` dropped to about 0.35s.
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-ssjpkoku-lved-direct-variants.jsonl`
  - `_DCT_SSJPKOKU` package status remained `ok`.
  - Package elapsed time dropped from 5348 ms in the latest full-gate baseline
    to 1243 ms in focused validation.
  - The `search_full_text` row dropped from 2185 ms with a 1084 ms cursor probe
    to 1 ms with a 0 ms cursor probe in focused validation.
  - The `search_partial` row dropped from 1305 ms with a 644 ms cursor probe to
    5 ms with a 0 ms cursor probe in focused validation.
- Full-corpus regression gate passed:
  - `/tmp/lvcore-all-corpora-validation-20260613-lved-direct-fts-variants.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous full-gate path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback` (1936),
    which is deferred HC work.
  - `_DCT_SSJPKOKU` package elapsed time is 2374 ms.
  - The `search_full_text` row is 1 ms with a 0 ms cursor probe.
  - The `search_partial` row is 6 ms with a 0 ms cursor probe.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SQLCIPHER_DICTS_WINDOWS/_DCT_SSJPKOKU`
- Observed rows:
  - `kind`: `search_full_text`, query: `あ`
  - `kind`: `search_partial`, query: `あ`

### 0n. SSED MULTI filtered selector browse latency (resolved)

Why this matters:

- The latest full-corpus gate exposed `_DCT_EJJE100` `multi:MULTI1.DIC` as a
  concrete non-HC navigation-surface latency row.
- The slow target was the first MULTI selector child, `ビジネス・経済`, which
  opens a filtered MULTI record browse.
- The previous implementation found filtered rows by linearly scanning the
  referenced index component until `limit + 1` selector matches were seen.
  For `_DCT_EJJE100`, that meant scanning a large `0x91` MULTI index even
  though the index has native internal pages that can locate the exact selector
  key.

Current status:

- SSED MULTI descriptors and selector menu parses are cached per package
  instance, avoiding repeated descriptor/menu reads across home-surface,
  surface-render, and window paths.
- Filtered MULTI browse now uses a component-specific near-key scan for simple
  leaf index components. It seeks to candidate leaf pages for the exact
  normalized selector key, and falls back to the previous linear scan if the
  component is not a simple leaf index, candidate pages are unavailable, or row
  order looks unsafe.
- Focused tests passed:
  - `cargo test -p lvcore ssed_multi_descriptor_and_selector_menu_are_cached -- --nocapture`
  - `cargo test -p lvcore ssed_native_offset_continuation_defers_overfetch_after_first_page -- --nocapture`
  - `cargo fmt --check`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-ejje100-multi-near-key.jsonl`
  - `_DCT_EJJE100` package status remained `ok`.
  - The `multi:MULTI1.DIC` row dropped from the latest full-gate baseline
    1936 ms to 35 ms in focused validation.
  - Package focused validation wall time dropped from 3.20s before the near-key
    fast path to 1.66s after it.
- Full-corpus validation gate:
  - `/tmp/lvcore-all-corpora-validation-20260613-ssed-multi-near-key.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - Path set matched the previous baseline.
  - Warning diagnostics remained only the explicitly deferred HC common HTML
    fallback.
  - The `_DCT_EJJE100` `multi:MULTI1.DIC` row was 31 ms in the full gate, down
    from 1936 ms in the previous full-gate baseline.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_EJJE100`
- Observed row:
  - `surface_id`: `multi:MULTI1.DIC`
  - label: `ビジネス・経済`
  - opened kind: `hierarchical_tree`
  - view kind: `navigation_surface`

### 0m. SSED native offset continuation overfetch latency (resolved)

Why this matters:

- The latest full-corpus gate exposed `_DCT_NMEDEJ12` backward search for
  `規定` as a concrete non-HC search latency row.
- The first page was fast, but deep validation and a real reader following
  cursor `1` spent about 2.5-2.6s proving whether a third native match existed
  after returning the second hit.
- This was a user-visible continuation issue, not only validation overhead:
  `lvcore search ... --mode backward --limit 1 --cursor 1` reproduced the slow
  second page directly.

Current status:

- Native SSED exact/forward/backward numeric offset continuation pages no longer
  overfetch one extra row to prove further pagination.
- If such a continuation page fills, it returns an
  `ssed-offset-unverified:*` cursor for the next page, deferring the expensive
  proof until the user actually asks for another page.
- The validator treats `ssed-offset-unverified:*` as an unverified native
  continuation and does not speculatively probe it.
- Focused tests passed:
  - `cargo test -p lvcore ssed_native_offset_continuation_defers_overfetch_after_first_page -- --nocapture`
  - `cargo test -p lvcore-cli validate_search_cursor_probe_skips_expensive_fulltext_body_cursors -- --nocapture`
  - `cargo fmt --check`
- Direct real-package probes:
  - First page stayed about 0.10s and returns cursor `1`.
  - Cursor page `--cursor 1` dropped from about 2.6s to about 0.10s and returns
    `ssed-offset-unverified:2`.
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-nmedej12-native-offset-cursor.jsonl`
  - `_DCT_NMEDEJ12` package status remained `ok`.
  - The `search_backward` row dropped from 2510 ms with a 2493 ms cursor probe
    to 39 ms with a 19 ms cursor probe.
- Full-corpus validation gate:
  - `/tmp/lvcore-all-corpora-validation-20260612-native-offset-cursor.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - Path set matched the previous baseline.
  - Warning diagnostics remained only the explicitly deferred HC common HTML
    fallback.
  - The `_DCT_NMEDEJ12` `search_backward` row was 34 ms with a 17 ms cursor
    probe.
  - 225 cursor-probe continuations now expose `ssed-offset-unverified:*`
    cursors instead of proving the next page speculatively.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_NMEDEJ12`
- Observed row:
  - `kind`: `search_backward`
  - query: `規定`
  - first cursor: `1`

### 0l. Large GenericHtml resource-byte validation latency (resolved)

Why this matters:

- The latest full-corpus gate exposed `_DCT_SINJIGEN` `aux-index:0` as a slow
  concrete non-HC surface row.
- The surface and target render were already valid; the cost came from the
  validator's alternate `GenericHtml` probe inlining 16 known image resources
  into a 9.5 MB standalone HTML payload.
- The existing alternate-render cap handled very large native HTML and high
  resource counts, but not moderate resource counts with large byte totals.

Current status:

- Deep validation now skips the alternate `GenericHtml` probe when known native
  resource bytes exceed the validation cap.
- The skipped row reports `skipped_large_view` with
  `reason: resource_bytes_too_large` and includes `native_resource_bytes`.
- GenericHtml inlining still streams eligible resources directly into the final
  output buffer, avoiding an extra temporary data-URL allocation for probes that
  remain under the cap.
- Focused tests passed:
  - `cargo test -p lvcore generic_html -- --nocapture`
  - `cargo test -p lvcore render_modes_are_explicit_for_preserved_lved_html -- --nocapture`
  - `cargo test -p lvcore-cli validate_generic_html_probe_skips_large_native_views_only -- --nocapture`
  - `cargo test -p lvcore-cli validate_deep_exercises_reader_render_modes -- --nocapture`
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-sinjjigen-generic-html-resource-byte-cap.jsonl`
  - `_DCT_SINJIGEN` package status remained `ok`.
  - The `aux-index:0` row dropped from about 1.8-2.0s to 581 ms in the focused
    row; package wall time dropped from about 2.4s to 1.16s.
- Full-corpus validation gate:
  - `/tmp/lvcore-all-corpora-validation-20260612-generic-html-resource-byte-cap.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - Path set matched the previous baseline.
  - Warning diagnostics remained only the explicitly deferred HC common HTML
    fallback.
  - `skipped_large_view` now has 38 `native_display_html_too_large` skips and
    one `resource_bytes_too_large` skip.

Baseline evidence:

- Package:
  - `/home/shoui/Agents/CodexMax/LogoVista/LOGOVISTA_SSED_DICTS_WINDOWS/_DCT_SINJIGEN`
- Observed row:
  - `surface_id`: `aux-index:0`
  - label: `中国語の起原と特色`
  - native resources: 16
  - known native resource bytes: 7,161,321

### 0k. SSED full-text initial row-prefetch latency (resolved)

Why this matters:

- The latest full-corpus gate still showed a concrete SSED full-text first-page
  latency gap after the post-title cursor fix.
- `_DCT_KENE7J5`, `_DCT_NCOMP4`, and `_DCT_GEN2005` spent about 2.2-3.5s in
  `search_full_text` before returning the first page.
- Each row performed a 512-row native body prefetch with zero byte-candidate
  rows, then fell through to the direct HONMON byte-window scan that actually
  produced the result or proved exhaustion.
- This was user-visible first-page search time, not only validation cursor
  probing.

Current status:

- Initial row-driven full-text body prefetch now uses a smaller row budget when
  byte candidates exist and the request is the first page.
- Explicit row cursors and cases without byte candidates keep the existing
  512-row budget.
- Early row-driven hits still preserve native index titles; late/no-hit cases
  fall through to the direct body scan sooner.
- Focused test passed:
  - `cargo test -p lvcore ssed_fulltext -- --nocapture`
- Direct real-package probes after the change:
  - `_DCT_KENE7J5`, query `は殺`, first page about 1.2s instead of about 3.6s.
  - `_DCT_NCOMP4`, query `1計`, first page about 2.0s instead of about 3.5s.
  - `_DCT_GEN2005`, query `曙光`, first page about 1.3s instead of about 2.2s.
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-ssed-fulltext-row-prefetch-cap.jsonl`
  - `_DCT_KENE7J5` package status `ok`; `search_full_text` elapsed about
    1.1s and row prefetch checked 64 rows instead of 512.
  - `_DCT_NCOMP4` package status `ok`; `search_full_text` elapsed about 1.7s
    and row prefetch checked 64 rows instead of 512.
  - `_DCT_GEN2005` package status `ok`; `search_full_text` elapsed about
    0.69s and row prefetch checked 64 rows instead of 512.
- Full-corpus validation gate:
  - `/tmp/lvcore-all-corpora-validation-20260612-ssed-fulltext-row-prefetch-cap.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback`.
  - Total gate wall time was about 482s.
  - In the full gate, `_DCT_KENE7J5` `search_full_text` query `は殺` elapsed
    about 1.1s, `_DCT_NCOMP4` query `1計` about 1.6s, and `_DCT_GEN2005` query
    `曙光` about 0.69s.
  - The affected rows reported `checked_rows=64` before direct body scan.

Baseline evidence:

- Baseline full-corpus JSONL:
  - `/tmp/lvcore-all-corpora-validation-20260612-ssed-fulltext-body-cursor.jsonl`
- Baseline symptoms:
  - `_DCT_KENE7J5` `search_full_text` query `は殺` elapsed about 3.5s, with
    `checked_rows=512` before direct body scan.
  - `_DCT_NCOMP4` `search_full_text` query `1計` elapsed about 3.1s, with
    `checked_rows=512` before direct body scan.
  - `_DCT_GEN2005` `search_full_text` query `曙光` elapsed about 2.2s, with
    `checked_rows=512` before direct body scan.

Changed code area:

- `crates/lvcore/src/package/drivers/search_ssed.rs`

### 0j. SSED full-text post-title continuation latency (resolved)

Why this matters:

- The latest full-corpus gate had no non-HC correctness failures, but it exposed
  a repeated SSED full-text latency pattern after title/index prepass hits.
- Packages such as `_DCT_HKDKSR10`, `_DCT_KQJCOLLO`, and `_DCT_RPLUSREV`
  returned a fast title/index hit first, then advertised `row:0`.
- Deep validation probed that cursor and spent about 3.1-3.7s reading bounded
  native body rows. In at least one sampled real package, the probed row page
  returned no hits and only another `row:*` continuation.
- This is both a validation latency issue and a user-visible continuation issue:
  a reader following "more results" could get an empty, slow row page.

Current status:

- Post-title-prepass full-text continuation now uses `body:0` unless an
  available dense sidecar body phase should run first.
- Initial body-only full-text searches still use the row-driven prefetch path
  where it is useful; the change is scoped to the continuation after a title
  prepass page.
- The validator already treats `body:0` as an expensive full-text body cursor
  and records it as `not_probed` instead of turning continuation work back into
  first-page validation time.
- Focused tests passed:
  - `cargo test -p lvcore ssed_fulltext -- --nocapture`
  - `cargo test -p lvcore-cli validate_search_cursor_probe_skips_expensive_fulltext_body_cursors -- --nocapture`
- Direct real-package probes after the change:
  - `_DCT_HKDKSR10`, query `FU`, first page about 0.8s with next cursor
    `body:0`.
  - `_DCT_KQJCOLLO`, query `BE`, first page about 0.02s with next cursor
    `body:0`.
  - `_DCT_RPLUSREV`, query `O1`, first page about 0.04s with next cursor
    `body:0`.
- Focused real-package validation passed:
  - `/tmp/lvcore-focused-validate-ssed-fulltext-body-cursor.jsonl`
  - `_DCT_HKDKSR10` package status `ok`; `search_full_text` elapsed about
    0.63s with `body:0` cursor status `not_probed`.
  - `_DCT_KQJCOLLO` package status `ok`; `search_full_text` elapsed about
    5ms with `body:0` cursor status `not_probed`.
  - `_DCT_RPLUSREV` package status `ok`; `search_full_text` elapsed about
    17ms with `body:0` cursor status `not_probed`.
- Full-corpus validation gate:
  - `/tmp/lvcore-all-corpora-validation-20260612-ssed-fulltext-body-cursor.jsonl`
  - 336 packages validated with package status 336 `ok`.
  - The previous 336-package baseline path set is fully covered.
  - Warning diagnostics remain only `hc_render_common_html_fallback`.
  - Total gate wall time was about 491s.
  - The gate has 122 `body:0` full-text cursors marked `not_probed` and no
    remaining `row:0` full-text cursor probes.
  - In the full gate, `_DCT_HKDKSR10` `search_full_text` query `FU` elapsed
    about 0.64s, `_DCT_KQJCOLLO` query `BE` about 5ms, and `_DCT_RPLUSREV`
    query `O1` about 17ms.

Baseline evidence:

- Baseline full-corpus JSONL:
  - `/tmp/lvcore-all-corpora-validation-20260612-ios-panel-cache.jsonl`
- Baseline symptoms:
  - `_DCT_HKDKSR10` `search_full_text` query `FU` elapsed about 3.8s; cursor
    probe `row:0` took about 3.1s.
  - `_DCT_KQJCOLLO` `search_full_text` query `BE` elapsed about 3.7s; cursor
    probe `row:0` took about 3.7s.
  - `_DCT_RPLUSREV` `search_full_text` query `O1` elapsed about 3.5s; cursor
    probe `row:0` took about 3.5s.

Changed code area:

- `crates/lvcore/src/package/drivers/search_ssed.rs`
- `crates/lvcore/src/package/drivers/tests/fulltext.rs`

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

- All 335 reconstructed packages in the latest baseline open and deep-validate
  at package status `ok`.

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

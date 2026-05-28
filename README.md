# lvcore-rs

Rust reader core for LogoVista dictionaries.

This repository is the production `lvcore` implementation target. It is not a
Rust port of `src/lvcore-experimental`, not an exporter, and not a replacement
for `logovista-tools` research commands.

## Architecture

The core model is:

```text
storage/container driver -> discovered stores/resources -> shared capability providers -> target resolution -> renderer output
```

Package families are first-class:

- `SSED`
- `LVED_SQLITE3`
- `LVLMultiView`
- `Hourei`

Features are capability-based, not family-gated. For example, retained SSED-like
components inside an LVED_SQLITE3 package should use the same component parsers
as a normal SSED package.

## Crates

```text
crates/lvcore      main Rust library
crates/lvcore-cli  small developer validation CLI
```

The C ABI crate is intentionally deferred until the Rust-native API stabilizes.

## Reader Contract

Frontend code should treat `lvcore` as the only component that understands
LogoVista internals. The frontend receives:

- book metadata and format labels for library UI badges;
- explicitly available search modes, including LVED advanced search columns when
  present;
- home/navigation/search surfaces;
- home-surface status values distinguish available, missing, empty,
  unsupported, and deferred surfaces so dead placeholder components do not
  become clickable UI;
- stable opaque `TargetToken` values;
- rendered target views with HTML/text/resources/links/diagnostics;
- resource tokens for images, audio, PDFs, media BLOBs, gaiji assets, and other
  dictionary-local resources.

The frontend should not parse HONMON, `lved.*` links, Panel rows, law references,
or gaiji codes directly.

## Non-Goals

- No export support.
- No copying dictionary data into a new database.
- No scanner-first architecture.
- No full HC renderer parity claim before renderer profiles are ported.
- No LVED_SQLITE3/LVLMultiView/Hourei fake rendering.
- No frontend cache ownership. The Tauri/Svelte app owns UI cache state.

## Current Status

This repo now contains the Rust-native reader-core skeleton plus working
provider slices:

- typed package-family detection for SSED, LVED_SQLITE3, LVLMultiView, and
  Hourei;
- casefolded storage lookup with casing preservation;
- readable SSED component materialization for plain payloads, LogoFontCipher
  payloads, Mac OS X AES payloads, and observed Mac OS X ZipCrypto `HONMON`
  wrappers;
- round-trippable frontend-safe target tokens;
- shared provider traits for search, navigation, body, render, resources,
  gaiji, and continuous view;
- frontend-safe tagged JSON for search scopes and continuous-view sequence
  hints, matching the Tauri/Svelte contract shape;
- native indexed search/browse for the observed SSED leaf row families:
  simple, keyless pointer-table, body-only, tagged/grouped, keyword,
  cross-reference, and multi-selector rows;
- SSED simple-index internal-page traversal for exact/forward searches, so large
  title indexes can seek near the requested key instead of always scanning from
  the first leaf page;
- SSED title-label decoding for raw JIS X 0208 title bytes that can otherwise
  look like printable ASCII, while preserving real Latin title labels;
- initial SSED full-text search over HONMON body windows behind native index
  targets, with diagnostics and explicit HC rendering separation;
- dense HONMON anchor dereference for supported SQLite sidecars, including
  `t_contents`, `HONBUN`, extensionless main wordlist tables, and conservative
  generic id/body schemas;
- SQLCipher-backed LVED_SQLITE3 list/search/content/info/media access where the
  package key is available;
- frontend-visible search-mode metadata, with SSED modes derived from available
  title indexes/HONMON payloads and LVED advanced modes derived from actual
  `search` table columns;
- LVLMultiView menu/search/body access for decoded payloads;
- Hourei law tree/search/body/resource access for decoded law packages;
- developer CLI commands for package validation, search/render, renderer-input
  inspection, resource-token reads, and arbitrary-target continuous-view
  windows;
- SSED HANREI/info surface discovery for the three observed help layouts:
  Windows-style `HANREI.chm` packages, folder-style `HANREI/index.html` plus
  sibling pages, and Mac OS X `_HELP.localized` bundles. All three use
  package-local/CHM HTML resource rendering and relative CSS/image/link
  rewriting;
- SSED MENU/TOC decoding reports explicit empty sentinel components as
  diagnostic-only surfaces rather than targetable menus;
- explicit deferred/unsupported diagnostics instead of fake output.

`logovista-tools` remains the research oracle while `lvcore-rs` ports stable
reader-facing behavior incrementally. Real corpus validation is the main
compatibility signal; synthetic fixtures are only regression guardrails for
known structures.

## Important Gaps

- SSED HC renderer parity is not ported yet. Plain SSED body targets currently
  resolve to explicit HC renderer input rather than claiming rendered HTML;
  supported dense sidecar targets resolve to preserved HTML or exact sidecar
  text where available.
- SSED full-text search is implemented as a bounded, index-anchored HONMON scan;
  it is not a substitute for HC-rendered semantic text and may need more product
  tuning for dense/sidecar-heavy dictionaries.
- SSED internal-page traversal is currently implemented only for simple
  exact/forward title-index paths. Backward, partial, keyword, cross-reference,
  and multi-selector performance still need format-specific indexing work.
- CHM table-of-contents semantics are supported at the reader-core level:
  lvcore reads `.hhc` Name/Local entries and exposes them as nested HANREI
  navigation trees with target tokens and scroll anchors. Higher-level reader
  wrapping/styling remains frontend work.

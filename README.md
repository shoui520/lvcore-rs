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
- home/navigation/search surfaces;
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
- native indexed search/browse for implemented SSED title/index forms;
- initial SSED full-text search over HONMON body windows behind native index
  targets, with diagnostics and explicit HC rendering separation;
- SQLCipher-backed LVED_SQLITE3 list/search/content/info/media access where the
  package key is available;
- LVLMultiView menu/search/body access for decoded payloads;
- Hourei law tree/search/body/resource access for decoded law packages;
- explicit deferred/unsupported diagnostics instead of fake output.

`logovista-tools` remains the research oracle while `lvcore-rs` ports stable
reader-facing behavior incrementally. Real corpus validation is the main
compatibility signal; synthetic fixtures are only regression guardrails for
known structures.

## Important Gaps

- SSED HC renderer parity is not ported yet. SSED body targets currently resolve
  to explicit HC renderer input rather than claiming rendered HTML.
- SSED full-text search is implemented as a bounded, index-anchored HONMON scan;
  it is not a substitute for HC-rendered semantic text and may need more product
  tuning for dense/sidecar-heavy dictionaries.
- Some SSED index variants are still reported as deferred when they are not the
  simple leaf layout currently parsed.
- CHM/HANREI wrapping is still reader-surface work, not finished rendering.

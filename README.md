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

This repo currently contains the Rust-native architecture scaffold:

- typed package-family detection;
- casefolded storage lookup with casing preservation;
- round-trippable frontend-safe target tokens;
- shared provider traits for search, navigation, body, render, resources,
  gaiji, and continuous view;
- explicit deferred/unsupported diagnostics instead of fake output.

`logovista-tools` remains the research oracle while `lvcore-rs` fills in real
providers incrementally.

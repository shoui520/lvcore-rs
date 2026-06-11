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
- cheap package candidates for library cache refresh, before encrypted/database
  payloads are opened;
- explicitly available search modes, including LVED advanced search columns when
  present;
- home/navigation/search surfaces;
- home-surface status values distinguish available, missing, empty,
  unsupported, and deferred surfaces so dead placeholder components do not
  become clickable UI;
- typed SSED screen-menu surfaces for bitmap-backed navigation such as
  KOJIEN6 `SCRMENU.DIC`, with COLSCR background resources and hotspot target
  tokens;
- pageable Panel surfaces; large LogoVista Panel BIN grids are exposed through
  `next_cursor` instead of forcing the frontend to receive every cell at once;
- stable opaque `TargetToken` values;
- rendered target views with HTML/text/resources/links/diagnostics;
- resource tokens for images, audio, PDFs, media BLOBs, gaiji assets, and other
  dictionary-local resources.
- search result pages may include an opaque `result_sequence` value. Frontends
  pass it back as `SequenceHint::SearchResults { value }` when opening a hit
  with continuous view in the visible result order, including 串刺し検索 pages
  where neighboring hits may belong to different books.
- selected-book 串刺し検索 preserves the frontend-provided book order for both
  hit pages and search-result continuous view windows. All-book search uses the
  library's deterministic loaded-book order.

The frontend should not parse HONMON, `lved.*` links, Panel rows, law references,
or gaiji codes directly.

### Library Manager Flow

The intended frontend library flow is two-phase:

1. `DriverRegistry::discover_package_candidates` finds packages under one root
   without opening/decrypting body stores. Candidate fingerprints are based on
   package-root metadata, not full payload hashing, so this is suitable for a
   quick library refresh and UI cache reconciliation.
2. `BookLibrary::open_discovered_paths` or `try_open_discovered_paths` opens the
   selected packages and returns full `BookMetadata`, search modes, surfaces,
   resource providers, and target routing.

For progressive UI updates, the developer CLI mirrors this split:

- `library-discover --jsonl` streams cheap candidate rows for cache comparison
  and ends with a summary row.
- `library-import --jsonl` streams cacheable `book` rows only for successfully
  opened books, emits duplicate/open-failure cases as diagnostic rows, and ends
  with a summary. The frontend does not need to wait for an entire corpus import
  before showing usable books, and it does not need to special-case duplicate
  book rows.

The developer CLI opens packages per command invocation. It is useful for
validation and reports, but it is not the frontend performance model for
repeated search pages. A reader app should keep a `BookLibrary` open and request
additional pages/windows against that in-memory library state.

The frontend cache may store candidate fingerprints, book metadata, app-supplied
icon state, recently rendered snippets, and bookmark/history tokens. It should
still pass stored `TargetToken`/`ResourceToken` values back to lvcore for
resolution rather than decoding or trusting them itself.

### Integration Safety Contract

- `TargetToken` and `ResourceToken` are opaque routing/cache handles, not trust
  boundaries. Frontends may store and return them to lvcore, but lvcore must
  always re-resolve them against the opened book before reading bodies,
  resources, SQL rows, paths, or byte ranges.
- `display_html` is reader-ready, package-authored HTML after lvcore has
  rewritten understood package links and resource references. It is not
  sanitized application UI. Dedicated frontends should render it in a constrained
  document/webview context with app chrome and privileged APIs kept separate.
- `BasicText` is the low-risk flattened mode. It is useful for snippets,
  previews, and simple external integrations, but it is not the visual parity
  path for LogoVista dictionaries.
- `BookMetadata` uses a stable cache-friendly JSON shape for always-present
  list fields such as `search_modes` and `diagnostics`, even when they are
  empty. Nested view/surface diagnostics may still be omitted when empty to keep
  per-entry payloads compact.

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
- library-owned package-root discovery for corpus/library directories, so the
  reader can find nested books without duplicating LogoVista package heuristics
  in frontend code;
- cheap `PackageCandidate` discovery for library-cache refresh, with package
  format labels, optional title hints, and root metadata fingerprints;
- library-owned multi-root import through `BookLibrary::open_discovered_paths`
  and tolerant `try_open_discovered_paths` import reports, plus developer
  `library-discover`, `library-discover --jsonl`, `library-import`,
  `library-import --jsonl`, and `library-search` CLI commands that exercise
  frontend-cacheable book metadata, progressive discovery/import, all-book
  串刺し検索, and routed first-hit rendering across opened books;
- library search pages expose an opaque search-result sequence for continuous
  view. The frontend does not need to reconstruct search-result order from
  target internals; it can pass the returned value back to lvcore when opening a
  result with surrounding entries; selected-book search preserves the
  frontend-provided book order in that sequence;
- casefolded storage lookup with casing preservation;
- readable SSED component materialization for plain payloads, LogoFontCipher
  payloads, Mac OS X AES payloads, and observed Mac OS X ZipCrypto `HONMON`
  wrappers;
- round-trippable frontend-safe target tokens;
- shared provider traits for search, navigation, body, render, resources,
  gaiji, and continuous view;
- frontend-safe tagged JSON for search scopes and continuous-view sequence
  hints, matching the Tauri/Svelte contract shape;
- render-mode separation for the three reader output targets: `Native` keeps
  app-internal target/resource routing for the dedicated reader, `GenericHtml`
  attempts standalone browser output by embedding readable resources as data
  URLs and fragmentizing target links, and `BasicText` flattens renderer output
  after lvcore has interpreted known dictionary markup;
- native indexed search/browse for the observed SSED leaf row families:
  simple, keyless pointer-table, body-only, tagged/grouped, keyword,
  cross-reference, and multi-selector rows;
- SSED internal-page traversal for native exact/forward/backward indexed
  searches, so large title indexes can seek near the requested key instead of
  always scanning from the first leaf page;
- SSED title-label decoding for raw JIS X 0208 title bytes that can otherwise
  look like printable ASCII, while preserving real Latin title labels;
- initial SSED full-text search over HONMON body windows behind native index
  targets, plus dense HONMON sidecar body text/HTML where renderable sidecar
  tables are understood, with diagnostics and explicit HC rendering separation;
- dense HONMON anchor dereference for supported SQLite sidecars, including
  `t_contents`, `HONBUN`, extensionless main wordlist tables, and conservative
  generic id/body schemas. Android SSED app body databases that store HTML in a
  dict-code table without an ID column are resolved through the observed
  `raw_honmon_id = rowid * 5` rule;
- SQLCipher-backed LVED_SQLITE3 list/search/content/info/media access where the
  package key is available, plus retained text-tree navigation from `tree.idx`
  and product-specific `res/*.idx` files;
- iOS SSED shells that declare retained `DictFtsDB` `.dbc` payloads through
  `DictList.plist` can open those payloads as embedded LVED_SQLITE3 stores when
  the observed dict id/code-derived key is known. Known retained payloads expose
  LVED list/info/search/body behavior through the same provider path as normal
  LVED_SQLITE3 books; unknown encrypted retained payloads are reported as
  deferred diagnostics rather than fake search support;
- iOS SSED `DictSearchDB`, `DictFULLDB`, and `DictConvertAddrDB` sidecars are
  integrated into the SSED provider path. Search hits and direct SSED address
  targets canonicalize through the observed conversion table when present, so
  stale wrapper addresses do not leak to user-facing targets;
- iOS SSED plist navigation sidecars are exposed as first-class reader
  surfaces: panel-style plist indexes such as `indexSearch.plist`, preserved
  `HTMLList.plist` info pages, and `tableList.plist` title/index rows when the
  row address namespace resolves through retained SSED/sidecar targets.
  Unresolved `tableList.plist` namespaces remain diagnostic-only surfaces rather
  than guessed routes. Preserved HTML pages rewrite `lved.addrXXXXXXXX:YYYY`
  links to normal target tokens, resolve package resources from observed iOS
  locations such as `OTHER/_images`, and fall back through iOS `Gaiji.plist`
  Unicode mappings when an HTML page references a gaiji PNG that is not present
  on disk;
- frontend-visible search-mode metadata, with SSED modes derived from available
  title indexes/HONMON payloads and LVED advanced modes derived from actual
  `search` table columns;
- LVLMultiView menu/search/body access for decoded payloads;
- Hourei law tree/search/body/resource access for decoded law packages;
- developer CLI commands for package validation, home-surface inspection,
  search/render, renderer-input inspection, resource-token reads, and
  arbitrary-target continuous-view windows;
- SSED HANREI/info surface discovery for the three observed help layouts:
  Windows-style `HANREI.chm` packages, folder-style `HANREI/index.html` plus
  sibling pages, and Mac OS X `_HELP.localized` bundles. All three use
  package-local/CHM HTML resource rendering and relative CSS/image/link
  rewriting;
- SSED MENU/TOC decoding reports explicit empty sentinel components as
  diagnostic-only surfaces rather than targetable menus;
- SSED MENU/TOC surfaces are cursor-paged by produced navigation nodes,
  including records that expand to many entry links, so a large menu row cannot
  force the frontend to receive the entire navigation surface at once;
- SSED screen-menu decoding for the KOJIEN6-style `SCRMENU.DIC` component:
  background images are exposed as COLSCR resource tokens, screen jumps remain
  navigation targets, and body hotspots resolve to normal SSED address targets;
- SSED KOJIEN6-style `encyclop.idx` LVEDBRSR multimedia indexes are exposed as
  hierarchical navigation surfaces with normal SSED address targets;
- SSED `EXINFO.INI` auxiliary text `*.IDX` declarations and unreferenced
  numeric auxiliary `00000xxx.idx` trees are exposed as hierarchical navigation
  surfaces when rows resolve to SSED component addresses;
- SSED Panel metadata from fixed XML/plist names, EXINFO-declared panel XML/plist
  names (`PANELXML` and XML/plist-valued `ROSQLNAME`), and mobile menu layouts is
  exposed as Panel surfaces; home and rendered Panel surface titles come from the
  Panel metadata when available, while internal numeric Panel ids remain routing
  details instead of user-visible titles; large BIN-backed panels are cursor-paged
  and continuous view can resolve targets beyond the first page; observed
  little-endian headered Panel BIN rows and headerless big-endian UTF-8 mobile
  rows are both decoded; external Panel HTML payloads are exposed as package-file
  resource targets, while other non-BIN external payloads remain explicit
  deferred diagnostics rather than misreported as missing BIN grids;
- SSED Britannica loose media directories are exposed as reader navigation:
  `whatday/*.body`/`*.top` CP932 HTML fragments become info pages with
  `lved.addrXXXXXXXX:YYYY` links rewritten to normal target tokens, and
  `top/top_*.dat` five-line media indexes become hierarchical navigation
  surfaces with thumbnail resources and SSED body-address targets;
- SSED loose KOJIEN6-style media: `_PCM_U/WaveFile.map` can back `PCMDATA.DIC`
  address resources with decrypted MP3 bytes, and `_MOVIE` entries are exposed
  as typed video resources;
- SSED HC/sidecar `lved.ziptomedia:*` sound references resolve to sibling loose
  `Sound_Files`/`*_Sound_Files` resources. Extensionless on-disk files are
  matched from `.wav` references, decrypted through LogoFontCipher, and exposed
  as normal typed audio resource tokens;
- SSED `PCMDATA.DIC` start/end range resources can be read as portable WAVE or
  MP3 audio without expanding the whole component;
- SSED loose `Sound/SoundData` stores are exposed as typed sound resources
  using `Sound/WaveFile.map`, including MWALEARN-style RIFF/WAVE records;
- SSED KOJIEN6-style `MONOSCR.DIC` component-address resources can be decoded
  as generated PNG images from 64x64 1bpp bitmap cells;
- SSED older `FIGURE.DIC` variable-size 1bpp figure bitmaps can be decoded as
  generated PNG image resources when renderer controls provide dimensions;
- SSED GA16/GAI16 bitmap gaiji fallback resources are exposed as per-glyph PNG
  image resources when the code maps into the observed direct JIS grid range,
  preserving the frontend-controlled gaiji priority policy without handing the
  whole GA16 file to the UI;
- SSED HC renderer inputs now carry structured HC profile metadata. When an
  `HC????.dll` is present, lvcore reports the HC profile id, exact DLL SHA-256,
  DLL size, and input-only support status; when only `EXINFO.INI` declares
  `HTMLDLL`, lvcore reports that declaration as a weaker profile source. HC
  inputs also perform a bounded stream scan for understood media controls and
  carry typed resource refs for observed `COLSCR.DIC`, `PCMDATA.DIC`,
  `MONOSCR.DIC`, and `FIGURE.DIC` targets. Preserved sidecar HTML normalization
  separately rewrites ziptomedia sound links to typed audio resources. This
  gives the future HC/profile renderer the resource tokens and binary-family
  identity it needs without claiming HC rendering parity yet;
- SSED Mac HC03E9-style PDFSpread sidecars are exposed as PDF resource refs
  from fullwidth HONMON page anchors, with the original PDF bytes available
  through the normal resource API;
- SSED plain HONMON renderer inputs infer conservative stream lengths from
  generic `1f09 0001` entry markers, their observed `1f02`-prefixed form, and
  native index body boundaries for marker-variant entries, keeping
  renderer/resource scans scoped to the focused entry where the boundary is
  known;
- SSED plain HONMON targets in `Native` and `GenericHtml` render modes now
  return a bounded common-HC HTML fallback instead of an empty deferred view.
  The fallback handles shared controls such as line breaks, common style spans,
  address links, URL spans, media placeholders, gaiji Unicode placeholders, and
  balanced tag closure. The output keeps `HcRenderInput` capability metadata and
  emits an explicit `hc_render_common_html_fallback` diagnostic, so reader apps
  get something displayable without mistaking it for product HC visual parity;
- SSED KOJIEN6-style `COLSMPL.DIC` records are parsed as typed color-sample
  metadata preserving exact Munsell notation and JIS labels, plus an explicitly
  estimated RGB swatch value for reader display when the Munsell notation parses;
- explicit deferred/unsupported diagnostics instead of fake output.

`logovista-tools` remains the research oracle while `lvcore-rs` ports stable
reader-facing behavior incrementally. Real corpus validation is the main
compatibility signal; synthetic fixtures are only regression guardrails for
known structures.

## Important Gaps

- SSED HC renderer parity is not ported yet. Plain SSED body targets expose
  structured HC renderer input and, for native/generic display, a common-HC HTML
  fallback with diagnostics. This is intentionally displayable but not a visual
  parity claim. Supported dense sidecar targets resolve to preserved HTML or
  exact sidecar text.
- SSED full-text search is implemented as sidecar-body search for understood
  dense HONMON databases plus a bounded, index-anchored HONMON scan for stream
  bodies; it is not a substitute for HC-rendered semantic text and may need more
  product tuning.
- SSED partial/non-prefix search still relies on bounded native index scans
  after the prefix prepass. More product-specific indexing may be needed for
  consistently low-latency substring search on very large packages.
- KOJIEN6-specific COLSMPL official RGB/rendering parity remains deferred; the
  parser preserves exact Munsell/label data and labels conservative RGB values
  as estimates rather than faking the proprietary color-map bridge.
- CHM table-of-contents semantics are supported at the reader-core level:
  lvcore reads `.hhc` Name/Local entries and exposes them as nested HANREI
  navigation trees with target tokens and scroll anchors. Higher-level reader
  wrapping/styling remains frontend work.

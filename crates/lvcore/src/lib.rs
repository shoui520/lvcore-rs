//! Rust reader core for LogoVista dictionary packages.
//!
//! `lvcore` is a reader library, not an exporter or corpus research toolkit.
//! Package drivers discover storage, shared providers expose reader
//! capabilities, and all frontend navigation goes through stable target tokens.

pub mod body;
mod chm;
pub mod crypto;
pub mod diagnostics;
pub mod error;
pub mod gaiji;
pub mod hourei;
mod image;
pub mod library;
pub mod lved_sqlite;
pub mod multiview;
pub mod navigation;
pub mod package;
pub mod render;
pub mod resources;
pub mod search;
pub mod sequence;
pub mod ssed;
pub mod ssed_aux_index;
pub mod ssed_color_sample;
pub mod ssed_encyclopedia;
pub mod ssed_figure;
pub mod ssed_ga16;
pub mod ssed_index;
pub mod ssed_loose_media;
pub mod ssed_menu;
pub mod ssed_multi;
pub mod ssed_panel;
pub mod ssed_pcmdata;
pub mod ssed_pdfspread;
pub mod ssed_screen_menu;
pub mod ssed_sidecar;
pub mod ssed_sound_data;
pub mod storage;
pub mod target;

pub use body::{BodyProvider, BodySourceKind, VisualBody};
pub use diagnostics::{Diagnostic, DiagnosticSeverity};
pub use error::{Error, Result};
pub use gaiji::{
    GaijiPolicy, GaijiProvider, GaijiResolution, GaijiSourcePreference, RichLabel,
    resolve_rich_label,
};
pub use hourei::{HoureiLawEntry, HoureiLawWindow, HoureiSearchHit, HoureiStore};
pub use library::{
    BookLibrary, LibraryImportReport, LibraryImportResult, LibrarySnapshot, RoutedTargetView,
    RoutedTargetWindow,
};
pub use lved_sqlite::{
    AndroidDictInfo, LvedInfoPage, LvedKeyFile, LvedListItem, LvedListWindow, LvedSearchHit,
    LvedSqliteStore, LvedSqliteSummary, LvedTreeIndex, LvedTreeIndexItem,
};
pub use navigation::{
    HomeSurface, NavigationProvider, NavigationStatus, NavigationSurface, NavigationSurfaceKind,
    NavigationTarget,
};
pub use package::{
    BookAlias, BookAliasKind, BookId, BookMetadata, BookPackage, BookPackageExt, Capability,
    DetectedPackage, DriverRegistry, FormatFamily, PackageDiscoveryOptions, PackageDriver,
};
pub use render::{
    HcRendererProfile, HcRendererProfileSource, HcRendererProfileStatus, RenderCapability,
    RenderMode, RenderOptions, RendererInput, RendererInputKind, RendererInputProvider,
    RendererProvider, ResolvedTargetKind, ResolvedTargetView,
};
pub use resources::{InternalResource, ResourceKind, ResourceProvider, ResourceRef, ResourceToken};
pub use search::{SearchHit, SearchMode, SearchPage, SearchProvider, SearchQuery, SearchScope};
pub use sequence::{SequenceHint, SequenceProvider, TargetWindow};
pub use ssed::{
    ANDROID_LVEDINFO_MAGIC, SSEDDATA_MAGIC, SSEDINFO_MAGIC, SsedCatalog, SsedComponent,
    SsedComponentRole, SsedDataFile, SsedDataHeader, SsedDataReader, SsedInfoLayout,
};
pub use ssed_aux_index::{
    SsedAuxIndexRow, SsedAuxIndexSpec, is_numeric_aux_index_filename,
    parse_aux_index_specs_from_exinfo, parse_aux_index_text_bytes,
};
pub use ssed_color_sample::{ColorSampleRecord, ColorSampleTable};
pub use ssed_encyclopedia::{
    ENCYCLOPEDIA_HEADER, SsedEncyclopediaIndex, SsedEncyclopediaRow, SsedEncyclopediaSection,
    parse_encyclopedia_index, parse_encyclopedia_index_bytes,
};
pub use ssed_index::{SsedIndexPointer, SsedIndexRow};
pub use ssed_loose_media::{
    BritannicaLooseResourcePath, BritannicaMediaRoot, BritannicaTopDat, BritannicaTopRecord,
    BritannicaWhatdayFile, BritannicaWhatdayKind, BritannicaWhatdayPath, LooseAddress, PcmuIndex,
    PcmuMapRecord,
};
pub use ssed_menu::{
    SsedMenuDestination, SsedMenuDestinationEncoding, SsedMenuLink, SsedMenuParse, SsedMenuRecord,
};
pub use ssed_panel::{
    SsedPanelBin, SsedPanelBinRecord, SsedPanelDataRef, SsedPanelInlineCell, SsedPanelXml,
};
pub use ssed_pdfspread::{
    PdfSpreadLookup, PdfSpreadSide, find_pdfspread_database, lookup_pdfspread,
    normalize_pdfspread_page_id, pdfspread_lookup_side, pdfspread_page_number,
};
pub use ssed_screen_menu::{
    SsedScreenMenuDirectTarget, SsedScreenMenuHotspot, SsedScreenMenuParse, SsedScreenMenuPointer,
    SsedScreenMenuPointerTarget, SsedScreenMenuRect, SsedScreenMenuScreen,
};
pub use ssed_sidecar::{
    SsedSidecarBody, SsedSidecarBodyResolver, SsedSidecarIdRule, SsedSidecarKind, SsedSidecarLookup,
};
pub use ssed_sound_data::{SoundDataIndex, SoundDataMapRecord};
pub use storage::{CaseFoldedDirectory, DirectoryStorage, StorageBackend};
pub use target::{InternalTarget, TargetKind, TargetLink, TargetToken};

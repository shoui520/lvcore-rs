//! Rust reader core for LogoVista dictionary packages.
//!
//! `lvcore` is a reader library, not an exporter or corpus research toolkit.
//! Package drivers discover storage, shared providers expose reader
//! capabilities, and all frontend navigation goes through stable target tokens.

pub mod body;
pub mod diagnostics;
pub mod error;
pub mod gaiji;
pub mod library;
pub mod lved_sqlite;
pub mod navigation;
pub mod package;
pub mod render;
pub mod resources;
pub mod search;
pub mod sequence;
pub mod ssed;
pub mod ssed_index;
pub mod storage;
pub mod target;

pub use body::{BodyProvider, BodySourceKind, VisualBody};
pub use diagnostics::{Diagnostic, DiagnosticSeverity};
pub use error::{Error, Result};
pub use gaiji::{GaijiPolicy, GaijiProvider, GaijiResolution, GaijiSourcePreference};
pub use library::BookLibrary;
pub use lved_sqlite::{LvedKeyFile, LvedSqliteStore};
pub use navigation::{
    HomeSurface, NavigationProvider, NavigationStatus, NavigationSurface, NavigationSurfaceKind,
};
pub use package::{
    BookId, BookMetadata, BookPackage, BookPackageExt, Capability, DetectedPackage, DriverRegistry,
    FormatFamily, PackageDriver,
};
pub use render::{
    RenderCapability, RenderMode, RenderOptions, RendererProvider, ResolvedTargetKind,
    ResolvedTargetView,
};
pub use resources::{InternalResource, ResourceKind, ResourceProvider, ResourceRef, ResourceToken};
pub use search::{SearchHit, SearchMode, SearchPage, SearchProvider, SearchQuery, SearchScope};
pub use sequence::{SequenceHint, SequenceProvider, TargetWindow};
pub use ssed::{
    SSEDDATA_MAGIC, SSEDINFO_MAGIC, SsedCatalog, SsedComponent, SsedComponentRole, SsedDataFile,
    SsedDataHeader, SsedDataReader, SsedInfoLayout,
};
pub use ssed_index::{SsedIndexPointer, SsedIndexRow};
pub use storage::{CaseFoldedDirectory, DirectoryStorage, StorageBackend};
pub use target::{InternalTarget, TargetKind, TargetLink, TargetToken};

mod drivers;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::body::BodyProvider;
use crate::error::{Error, Result};
use crate::gaiji::GaijiProvider;
use crate::navigation::NavigationProvider;
use crate::render::RendererProvider;
use crate::resources::ResourceProvider;
use crate::search::SearchProvider;
use crate::sequence::SequenceProvider;

pub use drivers::{
    HoureiDriver, LvedSqliteDriver, LvlMultiViewDriver, SsedDriver, StubBookPackage,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BookId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatFamily {
    Ssed,
    LvedSqlite3,
    LvlMultiView,
    Hourei,
    Unknown,
}

impl FormatFamily {
    pub fn ui_label(self) -> &'static str {
        match self {
            Self::Ssed => "SSED",
            Self::LvedSqlite3 => "LVED_SQLITE3",
            Self::LvlMultiView => "LVLMultiView",
            Self::Hourei => "Hourei",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    NativeSearch,
    FullTextSearch,
    TitleIndexBrowse,
    Menu,
    Toc,
    Panels,
    Hanrei,
    Resources,
    Gaiji,
    HcRenderInput,
    PreservedHtml,
    ContinuousView,
    LawNavigation,
    DeferredRendering,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookMetadata {
    pub book_id: BookId,
    pub format_family: FormatFamily,
    pub format_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_hint: Option<String>,
    pub root_fingerprint: String,
    pub capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedPackage {
    pub root: PathBuf,
    pub format_family: FormatFamily,
    pub confidence: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub evidence: Vec<String>,
}

pub trait BookPackage:
    Send
    + Sync
    + SearchProvider
    + NavigationProvider
    + RendererProvider
    + ResourceProvider
    + GaijiProvider
    + SequenceProvider
    + BodyProvider
{
    fn metadata(&self) -> &BookMetadata;
    fn root(&self) -> &Path;
}

pub trait BookPackageExt: BookPackage {
    fn book_id(&self) -> &BookId {
        &self.metadata().book_id
    }

    fn format_family(&self) -> FormatFamily {
        self.metadata().format_family
    }
}

impl<T: BookPackage + ?Sized> BookPackageExt for T {}

pub trait PackageDriver: Send + Sync {
    fn family(&self) -> FormatFamily;
    fn detect(&self, root: &Path) -> Result<Option<DetectedPackage>>;
    fn open(&self, root: &Path) -> Result<Box<dyn BookPackage>>;
}

pub struct DriverRegistry {
    drivers: Vec<Box<dyn PackageDriver>>,
}

impl DriverRegistry {
    pub fn new(drivers: Vec<Box<dyn PackageDriver>>) -> Self {
        Self { drivers }
    }

    pub fn detect(&self, root: &Path) -> Result<Vec<DetectedPackage>> {
        let mut rows = Vec::new();
        for driver in &self.drivers {
            if let Some(detected) = driver.detect(root)? {
                rows.push(detected);
            }
        }
        rows.sort_by(|a, b| {
            b.confidence
                .cmp(&a.confidence)
                .then_with(|| a.root.cmp(&b.root))
        });
        Ok(rows)
    }

    pub fn open_best(&self, root: &Path) -> Result<Box<dyn BookPackage>> {
        let detected = self
            .detect(root)?
            .into_iter()
            .next()
            .ok_or(Error::UnrecognizedPackage)?;
        for driver in &self.drivers {
            if driver.family() == detected.format_family {
                return driver.open(&detected.root);
            }
        }
        Err(Error::UnsupportedFamily(
            detected.format_family.ui_label().to_owned(),
        ))
    }
}

impl Default for DriverRegistry {
    fn default() -> Self {
        Self::new(vec![
            Box::new(SsedDriver),
            Box::new(LvedSqliteDriver),
            Box::new(LvlMultiViewDriver),
            Box::new(HoureiDriver),
        ])
    }
}

use serde::{Deserialize, Serialize};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::diagnostics::Diagnostic;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Image,
    Audio,
    Template,
    Colscr,
    PcmData,
    SoundData,
    Video,
    MediaBlob,
    Pdf,
    Html,
    Css,
    Javascript,
    Font,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InternalResource {
    PackageFile {
        path: String,
        resource_kind: ResourceKind,
    },
    SsedLooseFile {
        root_name: String,
        path: String,
        resource_kind: ResourceKind,
    },
    SsedComponentAddress {
        component: String,
        block: u32,
        offset: u32,
        resource_kind: ResourceKind,
    },
    SsedPcmDataRange {
        component: String,
        start_block: u32,
        start_offset: u32,
        end_block: u32,
        end_offset: u32,
    },
    SsedFigure {
        component: String,
        block: u32,
        offset: u32,
        width: u32,
        height: u32,
    },
    SsedGa16Glyph {
        path: String,
        code: String,
    },
    MediaBlob {
        store: String,
        key: String,
        resource_kind: ResourceKind,
    },
    SoundData {
        sound_id: u32,
    },
    LooseMovie {
        movie_id: String,
    },
    SsedPdfSpread {
        page_id: String,
    },
    ChmFile {
        chm_path: String,
        entry_path: String,
        resource_kind: ResourceKind,
    },
    Unsupported {
        reason: String,
    },
}

impl InternalResource {
    pub fn resource_kind(&self) -> ResourceKind {
        match self {
            Self::PackageFile { resource_kind, .. }
            | Self::SsedLooseFile { resource_kind, .. }
            | Self::SsedComponentAddress { resource_kind, .. }
            | Self::ChmFile { resource_kind, .. } => *resource_kind,
            Self::SsedPcmDataRange { .. } => ResourceKind::PcmData,
            Self::SsedFigure { .. } | Self::SsedGa16Glyph { .. } => ResourceKind::Image,
            Self::MediaBlob { resource_kind, .. } => *resource_kind,
            Self::SoundData { .. } => ResourceKind::SoundData,
            Self::LooseMovie { .. } => ResourceKind::Video,
            Self::SsedPdfSpread { .. } => ResourceKind::Pdf,
            Self::Unsupported { .. } => ResourceKind::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
/// Opaque, round-trippable frontend handle for LogoVista resources.
///
/// Like `TargetToken`, this is transport data, not authority. Resource tokens
/// are intentionally stable enough for frontend routing and cache keys, but all
/// decoded internals must still be resolved through `ResourceProvider`, where
/// package-relative paths, table names, and byte ranges are validated against
/// the opened book.
pub struct ResourceToken(String);

impl ResourceToken {
    pub fn from_opaque(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn new(resource: &InternalResource) -> Result<Self> {
        let envelope = ResourceEnvelope {
            version: 1,
            resource: resource.clone(),
        };
        let bytes = serde_json::to_vec(&envelope)?;
        Ok(Self(URL_SAFE_NO_PAD.encode(bytes)))
    }

    pub fn decode(&self) -> Result<InternalResource> {
        let bytes = URL_SAFE_NO_PAD
            .decode(&self.0)
            .map_err(|_| Error::InvalidResourceToken)?;
        let envelope: ResourceEnvelope =
            serde_json::from_slice(&bytes).map_err(|_| Error::InvalidResourceToken)?;
        if envelope.version != 1 {
            return Err(Error::InvalidResourceToken);
        }
        Ok(envelope.resource)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ResourceToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<&InternalResource> for ResourceToken {
    type Error = Error;

    fn try_from(value: &InternalResource) -> Result<Self> {
        Self::new(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ResourceEnvelope {
    version: u8,
    resource: InternalResource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRef {
    pub token: ResourceToken,
    pub kind: ResourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

pub trait ResourceProvider: Send + Sync {
    fn resolve_resource(&self, token: &ResourceToken) -> Result<ResourceRef>;
    fn read_resource(&self, token: &ResourceToken) -> Result<Vec<u8>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_round_trips_package_file_resource() {
        let resource = InternalResource::PackageFile {
            path: "Templates/B123.svg".to_owned(),
            resource_kind: ResourceKind::Template,
        };
        let token = ResourceToken::new(&resource).unwrap();
        assert_eq!(token.decode().unwrap(), resource);
        assert!(!token.as_str().contains("Templates"));
    }

    #[test]
    fn token_round_trips_ssed_loose_file_resource() {
        let resource = InternalResource::SsedLooseFile {
            root_name: "_DCT_BRI2016_Media".to_owned(),
            path: "whatday/12-5.body".to_owned(),
            resource_kind: ResourceKind::Html,
        };
        let token = ResourceToken::new(&resource).unwrap();
        assert_eq!(token.decode().unwrap(), resource);
        assert!(!token.as_str().contains("BRI2016"));
    }

    #[test]
    fn token_round_trips_chm_file_resource() {
        let resource = InternalResource::ChmFile {
            chm_path: "HANREI.chm".to_owned(),
            entry_path: "Source/top.htm".to_owned(),
            resource_kind: ResourceKind::Html,
        };
        let token = ResourceToken::new(&resource).unwrap();
        assert_eq!(token.decode().unwrap(), resource);
        assert!(!token.as_str().contains("HANREI"));
    }

    #[test]
    fn token_round_trips_loose_movie_resource() {
        let resource = InternalResource::LooseMovie {
            movie_id: "05011360".to_owned(),
        };
        let token = ResourceToken::new(&resource).unwrap();
        assert_eq!(token.decode().unwrap(), resource);
        assert!(!token.as_str().contains("05011360"));
    }

    #[test]
    fn token_round_trips_sounddata_resource() {
        let resource = InternalResource::SoundData { sound_id: 32768 };
        let token = ResourceToken::new(&resource).unwrap();
        assert_eq!(token.decode().unwrap(), resource);
        assert!(!token.as_str().contains("32768"));
    }

    #[test]
    fn token_round_trips_ssed_pdfspread_resource() {
        let resource = InternalResource::SsedPdfSpread {
            page_id: "００００００１７".to_owned(),
        };
        let token = ResourceToken::new(&resource).unwrap();
        assert_eq!(token.decode().unwrap(), resource);
        assert!(!token.as_str().contains("00000017"));
    }

    #[test]
    fn token_round_trips_ssed_pcmdata_range_resource() {
        let resource = InternalResource::SsedPcmDataRange {
            component: "PCMDATA.DIC".to_owned(),
            start_block: 100,
            start_offset: 32,
            end_block: 100,
            end_offset: 63,
        };
        let token = ResourceToken::new(&resource).unwrap();
        assert_eq!(token.decode().unwrap(), resource);
        assert!(!token.as_str().contains("PCMDATA"));
    }

    #[test]
    fn token_round_trips_ssed_figure_resource() {
        let resource = InternalResource::SsedFigure {
            component: "FIGURE.DIC".to_owned(),
            block: 1200,
            offset: 17,
            width: 9,
            height: 2,
        };
        let token = ResourceToken::new(&resource).unwrap();
        assert_eq!(token.decode().unwrap(), resource);
        assert!(!token.as_str().contains("FIGURE"));
    }

    #[test]
    fn token_round_trips_ssed_ga16_glyph_resource() {
        let resource = InternalResource::SsedGa16Glyph {
            path: "GA16FULL".to_owned(),
            code: "B121".to_owned(),
        };
        let token = ResourceToken::new(&resource).unwrap();
        assert_eq!(token.decode().unwrap(), resource);
        assert!(!token.as_str().contains("B121"));
    }
}

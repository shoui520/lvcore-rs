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
    MediaBlob {
        store: String,
        key: String,
        resource_kind: ResourceKind,
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
            | Self::MediaBlob { resource_kind, .. }
            | Self::ChmFile { resource_kind, .. } => *resource_kind,
            Self::Unsupported { .. } => ResourceKind::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
}

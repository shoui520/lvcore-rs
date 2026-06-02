use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::error::{Error, Result};
use crate::package::BookId;
use crate::render::{RenderOptions, ResolvedTargetView};
use crate::search::{SearchHit, SearchPage};
use crate::target::TargetToken;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

const SEARCH_RESULT_SEQUENCE_VERSION: u8 = 1;
const SEARCH_RESULT_SEQUENCE_MAX_TARGETS: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SequenceHint {
    TitleIndexOrder { value: String },
    SearchResults { value: String },
    BodyOrder,
    MenuOrder { value: String },
    PanelOrder { value: String },
    LvedListOrder,
    LvedTreeOrder,
    HoureiLawArticleOrder,
    MultiviewTreeOrder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResultSequence {
    pub version: u8,
    pub targets: Vec<SearchResultSequenceTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResultSequenceTarget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub book_id: Option<BookId>,
    pub target: TargetToken,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl SearchResultSequence {
    pub fn new(targets: Vec<SearchResultSequenceTarget>) -> Result<Self> {
        validate_search_result_sequence_len(targets.len())?;
        Ok(Self {
            version: SEARCH_RESULT_SEQUENCE_VERSION,
            targets,
        })
    }

    pub fn from_search_page(page: &SearchPage) -> Result<Self> {
        Self::from_hits(&page.hits)
    }

    pub fn from_hits(hits: &[SearchHit]) -> Result<Self> {
        Self::new(
            hits.iter()
                .map(|hit| SearchResultSequenceTarget {
                    book_id: Some(hit.book_id.clone()),
                    target: hit.target.clone(),
                    title: Some(hit.title_text.clone()),
                })
                .collect(),
        )
    }

    pub fn encode(&self) -> Result<String> {
        self.validate()?;
        Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(self)?))
    }

    pub fn decode(value: &str) -> Result<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(Error::Driver("empty search-result sequence".to_owned()));
        }
        let bytes = if trimmed.starts_with('{') {
            trimmed.as_bytes().to_vec()
        } else {
            URL_SAFE_NO_PAD
                .decode(trimmed)
                .map_err(|_| Error::Driver("invalid search-result sequence encoding".to_owned()))?
        };
        let sequence: Self = serde_json::from_slice(&bytes).map_err(|error| {
            Error::Driver(format!("invalid search-result sequence JSON: {error}"))
        })?;
        sequence.validate()?;
        Ok(sequence)
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != SEARCH_RESULT_SEQUENCE_VERSION {
            return Err(Error::Driver(format!(
                "unsupported search-result sequence version {}",
                self.version
            )));
        }
        validate_search_result_sequence_len(self.targets.len())
    }
}

fn validate_search_result_sequence_len(len: usize) -> Result<()> {
    if len > SEARCH_RESULT_SEQUENCE_MAX_TARGETS {
        return Err(Error::Driver(format!(
            "search-result sequence has {len} targets; maximum is {SEARCH_RESULT_SEQUENCE_MAX_TARGETS}"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetWindow {
    pub center: ResolvedTargetView,
    pub before: Vec<ResolvedTargetView>,
    pub after: Vec<ResolvedTargetView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

pub trait SequenceProvider: Send + Sync {
    fn resolve_target_window(
        &self,
        target: &TargetToken,
        sequence_hint: Option<&SequenceHint>,
        before: usize,
        after: usize,
        options: &RenderOptions,
    ) -> Result<TargetWindow>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_hint_has_frontend_safe_tagged_json_shape() {
        assert_eq!(
            serde_json::to_value(SequenceHint::TitleIndexOrder {
                value: "title-index".to_owned()
            })
            .unwrap(),
            serde_json::json!({ "kind": "title_index_order", "value": "title-index" })
        );
        assert_eq!(
            serde_json::to_value(SequenceHint::BodyOrder).unwrap(),
            serde_json::json!({ "kind": "body_order" })
        );
        assert_eq!(
            serde_json::from_value::<SequenceHint>(serde_json::json!({
                "kind": "panel_order",
                "value": "01010000"
            }))
            .unwrap(),
            SequenceHint::PanelOrder {
                value: "01010000".to_owned()
            }
        );
    }

    #[test]
    fn search_result_sequence_round_trips_as_opaque_hint_value() {
        let target = TargetToken::from_opaque("opaque-target");
        let sequence = SearchResultSequence::new(vec![SearchResultSequenceTarget {
            book_id: None,
            target: target.clone(),
            title: Some("alpha".to_owned()),
        }])
        .unwrap();

        let encoded = sequence.encode().unwrap();
        assert!(!encoded.contains("alpha"));
        assert_eq!(SearchResultSequence::decode(&encoded).unwrap(), sequence);

        let json_value = serde_json::to_string(&sequence).unwrap();
        assert_eq!(SearchResultSequence::decode(&json_value).unwrap(), sequence);
    }
}

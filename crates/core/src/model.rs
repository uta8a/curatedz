use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const NORMALIZER_VERSION: &str = "1";
pub const HEAD_KEY: &str = "state/v1/head.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Document {
    pub schema_version: u32,
    pub document_id: String,
    pub canonical_url: Option<String>,
    pub title: String,
    pub published_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub authors: Vec<String>,
    pub summary: Option<String>,
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Discovery {
    pub discovery_id: String,
    pub document_id: String,
    pub source_id: String,
    pub source_item_id: String,
    pub observed_at: DateTime<Utc>,
    pub run_id: String,
    pub raw_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Association {
    pub document_id: String,
    pub topic_id: String,
    pub discovery_id: String,
    pub config_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capture {
    pub source_id: String,
    pub source_url: String,
    pub config_version: String,
    pub observed_at: DateTime<Utc>,
    pub raw_key: Option<String>,
    pub content_type: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Counts {
    pub documents: usize,
    pub unchanged: usize,
    pub discoveries: usize,
    pub associations: usize,
    pub failed_sources: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceResult {
    pub source_id: String,
    pub entries: usize,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub normalizer_version: String,
    pub run_id: String,
    pub config_version: String,
    pub request_hash: String,
    pub replay_of: Option<String>,
    pub generation: u64,
    pub status: String,
    pub counts: Counts,
    pub sources: Vec<SourceResult>,
    pub objects: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexEntry {
    pub content_hash: String,
    pub observed_at: DateTime<Utc>,
    pub source_id: String,
    #[serde(default)]
    pub normalizer_revision: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommittedRun {
    pub manifest_key: String,
    pub request_hash: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Head {
    pub generation: u64,
    pub documents: BTreeMap<String, IndexEntry>,
    pub runs: BTreeMap<String, CommittedRun>,
}

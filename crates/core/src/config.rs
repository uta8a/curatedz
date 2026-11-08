use crate::{Error, Result, hash};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub sources: Vec<Source>,
    pub topics: Vec<Topic>,
    #[serde(default)]
    pub http: HttpConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Topic {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub sources: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpConfig {
    pub timeout_seconds: u64,
    pub attempts: u32,
    pub max_bytes: usize,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 10,
            attempts: 2,
            max_bytes: 2 * 1024 * 1024,
        }
    }
}

pub fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn http_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| Error::Config("URLが不正です".into()))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Error::Config(
            "http/httpsの認証情報なしURLが必要です".into(),
        ));
    }
    Ok(url)
}

impl Config {
    pub fn parse(yaml: &str) -> Result<Self> {
        let config: Self =
            serde_yaml_ng::from_str(yaml).map_err(|e| Error::Config(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || self.sources.is_empty()
            || self.sources.len() > 20
            || self.topics.is_empty()
            || self.topics.len() > 100
        {
            return Err(Error::Config(
                "schema_version=1、情報源1〜20件、テーマ1〜100件が必要です".into(),
            ));
        }
        let mut ids = BTreeSet::new();
        let mut urls = BTreeSet::new();
        for source in &self.sources {
            if !safe_id(&source.id)
                || !ids.insert(&source.id)
                || !urls.insert(http_url(&source.url)?.to_string())
            {
                return Err(Error::Config("情報源のIDまたはURLが重複・不正です".into()));
            }
        }
        let mut topic_ids = BTreeSet::new();
        for topic in &self.topics {
            let refs: BTreeSet<_> = topic.sources.iter().collect();
            if !safe_id(&topic.id)
                || !topic_ids.insert(&topic.id)
                || topic.name.trim().is_empty()
                || refs.is_empty()
                || refs.len() != topic.sources.len()
                || refs.iter().any(|id| !ids.contains(*id))
            {
                return Err(Error::Config("テーマID・名前・情報源参照が不正です".into()));
            }
        }
        if !(1..=10).contains(&self.http.timeout_seconds)
            || !(1..=2).contains(&self.http.attempts)
            || !(1..=2 * 1024 * 1024).contains(&self.http.max_bytes)
        {
            return Err(Error::Config("HTTP設定が上限を超えています".into()));
        }
        Ok(())
    }

    pub fn version(&self) -> Result<String> {
        Ok(hash(&serde_json::to_vec(self)?))
    }

    pub fn selection(&self, topics: &[String]) -> Result<BTreeMap<String, Vec<String>>> {
        let requested: BTreeSet<_> = topics.iter().collect();
        if requested
            .iter()
            .any(|id| !self.topics.iter().any(|t| &t.id == *id))
        {
            return Err(Error::Config("指定テーマがありません".into()));
        }
        let mut result: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for topic in &self.topics {
            if requested.is_empty() || requested.contains(&topic.id) {
                for source in &topic.sources {
                    result
                        .entry(source.clone())
                        .or_default()
                        .push(topic.id.clone());
                }
            }
        }
        for values in result.values_mut() {
            values.sort();
        }
        Ok(result)
    }
}

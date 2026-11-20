use crate::{Error, Result, hash, model::PROCESSING_VERSION, store::Store};
use chrono::{DateTime, Utc};
use readabilityrs::{Readability, ReadabilityOptions};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, net::IpAddr};
use url::Url;

pub const CONTENT_HEAD_KEY: &str = "article-content/v1/state/head.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentStatus {
    Succeeded,
    FetchFailed,
    ExtractionFailed,
    Skipped,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContentFailure {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContentRecord {
    pub schema_version: u32,
    pub article_id: String,
    pub requested_url: String,
    pub final_url: Option<String>,
    pub declared_canonical_url: Option<String>,
    pub raw_key: Option<String>,
    pub text_key: Option<String>,
    pub text_hash: Option<String>,
    pub status: ContentStatus,
    pub fetched_at: DateTime<Utc>,
    pub http_status: Option<u16>,
    pub content_type: Option<String>,
    pub failure: Option<ContentFailure>,
    pub processing_version: String,
    pub attempt: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentIndexEntry {
    pub record_key: String,
    pub requested_url: String,
    pub processing_version: String,
    pub status: ContentStatus,
    pub text_hash: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentCommitEntry {
    pub article_id: String,
    pub record_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentCommit {
    pub schema_version: u32,
    pub generation: u64,
    pub entries: Vec<ContentCommitEntry>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentHead {
    pub generation: u64,
    pub records: BTreeMap<String, ContentIndexEntry>,
    pub commits: BTreeMap<u64, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedContent {
    pub text: String,
    pub declared_canonical_url: Option<String>,
}

pub fn validate_article_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| Error::Security("記事URLが不正です".into()))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
    {
        return Err(Error::Security(
            "記事URLは認証情報なしのHTTP(S)に限ります".into(),
        ));
    }
    if !matches!(url.port_or_known_default(), Some(80 | 443)) {
        return Err(Error::Security("記事URLのportは80/443に限ります".into()));
    }
    Ok(url)
}

pub fn validate_public_ip(ip: IpAddr) -> Result<()> {
    let rejected = match ip {
        IpAddr::V4(ip) => {
            let [a, b, _, _] = ip.octets();
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_multicast()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_documentation()
                || a == 0
                || a >= 240
                || (a == 100 && (64..=127).contains(&b))
                || (a == 192 && b == 0)
                || (a == 198 && matches!(b, 18 | 19))
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_multicast()
                || ip.is_unspecified()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
                || ip.segments()[0..2] == [0x2001, 0x0db8]
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| validate_public_ip(IpAddr::V4(mapped)).is_err())
        }
    };
    if rejected {
        return Err(Error::Security(
            "公開インターネット以外の接続先を拒否しました".into(),
        ));
    }
    Ok(())
}

pub fn extract(html: &str, base_url: &str) -> Result<ExtractedContent> {
    let base = validate_article_url(base_url)?;
    let document = Html::parse_document(html);
    let canonical_selector = Selector::parse("link[rel~='canonical']")
        .map_err(|_| Error::Extraction("canonical selectorが不正です".into()))?;
    let declared_canonical_url = document
        .select(&canonical_selector)
        .find_map(|element| element.value().attr("href"))
        .and_then(|value| base.join(value).ok())
        .and_then(|url| validate_article_url(url.as_ref()).ok())
        .map(|url| url.to_string());
    let options = ReadabilityOptions::builder()
        .max_elems_to_parse(100_000)
        .char_threshold(80)
        .build();
    let readability = Readability::new(html, Some(base_url), Some(options))
        .map_err(|_| Error::Extraction("HTMLを解析できません".into()))?;
    let article = readability
        .parse()
        .ok_or_else(|| Error::Extraction("記事本文を抽出できません".into()))?;
    let text = article.text_content.unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return Err(Error::Extraction("記事本文が空です".into()));
    }
    Ok(ExtractedContent {
        text,
        declared_canonical_url,
    })
}

pub async fn is_current_success(
    store: &dyn Store,
    article_id: &str,
    requested_url: &str,
    processing_version: &str,
) -> Result<bool> {
    let Some(object) = store.get(CONTENT_HEAD_KEY).await? else {
        return Ok(false);
    };
    let head: ContentHead = serde_json::from_slice(&object.bytes)?;
    Ok(head.records.get(article_id).is_some_and(|entry| {
        entry.status == ContentStatus::Succeeded
            && entry.requested_url == requested_url
            && entry.processing_version == processing_version
    }))
}

pub async fn publish(
    store: &dyn Store,
    mut record: ContentRecord,
    raw: Option<&[u8]>,
    text: Option<&str>,
) -> Result<ContentCommit> {
    if record.processing_version != PROCESSING_VERSION {
        return Err(Error::Config("未対応の本文処理versionです".into()));
    }
    if let Some(bytes) = raw {
        let raw_hash = hash(bytes);
        let key = format!(
            "article-content/v1/raw/article={}/response={}.bin",
            record.article_id,
            raw_hash.trim_start_matches("sha256:")
        );
        store.immutable(&key, bytes).await?;
        record.raw_key = Some(key);
    }
    if let Some(value) = text {
        let text_hash = hash(value.as_bytes());
        let key = format!(
            "article-content/v1/blobs/{}.txt",
            text_hash.trim_start_matches("sha256:")
        );
        store.immutable(&key, value.as_bytes()).await?;
        record.text_key = Some(key);
        record.text_hash = Some(text_hash);
    }
    let record_bytes = serde_json::to_vec(&record)?;
    let record_hash = hash(&record_bytes);
    let record_key = format!(
        "article-content/v1/records/article={}/version={}/{}.json",
        record.article_id,
        record.processing_version,
        record_hash.trim_start_matches("sha256:")
    );
    store.immutable(&record_key, &record_bytes).await?;

    for _ in 0..8 {
        let current = store.get(CONTENT_HEAD_KEY).await?;
        let mut head: ContentHead = current
            .as_ref()
            .map(|object| serde_json::from_slice(&object.bytes))
            .transpose()?
            .unwrap_or_default();
        if head.records.get(&record.article_id).is_some_and(|entry| {
            entry.record_key == record_key
                || (entry.status == ContentStatus::Succeeded
                    && entry.requested_url == record.requested_url
                    && entry.processing_version == record.processing_version)
        }) {
            return Ok(ContentCommit {
                schema_version: 1,
                generation: head.generation,
                entries: Vec::new(),
            });
        }
        let generation = head.generation + 1;
        let commit = ContentCommit {
            schema_version: 1,
            generation,
            entries: vec![ContentCommitEntry {
                article_id: record.article_id.clone(),
                record_key: record_key.clone(),
            }],
        };
        let commit_bytes = serde_json::to_vec(&commit)?;
        let commit_hash = hash(&commit_bytes);
        let commit_key = format!(
            "article-content/v1/commits/generation={generation:020}-{}.json",
            commit_hash.trim_start_matches("sha256:")
        );
        store.immutable(&commit_key, &commit_bytes).await?;
        head.generation = generation;
        head.records.insert(
            record.article_id.clone(),
            ContentIndexEntry {
                record_key: record_key.clone(),
                requested_url: record.requested_url.clone(),
                processing_version: record.processing_version.clone(),
                status: record.status.clone(),
                text_hash: record.text_hash.clone(),
            },
        );
        head.commits.insert(generation, commit_key);
        if store
            .compare_and_swap(
                CONTENT_HEAD_KEY,
                current.as_ref().map(|object| object.version.as_str()),
                &serde_json::to_vec(&head)?,
            )
            .await?
        {
            return Ok(commit);
        }
    }
    Err(Error::Storage(
        "本文索引更新が8回競合しました。SQSで再試行してください".into(),
    ))
}

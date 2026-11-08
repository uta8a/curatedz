use crate::{
    Error, Result,
    config::{Config, safe_id},
    fetch::Fetcher,
    hash, id,
    model::*,
    normalize,
    store::Store,
};
use chrono::{DateTime, Utc};
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::BTreeMap, time::Instant};

#[derive(Clone, Debug)]
pub struct Run {
    pub run_id: String,
    pub topics: Vec<String>,
    pub replay_of: Option<String>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct Contract<'a> {
    config: &'a Config,
    selection: &'a BTreeMap<String, Vec<String>>,
    replay_of: &'a Option<String>,
    normalizer_version: &'static str,
}

async fn load<T: DeserializeOwned>(store: &dyn Store, key: &str) -> Result<T> {
    let object = store
        .get(key)
        .await?
        .ok_or_else(|| Error::Storage(format!("オブジェクトがありません: {key}")))?;
    Ok(serde_json::from_slice(&object.bytes)?)
}

fn jsonl<T: Serialize>(values: impl IntoIterator<Item = T>) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    for value in values {
        serde_json::to_writer(&mut result, &value)?;
        result.push(b'\n');
    }
    crate::store::check_size(&result)?;
    Ok(result)
}

fn capture_key(run_id: &str, source_id: &str) -> String {
    format!("raw/v1/run={run_id}/source={source_id}/capture.json")
}

fn rank(index: &IndexEntry) -> (DateTime<Utc>, u32, &str, &str) {
    (
        index.observed_at,
        index.normalizer_revision,
        &index.source_id,
        &index.content_hash,
    )
}

async fn committed(
    store: &dyn Store,
    head: &Head,
    run_id: &str,
    request_hash: &str,
) -> Result<Option<Manifest>> {
    match head.runs.get(run_id) {
        Some(run) if run.request_hash != request_hash => Err(Error::Config(
            "同じrun_idを異なる設定・選択・replay元で使えません".into(),
        )),
        Some(run) => Ok(Some(load(store, &run.manifest_key).await?)),
        None => Ok(None),
    }
}

pub async fn execute(
    config: &Config,
    run: &Run,
    fetcher: &dyn Fetcher,
    store: &dyn Store,
) -> Result<Manifest> {
    let started = Instant::now();
    config.validate()?;
    if !safe_id(&run.run_id)
        || run
            .replay_of
            .as_ref()
            .is_some_and(|id| !safe_id(id) || id == &run.run_id)
    {
        return Err(Error::Config("run_id/replay元が不正です".into()));
    }
    let selection = config.selection(&run.topics)?;
    let contract = serde_json::to_vec(&Contract {
        config,
        selection: &selection,
        replay_of: &run.replay_of,
        normalizer_version: NORMALIZER_VERSION,
    })?;
    let request_hash = hash(&contract);
    let config_version = config.version()?;
    let initial_head = store.get(HEAD_KEY).await?;
    if let Some(object) = initial_head {
        let head: Head = serde_json::from_slice(&object.bytes)?;
        if let Some(result) = committed(store, &head, &run.run_id, &request_hash).await? {
            return Ok(result);
        }
    }
    store
        .immutable(
            &format!("raw/v1/run={}/request.json", run.run_id),
            &contract,
        )
        .await?;
    let mut candidates: BTreeMap<String, (Document, IndexEntry)> = BTreeMap::new();
    let mut discoveries = BTreeMap::new();
    let mut associations = BTreeMap::new();
    let mut results = Vec::new();
    for (source_id, topics) in &selection {
        let source = config
            .sources
            .iter()
            .find(|source| &source.id == source_id)
            .expect("設定参照を検証済み");
        let key = capture_key(&run.run_id, source_id);
        let capture: Capture = if let Some(object) = store.get(&key).await? {
            serde_json::from_slice(&object.bytes)?
        } else {
            let captured = if let Some(original) = &run.replay_of {
                load::<Capture>(store, &capture_key(original, source_id)).await?
            } else {
                let response = fetcher.fetch(source).await;
                let mut captured = Capture {
                    source_id: source_id.clone(),
                    source_url: source.url.clone(),
                    config_version: config_version.clone(),
                    observed_at: run.observed_at,
                    raw_key: None,
                    content_type: None,
                    error: None,
                };
                match response {
                    Ok(response) => {
                        if response.bytes.len() > config.http.max_bytes {
                            return Err(Error::Fetch("取得データが上限を超えています".into()));
                        }
                        let raw_key = format!(
                            "raw/v1/run={}/source={}/body={}.bin",
                            run.run_id,
                            source_id,
                            hash(&response.bytes)
                        );
                        store.immutable(&raw_key, &response.bytes).await?;
                        captured.raw_key = Some(raw_key);
                        captured.content_type = response.content_type;
                    }
                    Err(error) => captured.error = Some(error.to_string()),
                }
                captured
            };
            // 同一runの並行実行では最初に保存された取得結果を両者で使う。
            store
                .compare_and_swap(&key, None, &serde_json::to_vec(&captured)?)
                .await?;
            load::<Capture>(store, &key).await?
        };
        let parsed = match (&capture.raw_key, &capture.error) {
            (Some(raw_key), None) => {
                let body = store
                    .get(raw_key)
                    .await?
                    .ok_or_else(|| Error::Storage("Rawがありません".into()))?;
                normalize::parse(&body.bytes, source_id, &capture.source_url)
            }
            _ => Err(Error::Fetch(
                capture
                    .error
                    .clone()
                    .unwrap_or_else(|| "取得結果が不正です".into()),
            )),
        };
        match parsed {
            Ok(entries) => {
                results.push(SourceResult {
                    source_id: source_id.clone(),
                    entries: entries.len(),
                    error: None,
                });
                for entry in entries {
                    let document = entry.document;
                    let index = IndexEntry {
                        content_hash: document.content_hash.clone(),
                        observed_at: capture.observed_at,
                        source_id: source_id.clone(),
                        normalizer_revision: NORMALIZER_VERSION
                            .parse()
                            .expect("正規化版は正の整数"),
                    };
                    if candidates
                        .get(&document.document_id)
                        .is_none_or(|(_, current)| rank(&index) > rank(current))
                    {
                        candidates.insert(document.document_id.clone(), (document.clone(), index));
                    }
                    let discovery_id = id(&[
                        "discovery",
                        &run.run_id,
                        source_id,
                        &entry.source_item_id,
                        &document.document_id,
                    ]);
                    discoveries.insert(
                        discovery_id.clone(),
                        Discovery {
                            discovery_id: discovery_id.clone(),
                            document_id: document.document_id.clone(),
                            source_id: source_id.clone(),
                            source_item_id: entry.source_item_id,
                            observed_at: capture.observed_at,
                            run_id: run.run_id.clone(),
                            raw_key: capture.raw_key.clone().expect("解析対象Rawあり"),
                        },
                    );
                    for topic_id in topics {
                        associations.insert(
                            (discovery_id.clone(), topic_id.clone()),
                            Association {
                                document_id: document.document_id.clone(),
                                topic_id: topic_id.clone(),
                                discovery_id: discovery_id.clone(),
                                config_version: config_version.clone(),
                            },
                        );
                    }
                }
            }
            Err(error) => {
                tracing::warn!(run_id = run.run_id, source_id, stage = "source_failed", error = %error);
                results.push(SourceResult {
                    source_id: source_id.clone(),
                    entries: 0,
                    error: Some(error.to_string()),
                });
            }
        }
    }
    for _ in 0..8 {
        let current = store.get(HEAD_KEY).await?;
        let mut head: Head = current
            .as_ref()
            .map(|object| serde_json::from_slice(&object.bytes))
            .transpose()?
            .unwrap_or_default();
        if let Some(result) = committed(store, &head, &run.run_id, &request_hash).await? {
            return Ok(result);
        }
        let mut changed = Vec::new();
        for (document_id, (document, index)) in &candidates {
            match head.documents.get(document_id) {
                Some(existing) if rank(index) <= rank(existing) => continue,
                Some(existing) if existing.content_hash == index.content_hash => {}
                _ => changed.push(document.clone()),
            }
            head.documents.insert(document_id.clone(), index.clone());
        }
        let counts = Counts {
            documents: changed.len(),
            unchanged: candidates.len() - changed.len(),
            discoveries: discoveries.len(),
            associations: associations.len(),
            failed_sources: results
                .iter()
                .filter(|result| result.error.is_some())
                .count(),
        };
        let status = if counts.failed_sources == results.len() {
            "failed"
        } else if counts.failed_sources > 0 {
            "partial"
        } else {
            "success"
        };
        head.generation += 1;
        let mut data = BTreeMap::new();
        data.insert("documents", jsonl(&changed)?);
        data.insert("discoveries", jsonl(discoveries.values())?);
        data.insert("associations", jsonl(associations.values())?);
        // 親headと実際の出力からキーを決め、CAS競合後に別の計画を上書きしない。
        let transaction = hash(&serde_json::to_vec(&(
            &request_hash,
            &run.run_id,
            current.as_ref().map(|o| &o.version),
            &data,
        ))?);
        let prefix = format!("batches/v1/{transaction}");
        let mut objects = BTreeMap::new();
        for (kind, bytes) in data {
            let key = format!("{prefix}/{kind}.jsonl");
            store.immutable(&key, &bytes).await?;
            objects.insert(kind.into(), key);
        }
        let manifest = Manifest {
            schema_version: 1,
            normalizer_version: NORMALIZER_VERSION.into(),
            run_id: run.run_id.clone(),
            config_version: config_version.clone(),
            request_hash: request_hash.clone(),
            replay_of: run.replay_of.clone(),
            generation: head.generation,
            status: status.into(),
            counts,
            sources: results.clone(),
            objects,
        };
        let manifest_key = format!("{prefix}/manifest.json");
        store
            .immutable(&manifest_key, &serde_json::to_vec(&manifest)?)
            .await?;
        head.runs.insert(
            run.run_id.clone(),
            CommittedRun {
                manifest_key,
                request_hash: request_hash.clone(),
            },
        );
        if store
            .compare_and_swap(
                HEAD_KEY,
                current.as_ref().map(|object| object.version.as_str()),
                &serde_json::to_vec(&head)?,
            )
            .await?
        {
            tracing::info!(
                run_id = run.run_id,
                stage = "committed",
                documents = manifest.counts.documents,
                unchanged = manifest.counts.unchanged,
                discoveries = manifest.counts.discoveries,
                failed_sources = manifest.counts.failed_sources,
                elapsed_ms = started.elapsed().as_millis() as u64
            );
            return Ok(manifest);
        }
    }
    Err(Error::Storage(
        "索引更新が8回競合しました。同じrun_idで再試行してください".into(),
    ))
}

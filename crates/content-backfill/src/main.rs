use clap::Parser;
use curatedz_core::{
    content::{CONTENT_HEAD_KEY, ContentHead, ContentStatus},
    hash,
    model::{
        ArticleQueueMessage, ArticleRequest, Document, HEAD_KEY, Head, Manifest, PROCESSING_VERSION,
    },
    store::{S3Store, Store},
};
use std::{collections::BTreeMap, process::ExitCode};

#[derive(Parser)]
struct Args {
    #[arg(long, env = "CURATEDZ_BUCKET")]
    bucket: String,
    #[arg(long, env = "CURATEDZ_ARTICLE_QUEUE_URL")]
    queue_url: String,
    #[arg(long)]
    dry_run: bool,
}

async fn load<T: serde::de::DeserializeOwned>(
    store: &dyn Store,
    key: &str,
) -> Result<T, Box<dyn std::error::Error>> {
    let object = store
        .get(key)
        .await?
        .ok_or_else(|| format!("S3 objectがありません: {key}"))?;
    Ok(serde_json::from_slice(&object.bytes)?)
}

async fn documents(
    store: &dyn Store,
    manifest: &Manifest,
) -> Result<Vec<Document>, Box<dyn std::error::Error>> {
    let key = manifest
        .objects
        .get("documents")
        .ok_or("manifestにdocumentsがありません")?;
    let object = store
        .get(key)
        .await?
        .ok_or_else(|| format!("S3 objectがありません: {key}"))?;
    object
        .bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| Ok(serde_json::from_slice(line)?))
        .collect()
}

async fn candidates(store: &dyn Store) -> Result<Vec<ArticleRequest>, Box<dyn std::error::Error>> {
    let head: Head = load(store, HEAD_KEY).await?;
    let content: ContentHead = match store.get(CONTENT_HEAD_KEY).await? {
        Some(object) => serde_json::from_slice(&object.bytes)?,
        None => ContentHead::default(),
    };
    let mut manifests = BTreeMap::new();
    for run in head.runs.values() {
        let manifest: Manifest = load(store, &run.manifest_key).await?;
        manifests.insert(manifest.generation, manifest);
    }
    let mut latest = BTreeMap::new();
    for manifest in manifests.values() {
        for document in documents(store, manifest).await? {
            latest.insert(document.document_id.clone(), document);
        }
    }
    let mut requests = Vec::new();
    for (article_id, document) in latest {
        let Some(requested_url) = document.canonical_url else {
            continue;
        };
        let already_done = content.records.get(&article_id).is_some_and(|entry| {
            entry.status == ContentStatus::Succeeded
                && entry.requested_url == requested_url
                && entry.processing_version == PROCESSING_VERSION
        });
        if !already_done {
            let source_id = head
                .documents
                .get(&article_id)
                .map(|entry| entry.source_id.clone())
                .unwrap_or_else(|| "backfill".into());
            requests.push(ArticleRequest {
                schema_version: 1,
                article_id,
                requested_url,
                source_id,
                processing_version: PROCESSING_VERSION.into(),
            });
        }
    }
    Ok(requests)
}

async fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let store = S3Store::new(args.bucket.clone()).await;
    let requests = candidates(&store).await?;
    println!("本文処理対象: {}件", requests.len());
    if args.dry_run || requests.is_empty() {
        return Ok(());
    }
    let bytes = requests
        .iter()
        .try_fold(Vec::new(), |mut output, request| {
            serde_json::to_writer(&mut output, request)?;
            output.push(b'\n');
            Ok::<_, serde_json::Error>(output)
        })?;
    let request_key = format!(
        "article-requests/v1/backfill-{}.jsonl",
        hash(&bytes).trim_start_matches("sha256:")
    );
    store.immutable(&request_key, &bytes).await?;
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let sqs = aws_sdk_sqs::Client::new(&config);
    for chunk in requests.chunks(10) {
        let mut entries = Vec::new();
        for (index, request) in chunk.iter().enumerate() {
            let message = ArticleQueueMessage {
                schema_version: 1,
                bucket: args.bucket.clone(),
                request_key: request_key.clone(),
                article_id: request.article_id.clone(),
                source_id: request.source_id.clone(),
                requested_url: request.requested_url.clone(),
                processing_version: request.processing_version.clone(),
            };
            entries.push(
                aws_sdk_sqs::types::SendMessageBatchRequestEntry::builder()
                    .id(format!("item-{index}"))
                    .message_body(serde_json::to_string(&message)?)
                    .build()?,
            );
        }
        let output = sqs
            .send_message_batch()
            .queue_url(&args.queue_url)
            .set_entries(Some(entries))
            .send()
            .await?;
        if !output.failed().is_empty() {
            return Err(format!(
                "SQS送信に{}件失敗しました。同じコマンドを再実行してください",
                output.failed().len()
            )
            .into());
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .init();
    match run(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(stage="content_backfill", error=%error);
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use curatedz_core::{
        content::{ContentIndexEntry, ContentStatus},
        model::{CommittedRun, Counts, IndexEntry},
        store::MemoryStore,
    };

    #[tokio::test]
    async fn finds_only_documents_without_current_success() {
        let store = MemoryStore::default();
        let seen = Utc.with_ymd_and_hms(2026, 11, 10, 0, 0, 0).unwrap();
        let document = Document {
            schema_version: 1,
            document_id: "sha256:a".into(),
            canonical_url: Some("https://example.com/a".into()),
            title: "A".into(),
            published_at: None,
            updated_at: None,
            authors: vec![],
            summary: None,
            content_hash: "sha256:c".into(),
        };
        store
            .immutable(
                "batches/v1/test/documents.jsonl",
                &[serde_json::to_vec(&document).unwrap(), b"\n".to_vec()].concat(),
            )
            .await
            .unwrap();
        let manifest = Manifest {
            schema_version: 1,
            normalizer_version: "1".into(),
            run_id: "run".into(),
            config_version: "config".into(),
            request_hash: "request".into(),
            replay_of: None,
            generation: 1,
            status: "success".into(),
            counts: Counts::default(),
            sources: vec![],
            objects: BTreeMap::from([(
                "documents".into(),
                "batches/v1/test/documents.jsonl".into(),
            )]),
        };
        store
            .immutable(
                "batches/v1/test/manifest.json",
                &serde_json::to_vec(&manifest).unwrap(),
            )
            .await
            .unwrap();
        let head = Head {
            generation: 1,
            documents: BTreeMap::from([(
                "sha256:a".into(),
                IndexEntry {
                    content_hash: "sha256:c".into(),
                    observed_at: seen,
                    source_id: "source".into(),
                    normalizer_revision: 1,
                },
            )]),
            runs: BTreeMap::from([(
                "run".into(),
                CommittedRun {
                    manifest_key: "batches/v1/test/manifest.json".into(),
                    request_hash: "request".into(),
                },
            )]),
        };
        store
            .immutable(HEAD_KEY, &serde_json::to_vec(&head).unwrap())
            .await
            .unwrap();
        assert_eq!(candidates(&store).await.unwrap().len(), 1);
        let content = ContentHead {
            generation: 1,
            records: BTreeMap::from([(
                "sha256:a".into(),
                ContentIndexEntry {
                    record_key: "record".into(),
                    requested_url: "https://example.com/a".into(),
                    processing_version: PROCESSING_VERSION.into(),
                    status: ContentStatus::Succeeded,
                    text_hash: Some("sha256:text".into()),
                },
            )]),
            commits: BTreeMap::new(),
        };
        store
            .immutable(CONTENT_HEAD_KEY, &serde_json::to_vec(&content).unwrap())
            .await
            .unwrap();
        assert!(candidates(&store).await.unwrap().is_empty());
    }
}

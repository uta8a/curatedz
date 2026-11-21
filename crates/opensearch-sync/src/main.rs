use clap::Parser;
use curatedz_core::{
    content::{CONTENT_HEAD_KEY, ContentCommit, ContentHead, ContentRecord},
    model::{Discovery, Document, HEAD_KEY, Head, Manifest},
    store::{S3Store, Store},
};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::ExitCode,
};
use tracing_subscriber::EnvFilter;

const ALIAS: &str = "curatedz-articles";

fn log_filter() -> EnvFilter {
    EnvFilter::new("info,aws_config=warn")
}

#[derive(Parser)]
struct Args {
    #[arg(long, env = "CURATEDZ_BUCKET")]
    bucket: String,
    #[arg(long, env = "CURATEDZ_OPENSEARCH_URL")]
    opensearch_url: String,
    #[arg(
        long,
        env = "CURATEDZ_CHECKPOINT",
        default_value = "/data/checkpoint.json"
    )]
    checkpoint: PathBuf,
    #[arg(long)]
    rebuild: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Checkpoint {
    rss_generation: u64,
    content_generation: u64,
    index: String,
}

async fn load<T: DeserializeOwned>(
    store: &dyn Store,
    key: &str,
) -> Result<T, Box<dyn std::error::Error>> {
    let object = store
        .get(key)
        .await?
        .ok_or_else(|| format!("S3 objectがありません: {key}"))?;
    Ok(serde_json::from_slice(&object.bytes)?)
}

async fn jsonl<T: DeserializeOwned>(
    store: &dyn Store,
    key: &str,
) -> Result<Vec<T>, Box<dyn std::error::Error>> {
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

fn read_checkpoint(path: &Path) -> Result<Checkpoint, Box<dyn std::error::Error>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Checkpoint::default()),
        Err(error) => Err(error.into()),
    }
}

fn write_checkpoint(
    path: &Path,
    checkpoint: &Checkpoint,
) -> Result<(), Box<dyn std::error::Error>> {
    let parent = path.parent().ok_or("checkpoint pathが不正です")?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(".checkpoint.tmp");
    std::fs::write(&temporary, serde_json::to_vec(checkpoint)?)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

async fn request(request: reqwest::RequestBuilder) -> Result<Value, Box<dyn std::error::Error>> {
    let response = request.send().await?;
    let status = response.status();
    let bytes = response.bytes().await?;
    if !status.is_success() {
        return Err(format!("OpenSearch HTTP {}", status.as_u16()).into());
    }
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

async fn create_index(
    client: &Client,
    base: &str,
    index: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mapping = json!({
        "settings": {"index": {"number_of_shards": 1, "number_of_replicas": 0}},
        "mappings": {"dynamic": "strict", "properties": {
            "schema_version": {"type": "integer"},
            "article_id": {"type": "keyword"},
            "canonical_url": {"type": "keyword", "index": false},
            "title": {"type": "text", "analyzer": "cjk"},
            "description": {"type": "text", "analyzer": "cjk"},
            "published_at": {"type": "date"},
            "updated_at": {"type": "date"},
            "first_seen_at": {"type": "date"},
            "last_seen_at": {"type": "date"},
            "source_ids": {"type": "keyword"},
            "source_types": {"type": "keyword"},
            "feed_urls": {"type": "keyword", "index": false},
            "original_urls": {"type": "keyword", "index": false},
            "content": {"type": "text", "analyzer": "cjk"},
            "content_status": {"type": "keyword"},
            "content_fetched_at": {"type": "date"},
            "processing_status": {"type": "keyword"},
            "processing_version": {"type": "keyword"}
        }}
    });
    request(client.put(format!("{base}/{index}")).json(&mapping)).await?;
    Ok(())
}

async fn bulk(client: &Client, base: &str, body: String) -> Result<(), Box<dyn std::error::Error>> {
    if body.is_empty() {
        return Ok(());
    }
    let lines: Vec<&str> = body.lines().collect();
    if !lines.len().is_multiple_of(2) {
        return Err("OpenSearch Bulk bodyが不正です".into());
    }
    let mut pending: Vec<String> = lines
        .chunks(2)
        .map(|pair| format!("{}\n{}\n", pair[0], pair[1]))
        .collect();
    for attempt in 0..3 {
        let request_body = pending.concat();
        let result = request(
            client
                .post(format!("{base}/_bulk"))
                .header("Content-Type", "application/x-ndjson")
                .body(request_body),
        )
        .await?;
        if result.get("errors").and_then(Value::as_bool) != Some(true) {
            return Ok(());
        }
        let items = result
            .get("items")
            .and_then(Value::as_array)
            .ok_or("OpenSearch Bulk応答にitemsがありません")?;
        if items.len() != pending.len() {
            return Err("OpenSearch Bulk応答のitem数が不正です".into());
        }
        let mut retry = Vec::new();
        for (operation, item) in pending.into_iter().zip(items) {
            let status = item
                .as_object()
                .and_then(|value| value.values().next())
                .and_then(|value| value.get("status"))
                .and_then(Value::as_u64)
                .ok_or("OpenSearch Bulk応答のstatusが不正です")?;
            if (200..300).contains(&status) {
                continue;
            }
            if status == 429 || status >= 500 {
                retry.push(operation);
            } else {
                return Err(format!("OpenSearch Bulk itemがHTTP {status}で失敗しました").into());
            }
        }
        if retry.is_empty() {
            return Ok(());
        }
        pending = retry;
        if attempt < 2 {
            tokio::time::sleep(std::time::Duration::from_secs(1 << attempt)).await;
        }
    }
    Err(format!("OpenSearch Bulkの{} itemが3回失敗しました", pending.len()).into())
}

fn document_bulk(index: &str, documents: Vec<Document>) -> Result<String, serde_json::Error> {
    let mut body = String::new();
    for document in documents {
        body.push_str(&serde_json::to_string(
            &json!({"update": {"_index": index, "_id": document.document_id}}),
        )?);
        body.push('\n');
        let document_fields = json!({
            "schema_version": 1, "article_id": document.document_id,
            "canonical_url": document.canonical_url, "title": document.title,
            "description": document.summary, "published_at": document.published_at,
            "updated_at": document.updated_at
        });
        let upsert = json!({
            "schema_version": 1, "article_id": document.document_id,
            "canonical_url": document.canonical_url, "title": document.title,
            "description": document.summary, "published_at": document.published_at,
            "updated_at": document.updated_at, "first_seen_at": Value::Null,
            "last_seen_at": Value::Null, "source_ids": [], "source_types": [],
            "feed_urls": [], "original_urls": []
        });
        body.push_str(&serde_json::to_string(&json!({
            "doc": document_fields, "upsert": upsert
        }))?);
        body.push('\n');
    }
    Ok(body)
}

fn discovery_bulk(index: &str, discoveries: Vec<Discovery>) -> Result<String, serde_json::Error> {
    let mut body = String::new();
    for discovery in discoveries {
        body.push_str(&serde_json::to_string(
            &json!({"update": {"_index": index, "_id": discovery.document_id}}),
        )?);
        body.push('\n');
        body.push_str(&serde_json::to_string(&json!({
            "script": {"lang": "painless", "source": "if (ctx._source.first_seen_at == null || params.seen.compareTo(ctx._source.first_seen_at) < 0) ctx._source.first_seen_at=params.seen; if (ctx._source.last_seen_at == null || params.seen.compareTo(ctx._source.last_seen_at) > 0) ctx._source.last_seen_at=params.seen; if (!ctx._source.source_ids.contains(params.source)) ctx._source.source_ids.add(params.source); if (!ctx._source.source_types.contains(params.source_type)) ctx._source.source_types.add(params.source_type); if (params.feed != null && !ctx._source.feed_urls.contains(params.feed)) ctx._source.feed_urls.add(params.feed); if (params.original != null && !ctx._source.original_urls.contains(params.original)) ctx._source.original_urls.add(params.original)",
                "params": {"seen": discovery.observed_at, "source": discovery.source_id, "source_type": discovery.source_type, "feed": discovery.feed_url, "original": discovery.original_url}},
            "scripted_upsert": false
        }))?);
        body.push('\n');
    }
    Ok(body)
}

async fn sync_rss(
    client: &Client,
    base: &str,
    index: &str,
    store: &dyn Store,
    checkpoint: &mut Checkpoint,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(head_object) = store.get(HEAD_KEY).await? else {
        return Ok(());
    };
    let head: Head = serde_json::from_slice(&head_object.bytes)?;
    let mut manifests = BTreeMap::new();
    for run in head.runs.values() {
        let manifest: Manifest = load(store, &run.manifest_key).await?;
        manifests.insert(manifest.generation, manifest);
    }
    for (generation, manifest) in manifests.range((checkpoint.rss_generation + 1)..) {
        let documents: Vec<Document> = jsonl(store, &manifest.objects["documents"]).await?;
        let discoveries: Vec<Discovery> = jsonl(store, &manifest.objects["discoveries"]).await?;
        let document_count = documents.len();
        let discovery_count = discoveries.len();
        bulk(client, base, document_bulk(index, documents)?).await?;
        bulk(client, base, discovery_bulk(index, discoveries)?).await?;
        checkpoint.rss_generation = *generation;
        write_checkpoint(path, checkpoint)?;
        tracing::info!(
            stage = "opensearch_rss_sync",
            generation,
            document_count,
            discovery_count,
            index
        );
    }
    Ok(())
}

async fn sync_content(
    client: &Client,
    base: &str,
    index: &str,
    store: &dyn Store,
    checkpoint: &mut Checkpoint,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(head_object) = store.get(CONTENT_HEAD_KEY).await? else {
        return Ok(());
    };
    let head: ContentHead = serde_json::from_slice(&head_object.bytes)?;
    for (generation, key) in head.commits.range((checkpoint.content_generation + 1)..) {
        let commit: ContentCommit = load(store, key).await?;
        let content_count = commit.entries.len();
        let mut body = String::new();
        for entry in commit.entries {
            let record: ContentRecord = load(store, &entry.record_key).await?;
            let text = match &record.text_key {
                Some(key) => Some(String::from_utf8(load_bytes(store, key).await?)?),
                None => None,
            };
            body.push_str(&serde_json::to_string(
                &json!({"update": {"_index": index, "_id": entry.article_id}}),
            )?);
            body.push('\n');
            body.push_str(&serde_json::to_string(&json!({"doc": {
                "content": text,
                "content_status": record.status,
                "content_fetched_at": record.fetched_at,
                "processing_status": record.status,
                "processing_version": record.processing_version
            }}))?);
            body.push('\n');
        }
        bulk(client, base, body).await?;
        checkpoint.content_generation = *generation;
        write_checkpoint(path, checkpoint)?;
        tracing::info!(
            stage = "opensearch_content_sync",
            generation,
            content_count,
            index
        );
    }
    Ok(())
}

async fn load_bytes(store: &dyn Store, key: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(store
        .get(key)
        .await?
        .ok_or_else(|| format!("S3 objectがありません: {key}"))?
        .bytes)
}

async fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let base = args.opensearch_url.trim_end_matches('/');
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let store = S3Store::new(args.bucket).await;
    let mut checkpoint = if args.rebuild {
        Checkpoint::default()
    } else {
        read_checkpoint(&args.checkpoint)?
    };
    let working_checkpoint = if args.rebuild {
        args.checkpoint.with_extension("rebuild.json")
    } else {
        args.checkpoint.clone()
    };
    let index = if args.rebuild || checkpoint.index.is_empty() {
        format!(
            "curatedz-articles-{}",
            chrono::Utc::now().format("%Y%m%d%H%M%S")
        )
    } else {
        checkpoint.index.clone()
    };
    let exists = client.head(format!("{base}/{index}")).send().await?;
    if exists.status() == StatusCode::NOT_FOUND {
        create_index(&client, base, &index).await?;
    } else if !exists.status().is_success() {
        return Err(format!("OpenSearch HTTP {}", exists.status()).into());
    }
    checkpoint.index = index.clone();
    sync_rss(
        &client,
        base,
        &index,
        &store,
        &mut checkpoint,
        &working_checkpoint,
    )
    .await?;
    sync_content(
        &client,
        base,
        &index,
        &store,
        &mut checkpoint,
        &working_checkpoint,
    )
    .await?;
    if args.rebuild {
        request(client.post(format!("{base}/_aliases")).json(&json!({"actions": [
            {"remove": {"index": "curatedz-articles-*", "alias": ALIAS, "must_exist": false}},
            {"add": {"index": index, "alias": ALIAS}}
        ]}))).await?;
    } else {
        let alias = client.head(format!("{base}/_alias/{ALIAS}")).send().await?;
        if alias.status() == StatusCode::NOT_FOUND {
            request(
                client
                    .post(format!("{base}/_aliases"))
                    .json(&json!({"actions": [{"add": {"index": index, "alias": ALIAS}}]})),
            )
            .await?;
        }
    }
    write_checkpoint(&args.checkpoint, &checkpoint)?;
    if args.rebuild {
        match std::fs::remove_file(&working_checkpoint) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    tracing::info!(
        stage = "opensearch_sync_complete",
        index,
        rss_generation = checkpoint.rss_generation,
        content_generation = checkpoint.content_generation,
        rebuild = args.rebuild
    );
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(log_filter())
        .with_writer(std::io::stderr)
        .init();
    match run(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(stage = "opensearch_sync", error = %error);
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        sync::{Arc, Mutex},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);

    struct LogWriter(LogBuffer);

    impl Write for LogWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for LogBuffer {
        type Writer = LogWriter;

        fn make_writer(&'a self) -> Self::Writer {
            LogWriter(self.clone())
        }
    }

    #[test]
    fn aws_credential_info_logs_are_suppressed() {
        let output = LogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_env_filter(log_filter())
            .with_writer(output.clone())
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                target: "aws_config::profile::credentials",
                creds = "temporary-credential-marker"
            );
            tracing::warn!(
                target: "aws_config::profile::credentials",
                message = "credential-warning-marker"
            );
        });

        let bytes = output.0.lock().unwrap().clone();
        let logs = String::from_utf8(bytes).unwrap();
        assert!(!logs.contains("temporary-credential-marker"));
        assert!(logs.contains("credential-warning-marker"));
    }

    #[test]
    fn document_update_preserves_content_and_discovery_aggregation() {
        let document = Document {
            schema_version: 1,
            document_id: "sha256:test".into(),
            canonical_url: Some("https://example.com/article".into()),
            title: "title".into(),
            published_at: None,
            updated_at: None,
            authors: Vec::new(),
            summary: None,
            content_hash: "sha256:content".into(),
        };
        let bulk = document_bulk("index", vec![document]).unwrap();
        let lines: Vec<Value> = bulk
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(lines[0].get("update").is_some());
        assert!(lines[1].pointer("/doc/content").is_none());
        assert!(lines[1].pointer("/doc/source_ids").is_none());
        assert!(lines[1].pointer("/doc/first_seen_at").is_none());
        assert!(lines[1].pointer("/upsert/article_id").is_some());
        assert_eq!(lines[1].pointer("/upsert/source_ids"), Some(&json!([])));
    }

    #[test]
    fn checkpoint_round_trip_is_atomic() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("checkpoint.json");
        let expected = Checkpoint {
            rss_generation: 3,
            content_generation: 4,
            index: "index".into(),
        };
        write_checkpoint(&path, &expected).unwrap();
        let actual = read_checkpoint(&path).unwrap();
        assert_eq!(actual.rss_generation, 3);
        assert_eq!(actual.content_generation, 4);
        assert_eq!(actual.index, "index");
    }

    #[tokio::test]
    async fn bulk_retries_only_transient_failed_items() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let responses = [
                json!({"errors": true, "items": [
                    {"update": {"status": 200}}, {"update": {"status": 429}}
                ]}),
                json!({"errors": false, "items": [{"update": {"status": 200}}]}),
            ];
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = vec![0; 8192];
                let size = stream.read(&mut bytes).await.unwrap();
                requests.push(String::from_utf8_lossy(&bytes[..size]).to_string());
                let body = serde_json::to_string(&response).unwrap();
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
            requests
        });
        let body = concat!(
            "{\"update\":{\"_index\":\"index\",\"_id\":\"one\"}}\n",
            "{\"doc\":{\"title\":\"one\"}}\n",
            "{\"update\":{\"_index\":\"index\",\"_id\":\"two\"}}\n",
            "{\"doc\":{\"title\":\"two\"}}\n"
        );
        bulk(&Client::new(), &base, body.into()).await.unwrap();
        let requests = server.await.unwrap();
        assert!(requests[0].contains("\"_id\":\"one\""));
        assert!(requests[0].contains("\"_id\":\"two\""));
        assert!(!requests[1].contains("\"_id\":\"one\""));
        assert!(requests[1].contains("\"_id\":\"two\""));
    }
}

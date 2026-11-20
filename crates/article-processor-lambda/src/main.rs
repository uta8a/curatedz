use chrono::Utc;
use curatedz_core::{
    Error,
    article_fetch::{fetch_html, fetch_robots, robots_allowed},
    content::{ContentFailure, ContentRecord, ContentStatus, extract, is_current_success, publish},
    model::{ArticleQueueMessage, ArticleRequest, PROCESSING_VERSION},
    store::{S3Store, Store},
};
use lambda_runtime::{Error as LambdaError, LambdaEvent, service_fn};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

type RobotsCache = Arc<Mutex<BTreeMap<String, (Instant, Option<String>)>>>;
type OriginThrottle = Arc<Mutex<BTreeMap<String, Instant>>>;

async fn throttle(cache: &OriginThrottle, article_url: &str) -> Result<(), Error> {
    const MIN_INTERVAL: Duration = Duration::from_secs(1);
    let url =
        url::Url::parse(article_url).map_err(|_| Error::Security("記事URLが不正です".into()))?;
    let origin = url.origin().ascii_serialization();
    let mut cache = cache.lock().await;
    if let Some(previous) = cache.get(&origin) {
        let wait = MIN_INTERVAL.saturating_sub(previous.elapsed());
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
    cache.insert(origin, Instant::now());
    Ok(())
}

async fn robots(
    cache: &RobotsCache,
    throttle_cache: &OriginThrottle,
    article_url: &str,
) -> Result<Option<String>, Error> {
    let url =
        url::Url::parse(article_url).map_err(|_| Error::Security("記事URLが不正です".into()))?;
    let origin = url.origin().ascii_serialization();
    if let Some((loaded, body)) = cache.lock().await.get(&origin)
        && loaded.elapsed() < Duration::from_secs(3600)
    {
        return Ok(body.clone());
    }
    throttle(throttle_cache, article_url).await?;
    let body = fetch_robots(article_url).await?;
    cache
        .lock()
        .await
        .insert(origin, (Instant::now(), body.clone()));
    Ok(body)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SqsRecord {
    message_id: String,
    body: String,
    #[serde(default)]
    attributes: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SqsEvent {
    records: Vec<SqsRecord>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchItemFailure {
    item_identifier: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchResponse {
    batch_item_failures: Vec<BatchItemFailure>,
}

async fn verify_request(store: &S3Store, message: &ArticleQueueMessage) -> Result<(), Error> {
    if !(message.request_key.starts_with("batches/v1/")
        || message.request_key.starts_with("article-requests/v1/"))
        || !message.request_key.ends_with(".jsonl")
    {
        return Err(Error::Security("記事処理要求のS3 keyが不正です".into()));
    }
    let object = store
        .get(&message.request_key)
        .await?
        .ok_or_else(|| Error::Security("記事処理要求のS3 objectがありません".into()))?;
    for line in object.bytes.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let request: ArticleRequest = serde_json::from_slice(line)?;
        if request.article_id == message.article_id
            && request.source_id == message.source_id
            && request.requested_url == message.requested_url
            && request.processing_version == message.processing_version
        {
            return Ok(());
        }
    }
    Err(Error::Security(
        "S3の公開済み要求とSQS messageが一致しません".into(),
    ))
}

fn failure_record(
    message: &ArticleQueueMessage,
    status: ContentStatus,
    error: &Error,
    attempt: u32,
) -> ContentRecord {
    ContentRecord {
        schema_version: 1,
        article_id: message.article_id.clone(),
        requested_url: message.requested_url.clone(),
        final_url: None,
        declared_canonical_url: None,
        raw_key: None,
        text_key: None,
        text_hash: None,
        status,
        fetched_at: Utc::now(),
        http_status: None,
        content_type: None,
        failure: Some(ContentFailure {
            code: match error {
                Error::Security(_) => "security_rejected",
                Error::Extraction(_) => "extraction_failed",
                _ => "fetch_failed",
            }
            .into(),
            message: error.to_string().chars().take(300).collect(),
        }),
        processing_version: message.processing_version.clone(),
        attempt,
    }
}

fn retryable(error: &Error) -> bool {
    let Error::Fetch(message) = error else {
        return false;
    };
    message.contains("タイムアウト")
        || message.contains("接続失敗")
        || message.contains("解決")
        || message.contains("読み取り")
        || message.contains("HTTP 429")
        || (500..=599).any(|status| message.contains(&format!("HTTP {status}")))
}

async fn process(
    store: &S3Store,
    bucket: &str,
    body: &str,
    attempt: u32,
    robots_cache: &RobotsCache,
    throttle_cache: &OriginThrottle,
) -> Result<ContentStatus, Error> {
    let message: ArticleQueueMessage = serde_json::from_str(body)?;
    if message.schema_version != 1
        || message.processing_version != PROCESSING_VERSION
        || message.bucket != bucket
    {
        return Err(Error::Security("記事処理要求の契約が不正です".into()));
    }
    verify_request(store, &message).await?;
    if is_current_success(
        store,
        &message.article_id,
        &message.requested_url,
        &message.processing_version,
    )
    .await?
    {
        return Ok(ContentStatus::Succeeded);
    }
    if let Some(robots) = robots(robots_cache, throttle_cache, &message.requested_url).await? {
        let path = url::Url::parse(&message.requested_url)
            .map_err(|_| Error::Security("記事URLが不正です".into()))?
            .path()
            .to_string();
        if !robots_allowed(&robots, &path) {
            let error = Error::Security("robots.txtにより本文取得を禁止されています".into());
            publish(
                store,
                failure_record(&message, ContentStatus::Skipped, &error, attempt),
                None,
                None,
            )
            .await?;
            return Ok(ContentStatus::Skipped);
        }
    }
    throttle(throttle_cache, &message.requested_url).await?;
    let response = match fetch_html(&message.requested_url).await {
        Ok(response) => response,
        Err(error @ Error::Security(_)) => {
            publish(
                store,
                failure_record(&message, ContentStatus::Skipped, &error, attempt),
                None,
                None,
            )
            .await?;
            return Ok(ContentStatus::Skipped);
        }
        Err(error) => {
            publish(
                store,
                failure_record(&message, ContentStatus::FetchFailed, &error, attempt),
                None,
                None,
            )
            .await?;
            if retryable(&error) {
                return Err(error);
            }
            return Ok(ContentStatus::FetchFailed);
        }
    };
    let html = String::from_utf8_lossy(&response.bytes);
    let extracted = match extract(&html, &response.final_url) {
        Ok(extracted) => extracted,
        Err(error) => {
            let mut record =
                failure_record(&message, ContentStatus::ExtractionFailed, &error, attempt);
            record.final_url = Some(response.final_url);
            record.http_status = Some(response.status);
            record.content_type = response.content_type;
            publish(store, record, Some(&response.bytes), None).await?;
            return Ok(ContentStatus::ExtractionFailed);
        }
    };
    let record = ContentRecord {
        schema_version: 1,
        article_id: message.article_id,
        requested_url: message.requested_url,
        final_url: Some(response.final_url),
        declared_canonical_url: extracted.declared_canonical_url,
        raw_key: None,
        text_key: None,
        text_hash: None,
        status: ContentStatus::Succeeded,
        fetched_at: Utc::now(),
        http_status: Some(response.status),
        content_type: response.content_type,
        failure: None,
        processing_version: message.processing_version,
        attempt,
    };
    publish(store, record, Some(&response.bytes), Some(&extracted.text)).await?;
    Ok(ContentStatus::Succeeded)
}

#[tokio::main]
async fn main() -> Result<(), LambdaError> {
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .init();
    let bucket = std::env::var("CURATEDZ_BUCKET")?;
    let store = S3Store::new(bucket.clone()).await;
    let robots_cache: RobotsCache = Arc::new(Mutex::new(BTreeMap::new()));
    let throttle_cache: OriginThrottle = Arc::new(Mutex::new(BTreeMap::new()));
    lambda_runtime::run(service_fn(|event: LambdaEvent<SqsEvent>| {
        let store = &store;
        let bucket = &bucket;
        let robots_cache = &robots_cache;
        let throttle_cache = &throttle_cache;
        async move {
            let mut failures = Vec::new();
            for record in event.payload.records {
                let message = serde_json::from_str::<ArticleQueueMessage>(&record.body).ok();
                let article_id = message.as_ref().map(|value| value.article_id.as_str()).unwrap_or("invalid");
                let source_id = message.as_ref().map(|value| value.source_id.as_str()).unwrap_or("invalid");
                let attempt = record
                    .attributes
                    .get("ApproximateReceiveCount")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(1);
                match process(
                    store,
                    bucket,
                    &record.body,
                    attempt,
                    robots_cache,
                    throttle_cache,
                )
                .await
                {
                    Ok(status) => tracing::info!(message_id = record.message_id, article_id, source_id, stage = "article_processed", status = ?status),
                    Err(error) => {
                        tracing::error!(message_id = record.message_id, article_id, source_id, stage = "article_failed", error = %error);
                        failures.push(BatchItemFailure { item_identifier: record.message_id });
                    }
                }
            }
            Ok::<_, LambdaError>(BatchResponse { batch_item_failures: failures })
        }
    }))
    .await
}

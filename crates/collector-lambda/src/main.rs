use chrono::{DateTime, Utc};
use curatedz_core::{
    config::Config,
    fetch::HttpFetcher,
    id,
    model::{ArticleQueueMessage, Manifest},
    pipeline::{Run, article_requests, execute},
    store::{S3Store, Store},
};
use lambda_runtime::{Error, LambdaEvent, service_fn};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    schedule_arn: String,
    scheduled_time: DateTime<Utc>,
}

async fn enqueue(
    client: &aws_sdk_sqs::Client,
    queue_url: &str,
    bucket: &str,
    store: &S3Store,
    manifest: &Manifest,
) -> Result<usize, Error> {
    let (request_key, requests) = article_requests(store, manifest).await?;
    let mut sent = 0;
    for chunk in requests.chunks(10) {
        let mut entries = Vec::with_capacity(chunk.len());
        for (index, request) in chunk.iter().enumerate() {
            let message = ArticleQueueMessage {
                schema_version: 1,
                bucket: bucket.into(),
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
        let output = client
            .send_message_batch()
            .queue_url(queue_url)
            .set_entries(Some(entries))
            .send()
            .await?;
        if !output.failed().is_empty() {
            return Err(format!(
                "記事処理要求の送信に{}件失敗しました。同じrunを再実行してください",
                output.failed().len()
            )
            .into());
        }
        sent += output.successful().len();
    }
    Ok(sent)
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .init();
    let bucket = std::env::var("CURATEDZ_BUCKET")?;
    let config_key = std::env::var("CURATEDZ_CONFIG_KEY")?;
    let queue_url = std::env::var("CURATEDZ_ARTICLE_QUEUE_URL")?;
    let aws_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let sqs = aws_sdk_sqs::Client::new(&aws_config);
    let store = S3Store::new(bucket.clone()).await;
    let config_object = store
        .get(&config_key)
        .await?
        .ok_or_else(|| format!("S3に設定がありません: {config_key}"))?;
    let config_yaml = std::str::from_utf8(&config_object.bytes)
        .map_err(|_| "S3の設定はUTF-8 YAMLである必要があります")?;
    let config = Config::parse(config_yaml)?;
    let fetcher = HttpFetcher::new(config.http.clone())?;
    lambda_runtime::run(service_fn(|event: LambdaEvent<Event>| {
        let config = &config;
        let store = &store;
        let fetcher = &fetcher;
        let sqs = &sqs;
        let bucket = &bucket;
        let queue_url = &queue_url;
        async move {
            let scheduled = event.payload.scheduled_time.to_rfc3339();
            let run_id =
                id(&["schedule", &event.payload.schedule_arn, &scheduled]).replace(':', "-");
            let run = Run {
                run_id,
                topics: vec![],
                replay_of: None,
                observed_at: event.payload.scheduled_time,
            };
            let result = execute(config, &run, fetcher, store).await?;
            if result.status == "failed" {
                return Err::<serde_json::Value, Error>(
                    "全情報源の収集に失敗しました。新しいrunで復旧してください".into(),
                );
            }
            let queued = enqueue(sqs, queue_url, bucket, store, &result).await?;
            tracing::info!(
                run_id = run.run_id,
                stage = "article_requests_queued",
                queued
            );
            Ok(serde_json::to_value(result)?)
        }
    }))
    .await
}

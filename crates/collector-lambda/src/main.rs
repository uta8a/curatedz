use chrono::{DateTime, Utc};
use curatedz_core::{
    config::Config,
    fetch::HttpFetcher,
    id,
    pipeline::{Run, execute},
    store::S3Store,
};
use lambda_runtime::{Error, LambdaEvent, service_fn};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    schedule_arn: String,
    scheduled_time: DateTime<Utc>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .init();
    let config = Config::parse(include_str!("../../../config/topics.yaml"))?;
    let store = S3Store::new(std::env::var("CURATEDZ_BUCKET")?).await;
    let fetcher = HttpFetcher::new(config.http.clone())?;
    lambda_runtime::run(service_fn(|event: LambdaEvent<Event>| {
        let config = &config;
        let store = &store;
        let fetcher = &fetcher;
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
            Ok(serde_json::to_value(result)?)
        }
    }))
    .await
}

use chrono::Utc;
use clap::Parser;
use curatedz_core::{
    config::Config,
    fetch::{Fetcher, FixtureFetcher, HttpFetcher},
    pipeline::{Run, execute},
    store::{LocalStore, MemoryStore, Store},
};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(version, about = "テーマを指定してRSS/Atomを収集・再処理します")]
struct Args {
    #[arg(long, default_value = "config/topics.yaml")]
    config: PathBuf,
    #[arg(long)]
    topic: Vec<String>,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long, default_value = "output")]
    output: PathBuf,
    #[arg(long, conflicts_with_all = ["fixtures", "dry_run"])]
    bucket: Option<String>,
    #[arg(long)]
    fixtures: Option<PathBuf>,
    #[arg(long, conflicts_with = "replay")]
    dry_run: bool,
    #[arg(long, conflicts_with = "fixtures")]
    replay: Option<String>,
    #[arg(long)]
    validate: bool,
}

async fn run(args: Args) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let config = Config::parse(&std::fs::read_to_string(args.config)?)?;
    config.selection(&args.topic)?;
    if args.validate {
        println!("設定は有効です: {}", config.version()?);
        return Ok(ExitCode::SUCCESS);
    }
    let fetcher: Box<dyn Fetcher> = match args.fixtures {
        Some(path) => Box::new(FixtureFetcher(path)),
        None => Box::new(HttpFetcher::new(config.http.clone())?),
    };
    let store: Box<dyn Store> = if args.dry_run {
        Box::<MemoryStore>::default()
    } else if let Some(bucket) = args.bucket {
        #[cfg(feature = "aws")]
        {
            Box::new(curatedz_core::store::S3Store::new(bucket).await)
        }
        #[cfg(not(feature = "aws"))]
        {
            let _ = bucket;
            return Err("S3出力には --features aws でビルドしてください".into());
        }
    } else {
        Box::new(LocalStore::new(args.output)?)
    };
    let now = Utc::now();
    let run = Run {
        run_id: args
            .run_id
            .unwrap_or_else(|| format!("local-{}", now.format("%Y%m%dT%H%M%S%9f"))),
        topics: args.topic,
        replay_of: args.replay,
        observed_at: now,
    };
    let manifest = execute(&config, &run, fetcher.as_ref(), store.as_ref()).await?;
    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(match manifest.status.as_str() {
        "success" => ExitCode::SUCCESS,
        "partial" => ExitCode::from(2),
        _ => ExitCode::from(1),
    })
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .init();
    match run(Args::parse()).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

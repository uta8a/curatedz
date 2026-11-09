use crate::{
    Error, Result,
    config::{HttpConfig, Source},
};
use async_trait::async_trait;
use std::{path::PathBuf, time::Duration};

pub struct Response {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
}

#[async_trait]
pub trait Fetcher: Send + Sync {
    async fn fetch(&self, source: &Source) -> Result<Response>;
}

pub struct HttpFetcher {
    client: reqwest::Client,
    config: HttpConfig,
}

impl HttpFetcher {
    pub fn new(config: HttpConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::limited(3))
            .user_agent("curatedz/0.1 RSS collector")
            .build()
            .map_err(|_| Error::Fetch("HTTPクライアントを初期化できません".into()))?;
        Ok(Self { client, config })
    }

    async fn attempt(&self, source: &Source) -> std::result::Result<Response, (String, bool, u64)> {
        let mut response = self
            .client
            .get(&source.url)
            .header("Accept-Encoding", "identity")
            .send()
            .await
            .map_err(|error| {
                let message = if error.is_timeout() {
                    "HTTPタイムアウト"
                } else {
                    "HTTP接続失敗"
                };
                (message.into(), true, 1)
            })?;
        let status = response.status();
        if !status.is_success() {
            let delay = response
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(1);
            return Err((
                format!("HTTP {}", status.as_u16()),
                (status.as_u16() == 429 || status.is_server_error()) && delay <= 5,
                delay,
            ));
        }
        if response
            .content_length()
            .is_some_and(|size| size > self.config.max_bytes as u64)
        {
            return Err(("HTTP応答がサイズ上限を超えています".into(), false, 0));
        }
        let content_type = response
            .headers()
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.chars().take(200).collect());
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ("HTTP応答の読み取り失敗".into(), true, 1))?
        {
            if bytes.len() + chunk.len() > self.config.max_bytes {
                return Err(("HTTP応答がサイズ上限を超えています".into(), false, 0));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Response {
            bytes,
            content_type,
        })
    }
}

#[async_trait]
impl Fetcher for HttpFetcher {
    async fn fetch(&self, source: &Source) -> Result<Response> {
        for attempt in 0..self.config.attempts {
            match self.attempt(source).await {
                Ok(response) => return Ok(response),
                Err((message, retry, delay)) => {
                    if !retry || attempt + 1 == self.config.attempts {
                        return Err(Error::Fetch(message));
                    }
                    tracing::warn!(
                        source_id = source.id,
                        stage = "fetch_retry",
                        attempt = attempt + 1,
                        error = message
                    );
                    tokio::time::sleep(Duration::from_secs(delay * (attempt as u64 + 1))).await;
                }
            }
        }
        Err(Error::Fetch("取得試行回数が不正です".into()))
    }
}

pub struct FixtureFetcher(pub PathBuf);

#[async_trait]
impl Fetcher for FixtureFetcher {
    async fn fetch(&self, source: &Source) -> Result<Response> {
        let path = self.0.join(format!("{}.xml", source.id));
        let bytes = std::fs::read(path).map_err(|_| Error::Fetch("fixtureを読めません".into()))?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(Error::Fetch("fixtureが大きすぎます".into()));
        }
        Ok(Response {
            bytes,
            content_type: Some("application/xml".into()),
        })
    }
}

use super::{MAX_OBJECT_BYTES, Object, Store, check_size, safe_key};
use crate::{Error, Result};
use async_trait::async_trait;
use aws_sdk_s3::{Client, primitives::ByteStream};

pub struct S3Store {
    pub(super) client: Client,
    pub(super) bucket: String,
}

impl S3Store {
    pub async fn new(bucket: String) -> Self {
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .timeout_config(
                aws_config::timeout::TimeoutConfig::builder()
                    .operation_timeout(std::time::Duration::from_secs(30))
                    .build(),
            )
            .retry_config(aws_config::retry::RetryConfig::standard().with_max_attempts(3))
            .load()
            .await;
        Self {
            client: Client::new(&config),
            bucket,
        }
    }
}

#[async_trait]
impl Store for S3Store {
    async fn get(&self, key: &str) -> Result<Option<Object>> {
        safe_key(key)?;
        let mut response = match self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) if error.as_service_error().is_some_and(|e| e.is_no_such_key()) => {
                return Ok(None);
            }
            Err(_) => return Err(Error::Storage(format!("S3 GetObjectに失敗しました: {key}"))),
        };
        let version = response
            .e_tag()
            .ok_or_else(|| Error::Storage("S3応答にETagがありません".into()))?
            .to_string();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.body.next().await {
            let chunk =
                chunk.map_err(|_| Error::Storage("S3応答の読み取りに失敗しました".into()))?;
            if bytes.len() + chunk.len() > MAX_OBJECT_BYTES {
                return Err(Error::Storage("S3オブジェクトが上限を超えています".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Some(Object { bytes, version }))
    }

    async fn compare_and_swap(
        &self,
        key: &str,
        expected: Option<&str>,
        bytes: &[u8],
    ) -> Result<bool> {
        safe_key(key)?;
        check_size(bytes)?;
        let request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(ByteStream::from(bytes.to_vec()));
        let request = match expected {
            Some(etag) => request.if_match(etag),
            None => request.if_none_match("*"),
        };
        match request.send().await {
            Ok(_) => Ok(true),
            Err(error)
                if error
                    .raw_response()
                    .is_some_and(|r| matches!(r.status().as_u16(), 409 | 412)) =>
            {
                Ok(false)
            }
            Err(_) => Err(Error::Storage(format!("S3 PutObjectに失敗しました: {key}"))),
        }
    }
}

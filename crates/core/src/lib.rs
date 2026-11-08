pub mod config;
pub mod fetch;
pub mod model;
pub mod normalize;
pub mod pipeline;
pub mod store;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("設定エラー: {0}")]
    Config(String),
    #[error("保存エラー: {0}")]
    Storage(String),
    #[error("取得エラー: {0}")]
    Fetch(String),
    #[error("正規化エラー: {0}")]
    Normalize(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}

pub fn id(parts: &[&str]) -> String {
    hash(&serde_json::to_vec(parts).expect("文字列配列はJSONに変換可能"))
}

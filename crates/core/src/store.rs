use crate::{Error, Result, hash};
use async_trait::async_trait;
use fs2::FileExt;
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub const MAX_OBJECT_BYTES: usize = 16 * 1024 * 1024;

pub struct Object {
    pub bytes: Vec<u8>,
    pub version: String,
}

#[async_trait]
pub trait Store: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<Object>>;
    async fn compare_and_swap(
        &self,
        key: &str,
        expected: Option<&str>,
        bytes: &[u8],
    ) -> Result<bool>;

    async fn immutable(&self, key: &str, bytes: &[u8]) -> Result<()> {
        check_size(bytes)?;
        if self.compare_and_swap(key, None, bytes).await? {
            return Ok(());
        }
        match self.get(key).await? {
            Some(existing) if existing.bytes == bytes => Ok(()),
            _ => Err(Error::Storage(format!(
                "不変オブジェクトが競合しました: {key}"
            ))),
        }
    }
}

pub fn check_size(bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_OBJECT_BYTES {
        return Err(Error::Storage("16MiB上限を超えています".into()));
    }
    Ok(())
}

pub fn safe_key(key: &str) -> Result<()> {
    if key.is_empty()
        || key.contains('\\')
        || key
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error::Storage("不正なオブジェクトキー".into()));
    }
    Ok(())
}

pub struct LocalStore {
    root: PathBuf,
}

impl LocalStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    fn lock(&self) -> Result<File> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join(".store.lock"))?;
        FileExt::lock_exclusive(&lock)?;
        Ok(lock)
    }

    fn read(&self, path: &Path) -> Result<Option<Object>> {
        match std::fs::metadata(path) {
            Ok(meta) if meta.len() > MAX_OBJECT_BYTES as u64 => Err(Error::Storage(
                "保存済みオブジェクトが上限を超えています".into(),
            )),
            Ok(_) => {
                let bytes = std::fs::read(path)?;
                Ok(Some(Object {
                    version: hash(&bytes),
                    bytes,
                }))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

#[async_trait]
impl Store for LocalStore {
    async fn get(&self, key: &str) -> Result<Option<Object>> {
        safe_key(key)?;
        let _lock = self.lock()?;
        self.read(&self.root.join(key))
    }

    async fn compare_and_swap(
        &self,
        key: &str,
        expected: Option<&str>,
        bytes: &[u8],
    ) -> Result<bool> {
        safe_key(key)?;
        check_size(bytes)?;
        let _lock = self.lock()?;
        let path = self.root.join(key);
        if self
            .read(&path)?
            .as_ref()
            .map(|object| object.version.as_str())
            != expected
        {
            return Ok(false);
        }
        let parent = path
            .parent()
            .ok_or_else(|| Error::Storage("保存先が不正です".into()))?;
        std::fs::create_dir_all(parent)?;
        let temporary = parent.join(format!(
            ".tmp-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, &path)?;
        File::open(parent)?.sync_all()?;
        Ok(true)
    }
}

#[derive(Default)]
pub struct MemoryStore(Mutex<BTreeMap<String, Vec<u8>>>);

#[async_trait]
impl Store for MemoryStore {
    async fn get(&self, key: &str) -> Result<Option<Object>> {
        safe_key(key)?;
        Ok(self
            .0
            .lock()
            .expect("memory store lock")
            .get(key)
            .map(|bytes| Object {
                bytes: bytes.clone(),
                version: hash(bytes),
            }))
    }

    async fn compare_and_swap(
        &self,
        key: &str,
        expected: Option<&str>,
        bytes: &[u8],
    ) -> Result<bool> {
        safe_key(key)?;
        check_size(bytes)?;
        let mut data = self.0.lock().expect("memory store lock");
        if data.get(key).map(|bytes| hash(bytes)).as_deref() != expected {
            return Ok(false);
        }
        data.insert(key.to_string(), bytes.to_vec());
        Ok(true)
    }
}

#[cfg(feature = "aws")]
mod s3;
#[cfg(feature = "aws")]
pub use s3::S3Store;

#[cfg(all(test, feature = "aws"))]
mod s3_tests;

use async_trait::async_trait;
use chrono::{Duration, TimeZone, Utc};
use curatedz_core::{
    Error, Result,
    config::Config,
    fetch::{Fetcher, Response},
    model::{HEAD_KEY, Head, Manifest},
    normalize,
    pipeline::{Run, execute},
    store::{LocalStore, MemoryStore, Object, Store},
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const RSS: &[u8] = include_bytes!("../../../fixtures/rust-blog.xml");

fn config() -> Config {
    Config::parse("schema_version: 1\nsources:\n  - {id: rss, url: 'https://example.com/feed'}\ntopics:\n  - {id: rust, name: Rust, sources: [rss]}\n  - {id: tools, name: Tools, sources: [rss]}\n").unwrap()
}

fn run(id: &str) -> Run {
    Run {
        run_id: id.into(),
        topics: vec![],
        replay_of: None,
        observed_at: Utc.with_ymd_and_hms(2026, 11, 9, 0, 0, 0).unwrap(),
    }
}

struct FakeFetcher {
    bytes: Vec<u8>,
    calls: AtomicUsize,
}
impl FakeFetcher {
    fn new(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.to_vec(),
            calls: AtomicUsize::new(0),
        }
    }
}
#[async_trait]
impl Fetcher for FakeFetcher {
    async fn fetch(&self, source: &curatedz_core::config::Source) -> Result<Response> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if source.id == "broken" {
            return Err(Error::Fetch("想定した失敗".into()));
        }
        Ok(Response {
            bytes: self.bytes.clone(),
            content_type: None,
        })
    }
}

#[test]
fn config_rejects_unknown_fields_references_and_duplicates() {
    let yaml = serde_yaml_ng::to_string(&config()).unwrap();
    assert!(Config::parse(&(yaml.clone() + "typo: true\n")).is_err());
    assert!(Config::parse(&yaml.replace("sources:\n  - rss", "sources:\n  - missing")).is_err());
    let mut config = config();
    config.sources.push(config.sources[0].clone());
    assert!(config.validate().is_err());
}

#[test]
fn normalization_handles_rss_atom_missing_date_and_encoding() {
    let rss = normalize::parse(RSS, "rss", "https://example.com/feed").unwrap();
    assert_eq!(rss[0].document.title, "Rust & tools");
    assert_eq!(
        rss[0].document.document_id,
        "sha256:23054c70b10ed11e30a66881786fb389449f0f4d74ad64e2426b8418eae4b421"
    );
    assert_eq!(
        rss[0].document.canonical_url.as_deref(),
        Some("https://example.com/post?x=1")
    );
    assert!(rss[1].document.published_at.is_none());
    let atom = normalize::parse(
        include_bytes!("../../../fixtures/atom.xml"),
        "atom",
        "https://example.com/feed",
    )
    .unwrap();
    assert_eq!(
        atom[0].document.canonical_url.as_deref(),
        Some("https://example.com/atom-post")
    );
    assert_eq!(atom[0].document.authors, ["Alice"]);
    assert!(atom[1].document.canonical_url.is_none());
    let latin = normalize::parse(
        include_bytes!("../../../fixtures/latin1.xml"),
        "latin",
        "https://example.com/feed",
    )
    .unwrap();
    assert_eq!(latin[0].document.title, "Café & thé");
    assert!(normalize::parse(b"not xml", "bad", "https://example.com/feed").is_err());
}

#[test]
fn document_identity_survives_title_date_and_source_changes() {
    let a = normalize::parse(RSS, "a", "https://example.com/feed").unwrap();
    let changed = String::from_utf8(RSS.to_vec())
        .unwrap()
        .replace("Rust &amp; tools", "Updated title")
        .replace("02 Nov", "03 Nov");
    let b = normalize::parse(changed.as_bytes(), "b", "https://example.com/feed").unwrap();
    assert_eq!(a[0].document.document_id, b[0].document.document_id);
    assert_ne!(a[0].document.content_hash, b[0].document.content_hash);
}

#[tokio::test]
async fn same_run_skips_fetch_and_next_day_keeps_discovery_and_topics() {
    let store = MemoryStore::default();
    let fetcher = FakeFetcher::new(RSS);
    let first = execute(&config(), &run("one"), &fetcher, &store)
        .await
        .unwrap();
    assert_eq!(first.counts.documents, 2);
    assert_eq!(first.counts.associations, 4);
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 1);
    let again = execute(&config(), &run("one"), &fetcher, &store)
        .await
        .unwrap();
    assert_eq!(again.generation, first.generation);
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 1);
    let mut tomorrow = run("two");
    tomorrow.observed_at += Duration::days(1);
    let next = execute(&config(), &tomorrow, &fetcher, &store)
        .await
        .unwrap();
    assert_eq!(next.counts.documents, 0);
    assert_eq!(next.counts.discoveries, 2);
    assert_eq!(next.counts.associations, 4);
    let mut different = run("one");
    different.topics.push("rust".into());
    assert!(
        execute(&config(), &different, &fetcher, &store)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn updates_and_replay_do_not_regress_newer_documents() {
    let store = MemoryStore::default();
    let fetcher = FakeFetcher::new(RSS);
    execute(&config(), &run("one"), &fetcher, &store)
        .await
        .unwrap();
    let updated = String::from_utf8(RSS.to_vec())
        .unwrap()
        .replace("Rust &amp; tools", "New title");
    let mut next = run("two");
    next.observed_at += Duration::days(1);
    assert_eq!(
        execute(
            &config(),
            &next,
            &FakeFetcher::new(updated.as_bytes()),
            &store
        )
        .await
        .unwrap()
        .counts
        .documents,
        1
    );
    let mut replay = run("replay");
    replay.replay_of = Some("one".into());
    let result = execute(&config(), &replay, &fetcher, &store).await.unwrap();
    assert_eq!(result.counts.documents, 0);
    assert_eq!(result.counts.discoveries, 2);
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn partial_success_and_raw_survive_parse_failure() {
    let mut cfg = config();
    cfg.sources.push(curatedz_core::config::Source {
        id: "broken".into(),
        url: "https://example.com/bad".into(),
    });
    cfg.topics[0].sources.push("broken".into());
    let store = MemoryStore::default();
    let result = execute(&cfg, &run("partial"), &FakeFetcher::new(RSS), &store)
        .await
        .unwrap();
    assert_eq!(result.status, "partial");
    assert_eq!(result.counts.failed_sources, 1);
    let failed = execute(
        &config(),
        &run("badxml"),
        &FakeFetcher::new(b"broken xml"),
        &store,
    )
    .await
    .unwrap();
    assert_eq!(failed.status, "failed");
    let raw = store
        .get("raw/v1/run=badxml/source=rss/capture.json")
        .await
        .unwrap()
        .unwrap();
    let capture: curatedz_core::model::Capture = serde_json::from_slice(&raw.bytes).unwrap();
    assert!(
        store
            .get(capture.raw_key.as_ref().unwrap())
            .await
            .unwrap()
            .is_some()
    );
}

struct InterruptStore {
    inner: MemoryStore,
    fail_manifest: AtomicBool,
    conflict: AtomicBool,
}

struct LostAckStore {
    inner: MemoryStore,
    lose_ack: AtomicBool,
}

#[async_trait]
impl Store for LostAckStore {
    async fn get(&self, key: &str) -> Result<Option<Object>> {
        self.inner.get(key).await
    }
    async fn compare_and_swap(
        &self,
        key: &str,
        expected: Option<&str>,
        bytes: &[u8],
    ) -> Result<bool> {
        let result = self.inner.compare_and_swap(key, expected, bytes).await?;
        if key == HEAD_KEY && self.lose_ack.swap(false, Ordering::SeqCst) {
            return Err(Error::Storage("公開成功後に応答が失われました".into()));
        }
        Ok(result)
    }
}

#[tokio::test]
async fn lost_commit_ack_is_recovered_without_duplicate_generation() {
    let store = LostAckStore {
        inner: MemoryStore::default(),
        lose_ack: AtomicBool::new(true),
    };
    let fetcher = FakeFetcher::new(RSS);
    assert!(
        execute(&config(), &run("lost-ack"), &fetcher, &store)
            .await
            .is_err()
    );
    let result = execute(&config(), &run("lost-ack"), &fetcher, &store)
        .await
        .unwrap();
    assert_eq!(result.generation, 1);
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn local_store_allows_only_one_concurrent_creator() {
    let directory = tempfile::tempdir().unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let path = directory.path().to_path_buf();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let store = LocalStore::new(path).unwrap();
                barrier.wait();
                tokio::runtime::Runtime::new()
                    .unwrap()
                    .block_on(store.compare_and_swap("state/head", None, b"created"))
                    .unwrap()
            })
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap() as usize)
            .sum::<usize>(),
        1
    );
}

#[tokio::test]
async fn multiple_sources_share_one_document_and_keep_provenance() {
    let mut cfg = config();
    cfg.sources.push(curatedz_core::config::Source {
        id: "mirror".into(),
        url: "https://example.com/mirror".into(),
    });
    cfg.topics[0].sources.push("mirror".into());
    let fetcher = FakeFetcher::new(RSS);
    let result = execute(&cfg, &run("multiple"), &fetcher, &MemoryStore::default())
        .await
        .unwrap();
    assert_eq!(result.counts.documents, 2);
    assert_eq!(result.counts.discoveries, 4);
    assert_eq!(result.counts.associations, 6);
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 2);
}

#[derive(Default)]
struct YieldStore(MemoryStore);

#[async_trait]
impl Store for YieldStore {
    async fn get(&self, key: &str) -> Result<Option<Object>> {
        let result = self.0.get(key).await;
        if key == HEAD_KEY {
            tokio::task::yield_now().await;
        }
        result
    }
    async fn compare_and_swap(
        &self,
        key: &str,
        expected: Option<&str>,
        bytes: &[u8],
    ) -> Result<bool> {
        self.0.compare_and_swap(key, expected, bytes).await
    }
}

#[tokio::test]
async fn concurrent_runs_preserve_both_commits_and_deduplicate() {
    let store = YieldStore::default();
    let fetcher = FakeFetcher::new(RSS);
    let cfg = config();
    let a = run("concurrent-a");
    let b = run("concurrent-b");
    let (a, b) = tokio::join!(
        execute(&cfg, &a, &fetcher, &store),
        execute(&cfg, &b, &fetcher, &store)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.counts.documents + b.counts.documents, 2);
    assert_eq!(a.counts.discoveries + b.counts.discoveries, 4);
    let head: Head =
        serde_json::from_slice(&store.get(HEAD_KEY).await.unwrap().unwrap().bytes).unwrap();
    assert_eq!(head.generation, 2);
    assert_eq!(head.runs.len(), 2);
}
#[async_trait]
impl Store for InterruptStore {
    async fn get(&self, key: &str) -> Result<Option<Object>> {
        self.inner.get(key).await
    }
    async fn compare_and_swap(
        &self,
        key: &str,
        expected: Option<&str>,
        bytes: &[u8],
    ) -> Result<bool> {
        if key.ends_with("manifest.json") && self.fail_manifest.swap(false, Ordering::SeqCst) {
            return Err(Error::Storage("想定した保存中断".into()));
        }
        if key == HEAD_KEY && self.conflict.swap(false, Ordering::SeqCst) {
            return Ok(false);
        }
        self.inner.compare_and_swap(key, expected, bytes).await
    }
}

#[tokio::test]
async fn interrupted_save_is_invisible_and_resumes_without_refetch() {
    let store = InterruptStore {
        inner: MemoryStore::default(),
        fail_manifest: AtomicBool::new(true),
        conflict: AtomicBool::new(true),
    };
    let fetcher = FakeFetcher::new(RSS);
    assert!(
        execute(&config(), &run("interrupted"), &fetcher, &store)
            .await
            .is_err()
    );
    assert!(store.get(HEAD_KEY).await.unwrap().is_none());
    let result = execute(&config(), &run("interrupted"), &fetcher, &store)
        .await
        .unwrap();
    assert_eq!(result.counts.documents, 2);
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 1);
    let head: Head =
        serde_json::from_slice(&store.get(HEAD_KEY).await.unwrap().unwrap().bytes).unwrap();
    assert_eq!(head.generation, 1);
    let manifest = store
        .get(&head.runs["interrupted"].manifest_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Manifest>(&manifest.bytes)
            .unwrap()
            .run_id,
        "interrupted"
    );
}

#[tokio::test]
async fn local_store_cas_and_key_validation() {
    let temp = tempfile::tempdir().unwrap();
    let a = LocalStore::new(temp.path()).unwrap();
    let b = LocalStore::new(temp.path()).unwrap();
    assert!(
        a.compare_and_swap("state/head", None, b"one")
            .await
            .unwrap()
    );
    assert!(
        !b.compare_and_swap("state/head", None, b"two")
            .await
            .unwrap()
    );
    let current = b.get("state/head").await.unwrap().unwrap();
    assert!(
        b.compare_and_swap("state/head", Some(&current.version), b"two")
            .await
            .unwrap()
    );
    assert!(a.get("../outside").await.is_err());
    assert!(a.immutable("state/head", b"different").await.is_err());
}

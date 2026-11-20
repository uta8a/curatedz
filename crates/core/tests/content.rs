use chrono::{TimeZone, Utc};
use curatedz_core::{
    content::{
        ContentRecord, ContentStatus, extract, is_current_success, publish, validate_article_url,
        validate_public_ip,
    },
    model::PROCESSING_VERSION,
    store::MemoryStore,
};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

fn record() -> ContentRecord {
    ContentRecord {
        schema_version: 1,
        article_id: "sha256:article".into(),
        requested_url: "https://example.com/article".into(),
        final_url: Some("https://example.com/article".into()),
        declared_canonical_url: None,
        raw_key: None,
        text_key: None,
        text_hash: None,
        status: ContentStatus::Succeeded,
        fetched_at: Utc.with_ymd_and_hms(2026, 11, 10, 0, 0, 0).unwrap(),
        http_status: Some(200),
        content_type: Some("text/html".into()),
        failure: None,
        processing_version: PROCESSING_VERSION.into(),
        attempt: 1,
    }
}

#[test]
fn rejects_unsafe_urls_and_addresses() {
    assert!(validate_article_url("file:///etc/passwd").is_err());
    assert!(validate_article_url("http://user@example.com/").is_err());
    assert!(validate_article_url("https://example.com:8443/").is_err());
    assert!(validate_public_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))).is_err());
    assert!(validate_public_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))).is_err());
    assert!(validate_public_ip(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))).is_err());
    assert!(validate_public_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)).is_err());
    assert!(validate_public_ip("2606:4700:4700::1111".parse().unwrap()).is_ok());
    assert!(validate_public_ip("1.1.1.1".parse().unwrap()).is_ok());
}

#[test]
fn extracts_article_text_from_fixture_html() {
    let html = r#"<!doctype html><html><head><title>Example</title>
      <link rel="canonical" href="https://example.com/canonical"></head><body>
      <nav>menu menu menu</nav><article><h1>Example article</h1>
      <p>This is the first paragraph of the article with enough useful content for extraction.</p>
      <p>This is the second paragraph. It contains the durable information that should be searchable later.</p>
      </article><footer>footer links</footer></body></html>"#;
    let result = extract(html, "https://example.com/article").unwrap();
    assert!(result.text.contains("first paragraph"));
    assert!(!result.text.contains("menu menu menu"));
    assert_eq!(
        result.declared_canonical_url.as_deref(),
        Some("https://example.com/canonical")
    );
}

#[tokio::test]
async fn publishes_content_once_and_exposes_current_success() {
    let store = MemoryStore::default();
    let first = publish(
        &store,
        record(),
        Some(b"<article>body</article>"),
        Some("body"),
    )
    .await
    .unwrap();
    assert_eq!(first.generation, 1);
    assert_eq!(first.entries.len(), 1);
    assert!(
        is_current_success(
            &store,
            "sha256:article",
            "https://example.com/article",
            PROCESSING_VERSION
        )
        .await
        .unwrap()
    );
    let duplicate = publish(
        &store,
        record(),
        Some(b"<article>body</article>"),
        Some("body"),
    )
    .await
    .unwrap();
    assert_eq!(duplicate.generation, 1);
    assert!(duplicate.entries.is_empty());
}

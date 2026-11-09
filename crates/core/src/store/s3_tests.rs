use super::{Store, s3::S3Store};
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region, retry::RetryConfig};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

async fn store(response: &str) -> (S3Store, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let response = response.to_string();
    let request = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = vec![0; 16384];
        let size = socket.read(&mut bytes).await.unwrap();
        socket.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8_lossy(&bytes[..size])
            .to_string()
            .to_lowercase()
    });
    let config = aws_sdk_s3::config::Builder::new()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(Credentials::new("test", "test", None, None, "test"))
        .endpoint_url(endpoint)
        .force_path_style(true)
        .retry_config(RetryConfig::disabled())
        .build();
    (
        S3Store {
            client: aws_sdk_s3::Client::from_conf(config),
            bucket: "test".into(),
        },
        request,
    )
}

#[tokio::test]
async fn no_such_key_is_absent_but_access_denied_is_failure() {
    let body = "<Error><Code>NoSuchKey</Code></Error>";
    let (s3, request) = store(&format!(
        "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ))
    .await;
    assert!(s3.get("raw/v1/missing").await.unwrap().is_none());
    request.await.unwrap();
    let (s3, request) =
        store("HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
    assert!(s3.get("raw/v1/forbidden").await.is_err());
    request.await.unwrap();
}

#[tokio::test]
async fn writes_send_preconditions_and_report_conflicts() {
    let (s3, request) =
        store("HTTP/1.1 412 Precondition Failed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
    assert!(
        !s3.compare_and_swap("state/v1/head.json", Some("\"etag\""), b"{}")
            .await
            .unwrap()
    );
    assert!(request.await.unwrap().contains("if-match: \"etag\""));
    let (s3, request) =
        store("HTTP/1.1 200 OK\r\nETag: \"new\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
    assert!(
        s3.compare_and_swap("raw/v1/new", None, b"{}")
            .await
            .unwrap()
    );
    assert!(request.await.unwrap().contains("if-none-match: *"));
}

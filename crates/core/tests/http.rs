use curatedz_core::{
    config::{HttpConfig, Source},
    fetch::{Fetcher, HttpFetcher},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

async fn server(responses: Vec<String>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for response in responses {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0; 4096];
            let size = socket.read(&mut buffer).await.unwrap();
            assert!(size > 0);
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    format!("http://{address}/feed?token=must-not-appear")
}

#[tokio::test]
async fn transient_status_retries_and_succeeds() {
    let url = server(vec![
        "HTTP/1.1 503 Unavailable\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nContent-Length: 4\r\nConnection: close\r\n\r\nfeed".into(),
    ]).await;
    let result = HttpFetcher::new(HttpConfig::default())
        .unwrap()
        .fetch(&Source {
            id: "test".into(),
            url,
        })
        .await
        .unwrap();
    assert_eq!(result.bytes, b"feed");
}

#[tokio::test]
async fn permanent_status_redacts_url_and_large_body_is_rejected() {
    let url = server(vec![
        "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
    ])
    .await;
    let error = HttpFetcher::new(HttpConfig::default())
        .unwrap()
        .fetch(&Source {
            id: "test".into(),
            url,
        })
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("403"));
    assert!(!error.contains("must-not-appear"));
    let url = server(vec![
        "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nlarge".into(),
    ])
    .await;
    let config = HttpConfig {
        max_bytes: 4,
        ..HttpConfig::default()
    };
    assert!(
        HttpFetcher::new(config)
            .unwrap()
            .fetch(&Source {
                id: "test".into(),
                url
            })
            .await
            .is_err()
    );
}

#[tokio::test]
async fn stalled_body_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/feed", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = vec![0; 4096];
        let size = socket.read(&mut buffer).await.unwrap();
        assert!(size > 0);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n")
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    });
    let config = HttpConfig {
        timeout_seconds: 1,
        attempts: 1,
        ..HttpConfig::default()
    };
    assert!(
        HttpFetcher::new(config)
            .unwrap()
            .fetch(&Source {
                id: "test".into(),
                url
            })
            .await
            .is_err()
    );
    server.abort();
}

use crate::{
    Error, Result,
    content::{validate_article_url, validate_public_ip},
};
use async_trait::async_trait;
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use url::Url;

pub const MAX_ARTICLE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_REDIRECTS: usize = 5;

fn unavailable_is_empty(status: reqwest::StatusCode, enabled: bool) -> bool {
    enabled && status.is_client_error() && status != reqwest::StatusCode::TOO_MANY_REQUESTS
}

#[derive(Clone, Debug)]
pub struct ArticleResponse {
    pub bytes: Vec<u8>,
    pub final_url: String,
    pub status: u16,
    pub content_type: Option<String>,
}

async fn resolve_public(url: &Url) -> Result<(String, SocketAddr)> {
    let host = url
        .host_str()
        .ok_or_else(|| Error::Security("記事URLにhostがありません".into()))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| Error::Security("記事URLのportが不明です".into()))?;
    let addresses: Vec<SocketAddr> = if let Ok(ip) = host.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, port)]
    } else {
        tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| Error::Fetch("記事hostを解決できません".into()))?
            .collect()
    };
    if addresses.is_empty() {
        return Err(Error::Fetch("記事hostの解決結果が空です".into()));
    }
    for address in &addresses {
        validate_public_ip(address.ip())?;
    }
    Ok((host.to_string(), addresses[0]))
}

#[async_trait]
trait ArticleResolver: Send + Sync {
    async fn resolve(&self, url: &Url) -> Result<(String, SocketAddr)>;
}

struct PublicResolver;

#[async_trait]
impl ArticleResolver for PublicResolver {
    async fn resolve(&self, url: &Url) -> Result<(String, SocketAddr)> {
        resolve_public(url).await
    }
}

async fn fetch_resource(
    value: &str,
    accept: &str,
    allowed_content_types: &[&str],
    max_bytes: usize,
    missing_is_empty: bool,
) -> Result<Option<ArticleResponse>> {
    fetch_resource_with(
        value,
        accept,
        allowed_content_types,
        max_bytes,
        missing_is_empty,
        Duration::from_secs(15),
        &PublicResolver,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn fetch_resource_with(
    value: &str,
    accept: &str,
    allowed_content_types: &[&str],
    max_bytes: usize,
    missing_is_empty: bool,
    timeout: Duration,
    resolver: &dyn ArticleResolver,
) -> Result<Option<ArticleResponse>> {
    let mut url = validate_article_url(value)?;
    for redirects in 0..=MAX_REDIRECTS {
        let (host, address) = resolver.resolve(&url).await?;
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(timeout.min(Duration::from_secs(5)))
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve(&host, address)
            .user_agent("curatedz/0.1 article processor (+personal archive)")
            .build()
            .map_err(|_| Error::Fetch("記事HTTPクライアントを初期化できません".into()))?;
        let mut response = client
            .get(url.clone())
            .header("Accept", accept)
            .header("Accept-Encoding", "identity")
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    Error::Fetch("記事HTTPタイムアウト".into())
                } else {
                    Error::Fetch("記事HTTP接続失敗".into())
                }
            })?;
        if response.status().is_redirection() {
            if redirects == MAX_REDIRECTS {
                return Err(Error::Fetch("記事redirect回数が上限を超えました".into()));
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| Error::Fetch("記事redirect先が不正です".into()))?;
            let redirected = url
                .join(location)
                .map_err(|_| Error::Fetch("記事redirect先が不正です".into()))?;
            url = validate_article_url(redirected.as_ref())?;
            continue;
        }
        let status = response.status().as_u16();
        // RFC 9309ではrobots.txtの4xx（429を除く）は「unavailable」として扱える。
        // robots取得だけがmissing_is_empty=trueを指定する。
        if unavailable_is_empty(response.status(), missing_is_empty) {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(Error::Fetch(format!("記事HTTP {status}")));
        }
        if response
            .content_length()
            .is_some_and(|size| size > max_bytes as u64)
        {
            return Err(Error::Fetch(
                "記事HTTP応答がサイズ上限を超えています".into(),
            ));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.chars().take(200).collect::<String>());
        if response
            .headers()
            .get(reqwest::header::CONTENT_ENCODING)
            .is_some_and(|value| !value.as_bytes().eq_ignore_ascii_case(b"identity"))
        {
            return Err(Error::Fetch(
                "圧縮された記事HTTP応答は受け付けません".into(),
            ));
        }
        if !allowed_content_types.is_empty()
            && !content_type.as_deref().is_some_and(|value| {
                let value = value.to_ascii_lowercase();
                allowed_content_types
                    .iter()
                    .any(|allowed| value.starts_with(allowed))
            })
        {
            return Err(Error::Fetch("HTTP応答のContent-Typeが対象外です".into()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::Fetch("記事HTTP応答の読み取りに失敗しました".into()))?
        {
            if bytes.len() + chunk.len() > max_bytes {
                return Err(Error::Fetch(
                    "記事HTTP応答がサイズ上限を超えています".into(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(Some(ArticleResponse {
            bytes,
            final_url: url.to_string(),
            status,
            content_type,
        }));
    }
    unreachable!("redirect loop always returns or errors")
}

pub async fn fetch_html(value: &str) -> Result<ArticleResponse> {
    fetch_resource(
        value,
        "text/html,application/xhtml+xml",
        &["text/html", "application/xhtml+xml"],
        MAX_ARTICLE_BYTES,
        false,
    )
    .await?
    .ok_or_else(|| Error::Fetch("記事HTTP応答がありません".into()))
}

pub async fn fetch_robots(article_url: &str) -> Result<Option<String>> {
    let mut url = validate_article_url(article_url)?;
    url.set_path("/robots.txt");
    url.set_query(None);
    url.set_fragment(None);
    let response =
        fetch_resource(url.as_str(), "text/plain,*/*;q=0.1", &[], 256 * 1024, true).await?;
    Ok(response.map(|response| String::from_utf8_lossy(&response.bytes).into_owned()))
}

pub fn robots_allowed(body: &str, path: &str) -> bool {
    let mut applies = false;
    let mut group_has_rules = false;
    let mut rules: Vec<(bool, String)> = Vec::new();
    for raw in body.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        if name == "user-agent" {
            if group_has_rules {
                applies = false;
                group_has_rules = false;
            }
            applies |= value == "*" || value.eq_ignore_ascii_case("curatedz");
        } else if applies && matches!(name.as_str(), "allow" | "disallow") && !value.is_empty() {
            group_has_rules = true;
            rules.push((name == "allow", value.to_string()));
        } else if matches!(name.as_str(), "allow" | "disallow") {
            group_has_rules = true;
        }
    }
    rules
        .into_iter()
        .filter(|(_, prefix)| path.starts_with(prefix))
        .max_by_key(|(_, prefix)| prefix.len())
        .is_none_or(|(allow, _)| allow)
}

#[cfg(test)]
mod tests {
    use super::{ArticleResolver, fetch_resource_with, unavailable_is_empty};
    use crate::{Error, Result};
    use async_trait::async_trait;
    use reqwest::StatusCode;
    use std::{
        net::SocketAddr,
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use url::Url;

    struct FixedResolver {
        address: SocketAddr,
        hosts: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl ArticleResolver for FixedResolver {
        async fn resolve(&self, url: &Url) -> Result<(String, SocketAddr)> {
            let host = url.host_str().unwrap().to_string();
            self.hosts.lock().unwrap().push(host.clone());
            Ok((host, self.address))
        }
    }

    async fn serve(responses: Vec<&'static str>) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request).await.unwrap();
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (address, task)
    }

    #[test]
    fn robots_client_errors_are_unavailable_but_rate_limit_retries() {
        assert!(unavailable_is_empty(StatusCode::NOT_FOUND, true));
        assert!(unavailable_is_empty(StatusCode::FORBIDDEN, true));
        assert!(!unavailable_is_empty(StatusCode::TOO_MANY_REQUESTS, true));
        assert!(!unavailable_is_empty(StatusCode::NOT_FOUND, false));
        assert!(!unavailable_is_empty(StatusCode::SERVICE_UNAVAILABLE, true));
    }

    #[tokio::test]
    async fn redirect_is_re_resolved_and_response_is_bounded() {
        let (address, server) = serve(vec![
            "HTTP/1.1 302 Found\r\nLocation: http://second.test/final\r\nContent-Length: 0\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\n\r\nok",
        ])
        .await;
        let hosts = Arc::new(Mutex::new(Vec::new()));
        let resolver = FixedResolver {
            address,
            hosts: hosts.clone(),
        };
        let response = fetch_resource_with(
            "http://first.test/start",
            "text/html",
            &["text/html"],
            2,
            false,
            Duration::from_secs(1),
            &resolver,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(response.bytes, b"ok");
        assert_eq!(response.final_url, "http://second.test/final");
        assert_eq!(&*hosts.lock().unwrap(), &["first.test", "second.test"]);
        server.await.unwrap();

        let (address, server) = serve(vec![
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 3\r\n\r\nabc",
        ])
        .await;
        let resolver = FixedResolver {
            address,
            hosts: Arc::new(Mutex::new(Vec::new())),
        };
        let error = fetch_resource_with(
            "http://size.test/",
            "text/html",
            &["text/html"],
            2,
            false,
            Duration::from_secs(1),
            &resolver,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::Fetch(message) if message.contains("サイズ上限")));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn stalled_article_response_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        let resolver = FixedResolver {
            address,
            hosts: Arc::new(Mutex::new(Vec::new())),
        };
        let error = fetch_resource_with(
            "http://timeout.test/",
            "text/html",
            &["text/html"],
            1024,
            false,
            Duration::from_millis(30),
            &resolver,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::Fetch(message) if message.contains("タイムアウト")));
        server.await.unwrap();
    }
}

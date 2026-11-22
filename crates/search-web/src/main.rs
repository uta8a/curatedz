use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use chrono::{Days, NaiveDate, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

#[derive(Clone)]
struct AppState {
    http: reqwest::Client,
    opensearch: String,
    issuer: String,
    audience: String,
    jwks_url: String,
    jwks: Arc<RwLock<Option<(Instant, JwkSet)>>>,
}

#[derive(Deserialize, Serialize)]
struct Claims {
    exp: usize,
    iss: String,
    aud: Value,
}

fn validate_token(
    token: &str,
    key: &DecodingKey,
    issuer: &str,
    audience: &str,
) -> Result<(), StatusCode> {
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);
    validation.set_required_spec_claims(&["exp", "iss", "aud"]);
    let claims = decode::<Claims>(token, key, &validation)
        .map_err(|_| StatusCode::UNAUTHORIZED)?
        .claims;
    let _ = (claims.exp, claims.iss, claims.aud);
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchQuery {
    #[serde(default)]
    q: String,
    source: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_date")]
    date_from: Option<NaiveDate>,
    #[serde(default, deserialize_with = "deserialize_optional_date")]
    date_to: Option<NaiveDate>,
    page: Option<u32>,
    size: Option<u32>,
}

fn deserialize_optional_date<'de, D>(deserializer: D) -> Result<Option<NaiveDate>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    match value.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(value) => NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map(Some)
            .map_err(de::Error::custom),
    }
}

#[derive(Serialize)]
struct SearchItem {
    title: String,
    url: String,
    excerpt: String,
    published_at: Option<String>,
    source_ids: Vec<String>,
}
#[derive(Serialize)]
struct SearchResponse {
    total: u64,
    items: Vec<SearchItem>,
}

async fn keys(state: &AppState) -> Result<JwkSet, StatusCode> {
    if let Some((loaded, keys)) = &*state.jwks.read().await
        && loaded.elapsed() < Duration::from_secs(3600)
    {
        return Ok(keys.clone());
    }
    let keys: JwkSet = state
        .http
        .get(&state.jwks_url)
        .send()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .error_for_status()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .json()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    *state.jwks.write().await = Some((Instant::now(), keys.clone()));
    Ok(keys)
}

async fn access_auth(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let Some(token) = headers
        .get("Cf-Access-Jwt-Assertion")
        .and_then(|value| value.to_str().ok())
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let verified = async {
        let header = decode_header(token).map_err(|_| StatusCode::UNAUTHORIZED)?;
        if header.alg != Algorithm::RS256 {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let kid = header.kid.ok_or(StatusCode::UNAUTHORIZED)?;
        let keys = keys(&state).await?;
        let jwk = keys.find(&kid).ok_or(StatusCode::UNAUTHORIZED)?;
        let key = DecodingKey::from_jwk(jwk).map_err(|_| StatusCode::UNAUTHORIZED)?;
        validate_token(token, &key, &state.issuer, &state.audience)
    }
    .await;
    if let Err(status) = verified {
        return status.into_response();
    }
    next.run(request).await
}

async fn index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}
async fn health() -> &'static str {
    "ok"
}

fn app(state: AppState) -> Router {
    let protected = Router::new()
        .route("/", get(index))
        .route("/api/search", get(search))
        .route_layer(middleware::from_fn_with_state(state.clone(), access_auth));
    Router::new()
        .route("/healthz", get(health))
        .merge(protected)
        .with_state(state)
}

async fn search(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, (StatusCode, String)> {
    let body = search_body(query)?;
    let response = state
        .http
        .post(format!("{}/curatedz-articles/_search", state.opensearch))
        .json(&body)
        .send()
        .await
        .map_err(|_| (StatusCode::BAD_GATEWAY, "検索基盤へ接続できません".into()))?;
    if !response.status().is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            "検索基盤がエラーを返しました".into(),
        ));
    }
    let result: Value = response
        .json()
        .await
        .map_err(|_| (StatusCode::BAD_GATEWAY, "検索応答が不正です".into()))?;
    let total = result
        .pointer("/hits/total/value")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let items = result
        .pointer("/hits/hits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|hit| {
            let source = &hit["_source"];
            let excerpt = hit
                .pointer("/highlight/content/0")
                .or_else(|| hit.pointer("/highlight/description/0"))
                .and_then(Value::as_str)
                .or_else(|| source["description"].as_str())
                .unwrap_or("")
                .to_string();
            SearchItem {
                title: source["title"].as_str().unwrap_or("").into(),
                url: source["canonical_url"].as_str().unwrap_or("").into(),
                excerpt,
                published_at: source["published_at"].as_str().map(Into::into),
                source_ids: source["source_ids"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(Into::into)
                    .collect(),
            }
        })
        .collect();
    Ok(Json(SearchResponse { total, items }))
}

fn search_body(query: SearchQuery) -> Result<Value, (StatusCode, String)> {
    search_body_on(query, Utc::now().date_naive())
}

fn search_body_on(mut query: SearchQuery, today: NaiveDate) -> Result<Value, (StatusCode, String)> {
    if query.date_from.is_none() && query.date_to.is_none() {
        query.date_from = today.checked_sub_days(Days::new(6));
        query.date_to = Some(today);
    }
    let q = query.q.trim();
    if q.chars().count() > 200 {
        return Err((StatusCode::BAD_REQUEST, "qが不正です".into()));
    }
    let page = query.page.unwrap_or(0);
    let size = query.size.unwrap_or(20);
    if page > 500
        || !(1..=50).contains(&size)
        || page.checked_mul(size).is_none_or(|offset| offset >= 10_000)
    {
        return Err((StatusCode::BAD_REQUEST, "page/sizeが不正です".into()));
    }
    if query.source.as_ref().is_some_and(|value| value.len() > 100) {
        return Err((StatusCode::BAD_REQUEST, "sourceが不正です".into()));
    }
    if query
        .date_from
        .zip(query.date_to)
        .is_some_and(|(from, to)| from > to)
    {
        return Err((StatusCode::BAD_REQUEST, "日付範囲が不正です".into()));
    }
    let mut filters = Vec::new();
    if let Some(source) = query.source
        && !source.is_empty()
    {
        filters.push(json!({"term": {"source_ids": source}}));
    }
    if query.date_from.is_some() || query.date_to.is_some() {
        let mut range = serde_json::Map::new();
        if let Some(value) = query.date_from {
            range.insert("gte".into(), json!(value));
        }
        if let Some(value) = query.date_to {
            range.insert("lte".into(), json!(value));
        }
        filters.push(json!({"range": {"published_at": range}}));
    }
    let query = if q.is_empty() {
        json!({"bool": {"filter": filters}})
    } else {
        json!({"bool": {
            "must": [{"multi_match": {
                "query": q,
                "fields": ["title^3", "description^2", "content"]
            }}],
            "filter": filters
        }})
    };
    let sort = if q.is_empty() {
        json!([
            {"published_at": {"order": "desc", "missing": "_last"}},
            {"last_seen_at": {"order": "desc", "missing": "_last"}},
            {"article_id": "asc"}
        ])
    } else {
        json!([
            {"_score": "desc"},
            {"published_at": {"order": "desc", "missing": "_last"}},
            {"article_id": "asc"}
        ])
    };
    Ok(json!({
        "from": page * size,
        "size": size,
        "track_total_hits": true,
        "query": query,
        "highlight": {"fields": {
            "content": {"fragment_size": 180, "number_of_fragments": 1},
            "description": {}
        }},
        "sort": sort
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .init();
    let team = std::env::var("CF_ACCESS_TEAM_DOMAIN")?
        .trim_end_matches('/')
        .to_string();
    let state = AppState {
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?,
        opensearch: std::env::var("CURATEDZ_OPENSEARCH_URL")?
            .trim_end_matches('/')
            .into(),
        issuer: format!("https://{team}"),
        audience: std::env::var("CF_ACCESS_AUD")?,
        jwks_url: format!("https://{team}/cdn-cgi/access/certs"),
        jwks: Arc::new(RwLock::new(None)),
    };
    let app = app(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use jsonwebtoken::{EncodingKey, Header, encode};
    use rand::thread_rng;
    use rsa::{
        RsaPrivateKey, RsaPublicKey,
        pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey},
    };
    use tower::ServiceExt;

    fn state() -> AppState {
        AppState {
            http: reqwest::Client::new(),
            opensearch: "http://opensearch".into(),
            issuer: "https://example.cloudflareaccess.com".into(),
            audience: "audience".into(),
            jwks_url: "https://example.cloudflareaccess.com/cdn-cgi/access/certs".into(),
            jwks: Arc::new(RwLock::new(None)),
        }
    }

    #[tokio::test]
    async fn health_is_internal_but_ui_and_api_require_access_jwt() {
        let app = app(state());
        assert_eq!(
            app.clone()
                .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            app.clone()
                .oneshot(Request::get("/").body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            app.oneshot(
                Request::get("/api/search?q=rust")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn access_jwt_checks_expiry_issuer_and_audience() {
        let private = RsaPrivateKey::new(&mut thread_rng(), 2048).unwrap();
        let public = RsaPublicKey::from(&private);
        let private_der = private.to_pkcs1_der().unwrap();
        let public_der = public.to_pkcs1_der().unwrap();
        let encoding = EncodingKey::from_rsa_der(private_der.as_bytes());
        let decoding = DecodingKey::from_rsa_der(public_der.as_bytes());
        let now = chrono::Utc::now().timestamp() as usize;
        let token = |exp, iss: &str, aud: &str| {
            encode(
                &Header::new(Algorithm::RS256),
                &Claims {
                    exp,
                    iss: iss.into(),
                    aud: json!(aud),
                },
                &encoding,
            )
            .unwrap()
        };
        assert!(
            validate_token(
                &token(now + 300, "https://team.example", "aud"),
                &decoding,
                "https://team.example",
                "aud"
            )
            .is_ok()
        );
        assert!(
            validate_token(
                &token(now - 120, "https://team.example", "aud"),
                &decoding,
                "https://team.example",
                "aud"
            )
            .is_err()
        );
        assert!(
            validate_token(
                &token(now + 300, "https://wrong.example", "aud"),
                &decoding,
                "https://team.example",
                "aud"
            )
            .is_err()
        );
        assert!(
            validate_token(
                &token(now + 300, "https://team.example", "wrong"),
                &decoding,
                "https://team.example",
                "aud"
            )
            .is_err()
        );
    }

    #[test]
    fn search_query_validates_bounds_and_builds_filters() {
        let valid = search_body(SearchQuery {
            q: " Rust ".into(),
            source: Some("feed-a".into()),
            date_from: Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
            date_to: Some(NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()),
            page: Some(2),
            size: Some(10),
        })
        .unwrap();
        assert_eq!(valid["from"], 20);
        assert_eq!(valid["size"], 10);
        assert_eq!(
            valid.pointer("/query/bool/filter/0/term/source_ids"),
            Some(&json!("feed-a"))
        );

        let invalid = |q: &str, page, size, from, to| {
            search_body(SearchQuery {
                q: q.into(),
                source: None,
                date_from: from,
                date_to: to,
                page,
                size,
            })
            .unwrap_err()
            .0
        };
        let today = NaiveDate::from_ymd_opt(2026, 11, 22).unwrap();
        let recent = search_body_on(
            SearchQuery {
                q: String::new(),
                source: None,
                date_from: None,
                date_to: None,
                page: None,
                size: None,
            },
            today,
        )
        .unwrap();
        assert_eq!(
            recent.pointer("/query/bool/filter/0/range/published_at/gte"),
            Some(&json!("2026-11-16"))
        );
        assert_eq!(
            recent.pointer("/query/bool/filter/0/range/published_at/lte"),
            Some(&json!("2026-11-22"))
        );
        assert_eq!(
            recent.pointer("/sort/0/published_at/order"),
            Some(&json!("desc"))
        );
        assert_eq!(
            invalid("x", Some(501), None, None, None),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            invalid("x", Some(500), Some(20), None, None),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            invalid("x", None, Some(51), None, None),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            invalid(
                "x",
                None,
                None,
                Some(NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()),
                Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap())
            ),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn empty_date_query_values_are_treated_as_unspecified() {
        let query: SearchQuery = serde_json::from_value(json!({
            "q": "",
            "source": "",
            "date_from": "",
            "date_to": "",
            "page": 0,
            "size": 20
        }))
        .unwrap();

        assert_eq!(query.date_from, None);
        assert_eq!(query.date_to, None);
        assert!(
            serde_json::from_value::<SearchQuery>(json!({
                "q": "",
                "date_from": "2026-02-30"
            }))
            .is_err()
        );
    }
}

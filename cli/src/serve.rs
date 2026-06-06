//! `ytdown serve`: serve the browser demo + a local CORS proxy. Feature-gated.

use std::collections::HashMap;
use std::net::SocketAddr;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{any, get},
    Router,
};

/// The self-contained demo page, generated at build time by `build.rs`.
const DEMO_HTML: &str = include_str!(concat!(env!("OUT_DIR"), "/demo.html"));

/// Inverse of the SDK's forbidden-header aliasing. Keep in sync with
/// web/src/proxy.ts and web/proxy/src/worker.ts.
const ALIAS_TO_REAL: &[(&str, &str)] = &[
    ("x-ytdown-origin", "origin"),
    ("x-ytdown-referer", "referer"),
];

#[derive(Clone)]
struct ProxyState {
    http: reqwest::Client,
}

async fn index() -> Html<&'static str> {
    Html(DEMO_HTML)
}

/// Forward a request to `?url=<target>`, restoring forbidden-header aliases,
/// streaming the upstream response body back (media may be very large).
async fn proxy(
    State(st): State<ProxyState>,
    method: Method,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(target) = q.get("url") else {
        return (StatusCode::BAD_REQUEST, "missing ?url").into_response();
    };

    let mut fwd = reqwest::header::HeaderMap::new();
    for (name, value) in headers.iter() {
        let lname = name.as_str().to_ascii_lowercase();
        if lname == "host" || ALIAS_TO_REAL.iter().any(|(a, _)| *a == lname) {
            continue;
        }
        if let (Ok(n), Ok(v)) = (
            reqwest::header::HeaderName::from_bytes(name.as_ref()),
            reqwest::header::HeaderValue::from_bytes(value.as_bytes()),
        ) {
            fwd.insert(n, v);
        }
    }
    for (alias, real) in ALIAS_TO_REAL {
        if let Some(v) = headers.get(*alias) {
            if let (Ok(n), Ok(val)) = (
                reqwest::header::HeaderName::from_bytes(real.as_bytes()),
                reqwest::header::HeaderValue::from_bytes(v.as_bytes()),
            ) {
                fwd.insert(n, val);
            }
        }
    }

    let rmethod =
        reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap_or(reqwest::Method::GET);
    let mut rb = st.http.request(rmethod, target).headers(fwd);
    if !body.is_empty() {
        rb = rb.body(body);
    }
    match rb.send().await {
        Ok(up) => {
            let status =
                StatusCode::from_u16(up.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut builder = Response::builder().status(status);
            for (k, v) in up.headers().iter() {
                if k.as_str()
                    .eq_ignore_ascii_case("access-control-allow-origin")
                {
                    continue;
                }
                if let (Ok(name), Ok(val)) = (
                    HeaderName::from_bytes(k.as_ref()),
                    HeaderValue::from_bytes(v.as_bytes()),
                ) {
                    builder = builder.header(name, val);
                }
            }
            builder = builder.header("access-control-allow-origin", "*");
            builder
                .body(Body::from_stream(up.bytes_stream()))
                .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
        }
        Err(e) => (StatusCode::BAD_GATEWAY, format!("proxy error: {e}")).into_response(),
    }
}

/// Build the router. Split out so tests can exercise it without binding a port.
pub(crate) fn app() -> Router {
    let state = ProxyState {
        http: reqwest::Client::new(),
    };
    Router::new()
        .route("/", get(index))
        .route("/proxy", any(proxy))
        .with_state(state)
}

/// Run the demo server on `127.0.0.1:port`.
pub(crate) async fn run(port: u16, open: bool) -> anyhow::Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("could not bind {addr}: {e}"))?;
    let url = format!("http://{addr}");
    eprintln!("ytdown demo serving at {url}");
    if open {
        let _ = webbrowser::open(&url);
    }
    axum::serve(listener, app()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt; // for `oneshot`

    #[tokio::test]
    async fn index_returns_html() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 64 * 1024 * 1024).await.unwrap();
        let s = String::from_utf8_lossy(&body);
        assert!(s.contains("Best progressive"), "demo UI present");
        assert!(!s.contains("__WASM_B64__"), "wasm placeholder filled");
        assert!(!s.contains("__GLUE_B64__"), "glue placeholder filled");
    }

    #[tokio::test]
    async fn proxy_forwards_and_restores_aliased_headers() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let upstream = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/player"))
            .and(header("origin", "https://www.youtube.com"))
            .and(header("referer", "https://www.youtube.com/"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&upstream)
            .await;

        let target = format!("{}/youtubei/v1/player", upstream.uri());
        let uri = format!("/proxy?url={}", urlencoding(&target));
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("x-ytdown-origin", "https://www.youtube.com")
                    .header("x-ytdown-referer", "https://www.youtube.com/")
                    .header("x-goog-api-key", "k")
                    .body(axum::body::Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        assert_eq!(&body[..], b"ok");
    }

    #[tokio::test]
    async fn proxy_rejects_missing_url() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/proxy")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // Minimal percent-encoder for the test (avoids a dep): encode all non-alnum.
    fn urlencoding(s: &str) -> String {
        s.bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    }
}

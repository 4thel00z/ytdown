//! Runtime-agnostic HTTP transport used by the extraction core.
//!
//! The extractor builds [`HttpRequest`]s and hands them to an [`HttpClient`].
//! Native builds use [`ReqwestClient`]; the wasm build (in the `ytdown-wasm`
//! crate) supplies a transport backed by a JS `fetch` callback.

use crate::error::Result;

/// HTTP method for an [`HttpRequest`]. Only the verbs the extractor uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// HTTP GET.
    Get,
    /// HTTP POST.
    Post,
}

/// A buffered HTTP request issued by the extraction core.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// The label of the operation, used in error messages (e.g. `"innertube"`).
    pub stage: &'static str,
    /// HTTP method.
    pub method: Method,
    /// Absolute request URL.
    pub url: String,
    /// Request headers as ordered `(name, value)` pairs.
    pub headers: Vec<(String, String)>,
    /// Optional request body (JSON for InnerTube POSTs).
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    /// A GET request for `url` tagged with `stage`.
    pub fn get(stage: &'static str, url: impl Into<String>) -> Self {
        Self {
            stage,
            method: Method::Get,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// A POST request for `url` tagged with `stage`.
    pub fn post(stage: &'static str, url: impl Into<String>) -> Self {
        Self {
            stage,
            method: Method::Post,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// Add a header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Set the body.
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }
}

/// A buffered HTTP response. Control-plane bodies are bounded (<= 32 MiB).
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response headers as `(name, value)` pairs. The built-in transports
    /// lowercase the names, but `header()` matches case-insensitively regardless.
    pub headers: Vec<(String, String)>,
    /// Fully-buffered response body.
    pub body: Vec<u8>,
    /// The URL the response was served from, after following any redirects.
    /// `None` when the transport cannot know it (e.g. an opaque JS `fetch`).
    /// Extractors use this to resolve share-shortlinks into canonical URLs.
    pub final_url: Option<String>,
}

impl HttpResponse {
    /// Whether the status is in the 2xx range.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// First header whose name matches `name` case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// A runtime-agnostic HTTP client used by the extraction core.
///
/// The `Send + Sync` supertrait bound is kept on all targets so the rest of the
/// async extraction stack (which is `Send` on native) does not need to relax its
/// bounds. On `wasm32` the returned `execute` future is `?Send` (a JS `fetch`
/// future is not `Send`), while the implementor itself is made `Send + Sync` via
/// an `unsafe impl` that is sound under wasm's single-threaded model — see the
/// `ytdown-wasm` crate's transport.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait HttpClient: Send + Sync {
    /// Execute a buffered request, returning a buffered response.
    async fn execute(&self, req: HttpRequest) -> Result<HttpResponse>;
}

/// Maximum control-plane response body buffered by default (32 MiB).
#[cfg(not(target_arch = "wasm32"))]
pub const MAX_RESPONSE_BYTES: u64 = 32 * 1024 * 1024;

/// Native [`HttpClient`] backed by a [`reqwest::Client`].
#[cfg(not(target_arch = "wasm32"))]
pub struct ReqwestClient {
    http: reqwest::Client,
    max_bytes: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl ReqwestClient {
    /// Wrap an existing client with the default body cap.
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            max_bytes: MAX_RESPONSE_BYTES,
        }
    }

    /// Wrap a client with a custom body cap (testing).
    pub fn with_max_bytes(http: reqwest::Client, max_bytes: u64) -> Self {
        Self { http, max_bytes }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait::async_trait]
impl HttpClient for ReqwestClient {
    async fn execute(&self, req: HttpRequest) -> Result<HttpResponse> {
        use crate::error::Error;
        use futures::StreamExt;

        let mut builder = match req.method {
            Method::Get => self.http.get(&req.url),
            Method::Post => self.http.post(&req.url),
        };
        for (k, v) in &req.headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        if let Some(body) = req.body {
            builder = builder.body(body);
        }
        let resp = builder.send().await.map_err(|e| Error::Network {
            stage: req.stage,
            message: e.to_string(),
        })?;

        let final_url = Some(resp.url().to_string());
        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_ascii_lowercase(),
                    v.to_str().unwrap_or("").to_string(),
                )
            })
            .collect();

        if let Some(len) = resp.content_length() {
            if len > self.max_bytes {
                return Err(Error::Extraction {
                    stage: req.stage,
                    message: format!("response body too large: {len} bytes"),
                });
            }
        }
        let mut buf = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| Error::Network {
                stage: req.stage,
                message: e.to_string(),
            })?;
            if buf.len() as u64 + chunk.len() as u64 > self.max_bytes {
                return Err(Error::Extraction {
                    stage: req.stage,
                    message: "response body exceeded size limit".into(),
                });
            }
            buf.extend_from_slice(&chunk);
        }

        Ok(HttpResponse {
            status,
            headers,
            body: buf,
            final_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_builder_sets_fields() {
        let r = HttpRequest::post("innertube", "https://x.test/p")
            .header("Content-Type", "application/json")
            .body(b"{}".to_vec());
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.stage, "innertube");
        assert_eq!(
            r.headers,
            vec![("Content-Type".into(), "application/json".into())]
        );
        assert_eq!(r.body.as_deref(), Some(&b"{}"[..]));
    }

    #[test]
    fn response_helpers_work() {
        let resp = HttpResponse {
            status: 204,
            headers: vec![("Content-Length".into(), "0".into())],
            body: Vec::new(),
            final_url: None,
        };
        assert!(resp.is_success());
        assert_eq!(resp.header("content-length"), Some("0"));
        assert_eq!(resp.header("missing"), None);
    }

    #[test]
    fn get_builder_sets_method_and_url() {
        let r = HttpRequest::get("player", "https://x.test/base.js");
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.url, "https://x.test/base.js");
        assert!(r.headers.is_empty());
        assert!(r.body.is_none());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod reqwest_tests {
    use super::*;
    use wiremock::matchers::{method as m, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn reqwest_client_executes_get_and_buffers_body() {
        let server = MockServer::start().await;
        Mock::given(m("GET"))
            .and(path("/hi"))
            .respond_with(ResponseTemplate::new(200).set_body_string("pong"))
            .mount(&server)
            .await;

        let client = ReqwestClient::new(reqwest::Client::new());
        let resp = client
            .execute(HttpRequest::get("test", format!("{}/hi", server.uri())))
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"pong");
    }

    #[tokio::test]
    async fn reqwest_client_reports_final_url_after_redirect() {
        let server = MockServer::start().await;
        Mock::given(m("GET"))
            .and(path("/short"))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", "/final"))
            .mount(&server)
            .await;
        Mock::given(m("GET"))
            .and(path("/final"))
            .respond_with(ResponseTemplate::new(200).set_body_string("here"))
            .mount(&server)
            .await;

        let client = ReqwestClient::new(reqwest::Client::new());
        let resp = client
            .execute(HttpRequest::get("test", format!("{}/short", server.uri())))
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
        // The resolved URL, not the request URL echoed back.
        assert_eq!(
            resp.final_url.as_deref(),
            Some(format!("{}/final", server.uri()).as_str())
        );
    }

    #[tokio::test]
    async fn reqwest_client_rejects_oversized_body() {
        let server = MockServer::start().await;
        let huge = "x".repeat(64);
        Mock::given(m("GET"))
            .and(path("/big"))
            .respond_with(ResponseTemplate::new(200).set_body_string(&huge))
            .mount(&server)
            .await;

        let client = ReqwestClient::with_max_bytes(reqwest::Client::new(), 8);
        let err = client
            .execute(HttpRequest::get("test", format!("{}/big", server.uri())))
            .await
            .unwrap_err();
        assert!(matches!(err, crate::Error::Extraction { .. }));
    }
}

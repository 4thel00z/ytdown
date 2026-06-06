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
    /// Response headers as `(lowercased-name, value)` pairs.
    pub headers: Vec<(String, String)>,
    /// Fully-buffered response body.
    pub body: Vec<u8>,
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
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait HttpClient: Send + Sync {
    /// Execute a buffered request, returning a buffered response.
    async fn execute(&self, req: HttpRequest) -> Result<HttpResponse>;
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
        };
        assert!(resp.is_success());
        assert_eq!(resp.header("content-length"), Some("0"));
        assert_eq!(resp.header("missing"), None);
    }
}

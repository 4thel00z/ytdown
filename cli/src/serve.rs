//! `ytdown serve`: serve the browser demo + a local CORS proxy. Feature-gated.

use std::net::SocketAddr;

use axum::{routing::get, Router};

/// The self-contained demo page, generated at build time by `build.rs`.
const DEMO_HTML: &str = include_str!(concat!(env!("OUT_DIR"), "/demo.html"));

async fn index() -> axum::response::Html<&'static str> {
    axum::response::Html(DEMO_HTML)
}

/// Build the router. Split out so tests can exercise it without binding a port.
/// The `/proxy` route is added in the next task.
pub(crate) fn app() -> Router {
    Router::new().route("/", get(index))
}

/// Run the demo server on `127.0.0.1:port`.
pub async fn run(port: u16, open: bool) -> anyhow::Result<()> {
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
    }
}

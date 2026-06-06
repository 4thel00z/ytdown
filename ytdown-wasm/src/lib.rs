//! WebAssembly bindings for `ytdown`.
//!
//! Network I/O is delegated to a JS async callback the host supplies, so every
//! request can be routed through a CORS proxy the user controls. This module
//! resolves and deciphers URLs; byte downloads happen host-side in JS.

use std::sync::Arc;

use js_sys::{Promise, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use ytdown::transport::{HttpClient, HttpRequest, HttpResponse, Method};
use ytdown::{Error, MediaInfo, Result};

/// `js_sys::Function` is not `Send`; under wasm there is a single thread, so we
/// assert `Send`/`Sync` to satisfy the `HttpClient: Send + Sync` supertrait.
struct SendFn(js_sys::Function);
// SAFETY: wasm32 in the browser is single-threaded; the function is never sent
// across threads. This is the standard pattern for holding JS values behind a
// `Send` trait object on wasm.
unsafe impl Send for SendFn {}
unsafe impl Sync for SendFn {}

/// A transport that forwards each request to a JS async function:
/// `(req: {stage, method, url, headers, body}) => Promise<{status, headers, body}>`.
struct JsHttpClient {
    func: SendFn,
}

#[async_trait::async_trait(?Send)]
impl HttpClient for JsHttpClient {
    async fn execute(&self, req: HttpRequest) -> Result<HttpResponse> {
        let obj = js_sys::Object::new();
        let method = match req.method {
            Method::Get => "GET",
            Method::Post => "POST",
        };
        set(&obj, "stage", &JsValue::from_str(req.stage));
        set(&obj, "method", &JsValue::from_str(method));
        set(&obj, "url", &JsValue::from_str(&req.url));
        let headers = js_sys::Array::new();
        for (k, v) in &req.headers {
            let pair = js_sys::Array::new();
            pair.push(&JsValue::from_str(k));
            pair.push(&JsValue::from_str(v));
            headers.push(&pair);
        }
        set(&obj, "headers", &headers);
        if let Some(body) = &req.body {
            set(&obj, "body", &Uint8Array::from(body.as_slice()));
        }

        let promise: Promise = self
            .func
            .0
            .call1(&JsValue::NULL, &obj)
            .map_err(|e| transport_err(req.stage, &e))?
            .into();
        let resp = JsFuture::from(promise)
            .await
            .map_err(|e| transport_err(req.stage, &e))?;

        let status = num(&resp, "status").unwrap_or(0.0) as u16;
        let body = match Reflect::get(&resp, &JsValue::from_str("body")) {
            Ok(v) if !v.is_undefined() && !v.is_null() => Uint8Array::new(&v).to_vec(),
            _ => Vec::new(),
        };
        let headers = read_headers(&resp);
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

fn set(obj: &js_sys::Object, key: &str, val: &JsValue) {
    let _ = Reflect::set(obj, &JsValue::from_str(key), val);
}
fn num(obj: &JsValue, key: &str) -> Option<f64> {
    Reflect::get(obj, &JsValue::from_str(key)).ok()?.as_f64()
}
fn read_headers(resp: &JsValue) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Ok(h) = Reflect::get(resp, &JsValue::from_str("headers")) {
        if let Ok(arr) = h.dyn_into::<js_sys::Array>() {
            for pair in arr.iter() {
                if let Ok(p) = pair.dyn_into::<js_sys::Array>() {
                    let k = p.get(0).as_string().unwrap_or_default();
                    let v = p.get(1).as_string().unwrap_or_default();
                    out.push((k.to_ascii_lowercase(), v));
                }
            }
        }
    }
    out
}
fn transport_err(stage: &'static str, e: &JsValue) -> Error {
    Error::Network {
        stage,
        message: e
            .as_string()
            .unwrap_or_else(|| "JS fetch callback rejected".into()),
    }
}

/// Browser-facing handle. Construct in JS with `new Ytdown(fetchCallback)`.
#[wasm_bindgen]
pub struct Ytdown {
    inner: ytdown::Ytdown,
}

#[wasm_bindgen]
impl Ytdown {
    /// Create a resolver. `fetch_cb` is `(req) => Promise<resp>` (see the SDK README).
    #[wasm_bindgen(constructor)]
    pub fn new(fetch_cb: js_sys::Function) -> Ytdown {
        console_error_panic_hook::set_once();
        let transport: Arc<dyn HttpClient> = Arc::new(JsHttpClient {
            func: SendFn(fetch_cb),
        });
        let inner = ytdown::Ytdown::builder().build_with_transport(transport);
        Ytdown { inner }
    }

    /// Resolve a URL into media info. Returns the serialized video on success.
    ///
    /// Playlists/channels/searches (collections) are not yet supported in the
    /// browser build and return an error.
    pub async fn resolve(&self, url: String) -> std::result::Result<JsValue, JsValue> {
        match self.inner.resolve(&url).await.map_err(err_to_js)? {
            MediaInfo::Single(video) => {
                serde_wasm_bindgen::to_value(&video).map_err(|e| JsValue::from_str(&e.to_string()))
            }
            MediaInfo::Collection(_) => Err(JsValue::from_str(
                "collections (playlists/channels/search) are not yet supported in the browser SDK",
            )),
        }
    }
}

fn err_to_js(e: Error) -> JsValue {
    JsValue::from_str(&e.to_string())
}

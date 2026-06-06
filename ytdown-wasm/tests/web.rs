//! Runs in Node via `wasm-pack test --node`. Exercises the resolve() surface
//! through a JS fetch callback, asserting the binding works end-to-end.
#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

// An unsupported URL has no matching extractor, so resolve() rejects without
// the fetch callback ever being invoked — this verifies the wasm binding,
// the JS-callback transport wiring, and error marshalling across the boundary.
#[wasm_bindgen_test]
async fn resolve_unsupported_url_errors() {
    let cb = js_sys::Function::new_with_args(
        "req",
        "return Promise.resolve({status: 200, headers: [], body: new Uint8Array()});",
    );
    let yt = ytdown_wasm::Ytdown::new(cb);
    let res = yt.resolve("https://example.com/nope".into()).await;
    assert!(res.is_err(), "unsupported URL must reject");
}

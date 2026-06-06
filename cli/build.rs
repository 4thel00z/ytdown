use std::path::Path;

fn main() {
    // Only embed the demo page when building with --features serve.
    if std::env::var_os("CARGO_FEATURE_SERVE").is_none() {
        return;
    }

    println!("cargo:rerun-if-changed=../web/demo/template.html");
    println!("cargo:rerun-if-changed=../web/wasm/ytdown_wasm.js");
    println!("cargo:rerun-if-changed=../web/wasm/ytdown_wasm_bg.wasm");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let root = Path::new(&manifest).parent().expect("workspace root");
    let template_path = root.join("web/demo/template.html");
    let glue_path = root.join("web/wasm/ytdown_wasm.js");
    let wasm_path = root.join("web/wasm/ytdown_wasm_bg.wasm");

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;

    // Template is committed; the wasm is generated (gitignored). If the wasm is
    // absent (e.g. a clean checkout under `--all-features`), emit a still-valid
    // page with empty placeholders + a warning, so the build/tests/doc do not
    // break. Build the wasm (`cd web && bun run build:wasm`) for a working demo.
    let template = std::fs::read_to_string(&template_path).unwrap_or_else(|_| {
        println!(
            "cargo:warning=serve: missing {}; embedding an empty demo page",
            template_path.display()
        );
        String::from(
            "<!doctype html><meta charset=utf-8><title>ytdown</title>__GLUE_B64____WASM_B64__",
        )
    });

    let (glue_b64, wasm_b64) = match (std::fs::read(&glue_path), std::fs::read(&wasm_path)) {
        (Ok(glue), Ok(wasm)) => (b64.encode(glue), b64.encode(wasm)),
        _ => {
            println!(
                "cargo:warning=serve: wasm not found at {} — embedding a non-functional demo. \
                 Run `cd web && bun run build:wasm` for a working page.",
                wasm_path.display()
            );
            (String::new(), String::new())
        }
    };

    let html = template
        .replace("__GLUE_B64__", &glue_b64)
        .replace("__WASM_B64__", &wasm_b64);

    std::fs::write(Path::new(&out_dir).join("demo.html"), html).expect("write demo.html");
}

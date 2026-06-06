use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=../web/demo/template.html");
    println!("cargo:rerun-if-changed=../web/wasm/ytdown_wasm.js");
    println!("cargo:rerun-if-changed=../web/wasm/ytdown_wasm_bg.wasm");

    // Only embed the demo page when building with --features serve.
    if std::env::var_os("CARGO_FEATURE_SERVE").is_none() {
        return;
    }

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let root = Path::new(&manifest).parent().expect("workspace root");
    let template = root.join("web/demo/template.html");
    let glue = root.join("web/wasm/ytdown_wasm.js");
    let wasm = root.join("web/wasm/ytdown_wasm_bg.wasm");

    for p in [&template, &glue, &wasm] {
        if !p.exists() {
            panic!(
                "serve feature needs the wasm built: missing {}.\n\
                 Run `cd web && bun run build:wasm` first.",
                p.display()
            );
        }
    }

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let html = std::fs::read_to_string(&template)
        .expect("read template")
        .replace(
            "__GLUE_B64__",
            &b64.encode(std::fs::read(&glue).expect("read glue")),
        )
        .replace(
            "__WASM_B64__",
            &b64.encode(std::fs::read(&wasm).expect("read wasm")),
        );

    std::fs::write(Path::new(&out_dir).join("demo.html"), html).expect("write demo.html");
}

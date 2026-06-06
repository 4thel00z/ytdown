# ytdown browser demo (single-file)

`bun run build` reads the wasm-pack output from `../wasm/` and inlines it into a
self-contained `index.html` (wasm base64-inlined, glue + SDK logic inlined,
Tailwind via CDN).

## Build

    cd web && bun run build:wasm    # produce ../wasm (once)
    cd web/demo && bun build.mjs    # writes index.html

`index.html` needs a CORS proxy. Either run `ytdown serve` (serves this page +
a local proxy; the Proxy URL field defaults to the same origin), or deploy
`web/proxy` and paste its URL into the Proxy URL field.

## Limitations

Single videos only (collections error). Streaming download needs the File
System Access API (Chromium); the Blob fallback buffers in memory.

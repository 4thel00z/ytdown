// Generates a self-contained web/demo/index.html from template.html + the
// wasm-pack output in web/wasm/. Run: bun build.mjs (from web/demo).
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const wasmDir = join(here, "..", "wasm");

const glue = readFileSync(join(wasmDir, "ytdown_wasm.js"));
const wasm = readFileSync(join(wasmDir, "ytdown_wasm_bg.wasm"));
const template = readFileSync(join(here, "template.html"), "utf8");

const out = template
  .replace("__GLUE_B64__", glue.toString("base64"))
  .replace("__WASM_B64__", wasm.toString("base64"));

writeFileSync(join(here, "index.html"), out);
console.log(`wrote index.html (${(out.length / 1e6).toFixed(1)} MB)`);

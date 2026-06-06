import { defineConfig } from "tsup";

export default defineConfig({
  entry: ["src/index.ts"],
  format: ["esm", "cjs"],
  dts: true,
  clean: true,
  sourcemap: true,
  // The wasm glue is loaded at runtime from ./wasm; do not bundle it.
  external: ["../wasm/ytdown_wasm.js"],
});

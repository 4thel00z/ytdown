import { defineConfig } from "vitest/config";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  test: {
    environment: "jsdom",
    globals: true,
    // The reference Worker in proxy/ is a separate package with its own vitest
    // run; don't crawl into it (its tests must not run under the SDK's jsdom env).
    exclude: ["proxy/**", "node_modules/**", "dist/**"],
  },
  resolve: {
    alias: {
      // Stub the wasm glue at test time; tests that need the wasm
      // pass their own `loadWasm` override to Ytdown.create.
      "../wasm/ytdown_wasm.js": path.resolve(__dirname, "test/__stubs__/ytdown_wasm.ts"),
    },
  },
});

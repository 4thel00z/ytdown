// Loads the wasm-pack ESM glue. The path resolves to the published `wasm/`
// dir (see package.json "files"). Overridable in tests via Ytdown.create.
export interface WasmModule {
  default: (input?: unknown) => Promise<unknown>;
  Ytdown: new (fetchCb: (req: unknown) => Promise<unknown>) => {
    resolve: (url: string) => Promise<unknown>;
  };
}

export async function loadWasm(): Promise<WasmModule> {
  // @ts-ignore resolved at runtime from the package's wasm/ output
  const mod = (await import("../wasm/ytdown_wasm.js")) as unknown as WasmModule;
  await mod.default();
  return mod;
}

// Stub for the wasm-pack generated glue module.
// Tests that exercise the wasm core pass their own `loadWasm` override
// to Ytdown.create, so this module is never actually called in tests.
export default async function init(_input?: unknown): Promise<void> {
  throw new Error("wasm stub: use loadWasm override in tests");
}

export class Ytdown {
  constructor(_fetchCb: (req: unknown) => Promise<unknown>) {
    throw new Error("wasm stub: use loadWasm override in tests");
  }
  resolve(_url: string): Promise<unknown> {
    throw new Error("wasm stub: use loadWasm override in tests");
  }
}

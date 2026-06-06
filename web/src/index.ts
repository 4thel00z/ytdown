import { makeProxyFetch, type ProxyFetch } from "./proxy";
import { downloadToDisk, type DownloadOptions } from "./download";
import { loadWasm as defaultLoadWasm, type WasmModule } from "./wasm";

export type { DownloadProgress, DownloadOptions } from "./download";
export type { WasmRequest, WasmResponse, ProxyFetch } from "./proxy";

export interface YtdownConfig {
  /** Base URL of your CORS proxy (see web/proxy for a reference Worker). */
  proxy: string;
  /** Override the wasm loader (tests / custom hosting). */
  loadWasm?: () => Promise<WasmModule>;
  /** Override fetch (tests). */
  fetchImpl?: typeof fetch;
}

/** Browser entry point for ytdown. */
export class Ytdown {
  private constructor(
    private readonly core: { resolve: (url: string) => Promise<unknown> },
    private readonly proxy: string,
    private readonly fetchImpl: typeof fetch,
  ) {}

  /** Load the wasm core and wire it to your proxy. */
  static async create(config: YtdownConfig): Promise<Ytdown> {
    const loader = config.loadWasm ?? defaultLoadWasm;
    const fetchImpl = config.fetchImpl ?? fetch;
    const mod = await loader();
    const cb: ProxyFetch = makeProxyFetch(config.proxy, fetchImpl);
    const core = new mod.Ytdown(cb as unknown as (req: unknown) => Promise<unknown>);
    return new Ytdown(core, config.proxy, fetchImpl);
  }

  /** Resolve a URL into media info. */
  async resolve(url: string): Promise<unknown> {
    return this.core.resolve(url);
  }

  /** Download a resolved format URL to disk (streams when possible). */
  async download(
    formatUrl: string,
    opts: Omit<DownloadOptions, "proxyUrl" | "fetchImpl">,
  ): Promise<void> {
    return downloadToDisk(formatUrl, {
      ...opts,
      proxyUrl: this.proxy,
      fetchImpl: this.fetchImpl,
    });
  }
}

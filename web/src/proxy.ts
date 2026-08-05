/** A request emitted by the WASM core. */
export interface WasmRequest {
  stage: string;
  method: "GET" | "POST";
  url: string;
  headers: [string, string][];
  body?: Uint8Array;
}

/** A response the WASM core expects back. */
export interface WasmResponse {
  status: number;
  headers: [string, string][];
  body?: Uint8Array;
  /** The URL the response was served from, after redirects (if known). */
  url?: string;
}

/** The callback shape the WASM `Ytdown` constructor takes. */
export type ProxyFetch = (req: WasmRequest) => Promise<WasmResponse>;

/**
 * Headers the browser forbids client JS from setting on a `fetch` (it silently
 * drops them). We alias them so they survive to the proxy, which restores the
 * real names server-side. Keep in sync with web/proxy/src/worker.ts.
 */
const FORBIDDEN_HEADER_ALIASES: Record<string, string> = {
  origin: "x-ytdown-origin",
  referer: "x-ytdown-referer",
};

/**
 * Build a fetch callback that routes every request through `proxyUrl`. The
 * target URL is passed as `?url=<encoded>`; the proxy forwards method, headers,
 * and body, and must respond with permissive CORS headers (see web/proxy).
 */
export function makeProxyFetch(
  proxyUrl: string,
  fetchImpl: typeof fetch = fetch,
): ProxyFetch {
  return async (req) => {
    const target = `${proxyUrl}?url=${encodeURIComponent(req.url)}`;
    const headers = new Headers();
    for (const [k, v] of req.headers) {
      const alias = FORBIDDEN_HEADER_ALIASES[k.toLowerCase()];
      headers.set(alias ?? k, v);
    }
    const res = await fetchImpl(target, {
      method: req.method,
      headers,
      body: req.body && req.method !== "GET" ? (req.body as unknown as BodyInit) : undefined,
    });
    const buf = new Uint8Array(await res.arrayBuffer());
    const respHeaders: [string, string][] = [];
    res.headers.forEach((v, k) => respHeaders.push([k, v]));
    // The proxy exposes the upstream's post-redirect URL; `res.url` itself
    // would only be the proxy URL.
    const finalUrl = res.headers.get("x-ytdown-final-url") ?? undefined;
    return { status: res.status, headers: respHeaders, body: buf, url: finalUrl };
  };
}

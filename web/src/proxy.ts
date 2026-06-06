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
}

/** The callback shape the WASM `Ytdown` constructor takes. */
export type ProxyFetch = (req: WasmRequest) => Promise<WasmResponse>;

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
    for (const [k, v] of req.headers) headers.set(k, v);
    const res = await fetchImpl(target, {
      method: req.method,
      headers,
      body: req.body && req.method !== "GET" ? (req.body as unknown as BodyInit) : undefined,
    });
    const buf = new Uint8Array(await res.arrayBuffer());
    const respHeaders: [string, string][] = [];
    res.headers.forEach((v, k) => respHeaders.push([k, v]));
    return { status: res.status, headers: respHeaders, body: buf };
  };
}

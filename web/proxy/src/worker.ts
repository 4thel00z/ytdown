const CORS = {
  "access-control-allow-origin": "*",
  "access-control-allow-methods": "GET, POST, OPTIONS",
  "access-control-allow-headers": "*",
  "access-control-expose-headers": "*",
};

/** Reverse of the SDK's forbidden-header aliasing. Keep in sync with web/src/proxy.ts. */
const ALIAS_TO_REAL: Record<string, string> = {
  "x-ytdown-origin": "origin",
  "x-ytdown-referer": "referer",
};

export default {
  async fetch(request: Request): Promise<Response> {
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: CORS });
    }
    const target = new URL(request.url).searchParams.get("url");
    if (!target) {
      return new Response("missing ?url", { status: 400, headers: CORS });
    }
    const fwd = new Headers(request.headers);
    for (const [alias, real] of Object.entries(ALIAS_TO_REAL)) {
      const val = fwd.get(alias);
      if (val !== null) {
        fwd.set(real, val);
        fwd.delete(alias);
      }
    }
    const init: RequestInit & { duplex?: "half" } = {
      method: request.method,
      headers: fwd,
      body: request.method === "GET" || request.method === "HEAD" ? undefined : request.body,
      duplex: "half",
    };
    const upstream = await fetch(target, init);
    const headers = new Headers(upstream.headers);
    for (const [k, v] of Object.entries(CORS)) headers.set(k, v);
    return new Response(upstream.body, { status: upstream.status, headers });
  },
};

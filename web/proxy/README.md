# ytdown-proxy

A transparent CORS-adding passthrough Cloudflare Worker required by `@4thel00z/ytdown`. Browsers cannot call YouTube's APIs directly due to CORS restrictions — this worker sits in between, forwarding requests and injecting permissive CORS headers so the browser SDK can read the responses.

## Deploy

```bash
npx wrangler deploy
# or
bun run deploy
```

After deploying, pass the worker URL as the `proxy` option to the SDK:

```ts
import { Ytdown } from "@4thel00z/ytdown";

const ytdown = await Ytdown.create({
  proxy: "https://ytdown-proxy.<you>.workers.dev",
});
```

## Contract

- `GET/POST {workerUrl}?url=<encoded-target>` — forwards the request (method, headers, body) to the decoded target URL and returns the upstream response with `Access-Control-Allow-Origin: *` and related CORS headers set.
- `OPTIONS {workerUrl}` — answers CORS preflight with `204 No Content` and full CORS headers.
- Missing `?url` param — returns `400 Bad Request`.

## WARNING: Open Passthrough — No Auth or Rate Limiting

This worker is a **fully open proxy**. Anyone who discovers its URL can use it to proxy arbitrary HTTP requests through your Cloudflare account. Before exposing it publicly you are responsible for adding access control. Options include:

- **Cloudflare Access** — put the worker behind Zero Trust authentication.
- **Target allowlist** — check that the decoded `url` param matches an allowed hostname before forwarding.
- **Rate limiting** — use Cloudflare rate limiting rules or a KV-backed counter to cap requests per IP.
- **ngrok** — expose only to trusted parties via an ngrok endpoint with basic auth.

Running this worker as-is on a public URL is suitable only for local development or controlled testing environments.

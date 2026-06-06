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

### Forbidden-header aliasing

YouTube's InnerTube API requires `Origin: https://www.youtube.com` and `Referer: https://www.youtube.com/` headers, or it answers `400`/`403`. But `Origin` and `Referer` are on the WHATWG "forbidden header names" list — browsers **silently drop** them when page JS tries to set them on a `fetch`. So the SDK sends them under safe aliases the browser will pass through:

| Real header | Alias the SDK sends |
| ----------- | ------------------- |
| `Origin`    | `x-ytdown-origin`   |
| `Referer`   | `x-ytdown-referer`  |

Before forwarding upstream, this worker restores each `x-ytdown-<name>` header to its real name and deletes the alias. A server-side Worker `fetch` is allowed to set `Origin`/`Referer` (the forbidden-header restriction is browser-only), so they reach YouTube intact. The mapping lives in `web/proxy/src/worker.ts` (`ALIAS_TO_REAL`) and must stay in sync with `web/src/proxy.ts` (`FORBIDDEN_HEADER_ALIASES`).

## WARNING: Open Passthrough — No Auth or Rate Limiting

This worker is a **fully open proxy**. Anyone who discovers its URL can use it to proxy arbitrary HTTP requests through your Cloudflare account. Before exposing it publicly you are responsible for adding access control. Options include:

- **Cloudflare Access** — put the worker behind Zero Trust authentication.
- **Target allowlist** — check that the decoded `url` param matches an allowed hostname before forwarding.
- **Rate limiting** — use Cloudflare rate limiting rules or a KV-backed counter to cap requests per IP.
- **ngrok** — expose only to trusted parties via an ngrok endpoint with basic auth.

Running this worker as-is on a public URL is suitable only for local development or controlled testing environments.

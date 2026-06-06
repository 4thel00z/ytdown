import { describe, it, expect, vi } from "vitest";
import { makeProxyFetch, type WasmRequest } from "../src/proxy";

describe("makeProxyFetch", () => {
  it("rewrites the target URL through the proxy and forwards method/headers/body", async () => {
    const fetchMock = vi.fn(async () =>
      new Response(new Uint8Array([1, 2, 3]), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    );
    const cb = makeProxyFetch("https://proxy.example/p", fetchMock as any);
    const req: WasmRequest = {
      stage: "innertube",
      method: "POST",
      url: "https://www.youtube.com/youtubei/v1/player",
      headers: [["X-Goog-Api-Key", "k"]],
      body: new Uint8Array([9]),
    };
    const resp = await cb(req);

    const call = fetchMock.mock.calls[0] as unknown as [RequestInfo | URL, RequestInit];
    const [calledUrl, init] = call;
    expect(String(calledUrl)).toContain("https://proxy.example/p");
    expect(String(calledUrl)).toContain(encodeURIComponent(req.url));
    expect(init.method).toBe("POST");
    expect(resp.status).toBe(200);
    expect(Array.from(resp.body!)).toEqual([1, 2, 3]);
    expect(resp.headers.find(([k]) => k === "content-type")?.[1]).toBe("application/json");
  });

  it("aliases forbidden headers (Origin/Referer) so the browser won't drop them", async () => {
    const fetchMock = vi.fn(async () => new Response(new Uint8Array(), { status: 200 }));
    const cb = makeProxyFetch("https://proxy.example/p", fetchMock as any);
    await cb({
      stage: "innertube",
      method: "POST",
      url: "https://www.youtube.com/youtubei/v1/player",
      headers: [
        ["Origin", "https://www.youtube.com"],
        ["Referer", "https://www.youtube.com/"],
        ["X-Goog-Api-Key", "k"],
      ],
      body: new Uint8Array([1]),
    });
    const init = (fetchMock.mock.calls[0] as unknown as [unknown, RequestInit])[1];
    const sent = new Headers(init.headers);
    // Forbidden headers are aliased; the raw forbidden names are NOT set by us.
    expect(sent.get("x-ytdown-origin")).toBe("https://www.youtube.com");
    expect(sent.get("x-ytdown-referer")).toBe("https://www.youtube.com/");
    // Non-forbidden headers pass through unchanged.
    expect(sent.get("x-goog-api-key")).toBe("k");
  });
});
